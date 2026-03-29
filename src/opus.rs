//! Opus header parsing via Ogg container.
//!
//! Parses `OpusHead` and `OpusTags` packets from an Ogg bitstream.
//! No Opus audio decoding is performed — samples are not produced.

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

/// Parsed Opus identification header (`OpusHead`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpusHead {
    /// Version number (must be 1 for the current spec).
    pub version: u8,
    /// Number of output channels.
    pub channel_count: u8,
    /// Number of samples to discard from the beginning of the decoded stream.
    pub pre_skip: u16,
    /// Original input sample rate (informational, Opus always decodes at 48 kHz).
    pub input_sample_rate: u32,
    /// Output gain in Q7.8 dB.
    pub output_gain: i16,
    /// Channel mapping family (0 = mono/stereo, 1 = Vorbis order, 255 = unspec).
    pub channel_mapping_family: u8,
}

/// Parse an `OpusHead` identification packet.
///
/// The packet must be at least 19 bytes and begin with the `OpusHead` magic.
///
/// # Errors
///
/// Returns [`ShravanError::InvalidHeader`] for wrong magic or truncated data.
pub fn parse_opus_head(packet: &[u8]) -> Result<OpusHead> {
    if packet.len() < 19 {
        return Err(ShravanError::InvalidHeader(
            "OpusHead packet too short (need >= 19 bytes)".into(),
        ));
    }
    if &packet[0..8] != b"OpusHead" {
        return Err(ShravanError::InvalidHeader("missing OpusHead magic".into()));
    }

    let version = packet[8];
    let channel_count = packet[9];
    let pre_skip = u16::from_le_bytes([packet[10], packet[11]]);
    let input_sample_rate = u32::from_le_bytes([packet[12], packet[13], packet[14], packet[15]]);
    let output_gain = i16::from_le_bytes([packet[16], packet[17]]);
    let channel_mapping_family = packet[18];

    if channel_count == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }

    Ok(OpusHead {
        version,
        channel_count,
        pre_skip,
        input_sample_rate,
        output_gain,
        channel_mapping_family,
    })
}

/// Parse an `OpusTags` comment packet.
///
/// If the `tag` feature is enabled, delegates to the Vorbis Comment parser
/// in [`crate::tag`]. Otherwise returns default metadata.
///
/// # Errors
///
/// Returns [`ShravanError::InvalidHeader`] for wrong magic or truncated data.
#[cfg(feature = "tag")]
pub fn parse_opus_tags(packet: &[u8]) -> Result<crate::tag::AudioMetadata> {
    if packet.len() < 8 {
        return Err(ShravanError::InvalidHeader(
            "OpusTags packet too short".into(),
        ));
    }
    if &packet[0..8] != b"OpusTags" {
        return Err(ShravanError::InvalidHeader("missing OpusTags magic".into()));
    }

    crate::tag::read_vorbis_comment(&packet[8..])
}

/// Parse an `OpusTags` comment packet (tag feature disabled — returns default).
#[cfg(not(feature = "tag"))]
pub fn parse_opus_tags(packet: &[u8]) -> Result<()> {
    if packet.len() < 8 {
        return Err(ShravanError::InvalidHeader(
            "OpusTags packet too short".into(),
        ));
    }
    if &packet[0..8] != b"OpusTags" {
        return Err(ShravanError::InvalidHeader("missing OpusTags magic".into()));
    }
    Ok(())
}

/// Scan backwards from the end of `data` for the last Ogg page and read
/// its granule position.
fn find_last_granule(data: &[u8]) -> Option<i64> {
    // Search backwards for OggS capture pattern
    if data.len() < 27 {
        return None;
    }
    let mut pos = data.len().saturating_sub(27);
    loop {
        if pos + 14 <= data.len() && &data[pos..pos + 4] == b"OggS" && data[pos + 4] == 0 {
            // Read granule position at offset 6
            if pos + 14 <= data.len() {
                let granule = i64::from_le_bytes([
                    data[pos + 6],
                    data[pos + 7],
                    data[pos + 8],
                    data[pos + 9],
                    data[pos + 10],
                    data[pos + 11],
                    data[pos + 12],
                    data[pos + 13],
                ]);
                return Some(granule);
            }
        }
        if pos == 0 {
            break;
        }
        pos -= 1;
    }
    None
}

/// Decode an Opus stream from pre-extracted Ogg packets.
///
/// Called by [`crate::ogg::decode`] when the first packet is identified as
/// `OpusHead`. Parses the identification and comment headers, estimates
/// duration from the last Ogg page granule position, and returns
/// [`FormatInfo`] with an empty samples vector (no audio decoding).
///
/// # Errors
///
/// Returns errors for invalid Opus headers or missing packets.
pub(crate) fn decode_from_packets(
    packets: &[Vec<u8>],
    raw_data: &[u8],
) -> Result<(FormatInfo, Vec<f32>)> {
    if packets.is_empty() {
        return Err(ShravanError::EndOfStream);
    }

    let head = parse_opus_head(&packets[0])?;

    // Try parsing tags from second packet (best-effort)
    if packets.len() >= 2 {
        let _ = parse_opus_tags(&packets[1]);
    }

    // Estimate duration from last granule position
    let duration_secs = if let Some(granule) = find_last_granule(raw_data) {
        let effective = granule.saturating_sub(i64::from(head.pre_skip));
        if effective > 0 {
            effective as f64 / 48000.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let total_samples = if duration_secs > 0.0 {
        (duration_secs * 48000.0) as u64
    } else {
        0
    };

    let info = FormatInfo {
        format: AudioFormat::Opus,
        sample_rate: 48000, // Opus always decodes at 48 kHz
        channels: u16::from(head.channel_count),
        bit_depth: 16, // Opus is typically decoded to 16-bit
        duration_secs,
        total_samples,
    };

    Ok((info, Vec::new()))
}

/// Decode an Opus stream from raw Ogg container data.
///
/// Delegates to [`crate::ogg::extract_packets`] for Ogg demuxing, then
/// parses the Opus headers. No audio decoding is performed.
///
/// # Errors
///
/// Returns errors for invalid Ogg/Opus structure.
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    let packets = crate::ogg::extract_packets(data)?;
    decode_from_packets(&packets, data)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Build a minimal valid OpusHead packet.
    fn make_opus_head(channels: u8, pre_skip: u16, sample_rate: u32) -> Vec<u8> {
        let mut pkt = Vec::new();
        pkt.extend_from_slice(b"OpusHead");
        pkt.push(1); // version
        pkt.push(channels);
        pkt.extend_from_slice(&pre_skip.to_le_bytes());
        pkt.extend_from_slice(&sample_rate.to_le_bytes());
        pkt.extend_from_slice(&0i16.to_le_bytes()); // output gain
        pkt.push(0); // channel mapping family
        pkt
    }

    #[test]
    fn parse_valid_opus_head() {
        let pkt = make_opus_head(2, 312, 48000);
        let head = parse_opus_head(&pkt).unwrap();

        assert_eq!(head.version, 1);
        assert_eq!(head.channel_count, 2);
        assert_eq!(head.pre_skip, 312);
        assert_eq!(head.input_sample_rate, 48000);
        assert_eq!(head.output_gain, 0);
        assert_eq!(head.channel_mapping_family, 0);
    }

    #[test]
    fn reject_wrong_magic() {
        let mut pkt = make_opus_head(2, 312, 48000);
        pkt[0..8].copy_from_slice(b"NotOpus!");
        assert!(parse_opus_head(&pkt).is_err());
    }

    #[test]
    fn reject_short_data() {
        let pkt = b"OpusHead1234567"; // only 15 bytes
        assert!(parse_opus_head(pkt).is_err());
    }

    #[test]
    fn reject_zero_channels() {
        let pkt = make_opus_head(0, 312, 48000);
        assert!(parse_opus_head(&pkt).is_err());
    }

    #[test]
    fn opus_head_serde_roundtrip() {
        let pkt = make_opus_head(2, 312, 44100);
        let head = parse_opus_head(&pkt).unwrap();
        let json = serde_json::to_string(&head).unwrap();
        let head2: OpusHead = serde_json::from_str(&json).unwrap();
        assert_eq!(head, head2);
    }

    #[test]
    fn find_last_granule_none_on_empty() {
        assert_eq!(find_last_granule(&[]), None);
    }

    #[test]
    fn find_last_granule_finds_page() {
        // Build a fake Ogg page header with known granule
        let mut data = Vec::new();
        data.extend_from_slice(b"OggS");
        data.push(0); // version
        data.push(0x04); // header type (EOS)
        let granule: i64 = 96000;
        data.extend_from_slice(&granule.to_le_bytes());
        // serial, page_seq, crc, segments... pad to make it valid-looking
        data.resize(40, 0);

        assert_eq!(find_last_granule(&data), Some(96000));
    }

    #[test]
    fn opus_tags_reject_short() {
        let pkt = b"Opus";
        assert!(parse_opus_tags(pkt).is_err());
    }

    #[test]
    fn opus_tags_reject_wrong_magic() {
        let pkt = b"NotOpusTags_data";
        assert!(parse_opus_tags(pkt).is_err());
    }

    #[cfg(not(feature = "tag"))]
    #[test]
    fn opus_tags_no_tag_feature() {
        let mut pkt = Vec::new();
        pkt.extend_from_slice(b"OpusTags");
        pkt.extend_from_slice(&[0; 20]);
        assert!(parse_opus_tags(&pkt).is_ok());
    }
}
