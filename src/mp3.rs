//! MP3 frame sync and header parsing.
//!
//! Provides frame-level parsing of MPEG audio files: sync word detection,
//! header field extraction, ID3v2 tag skipping, and multi-frame scanning.
//! This module does **not** perform audio decoding — samples are not produced.

use alloc::format;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

/// MPEG version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MpegVersion {
    /// MPEG-1.
    V1,
    /// MPEG-2 (half sample rate).
    V2,
    /// MPEG-2.5 (quarter sample rate, unofficial).
    V25,
}

/// MPEG audio layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MpegLayer {
    /// Layer I.
    I,
    /// Layer II.
    II,
    /// Layer III (MP3).
    III,
}

/// Channel mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ChannelMode {
    /// Stereo.
    Stereo,
    /// Joint stereo.
    JointStereo,
    /// Dual channel (two independent mono channels).
    DualChannel,
    /// Mono.
    Mono,
}

/// Parsed information from a single MP3 frame header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mp3FrameInfo {
    /// MPEG version.
    pub version: MpegVersion,
    /// MPEG layer.
    pub layer: MpegLayer,
    /// Bitrate in kbps.
    pub bitrate: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel mode.
    pub channel_mode: ChannelMode,
    /// Frame size in bytes (including header).
    pub frame_size: usize,
    /// Number of PCM samples per frame.
    pub samples_per_frame: u32,
    /// Whether the frame has a padding byte.
    pub padding: bool,
}

/// MPEG1 Layer III bitrate table (kbps), indexed 0..16.
const BITRATE_V1_L3: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];

/// MPEG2/2.5 Layer III bitrate table (kbps), indexed 0..16.
const BITRATE_V2_L3: [u32; 16] = [
    0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
];

/// MPEG1 sample rates (Hz).
const SAMPLE_RATE_V1: [u32; 3] = [44100, 48000, 32000];

/// MPEG2 sample rates (Hz).
const SAMPLE_RATE_V2: [u32; 3] = [22050, 24000, 16000];

/// MPEG2.5 sample rates (Hz).
const SAMPLE_RATE_V25: [u32; 3] = [11025, 12000, 8000];

/// Read a syncsafe integer from 4 bytes (7 bits per byte).
#[must_use]
#[inline]
fn syncsafe_to_u32(data: &[u8]) -> u32 {
    (u32::from(data[0]) << 21)
        | (u32::from(data[1]) << 14)
        | (u32::from(data[2]) << 7)
        | u32::from(data[3])
}

/// Compute the number of bytes to skip for an ID3v2 tag at the start of `data`.
///
/// Returns 0 if no ID3v2 tag is present.
#[must_use]
fn id3v2_skip(data: &[u8]) -> usize {
    if data.len() < 10 {
        return 0;
    }
    if &data[0..3] != b"ID3" {
        return 0;
    }
    let size = syncsafe_to_u32(&data[6..10]) as usize;
    10 + size
}

/// Parse a 4-byte MPEG audio frame header.
///
/// # Errors
///
/// Returns [`ShravanError::InvalidHeader`] if the header is invalid (bad sync,
/// reserved version/layer/bitrate/sample-rate fields).
pub fn parse_frame_header(header: &[u8; 4]) -> Result<Mp3FrameInfo> {
    // Sync word: first 11 bits must be 1
    if header[0] != 0xFF || (header[1] & 0xE0) != 0xE0 {
        return Err(ShravanError::InvalidHeader("invalid sync word".into()));
    }

    // Version: bits 4-3 of byte 1
    let version_bits = (header[1] >> 3) & 0x03;
    let version = match version_bits {
        0b00 => MpegVersion::V25,
        0b10 => MpegVersion::V2,
        0b11 => MpegVersion::V1,
        _ => return Err(ShravanError::InvalidHeader("reserved MPEG version".into())),
    };

    // Layer: bits 2-1 of byte 1
    let layer_bits = (header[1] >> 1) & 0x03;
    let layer = match layer_bits {
        0b01 => MpegLayer::III,
        0b10 => MpegLayer::II,
        0b11 => MpegLayer::I,
        _ => return Err(ShravanError::InvalidHeader("reserved MPEG layer".into())),
    };

    // Bitrate: bits 7-4 of byte 2
    let bitrate_index = ((header[2] >> 4) & 0x0F) as usize;
    let bitrate = match (version, layer) {
        (MpegVersion::V1, MpegLayer::III) => BITRATE_V1_L3[bitrate_index],
        (MpegVersion::V2 | MpegVersion::V25, MpegLayer::III) => BITRATE_V2_L3[bitrate_index],
        _ => {
            // For non-Layer-III we still attempt V1 L3 table as a fallback
            // but in practice this module focuses on Layer III.
            BITRATE_V1_L3[bitrate_index]
        }
    };
    if bitrate == 0 {
        return Err(ShravanError::InvalidHeader(format!(
            "invalid bitrate index: {bitrate_index}"
        )));
    }

    // Sample rate: bits 3-2 of byte 2
    let sr_index = ((header[2] >> 2) & 0x03) as usize;
    if sr_index >= 3 {
        return Err(ShravanError::InvalidHeader(
            "reserved sample rate index".into(),
        ));
    }
    let sample_rate = match version {
        MpegVersion::V1 => SAMPLE_RATE_V1[sr_index],
        MpegVersion::V2 => SAMPLE_RATE_V2[sr_index],
        MpegVersion::V25 => SAMPLE_RATE_V25[sr_index],
    };

    // Padding: bit 1 of byte 2
    let padding = (header[2] >> 1) & 0x01 == 1;

    // Channel mode: bits 7-6 of byte 3
    let channel_mode = match (header[3] >> 6) & 0x03 {
        0b00 => ChannelMode::Stereo,
        0b01 => ChannelMode::JointStereo,
        0b10 => ChannelMode::DualChannel,
        _ => ChannelMode::Mono,
    };

    // Samples per frame
    let samples_per_frame = match (version, layer) {
        (MpegVersion::V1, MpegLayer::I) => 384,
        (MpegVersion::V1, MpegLayer::II | MpegLayer::III) => 1152,
        (MpegVersion::V2 | MpegVersion::V25, MpegLayer::I) => 384,
        (MpegVersion::V2 | MpegVersion::V25, MpegLayer::II) => 1152,
        (MpegVersion::V2 | MpegVersion::V25, MpegLayer::III) => 576,
    };

    // Frame size calculation
    let padding_bytes: usize = if padding { 1 } else { 0 };
    let frame_size = match layer {
        MpegLayer::I => (12 * bitrate as usize * 1000 / sample_rate as usize + padding_bytes) * 4,
        MpegLayer::II | MpegLayer::III => {
            let spf_divisor = match version {
                MpegVersion::V1 => 1152,
                MpegVersion::V2 | MpegVersion::V25 => 576,
            };
            spf_divisor * bitrate as usize * 1000 / (8 * sample_rate as usize) + padding_bytes
        }
    };

    Ok(Mp3FrameInfo {
        version,
        layer,
        bitrate,
        sample_rate,
        channel_mode,
        frame_size,
        samples_per_frame,
        padding,
    })
}

/// Scan `data` for all valid MP3 frame headers.
///
/// # Errors
///
/// Returns [`ShravanError::InvalidHeader`] if no valid frames are found.
pub fn scan_frames(data: &[u8]) -> Result<Vec<Mp3FrameInfo>> {
    let skip = id3v2_skip(data);
    let mut pos = skip;
    let mut frames = Vec::new();

    while pos + 4 <= data.len() {
        if data[pos] == 0xFF && (data[pos + 1] & 0xE0) == 0xE0 {
            let header: [u8; 4] = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
            if let Ok(info) = parse_frame_header(&header)
                && info.frame_size > 0
            {
                frames.push(info);
                pos += frames.last().map_or(1, |f| f.frame_size.max(1));
                continue;
            }
        }
        pos += 1;
    }

    if frames.is_empty() {
        return Err(ShravanError::InvalidHeader(
            "no valid MP3 frames found".into(),
        ));
    }

    Ok(frames)
}

/// Inspect MP3 data and return format information.
///
/// Skips an ID3v2 tag if present, scans for frame headers, and builds
/// [`FormatInfo`] from the first valid frame. Duration is estimated from
/// the total number of frames. No audio decoding is performed — the
/// returned samples vector is empty.
///
/// # Errors
///
/// Returns errors for missing or invalid frame headers.
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    let frames = scan_frames(data)?;

    let first = &frames[0];

    let total_samples_all_frames: u64 = frames.iter().map(|f| u64::from(f.samples_per_frame)).sum();

    let duration_secs = total_samples_all_frames as f64 / f64::from(first.sample_rate);

    let channels: u16 = match first.channel_mode {
        ChannelMode::Mono => 1,
        _ => 2,
    };

    let info = FormatInfo {
        format: AudioFormat::Mp3,
        sample_rate: first.sample_rate,
        channels,
        bit_depth: 16, // MP3 is typically decoded to 16-bit
        duration_secs,
        total_samples: total_samples_all_frames,
    };

    Ok((info, Vec::new()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Build a valid MPEG1 Layer III frame header.
    /// 128 kbps, 44100 Hz, stereo, no padding.
    fn make_valid_header() -> [u8; 4] {
        // sync=0xFFE, version=11(V1), layer=01(III), no CRC
        // byte0 = 0xFF
        // byte1 = 1111_1011 = 0xFB  (sync + V1 + LayerIII + no CRC)
        // bitrate index 9 = 128kbps for V1 L3, sr index 0 = 44100
        // byte2 = 1001_00_0_0 = 0x90
        // channel mode 00 = stereo
        // byte3 = 0x00
        [0xFF, 0xFB, 0x90, 0x00]
    }

    #[test]
    fn parse_valid_header() {
        let header = make_valid_header();
        let info = parse_frame_header(&header).unwrap();

        assert_eq!(info.version, MpegVersion::V1);
        assert_eq!(info.layer, MpegLayer::III);
        assert_eq!(info.bitrate, 128);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channel_mode, ChannelMode::Stereo);
        assert_eq!(info.samples_per_frame, 1152);
        assert!(!info.padding);
        // Frame size = 1152 * 128000 / (8 * 44100) = 417 bytes
        assert_eq!(info.frame_size, 417);
    }

    #[test]
    fn reject_bad_sync() {
        let header = [0x00, 0x00, 0x90, 0x00];
        assert!(parse_frame_header(&header).is_err());
    }

    #[test]
    fn reject_reserved_version() {
        // version bits = 01 -> reserved
        let header = [0xFF, 0xE9, 0x90, 0x00]; // 0xE9 = 1110_1001 -> sync ok, version=01
        assert!(parse_frame_header(&header).is_err());
    }

    #[test]
    fn reject_reserved_layer() {
        // layer bits = 00 -> reserved
        let header = [0xFF, 0xF1, 0x90, 0x00]; // 0xF1 = 1111_0001 -> layer=00
        assert!(parse_frame_header(&header).is_err());
    }

    #[test]
    fn reject_zero_bitrate() {
        // bitrate index 0 -> free format (treated as 0, rejected)
        let header = [0xFF, 0xFB, 0x00, 0x00];
        assert!(parse_frame_header(&header).is_err());
    }

    #[test]
    fn reject_reserved_sample_rate() {
        // sr index 3 -> reserved
        let header = [0xFF, 0xFB, 0x9C, 0x00]; // bits 3-2 of byte2 = 11
        assert!(parse_frame_header(&header).is_err());
    }

    #[test]
    fn id3v2_skip_no_tag() {
        let data = [0xFF, 0xFB, 0x90, 0x00];
        assert_eq!(id3v2_skip(&data), 0);
    }

    #[test]
    fn id3v2_skip_with_tag() {
        let mut data = Vec::new();
        data.extend_from_slice(b"ID3");
        data.push(4); // version major
        data.push(0); // version minor
        data.push(0); // flags
        // syncsafe size = 100 -> bytes: 0,0,0,100
        data.extend_from_slice(&[0, 0, 0, 100]);
        // padding
        data.resize(data.len() + 200, 0);

        assert_eq!(id3v2_skip(&data), 110); // 10 + 100
    }

    #[test]
    fn frame_size_with_padding() {
        // Same as valid header but with padding bit set
        let header = [0xFF, 0xFB, 0x92, 0x00]; // bit1 of byte2 set
        let info = parse_frame_header(&header).unwrap();
        assert!(info.padding);
        assert_eq!(info.frame_size, 418); // 417 + 1
    }

    #[test]
    fn scan_multiple_frames() {
        let header = make_valid_header();
        let frame_info = parse_frame_header(&header).unwrap();
        let frame_size = frame_info.frame_size;

        // Build data with 3 frames
        let mut data = Vec::new();
        for _ in 0..3 {
            data.extend_from_slice(&header);
            data.resize(data.len() + frame_size - 4, 0);
        }

        let frames = scan_frames(&data).unwrap();
        assert_eq!(frames.len(), 3);
    }

    #[test]
    fn scan_frames_empty() {
        let data = vec![0u8; 100];
        assert!(scan_frames(&data).is_err());
    }

    #[test]
    fn mpeg_version_serde_roundtrip() {
        let v = MpegVersion::V1;
        let json = serde_json::to_string(&v).unwrap();
        let v2: MpegVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn mpeg_layer_serde_roundtrip() {
        let l = MpegLayer::III;
        let json = serde_json::to_string(&l).unwrap();
        let l2: MpegLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(l, l2);
    }

    #[test]
    fn channel_mode_serde_roundtrip() {
        let c = ChannelMode::JointStereo;
        let json = serde_json::to_string(&c).unwrap();
        let c2: ChannelMode = serde_json::from_str(&json).unwrap();
        assert_eq!(c, c2);
    }

    #[test]
    fn mp3_frame_info_serde_roundtrip() {
        let header = make_valid_header();
        let info = parse_frame_header(&header).unwrap();
        let json = serde_json::to_string(&info).unwrap();
        let info2: Mp3FrameInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, info2);
    }

    #[test]
    fn decode_produces_format_info() {
        let header = make_valid_header();
        let frame_info = parse_frame_header(&header).unwrap();
        let frame_size = frame_info.frame_size;

        let mut data = Vec::new();
        for _ in 0..10 {
            data.extend_from_slice(&header);
            data.resize(data.len() + frame_size - 4, 0);
        }

        let (info, samples) = decode(&data).unwrap();
        assert_eq!(info.format, AudioFormat::Mp3);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.channels, 2);
        assert!(samples.is_empty());
        assert!(info.duration_secs > 0.0);
    }

    #[test]
    fn mono_channel_count() {
        // channel mode = 11 -> mono
        let header = [0xFF, 0xFB, 0x90, 0xC0];
        let info = parse_frame_header(&header).unwrap();
        assert_eq!(info.channel_mode, ChannelMode::Mono);
    }
}
