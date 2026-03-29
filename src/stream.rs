//! Streaming decoders for chunk-at-a-time audio decoding.
//!
//! Provides [`StreamDecoder`] implementations for WAV, FLAC, and AIFF that
//! accept incremental byte input and emit [`StreamEvent`]s as data becomes
//! available. Also includes `std::io::Read` adapters and file helpers.
//!
//! This module requires the `streaming` feature (which implies `std`).

use alloc::vec::Vec;

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Event emitted by a streaming decoder.
#[non_exhaustive]
pub enum StreamEvent {
    /// Format info parsed from header (emitted once, first).
    Header(FormatInfo),
    /// A chunk of decoded interleaved f32 samples.
    Samples(Vec<f32>),
    /// End of stream.
    End,
}

/// Chunk-at-a-time audio decoder.
pub trait StreamDecoder {
    /// Feed raw bytes. Returns zero or more events.
    fn feed(&mut self, data: &[u8]) -> Result<Vec<StreamEvent>>;
    /// Signal end of input. Flush remaining samples.
    fn flush(&mut self) -> Result<Vec<StreamEvent>>;
    /// Get format info if header has been parsed.
    fn format_info(&self) -> Option<&FormatInfo>;
}

// ---------------------------------------------------------------------------
// WAV streaming decoder
// ---------------------------------------------------------------------------

/// WAV header fields extracted during parsing.
struct WavHeader {
    format_code: u16,
    bits_per_sample: u16,
    channels: u16,
    #[allow(dead_code)]
    sample_rate: u32,
    data_size: usize,
}

/// State machine for the WAV streaming decoder.
enum WavState {
    ParsingHeader,
    DecodingSamples,
    Done,
}

/// Streaming WAV decoder that processes data chunk-at-a-time.
pub struct WavStreamDecoder {
    state: WavState,
    buffer: Vec<u8>,
    info: Option<FormatInfo>,
    header: Option<WavHeader>,
    chunk_frames: usize,
    /// Number of raw PCM bytes consumed so far from the data chunk.
    data_consumed: usize,
}

impl Default for WavStreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl WavStreamDecoder {
    /// Create a new WAV streaming decoder with the default chunk size (4096 frames).
    #[must_use]
    pub fn new() -> Self {
        Self::with_chunk_size(4096)
    }

    /// Create a new WAV streaming decoder with the given chunk size in frames.
    #[must_use]
    pub fn with_chunk_size(frames: usize) -> Self {
        Self {
            state: WavState::ParsingHeader,
            buffer: Vec::new(),
            info: None,
            header: None,
            chunk_frames: if frames == 0 { 4096 } else { frames },
            data_consumed: 0,
        }
    }

    /// Try to parse the WAV header from the accumulated buffer.
    fn try_parse_header(&mut self, events: &mut Vec<StreamEvent>) -> Result<()> {
        let data = &self.buffer;

        if data.len() < 12 {
            return Ok(());
        }
        if &data[0..4] != b"RIFF" {
            return Err(ShravanError::InvalidHeader("missing RIFF magic".into()));
        }
        if &data[8..12] != b"WAVE" {
            return Err(ShravanError::InvalidHeader(
                "missing WAVE identifier".into(),
            ));
        }

        let mut pos = 12;
        let mut fmt_found = false;
        let mut data_found = false;
        let mut fmt_format_code: u16 = 0;
        let mut fmt_channels: u16 = 0;
        let mut fmt_sample_rate: u32 = 0;
        let mut fmt_bits_per_sample: u16 = 0;
        let mut data_start: usize = 0;
        let mut data_size: usize = 0;

        while pos + 8 <= data.len() {
            let chunk_id = &data[pos..pos + 4];
            if pos + 8 > data.len() {
                return Ok(()); // need more data
            }
            let chunk_size =
                u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;

            if chunk_id == b"fmt " {
                if chunk_size < 16 || pos + 8 + 16 > data.len() {
                    return Ok(()); // need more data
                }
                fmt_format_code = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
                fmt_channels = u16::from_le_bytes([data[pos + 10], data[pos + 11]]);
                fmt_sample_rate = u32::from_le_bytes([
                    data[pos + 12],
                    data[pos + 13],
                    data[pos + 14],
                    data[pos + 15],
                ]);
                fmt_bits_per_sample = u16::from_le_bytes([data[pos + 22], data[pos + 23]]);
                fmt_found = true;
            } else if chunk_id == b"data" {
                data_start = pos + 8;
                data_size = chunk_size;
                data_found = true;
            }

            let padded_size = chunk_size.saturating_add(chunk_size & 1);
            pos = pos.saturating_add(8).saturating_add(padded_size);

            if fmt_found && data_found {
                break;
            }
        }

        if !fmt_found || !data_found {
            return Ok(()); // need more data
        }
        if fmt_channels == 0 {
            return Err(ShravanError::InvalidChannels(0));
        }
        if fmt_sample_rate == 0 {
            return Err(ShravanError::InvalidSampleRate(0));
        }

        // We don't know the full duration yet in streaming mode; set to 0 and
        // update on flush.
        let total_frames = {
            let bytes_per_sample = usize::from(fmt_bits_per_sample / 8);
            let frame_bytes = usize::from(fmt_channels) * bytes_per_sample;
            if frame_bytes > 0 {
                data_size / frame_bytes
            } else {
                0
            }
        };
        let duration_secs = total_frames as f64 / f64::from(fmt_sample_rate);

        let info = FormatInfo {
            format: AudioFormat::Wav,
            sample_rate: fmt_sample_rate,
            channels: fmt_channels,
            bit_depth: fmt_bits_per_sample,
            duration_secs,
            total_samples: total_frames as u64,
        };

        self.header = Some(WavHeader {
            format_code: fmt_format_code,
            bits_per_sample: fmt_bits_per_sample,
            channels: fmt_channels,
            sample_rate: fmt_sample_rate,
            data_size,
        });

        events.push(StreamEvent::Header(info.clone()));
        self.info = Some(info);

        // Remove header bytes, keep only data-chunk PCM bytes.
        let remaining = self.buffer.split_off(data_start);
        self.buffer = remaining;
        self.data_consumed = 0;
        self.state = WavState::DecodingSamples;

        Ok(())
    }

    /// Decode as many complete frames as possible from the buffer.
    fn decode_samples(&mut self, events: &mut Vec<StreamEvent>, emit_partial: bool) -> Result<()> {
        let hdr = match &self.header {
            Some(h) => h,
            None => return Ok(()),
        };

        let bytes_per_sample = usize::from(hdr.bits_per_sample / 8);
        let frame_byte_size = usize::from(hdr.channels) * bytes_per_sample;
        if frame_byte_size == 0 {
            return Ok(());
        }

        // Limit buffer to data_size
        let remaining_data_bytes = hdr.data_size.saturating_sub(self.data_consumed);
        let usable = self.buffer.len().min(remaining_data_bytes);

        let chunk_byte_size = self.chunk_frames * frame_byte_size;
        let mut offset = 0;

        while offset + chunk_byte_size <= usable {
            let chunk = &self.buffer[offset..offset + chunk_byte_size];
            let samples = wav_pcm_to_f32(chunk, hdr.format_code, hdr.bits_per_sample)?;
            events.push(StreamEvent::Samples(samples));
            offset += chunk_byte_size;
        }

        // Partial chunk on flush
        if emit_partial {
            let leftover = usable - offset;
            let complete_frames = leftover / frame_byte_size;
            if complete_frames > 0 {
                let len = complete_frames * frame_byte_size;
                let chunk = &self.buffer[offset..offset + len];
                let samples = wav_pcm_to_f32(chunk, hdr.format_code, hdr.bits_per_sample)?;
                events.push(StreamEvent::Samples(samples));
                offset += len;
            }
        }

        if offset > 0 {
            self.data_consumed += offset;
            self.buffer.drain(..offset);
        }

        // Check if we've consumed all data
        if self.data_consumed >= hdr.data_size {
            self.state = WavState::Done;
        }

        Ok(())
    }
}

impl StreamDecoder for WavStreamDecoder {
    fn feed(&mut self, data: &[u8]) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        match self.state {
            WavState::Done => return Ok(events),
            WavState::ParsingHeader => {
                self.buffer.extend_from_slice(data);
                self.try_parse_header(&mut events)?;
                if matches!(self.state, WavState::DecodingSamples) {
                    self.decode_samples(&mut events, false)?;
                }
            }
            WavState::DecodingSamples => {
                self.buffer.extend_from_slice(data);
                self.decode_samples(&mut events, false)?;
            }
        }

        Ok(events)
    }

    fn flush(&mut self) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        match self.state {
            WavState::Done => {}
            WavState::ParsingHeader => {
                // Try one final parse
                self.try_parse_header(&mut events)?;
                if matches!(self.state, WavState::DecodingSamples) {
                    self.decode_samples(&mut events, true)?;
                }
            }
            WavState::DecodingSamples => {
                self.decode_samples(&mut events, true)?;
            }
        }

        self.state = WavState::Done;
        events.push(StreamEvent::End);
        Ok(events)
    }

    fn format_info(&self) -> Option<&FormatInfo> {
        self.info.as_ref()
    }
}

/// Convert raw WAV PCM bytes to f32 samples.
fn wav_pcm_to_f32(data: &[u8], format_code: u16, bits_per_sample: u16) -> Result<Vec<f32>> {
    const WAV_FORMAT_PCM: u16 = 1;
    const WAV_FORMAT_IEEE_FLOAT: u16 = 3;

    match (format_code, bits_per_sample) {
        (WAV_FORMAT_PCM, 8) => Ok(data.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect()),
        (WAV_FORMAT_PCM, 16) => Ok(data
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_le_bytes([c[0], c[1]]);
                s as f32 / 32768.0
            })
            .collect()),
        (WAV_FORMAT_PCM, 24) => Ok(data
            .chunks_exact(3)
            .map(|c| {
                let raw = i32::from(c[0]) | (i32::from(c[1]) << 8) | (i32::from(c[2]) << 16);
                let extended = (raw << 8) >> 8;
                extended as f32 / 8_388_608.0
            })
            .collect()),
        (WAV_FORMAT_PCM, 32) => Ok(data
            .chunks_exact(4)
            .map(|c| {
                let s = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                s as f32 / 2_147_483_648.0
            })
            .collect()),
        (WAV_FORMAT_IEEE_FLOAT, 32) => Ok(data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()),
        _ => Err(ShravanError::DecodeError(alloc::format!(
            "unsupported WAV format: code={format_code}, bits={bits_per_sample}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// FLAC streaming decoder
// ---------------------------------------------------------------------------

/// State machine for the FLAC streaming decoder.
enum FlacState {
    ParsingMetadata,
    DecodingFrames,
    Done,
}

/// Streaming FLAC decoder that processes data chunk-at-a-time.
///
/// Strategy: accumulate bytes and decode all available complete frames.
/// Track the byte position of the last successfully decoded frame end to
/// avoid re-decoding.
pub struct FlacStreamDecoder {
    state: FlacState,
    buffer: Vec<u8>,
    info: Option<FormatInfo>,
    /// Total samples already emitted (to avoid duplicates).
    samples_emitted: u64,
}

impl Default for FlacStreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FlacStreamDecoder {
    /// Create a new FLAC streaming decoder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: FlacState::ParsingMetadata,
            buffer: Vec::new(),
            info: None,
            samples_emitted: 0,
        }
    }

    /// Try decoding accumulated data. Returns newly decoded samples.
    fn try_decode(&mut self, events: &mut Vec<StreamEvent>) -> Result<()> {
        // Attempt a full decode of the accumulated buffer. This will decode
        // all complete frames and stop at incomplete ones (returning EndOfStream
        // or similar). We use the one-shot decode_range, skipping already-emitted
        // samples.
        match crate::flac::decode_range(&self.buffer, self.samples_emitted, None) {
            Ok((info, samples)) => {
                if !samples.is_empty() {
                    let channels = u64::from(info.channels);
                    let new_frames = samples.len() as u64 / channels.max(1);
                    self.samples_emitted += new_frames;
                    events.push(StreamEvent::Samples(samples));
                }
            }
            Err(ShravanError::EndOfStream) => {
                // Not enough data for more frames; that's fine in streaming mode.
            }
            Err(ShravanError::DecodeError(_)) => {
                // Partial frame at end of buffer; wait for more data.
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }
}

impl StreamDecoder for FlacStreamDecoder {
    fn feed(&mut self, data: &[u8]) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        match self.state {
            FlacState::Done => return Ok(events),
            FlacState::ParsingMetadata => {
                self.buffer.extend_from_slice(data);

                // Try to parse metadata from accumulated buffer
                match crate::flac::decode(&self.buffer) {
                    Ok((info, samples)) => {
                        events.push(StreamEvent::Header(info.clone()));
                        if !samples.is_empty() {
                            let channels = u64::from(info.channels);
                            let new_frames = samples.len() as u64 / channels.max(1);
                            self.samples_emitted = new_frames;
                            events.push(StreamEvent::Samples(samples));
                        }
                        self.info = Some(info);
                        self.state = FlacState::DecodingFrames;
                    }
                    Err(ShravanError::EndOfStream) => {
                        // Need more data
                    }
                    Err(ShravanError::DecodeError(_)) => {
                        // Might be partial; wait for more data. But first check
                        // if we have enough for metadata at least.
                        // Try parse_metadata via a full decode to see if header is OK
                    }
                    Err(e) => return Err(e),
                }
            }
            FlacState::DecodingFrames => {
                self.buffer.extend_from_slice(data);
                self.try_decode(&mut events)?;
            }
        }

        Ok(events)
    }

    fn flush(&mut self) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        match self.state {
            FlacState::Done => {}
            FlacState::ParsingMetadata => {
                // Final attempt
                match crate::flac::decode(&self.buffer) {
                    Ok((info, samples)) => {
                        events.push(StreamEvent::Header(info.clone()));
                        if !samples.is_empty() {
                            events.push(StreamEvent::Samples(samples));
                        }
                        self.info = Some(info);
                    }
                    Err(ShravanError::EndOfStream) => {}
                    Err(ShravanError::DecodeError(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            FlacState::DecodingFrames => {
                self.try_decode(&mut events)?;
            }
        }

        self.state = FlacState::Done;
        events.push(StreamEvent::End);
        Ok(events)
    }

    fn format_info(&self) -> Option<&FormatInfo> {
        self.info.as_ref()
    }
}

// ---------------------------------------------------------------------------
// AIFF streaming decoder
// ---------------------------------------------------------------------------

/// State machine for the AIFF streaming decoder.
enum AiffState {
    ParsingHeader,
    DecodingSamples,
    Done,
}

/// AIFF header fields extracted during parsing.
struct AiffHeader {
    channels: u16,
    #[allow(dead_code)]
    sample_rate: u32,
    bits_per_sample: u16,
    data_size: usize,
    big_endian: bool,
}

/// Streaming AIFF decoder that processes data chunk-at-a-time.
pub struct AiffStreamDecoder {
    state: AiffState,
    buffer: Vec<u8>,
    info: Option<FormatInfo>,
    header: Option<AiffHeader>,
    chunk_frames: usize,
    data_consumed: usize,
}

impl Default for AiffStreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AiffStreamDecoder {
    /// Create a new AIFF streaming decoder with the default chunk size (4096 frames).
    #[must_use]
    pub fn new() -> Self {
        Self::with_chunk_size(4096)
    }

    /// Create a new AIFF streaming decoder with the given chunk size in frames.
    #[must_use]
    pub fn with_chunk_size(frames: usize) -> Self {
        Self {
            state: AiffState::ParsingHeader,
            buffer: Vec::new(),
            info: None,
            header: None,
            chunk_frames: if frames == 0 { 4096 } else { frames },
            data_consumed: 0,
        }
    }

    /// Try to parse the AIFF header from the accumulated buffer.
    fn try_parse_header(&mut self, events: &mut Vec<StreamEvent>) -> Result<()> {
        let data = &self.buffer;

        if data.len() < 12 {
            return Ok(());
        }
        if &data[0..4] != b"FORM" {
            return Err(ShravanError::InvalidHeader("missing FORM magic".into()));
        }

        let form_type = &data[8..12];
        let is_aifc = form_type == b"AIFC";
        if form_type != b"AIFF" && !is_aifc {
            return Err(ShravanError::InvalidHeader(
                "missing AIFF/AIFC identifier".into(),
            ));
        }

        let mut pos: usize = 12;
        let mut channels: u16 = 0;
        let mut sample_size: u16 = 0;
        let mut sample_rate_f64: f64 = 0.0;
        let mut comm_found = false;
        let mut big_endian = true;

        let mut ssnd_data_start: usize = 0;
        let mut ssnd_data_len: usize = 0;
        let mut ssnd_found = false;

        while pos + 8 <= data.len() {
            let chunk_id = &data[pos..pos + 4];
            if pos + 8 > data.len() {
                return Ok(());
            }
            let chunk_size =
                u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;

            if chunk_id == b"COMM" {
                let min_comm = if is_aifc { 22 } else { 18 };
                if chunk_size < min_comm || pos + 8 + min_comm > data.len() {
                    return Ok(()); // need more data
                }
                let base = pos + 8;
                channels = i16::from_be_bytes([data[base], data[base + 1]]) as u16;
                // num_sample_frames at base+2..base+6
                sample_size = i16::from_be_bytes([data[base + 6], data[base + 7]]) as u16;

                if base + 18 > data.len() {
                    return Ok(());
                }
                sample_rate_f64 = extended_to_f64(&data[base + 8..base + 18]);

                if is_aifc {
                    if base + 22 > data.len() {
                        return Ok(());
                    }
                    let comp = &data[base + 18..base + 22];
                    if comp == b"NONE" {
                        big_endian = true;
                    } else if comp == b"sowt" {
                        big_endian = false;
                    } else {
                        return Err(ShravanError::DecodeError(alloc::format!(
                            "unsupported AIFF-C compression: {:?}",
                            core::str::from_utf8(comp).unwrap_or("????")
                        )));
                    }
                }
                comm_found = true;
            } else if chunk_id == b"SSND" {
                if chunk_size < 8 || pos + 16 > data.len() {
                    return Ok(());
                }
                let ssnd_offset = u32::from_be_bytes([
                    data[pos + 8],
                    data[pos + 9],
                    data[pos + 10],
                    data[pos + 11],
                ]) as usize;
                let header_bytes = 8; // offset(4) + blockSize(4)
                let pcm_start = pos + 8 + header_bytes + ssnd_offset;
                let pcm_len = chunk_size.saturating_sub(header_bytes + ssnd_offset);
                ssnd_data_start = pcm_start;
                ssnd_data_len = pcm_len;
                ssnd_found = true;
            }

            let padded_size = chunk_size.saturating_add(chunk_size & 1);
            pos = pos.saturating_add(8).saturating_add(padded_size);

            if comm_found && ssnd_found {
                break;
            }
        }

        if !comm_found || !ssnd_found {
            return Ok(());
        }
        if channels == 0 {
            return Err(ShravanError::InvalidChannels(0));
        }
        let sample_rate = sample_rate_f64 as u32;
        if sample_rate == 0 {
            return Err(ShravanError::InvalidSampleRate(0));
        }

        let bytes_per_sample = usize::from(sample_size / 8);
        let frame_bytes = usize::from(channels) * bytes_per_sample;
        let total_frames = if frame_bytes > 0 {
            ssnd_data_len / frame_bytes
        } else {
            0
        };
        let duration_secs = total_frames as f64 / f64::from(sample_rate);

        let info = FormatInfo {
            format: AudioFormat::Aiff,
            sample_rate,
            channels,
            bit_depth: sample_size,
            duration_secs,
            total_samples: total_frames as u64,
        };

        self.header = Some(AiffHeader {
            channels,
            sample_rate,
            bits_per_sample: sample_size,
            data_size: ssnd_data_len,
            big_endian,
        });

        events.push(StreamEvent::Header(info.clone()));
        self.info = Some(info);

        // Keep only the PCM data from ssnd_data_start onward
        let remaining = self.buffer.split_off(ssnd_data_start);
        self.buffer = remaining;
        self.data_consumed = 0;
        self.state = AiffState::DecodingSamples;

        Ok(())
    }

    /// Decode as many complete frames as possible from the buffer.
    fn decode_samples(&mut self, events: &mut Vec<StreamEvent>, emit_partial: bool) -> Result<()> {
        let hdr = match &self.header {
            Some(h) => h,
            None => return Ok(()),
        };

        let bytes_per_sample = usize::from(hdr.bits_per_sample / 8);
        let frame_byte_size = usize::from(hdr.channels) * bytes_per_sample;
        if frame_byte_size == 0 {
            return Ok(());
        }

        let remaining_data_bytes = hdr.data_size.saturating_sub(self.data_consumed);
        let usable = self.buffer.len().min(remaining_data_bytes);

        let chunk_byte_size = self.chunk_frames * frame_byte_size;
        let mut offset = 0;

        while offset + chunk_byte_size <= usable {
            let chunk = &self.buffer[offset..offset + chunk_byte_size];
            let samples = aiff_pcm_to_f32(chunk, hdr.bits_per_sample, hdr.big_endian)?;
            events.push(StreamEvent::Samples(samples));
            offset += chunk_byte_size;
        }

        if emit_partial {
            let leftover = usable - offset;
            let complete_frames = leftover / frame_byte_size;
            if complete_frames > 0 {
                let len = complete_frames * frame_byte_size;
                let chunk = &self.buffer[offset..offset + len];
                let samples = aiff_pcm_to_f32(chunk, hdr.bits_per_sample, hdr.big_endian)?;
                events.push(StreamEvent::Samples(samples));
                offset += len;
            }
        }

        if offset > 0 {
            self.data_consumed += offset;
            self.buffer.drain(..offset);
        }

        if self.data_consumed >= hdr.data_size {
            self.state = AiffState::Done;
        }

        Ok(())
    }
}

impl StreamDecoder for AiffStreamDecoder {
    fn feed(&mut self, data: &[u8]) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        match self.state {
            AiffState::Done => return Ok(events),
            AiffState::ParsingHeader => {
                self.buffer.extend_from_slice(data);
                self.try_parse_header(&mut events)?;
                if matches!(self.state, AiffState::DecodingSamples) {
                    self.decode_samples(&mut events, false)?;
                }
            }
            AiffState::DecodingSamples => {
                self.buffer.extend_from_slice(data);
                self.decode_samples(&mut events, false)?;
            }
        }

        Ok(events)
    }

    fn flush(&mut self) -> Result<Vec<StreamEvent>> {
        let mut events = Vec::new();

        match self.state {
            AiffState::Done => {}
            AiffState::ParsingHeader => {
                self.try_parse_header(&mut events)?;
                if matches!(self.state, AiffState::DecodingSamples) {
                    self.decode_samples(&mut events, true)?;
                }
            }
            AiffState::DecodingSamples => {
                self.decode_samples(&mut events, true)?;
            }
        }

        self.state = AiffState::Done;
        events.push(StreamEvent::End);
        Ok(events)
    }

    fn format_info(&self) -> Option<&FormatInfo> {
        self.info.as_ref()
    }
}

/// Convert raw AIFF PCM bytes to f32 samples.
fn aiff_pcm_to_f32(data: &[u8], bits_per_sample: u16, big_endian: bool) -> Result<Vec<f32>> {
    match (bits_per_sample, big_endian) {
        (8, _) => Ok(data.iter().map(|&b| b as i8 as f32 / 128.0).collect()),
        (16, true) => Ok(data
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_be_bytes([c[0], c[1]]);
                s as f32 / 32768.0
            })
            .collect()),
        (16, false) => Ok(data
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_le_bytes([c[0], c[1]]);
                s as f32 / 32768.0
            })
            .collect()),
        (24, true) => Ok(data
            .chunks_exact(3)
            .map(|c| {
                let raw = (i32::from(c[0]) << 16) | (i32::from(c[1]) << 8) | i32::from(c[2]);
                let extended = (raw << 8) >> 8;
                extended as f32 / 8_388_608.0
            })
            .collect()),
        (24, false) => Ok(data
            .chunks_exact(3)
            .map(|c| {
                let raw = i32::from(c[0]) | (i32::from(c[1]) << 8) | (i32::from(c[2]) << 16);
                let extended = (raw << 8) >> 8;
                extended as f32 / 8_388_608.0
            })
            .collect()),
        (32, true) => Ok(data
            .chunks_exact(4)
            .map(|c| {
                let s = i32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                s as f32 / 2_147_483_648.0
            })
            .collect()),
        (32, false) => Ok(data
            .chunks_exact(4)
            .map(|c| {
                let s = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                s as f32 / 2_147_483_648.0
            })
            .collect()),
        _ => Err(ShravanError::DecodeError(alloc::format!(
            "unsupported AIFF bit depth: {bits_per_sample}"
        ))),
    }
}

/// Convert an 80-bit IEEE 754 extended-precision float to `f64`.
///
/// Duplicate of `aiff::extended_to_f64` to avoid cross-module dependency on
/// a private function.
#[inline]
#[must_use]
fn extended_to_f64(bytes: &[u8]) -> f64 {
    let sign = if bytes[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exponent = (((bytes[0] as u16 & 0x7F) << 8) | bytes[1] as u16) as i32;
    let mantissa = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    if exponent == 0 && mantissa == 0 {
        return 0.0;
    }
    sign * (mantissa as f64 / (1u64 << 63) as f64) * libm::pow(2.0, (exponent - 16383) as f64)
}

// ---------------------------------------------------------------------------
// std-only helpers
// ---------------------------------------------------------------------------

use std::io::Read;
use std::path::Path;

/// Read an entire stream into memory and decode using auto-detection.
///
/// # Errors
///
/// Returns errors if reading fails or the audio format is not supported.
pub fn decode_reader<R: Read>(reader: &mut R) -> Result<(FormatInfo, Vec<f32>)> {
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| ShravanError::DecodeError(e.to_string()))?;
    crate::codec::open(&buf)
}

/// Read a file from disk and auto-detect its format.
///
/// # Errors
///
/// Returns errors if the file cannot be read or the format is not supported.
pub fn decode_file(path: &Path) -> Result<(FormatInfo, Vec<f32>)> {
    let data = std::fs::read(path).map_err(|e| ShravanError::DecodeError(e.to_string()))?;
    crate::codec::open(&data)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ---- WAV streaming tests ----

    #[cfg(feature = "wav")]
    fn make_wav_data() -> (Vec<u8>, Vec<f32>) {
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 / 4800.0) * 2.0 - 1.0).collect();
        let encoded = crate::wav::encode(&samples, 44100, 1, crate::pcm::PcmFormat::I16).unwrap();
        (encoded, samples)
    }

    #[cfg(feature = "wav")]
    #[test]
    fn wav_streaming_small_chunks() {
        let (wav_data, original) = make_wav_data();

        let mut decoder = WavStreamDecoder::new();
        let mut all_events = Vec::new();

        // Feed in 100-byte chunks
        for chunk in wav_data.chunks(100) {
            let events = decoder.feed(chunk).unwrap();
            all_events.extend(events);
        }
        let flush_events = decoder.flush().unwrap();
        all_events.extend(flush_events);

        // Should have Header, one or more Samples, and End
        let mut got_header = false;
        let mut got_end = false;
        let mut collected_samples = Vec::new();

        for event in &all_events {
            match event {
                StreamEvent::Header(info) => {
                    assert_eq!(info.format, AudioFormat::Wav);
                    assert_eq!(info.sample_rate, 44100);
                    assert_eq!(info.channels, 1);
                    got_header = true;
                }
                StreamEvent::Samples(s) => {
                    collected_samples.extend_from_slice(s);
                }
                StreamEvent::End => {
                    got_end = true;
                }
            }
        }

        assert!(got_header);
        assert!(got_end);

        // Compare with one-shot decode
        let (_, oneshot_samples) = crate::wav::decode(&wav_data).unwrap();
        assert_eq!(collected_samples.len(), oneshot_samples.len());
        for (a, b) in collected_samples.iter().zip(oneshot_samples.iter()) {
            assert!((a - b).abs() < f32::EPSILON, "sample mismatch: {a} vs {b}");
        }

        // Also verify against original within quantization tolerance
        assert_eq!(collected_samples.len(), original.len());
        for (a, b) in collected_samples.iter().zip(original.iter()) {
            assert!((a - b).abs() < 0.001, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "wav")]
    #[test]
    fn wav_streaming_single_feed() {
        let (wav_data, _) = make_wav_data();

        let mut decoder = WavStreamDecoder::new();
        let mut all_events = decoder.feed(&wav_data).unwrap();
        all_events.extend(decoder.flush().unwrap());

        let mut collected_samples = Vec::new();
        for event in &all_events {
            if let StreamEvent::Samples(s) = event {
                collected_samples.extend_from_slice(s);
            }
        }

        let (_, oneshot_samples) = crate::wav::decode(&wav_data).unwrap();
        assert_eq!(collected_samples.len(), oneshot_samples.len());
        for (a, b) in collected_samples.iter().zip(oneshot_samples.iter()) {
            assert!((a - b).abs() < f32::EPSILON, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "wav")]
    #[test]
    fn wav_streaming_one_byte_chunks() {
        let (wav_data, _) = make_wav_data();

        let mut decoder = WavStreamDecoder::with_chunk_size(1024);
        let mut all_events = Vec::new();

        for &byte in &wav_data {
            let events = decoder.feed(&[byte]).unwrap();
            all_events.extend(events);
        }
        all_events.extend(decoder.flush().unwrap());

        let mut collected_samples = Vec::new();
        for event in &all_events {
            if let StreamEvent::Samples(s) = event {
                collected_samples.extend_from_slice(s);
            }
        }

        let (_, oneshot_samples) = crate::wav::decode(&wav_data).unwrap();
        assert_eq!(collected_samples.len(), oneshot_samples.len());
    }

    #[cfg(feature = "wav")]
    #[test]
    fn wav_streaming_1000_byte_chunks() {
        let (wav_data, _) = make_wav_data();

        let mut decoder = WavStreamDecoder::new();
        let mut all_events = Vec::new();

        for chunk in wav_data.chunks(1000) {
            let events = decoder.feed(chunk).unwrap();
            all_events.extend(events);
        }
        all_events.extend(decoder.flush().unwrap());

        let mut collected_samples = Vec::new();
        for event in &all_events {
            if let StreamEvent::Samples(s) = event {
                collected_samples.extend_from_slice(s);
            }
        }

        let (_, oneshot_samples) = crate::wav::decode(&wav_data).unwrap();
        assert_eq!(collected_samples.len(), oneshot_samples.len());
    }

    // ---- FLAC streaming tests ----

    #[cfg(feature = "flac")]
    fn make_flac_data() -> (Vec<u8>, Vec<f32>) {
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 / 4800.0) * 2.0 - 1.0).collect();
        let encoded = crate::flac::encode(&samples, 44100, 1, 16).unwrap();
        (encoded, samples)
    }

    #[cfg(feature = "flac")]
    #[test]
    fn flac_streaming_roundtrip() {
        let (flac_data, _original) = make_flac_data();

        let mut decoder = FlacStreamDecoder::new();
        let mut all_events = Vec::new();

        // Feed in chunks
        for chunk in flac_data.chunks(200) {
            let events = decoder.feed(chunk).unwrap();
            all_events.extend(events);
        }
        all_events.extend(decoder.flush().unwrap());

        let mut got_header = false;
        let mut got_end = false;
        let mut collected_samples = Vec::new();

        for event in &all_events {
            match event {
                StreamEvent::Header(info) => {
                    assert_eq!(info.format, AudioFormat::Flac);
                    assert_eq!(info.sample_rate, 44100);
                    got_header = true;
                }
                StreamEvent::Samples(s) => {
                    collected_samples.extend_from_slice(s);
                }
                StreamEvent::End => {
                    got_end = true;
                }
            }
        }

        assert!(got_header);
        assert!(got_end);

        // Compare with one-shot decode
        let (_, oneshot_samples) = crate::flac::decode(&flac_data).unwrap();
        assert_eq!(collected_samples.len(), oneshot_samples.len());
        for (a, b) in collected_samples.iter().zip(oneshot_samples.iter()) {
            assert!(
                (a - b).abs() < f32::EPSILON,
                "FLAC sample mismatch: {a} vs {b}"
            );
        }
    }

    // ---- AIFF streaming tests ----

    #[cfg(feature = "aiff")]
    fn make_aiff_data() -> (Vec<u8>, Vec<f32>) {
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 / 4800.0) * 2.0 - 1.0).collect();
        let encoded = crate::aiff::encode(&samples, 44100, 1, 16).unwrap();
        (encoded, samples)
    }

    #[cfg(feature = "aiff")]
    #[test]
    fn aiff_streaming_roundtrip() {
        let (aiff_data, original) = make_aiff_data();

        let mut decoder = AiffStreamDecoder::new();
        let mut all_events = Vec::new();

        for chunk in aiff_data.chunks(100) {
            let events = decoder.feed(chunk).unwrap();
            all_events.extend(events);
        }
        all_events.extend(decoder.flush().unwrap());

        let mut got_header = false;
        let mut got_end = false;
        let mut collected_samples = Vec::new();

        for event in &all_events {
            match event {
                StreamEvent::Header(info) => {
                    assert_eq!(info.format, AudioFormat::Aiff);
                    assert_eq!(info.sample_rate, 44100);
                    got_header = true;
                }
                StreamEvent::Samples(s) => {
                    collected_samples.extend_from_slice(s);
                }
                StreamEvent::End => {
                    got_end = true;
                }
            }
        }

        assert!(got_header);
        assert!(got_end);

        let (_, oneshot_samples) = crate::aiff::decode(&aiff_data).unwrap();
        assert_eq!(collected_samples.len(), oneshot_samples.len());
        for (a, b) in collected_samples.iter().zip(oneshot_samples.iter()) {
            assert!(
                (a - b).abs() < f32::EPSILON,
                "AIFF sample mismatch: {a} vs {b}"
            );
        }

        // Check against original within quantization tolerance
        assert_eq!(collected_samples.len(), original.len());
        for (a, b) in collected_samples.iter().zip(original.iter()) {
            assert!((a - b).abs() < 0.001, "sample mismatch: {a} vs {b}");
        }
    }

    // ---- Flush / empty tests ----

    #[test]
    fn flush_produces_end_event() {
        let mut decoder = WavStreamDecoder::new();
        let events = decoder.flush().unwrap();
        let has_end = events.iter().any(|e| matches!(e, StreamEvent::End));
        assert!(has_end);
    }

    #[test]
    fn empty_input_flush_produces_end() {
        let mut decoder = WavStreamDecoder::new();
        // Feed nothing
        let events = decoder.feed(&[]).unwrap();
        assert!(events.is_empty());
        let flush_events = decoder.flush().unwrap();
        let has_end = flush_events.iter().any(|e| matches!(e, StreamEvent::End));
        assert!(has_end);
    }

    // ---- decode_reader test ----

    #[cfg(feature = "wav")]
    #[test]
    fn decode_reader_with_cursor() {
        let (wav_data, _) = make_wav_data();
        let mut cursor = std::io::Cursor::new(&wav_data);
        let (info, samples) = decode_reader(&mut cursor).unwrap();
        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(samples.len(), 4800);
    }

    // ---- Various chunk sizes ----

    #[cfg(feature = "wav")]
    #[test]
    fn wav_streaming_various_chunk_sizes() {
        let (wav_data, _) = make_wav_data();
        let (_, oneshot_samples) = crate::wav::decode(&wav_data).unwrap();

        for &chunk_size in &[1usize, 13, 100, 1000, wav_data.len()] {
            let mut decoder = WavStreamDecoder::new();
            let mut all_events = Vec::new();

            for chunk in wav_data.chunks(chunk_size) {
                let events = decoder.feed(chunk).unwrap();
                all_events.extend(events);
            }
            all_events.extend(decoder.flush().unwrap());

            let mut collected = Vec::new();
            for event in &all_events {
                if let StreamEvent::Samples(s) = event {
                    collected.extend_from_slice(s);
                }
            }

            assert_eq!(
                collected.len(),
                oneshot_samples.len(),
                "mismatch at chunk_size={chunk_size}"
            );
        }
    }
}
