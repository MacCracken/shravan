//! FLAC (Free Lossless Audio Codec) decoder.
//!
//! Implements decoding of FLAC bitstreams including:
//! - STREAMINFO metadata block parsing
//! - Frame sync and header parsing
//! - Subframe types: Constant, Verbatim, Fixed (orders 0-4)
//! - Rice entropy coding for residuals
//! - Channel decorrelation (independent, left-side, right-side, mid-side)

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

/// FLAC metadata block types.
const STREAMINFO: u8 = 0;

/// FLAC frame channel assignment codes.
const _CHANNEL_INDEPENDENT: u8 = 0; // 0..=7 actually, but we check < 8
const CHANNEL_LEFT_SIDE: u8 = 8;
const CHANNEL_RIGHT_SIDE: u8 = 9;
const CHANNEL_MID_SIDE: u8 = 10;

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
    // Verify magic
    if data.len() < 4 || &data[0..4] != b"fLaC" {
        return Err(ShravanError::InvalidHeader("missing fLaC magic".into()));
    }

    // Parse metadata blocks
    let mut pos = 4;
    let mut stream_info: Option<StreamInfo> = None;

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
        }

        pos += block_size as usize;

        if is_last {
            break;
        }
    }

    let info = stream_info
        .ok_or_else(|| ShravanError::InvalidHeader("missing STREAMINFO block".into()))?;

    if info.sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    let bps = info.bits_per_sample;
    let channels = info.channels;
    let scale = 1.0f64 / f64::from(1u32 << (bps - 1));

    // Decode frames
    let mut all_samples: Vec<f32> = Vec::new();
    let mut reader = BitReader::new(data, pos);

    loop {
        // Find frame sync: 0xFFF8 or 0xFFF9
        let sync_result = find_frame_sync(&mut reader);
        let sync_code = match sync_result {
            Ok(code) => code,
            Err(_) => break, // End of data
        };

        let _blocking_strategy = sync_code & 1; // 0 = fixed, 1 = variable

        // Frame header after sync
        let block_size_code = reader.read_bits(4)? as u8;
        let sample_rate_code = reader.read_bits(4)? as u8;
        let channel_assignment = reader.read_bits(4)? as u8;
        let sample_size_code = reader.read_bits(3)? as u8;
        let _reserved = reader.read_bits(1)?;

        // Frame/sample number (UTF-8 coded)
        let _frame_or_sample = reader.read_utf8_u64()?;

        // Decode block size
        let block_size = decode_block_size(block_size_code, &mut reader)?;

        // Decode sample rate (use STREAMINFO if code indicates that)
        let _frame_sr = decode_sample_rate(sample_rate_code, &mut reader, info.sample_rate)?;

        // Decode bits per sample
        let frame_bps = decode_bps(sample_size_code, bps)?;

        // CRC-8 of header (skip the byte)
        reader.align_to_byte();
        let _crc8 = reader.read_bits(8)?;

        // Determine channel count and decorrelation mode
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

        // Decode subframes
        let mut channel_data: Vec<Vec<i64>> = Vec::with_capacity(ch_count as usize);
        for ch in 0..ch_count {
            // For side-channel stereo, the side channel gets +1 bit
            let effective_bps = match decorrelation {
                ChannelDecorrelation::LeftSide if ch == 1 => frame_bps + 1,
                ChannelDecorrelation::RightSide if ch == 0 => frame_bps + 1,
                ChannelDecorrelation::MidSide if ch == 1 => frame_bps + 1,
                _ => frame_bps,
            };
            let subframe = decode_subframe(&mut reader, block_size as usize, effective_bps)?;
            channel_data.push(subframe);
        }

        // Apply channel decorrelation
        apply_decorrelation(&mut channel_data, decorrelation);

        // Interleave and convert to f32
        let frame_scale = if frame_bps != bps {
            1.0f64 / f64::from(1u32 << (frame_bps - 1))
        } else {
            scale
        };
        for i in 0..block_size as usize {
            for ch_samples in &channel_data {
                if i < ch_samples.len() {
                    all_samples.push((ch_samples[i] as f64 * frame_scale) as f32);
                }
            }
        }

        // Align to byte boundary and skip CRC-16
        reader.align_to_byte();
        if reader.byte_pos + 2 <= reader.data.len() {
            let _crc16 = reader.read_bits(16)?;
        }
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
fn find_frame_sync(reader: &mut BitReader<'_>) -> Result<u16> {
    reader.align_to_byte();
    // Look for 0xFF followed by 0xF8 or 0xF9
    while reader.byte_pos + 1 < reader.data.len() {
        if reader.data[reader.byte_pos] == 0xFF {
            let next = reader.data[reader.byte_pos + 1];
            if next == 0xF8 || next == 0xF9 {
                let code = u16::from(reader.data[reader.byte_pos]) << 8 | u16::from(next);
                reader.byte_pos += 2;
                reader.bit_pos = 0;
                return Ok(code);
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
            return Err(ShravanError::DecodeError(
                "LPC subframes not yet implemented".into(),
            ));
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
        // Sample rate (20 bits) | channels-1 (3 bits) | bps-1 (5 bits) | total samples (36 bits)
        // For mono, 16-bit, sample_rate, block_size samples:
        // channels-1 = 0, bps-1 = 15
        let sr_hi = (sample_rate >> 12) as u8;
        let sr_mid = ((sample_rate >> 4) & 0xFF) as u8;
        let sr_lo_and_ch_bps = ((sample_rate & 0x0F) << 4) as u8 | (0 << 1) | ((15 >> 4) & 1);
        let bps_lo_and_total_hi =
            ((15 & 0x0F) << 4) as u8 | ((block_size as u64 >> 32) & 0x0F) as u8;
        out.push(sr_hi);
        out.push(sr_mid);
        out.push(sr_lo_and_ch_bps);
        out.push(bps_lo_and_total_hi);
        out.extend_from_slice(&(block_size as u32).to_be_bytes()); // total samples low 32 bits
        out.extend_from_slice(&[0u8; 16]); // MD5 (all zeros)

        // Build a frame with CONSTANT subframe
        // Frame header sync: 0xFFF8 (fixed block size)
        out.push(0xFF);
        out.push(0xF8);

        // Block size code + sample rate code (4+4 bits)
        // For block_size, we need to pick a code. Use code 6 (8-bit value, block_size-1)
        // or code 7 (16-bit). Let's use explicit 16-bit (code 7) if > 256, else code 6.
        let bs_code: u8;
        let sr_code: u8 = 9; // 44100
        if block_size <= 256 {
            bs_code = 6; // 8-bit follows
        } else {
            bs_code = 7; // 16-bit follows
        }
        out.push((bs_code << 4) | sr_code);

        // Channel assignment (4 bits) + sample size (3 bits) + reserved (1 bit)
        // Channel 0 = mono independent, sample size code 4 = 16-bit
        out.push((0 << 4) | (4 << 1) | 0);

        // Frame number (UTF-8 coded) — frame 0
        out.push(0x00);

        // Explicit block size (depends on bs_code)
        if bs_code == 6 {
            out.push((block_size - 1) as u8);
        } else {
            out.extend_from_slice(&(block_size - 1).to_be_bytes());
        }

        // CRC-8 placeholder (we write 0 — decoder reads but doesn't verify in our impl)
        out.push(0x00);

        // Subframe: CONSTANT
        // Header: 0 (zero bit) + 000000 (type=constant) + 0 (no wasted bits) = 0x00
        // Then 16-bit sample value
        let mut subframe_bits: Vec<u8> = Vec::new();

        // We need to write bit-aligned data.
        // Subframe header: 1 bit (0) + 6 bits (000000) + 1 bit (0) = 8 bits = 0x00
        subframe_bits.push(0x00);

        // 16-bit constant value (big-endian in bitstream)
        subframe_bits.extend_from_slice(&value.to_be_bytes());

        out.extend_from_slice(&subframe_bits);

        // Align + CRC-16 placeholder
        out.push(0x00);
        out.push(0x00);

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
}
