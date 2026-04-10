//! AIFF (Audio Interchange File Format) encoder and decoder.
//!
//! Supports AIFF and AIFF-C containers with big-endian signed PCM
//! in 8/16/24/32-bit depths. AIFF-C compression types `NONE` and `sowt`
//! (little-endian) are accepted; all others are rejected.

use alloc::format;
use alloc::vec::Vec;

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

/// Convert an 80-bit IEEE 754 extended-precision float to `f64`.
///
/// Used to decode the sample-rate field in the COMM chunk.
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

/// Convert an `f64` value to an 80-bit IEEE 754 extended-precision float.
///
/// Used to encode the sample-rate field in the COMM chunk.
#[inline]
#[must_use]
fn f64_to_extended(val: f64) -> [u8; 10] {
    if val == 0.0 {
        return [0u8; 10];
    }
    let (sign_bit, abs_val) = if val < 0.0 {
        (0x80u8, -val)
    } else {
        (0x00u8, val)
    };

    let log2 = libm::log2(abs_val);
    let exponent = libm::floor(log2) as i32;
    let biased = (exponent + 16383) as u16;

    // Normalise: mantissa = abs_val / 2^exponent, scaled to fill 64 bits
    // with the integer bit (bit 63) set.
    let significand = abs_val / libm::pow(2.0, exponent as f64);
    let mantissa = (significand * (1u64 << 63) as f64) as u64;

    let mut buf = [0u8; 10];
    buf[0] = sign_bit | ((biased >> 8) as u8 & 0x7F);
    buf[1] = biased as u8;
    let m = mantissa.to_be_bytes();
    buf[2..10].copy_from_slice(&m);
    buf
}

/// Read a big-endian `i16` from `data` at `offset`.
#[inline]
fn read_i16_be(data: &[u8], offset: usize) -> Result<i16> {
    if offset + 2 > data.len() {
        return Err(ShravanError::EndOfStream);
    }
    Ok(i16::from_be_bytes([data[offset], data[offset + 1]]))
}

/// Read a big-endian `u32` from `data` at `offset`.
#[inline]
fn read_u32_be(data: &[u8], offset: usize) -> Result<u32> {
    if offset + 4 > data.len() {
        return Err(ShravanError::EndOfStream);
    }
    Ok(u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// Whether PCM data should be read in big-endian or little-endian byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endianness {
    Big,
    Little,
}

/// Decode AIFF (or AIFF-C) data from a byte slice.
///
/// Returns format information and interleaved f32 samples normalised to \[-1.0, 1.0\].
///
/// # Errors
///
/// Returns errors for invalid headers, unsupported compression, or truncated data.
#[must_use = "decoded audio data is returned and should not be discarded"]
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    // Minimum: FORM(4) + size(4) + AIFF(4) = 12
    if data.len() < 12 {
        return Err(ShravanError::InvalidHeader("AIFF file too short".into()));
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

    // Walk chunks
    let mut pos: usize = 12;

    let mut channels: u16 = 0;
    let mut _num_sample_frames: u32 = 0;
    let mut sample_size: u16 = 0;
    let mut sample_rate_f64: f64 = 0.0;
    let mut comm_found = false;

    let mut ssnd_data_start: usize = 0;
    let mut ssnd_data_len: usize = 0;
    let mut ssnd_found = false;

    let mut endianness = Endianness::Big;

    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = read_u32_be(data, pos + 4)? as usize;

        if chunk_id == b"COMM" {
            // Standard COMM is at least 18 bytes; AIFC adds 4+ more
            let min_comm = if is_aifc { 22 } else { 18 };
            if chunk_size < min_comm {
                return Err(ShravanError::InvalidHeader("COMM chunk too small".into()));
            }
            let base = pos + 8;
            channels = read_i16_be(data, base)? as u16;
            _num_sample_frames = read_u32_be(data, base + 2)?;
            sample_size = read_i16_be(data, base + 6)? as u16;

            if base + 8 + 10 > data.len() {
                return Err(ShravanError::EndOfStream);
            }
            sample_rate_f64 = extended_to_f64(&data[base + 8..base + 18]);

            if is_aifc {
                // compressionType is a 4-byte OSType right after the 10-byte sample rate
                if base + 22 > data.len() {
                    return Err(ShravanError::EndOfStream);
                }
                let comp = &data[base + 18..base + 22];
                if comp == b"NONE" {
                    endianness = Endianness::Big;
                } else if comp == b"sowt" {
                    endianness = Endianness::Little;
                } else {
                    return Err(ShravanError::DecodeError(format!(
                        "unsupported AIFF-C compression: {:?}",
                        core::str::from_utf8(comp).unwrap_or("????")
                    )));
                }
            }
            comm_found = true;
        } else if chunk_id == b"SSND" {
            if chunk_size < 8 {
                return Err(ShravanError::InvalidHeader("SSND chunk too small".into()));
            }
            let ssnd_offset = read_u32_be(data, pos + 8)? as usize;
            // blockSize at pos+12, not needed for decoding
            let header_bytes = 8; // offset(4) + blockSize(4)
            let pcm_start = pos + 8 + header_bytes + ssnd_offset;
            let pcm_len = chunk_size.saturating_sub(header_bytes + ssnd_offset);
            ssnd_data_start = pcm_start;
            ssnd_data_len = pcm_len;
            ssnd_found = true;
        }

        // Advance past chunk (padded to even boundary)
        let padded_size = chunk_size.saturating_add(chunk_size & 1);
        pos = pos.saturating_add(8).saturating_add(padded_size);

        if comm_found && ssnd_found {
            break;
        }
    }

    if !comm_found {
        return Err(ShravanError::InvalidHeader("missing COMM chunk".into()));
    }
    if !ssnd_found {
        return Err(ShravanError::InvalidHeader("missing SSND chunk".into()));
    }
    if channels == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }
    let sample_rate = sample_rate_f64 as u32;
    if sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    // Clamp to available bytes
    let available = data.len().saturating_sub(ssnd_data_start);
    let actual_len = ssnd_data_len.min(available);
    let audio_data = &data[ssnd_data_start..ssnd_data_start + actual_len];

    let samples = decode_pcm(audio_data, sample_size, endianness)?;

    let sample_count = samples.len() as u64;
    let total_frames = sample_count / u64::from(channels);
    let duration_secs = total_frames as f64 / f64::from(sample_rate);

    let info = FormatInfo {
        format: AudioFormat::Aiff,
        sample_rate,
        channels,
        bit_depth: sample_size,
        duration_secs,
        total_samples: total_frames,
    };

    Ok((info, samples))
}

/// Decode raw PCM bytes into f32 samples.
#[inline]
fn decode_pcm(audio_data: &[u8], bits: u16, endianness: Endianness) -> Result<Vec<f32>> {
    match (bits, endianness) {
        // --- 8-bit (signed in AIFF, endianness irrelevant) ---
        (8, _) => Ok(audio_data.iter().map(|&b| b as i8 as f32 / 128.0).collect()),

        // --- 16-bit ---
        (16, Endianness::Big) => Ok(audio_data
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_be_bytes([c[0], c[1]]);
                s as f32 / 32768.0
            })
            .collect()),
        (16, Endianness::Little) => Ok(audio_data
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_le_bytes([c[0], c[1]]);
                s as f32 / 32768.0
            })
            .collect()),

        // --- 24-bit ---
        (24, Endianness::Big) => Ok(audio_data
            .chunks_exact(3)
            .map(|c| {
                let raw = (i32::from(c[0]) << 16) | (i32::from(c[1]) << 8) | i32::from(c[2]);
                let extended = (raw << 8) >> 8; // sign-extend
                extended as f32 / 8_388_608.0
            })
            .collect()),
        (24, Endianness::Little) => Ok(audio_data
            .chunks_exact(3)
            .map(|c| {
                let raw = i32::from(c[0]) | (i32::from(c[1]) << 8) | (i32::from(c[2]) << 16);
                let extended = (raw << 8) >> 8;
                extended as f32 / 8_388_608.0
            })
            .collect()),

        // --- 32-bit ---
        (32, Endianness::Big) => Ok(audio_data
            .chunks_exact(4)
            .map(|c| {
                let s = i32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                s as f32 / 2_147_483_648.0
            })
            .collect()),
        (32, Endianness::Little) => Ok(audio_data
            .chunks_exact(4)
            .map(|c| {
                let s = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                s as f32 / 2_147_483_648.0
            })
            .collect()),

        _ => Err(ShravanError::DecodeError(format!(
            "unsupported AIFF bit depth: {bits}"
        ))),
    }
}

/// Encode interleaved f32 samples as an AIFF byte stream (big-endian PCM).
///
/// # Arguments
///
/// * `samples` — Interleaved f32 sample data in \[-1.0, 1.0\]
/// * `sample_rate` — Sample rate in Hz
/// * `channels` — Number of audio channels
/// * `bits_per_sample` — Target bit depth (8, 16, 24, or 32)
///
/// # Errors
///
/// Returns errors for invalid parameters or unsupported bit depths.
#[must_use = "encoded AIFF bytes are returned and should not be discarded"]
#[cfg(feature = "pcm")]
pub fn encode(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
) -> Result<Vec<u8>> {
    if channels == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }
    if sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    let raw_data = encode_samples_be(samples, bits_per_sample)?;

    let num_sample_frames = samples.len() as u32 / u32::from(channels);

    // SSND chunk: offset(4) + blockSize(4) + raw PCM
    let ssnd_data_size = 8u32 + raw_data.len() as u32;
    // COMM chunk: 18 bytes (standard AIFF COMM)
    let comm_chunk_size = 18u32;

    // Total FORM payload: AIFF(4) + COMM header(8) + COMM body(18) + SSND header(8) + SSND body
    let form_payload = 4 + 8 + comm_chunk_size + 8 + ssnd_data_size;

    let total_size = 8 + form_payload as usize; // FORM(4) + size(4) + payload
    let mut out = Vec::with_capacity(total_size);

    // FORM header
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&form_payload.to_be_bytes());
    out.extend_from_slice(b"AIFF");

    // COMM chunk
    out.extend_from_slice(b"COMM");
    out.extend_from_slice(&comm_chunk_size.to_be_bytes());
    out.extend_from_slice(&(channels as i16).to_be_bytes());
    out.extend_from_slice(&num_sample_frames.to_be_bytes());
    out.extend_from_slice(&(bits_per_sample as i16).to_be_bytes());
    out.extend_from_slice(&f64_to_extended(f64::from(sample_rate)));

    // SSND chunk
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&ssnd_data_size.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // offset
    out.extend_from_slice(&0u32.to_be_bytes()); // blockSize
    out.extend_from_slice(&raw_data);

    Ok(out)
}

/// Encode f32 samples to raw big-endian PCM bytes.
#[cfg(feature = "pcm")]
fn encode_samples_be(samples: &[f32], bits: u16) -> Result<Vec<u8>> {
    match bits {
        8 => Ok(samples
            .iter()
            .map(|&s| {
                let clamped = s.clamp(-1.0, 1.0);
                (clamped * 127.0) as i8 as u8
            })
            .collect()),
        16 => {
            let mut out = Vec::with_capacity(samples.len() * 2);
            for &s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                let val = (clamped * 32767.0) as i16;
                out.extend_from_slice(&val.to_be_bytes());
            }
            Ok(out)
        }
        24 => {
            let mut out = Vec::with_capacity(samples.len() * 3);
            for &s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                let val = (clamped * 8_388_607.0) as i32;
                out.push((val >> 16) as u8);
                out.push((val >> 8) as u8);
                out.push(val as u8);
            }
            Ok(out)
        }
        32 => {
            let mut out = Vec::with_capacity(samples.len() * 4);
            for &s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                let val = (clamped as f64 * 2_147_483_647.0) as i32;
                out.extend_from_slice(&val.to_be_bytes());
            }
            Ok(out)
        }
        _ => Err(ShravanError::EncodeError(format!(
            "unsupported AIFF bit depth for encoding: {bits}"
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_non_aiff() {
        let data = vec![0u8; 44];
        assert!(decode(&data).is_err());
    }

    #[test]
    fn decode_rejects_short_data() {
        let data = vec![0u8; 6];
        assert!(decode(&data).is_err());
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn i16_roundtrip() {
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        let encoded = encode(&samples, 44100, 1, 16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.format, AudioFormat::Aiff);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bit_depth, 16);
        assert_eq!(decoded.len(), samples.len());

        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.001, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn i8_roundtrip() {
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        let encoded = encode(&samples, 44100, 1, 8).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.format, AudioFormat::Aiff);
        assert_eq!(info.bit_depth, 8);
        assert_eq!(decoded.len(), samples.len());

        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.02, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn i24_roundtrip() {
        let samples = vec![0.0f32, 0.5, -0.5, 0.99, -0.99];
        let encoded = encode(&samples, 44100, 1, 24).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.bit_depth, 24);
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.001, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn i32_roundtrip() {
        let samples = vec![0.0f32, 0.5, -0.5];
        let encoded = encode(&samples, 44100, 1, 32).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.bit_depth, 32);
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.001, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn stereo_roundtrip() {
        let samples = vec![0.5f32, -0.5, 0.3, -0.3, 0.1, -0.1];
        let encoded = encode(&samples, 44100, 2, 16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.channels, 2);
        assert_eq!(info.total_samples, 3); // 3 frames
        assert_eq!(decoded.len(), 6);
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn header_fields_correct() {
        let samples = vec![0.0f32; 44100]; // 1 second mono
        let encoded = encode(&samples, 44100, 1, 16).unwrap();
        let (info, _) = decode(&encoded).unwrap();

        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bit_depth, 16);
        assert_eq!(info.total_samples, 44100);
        assert!((info.duration_secs - 1.0).abs() < 0.001);
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn header_fields_48000() {
        let samples = vec![0.0f32; 48000];
        let encoded = encode(&samples, 48000, 1, 16).unwrap();
        let (info, _) = decode(&encoded).unwrap();

        assert_eq!(info.sample_rate, 48000);
        assert!((info.duration_secs - 1.0).abs() < 0.001);
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn encode_rejects_zero_channels() {
        assert!(encode(&[0.5], 44100, 0, 16).is_err());
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn encode_rejects_zero_rate() {
        assert!(encode(&[0.5], 0, 1, 16).is_err());
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn encode_rejects_unsupported_bits() {
        assert!(encode(&[0.5], 44100, 1, 12).is_err());
    }

    #[test]
    fn extended_float_roundtrip() {
        let rates: &[f64] = &[
            8000.0, 11025.0, 22050.0, 44100.0, 48000.0, 96000.0, 192000.0,
        ];
        for &rate in rates {
            let ext = f64_to_extended(rate);
            let recovered = extended_to_f64(&ext);
            assert!(
                (rate - recovered).abs() < 0.5,
                "extended float roundtrip failed for {rate}: got {recovered}"
            );
        }
    }

    #[test]
    fn extended_float_zero() {
        let ext = f64_to_extended(0.0);
        assert_eq!(extended_to_f64(&ext), 0.0);
    }
}
