//! WAV (RIFF WAVE) encoder and decoder.
//!
//! Supports PCM integer (8/16/24/32-bit) and IEEE float (32-bit) formats.

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};
#[cfg(feature = "pcm")]
use crate::pcm::PcmFormat;

/// WAV format codes.
const WAV_FORMAT_PCM: u16 = 1;
const WAV_FORMAT_IEEE_FLOAT: u16 = 3;
const WAV_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Read a little-endian u16 from a byte slice at the given offset.
#[inline]
fn read_u16_le(data: &[u8], offset: usize) -> Result<u16> {
    if offset + 2 > data.len() {
        return Err(ShravanError::EndOfStream);
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

/// Read a little-endian u32 from a byte slice at the given offset.
#[inline]
fn read_u32_le(data: &[u8], offset: usize) -> Result<u32> {
    if offset + 4 > data.len() {
        return Err(ShravanError::EndOfStream);
    }
    Ok(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// Decode WAV data from a byte slice.
///
/// Returns format information and interleaved f32 samples normalized to \[-1.0, 1.0\].
///
/// # Errors
///
/// Returns errors for invalid headers, unsupported formats, or truncated data.
#[must_use = "decoded audio data is returned and should not be discarded"]
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    // Validate RIFF header
    if data.len() < 44 {
        return Err(ShravanError::InvalidHeader("WAV file too short".into()));
    }
    if &data[0..4] != b"RIFF" {
        return Err(ShravanError::InvalidHeader("missing RIFF magic".into()));
    }
    if &data[8..12] != b"WAVE" {
        return Err(ShravanError::InvalidHeader(
            "missing WAVE identifier".into(),
        ));
    }

    // Find fmt chunk
    let mut pos = 12;
    let mut fmt_format_code: u16 = 0;
    let mut fmt_channels: u16 = 0;
    let mut fmt_sample_rate: u32 = 0;
    let mut fmt_bits_per_sample: u16 = 0;
    let mut fmt_found = false;

    // Find data chunk
    let mut data_start: usize = 0;
    let mut data_size: usize = 0;
    let mut data_found = false;

    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = read_u32_le(data, pos + 4)? as usize;

        if chunk_id == b"fmt " {
            if chunk_size < 16 {
                return Err(ShravanError::InvalidHeader("fmt chunk too small".into()));
            }
            fmt_format_code = read_u16_le(data, pos + 8)?;
            fmt_channels = read_u16_le(data, pos + 10)?;
            fmt_sample_rate = read_u32_le(data, pos + 12)?;
            // skip byte_rate (4 bytes) and block_align (2 bytes)
            fmt_bits_per_sample = read_u16_le(data, pos + 22)?;

            // WAVE_FORMAT_EXTENSIBLE: actual format is in the SubFormat GUID
            if fmt_format_code == WAV_FORMAT_EXTENSIBLE && chunk_size >= 40 {
                // wValidBitsPerSample at offset 18, dwChannelMask at 20
                let valid_bits = read_u16_le(data, pos + 26)?;
                if valid_bits > 0 {
                    fmt_bits_per_sample = valid_bits;
                }
                // SubFormat GUID starts at offset 24 from fmt data (pos+8+24 = pos+32)
                // First 2 bytes of GUID are the actual format code
                fmt_format_code = read_u16_le(data, pos + 32)?;
            }
            fmt_found = true;
        } else if chunk_id == b"data" {
            data_start = pos + 8;
            data_size = chunk_size;
            data_found = true;
        }

        // Move to next chunk (chunk sizes are padded to even boundaries)
        let padded_size = chunk_size.saturating_add(chunk_size & 1);
        let advance = padded_size.saturating_add(8);
        pos = pos.saturating_add(advance);

        if fmt_found && data_found {
            break;
        }
    }

    if !fmt_found {
        return Err(ShravanError::InvalidHeader("missing fmt chunk".into()));
    }
    if !data_found {
        return Err(ShravanError::InvalidHeader("missing data chunk".into()));
    }
    if fmt_channels == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }
    if fmt_sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    // Clamp data_size to available bytes
    let available = data.len().saturating_sub(data_start);
    let actual_data_size = data_size.min(available);
    let audio_data = &data[data_start..data_start + actual_data_size];

    // Decode samples to f32
    let samples = match (fmt_format_code, fmt_bits_per_sample) {
        (WAV_FORMAT_PCM, 8) => {
            // Unsigned 8-bit PCM
            audio_data
                .iter()
                .map(|&b| (b as f32 - 128.0) / 128.0)
                .collect()
        }
        (WAV_FORMAT_PCM, 16) => audio_data
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_le_bytes([c[0], c[1]]);
                s as f32 / 32768.0
            })
            .collect(),
        (WAV_FORMAT_PCM, 24) => audio_data
            .chunks_exact(3)
            .map(|c| {
                let raw = i32::from(c[0]) | (i32::from(c[1]) << 8) | (i32::from(c[2]) << 16);
                let extended = (raw << 8) >> 8;
                extended as f32 / 8_388_608.0
            })
            .collect(),
        (WAV_FORMAT_PCM, 32) => audio_data
            .chunks_exact(4)
            .map(|c| {
                let s = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                s as f32 / 2_147_483_648.0
            })
            .collect(),
        (WAV_FORMAT_IEEE_FLOAT, 32) => audio_data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect::<Vec<f32>>(),
        _ => {
            return Err(ShravanError::DecodeError(format!(
                "unsupported WAV format: code={fmt_format_code}, bits={fmt_bits_per_sample}"
            )));
        }
    };

    let sample_count: u64 = samples.len() as u64;
    let total_frames = sample_count / u64::from(fmt_channels);
    let duration_secs = total_frames as f64 / f64::from(fmt_sample_rate);

    let info = FormatInfo {
        format: AudioFormat::Wav,
        sample_rate: fmt_sample_rate,
        channels: fmt_channels,
        bit_depth: fmt_bits_per_sample,
        duration_secs,
        total_samples: total_frames,
    };

    Ok((info, samples))
}

/// Encode interleaved f32 samples as a WAV byte stream.
///
/// # Arguments
///
/// * `samples` - Interleaved f32 sample data in \[-1.0, 1.0\]
/// * `sample_rate` - Sample rate in Hz
/// * `channels` - Number of audio channels
/// * `format` - Target PCM format for encoding
///
/// # Errors
///
/// Returns errors for invalid parameters or unsupported formats.
#[must_use = "encoded WAV bytes are returned and should not be discarded"]
#[cfg(feature = "pcm")]
pub fn encode(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    format: PcmFormat,
) -> Result<Vec<u8>> {
    if channels == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }
    if sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    let (format_code, bits_per_sample) = match format {
        PcmFormat::I8 => (WAV_FORMAT_PCM, 8u16),
        PcmFormat::I16 => (WAV_FORMAT_PCM, 16u16),
        PcmFormat::I24 => (WAV_FORMAT_PCM, 24u16),
        PcmFormat::I32 => (WAV_FORMAT_PCM, 32u16),
        PcmFormat::F32 => (WAV_FORMAT_IEEE_FLOAT, 32u16),
        PcmFormat::F64 => {
            return Err(ShravanError::EncodeError(
                "f64 WAV encoding not supported".into(),
            ));
        }
    };

    let bytes_per_sample = bits_per_sample / 8;
    let block_align = channels * bytes_per_sample;
    let byte_rate = u32::from(block_align) * sample_rate;

    // Encode raw sample data
    let raw_data = encode_samples(samples, format)?;
    let data_size = raw_data.len() as u32;

    // RIFF header (12) + fmt chunk (24) + data chunk header (8) + data
    let file_size = 4 + 24 + 8 + data_size; // WAVE + fmt chunk + data chunk
    let total_size = 8 + file_size; // RIFF + size + content

    let mut out = Vec::with_capacity(total_size as usize);

    // RIFF header
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // fmt chunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    out.extend_from_slice(&format_code.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&raw_data);

    Ok(out)
}

/// Encode f32 samples to raw bytes in the specified PCM format.
#[cfg(feature = "pcm")]
fn encode_samples(samples: &[f32], format: PcmFormat) -> Result<Vec<u8>> {
    match format {
        PcmFormat::I8 => Ok(samples
            .iter()
            .map(|&s| {
                let clamped = s.clamp(-1.0, 1.0);
                ((clamped * 128.0) + 128.0).clamp(0.0, 255.0) as u8
            })
            .collect()),
        PcmFormat::I16 => {
            let mut out = Vec::with_capacity(samples.len() * 2);
            for &s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                let val = (clamped * 32767.0) as i16;
                out.extend_from_slice(&val.to_le_bytes());
            }
            Ok(out)
        }
        PcmFormat::I24 => {
            let mut out = Vec::with_capacity(samples.len() * 3);
            for &s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                let val = (clamped * 8_388_607.0) as i32;
                out.push(val as u8);
                out.push((val >> 8) as u8);
                out.push((val >> 16) as u8);
            }
            Ok(out)
        }
        PcmFormat::I32 => {
            let mut out = Vec::with_capacity(samples.len() * 4);
            for &s in samples {
                let clamped = s.clamp(-1.0, 1.0);
                let val = (clamped as f64 * 2_147_483_647.0) as i32;
                out.extend_from_slice(&val.to_le_bytes());
            }
            Ok(out)
        }
        PcmFormat::F32 => {
            let mut out = Vec::with_capacity(samples.len() * 4);
            for &s in samples {
                out.extend_from_slice(&s.to_le_bytes());
            }
            Ok(out)
        }
        PcmFormat::F64 => Err(ShravanError::EncodeError(
            "f64 WAV encoding not supported".into(),
        )),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_short_data() {
        let data = vec![0u8; 10];
        assert!(decode(&data).is_err());
    }

    #[test]
    fn decode_rejects_bad_riff() {
        let mut data = vec![0u8; 44];
        data[0..4].copy_from_slice(b"XXXX");
        assert!(decode(&data).is_err());
    }

    #[test]
    fn decode_rejects_bad_wave() {
        let mut data = vec![0u8; 44];
        data[0..4].copy_from_slice(b"RIFF");
        data[8..12].copy_from_slice(b"XXXX");
        assert!(decode(&data).is_err());
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn encode_rejects_zero_channels() {
        assert!(encode(&[0.5], 44100, 0, PcmFormat::I16).is_err());
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn encode_rejects_zero_rate() {
        assert!(encode(&[0.5], 0, 1, PcmFormat::I16).is_err());
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn i16_roundtrip() {
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        let encoded = encode(&samples, 44100, 1, PcmFormat::I16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.format, AudioFormat::Wav);
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
    fn f32_roundtrip() {
        let samples = vec![0.0f32, 0.25, -0.25, 0.99, -0.99];
        let encoded = encode(&samples, 48000, 1, PcmFormat::F32).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(info.sample_rate, 48000);
        assert_eq!(info.bit_depth, 32);

        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < f32::EPSILON, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn stereo_roundtrip() {
        let samples = vec![0.5f32, -0.5, 0.3, -0.3, 0.1, -0.1];
        let encoded = encode(&samples, 44100, 2, PcmFormat::I16).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.channels, 2);
        assert_eq!(info.total_samples, 3); // 3 frames
        assert_eq!(decoded.len(), 6);
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn i24_roundtrip() {
        let samples = vec![0.0f32, 0.5, -0.5, 0.99, -0.99];
        let encoded = encode(&samples, 44100, 1, PcmFormat::I24).unwrap();
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
        let encoded = encode(&samples, 44100, 1, PcmFormat::I32).unwrap();
        let (info, decoded) = decode(&encoded).unwrap();

        assert_eq!(info.bit_depth, 32);
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.001, "sample mismatch: {a} vs {b}");
        }
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn header_fields_correct() {
        let samples = vec![0.0f32; 44100]; // 1 second mono
        let encoded = encode(&samples, 44100, 1, PcmFormat::I16).unwrap();
        let (info, _) = decode(&encoded).unwrap();

        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bit_depth, 16);
        assert_eq!(info.total_samples, 44100);
        assert!((info.duration_secs - 1.0).abs() < 0.001);
    }

    #[cfg(feature = "pcm")]
    #[test]
    fn encode_rejects_f64() {
        assert!(encode(&[0.5], 44100, 1, PcmFormat::F64).is_err());
    }
}
