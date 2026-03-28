//! FLAC (Free Lossless Audio Codec) encoder and decoder.
//!
//! ## Decoder
//!
//! - STREAMINFO metadata block parsing
//! - Frame sync and header parsing with CRC-8 / CRC-16 verification
//! - Subframe types: Constant, Verbatim, Fixed (orders 0-4), LPC (orders 1-32)
//! - Rice entropy coding for residuals
//! - Channel decorrelation (independent, left-side, right-side, mid-side)
//! - SEEKTABLE parsing and sample-accurate seeking
//!
//! ## Encoder
//!
//! - Fixed prediction (orders 0-4) with automatic order selection
//! - Rice entropy coding with optimal parameter selection
//! - Channel decorrelation (independent and mid-side stereo)
//! - CRC-8 / CRC-16 checksums
//! - MD5 signature computation

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

/// FLAC metadata block types.
const STREAMINFO: u8 = 0;

/// FLAC frame channel assignment codes.
const CHANNEL_LEFT_SIDE: u8 = 8;
const CHANNEL_RIGHT_SIDE: u8 = 9;
const CHANNEL_MID_SIDE: u8 = 10;

// CRC-8 lookup table (polynomial 0x07, init 0).
const CRC8_TABLE: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut i = 0u16;
    while i < 256 {
        let mut crc = i as u8;
        let mut bit = 0;
        while bit < 8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
            bit += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

// CRC-16 lookup table (polynomial 0x8005, init 0).
const CRC16_TABLE: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0u16;
    while i < 256 {
        let mut crc = i << 8;
        let mut bit = 0;
        while bit < 8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x8005;
            } else {
                crc <<= 1;
            }
            bit += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
};

/// Compute FLAC CRC-8 over a byte slice.
#[inline]
fn crc8_flac(data: &[u8]) -> u8 {
    data.iter()
        .fold(0u8, |crc, &b| CRC8_TABLE[(crc ^ b) as usize])
}

/// Compute FLAC CRC-16 over a byte slice.
#[inline]
fn crc16_flac(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |crc, &b| {
        let idx = ((crc >> 8) ^ u16::from(b)) as usize;
        (crc << 8) ^ CRC16_TABLE[idx]
    })
}

/// Bitstream reader for parsing FLAC frames.
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0..8, bits remaining in current byte (MSB-first)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], byte_pos: usize) -> Self {
        Self {
            data,
            byte_pos,
            bit_pos: 0,
        }
    }

    /// Read up to 32 bits as a u32 (MSB-first).
    fn read_bits(&mut self, n: u8) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        if n > 32 {
            return Err(ShravanError::DecodeError(
                "cannot read more than 32 bits".into(),
            ));
        }
        let mut result: u32 = 0;
        let mut remaining = n;
        while remaining > 0 {
            if self.byte_pos >= self.data.len() {
                return Err(ShravanError::EndOfStream);
            }
            let available = 8 - self.bit_pos;
            let to_read = remaining.min(available);
            let shift = available - to_read;
            let mask = ((1u16 << to_read) - 1) as u8;
            let bits = (self.data[self.byte_pos] >> shift) & mask;
            result = (result << to_read) | u32::from(bits);
            remaining -= to_read;
            self.bit_pos += to_read;
            if self.bit_pos >= 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }
        Ok(result)
    }

    /// Read a single bit.
    fn read_bit(&mut self) -> Result<bool> {
        Ok(self.read_bits(1)? != 0)
    }

    /// Read a unary-coded value (count of 0 bits before the first 1 bit).
    fn read_unary(&mut self) -> Result<u32> {
        let mut count = 0u32;
        loop {
            if self.read_bit()? {
                return Ok(count);
            }
            count += 1;
            if count > 1_000_000 {
                return Err(ShravanError::DecodeError("unary value too large".into()));
            }
        }
    }

    /// Read a UTF-8-like coded number (FLAC uses this for frame/sample numbers).
    fn read_utf8_u64(&mut self) -> Result<u64> {
        let first = self.read_bits(8)? as u8;
        if first < 0x80 {
            return Ok(u64::from(first));
        }
        let leading_ones = first.leading_ones() as u8;
        if leading_ones > 7 {
            return Err(ShravanError::DecodeError(
                "invalid UTF-8 coded number".into(),
            ));
        }
        let mask = (1u8 << (7 - leading_ones)) - 1;
        let mut value = u64::from(first & mask);
        for _ in 1..leading_ones {
            let b = self.read_bits(8)? as u8;
            if b & 0xC0 != 0x80 {
                return Err(ShravanError::DecodeError(
                    "invalid UTF-8 continuation byte".into(),
                ));
            }
            value = (value << 6) | u64::from(b & 0x3F);
        }
        Ok(value)
    }

    /// Align to the next byte boundary.
    fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    /// Current absolute bit position.
    #[allow(dead_code)]
    fn position_bits(&self) -> usize {
        self.byte_pos * 8 + self.bit_pos as usize
    }
}

/// FLAC metadata block type: SEEKTABLE.
const SEEKTABLE: u8 = 3;

/// STREAMINFO metadata.
struct StreamInfo {
    #[allow(dead_code)]
    min_block_size: u16,
    #[allow(dead_code)]
    max_block_size: u16,
    sample_rate: u32,
    channels: u8,
    bits_per_sample: u8,
    #[allow(dead_code)]
    total_samples: u64,
}

/// A single seek point from a SEEKTABLE.
#[derive(Debug, Clone, Copy)]
struct SeekPoint {
    sample_number: u64,
    byte_offset: u64,
    #[allow(dead_code)]
    num_samples: u16,
}

/// Parsed FLAC metadata (STREAMINFO + optional SEEKTABLE).
struct FlacMetadata {
    stream_info: StreamInfo,
    seek_table: Vec<SeekPoint>,
    audio_start: usize, // byte offset where audio frames begin
}

/// Parse all FLAC metadata blocks, returning STREAMINFO, optional SEEKTABLE, and audio data offset.
fn parse_metadata(data: &[u8]) -> Result<FlacMetadata> {
    if data.len() < 4 || &data[0..4] != b"fLaC" {
        return Err(ShravanError::InvalidHeader("missing fLaC magic".into()));
    }

    let mut pos = 4;
    let mut stream_info: Option<StreamInfo> = None;
    let mut seek_table = Vec::new();

    loop {
        if pos + 4 > data.len() {
            return Err(ShravanError::EndOfStream);
        }
        let is_last = (data[pos] & 0x80) != 0;
        let block_type = data[pos] & 0x7F;
        let block_size = (u32::from(data[pos + 1]) << 16)
            | (u32::from(data[pos + 2]) << 8)
            | u32::from(data[pos + 3]);
        pos += 4;

        if block_type == STREAMINFO {
            stream_info = Some(parse_streaminfo(data, pos)?);
        } else if block_type == SEEKTABLE {
            // Each seek point is 18 bytes
            let num_points = block_size as usize / 18;
            let mut sp_pos = pos;
            for _ in 0..num_points {
                if sp_pos + 18 > data.len() {
                    break;
                }
                let sample_number = u64::from_be_bytes([
                    data[sp_pos],
                    data[sp_pos + 1],
                    data[sp_pos + 2],
                    data[sp_pos + 3],
                    data[sp_pos + 4],
                    data[sp_pos + 5],
                    data[sp_pos + 6],
                    data[sp_pos + 7],
                ]);
                // Placeholder seek points have sample_number == 0xFFFFFFFFFFFFFFFF
                if sample_number != 0xFFFFFFFFFFFFFFFF {
                    let byte_offset = u64::from_be_bytes([
                        data[sp_pos + 8],
                        data[sp_pos + 9],
                        data[sp_pos + 10],
                        data[sp_pos + 11],
                        data[sp_pos + 12],
                        data[sp_pos + 13],
                        data[sp_pos + 14],
                        data[sp_pos + 15],
                    ]);
                    let num_samples = u16::from_be_bytes([data[sp_pos + 16], data[sp_pos + 17]]);
                    seek_table.push(SeekPoint {
                        sample_number,
                        byte_offset,
                        num_samples,
                    });
                }
                sp_pos += 18;
            }
        }

        pos += block_size as usize;

        if is_last {
            break;
        }
    }

    let info = stream_info
        .ok_or_else(|| ShravanError::InvalidHeader("missing STREAMINFO block".into()))?;

    Ok(FlacMetadata {
        stream_info: info,
        seek_table,
        audio_start: pos,
    })
}

/// Parse the STREAMINFO metadata block.
fn parse_streaminfo(data: &[u8], offset: usize) -> Result<StreamInfo> {
    if offset + 34 > data.len() {
        return Err(ShravanError::InvalidHeader(
            "STREAMINFO block too short".into(),
        ));
    }
    let d = &data[offset..];
    let min_block_size = u16::from_be_bytes([d[0], d[1]]);
    let max_block_size = u16::from_be_bytes([d[2], d[3]]);
    // min/max frame size: d[4..10] (we skip these)
    // Sample rate: 20 bits, channels-1: 3 bits, bps-1: 5 bits, total samples: 36 bits
    // Bytes 10..14 contain: sample_rate(20) | channels-1(3) | bps-1(5) | total_samples_hi(4)
    let sr_hi = u32::from(d[10]) << 12 | u32::from(d[11]) << 4 | u32::from(d[12]) >> 4;
    let channels = ((d[12] >> 1) & 0x07) + 1;
    let bps = (u16::from(d[12] & 0x01) << 4 | u16::from(d[13] >> 4)) + 1;
    let total_samples_hi = u64::from(d[13] & 0x0F) << 32;
    let total_samples_lo = u64::from(u32::from_be_bytes([d[14], d[15], d[16], d[17]]));
    let total_samples = total_samples_hi | total_samples_lo;
    // MD5: d[18..34] (we skip)

    Ok(StreamInfo {
        min_block_size,
        max_block_size,
        sample_rate: sr_hi,
        channels,
        bits_per_sample: bps as u8,
        total_samples,
    })
}

/// Decode a FLAC file from a byte slice.
///
/// Returns format information and interleaved f32 samples normalized to \[-1.0, 1.0\].
///
/// Supports Constant, Verbatim, and Fixed subframe types with Rice entropy coding.
/// LPC subframes are not yet implemented and will return a decode error.
///
/// # Errors
///
/// Returns errors for invalid headers, unsupported subframe types, or truncated data.
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    decode_range(data, 0, None)
}

/// Decode a range of samples from a FLAC file.
///
/// Decodes samples from `start_sample` up to (but not including) `end_sample`.
/// If `end_sample` is `None`, decodes to the end of the file.
///
/// Uses the SEEKTABLE (if present) for fast seeking; otherwise scans sequentially.
///
/// # Errors
///
/// Returns errors for invalid headers, unsupported subframe types, CRC mismatches,
/// or truncated data.
pub fn decode_range(
    data: &[u8],
    start_sample: u64,
    end_sample: Option<u64>,
) -> Result<(FormatInfo, Vec<f32>)> {
    let meta = parse_metadata(data)?;
    let info = &meta.stream_info;

    if info.sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    let bps = info.bits_per_sample;
    let channels = info.channels;
    let scale = 1.0f64 / f64::from(1u32 << (bps - 1));

    // Determine starting byte offset
    let start_byte = if start_sample > 0 && !meta.seek_table.is_empty() {
        // Binary search the SEEKTABLE for the largest seek point <= start_sample
        let mut best_offset = meta.audio_start;
        for sp in &meta.seek_table {
            if sp.sample_number <= start_sample {
                best_offset = meta.audio_start + sp.byte_offset as usize;
            } else {
                break;
            }
        }
        best_offset
    } else {
        meta.audio_start
    };

    let mut all_samples: Vec<f32> = Vec::new();
    let mut reader = BitReader::new(data, start_byte);
    let mut current_sample: u64 = 0;
    // Track whether we've established the sample position from frame headers
    let mut sample_pos_known = start_sample == 0;

    loop {
        let sync_result = find_frame_sync(&mut reader);
        let (sync_code, frame_start) = match sync_result {
            Ok(v) => v,
            Err(_) => break,
        };

        let _blocking_strategy = sync_code & 1;

        let block_size_code = reader.read_bits(4)? as u8;
        let sample_rate_code = reader.read_bits(4)? as u8;
        let channel_assignment = reader.read_bits(4)? as u8;
        let sample_size_code = reader.read_bits(3)? as u8;
        let _reserved = reader.read_bits(1)?;

        let frame_or_sample = reader.read_utf8_u64()?;

        let block_size = decode_block_size(block_size_code, &mut reader)?;
        let _frame_sr = decode_sample_rate(sample_rate_code, &mut reader, info.sample_rate)?;
        let frame_bps = decode_bps(sample_size_code, bps)?;

        // CRC-8 verification
        reader.align_to_byte();
        let crc8_pos = reader.byte_pos;
        let expected_crc8 = reader.read_bits(8)? as u8;
        let computed_crc8 = crc8_flac(&data[frame_start..crc8_pos]);
        if computed_crc8 != expected_crc8 {
            return Err(ShravanError::DecodeError(format!(
                "CRC-8 mismatch: expected {expected_crc8:#04X}, computed {computed_crc8:#04X}"
            )));
        }

        // Determine current sample position from frame header
        if !sample_pos_known {
            // For fixed-blocksize: frame_or_sample is frame number
            // For variable-blocksize: frame_or_sample is sample number
            if _blocking_strategy == 0 {
                current_sample = frame_or_sample * u64::from(block_size);
            } else {
                current_sample = frame_or_sample;
            }
            sample_pos_known = true;
        }

        let frame_end_sample = current_sample + u64::from(block_size);

        // Check if we've gone past end_sample
        if let Some(end) = end_sample
            && current_sample >= end
        {
            break;
        }

        let (ch_count, decorrelation) = if channel_assignment < CHANNEL_LEFT_SIDE {
            (channel_assignment + 1, ChannelDecorrelation::Independent)
        } else {
            match channel_assignment {
                CHANNEL_LEFT_SIDE => (2, ChannelDecorrelation::LeftSide),
                CHANNEL_RIGHT_SIDE => (2, ChannelDecorrelation::RightSide),
                CHANNEL_MID_SIDE => (2, ChannelDecorrelation::MidSide),
                _ => {
                    return Err(ShravanError::DecodeError(format!(
                        "reserved channel assignment: {channel_assignment}"
                    )));
                }
            }
        };

        let mut channel_data: Vec<Vec<i64>> = Vec::with_capacity(ch_count as usize);
        for ch in 0..ch_count {
            let effective_bps = match decorrelation {
                ChannelDecorrelation::LeftSide if ch == 1 => frame_bps + 1,
                ChannelDecorrelation::RightSide if ch == 0 => frame_bps + 1,
                ChannelDecorrelation::MidSide if ch == 1 => frame_bps + 1,
                _ => frame_bps,
            };
            let subframe = decode_subframe(&mut reader, block_size as usize, effective_bps)?;
            channel_data.push(subframe);
        }

        apply_decorrelation(&mut channel_data, decorrelation);

        // Interleave and convert to f32, applying sample range trimming
        let frame_scale = if frame_bps != bps {
            1.0f64 / f64::from(1u32 << (frame_bps - 1))
        } else {
            scale
        };

        let skip_start = if current_sample < start_sample {
            (start_sample - current_sample) as usize
        } else {
            0
        };
        let take_end = if let Some(end) = end_sample {
            if frame_end_sample > end {
                (end - current_sample) as usize
            } else {
                block_size as usize
            }
        } else {
            block_size as usize
        };

        for i in skip_start..take_end.min(block_size as usize) {
            for ch_samples in &channel_data {
                if i < ch_samples.len() {
                    all_samples.push((ch_samples[i] as f64 * frame_scale) as f32);
                }
            }
        }

        // CRC-16 verification
        reader.align_to_byte();
        if reader.byte_pos + 2 <= reader.data.len() {
            let crc16_pos = reader.byte_pos;
            let expected_crc16 = reader.read_bits(16)? as u16;
            let computed_crc16 = crc16_flac(&data[frame_start..crc16_pos]);
            if computed_crc16 != expected_crc16 {
                return Err(ShravanError::DecodeError(format!(
                    "CRC-16 mismatch: expected {expected_crc16:#06X}, computed {computed_crc16:#06X}"
                )));
            }
        }

        current_sample = frame_end_sample;
    }

    let total_frames = if channels > 0 {
        all_samples.len() as u64 / u64::from(channels)
    } else {
        0
    };
    let duration_secs = total_frames as f64 / f64::from(info.sample_rate);

    let format_info = FormatInfo {
        format: AudioFormat::Flac,
        sample_rate: info.sample_rate,
        channels: u16::from(channels),
        bit_depth: u16::from(bps),
        duration_secs,
        total_samples: total_frames,
    };

    Ok((format_info, all_samples))
}

/// Channel decorrelation modes.
#[derive(Debug, Clone, Copy)]
enum ChannelDecorrelation {
    Independent,
    LeftSide,
    RightSide,
    MidSide,
}

/// Apply channel decorrelation to reconstruct original samples.
fn apply_decorrelation(channels: &mut [Vec<i64>], mode: ChannelDecorrelation) {
    if channels.len() != 2 {
        return;
    }
    let len = channels[0].len().min(channels[1].len());
    let (ch0, rest) = channels.split_at_mut(1);
    let ch0 = &mut ch0[0][..len];
    let ch1 = &mut rest[0][..len];
    match mode {
        ChannelDecorrelation::Independent => {}
        ChannelDecorrelation::LeftSide => {
            // ch0 = left, ch1 = left - right -> right = left - side
            for (l, s) in ch0.iter().zip(ch1.iter_mut()) {
                *s = *l - *s;
            }
        }
        ChannelDecorrelation::RightSide => {
            // ch0 = left - right, ch1 = right -> left = side + right
            for (s, r) in ch0.iter_mut().zip(ch1.iter()) {
                *s += *r;
            }
        }
        ChannelDecorrelation::MidSide => {
            // FLAC spec: mid <<= 1, mid |= (side & 1), left = (mid + side) >> 1, right = (mid - side) >> 1
            for (m, s) in ch0.iter_mut().zip(ch1.iter_mut()) {
                let mid = (*m << 1) | (*s & 1);
                let side = *s;
                *m = (mid + side) >> 1;
                *s = (mid - side) >> 1;
            }
        }
    }
}

/// Find the next frame sync code (0xFFF8 or 0xFFF9).
/// Returns (sync_code, byte_offset_of_sync).
fn find_frame_sync(reader: &mut BitReader<'_>) -> Result<(u16, usize)> {
    reader.align_to_byte();
    // Look for 0xFF followed by 0xF8 or 0xF9
    while reader.byte_pos + 1 < reader.data.len() {
        if reader.data[reader.byte_pos] == 0xFF {
            let next = reader.data[reader.byte_pos + 1];
            if next == 0xF8 || next == 0xF9 {
                let code = u16::from(reader.data[reader.byte_pos]) << 8 | u16::from(next);
                let sync_pos = reader.byte_pos;
                reader.byte_pos += 2;
                reader.bit_pos = 0;
                return Ok((code, sync_pos));
            }
        }
        reader.byte_pos += 1;
    }
    Err(ShravanError::EndOfStream)
}

/// Decode block size from the 4-bit code.
fn decode_block_size(code: u8, reader: &mut BitReader<'_>) -> Result<u32> {
    match code {
        0 => Err(ShravanError::DecodeError(
            "reserved block size code 0".into(),
        )),
        1 => Ok(192),
        2..=5 => Ok(576 << (code - 2)),
        6 => {
            let val = reader.read_bits(8)?;
            Ok(val + 1)
        }
        7 => {
            let val = reader.read_bits(16)?;
            Ok(val + 1)
        }
        8..=15 => Ok(256 << (code - 8)),
        _ => Err(ShravanError::DecodeError(format!(
            "invalid block size code: {code}"
        ))),
    }
}

/// Decode sample rate from the 4-bit code.
fn decode_sample_rate(code: u8, reader: &mut BitReader<'_>, streaminfo_rate: u32) -> Result<u32> {
    match code {
        0 => Ok(streaminfo_rate),
        1 => Ok(88200),
        2 => Ok(176400),
        3 => Ok(192000),
        4 => Ok(8000),
        5 => Ok(16000),
        6 => Ok(22050),
        7 => Ok(24000),
        8 => Ok(32000),
        9 => Ok(44100),
        10 => Ok(48000),
        11 => Ok(96000),
        12 => {
            let val = reader.read_bits(8)?;
            Ok(val * 1000)
        }
        13 => {
            let val = reader.read_bits(16)?;
            Ok(val)
        }
        14 => {
            let val = reader.read_bits(16)?;
            Ok(val * 10)
        }
        15 => Err(ShravanError::DecodeError(
            "invalid sample rate code 15".into(),
        )),
        _ => Err(ShravanError::DecodeError(format!(
            "invalid sample rate code: {code}"
        ))),
    }
}

/// Decode bits per sample from the 3-bit code.
fn decode_bps(code: u8, streaminfo_bps: u8) -> Result<u8> {
    match code {
        0 => Ok(streaminfo_bps),
        1 => Ok(8),
        2 => Ok(12),
        3 => Err(ShravanError::DecodeError("reserved bps code 3".into())),
        4 => Ok(16),
        5 => Ok(20),
        6 => Ok(24),
        7 => Err(ShravanError::DecodeError("reserved bps code 7".into())),
        _ => Err(ShravanError::DecodeError(format!(
            "invalid bps code: {code}"
        ))),
    }
}

/// Decode a single subframe.
fn decode_subframe(reader: &mut BitReader<'_>, block_size: usize, bps: u8) -> Result<Vec<i64>> {
    // Subframe header: 1 zero bit + 6-bit type + optional wasted bits
    let zero = reader.read_bits(1)?;
    if zero != 0 {
        return Err(ShravanError::DecodeError(
            "subframe header zero bit is not zero".into(),
        ));
    }

    let subframe_type = reader.read_bits(6)? as u8;
    let has_wasted = reader.read_bit()?;
    let wasted_bits = if has_wasted {
        // Read unary-coded wasted bits per sample (k+1)
        reader.read_unary()? + 1
    } else {
        0
    };

    let effective_bps = bps - wasted_bits as u8;

    let mut samples = match subframe_type {
        0 => {
            // CONSTANT: one sample repeated
            decode_constant(reader, block_size, effective_bps)?
        }
        1 => {
            // VERBATIM: raw samples
            decode_verbatim(reader, block_size, effective_bps)?
        }
        8..=12 => {
            // FIXED: prediction order = type - 8
            let order = (subframe_type - 8) as usize;
            if order > 4 {
                return Err(ShravanError::DecodeError(format!(
                    "invalid fixed prediction order: {order}"
                )));
            }
            decode_fixed(reader, block_size, effective_bps, order)?
        }
        32..=63 => {
            // LPC: order = type - 31
            let order = (subframe_type - 31) as usize;
            decode_lpc(reader, block_size, effective_bps, order)?
        }
        _ => {
            return Err(ShravanError::DecodeError(format!(
                "reserved subframe type: {subframe_type}"
            )));
        }
    };

    // Apply wasted bits shift
    if wasted_bits > 0 {
        for s in &mut samples {
            *s <<= wasted_bits;
        }
    }

    Ok(samples)
}

/// Decode a CONSTANT subframe.
fn decode_constant(reader: &mut BitReader<'_>, block_size: usize, bps: u8) -> Result<Vec<i64>> {
    let raw = reader.read_bits(bps)? as i64;
    let value = sign_extend(raw, bps);
    Ok(vec![value; block_size])
}

/// Decode a VERBATIM subframe.
fn decode_verbatim(reader: &mut BitReader<'_>, block_size: usize, bps: u8) -> Result<Vec<i64>> {
    let mut samples = Vec::with_capacity(block_size);
    for _ in 0..block_size {
        let raw = reader.read_bits(bps)? as i64;
        samples.push(sign_extend(raw, bps));
    }
    Ok(samples)
}

/// Decode a FIXED prediction subframe.
fn decode_fixed(
    reader: &mut BitReader<'_>,
    block_size: usize,
    bps: u8,
    order: usize,
) -> Result<Vec<i64>> {
    // Read warm-up samples
    let mut samples = Vec::with_capacity(block_size);
    for _ in 0..order {
        let raw = reader.read_bits(bps)? as i64;
        samples.push(sign_extend(raw, bps));
    }

    // Read residual
    let residuals = decode_residual(reader, block_size, order)?;

    // Apply fixed prediction
    for (i, &residual) in residuals.iter().enumerate() {
        let idx = order + i;
        let predicted = match order {
            0 => residual,
            1 => samples[idx - 1] + residual,
            2 => 2 * samples[idx - 1] - samples[idx - 2] + residual,
            3 => 3 * samples[idx - 1] - 3 * samples[idx - 2] + samples[idx - 3] + residual,
            4 => {
                4 * samples[idx - 1] - 6 * samples[idx - 2] + 4 * samples[idx - 3]
                    - samples[idx - 4]
                    + residual
            }
            _ => {
                return Err(ShravanError::DecodeError(format!(
                    "unsupported fixed order: {order}"
                )));
            }
        };
        samples.push(predicted);
    }

    Ok(samples)
}

/// Decode an LPC (Linear Predictive Coding) subframe.
fn decode_lpc(
    reader: &mut BitReader<'_>,
    block_size: usize,
    bps: u8,
    order: usize,
) -> Result<Vec<i64>> {
    // Read warm-up samples
    let mut samples = Vec::with_capacity(block_size);
    for _ in 0..order {
        let raw = reader.read_bits(bps)? as i64;
        samples.push(sign_extend(raw, bps));
    }

    // Quantized LP coefficient precision (4 bits, value 15 is invalid)
    let precision_raw = reader.read_bits(4)? as u8;
    if precision_raw == 15 {
        return Err(ShravanError::DecodeError(
            "invalid qlp_coeff_precision 15".into(),
        ));
    }
    let qlp_precision = precision_raw + 1;

    // Quantized LP coefficient shift (5 bits, signed)
    let shift_raw = reader.read_bits(5)? as i64;
    let qlp_shift = sign_extend(shift_raw, 5) as i32;

    // Read quantized LP coefficients
    let mut coefficients = Vec::with_capacity(order);
    for _ in 0..order {
        let raw = reader.read_bits(qlp_precision)? as i64;
        coefficients.push(sign_extend(raw, qlp_precision) as i32);
    }

    // Read residuals (same coding as Fixed subframes)
    let residuals = decode_residual(reader, block_size, order)?;

    // Apply LPC prediction
    for (i, &residual) in residuals.iter().enumerate() {
        let n = order + i;
        let mut accumulator: i64 = 0;
        for (j, &coeff) in coefficients.iter().enumerate() {
            accumulator += i64::from(coeff) * samples[n - 1 - j];
        }
        let predicted = if qlp_shift >= 0 {
            accumulator >> qlp_shift
        } else {
            accumulator << (-qlp_shift)
        };
        samples.push(predicted + residual);
    }

    Ok(samples)
}

/// Decode Rice-coded residuals.
fn decode_residual(
    reader: &mut BitReader<'_>,
    block_size: usize,
    predictor_order: usize,
) -> Result<Vec<i64>> {
    let coding_method = reader.read_bits(2)?;
    let rice_param_bits: u8 = match coding_method {
        0 => 4, // RICE
        1 => 5, // RICE2
        _ => {
            return Err(ShravanError::DecodeError(format!(
                "unsupported residual coding method: {coding_method}"
            )));
        }
    };

    let partition_order = reader.read_bits(4)? as usize;
    let num_partitions = 1usize << partition_order;

    let total_residuals = block_size - predictor_order;
    let mut residuals = Vec::with_capacity(total_residuals);

    for partition in 0..num_partitions {
        let count = if partition_order == 0 {
            block_size - predictor_order
        } else if partition == 0 {
            (block_size >> partition_order) - predictor_order
        } else {
            block_size >> partition_order
        };

        let rice_param = reader.read_bits(rice_param_bits)?;
        let escape = if rice_param_bits == 4 { 15 } else { 31 };

        if rice_param == escape {
            // Escape code: raw bits per sample
            let raw_bits = reader.read_bits(5)? as u8;
            for _ in 0..count {
                let raw = reader.read_bits(raw_bits)? as i64;
                residuals.push(sign_extend(raw, raw_bits));
            }
        } else {
            // Rice-coded residuals
            for _ in 0..count {
                let quotient = reader.read_unary()?;
                let remainder = if rice_param > 0 {
                    reader.read_bits(rice_param as u8)?
                } else {
                    0
                };
                let unsigned_val = (quotient << rice_param) | remainder;
                // Zigzag decode: even -> positive, odd -> negative
                let signed_val = if unsigned_val & 1 == 0 {
                    (unsigned_val >> 1) as i64
                } else {
                    -((unsigned_val >> 1) as i64) - 1
                };
                residuals.push(signed_val);
            }
        }
    }

    Ok(residuals)
}

/// Sign-extend a value from the given bit width.
#[inline]
fn sign_extend(value: i64, bits: u8) -> i64 {
    if bits == 0 || bits >= 64 {
        return value;
    }
    let shift = 64 - bits as u32;
    (value << shift) >> shift
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Bitstream writer for constructing FLAC frames (MSB-first).
struct BitWriter {
    buf: Vec<u8>,
    current_byte: u8,
    bit_pos: u8, // bits written in current byte (0..8)
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            current_byte: 0,
            bit_pos: 0,
        }
    }

    fn with_capacity(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
            current_byte: 0,
            bit_pos: 0,
        }
    }

    /// Write up to 32 bits (MSB-first).
    fn write_bits(&mut self, value: u32, n: u8) {
        if n == 0 {
            return;
        }
        let mut remaining = n;
        let val = value;
        while remaining > 0 {
            let available = 8 - self.bit_pos;
            let to_write = remaining.min(available);
            let bits = (val >> (remaining - to_write)) & ((1u32 << to_write) - 1);
            self.current_byte |= (bits as u8) << (available - to_write);
            remaining -= to_write;
            self.bit_pos += to_write;
            if self.bit_pos >= 8 {
                self.buf.push(self.current_byte);
                self.current_byte = 0;
                self.bit_pos = 0;
            }
        }
    }

    /// Write a single bit.
    #[inline]
    fn write_bit(&mut self, b: bool) {
        self.write_bits(u32::from(b), 1);
    }

    /// Write a unary-coded value (count zero-bits, then a 1-bit).
    fn write_unary(&mut self, val: u32) {
        for _ in 0..val {
            self.write_bit(false);
        }
        self.write_bit(true);
    }

    /// Pad to the next byte boundary with zero bits.
    fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.buf.push(self.current_byte);
            self.current_byte = 0;
            self.bit_pos = 0;
        }
    }

    /// Return the accumulated bytes. Flushes any partial byte.
    fn into_bytes(mut self) -> Vec<u8> {
        self.align_to_byte();
        self.buf
    }

    /// Current byte length (excluding any partial byte).
    #[allow(dead_code)]
    fn byte_len(&self) -> usize {
        self.buf.len()
    }

    /// Write a UTF-8-like coded frame number (FLAC convention).
    fn write_utf8_u64(&mut self, value: u64) {
        if value < 0x80 {
            self.write_bits(value as u32, 8);
        } else if value < 0x800 {
            self.write_bits(0xC0 | ((value >> 6) as u32), 8);
            self.write_bits(0x80 | ((value & 0x3F) as u32), 8);
        } else if value < 0x10000 {
            self.write_bits(0xE0 | ((value >> 12) as u32), 8);
            self.write_bits(0x80 | (((value >> 6) & 0x3F) as u32), 8);
            self.write_bits(0x80 | ((value & 0x3F) as u32), 8);
        } else if value < 0x200000 {
            self.write_bits(0xF0 | ((value >> 18) as u32), 8);
            self.write_bits(0x80 | (((value >> 12) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 6) & 0x3F) as u32), 8);
            self.write_bits(0x80 | ((value & 0x3F) as u32), 8);
        } else if value < 0x4000000 {
            self.write_bits(0xF8 | ((value >> 24) as u32), 8);
            self.write_bits(0x80 | (((value >> 18) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 12) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 6) & 0x3F) as u32), 8);
            self.write_bits(0x80 | ((value & 0x3F) as u32), 8);
        } else if value < 0x80000000 {
            self.write_bits(0xFC | ((value >> 30) as u32), 8);
            self.write_bits(0x80 | (((value >> 24) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 18) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 12) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 6) & 0x3F) as u32), 8);
            self.write_bits(0x80 | ((value & 0x3F) as u32), 8);
        } else {
            self.write_bits(0xFE, 8);
            self.write_bits(0x80 | (((value >> 30) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 24) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 18) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 12) & 0x3F) as u32), 8);
            self.write_bits(0x80 | (((value >> 6) & 0x3F) as u32), 8);
            self.write_bits(0x80 | ((value & 0x3F) as u32), 8);
        }
    }
}

/// Zigzag encode a signed value for Rice coding.
#[inline]
fn zigzag_encode(val: i64) -> u64 {
    if val >= 0 {
        (val as u64) << 1
    } else {
        ((-val as u64) << 1) - 1
    }
}

/// Compute the optimal Rice parameter for a partition.
fn optimal_rice_param(residuals: &[i64]) -> u8 {
    if residuals.is_empty() {
        return 0;
    }
    let sum: u64 = residuals.iter().map(|&r| zigzag_encode(r)).sum();
    let mean = sum / residuals.len() as u64;
    if mean == 0 {
        return 0;
    }
    (64 - mean.leading_zeros() - 1).min(14) as u8
}

/// Estimate the bit cost of Rice-encoding a set of residuals with a given parameter.
fn rice_bit_cost(residuals: &[i64], param: u8) -> u64 {
    let mut cost = 0u64;
    for &r in residuals {
        let unsigned = zigzag_encode(r);
        let quotient = unsigned >> param;
        cost += quotient + 1 + u64::from(param);
    }
    cost
}

/// Encode Rice-coded residuals into a BitWriter.
fn encode_residual(writer: &mut BitWriter, residuals: &[i64], block_size: usize, order: usize) {
    // Coding method 0 (RICE, 4-bit params), partition order 0
    writer.write_bits(0, 2); // coding method
    writer.write_bits(0, 4); // partition order

    let rice_param = optimal_rice_param(residuals);
    writer.write_bits(u32::from(rice_param), 4);

    for &r in residuals {
        let unsigned = zigzag_encode(r);
        let quotient = (unsigned >> rice_param) as u32;
        let remainder = (unsigned & ((1u64 << rice_param) - 1)) as u32;
        writer.write_unary(quotient);
        if rice_param > 0 {
            writer.write_bits(remainder, rice_param);
        }
    }
    let _ = (block_size, order); // used by caller for partition sizing
}

/// Compute fixed prediction residuals for a given order.
fn compute_fixed_residuals(samples: &[i64], order: usize) -> Vec<i64> {
    let mut residuals = Vec::with_capacity(samples.len().saturating_sub(order));
    for i in order..samples.len() {
        let predicted = match order {
            0 => 0,
            1 => samples[i - 1],
            2 => 2 * samples[i - 1] - samples[i - 2],
            3 => 3 * samples[i - 1] - 3 * samples[i - 2] + samples[i - 3],
            4 => 4 * samples[i - 1] - 6 * samples[i - 2] + 4 * samples[i - 3] - samples[i - 4],
            _ => 0,
        };
        residuals.push(samples[i] - predicted);
    }
    residuals
}

/// Encode a Verbatim subframe and return the bit cost.
fn encode_subframe_verbatim_cost(samples: &[i64], bps: u8) -> u64 {
    // 8-bit header + bps * block_size
    8 + u64::from(bps) * samples.len() as u64
}

/// Encode a Fixed subframe and return (order, bit_cost).
fn best_fixed_order(samples: &[i64], bps: u8) -> (usize, u64) {
    let mut best_order = 0usize;
    let mut best_cost = u64::MAX;

    for order in 0..=4.min(samples.len()) {
        let residuals = compute_fixed_residuals(samples, order);
        let param = optimal_rice_param(&residuals);
        // Cost: 8-bit header + order*bps warm-up + 6 bits residual header + rice encoded
        let cost = 8 + u64::from(bps) * order as u64 + 6 + rice_bit_cost(&residuals, param);
        if cost < best_cost {
            best_cost = cost;
            best_order = order;
        }
    }
    (best_order, best_cost)
}

/// Encode a single subframe into a BitWriter.
fn encode_subframe(writer: &mut BitWriter, samples: &[i64], bps: u8) {
    let verbatim_cost = encode_subframe_verbatim_cost(samples, bps);
    let (fixed_order, fixed_cost) = best_fixed_order(samples, bps);

    if verbatim_cost <= fixed_cost || samples.len() <= 4 {
        // Verbatim
        writer.write_bit(false); // zero padding
        writer.write_bits(1, 6); // type = verbatim
        writer.write_bit(false); // no wasted bits
        for &s in samples {
            writer.write_bits(s as u32, bps);
        }
    } else {
        // Fixed
        writer.write_bit(false); // zero padding
        writer.write_bits(8 + fixed_order as u32, 6); // type = fixed + order
        writer.write_bit(false); // no wasted bits

        // Warm-up samples
        for &s in &samples[..fixed_order] {
            writer.write_bits(s as u32, bps);
        }

        // Residuals
        let residuals = compute_fixed_residuals(samples, fixed_order);
        encode_residual(writer, &residuals, samples.len(), fixed_order);
    }
}

/// Apply mid-side encoding to stereo channels, returning (mid, side).
fn apply_mid_side(left: &[i64], right: &[i64]) -> (Vec<i64>, Vec<i64>) {
    let mut mid = Vec::with_capacity(left.len());
    let mut side = Vec::with_capacity(left.len());
    for (&l, &r) in left.iter().zip(right.iter()) {
        side.push(l - r);
        // Mid is stored with the LSB of side baked in via the decoder formula.
        // Encoder stores: mid = (left + right) >> 1 (integer truncation)
        mid.push((l + r) >> 1);
    }
    (mid, side)
}

/// Encode a block size value into a 4-bit code, returning (code, optional_tail_bits, tail_value).
fn block_size_code(block_size: u32) -> (u8, u8, u32) {
    match block_size {
        192 => (1, 0, 0),
        576 => (2, 0, 0),
        1152 => (3, 0, 0),
        2304 => (4, 0, 0),
        4608 => (5, 0, 0),
        256 => (8, 0, 0),
        512 => (9, 0, 0),
        1024 => (10, 0, 0),
        2048 => (11, 0, 0),
        4096 => (12, 0, 0),
        8192 => (13, 0, 0),
        16384 => (14, 0, 0),
        32768 => (15, 0, 0),
        1..=256 => (6, 8, block_size - 1),
        _ => (7, 16, block_size - 1),
    }
}

/// Map a sample rate to a 4-bit code.
fn sample_rate_code(rate: u32) -> (u8, u8, u32) {
    match rate {
        88200 => (1, 0, 0),
        176400 => (2, 0, 0),
        192000 => (3, 0, 0),
        8000 => (4, 0, 0),
        16000 => (5, 0, 0),
        22050 => (6, 0, 0),
        24000 => (7, 0, 0),
        32000 => (8, 0, 0),
        44100 => (9, 0, 0),
        48000 => (10, 0, 0),
        96000 => (11, 0, 0),
        r if r % 1000 == 0 && r / 1000 <= 255 => (12, 8, r / 1000),
        r if r <= 65535 => (13, 16, r),
        r if r % 10 == 0 && r / 10 <= 65535 => (14, 16, r / 10),
        _ => (0, 0, 0), // use STREAMINFO
    }
}

/// Map bits per sample to a 3-bit code.
fn bps_code(bps: u8) -> u8 {
    match bps {
        8 => 1,
        12 => 2,
        16 => 4,
        20 => 5,
        24 => 6,
        _ => 0, // use STREAMINFO
    }
}

/// Encode a single FLAC frame.
fn encode_frame(
    out: &mut Vec<u8>,
    channel_samples: &[&[i64]],
    channels: u16,
    bps: u8,
    sample_rate: u32,
    frame_number: u32,
    block_size: u32,
) {
    let frame_start = out.len();

    let mut header = BitWriter::new();

    // Sync code (14 bits) + reserved (1 bit) + blocking strategy (1 bit = 0 for fixed)
    header.write_bits(0xFFF8 >> 2, 14);
    header.write_bit(false); // reserved
    header.write_bit(false); // fixed-blocksize

    // Block size code
    let (bs_code, bs_tail_bits, bs_tail_val) = block_size_code(block_size);
    header.write_bits(u32::from(bs_code), 4);

    // Sample rate code
    let (sr_code, sr_tail_bits, sr_tail_val) = sample_rate_code(sample_rate);
    header.write_bits(u32::from(sr_code), 4);

    // Channel assignment
    let use_mid_side = channels == 2;
    let channel_assignment: u8 = if use_mid_side {
        CHANNEL_MID_SIDE // 10
    } else {
        channels as u8 - 1
    };
    header.write_bits(u32::from(channel_assignment), 4);

    // Sample size code
    header.write_bits(u32::from(bps_code(bps)), 3);

    // Reserved bit
    header.write_bit(false);

    // Frame number (UTF-8 coded)
    header.write_utf8_u64(u64::from(frame_number));

    // Block size tail
    if bs_tail_bits > 0 {
        header.write_bits(bs_tail_val, bs_tail_bits);
    }

    // Sample rate tail
    if sr_tail_bits > 0 {
        header.write_bits(sr_tail_val, sr_tail_bits);
    }

    header.align_to_byte();
    let header_bytes = header.into_bytes();

    // Compute CRC-8
    let crc8 = crc8_flac(&header_bytes);
    out.extend_from_slice(&header_bytes);
    out.push(crc8);

    // Encode subframes
    let mut subframe_writer = BitWriter::with_capacity(block_size as usize * channels as usize * 4);

    if use_mid_side && channel_samples.len() == 2 {
        let (mid, side) = apply_mid_side(channel_samples[0], channel_samples[1]);
        // Mid channel: normal bps
        encode_subframe(&mut subframe_writer, &mid, bps);
        // Side channel: bps + 1
        encode_subframe(&mut subframe_writer, &side, bps + 1);
    } else {
        for ch in channel_samples {
            encode_subframe(&mut subframe_writer, ch, bps);
        }
    }

    subframe_writer.align_to_byte();
    let subframe_bytes = subframe_writer.into_bytes();
    out.extend_from_slice(&subframe_bytes);

    // CRC-16 over entire frame
    let crc16 = crc16_flac(&out[frame_start..]);
    out.extend_from_slice(&crc16.to_be_bytes());
}

// Minimal inline MD5 implementation for FLAC STREAMINFO.
fn md5_compute(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    // Pre-processing: padding
    let orig_len_bits = (data.len() as u64) * 8;
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g])).rotate_left(S[i]),
            );
            a = temp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());
    result
}

/// Encode interleaved f32 samples as a FLAC byte stream.
///
/// # Arguments
///
/// * `samples` - Interleaved f32 sample data in \[-1.0, 1.0\]
/// * `sample_rate` - Sample rate in Hz
/// * `channels` - Number of audio channels
/// * `bits_per_sample` - Target bit depth (8, 12, 16, 20, or 24)
///
/// # Errors
///
/// Returns errors for invalid parameters.
pub fn encode(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u8,
) -> Result<Vec<u8>> {
    if channels == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }
    if sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }
    if bits_per_sample == 0 || bits_per_sample > 32 {
        return Err(ShravanError::EncodeError(
            "bits_per_sample must be 1..=32".into(),
        ));
    }

    let ch = channels as usize;
    let total_interleaved = samples.len();
    let total_frames = total_interleaved / ch;
    let scale = f64::from(1u32 << (bits_per_sample - 1));

    // Convert f32 -> i64 at target bit depth
    let int_samples: Vec<i64> = samples
        .iter()
        .map(|&s| (f64::from(s.clamp(-1.0, 1.0)) * (scale - 1.0)) as i64)
        .collect();

    // Compute MD5 over raw samples as little-endian signed integers
    let bytes_per_sample = bits_per_sample.div_ceil(8) as usize;
    let mut md5_data = Vec::with_capacity(int_samples.len() * bytes_per_sample);
    for &s in &int_samples {
        for b in 0..bytes_per_sample {
            md5_data.push((s >> (b * 8)) as u8);
        }
    }
    let md5 = md5_compute(&md5_data);

    // Deinterleave into per-channel buffers
    let mut channel_bufs: Vec<Vec<i64>> =
        (0..ch).map(|_| Vec::with_capacity(total_frames)).collect();
    for frame in 0..total_frames {
        for (c, buf) in channel_bufs.iter_mut().enumerate() {
            buf.push(int_samples[frame * ch + c]);
        }
    }

    let block_size: u32 = 4096;

    let mut out = Vec::with_capacity(total_interleaved * 2);

    // fLaC magic
    out.extend_from_slice(b"fLaC");

    // STREAMINFO metadata block
    let _streaminfo_offset = out.len();
    out.push(0x80); // is_last=1, type=0 (STREAMINFO)
    out.push(0);
    out.push(0);
    out.push(34); // size = 34

    let actual_bs = block_size.min(total_frames as u32).max(1);
    out.extend_from_slice(&(actual_bs as u16).to_be_bytes()); // min block size
    out.extend_from_slice(&(actual_bs as u16).to_be_bytes()); // max block size
    // Placeholder for min/max frame size (offset +8 and +11 from STREAMINFO data start)
    let frame_size_offset = out.len();
    out.extend_from_slice(&[0, 0, 0]); // min frame size
    out.extend_from_slice(&[0, 0, 0]); // max frame size

    // Sample rate (20 bits) | channels-1 (3 bits) | bps-1 (5 bits) | total_samples (36 bits)
    let sr = sample_rate;
    let ch_minus_1 = (channels - 1) as u8;
    let bps_minus_1 = bits_per_sample - 1;
    let total_samples_u36 = total_frames as u64;

    out.push((sr >> 12) as u8);
    out.push(((sr >> 4) & 0xFF) as u8);
    out.push((((sr & 0x0F) << 4) as u8) | ((ch_minus_1 & 0x07) << 1) | ((bps_minus_1 >> 4) & 0x01));
    out.push(((bps_minus_1 & 0x0F) << 4) | ((total_samples_u36 >> 32) & 0x0F) as u8);
    out.extend_from_slice(&(total_samples_u36 as u32).to_be_bytes());
    out.extend_from_slice(&md5);

    // Encode frames
    let mut min_frame_size = u32::MAX;
    let mut max_frame_size = 0u32;

    let num_blocks = total_frames.div_ceil(block_size as usize);
    for block_idx in 0..num_blocks {
        let start = block_idx * block_size as usize;
        let end = (start + block_size as usize).min(total_frames);
        let this_block_size = (end - start) as u32;

        let block_channels: Vec<&[i64]> = channel_bufs.iter().map(|b| &b[start..end]).collect();

        let frame_start_pos = out.len();
        encode_frame(
            &mut out,
            &block_channels,
            channels,
            bits_per_sample,
            sample_rate,
            block_idx as u32,
            this_block_size,
        );
        let frame_size = (out.len() - frame_start_pos) as u32;
        min_frame_size = min_frame_size.min(frame_size);
        max_frame_size = max_frame_size.max(frame_size);
    }

    // Backfill min/max frame sizes
    if min_frame_size != u32::MAX {
        out[frame_size_offset] = (min_frame_size >> 16) as u8;
        out[frame_size_offset + 1] = (min_frame_size >> 8) as u8;
        out[frame_size_offset + 2] = min_frame_size as u8;
        out[frame_size_offset + 3] = (max_frame_size >> 16) as u8;
        out[frame_size_offset + 4] = (max_frame_size >> 8) as u8;
        out[frame_size_offset + 5] = max_frame_size as u8;
    }

    Ok(out)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::identity_op,
    clippy::erasing_op,
    clippy::eq_op,
    clippy::needless_late_init
)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_flac() {
        let data = b"RIFF\x00\x00\x00\x00WAVE";
        assert!(decode(data).is_err());
    }

    #[test]
    fn rejects_too_short() {
        let data = b"fLa";
        assert!(decode(data).is_err());
    }

    #[test]
    fn sign_extend_positive() {
        assert_eq!(sign_extend(127, 8), 127);
        assert_eq!(sign_extend(0, 8), 0);
    }

    #[test]
    fn sign_extend_negative() {
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x80, 8), -128);
    }

    #[test]
    fn sign_extend_16bit() {
        assert_eq!(sign_extend(0xFFFF, 16), -1);
        assert_eq!(sign_extend(0x7FFF, 16), 32767);
        assert_eq!(sign_extend(0x8000, 16), -32768);
    }

    #[test]
    fn bitreader_reads_bytes() {
        let data = [0xAB, 0xCD];
        let mut reader = BitReader::new(&data, 0);
        assert_eq!(reader.read_bits(8).unwrap(), 0xAB);
        assert_eq!(reader.read_bits(8).unwrap(), 0xCD);
    }

    #[test]
    fn bitreader_reads_partial_bits() {
        let data = [0b1010_0110];
        let mut reader = BitReader::new(&data, 0);
        assert_eq!(reader.read_bits(4).unwrap(), 0b1010);
        assert_eq!(reader.read_bits(4).unwrap(), 0b0110);
    }

    #[test]
    fn bitreader_unary() {
        // 0001... -> unary value is 3
        let data = [0b0001_0000];
        let mut reader = BitReader::new(&data, 0);
        assert_eq!(reader.read_unary().unwrap(), 3);
    }

    #[test]
    fn bitreader_end_of_stream() {
        let data = [0xFF];
        let mut reader = BitReader::new(&data, 0);
        let _ = reader.read_bits(8).unwrap();
        assert!(reader.read_bits(1).is_err());
    }

    /// Build a minimal valid FLAC file with a CONSTANT subframe for testing.
    fn build_minimal_flac_constant(value: i16, block_size: u16, sample_rate: u32) -> Vec<u8> {
        let mut out = Vec::new();

        // fLaC magic
        out.extend_from_slice(b"fLaC");

        // STREAMINFO metadata block (last=true, type=0, size=34)
        out.push(0x80); // is_last=1, type=0
        out.push(0);
        out.push(0);
        out.push(34); // size = 34

        // STREAMINFO data (34 bytes)
        out.extend_from_slice(&block_size.to_be_bytes()); // min block size
        out.extend_from_slice(&block_size.to_be_bytes()); // max block size
        out.extend_from_slice(&[0, 0, 0]); // min frame size (0 = unknown)
        out.extend_from_slice(&[0, 0, 0]); // max frame size (0 = unknown)
        let sr_hi = (sample_rate >> 12) as u8;
        let sr_mid = ((sample_rate >> 4) & 0xFF) as u8;
        let sr_lo_and_ch_bps = ((sample_rate & 0x0F) << 4) as u8 | (0 << 1) | ((15 >> 4) & 1);
        let bps_lo_and_total_hi =
            ((15 & 0x0F) << 4) as u8 | ((block_size as u64 >> 32) & 0x0F) as u8;
        out.push(sr_hi);
        out.push(sr_mid);
        out.push(sr_lo_and_ch_bps);
        out.push(bps_lo_and_total_hi);
        out.extend_from_slice(&(block_size as u32).to_be_bytes());
        out.extend_from_slice(&[0u8; 16]); // MD5

        // Build frame
        let frame_start = out.len();
        out.push(0xFF);
        out.push(0xF8);

        let bs_code: u8 = if block_size <= 256 { 6 } else { 7 };
        let sr_code: u8 = 9; // 44100
        out.push((bs_code << 4) | sr_code);

        // Channel 0 = mono, sample size code 4 = 16-bit
        out.push((0 << 4) | (4 << 1) | 0);

        // Frame number 0
        out.push(0x00);

        // Explicit block size
        if bs_code == 6 {
            out.push((block_size - 1) as u8);
        } else {
            out.extend_from_slice(&(block_size - 1).to_be_bytes());
        }

        // Compute and write CRC-8
        let crc8 = crc8_flac(&out[frame_start..]);
        out.push(crc8);

        // Subframe: CONSTANT
        out.push(0x00); // header: zero bit + type 0 + no wasted bits
        out.extend_from_slice(&value.to_be_bytes()); // 16-bit value

        // CRC-16 over entire frame (from sync to just before CRC-16)
        let crc16 = crc16_flac(&out[frame_start..]);
        out.extend_from_slice(&crc16.to_be_bytes());

        out
    }

    #[test]
    fn decode_constant_subframe() {
        let flac_data = build_minimal_flac_constant(1000, 64, 44100);
        let result = decode(&flac_data);
        match result {
            Ok((info, samples)) => {
                assert_eq!(info.format, AudioFormat::Flac);
                assert_eq!(info.sample_rate, 44100);
                assert_eq!(info.channels, 1);
                assert_eq!(info.bit_depth, 16);
                assert_eq!(samples.len(), 64);
                // All samples should be 1000/32768
                let expected = 1000.0 / 32768.0;
                for (i, s) in samples.iter().enumerate() {
                    assert!(
                        (s - expected).abs() < 0.001,
                        "sample {i}: expected {expected}, got {s}"
                    );
                }
            }
            Err(e) => {
                panic!("decode failed: {e}");
            }
        }
    }

    #[test]
    fn streaminfo_parsing() {
        // Build just enough for STREAMINFO
        let mut data = Vec::new();
        data.extend_from_slice(b"fLaC");
        data.push(0x80); // last block, type 0
        data.push(0);
        data.push(0);
        data.push(34);
        // min/max block size
        data.extend_from_slice(&4096u16.to_be_bytes());
        data.extend_from_slice(&4096u16.to_be_bytes());
        // min/max frame size
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        // sample rate = 44100, channels = 2 (1 stored), bps = 16 (15 stored)
        // 44100 = 0xAC44
        // byte 10: 0xAC (sr >> 12 = 0x0A, but 44100 >> 12 = 10) nah:
        // 44100 in 20 bits: 0x0AC44
        // byte 10: (0x0AC44 >> 12) = 0x0A
        // byte 11: (0x0AC44 >> 4) & 0xFF = 0xC4
        // byte 12 upper 4: 0x0AC44 & 0x0F = 4
        // channels-1 = 1 (3 bits)
        // bps-1 = 15 (5 bits, upper 1 from byte 12 bit 0)
        // byte 12 = 0100_001_0 = 0x42 (sr_lo=4, ch-1=1, bps_hi=0)
        // byte 13 = 1111_0000 (bps_lo=15&0xF=F, total_samples_hi=0) = 0xF0
        data.push(0x0A);
        data.push(0xC4);
        data.push(0x42);
        data.push(0xF0);
        // total samples low 32 bits
        data.extend_from_slice(&0u32.to_be_bytes());
        // MD5
        data.extend_from_slice(&[0u8; 16]);

        let info = parse_streaminfo(&data, 8).unwrap();
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.min_block_size, 4096);
        assert_eq!(info.max_block_size, 4096);
    }

    #[test]
    fn crc8_known_vectors() {
        assert_eq!(crc8_flac(b""), 0x00);
        assert_eq!(crc8_flac(b"\x00"), 0x00);
        // "123456789" is a standard CRC test string
        assert_eq!(crc8_flac(b"123456789"), 0xF4);
    }

    #[test]
    fn crc16_known_vectors() {
        assert_eq!(crc16_flac(b""), 0x0000);
        assert_eq!(crc16_flac(b"\x00"), 0x0000);
        // FLAC CRC-16 (poly 0x8005, MSB-first, init 0)
        // Verify self-consistency: build + check cycle
        let test_data = b"hello FLAC";
        let crc = crc16_flac(test_data);
        assert_ne!(crc, 0); // non-trivial input should produce non-zero CRC
        // Changing any byte should produce a different CRC
        let mut corrupted = test_data.to_vec();
        corrupted[0] ^= 1;
        assert_ne!(crc16_flac(&corrupted), crc);
    }

    #[test]
    fn decode_rejects_corrupted_crc8() {
        let mut data = build_minimal_flac_constant(1000, 64, 44100);
        // Find the CRC-8 byte (it's right after the frame header)
        // Corrupt a byte in the frame header area
        let frame_start = 4 + 4 + 34; // magic + metadata header + STREAMINFO
        // The sync is at frame_start, flip a bit in a header byte after sync
        data[frame_start + 2] ^= 0x01;
        assert!(decode(&data).is_err());
    }

    #[test]
    fn decode_rejects_corrupted_crc16() {
        let mut data = build_minimal_flac_constant(1000, 64, 44100);
        // Corrupt the last byte before CRC-16 (a subframe data byte)
        let len = data.len();
        data[len - 3] ^= 0x01; // corrupt subframe data, CRC-16 will mismatch
        assert!(decode(&data).is_err());
    }

    // --- Encoder tests ---

    #[test]
    fn encode_decode_silence_mono_16bit() {
        let samples = vec![0.0f32; 4096];
        let encoded = encode(&samples, 44100, 1, 16).unwrap();
        assert_eq!(&encoded[0..4], b"fLaC");
        let (info, decoded) = decode(&encoded).unwrap();
        assert_eq!(info.format, AudioFormat::Flac);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bit_depth, 16);
        assert_eq!(decoded.len(), 4096);
        for s in &decoded {
            assert!(s.abs() < 0.001, "expected silence, got {s}");
        }
    }

    #[test]
    fn encode_decode_sine_mono_16bit() {
        let sr = 44100;
        let samples: Vec<f32> = (0..4096)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
            .collect();
        let encoded = encode(&samples, sr, 1, 16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();
        assert_eq!(info.sample_rate, sr);
        assert_eq!(decoded.len(), samples.len());
        // 16-bit quantization tolerance
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.001, "roundtrip mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn encode_decode_stereo_16bit() {
        let sr = 44100;
        let frames = 2048;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / sr as f32;
            let left = libm::sinf(2.0 * core::f32::consts::PI * 440.0 * t);
            let right = libm::sinf(2.0 * core::f32::consts::PI * 880.0 * t);
            samples.push(left);
            samples.push(right);
        }
        let encoded = encode(&samples, sr, 2, 16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(decoded.len(), samples.len());
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!(
                (a - b).abs() < 0.002,
                "stereo roundtrip mismatch: {a} vs {b}"
            );
        }
    }

    #[test]
    fn encode_decode_24bit() {
        let sr = 48000;
        let samples: Vec<f32> = (0..1024)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
            .collect();
        let encoded = encode(&samples, sr, 1, 24).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();
        assert_eq!(info.bit_depth, 24);
        assert_eq!(info.sample_rate, sr);
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!(
                (a - b).abs() < 0.0001,
                "24-bit roundtrip mismatch: {a} vs {b}"
            );
        }
    }

    #[test]
    fn encode_decode_8bit() {
        let samples: Vec<f32> = (0..256).map(|i| (i as f32 / 128.0) - 1.0).collect();
        let encoded = encode(&samples, 44100, 1, 8).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();
        assert_eq!(info.bit_depth, 8);
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.02, "8-bit roundtrip mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn encode_rejects_zero_channels() {
        assert!(encode(&[0.5], 44100, 0, 16).is_err());
    }

    #[test]
    fn encode_rejects_zero_rate() {
        assert!(encode(&[0.5], 0, 1, 16).is_err());
    }

    #[test]
    fn encode_rejects_invalid_bps() {
        assert!(encode(&[0.5], 44100, 1, 0).is_err());
        assert!(encode(&[0.5], 44100, 1, 33).is_err());
    }

    #[test]
    fn encode_empty_input() {
        let encoded = encode(&[], 44100, 1, 16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();
        assert_eq!(info.sample_rate, 44100);
        assert!(decoded.is_empty());
    }

    #[test]
    fn bitwriter_roundtrip_utf8() {
        // Test that BitWriter UTF-8 encoding matches BitReader decoding
        for val in [0u64, 1, 127, 128, 255, 256, 1000, 65535, 100000] {
            let mut w = BitWriter::new();
            w.write_utf8_u64(val);
            let bytes = w.into_bytes();
            let mut r = BitReader::new(&bytes, 0);
            let decoded = r.read_utf8_u64().unwrap();
            assert_eq!(val, decoded, "UTF-8 roundtrip failed for {val}");
        }
    }

    #[test]
    fn bitwriter_basic() {
        let mut w = BitWriter::new();
        w.write_bits(0xFF, 8);
        w.write_bits(0xAB, 8);
        let bytes = w.into_bytes();
        assert_eq!(bytes, vec![0xFF, 0xAB]);
    }

    #[test]
    fn bitwriter_partial_bits() {
        let mut w = BitWriter::new();
        w.write_bits(0b1010, 4);
        w.write_bits(0b0110, 4);
        let bytes = w.into_bytes();
        assert_eq!(bytes, vec![0b1010_0110]);
    }

    #[test]
    fn zigzag_encode_values() {
        assert_eq!(zigzag_encode(0), 0);
        assert_eq!(zigzag_encode(1), 2);
        assert_eq!(zigzag_encode(-1), 1);
        assert_eq!(zigzag_encode(2), 4);
        assert_eq!(zigzag_encode(-2), 3);
    }

    #[test]
    fn md5_basic() {
        // MD5 of empty string
        let result = md5_compute(b"");
        assert_eq!(
            result,
            [
                0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8,
                0x42, 0x7e
            ]
        );
        // MD5 of "abc"
        let result2 = md5_compute(b"abc");
        assert_eq!(
            result2,
            [
                0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
                0x7f, 0x72
            ]
        );
    }

    #[test]
    fn encode_multi_block() {
        // More than one block (>4096 frames)
        let sr = 44100;
        let frames = 8192 + 100; // 2 full blocks + partial
        let samples: Vec<f32> = (0..frames)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
            .collect();
        let encoded = encode(&samples, sr, 1, 16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();
        assert_eq!(info.total_samples, frames as u64);
        assert_eq!(decoded.len(), frames);
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!(
                (a - b).abs() < 0.001,
                "multi-block roundtrip mismatch: {a} vs {b}"
            );
        }
    }

    // --- Seeking tests ---

    #[test]
    fn decode_range_full_matches_decode() {
        let sr = 44100u32;
        let samples: Vec<f32> = (0..4096)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
            .collect();
        let encoded = encode(&samples, sr, 1, 16).unwrap();

        let (_, full) = decode(&encoded).unwrap();
        let (_, ranged) = decode_range(&encoded, 0, None).unwrap();
        assert_eq!(full.len(), ranged.len());
        for (a, b) in full.iter().zip(ranged.iter()) {
            assert!((a - b).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn decode_range_middle_of_block() {
        let sr = 44100u32;
        let frames = 4096;
        let samples: Vec<f32> = (0..frames)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
            .collect();
        let encoded = encode(&samples, sr, 1, 16).unwrap();

        // Decode only samples 1000..3000
        let (info, ranged) = decode_range(&encoded, 1000, Some(3000)).unwrap();
        assert_eq!(ranged.len(), 2000);
        assert_eq!(info.total_samples, 2000);

        // Verify against full decode
        let (_, full) = decode(&encoded).unwrap();
        for (a, b) in full[1000..3000].iter().zip(ranged.iter()) {
            assert!((a - b).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn decode_range_multi_block() {
        let sr = 44100u32;
        let frames = 8192 + 100;
        let samples: Vec<f32> = (0..frames)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
            .collect();
        let encoded = encode(&samples, sr, 1, 16).unwrap();

        // Decode range spanning block boundary (block size = 4096)
        let (_, ranged) = decode_range(&encoded, 4000, Some(4200)).unwrap();
        assert_eq!(ranged.len(), 200);

        let (_, full) = decode(&encoded).unwrap();
        for (a, b) in full[4000..4200].iter().zip(ranged.iter()) {
            assert!((a - b).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn decode_range_past_end() {
        let samples: Vec<f32> = vec![0.5; 100];
        let encoded = encode(&samples, 44100, 1, 16).unwrap();

        // start_sample past total samples -> empty result
        let (info, ranged) = decode_range(&encoded, 200, None).unwrap();
        assert!(ranged.is_empty());
        assert_eq!(info.total_samples, 0);
    }
}
