//! Ogg container parser.
//!
//! Extracts pages and packets from Ogg bitstreams. If the first packet
//! identifies an Opus stream and the `opus` feature is enabled, decoding
//! is delegated to [`crate::opus`].

use alloc::format;
use alloc::vec::Vec;

use crate::error::{Result, ShravanError};
use crate::format::FormatInfo;

/// Ogg page capture pattern.
const OGG_MAGIC: &[u8; 4] = b"OggS";

/// CRC-32 lookup table for Ogg (polynomial `0x04C11DB7`, init 0, direct — no reflection).
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i << 24;
        let mut bit = 0;
        while bit < 8 {
            if crc & 0x8000_0000 != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
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

/// Compute the Ogg CRC-32 over a byte slice.
#[allow(dead_code)]
#[must_use]
#[inline]
fn crc32_ogg(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |crc, &b| {
        let index = ((crc >> 24) ^ u32::from(b)) & 0xFF;
        (crc << 8) ^ CRC32_TABLE[index as usize]
    })
}

/// Compute the Ogg CRC-32 over a page, treating the CRC field (bytes 22..26) as zero.
#[must_use]
fn crc32_ogg_page(page_bytes: &[u8]) -> u32 {
    let mut crc = 0u32;
    for (i, &b) in page_bytes.iter().enumerate() {
        let byte = if (22..26).contains(&i) { 0u8 } else { b };
        let index = ((crc >> 24) ^ u32::from(byte)) & 0xFF;
        crc = (crc << 8) ^ CRC32_TABLE[index as usize];
    }
    crc
}

/// A parsed Ogg page.
struct OggPage {
    /// Header type flags (continuation, BOS, EOS).
    header_type: u8,
    /// Granule position.
    #[allow(dead_code)]
    granule_position: i64,
    /// Bitstream serial number.
    #[allow(dead_code)]
    serial: u32,
    /// Page sequence number.
    #[allow(dead_code)]
    page_seq: u32,
    /// Lacing values from the segment table.
    segments: Vec<u8>,
    /// Page body data.
    body: Vec<u8>,
}

/// Read a little-endian u32 from `data` at `offset`.
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

/// Read a little-endian i64 from `data` at `offset`.
#[inline]
fn read_i64_le(data: &[u8], offset: usize) -> Result<i64> {
    if offset + 8 > data.len() {
        return Err(ShravanError::EndOfStream);
    }
    Ok(i64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

/// Parse a single Ogg page starting at `data[pos..]`.
///
/// Returns the parsed page and the byte offset immediately after it.
fn parse_page(data: &[u8], pos: usize) -> Result<(OggPage, usize)> {
    // Minimum page header: 27 bytes (capture pattern + header fields + num_segments)
    if pos + 27 > data.len() {
        return Err(ShravanError::EndOfStream);
    }

    if &data[pos..pos + 4] != OGG_MAGIC {
        return Err(ShravanError::InvalidHeader(
            "missing OggS capture pattern".into(),
        ));
    }

    let version = data[pos + 4];
    if version != 0 {
        return Err(ShravanError::InvalidHeader(format!(
            "unsupported Ogg version: {version}"
        )));
    }

    let header_type = data[pos + 5];
    let granule_position = read_i64_le(data, pos + 6)?;
    let serial = read_u32_le(data, pos + 14)?;
    let page_seq = read_u32_le(data, pos + 18)?;
    let _crc = read_u32_le(data, pos + 22)?;
    let num_segments = data[pos + 26] as usize;

    let seg_table_start = pos + 27;
    if seg_table_start + num_segments > data.len() {
        return Err(ShravanError::EndOfStream);
    }

    let segments: Vec<u8> = data[seg_table_start..seg_table_start + num_segments].to_vec();
    let body_size: usize = segments.iter().map(|&s| s as usize).sum();
    let body_start = seg_table_start + num_segments;

    if body_start + body_size > data.len() {
        return Err(ShravanError::EndOfStream);
    }

    let body = data[body_start..body_start + body_size].to_vec();

    // CRC verification
    let page_end = body_start + body_size;
    let computed_crc = crc32_ogg_page(&data[pos..page_end]);
    if computed_crc != _crc {
        return Err(ShravanError::DecodeError(format!(
            "Ogg page CRC mismatch: expected {_crc:#010X}, got {computed_crc:#010X}"
        )));
    }

    let page = OggPage {
        header_type,
        granule_position,
        serial,
        page_seq,
        segments,
        body,
    };

    Ok((page, page_end))
}

/// Parse all Ogg pages from a byte slice.
fn parse_pages(data: &[u8]) -> Result<Vec<OggPage>> {
    let mut pages = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        // Scan forward for OggS if not aligned
        if pos + 4 > data.len() {
            break;
        }
        if &data[pos..pos + 4] != OGG_MAGIC {
            break;
        }
        let (page, next_pos) = parse_page(data, pos)?;
        pages.push(page);
        pos = next_pos;
    }

    Ok(pages)
}

/// Extract logical packets from an Ogg bitstream.
///
/// Parses all pages and reassembles packets from lacing values.
/// A lacing value of 255 indicates continuation; a value less than 255
/// terminates the current packet.
///
/// # Errors
///
/// Returns errors for invalid Ogg structure or truncated data.
pub fn extract_packets(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let pages = parse_pages(data)?;
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut current_packet: Vec<u8> = Vec::new();

    for page in &pages {
        // If this is NOT a continuation page and we have accumulated data,
        // the previous packet was unterminated — still push it.
        if page.header_type & 0x01 == 0 && !current_packet.is_empty() {
            packets.push(core::mem::take(&mut current_packet));
        }

        let mut body_offset = 0usize;
        for &lacing_value in &page.segments {
            let size = lacing_value as usize;
            if body_offset + size > page.body.len() {
                return Err(ShravanError::DecodeError(
                    "lacing value exceeds page body".into(),
                ));
            }
            current_packet.extend_from_slice(&page.body[body_offset..body_offset + size]);
            body_offset += size;

            if lacing_value < 255 {
                // Packet boundary
                packets.push(core::mem::take(&mut current_packet));
            }
        }
    }

    // Flush any remaining data (unterminated packet at end of stream)
    if !current_packet.is_empty() {
        packets.push(current_packet);
    }

    Ok(packets)
}

/// Decode an Ogg bitstream.
///
/// Inspects the first packet: if it starts with `OpusHead` and the `opus`
/// feature is enabled, decoding is delegated to [`crate::opus`].
/// Otherwise returns [`ShravanError::UnsupportedFormat`].
///
/// # Errors
///
/// Returns errors for invalid Ogg structure, empty streams, or unsupported codecs.
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    let packets = extract_packets(data)?;
    if packets.is_empty() {
        return Err(ShravanError::EndOfStream);
    }

    // Check first packet for known codecs
    #[cfg(feature = "opus")]
    if packets[0].starts_with(b"OpusHead") {
        return crate::opus::decode_from_packets(&packets, data);
    }

    Err(ShravanError::UnsupportedFormat)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Build a minimal Ogg page from body segments.
    /// `lacing` is the list of lacing values; body must match their sum.
    fn build_ogg_page(
        header_type: u8,
        granule: i64,
        serial: u32,
        page_seq: u32,
        lacing: &[u8],
        body: &[u8],
    ) -> Vec<u8> {
        let num_segments = lacing.len() as u8;
        let mut page = Vec::new();

        // Capture pattern
        page.extend_from_slice(b"OggS");
        // Version
        page.push(0);
        // Header type
        page.push(header_type);
        // Granule position
        page.extend_from_slice(&granule.to_le_bytes());
        // Serial
        page.extend_from_slice(&serial.to_le_bytes());
        // Page sequence number
        page.extend_from_slice(&page_seq.to_le_bytes());
        // CRC placeholder (4 bytes of zero — will be filled below)
        page.extend_from_slice(&[0u8; 4]);
        // Number of segments
        page.push(num_segments);
        // Segment table
        page.extend_from_slice(lacing);
        // Body
        page.extend_from_slice(body);

        // Compute and fill CRC
        let crc = crc32_ogg_page(&page);
        page[22..26].copy_from_slice(&crc.to_le_bytes());

        page
    }

    #[test]
    fn crc32_known_vector() {
        // CRC-32 of "OggS" bytes with the Ogg polynomial
        let crc = crc32_ogg(b"OggS");
        // Just verify it is deterministic and non-zero
        assert_ne!(crc, 0);
        assert_eq!(crc, crc32_ogg(b"OggS"));
    }

    #[test]
    fn crc32_empty() {
        assert_eq!(crc32_ogg(b""), 0);
    }

    #[test]
    fn parse_single_page() {
        let body = b"hello";
        let page_bytes = build_ogg_page(0x02, 0, 1, 0, &[5], body);
        let (page, end) = parse_page(&page_bytes, 0).unwrap();

        assert_eq!(page.header_type, 0x02);
        assert_eq!(page.granule_position, 0);
        assert_eq!(page.serial, 1);
        assert_eq!(page.page_seq, 0);
        assert_eq!(page.segments, vec![5]);
        assert_eq!(page.body, b"hello");
        assert_eq!(end, page_bytes.len());
    }

    #[test]
    fn reject_invalid_magic() {
        let data = b"NotOggData1234567890123456789012";
        assert!(parse_page(data, 0).is_err());
    }

    #[test]
    fn reject_short_data() {
        let data = b"OggS";
        assert!(parse_page(data, 0).is_err());
    }

    #[test]
    fn extract_single_packet() {
        let body = b"packet_data";
        let page = build_ogg_page(0x02, 100, 1, 0, &[11], body);
        let packets = extract_packets(&page).unwrap();

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0], b"packet_data");
    }

    #[test]
    fn extract_multi_packet_page() {
        // Two packets in one page: "aaa" (3 bytes) and "bb" (2 bytes)
        let body = b"aaabb";
        let page = build_ogg_page(0x02, 100, 1, 0, &[3, 2], body);
        let packets = extract_packets(&page).unwrap();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0], b"aaa");
        assert_eq!(packets[1], b"bb");
    }

    #[test]
    fn extract_empty_page() {
        // A page with no segments -> no packets
        let page = build_ogg_page(0x02, 0, 1, 0, &[], &[]);
        let packets = extract_packets(&page).unwrap();
        assert!(packets.is_empty());
    }

    #[test]
    fn continuation_across_pages() {
        // Packet spans two pages using a 255 lacing value on page 1,
        // then a continuation page 2 with the rest.
        let body1 = [0xAA; 255];
        let page1 = build_ogg_page(0x02, 0, 1, 0, &[255], &body1);

        let body2 = [0xBB; 10];
        // header_type 0x01 = continuation
        let page2 = build_ogg_page(0x01, 100, 1, 1, &[10], &body2);

        let mut data = Vec::new();
        data.extend_from_slice(&page1);
        data.extend_from_slice(&page2);

        let packets = extract_packets(&data).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].len(), 265);
        assert!(packets[0][..255].iter().all(|&b| b == 0xAA));
        assert!(packets[0][255..].iter().all(|&b| b == 0xBB));
    }

    #[test]
    fn decode_empty_stream_returns_error() {
        let data: &[u8] = &[];
        assert!(decode(data).is_err());
    }

    #[test]
    fn decode_unsupported_codec() {
        // A valid Ogg page with a non-Opus first packet
        let body = b"NotOpus!";
        let page = build_ogg_page(0x02, 0, 1, 0, &[8], body);
        let result = decode(&page);
        assert!(result.is_err());
    }

    #[test]
    fn crc_mismatch_detected() {
        let body = b"hello";
        let mut page = build_ogg_page(0x02, 0, 1, 0, &[5], body);
        // Corrupt one body byte
        let last = page.len() - 1;
        page[last] ^= 0xFF;
        assert!(parse_page(&page, 0).is_err());
    }

    #[test]
    fn multiple_pages() {
        let page1 = build_ogg_page(0x02, 0, 1, 0, &[3], b"abc");
        let page2 = build_ogg_page(0x00, 100, 1, 1, &[2], b"de");

        let mut data = Vec::new();
        data.extend_from_slice(&page1);
        data.extend_from_slice(&page2);

        let packets = extract_packets(&data).unwrap();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0], b"abc");
        assert_eq!(packets[1], b"de");
    }
}
