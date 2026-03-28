//! Audio metadata tag reading — ID3v2 and Vorbis Comment.

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShravanError};

/// Audio metadata extracted from tags.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioMetadata {
    /// Track title.
    pub title: Option<String>,
    /// Artist name.
    pub artist: Option<String>,
    /// Album name.
    pub album: Option<String>,
    /// Track number.
    pub track_number: Option<String>,
    /// Release year.
    pub year: Option<String>,
    /// Genre.
    pub genre: Option<String>,
    /// Comment.
    pub comment: Option<String>,
}

/// Read ID3v2 tags from the beginning of a file.
///
/// Parses the ID3v2 header and extracts text frames:
/// TIT2 (title), TPE1 (artist), TALB (album), TRCK (track),
/// TDRC/TYER (year), TCON (genre).
///
/// # Errors
///
/// Returns [`ShravanError::InvalidHeader`] if the data does not contain a valid ID3v2 header.
pub fn read_id3v2(data: &[u8]) -> Result<AudioMetadata> {
    // ID3v2 header: "ID3" + version (2 bytes) + flags (1 byte) + size (4 bytes syncsafe)
    if data.len() < 10 {
        return Err(ShravanError::InvalidHeader(
            "too short for ID3v2 header".into(),
        ));
    }
    if &data[0..3] != b"ID3" {
        return Err(ShravanError::InvalidHeader("missing ID3 magic".into()));
    }

    let major_version = data[3];
    let _revision = data[4];
    let _flags = data[5];

    // Syncsafe integer: 4 bytes, 7 bits each
    let tag_size = syncsafe_to_u32(&data[6..10]);

    if data.len() < 10 + tag_size as usize {
        return Err(ShravanError::EndOfStream);
    }

    let mut meta = AudioMetadata::default();
    let tag_data = &data[10..10 + tag_size as usize];

    if major_version == 2 {
        // ID3v2.2: 3-byte frame IDs, 3-byte size
        parse_id3v22_frames(tag_data, &mut meta);
    } else {
        // ID3v2.3 / v2.4: 4-byte frame IDs, 4-byte size
        parse_id3v23_frames(tag_data, &mut meta, major_version);
    }

    Ok(meta)
}

/// Parse ID3v2.2 frames (3-byte IDs).
fn parse_id3v22_frames(data: &[u8], meta: &mut AudioMetadata) {
    let mut pos = 0;
    while pos + 6 <= data.len() {
        let id = &data[pos..pos + 3];
        let size = (u32::from(data[pos + 3]) << 16)
            | (u32::from(data[pos + 4]) << 8)
            | u32::from(data[pos + 5]);
        pos += 6;

        if size == 0 || pos + size as usize > data.len() {
            break;
        }

        let frame_data = &data[pos..pos + size as usize];
        if let Some(text) = extract_text_frame(frame_data) {
            match id {
                b"TT2" => meta.title = Some(text),
                b"TP1" => meta.artist = Some(text),
                b"TAL" => meta.album = Some(text),
                b"TRK" => meta.track_number = Some(text),
                b"TYE" => meta.year = Some(text),
                b"TCO" => meta.genre = Some(text),
                _ => {}
            }
        }
        pos += size as usize;
    }
}

/// Parse ID3v2.3/v2.4 frames (4-byte IDs).
fn parse_id3v23_frames(data: &[u8], meta: &mut AudioMetadata, version: u8) {
    let mut pos = 0;
    while pos + 10 <= data.len() {
        let id = &data[pos..pos + 4];

        // Check for padding (all zeros)
        if id == b"\0\0\0\0" {
            break;
        }

        let size = if version == 4 {
            // v2.4 uses syncsafe integers for frame sizes
            syncsafe_to_u32(&data[pos + 4..pos + 8])
        } else {
            // v2.3 uses regular big-endian u32
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
        };

        let _flags = &data[pos + 8..pos + 10];
        pos += 10;

        if size == 0 || pos + size as usize > data.len() {
            break;
        }

        let frame_data = &data[pos..pos + size as usize];
        if let Some(text) = extract_text_frame(frame_data) {
            match id {
                b"TIT2" => meta.title = Some(text),
                b"TPE1" => meta.artist = Some(text),
                b"TALB" => meta.album = Some(text),
                b"TRCK" => meta.track_number = Some(text),
                b"TDRC" | b"TYER" => meta.year = Some(text),
                b"TCON" => meta.genre = Some(text),
                b"COMM" => {
                    // Comment frames have a different structure, but extract what we can
                    if frame_data.len() > 4 {
                        meta.comment = extract_text_frame(&frame_data[3..]);
                    }
                }
                _ => {}
            }
        }
        pos += size as usize;
    }
}

/// Extract text from an ID3v2 text frame.
///
/// First byte is encoding: 0=ISO-8859-1, 1=UTF-16, 2=UTF-16BE, 3=UTF-8.
fn extract_text_frame(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    let encoding = data[0];
    let text_data = &data[1..];

    match encoding {
        0 | 3 => {
            // ISO-8859-1 or UTF-8
            let s = String::from_utf8_lossy(text_data);
            let trimmed = s.trim_end_matches('\0');
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        1 => {
            // UTF-16 with BOM
            if text_data.len() < 2 {
                return None;
            }
            let is_le = text_data[0] == 0xFF && text_data[1] == 0xFE;
            let raw = &text_data[2..];
            decode_utf16(raw, is_le)
        }
        2 => {
            // UTF-16BE without BOM
            decode_utf16(text_data, false)
        }
        _ => None,
    }
}

/// Decode UTF-16 bytes to a String.
fn decode_utf16(data: &[u8], little_endian: bool) -> Option<String> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| {
            if little_endian {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();

    String::from_utf16(&units)
        .ok()
        .map(|s| {
            let trimmed = s.trim_end_matches('\0');
            trimmed.to_string()
        })
        .filter(|s| !s.is_empty())
}

/// Decode a syncsafe integer (4 bytes, 7 bits each).
fn syncsafe_to_u32(data: &[u8]) -> u32 {
    (u32::from(data[0] & 0x7F) << 21)
        | (u32::from(data[1] & 0x7F) << 14)
        | (u32::from(data[2] & 0x7F) << 7)
        | u32::from(data[3] & 0x7F)
}

/// Read Vorbis Comment tags from a data block.
///
/// Parses the vendor string and field=value comment pairs.
/// Recognized fields: TITLE, ARTIST, ALBUM, TRACKNUMBER, DATE, GENRE, COMMENT.
///
/// # Errors
///
/// Returns [`ShravanError::EndOfStream`] if the data is truncated.
pub fn read_vorbis_comment(data: &[u8]) -> Result<AudioMetadata> {
    if data.len() < 4 {
        return Err(ShravanError::EndOfStream);
    }

    let mut pos = 0;

    // Vendor string length (little-endian u32)
    let vendor_len = read_u32_le(data, pos)? as usize;
    pos += 4;
    if pos + vendor_len > data.len() {
        return Err(ShravanError::EndOfStream);
    }
    pos += vendor_len; // skip vendor string

    // Comment count
    if pos + 4 > data.len() {
        return Err(ShravanError::EndOfStream);
    }
    let comment_count = read_u32_le(data, pos)? as usize;
    pos += 4;

    let mut meta = AudioMetadata::default();

    for _ in 0..comment_count {
        if pos + 4 > data.len() {
            break;
        }
        let comment_len = read_u32_le(data, pos)? as usize;
        pos += 4;
        if pos + comment_len > data.len() {
            break;
        }

        let comment = &data[pos..pos + comment_len];
        pos += comment_len;

        // Parse field=value
        if let Ok(s) = core::str::from_utf8(comment)
            && let Some(eq_pos) = s.find('=')
        {
            let field = &s[..eq_pos];
            let value = &s[eq_pos + 1..];
            if !value.is_empty() {
                match field.to_ascii_uppercase().as_str() {
                    "TITLE" => meta.title = Some(value.to_string()),
                    "ARTIST" => meta.artist = Some(value.to_string()),
                    "ALBUM" => meta.album = Some(value.to_string()),
                    "TRACKNUMBER" => meta.track_number = Some(value.to_string()),
                    "DATE" => meta.year = Some(value.to_string()),
                    "GENRE" => meta.genre = Some(value.to_string()),
                    "COMMENT" => meta.comment = Some(value.to_string()),
                    _ => {}
                }
            }
        }
    }

    Ok(meta)
}

/// Read a little-endian u32.
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn build_id3v2_tag(frames: &[(&[u8; 4], &str)]) -> Vec<u8> {
        let mut frame_data = Vec::new();
        for (id, value) in frames {
            frame_data.extend_from_slice(*id);
            let size = (value.len() + 1) as u32; // +1 for encoding byte
            frame_data.extend_from_slice(&size.to_be_bytes());
            frame_data.extend_from_slice(&[0, 0]); // flags
            frame_data.push(3); // encoding = UTF-8
            frame_data.extend_from_slice(value.as_bytes());
        }

        let tag_size = frame_data.len() as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"ID3");
        out.push(3); // version 2.3
        out.push(0); // revision
        out.push(0); // flags
        // Syncsafe size
        out.push(((tag_size >> 21) & 0x7F) as u8);
        out.push(((tag_size >> 14) & 0x7F) as u8);
        out.push(((tag_size >> 7) & 0x7F) as u8);
        out.push((tag_size & 0x7F) as u8);
        out.extend_from_slice(&frame_data);
        out
    }

    #[test]
    fn id3v2_parse_text_frames() {
        let tag = build_id3v2_tag(&[
            (b"TIT2", "Test Song"),
            (b"TPE1", "Test Artist"),
            (b"TALB", "Test Album"),
            (b"TRCK", "5"),
            (b"TYER", "2025"),
            (b"TCON", "Rock"),
        ]);

        let meta = read_id3v2(&tag).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Test Song"));
        assert_eq!(meta.artist.as_deref(), Some("Test Artist"));
        assert_eq!(meta.album.as_deref(), Some("Test Album"));
        assert_eq!(meta.track_number.as_deref(), Some("5"));
        assert_eq!(meta.year.as_deref(), Some("2025"));
        assert_eq!(meta.genre.as_deref(), Some("Rock"));
    }

    #[test]
    fn id3v2_rejects_non_id3() {
        assert!(read_id3v2(b"RIFF data here").is_err());
    }

    #[test]
    fn id3v2_rejects_short() {
        assert!(read_id3v2(b"ID3").is_err());
    }

    fn build_vorbis_comment(vendor: &str, comments: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        // Vendor string
        out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        out.extend_from_slice(vendor.as_bytes());
        // Comment count
        out.extend_from_slice(&(comments.len() as u32).to_le_bytes());
        for (field, value) in comments {
            let comment = format!("{field}={value}");
            out.extend_from_slice(&(comment.len() as u32).to_le_bytes());
            out.extend_from_slice(comment.as_bytes());
        }
        out
    }

    #[test]
    fn vorbis_comment_parse() {
        let data = build_vorbis_comment(
            "shravan 0.1.0",
            &[
                ("TITLE", "Test Track"),
                ("ARTIST", "Test Musician"),
                ("ALBUM", "Test Record"),
                ("TRACKNUMBER", "3"),
                ("DATE", "2025"),
                ("GENRE", "Electronic"),
                ("COMMENT", "A test comment"),
            ],
        );

        let meta = read_vorbis_comment(&data).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Test Track"));
        assert_eq!(meta.artist.as_deref(), Some("Test Musician"));
        assert_eq!(meta.album.as_deref(), Some("Test Record"));
        assert_eq!(meta.track_number.as_deref(), Some("3"));
        assert_eq!(meta.year.as_deref(), Some("2025"));
        assert_eq!(meta.genre.as_deref(), Some("Electronic"));
        assert_eq!(meta.comment.as_deref(), Some("A test comment"));
    }

    #[test]
    fn vorbis_comment_case_insensitive() {
        let data =
            build_vorbis_comment("test", &[("title", "Lower Case"), ("Artist", "Mixed Case")]);

        let meta = read_vorbis_comment(&data).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Lower Case"));
        assert_eq!(meta.artist.as_deref(), Some("Mixed Case"));
    }

    #[test]
    fn vorbis_comment_rejects_short() {
        assert!(read_vorbis_comment(b"").is_err());
        assert!(read_vorbis_comment(b"\x00\x01").is_err());
    }

    #[test]
    fn syncsafe_decode() {
        assert_eq!(syncsafe_to_u32(&[0x00, 0x00, 0x02, 0x01]), 257);
        assert_eq!(syncsafe_to_u32(&[0x00, 0x00, 0x00, 0x00]), 0);
        assert_eq!(syncsafe_to_u32(&[0x7F, 0x7F, 0x7F, 0x7F]), 0x0FFFFFFF);
    }
}
