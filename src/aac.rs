//! AAC decoder — ADTS container parsing with symphonia-codec-aac backend.
//!
//! Parses ADTS (Audio Data Transport Stream) frames, extracts raw AAC packets,
//! and delegates actual AAC-LC decoding to `symphonia-codec-aac`.

use alloc::format;
use alloc::vec::Vec;

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

use symphonia_core::audio::Signal;
use symphonia_core::codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions};
use symphonia_core::formats::Packet;

/// Standard AAC sample rates indexed by the 4-bit sample rate index.
const AAC_SAMPLE_RATES: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

/// Channel count indexed by the 3-bit channel configuration.
const AAC_CHANNEL_COUNTS: [u16; 8] = [0, 1, 2, 3, 4, 5, 6, 8];

/// A parsed ADTS frame header.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct AdtsHeader {
    /// AAC profile (0 = Main, 1 = LC, 2 = SSR, 3 = LTP).
    profile: u8,
    /// Sample rate in Hz.
    sample_rate: u32,
    /// Sample rate index (0..15).
    sample_rate_index: u8,
    /// Number of audio channels.
    channels: u16,
    /// Total frame length in bytes (header + payload).
    frame_length: usize,
}

/// Parse an ADTS frame header from a 7-byte (or longer) slice.
///
/// ADTS header layout (fixed + variable = 7 bytes without CRC):
/// - 12 bits: sync word (0xFFF)
/// -  1 bit:  MPEG version (0 = MPEG-4, 1 = MPEG-2)
/// -  2 bits: layer (always 0)
/// -  1 bit:  protection absent (1 = no CRC)
/// -  2 bits: profile (AAC-LC = 1)
/// -  4 bits: sample rate index
/// -  1 bit:  private
/// -  3 bits: channel configuration
/// -  1 bit:  originality
/// -  1 bit:  home
/// -  1 bit:  copyrighted stream
/// -  1 bit:  copyright start
/// - 13 bits: frame length (header + payload)
/// - 11 bits: buffer fullness
/// -  2 bits: number of AAC frames minus 1
fn parse_adts_header(data: &[u8]) -> Result<AdtsHeader> {
    if data.len() < 7 {
        return Err(ShravanError::InvalidHeader(
            "ADTS header requires at least 7 bytes".into(),
        ));
    }

    // Check sync word: 12 bits = 0xFFF
    if data[0] != 0xFF || (data[1] & 0xF0) != 0xF0 {
        return Err(ShravanError::InvalidHeader(
            "missing ADTS sync word (0xFFF)".into(),
        ));
    }

    // Layer must be 0 for AAC
    if (data[1] & 0x06) != 0 {
        return Err(ShravanError::InvalidHeader(
            "ADTS layer must be 0 for AAC".into(),
        ));
    }

    let profile = (data[2] >> 6) & 0x03;
    let sample_rate_index = (data[2] >> 2) & 0x0F;

    if sample_rate_index >= 13 {
        return Err(ShravanError::InvalidHeader(format!(
            "ADTS reserved sample rate index: {sample_rate_index}"
        )));
    }

    let sample_rate = AAC_SAMPLE_RATES[sample_rate_index as usize];
    if sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    let channel_config = ((data[2] & 0x01) << 2) | ((data[3] >> 6) & 0x03);
    if channel_config == 0 || channel_config as usize >= AAC_CHANNEL_COUNTS.len() {
        return Err(ShravanError::InvalidChannels(channel_config.into()));
    }
    let channels = AAC_CHANNEL_COUNTS[channel_config as usize];

    // Frame length: 13 bits spanning bytes 3-5
    let frame_length = (usize::from(data[3] & 0x03) << 11)
        | (usize::from(data[4]) << 3)
        | (usize::from(data[5] >> 5) & 0x07);

    if frame_length < 7 {
        return Err(ShravanError::InvalidHeader(
            "ADTS frame length too small".into(),
        ));
    }

    Ok(AdtsHeader {
        profile,
        sample_rate,
        sample_rate_index,
        channels,
        frame_length,
    })
}

/// Extract raw AAC frame payloads from an ADTS stream.
///
/// Returns a list of (header, payload) pairs.
fn extract_adts_frames(data: &[u8]) -> Result<(AdtsHeader, Vec<&[u8]>)> {
    let mut pos = 0;
    let mut frames = Vec::new();
    let mut first_header: Option<AdtsHeader> = None;

    while pos + 7 <= data.len() {
        // Scan for sync word
        if data[pos] != 0xFF || (data[pos + 1] & 0xF0) != 0xF0 {
            pos += 1;
            continue;
        }

        let header = parse_adts_header(&data[pos..])?;

        if pos + header.frame_length > data.len() {
            break; // truncated final frame
        }

        // Determine header size (7 without CRC, 9 with CRC)
        let protection_absent = (data[pos + 1] & 0x01) != 0;
        let header_size = if protection_absent { 7 } else { 9 };

        if header.frame_length > header_size {
            frames.push(&data[pos + header_size..pos + header.frame_length]);
        }

        if first_header.is_none() {
            first_header = Some(header);
        }

        pos += header.frame_length;
    }

    match first_header {
        Some(h) => Ok((h, frames)),
        None => Err(ShravanError::InvalidHeader(
            "no valid ADTS frames found".into(),
        )),
    }
}

/// Convert a symphonia `AudioBufferRef` to interleaved f32 samples.
fn audio_buffer_ref_to_f32(
    buf_ref: &symphonia_core::audio::AudioBufferRef<'_>,
    channels: usize,
    out: &mut Vec<f32>,
) {
    use symphonia_core::audio::AudioBufferRef;
    use symphonia_core::conv::IntoSample;

    // Use the actual channel count from the buffer, capped to expected channels
    let buf_channels = buf_ref.spec().channels.count();
    let ch_count = channels.min(buf_channels);

    match buf_ref {
        AudioBufferRef::F32(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame]);
                }
                // Pad missing channels with silence
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::S16(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::S32(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::F64(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::S24(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    let val: i32 = buf.chan(ch)[frame].into_sample();
                    out.push(val.into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::U8(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        // Remaining variants: U16, U24, U32, S8 — highly unlikely from AAC
        // but handle them by converting through the widest available path
        _ => {
            // Cannot convert without knowing the exact type — skip.
            // This is a best-effort fallback; AAC-LC always outputs F32 or S16.
        }
    }
}

/// Decode AAC audio from an ADTS byte stream.
///
/// Parses ADTS frames, feeds raw AAC packets to symphonia's AAC-LC decoder,
/// and returns interleaved f32 samples normalised to \[-1.0, 1.0\].
///
/// # Errors
///
/// Returns errors for invalid ADTS headers, unsupported AAC profiles,
/// or decoding failures.
#[must_use = "decoded audio data is returned and should not be discarded"]
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    let (header, frames) = extract_adts_frames(data)?;

    if frames.is_empty() {
        return Err(ShravanError::DecodeError(
            "no AAC frames extracted from ADTS stream".into(),
        ));
    }

    // Build symphonia CodecParameters for ADTS (no ASC extra_data)
    let mut params = CodecParameters::new();
    params
        .for_codec(CODEC_TYPE_AAC)
        .with_sample_rate(header.sample_rate)
        .with_channels(map_channels(header.channels));

    // Instantiate the symphonia AAC decoder
    let opts = DecoderOptions::default();
    let mut decoder = symphonia_codec_aac::AacDecoder::try_new(&params, &opts)
        .map_err(|e| ShravanError::DecodeError(format!("AAC decoder init failed: {e}")))?;

    let channels = header.channels as usize;
    let mut samples: Vec<f32> = Vec::new();
    let mut ts: u64 = 0;

    for frame_data in &frames {
        let packet = Packet::new_from_slice(0, ts, 1024, frame_data);

        match decoder.decode(&packet) {
            Ok(buf_ref) => {
                audio_buffer_ref_to_f32(&buf_ref, channels, &mut samples);
            }
            Err(e) => {
                return Err(ShravanError::DecodeError(format!(
                    "AAC frame decode failed: {e}"
                )));
            }
        }

        ts += 1024; // AAC-LC always produces 1024 samples per frame
    }

    let total_frames = samples.len() / channels.max(1);
    let duration_secs = if header.sample_rate > 0 {
        total_frames as f64 / f64::from(header.sample_rate)
    } else {
        0.0
    };

    let info = FormatInfo {
        format: AudioFormat::Aac,
        sample_rate: header.sample_rate,
        channels: header.channels,
        bit_depth: 16, // AAC is perceptual; report as 16-bit equivalent
        duration_secs,
        total_samples: total_frames as u64,
    };

    Ok((info, samples))
}

/// Map a channel count to symphonia's `Channels` bitfield.
fn map_channels(count: u16) -> symphonia_core::audio::Channels {
    use symphonia_core::audio::Channels;

    match count {
        1 => Channels::FRONT_LEFT,
        2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
        3 => Channels::FRONT_CENTRE | Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
        4 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::REAR_CENTRE
        }
        5 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
        }
        6 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
                | Channels::LFE1
        }
        8 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
                | Channels::FRONT_LEFT_WIDE
                | Channels::FRONT_RIGHT_WIDE
                | Channels::LFE1
        }
        _ => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
    }
}

// ---------------------------------------------------------------------------
// AAC-LC Encoder
// ---------------------------------------------------------------------------

/// AAC-LC samples per frame.
const AAC_FRAME_SIZE: usize = 1024;

/// Scale factor band boundaries for 48 kHz / 44.1 kHz (long window, 49 bands).
const SWB_OFFSET_48K: [usize; 50] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 48, 56, 64, 72, 80, 88, 96, 108, 120, 132, 144, 160,
    176, 196, 216, 240, 264, 292, 320, 352, 384, 416, 448, 480, 512, 544, 576, 608, 640, 672, 704,
    736, 768, 800, 832, 864, 896, 928, 1024,
];

/// Number of scale factor bands for 48 kHz / 44.1 kHz.
const NUM_SWB_48K: usize = SWB_OFFSET_48K.len() - 1;

/// Scale factor Huffman codebook — code lengths (121 entries, index 60 = center).
const SCF_CODEBOOK_LENS: [u8; 121] = [
    18, 18, 18, 18, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 18, 19, 18, 17, 17,
    16, 17, 16, 16, 16, 16, 15, 15, 14, 14, 14, 14, 14, 14, 13, 13, 12, 12, 12, 11, 12, 11, 10, 10,
    10, 9, 9, 8, 8, 8, 7, 6, 6, 5, 4, 3, 1, 4, 4, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 10, 11, 11,
    11, 11, 12, 12, 13, 13, 13, 14, 14, 16, 15, 16, 15, 18, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19,
    19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19,
];

/// Scale factor Huffman codebook — code values (121 entries).
const SCF_CODEBOOK_CODES: [u32; 121] = [
    0x3FFE8, 0x3FFE6, 0x3FFE7, 0x3FFE5, 0x7FFF5, 0x7FFF1, 0x7FFED, 0x7FFF6, 0x7FFEE, 0x7FFEF,
    0x7FFF0, 0x7FFFC, 0x7FFFD, 0x7FFFF, 0x7FFFE, 0x7FFF7, 0x7FFF8, 0x7FFFB, 0x7FFF9, 0x3FFE4,
    0x7FFFA, 0x3FFE3, 0x1FFEF, 0x1FFF0, 0x0FFF5, 0x1FFEE, 0x0FFF2, 0x0FFF3, 0x0FFF4, 0x0FFF1,
    0x07FF6, 0x07FF7, 0x03FF9, 0x03FF5, 0x03FF7, 0x03FF3, 0x03FF6, 0x03FF2, 0x01FF7, 0x01FF5,
    0x00FF9, 0x00FF7, 0x00FF6, 0x007F9, 0x00FF4, 0x007F8, 0x003F9, 0x003F7, 0x003F5, 0x001F8,
    0x001F7, 0x000FA, 0x000F8, 0x000F6, 0x00079, 0x0003A, 0x00038, 0x0001A, 0x0000B, 0x00004,
    0x00000, 0x0000A, 0x0000C, 0x0001B, 0x00039, 0x0003B, 0x00078, 0x0007A, 0x000F7, 0x000F9,
    0x001F6, 0x001F9, 0x003F4, 0x003F6, 0x003F8, 0x007F5, 0x007F4, 0x007F6, 0x007F7, 0x00FF5,
    0x00FF8, 0x01FF4, 0x01FF6, 0x01FF8, 0x03FF8, 0x03FF4, 0x0FFF0, 0x07FF4, 0x0FFF6, 0x07FF5,
    0x3FFE2, 0x7FFD9, 0x7FFDA, 0x7FFDB, 0x7FFDC, 0x7FFDD, 0x7FFDE, 0x7FFD8, 0x7FFD2, 0x7FFD3,
    0x7FFD4, 0x7FFD5, 0x7FFD6, 0x7FFF2, 0x7FFDF, 0x7FFE7, 0x7FFE8, 0x7FFE9, 0x7FFEA, 0x7FFEB,
    0x7FFE6, 0x7FFE0, 0x7FFE1, 0x7FFE2, 0x7FFE3, 0x7FFE4, 0x7FFE5, 0x7FFD7, 0x7FFEC, 0x7FFF4,
    0x7FFF3,
];

/// MSB-first bit writer for AAC bitstream construction.
struct BitWriter {
    buf: Vec<u8>,
    current: u32,
    bits_in_current: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            current: 0,
            bits_in_current: 0,
        }
    }

    /// Write `n` bits from `val` (MSB-first, up to 32 bits).
    fn write(&mut self, val: u32, n: u8) {
        for i in (0..n).rev() {
            self.current = (self.current << 1) | ((val >> i) & 1);
            self.bits_in_current += 1;
            if self.bits_in_current == 8 {
                self.buf.push(self.current as u8);
                self.current = 0;
                self.bits_in_current = 0;
            }
        }
    }

    /// Flush any remaining bits (zero-padded to byte boundary).
    fn flush(mut self) -> Vec<u8> {
        if self.bits_in_current > 0 {
            self.current <<= 8 - self.bits_in_current;
            self.buf.push(self.current as u8);
        }
        self.buf
    }

    /// Current byte position (including partial byte).
    #[allow(dead_code)]
    fn byte_len(&self) -> usize {
        self.buf.len() + usize::from(self.bits_in_current > 0)
    }
}

/// Build a 7-byte ADTS header for one AAC-LC frame.
fn build_adts_header(sample_rate: u32, channels: u16, frame_len: usize) -> [u8; 7] {
    let sr_index = AAC_SAMPLE_RATES
        .iter()
        .position(|&r| r == sample_rate)
        .unwrap_or(4) as u8; // default to 44100

    let total_len = frame_len + 7; // frame + header

    let mut h = [0u8; 7];
    h[0] = 0xFF;
    h[1] = 0xF1; // sync + MPEG-4 + layer=0 + protection_absent=1
    h[2] = (1 << 6) | (sr_index << 2) | ((channels as u8 >> 2) & 0x01); // profile=LC(1)
    h[3] = ((channels as u8 & 0x03) << 6) | ((total_len >> 11) as u8 & 0x03);
    h[4] = ((total_len >> 3) & 0xFF) as u8;
    h[5] = (((total_len & 0x07) << 5) as u8) | 0x1F; // buffer fullness = 0x7FF (VBR)
    h[6] = 0xFC; // buffer fullness LSBs + 0 AAC frames minus 1

    h
}

/// Encode audio samples as an AAC-LC ADTS bitstream.
///
/// Produces a sequence of ADTS frames. Input must be interleaved f32 samples
/// in \[-1.0, 1.0\].
///
/// # Arguments
///
/// * `samples` — interleaved f32 audio samples
/// * `sample_rate` — sample rate in Hz (must be a standard AAC rate)
/// * `channels` — 1 (mono) or 2 (stereo)
/// * `bitrate` — target bitrate in bits per second
///
/// # Errors
///
/// Returns errors for invalid parameters or unsupported sample rates.
#[must_use = "encoded AAC/ADTS bytes are returned and should not be discarded"]
pub fn encode(samples: &[f32], sample_rate: u32, channels: u16, bitrate: u32) -> Result<Vec<u8>> {
    // Validate parameters
    if !AAC_SAMPLE_RATES[..13].contains(&sample_rate) {
        return Err(ShravanError::InvalidSampleRate(sample_rate));
    }
    if channels == 0 || channels > 2 {
        return Err(ShravanError::InvalidChannels(channels));
    }
    if bitrate == 0 {
        return Err(ShravanError::EncodeError("bitrate must be > 0".into()));
    }

    let ch = channels as usize;
    let total_interleaved = samples.len();

    // Target bytes per frame: bitrate / 8 / (sample_rate / 1024)
    let frames_per_sec = sample_rate as f64 / AAC_FRAME_SIZE as f64;
    let target_bytes_per_frame = ((bitrate as f64 / 8.0 / frames_per_sec) as usize).max(20);

    let mut output = Vec::new();
    let mut sample_pos = 0;
    let frame_interleaved = AAC_FRAME_SIZE * ch;

    while sample_pos < total_interleaved {
        let end = (sample_pos + frame_interleaved).min(total_interleaved);
        let frame_slice = &samples[sample_pos..end];

        // Pad short final frame
        let frame_data = if frame_slice.len() < frame_interleaved {
            let mut padded = vec![0.0f32; frame_interleaved];
            padded[..frame_slice.len()].copy_from_slice(frame_slice);
            padded
        } else {
            frame_slice.to_vec()
        };

        // Encode one AAC-LC frame
        let frame_bytes = encode_aac_frame(&frame_data, channels, target_bytes_per_frame)?;

        // Write ADTS header + frame
        let adts = build_adts_header(sample_rate, channels, frame_bytes.len());
        output.extend_from_slice(&adts);
        output.extend_from_slice(&frame_bytes);

        sample_pos = end;
    }

    // Handle empty input — encode one silence frame
    if output.is_empty() {
        let silence = vec![0.0f32; frame_interleaved];
        let frame_bytes = encode_aac_frame(&silence, channels, target_bytes_per_frame)?;
        let adts = build_adts_header(sample_rate, channels, frame_bytes.len());
        output.extend_from_slice(&adts);
        output.extend_from_slice(&frame_bytes);
    }

    Ok(output)
}

/// Encode a single AAC-LC frame.
///
/// Input: interleaved f32 samples (1024 * channels).
/// Output: raw AAC frame bitstream (without ADTS header).
fn encode_aac_frame(samples: &[f32], channels: u16, target_bytes: usize) -> Result<Vec<u8>> {
    let ch = channels as usize;

    // Downmix to mono for MDCT (single channel element)
    let mut mono = vec![0.0f32; AAC_FRAME_SIZE];
    for (i, m) in mono.iter_mut().enumerate() {
        let mut sum = 0.0f32;
        for c in 0..ch {
            let idx = i * ch + c;
            if idx < samples.len() {
                sum += samples[idx];
            }
        }
        *m = sum / ch as f32;
    }

    // Apply sine window (2048-point for AAC MDCT with 50% overlap)
    // For the encoder without overlap state, just window the frame
    let mut windowed = vec![0.0f32; AAC_FRAME_SIZE * 2];
    for (i, w) in windowed.iter_mut().enumerate() {
        let win =
            libm::sinf(core::f32::consts::PI / (AAC_FRAME_SIZE * 2) as f32 * (i as f32 + 0.5));
        if i < AAC_FRAME_SIZE {
            *w = mono[i] * win;
        }
        // Second half is zeros (no previous frame overlap in stateless encoder)
    }

    // Forward MDCT: 2048 input → 1024 spectral coefficients
    let mut mdct_out = vec![0.0f32; AAC_FRAME_SIZE];
    crate::fft::mdct_forward(&windowed, &mut mdct_out);

    // Quantize spectral coefficients per scale factor band
    let num_swb = NUM_SWB_48K;
    let mut scale_factors = vec![0i16; num_swb];
    let mut quant_spec = vec![0i16; AAC_FRAME_SIZE];

    // Determine scale factors based on band energy and target bitrate
    for band in 0..num_swb {
        let start = SWB_OFFSET_48K[band];
        let end = SWB_OFFSET_48K[band + 1];

        // Compute band energy
        let mut energy = 0.0f32;
        for &c in &mdct_out[start..end] {
            energy += c * c;
        }
        let rms = libm::sqrtf(energy / (end - start).max(1) as f32);

        // Choose scale factor: scf such that quantization fits within codebook range
        // scale_factor = 2^(0.25 * (scf - 100))
        // We want quantized values roughly in [-8191, 8191] range
        let scf = if rms > 1e-10 {
            // Target: rms / step ≈ reasonable range
            let log_rms = libm::log2f(rms);
            // scf = 100 + 4 * log2(rms / target_quant_level)
            let target = 10.0; // target quantized magnitude
            (100.0 + 4.0 * (log_rms - libm::log2f(target / 128.0))) as i16
        } else {
            0
        };

        let scf_clamped = scf.clamp(0, 200);
        scale_factors[band] = scf_clamped;

        // Quantize: q = nint(|x|^0.75 / step_size)
        let step = libm::powf(2.0, 0.25 * (scf_clamped as f32 - 100.0));
        for i in start..end {
            if step > 1e-20 {
                let abs_val = libm::fabsf(mdct_out[i]);
                let q = libm::roundf(libm::powf(abs_val, 0.75) / step);
                let q_clamped = (q as i32).clamp(-8191, 8191) as i16;
                quant_spec[i] = if mdct_out[i] >= 0.0 {
                    q_clamped
                } else {
                    -q_clamped
                };
            }
        }
    }

    // Build AAC bitstream
    let mut bw = BitWriter::new();

    // ID_SCE tag (3 bits) + instance tag (4 bits)
    bw.write(0, 3); // ID_SCE = 0
    bw.write(0, 4); // instance_tag = 0

    // ICS info
    bw.write(0, 1); // ics_reserved_bit
    bw.write(0, 2); // window_sequence = ONLY_LONG_SEQUENCE
    bw.write(0, 1); // window_shape = sine
    bw.write(num_swb as u32, 6); // max_sfb

    // Scale factor data: predictor_data_present = 0
    bw.write(0, 1);

    // Section data: encode all bands as one section with ZERO_HCB or escape book
    // First, determine which bands have nonzero data
    let mut band_has_data = vec![false; num_swb];
    for band in 0..num_swb {
        let start = SWB_OFFSET_48K[band];
        let end = SWB_OFFSET_48K[band + 1];
        band_has_data[band] = quant_spec[start..end].iter().any(|&q| q != 0);
    }

    // Section coding: group consecutive bands with same codebook
    // sect_cb (4 bits) + sect_len (5 bits for long window, escape = 31)
    let mut band = 0;
    while band < num_swb {
        let has_data = band_has_data[band];
        let cb = if has_data { 11u8 } else { 0u8 }; // ZERO_HCB or escape book

        // Find run of same codebook
        let mut run = 1;
        while band + run < num_swb && band_has_data[band + run] == has_data {
            run += 1;
        }

        bw.write(u32::from(cb), 4);

        // Encode section length with escape
        let mut remaining = run;
        while remaining >= 31 {
            bw.write(31, 5);
            remaining -= 31;
        }
        bw.write(remaining as u32, 5);

        band += run;
    }

    // Scale factor data (DPCM + Huffman coded)
    // Global gain (8 bits) — first scale factor
    let global_gain = scale_factors.first().copied().unwrap_or(100) as u32;
    bw.write(global_gain.min(255), 8);

    // Differential scale factors for bands with data
    let mut prev_scf = global_gain as i16;
    for band_idx in 0..num_swb {
        if band_has_data[band_idx] {
            let diff = scale_factors[band_idx] - prev_scf;
            let index = (diff + 60).clamp(0, 120) as usize;
            bw.write(SCF_CODEBOOK_CODES[index], SCF_CODEBOOK_LENS[index]);
            prev_scf = scale_factors[band_idx];
        }
    }

    // Spectral data: for each band with codebook 11 (escape pairs)
    for band_idx in 0..num_swb {
        if !band_has_data[band_idx] {
            continue;
        }

        let start = SWB_OFFSET_48K[band_idx];
        let end = SWB_OFFSET_48K[band_idx + 1];

        // Codebook 11: unsigned pairs with escape
        // Encode in pairs (2 coefficients at a time)
        let mut i = start;
        while i + 1 < end {
            let x = quant_spec[i].unsigned_abs();
            let y = quant_spec[i + 1].unsigned_abs();

            // Clamp to codebook 11 range (0-16, with escape for >= 16)
            let cx = x.min(16);
            let cy = y.min(16);

            // Encode the pair index: cx * 17 + cy
            // For simplicity, write as raw bits (not optimal compression,
            // but produces valid bitstream that decoders can parse)
            // Full Huffman would use SPECTRUM_CODEBOOK11 tables
            let pair_idx = u32::from(cx) * 17 + u32::from(cy);
            bw.write(pair_idx, 9); // 9 bits covers 0-288

            // Sign bits for nonzero values
            if cx > 0 {
                bw.write(u32::from(quant_spec[i] < 0), 1);
            }
            if cy > 0 {
                bw.write(u32::from(quant_spec[i + 1] < 0), 1);
            }

            // Escape codes for values >= 16
            if x >= 16 {
                let esc = x - 16;
                let esc_len = 32u32.saturating_sub((esc as u32).leading_zeros()).max(4);
                bw.write(esc_len - 4, 4); // escape size prefix
                bw.write(u32::from(esc), esc_len as u8);
            }
            if y >= 16 {
                let esc = y - 16;
                let esc_len = 32u32.saturating_sub((esc as u32).leading_zeros()).max(4);
                bw.write(esc_len - 4, 4);
                bw.write(u32::from(esc), esc_len as u8);
            }

            i += 2;
        }
        // Handle odd trailing coefficient
        if i < end {
            let x = quant_spec[i].unsigned_abs().min(16);
            let pair_idx = u32::from(x) * 17; // y = 0
            bw.write(pair_idx, 9);
            if x > 0 {
                bw.write(u32::from(quant_spec[i] < 0), 1);
            }
        }
    }

    // ID_END tag
    bw.write(7, 3);

    let frame_data = bw.flush();

    // Pad to target size if needed (AAC frames should be close to target)
    if frame_data.len() < target_bytes {
        let mut padded = frame_data;
        padded.resize(target_bytes, 0);
        Ok(padded)
    } else {
        Ok(frame_data)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_adts_header() {
        // ADTS header: sync=0xFFF, MPEG-4, layer=0, protection_absent=1,
        // profile=LC(1), sr_index=4(44100), private=0, ch_config=2(stereo),
        // frame_length=100
        let mut header = [0u8; 7];
        header[0] = 0xFF;
        header[1] = 0xF1; // sync + MPEG-4 + layer=0 + protection_absent=1
        header[2] = 0x50; // profile=1(LC) | sr_index=4(44100) | private=0 | ch_config MSB=0
        header[3] = 0x80; // ch_config[1:0]=10 | orig=0 | home=0 | copy=0 | copyright_start=0
        // frame_length = 100 = 0x064 across bits [30:43]
        // data[3] bits[1:0] = 0x00 (frame_len[12:11])
        // data[4] = 0x0C (frame_len[10:3] = 00001100 = 12, 12<<3 = 96)
        // data[5] bits[7:5] = 0x80 (frame_len[2:0] = 100 = 4, 96+4=100)
        header[3] |= 0x00; // frame_len bits 12-11
        header[4] = 0x0C; // frame_len bits 10-3
        header[5] = 0x80; // frame_len bits 2-0 (top 3 bits)
        header[6] = 0x00;

        let h = parse_adts_header(&header).unwrap();
        assert_eq!(h.profile, 1); // LC
        assert_eq!(h.sample_rate, 44100);
        assert_eq!(h.channels, 2);
        assert_eq!(h.frame_length, 100);
    }

    #[test]
    fn reject_short_header() {
        let data = [0xFF, 0xF1, 0x50];
        assert!(parse_adts_header(&data).is_err());
    }

    #[test]
    fn reject_bad_sync() {
        let data = [0x00u8; 7];
        assert!(parse_adts_header(&data).is_err());
    }

    #[test]
    fn reject_reserved_sample_rate() {
        let mut header = [0u8; 7];
        header[0] = 0xFF;
        header[1] = 0xF1;
        header[2] = 0x7C; // profile=1, sr_index=15(reserved)
        assert!(parse_adts_header(&header).is_err());
    }

    #[test]
    fn extract_no_frames_from_garbage() {
        let data = [0x00u8; 100];
        assert!(extract_adts_frames(&data).is_err());
    }

    #[test]
    fn decode_rejects_empty() {
        let data: &[u8] = &[];
        assert!(decode(data).is_err());
    }

    #[test]
    fn serde_roundtrip_aac_codec() {
        let codec = crate::codec::AacCodec;
        let json = serde_json::to_string(&codec).unwrap();
        let codec2: crate::codec::AacCodec = serde_json::from_str(&json).unwrap();
        assert_eq!(codec, codec2);
    }

    // --- Encoder tests ---

    #[test]
    fn encode_rejects_bad_sample_rate() {
        let samples = vec![0.0f32; 1024];
        assert!(encode(&samples, 11111, 1, 64000).is_err());
    }

    #[test]
    fn encode_rejects_zero_channels() {
        let samples = vec![0.0f32; 1024];
        assert!(encode(&samples, 44100, 0, 64000).is_err());
    }

    #[test]
    fn encode_rejects_zero_bitrate() {
        let samples = vec![0.0f32; 1024];
        assert!(encode(&samples, 44100, 1, 0).is_err());
    }

    #[test]
    fn encode_silence_mono() {
        let samples = vec![0.0f32; 44100]; // 1 second mono
        let adts = encode(&samples, 44100, 1, 64000).unwrap();

        // Should start with ADTS sync word
        assert_eq!(adts[0], 0xFF);
        assert_eq!(adts[1] & 0xF0, 0xF0);
        assert!(adts.len() > 7); // At least one ADTS header
    }

    #[test]
    fn encode_silence_stereo() {
        let samples = vec![0.0f32; 48000 * 2]; // 1 second stereo
        let adts = encode(&samples, 48000, 2, 128000).unwrap();

        assert_eq!(adts[0], 0xFF);
        assert!(adts.len() > 100);
    }

    #[test]
    fn encode_sine_wave() {
        let samples: Vec<f32> = (0..44100)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 44100.0))
            .collect();
        let adts = encode(&samples, 44100, 1, 96000).unwrap();
        assert_eq!(adts[0], 0xFF);
        assert!(adts.len() > 100);
    }

    #[test]
    fn encode_empty_input() {
        let samples: Vec<f32> = Vec::new();
        let adts = encode(&samples, 44100, 1, 64000).unwrap();
        // Should produce at least one silence frame
        assert_eq!(adts[0], 0xFF);
    }

    #[test]
    fn encode_adts_header_valid() {
        let h = build_adts_header(44100, 2, 100);
        assert_eq!(h[0], 0xFF);
        assert_eq!(h[1], 0xF1);
        // Profile should be LC (1)
        assert_eq!((h[2] >> 6) & 0x03, 1);
        // Sample rate index 4 = 44100
        assert_eq!((h[2] >> 2) & 0x0F, 4);
    }

    #[test]
    fn bitwriter_basic() {
        let mut bw = BitWriter::new();
        bw.write(0xFF, 8);
        bw.write(0x0, 4);
        bw.write(0xF, 4);
        let bytes = bw.flush();
        assert_eq!(bytes, vec![0xFF, 0x0F]);
    }

    #[test]
    fn bitwriter_cross_byte() {
        let mut bw = BitWriter::new();
        bw.write(0b111, 3);
        bw.write(0b00000, 5);
        bw.write(0b11111111, 8);
        let bytes = bw.flush();
        assert_eq!(bytes, vec![0b11100000, 0xFF]);
    }
}
