//! Audio format detection and metadata.

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShravanError};

/// Supported audio formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AudioFormat {
    /// RIFF WAVE format.
    Wav,
    /// Free Lossless Audio Codec.
    Flac,
    /// Raw PCM samples (no container).
    RawPcm,
}

impl core::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Wav => f.write_str("WAV"),
            Self::Flac => f.write_str("FLAC"),
            Self::RawPcm => f.write_str("Raw PCM"),
        }
    }
}

/// Descriptive information about an audio stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormatInfo {
    /// The container / codec format.
    pub format: AudioFormat,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of audio channels.
    pub channels: u16,
    /// Bits per sample in the source encoding.
    pub bit_depth: u16,
    /// Duration in seconds.
    pub duration_secs: f64,
    /// Total number of sample frames.
    pub total_samples: u64,
}

/// Detect the audio format from the first bytes of a file.
///
/// Requires at least 4 bytes of header data.
#[must_use]
pub fn detect_format(header: &[u8]) -> Result<AudioFormat> {
    if header.len() < 4 {
        return Err(ShravanError::InvalidHeader(
            "header too short for format detection".into(),
        ));
    }

    if header.starts_with(b"RIFF") {
        return Ok(AudioFormat::Wav);
    }
    if header.starts_with(b"fLaC") {
        return Ok(AudioFormat::Flac);
    }

    Err(ShravanError::UnsupportedFormat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_wav() {
        let header = b"RIFF\x00\x00\x00\x00WAVE";
        assert_eq!(detect_format(header).unwrap(), AudioFormat::Wav);
    }

    #[test]
    fn detect_flac() {
        let header = b"fLaC\x00\x00\x00\x22";
        assert_eq!(detect_format(header).unwrap(), AudioFormat::Flac);
    }

    #[test]
    fn detect_unknown() {
        let header = b"\x00\x00\x00\x00";
        assert!(detect_format(header).is_err());
    }

    #[test]
    fn detect_too_short() {
        let header = b"RI";
        assert!(detect_format(header).is_err());
    }

    #[test]
    fn audio_format_display() {
        assert_eq!(AudioFormat::Wav.to_string(), "WAV");
        assert_eq!(AudioFormat::Flac.to_string(), "FLAC");
        assert_eq!(AudioFormat::RawPcm.to_string(), "Raw PCM");
    }
}
