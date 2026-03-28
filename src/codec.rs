//! Unified codec interface — auto-detect and decode audio data.

use crate::error::{Result, ShravanError};
use crate::format::{detect_format, AudioFormat, FormatInfo};

/// Auto-detect the format and decode audio data.
///
/// Inspects the header bytes to determine the format, then delegates
/// to the appropriate decoder.
///
/// # Errors
///
/// Returns [`ShravanError::UnsupportedFormat`] if the format cannot be detected
/// or if the required feature is not enabled.
pub fn open(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    let format = detect_format(data)?;

    match format {
        #[cfg(feature = "wav")]
        AudioFormat::Wav => crate::wav::decode(data),

        #[cfg(not(feature = "wav"))]
        AudioFormat::Wav => Err(ShravanError::UnsupportedFormat),

        #[cfg(feature = "flac")]
        AudioFormat::Flac => crate::flac::decode(data),

        #[cfg(not(feature = "flac"))]
        AudioFormat::Flac => Err(ShravanError::UnsupportedFormat),

        // RawPcm and any future variants
        _ => Err(ShravanError::UnsupportedFormat),
    }
}

/// The `AudioCodec` trait provides a common interface for audio decoders.
pub trait AudioCodec {
    /// Decode audio data from a byte slice.
    ///
    /// Returns format information and interleaved f32 samples.
    fn decode(&self, data: &[u8]) -> Result<(FormatInfo, Vec<f32>)>;
}

#[cfg(feature = "wav")]
/// WAV codec implementation.
pub struct WavCodec;

#[cfg(feature = "wav")]
impl AudioCodec for WavCodec {
    fn decode(&self, data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
        crate::wav::decode(data)
    }
}

#[cfg(feature = "flac")]
/// FLAC codec implementation.
pub struct FlacCodec;

#[cfg(feature = "flac")]
impl AudioCodec for FlacCodec {
    fn decode(&self, data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
        crate::flac::decode(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "wav")]
    #[test]
    fn open_wav() {
        // Build a minimal WAV
        let samples = vec![0.0f32; 100];
        let encoded = crate::wav::encode(&samples, 44100, 1, crate::pcm::PcmFormat::I16).unwrap();
        let (info, decoded) = open(&encoded).unwrap();
        assert_eq!(info.format, AudioFormat::Wav);
        assert_eq!(decoded.len(), 100);
    }

    #[test]
    fn open_unknown() {
        let data = vec![0u8; 100];
        assert!(open(&data).is_err());
    }

    #[test]
    fn open_too_short() {
        let data = vec![0u8; 2];
        assert!(open(&data).is_err());
    }

    #[cfg(feature = "wav")]
    #[test]
    fn wav_codec_trait() {
        let codec = WavCodec;
        let samples = vec![0.5f32; 10];
        let encoded = crate::wav::encode(&samples, 44100, 1, crate::pcm::PcmFormat::F32).unwrap();
        let (info, _) = codec.decode(&encoded).unwrap();
        assert_eq!(info.format, AudioFormat::Wav);
    }
}
