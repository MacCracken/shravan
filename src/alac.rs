//! Apple Lossless Audio Codec (ALAC) decoder.
//!
//! Decodes raw ALAC frames as extracted from an MP4/M4A container.
//! Supports 16, 20, 24, and 32-bit depths, mono and stereo.
//!
//! The caller (e.g. tarang) is responsible for MP4 demuxing and providing
//! the `ALACSpecificConfig` (magic cookie) and raw frame packets.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default Rice history multiplier.
#[cfg(test)]
const PB0: u32 = 40;
/// Default Rice initial history.
const MB0: u32 = 10;
/// Default Rice history limit (max k).
#[cfg(test)]
const KB0: u32 = 14;
/// Rice quantization shift.
const QBSHIFT: u32 = 9;
/// Max prefix length before escape in Rice coding.
const MAX_PREFIX_32: u32 = 9;
/// Max supported LPC order.
const MAX_COEFS: usize = 32;

/// ALAC element type tags.
const ID_SCE: u8 = 0; // Single Channel Element
const ID_CPE: u8 = 1; // Channel Pair Element
const ID_LFE: u8 = 3; // Low Frequency Effects
const ID_END: u8 = 7; // End of frame

// ---------------------------------------------------------------------------
// ALACSpecificConfig (magic cookie)
// ---------------------------------------------------------------------------

/// ALAC decoder configuration parsed from the MP4 `alac` atom extradata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlacConfig {
    /// Samples per frame (typically 4096).
    pub frame_length: u32,
    /// PCM bit depth (16, 20, 24, or 32).
    pub bit_depth: u8,
    /// Rice parameter pb.
    pub pb: u8,
    /// Rice parameter mb.
    pub mb: u8,
    /// Rice parameter kb.
    pub kb: u8,
    /// Number of audio channels.
    pub num_channels: u8,
    /// Max run length (typically 255).
    pub max_run: u16,
    /// Maximum frame size in bytes.
    pub max_frame_bytes: u32,
    /// Average bitrate (informational).
    pub avg_bit_rate: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
}

/// Parse an `ALACSpecificConfig` from 24 bytes of extradata.
///
/// # Errors
///
/// Returns errors for truncated or invalid config data.
#[must_use = "parsed ALAC config should not be discarded"]
pub fn parse_config(data: &[u8]) -> Result<AlacConfig> {
    if data.len() < 24 {
        return Err(ShravanError::InvalidHeader(
            "ALACSpecificConfig requires 24 bytes".into(),
        ));
    }

    let frame_length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let compatible_version = data[4];
    if compatible_version != 0 {
        return Err(ShravanError::InvalidHeader(format!(
            "unsupported ALAC version: {compatible_version}"
        )));
    }

    let bit_depth = data[5];
    if !matches!(bit_depth, 16 | 20 | 24 | 32) {
        return Err(ShravanError::InvalidHeader(format!(
            "unsupported ALAC bit depth: {bit_depth}"
        )));
    }

    let pb = data[6];
    let mb = data[7];
    let kb = data[8];
    let num_channels = data[9];
    if num_channels == 0 || num_channels > 8 {
        return Err(ShravanError::InvalidChannels(u16::from(num_channels)));
    }

    let max_run = u16::from_be_bytes([data[10], data[11]]);
    let max_frame_bytes = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    let avg_bit_rate = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let sample_rate = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);

    if sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    Ok(AlacConfig {
        frame_length,
        bit_depth,
        pb,
        mb,
        kb,
        num_channels,
        max_run,
        max_frame_bytes,
        avg_bit_rate,
        sample_rate,
    })
}

// ---------------------------------------------------------------------------
// Bit reader
// ---------------------------------------------------------------------------

/// MSB-first bit reader for ALAC bitstream parsing.
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0-7, bits consumed in current byte (from MSB)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Read up to 32 bits.
    fn read(&mut self, bits: u32) -> Result<u32> {
        if bits == 0 {
            return Ok(0);
        }
        let mut result = 0u32;
        let mut remaining = bits;

        while remaining > 0 {
            if self.byte_pos >= self.data.len() {
                return Err(ShravanError::EndOfStream);
            }

            let avail = 8 - self.bit_pos as u32;
            let take = remaining.min(avail);
            let shift = avail - take;
            let mask = ((1u32 << take) - 1) << shift;
            let val = (u32::from(self.data[self.byte_pos]) & mask) >> shift;

            result = (result << take) | val;
            remaining -= take;
            self.bit_pos += take as u8;

            if self.bit_pos >= 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }

        Ok(result)
    }

    /// Read a single bit.
    fn read_bit(&mut self) -> Result<bool> {
        Ok(self.read(1)? != 0)
    }

    /// Advance to the next byte boundary.
    #[allow(dead_code)]
    fn align(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Rice-Golomb decoder (adaptive)
// ---------------------------------------------------------------------------

/// Decode a single Rice-coded value.
fn rice_decode(br: &mut BitReader<'_>, m: u32, k: u32, max_bits: u32) -> Result<u32> {
    // Count leading 1-bits (prefix)
    let mut prefix: u32 = 0;
    while prefix < MAX_PREFIX_32 {
        if !br.read_bit()? {
            break;
        }
        prefix += 1;
    }

    if prefix >= MAX_PREFIX_32 {
        // Escape code: read full value
        let val = br.read(max_bits)?;
        return Ok(val);
    }

    // Read k-bit suffix
    let suffix = if k > 0 { br.read(k)? } else { 0 };
    let val = prefix * m + suffix;

    Ok(val)
}

/// Floor of log2(x + 3) — used for adaptive Rice parameter.
#[inline]
fn lg3(x: u32) -> u32 {
    31u32.saturating_sub((x + 3).leading_zeros())
}

/// Decode a block of Rice-coded residuals with adaptive parameter.
fn rice_decode_block(
    br: &mut BitReader<'_>,
    output: &mut [i32],
    num_samples: usize,
    max_bits: u32,
    pb: u32,
    kb: u32,
) -> Result<()> {
    let mut history: u32 = MB0;
    let mut sign_modifier: u32 = 0;

    let mut i = 0;
    while i < num_samples {
        let k = lg3(history).min(kb);
        let m = (1u32 << k) - 1;

        let raw = rice_decode(br, m, k, max_bits)?.wrapping_add(sign_modifier);

        // Unsigned to signed: map 0,1,2,3,4,... to 0,-1,1,-2,2,...
        let signed = if raw & 1 != 0 {
            -((raw as i32 + 1) >> 1)
        } else {
            (raw >> 1) as i32
        };

        output[i] = signed;
        sign_modifier = 0;

        // Update history
        if raw > 0xFFFF {
            history = 0xFFFF;
        } else {
            history = history
                .wrapping_add(raw.wrapping_mul(pb))
                .wrapping_sub(history.wrapping_mul(pb) >> QBSHIFT);
        }

        // Zero-run detection: if history is very small, check for run of zeros
        if history < 128 && (i + 1) < num_samples {
            let k_run = lg3(history).min(kb);
            let m_run = (1u32 << k_run) - 1;
            let run_len = rice_decode(br, m_run, k_run, 16)?;
            let run = run_len.min((num_samples - i - 1) as u32);

            for _ in 0..run {
                i += 1;
                if i < num_samples {
                    output[i] = 0;
                }
            }

            history = 0;
            sign_modifier = 1;
        }

        i += 1;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// LPC prediction (unfilter)
// ---------------------------------------------------------------------------

/// Apply inverse LPC prediction to reconstruct samples from residuals.
fn unfilter(
    output: &mut [i32],
    num_samples: usize,
    coefs: &[i16],
    num_coefs: usize,
    den_shift: u32,
) {
    if num_coefs == 0 || num_samples == 0 {
        return;
    }

    // First sample is passed through
    // For samples 1..=num_coefs, use simple first-order prediction (cumulative sum)
    for j in 1..=num_coefs.min(num_samples.saturating_sub(1)) {
        output[j] = output[j].wrapping_add(output[j - 1]);
    }

    if num_coefs + 1 >= num_samples {
        return;
    }

    // Main prediction loop (starts at num_coefs + 1 because we need output[j-num_coefs-1])
    for j in (num_coefs + 1)..num_samples {
        // Compute prediction: Σ coefs[i] * (output[j-1-i] - output[j-num_coefs-1])
        let mut prediction: i64 = 0;
        let base = i64::from(output[j - num_coefs - 1]);
        for (c, &coef) in coefs.iter().take(num_coefs).enumerate() {
            let diff = i64::from(output[j - 1 - c]) - base;
            prediction += diff * i64::from(coef);
        }

        // Apply denominator shift and add to residual + base
        let pred_shifted = (prediction >> den_shift) as i32;
        output[j] = output[j]
            .wrapping_add(pred_shifted)
            .wrapping_add(output[j - 1]);
    }
}

// ---------------------------------------------------------------------------
// Channel demixing
// ---------------------------------------------------------------------------

/// Apply stereo de-matrixing (unmix).
fn unmix_stereo(
    u: &mut [i32],
    v: &[i32],
    out_left: &mut [i32],
    out_right: &mut [i32],
    num_samples: usize,
    mix_bits: u32,
    mix_res: i32,
) {
    for i in 0..num_samples {
        if mix_res != 0 {
            let left = u[i]
                .wrapping_add(v[i])
                .wrapping_sub(((mix_res as i64 * i64::from(v[i])) >> mix_bits) as i32);
            let right = left.wrapping_sub(v[i]);
            out_left[i] = left;
            out_right[i] = right;
        } else {
            out_left[i] = u[i];
            out_right[i] = v[i];
        }
    }
}

// ---------------------------------------------------------------------------
// Frame decoder
// ---------------------------------------------------------------------------

/// Decode a single raw ALAC frame into interleaved f32 samples.
///
/// `config` is the parsed `ALACSpecificConfig` from the MP4 extradata.
/// `frame_data` is the raw compressed frame bytes.
///
/// Returns interleaved f32 samples in \[-1.0, 1.0\].
///
/// # Errors
///
/// Returns errors for invalid frame headers, unsupported element types,
/// or truncated data.
#[must_use = "decoded audio data is returned and should not be discarded"]
pub fn decode_frame(config: &AlacConfig, frame_data: &[u8]) -> Result<Vec<f32>> {
    let mut br = BitReader::new(frame_data);
    let mut all_samples: Vec<Vec<i32>> = Vec::new();
    let bit_depth = config.bit_depth;
    let max_bits = u32::from(bit_depth);

    loop {
        let tag = br.read(3)? as u8;

        if tag == ID_END {
            break;
        }

        let _instance_tag = br.read(4)?;

        match tag {
            ID_SCE | ID_LFE => {
                let samples = decode_element(&mut br, config, 1, max_bits)?;
                all_samples.push(samples);
            }
            ID_CPE => {
                let samples = decode_element(&mut br, config, 2, max_bits)?;
                // Deinterleave into two channels
                let num_frames = samples.len() / 2;
                let mut left = vec![0i32; num_frames];
                let mut right = vec![0i32; num_frames];
                for i in 0..num_frames {
                    left[i] = samples[i * 2];
                    right[i] = samples[i * 2 + 1];
                }
                all_samples.push(left);
                all_samples.push(right);
            }
            _ => {
                return Err(ShravanError::DecodeError(format!(
                    "unsupported ALAC element tag: {tag}"
                )));
            }
        }
    }

    if all_samples.is_empty() {
        return Ok(Vec::new());
    }

    // Convert to interleaved f32
    let num_channels = all_samples.len();
    let num_frames = all_samples[0].len();
    let scale = (1i64 << (bit_depth - 1)) as f64;
    let mut output = Vec::with_capacity(num_frames * num_channels);

    for frame in 0..num_frames {
        for ch in &all_samples {
            if frame < ch.len() {
                output.push((f64::from(ch[frame]) / scale) as f32);
            } else {
                output.push(0.0);
            }
        }
    }

    Ok(output)
}

/// Decode a single element (mono or stereo).
fn decode_element(
    br: &mut BitReader<'_>,
    config: &AlacConfig,
    channels: usize,
    max_bits: u32,
) -> Result<Vec<i32>> {
    // Unused header (12 bits)
    let _unused = br.read(12)?;

    // Per-element header
    let partial_frame = br.read_bit()?;
    let bytes_shifted = br.read(2)? as u8;
    let escape_flag = br.read_bit()?;

    let num_samples = if partial_frame {
        let hi = br.read(16)?;
        let lo = br.read(16)?;
        ((hi << 16) | lo) as usize
    } else {
        config.frame_length as usize
    };

    if num_samples == 0 {
        return Ok(Vec::new());
    }

    let shifted_bits = u32::from(bytes_shifted) * 8;
    let effective_bits = max_bits - shifted_bits;

    if escape_flag {
        // Uncompressed: read raw samples directly
        let total = num_samples * channels;
        let mut samples = Vec::with_capacity(total);
        for _ in 0..total {
            let raw = br.read(max_bits)?;
            // Sign-extend
            let signed = sign_extend(raw, max_bits);
            samples.push(signed);
        }
        return Ok(samples);
    }

    // Compressed frame
    let mix_bits = br.read(8)?;
    let mix_res = br.read(8)? as i8 as i32;

    // Read prediction parameters for each channel
    let mut pred_modes = vec![0u32; channels];
    let mut den_shifts = vec![0u32; channels];
    let mut pb_factors = vec![0u32; channels];
    let mut num_coefs_arr = vec![0usize; channels];
    let mut coefs_arr: Vec<Vec<i16>> = Vec::with_capacity(channels);

    for ch in 0..channels {
        pred_modes[ch] = br.read(4)?;
        den_shifts[ch] = br.read(4)?;
        pb_factors[ch] = br.read(3)?;
        let nc = br.read(5)? as usize;
        num_coefs_arr[ch] = nc;

        let mut coefs = vec![0i16; nc.min(MAX_COEFS)];
        for c in &mut coefs {
            *c = br.read(16)? as i16;
        }
        coefs_arr.push(coefs);
    }

    // Decode residuals for each channel
    let pb = u32::from(config.pb);
    let kb = u32::from(config.kb);

    let mut channel_bufs: Vec<Vec<i32>> = Vec::with_capacity(channels);
    for &pb_factor in pb_factors.iter().take(channels) {
        let adj_pb = pb.wrapping_mul(pb_factor) >> 2;
        let mut residuals = vec![0i32; num_samples];
        rice_decode_block(br, &mut residuals, num_samples, effective_bits, adj_pb, kb)?;
        channel_bufs.push(residuals);
    }

    // Read shifted bytes if present
    let mut shift_bufs: Vec<Vec<u32>> = Vec::new();
    if bytes_shifted > 0 {
        for _ch in 0..channels {
            let mut shifts = Vec::with_capacity(num_samples);
            for _ in 0..num_samples {
                shifts.push(br.read(shifted_bits)?);
            }
            shift_bufs.push(shifts);
        }
    }

    // Apply inverse LPC prediction (unfilter) per channel
    for ch in 0..channels {
        if pred_modes[ch] == 0 {
            unfilter(
                &mut channel_bufs[ch],
                num_samples,
                &coefs_arr[ch],
                num_coefs_arr[ch],
                den_shifts[ch],
            );
        }
        // mode 31 = passthrough (no filtering needed)
    }

    // Stereo de-matrixing
    if channels == 2 && mix_res != 0 {
        let (left_buf, right_buf) = channel_bufs.split_at_mut(1);
        let mut out_left = vec![0i32; num_samples];
        let mut out_right = vec![0i32; num_samples];
        unmix_stereo(
            &mut left_buf[0],
            &right_buf[0],
            &mut out_left,
            &mut out_right,
            num_samples,
            mix_bits,
            mix_res,
        );
        channel_bufs[0] = out_left;
        channel_bufs[1] = out_right;
    }

    // Reconstruct shifted samples
    if bytes_shifted > 0 {
        for (ch, buf) in channel_bufs.iter_mut().enumerate() {
            if ch < shift_bufs.len() {
                for (i, sample) in buf.iter_mut().enumerate() {
                    if i < shift_bufs[ch].len() {
                        *sample = (*sample << shifted_bits) | shift_bufs[ch][i] as i32;
                    }
                }
            }
        }
    }

    // Interleave channels
    let mut output = Vec::with_capacity(num_samples * channels);
    for i in 0..num_samples {
        for ch in &channel_bufs {
            output.push(ch[i]);
        }
    }

    Ok(output)
}

/// Sign-extend a value from `bits` width to i32.
#[inline]
fn sign_extend(val: u32, bits: u32) -> i32 {
    if bits >= 32 {
        return val as i32;
    }
    let shift = 32 - bits;
    ((val << shift) as i32) >> shift
}

// ---------------------------------------------------------------------------
// High-level decode API
// ---------------------------------------------------------------------------

/// Decode raw ALAC frame data with a prepended config.
///
/// Expected format: 24-byte `ALACSpecificConfig` followed by raw frame bytes.
/// This is a convenience for callers that bundle config + frame together.
/// For production use with MP4, prefer [`parse_config`] + [`decode_frame`].
///
/// # Errors
///
/// Returns errors for invalid config or frame data.
#[must_use = "decoded audio data is returned and should not be discarded"]
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    if data.len() < 24 {
        return Err(ShravanError::InvalidHeader(
            "ALAC data too short for config".into(),
        ));
    }

    let config = parse_config(&data[..24])?;
    let samples = decode_frame(&config, &data[24..])?;

    let num_channels = config.num_channels as usize;
    let total_frames = if num_channels > 0 {
        samples.len() / num_channels
    } else {
        0
    };
    let duration_secs = if config.sample_rate > 0 {
        total_frames as f64 / f64::from(config.sample_rate)
    } else {
        0.0
    };

    let info = FormatInfo {
        format: AudioFormat::Alac,
        sample_rate: config.sample_rate,
        channels: u16::from(config.num_channels),
        bit_depth: u16::from(config.bit_depth),
        duration_secs,
        total_samples: total_frames as u64,
    };

    Ok((info, samples))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn make_config(frame_length: u32, bit_depth: u8, channels: u8, sample_rate: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity(24);
        buf.extend_from_slice(&frame_length.to_be_bytes());
        buf.push(0); // compatible version
        buf.push(bit_depth);
        buf.push(PB0 as u8); // pb
        buf.push(MB0 as u8); // mb
        buf.push(KB0 as u8); // kb
        buf.push(channels);
        buf.extend_from_slice(&255u16.to_be_bytes()); // max_run
        buf.extend_from_slice(&0u32.to_be_bytes()); // max_frame_bytes
        buf.extend_from_slice(&0u32.to_be_bytes()); // avg_bit_rate
        buf.extend_from_slice(&sample_rate.to_be_bytes());
        buf
    }

    #[test]
    fn parse_config_valid() {
        let data = make_config(4096, 16, 2, 44100);
        let config = parse_config(&data).unwrap();

        assert_eq!(config.frame_length, 4096);
        assert_eq!(config.bit_depth, 16);
        assert_eq!(config.num_channels, 2);
        assert_eq!(config.sample_rate, 44100);
        assert_eq!(config.pb, PB0 as u8);
        assert_eq!(config.mb, MB0 as u8);
        assert_eq!(config.kb, KB0 as u8);
    }

    #[test]
    fn parse_config_rejects_short() {
        let data = [0u8; 10];
        assert!(parse_config(&data).is_err());
    }

    #[test]
    fn parse_config_rejects_bad_version() {
        let mut data = make_config(4096, 16, 2, 44100);
        data[4] = 1; // unsupported version
        assert!(parse_config(&data).is_err());
    }

    #[test]
    fn parse_config_rejects_zero_channels() {
        let data = make_config(4096, 16, 0, 44100);
        assert!(parse_config(&data).is_err());
    }

    #[test]
    fn parse_config_rejects_zero_rate() {
        let data = make_config(4096, 16, 2, 0);
        assert!(parse_config(&data).is_err());
    }

    #[test]
    fn parse_config_rejects_bad_bit_depth() {
        let data = make_config(4096, 15, 2, 44100);
        assert!(parse_config(&data).is_err());
    }

    #[test]
    fn sign_extend_16bit() {
        assert_eq!(sign_extend(0x7FFF, 16), 32767);
        assert_eq!(sign_extend(0xFFFF, 16), -1);
        assert_eq!(sign_extend(0x8000, 16), -32768);
    }

    #[test]
    fn sign_extend_24bit() {
        assert_eq!(sign_extend(0x7FFFFF, 24), 8_388_607);
        assert_eq!(sign_extend(0xFFFFFF, 24), -1);
        assert_eq!(sign_extend(0x800000, 24), -8_388_608);
    }

    #[test]
    fn bitreader_reads_bits() {
        let data = [0b1010_1100, 0b0011_0000];
        let mut br = BitReader::new(&data);

        assert_eq!(br.read(4).unwrap(), 0b1010);
        assert_eq!(br.read(4).unwrap(), 0b1100);
        assert_eq!(br.read(4).unwrap(), 0b0011);
    }

    #[test]
    fn bitreader_cross_byte() {
        let data = [0xFF, 0x00];
        let mut br = BitReader::new(&data);

        assert_eq!(br.read(12).unwrap(), 0xFF0);
        assert_eq!(br.read(4).unwrap(), 0x0);
    }

    #[test]
    fn bitreader_end_of_stream() {
        let data = [0xFF];
        let mut br = BitReader::new(&data);
        assert!(br.read(8).is_ok());
        assert!(br.read(1).is_err());
    }

    #[test]
    fn lg3_values() {
        assert_eq!(lg3(0), 1); // log2(3) = 1
        assert_eq!(lg3(1), 2); // log2(4) = 2
        assert_eq!(lg3(5), 3); // log2(8) = 3
        assert_eq!(lg3(13), 4); // log2(16) = 4
    }

    #[test]
    fn unfilter_passthrough() {
        // No coefficients = residuals are output directly (with cumulative sum)
        let mut buf = vec![1, 2, 3, 4];
        unfilter(&mut buf, 4, &[], 0, 0);
        assert_eq!(buf, vec![1, 2, 3, 4]);
    }

    #[test]
    fn unfilter_first_order() {
        // 1 coefficient with value 0: first-order prediction
        // j=1 (in warmup): out[1] += out[0] → 5+10 = 15
        // j=2 (main loop): base=out[0]=10, pred=coef[0]*(out[1]-10)=0*(15-10)=0
        //   out[2] = 3 + 0 + out[1] = 3 + 0 + 15 = 18
        // j=3: base=out[1]=15, pred=coef[0]*(out[2]-15)=0
        //   out[3] = -2 + 0 + out[2] = -2 + 18 = 16
        let mut buf = vec![10, 5, 3, -2];
        unfilter(&mut buf, 4, &[0], 1, 0);
        assert_eq!(buf[0], 10);
        assert_eq!(buf[1], 15);
        assert_eq!(buf[2], 18);
        assert_eq!(buf[3], 16);
    }

    #[test]
    fn config_serde_roundtrip() {
        let data = make_config(4096, 16, 2, 44100);
        let config = parse_config(&data).unwrap();
        let json = serde_json::to_string(&config).unwrap();
        let config2: AlacConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, config2);
    }

    #[test]
    fn codec_serde_roundtrip() {
        let codec = crate::codec::AlacCodec;
        let json = serde_json::to_string(&codec).unwrap();
        let codec2: crate::codec::AlacCodec = serde_json::from_str(&json).unwrap();
        assert_eq!(codec, codec2);
    }

    #[test]
    fn decode_rejects_short_data() {
        let data = [0u8; 10];
        assert!(decode(&data).is_err());
    }

    #[test]
    fn decode_verbatim_mono_frame() {
        // Build: config (24 bytes) + frame with ID_SCE, escape=1, 4 samples of 16-bit
        let mut data = make_config(4, 16, 1, 44100);

        // Frame:
        // tag=0 (ID_SCE) 3 bits
        // instance=0 4 bits
        // unused=0 12 bits
        // partial=0 1 bit
        // bytes_shifted=0 2 bits
        // escape=1 1 bit
        // 4 samples * 16 bits = 64 bits
        // ID_END tag = 7 (3 bits)
        // Then byte-align

        // Assemble bits manually
        let mut bits: Vec<bool> = Vec::new();
        // tag = 000
        bits.extend([false, false, false]);
        // instance = 0000
        bits.extend([false, false, false, false]);
        // unused = 000000000000
        bits.extend([false; 12]);
        // partial = 0
        bits.push(false);
        // bytes_shifted = 00
        bits.extend([false, false]);
        // escape = 1
        bits.push(true);

        // 4 samples: 1000, -1000, 500, -500 as i16
        for &val in &[1000i16, -1000, 500, -500] {
            let u = val as u16;
            for bit in (0..16).rev() {
                bits.push((u >> bit) & 1 != 0);
            }
        }

        // ID_END = 111
        bits.extend([true, true, true]);

        // Pack bits into bytes
        let mut frame_bytes = Vec::new();
        for chunk in bits.chunks(8) {
            let mut byte = 0u8;
            for (i, &b) in chunk.iter().enumerate() {
                if b {
                    byte |= 1 << (7 - i);
                }
            }
            frame_bytes.push(byte);
        }

        data.extend_from_slice(&frame_bytes);

        let (info, samples) = decode(&data).unwrap();
        assert_eq!(info.format, AudioFormat::Alac);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bit_depth, 16);
        assert_eq!(samples.len(), 4);

        // Check decoded values (normalized to [-1,1])
        let scale = 32768.0f32;
        assert!((samples[0] - 1000.0 / scale).abs() < 0.001);
        assert!((samples[1] - (-1000.0 / scale)).abs() < 0.001);
        assert!((samples[2] - 500.0 / scale).abs() < 0.001);
        assert!((samples[3] - (-500.0 / scale)).abs() < 0.001);
    }
}
