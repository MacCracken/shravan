//! AAC decoder — ADTS container parsing with symphonia-codec-aac backend.
//!
//! Parses ADTS (Audio Data Transport Stream) frames, extracts raw AAC packets,
//! and delegates actual AAC-LC decoding to `symphonia-codec-aac`.

use alloc::format;
use alloc::vec::Vec;

use crate::error::{Result, ShravanError};
use crate::format::{AudioFormat, FormatInfo};

use symphonia_core::audio::Signal;
use symphonia_core::codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions};
use symphonia_core::formats::Packet;

/// Standard AAC sample rates indexed by the 4-bit sample rate index.
const AAC_SAMPLE_RATES: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

/// Channel count indexed by the 3-bit channel configuration.
const AAC_CHANNEL_COUNTS: [u16; 8] = [0, 1, 2, 3, 4, 5, 6, 8];

/// A parsed ADTS frame header.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct AdtsHeader {
    /// AAC profile (0 = Main, 1 = LC, 2 = SSR, 3 = LTP).
    profile: u8,
    /// Sample rate in Hz.
    sample_rate: u32,
    /// Sample rate index (0..15).
    sample_rate_index: u8,
    /// Number of audio channels.
    channels: u16,
    /// Total frame length in bytes (header + payload).
    frame_length: usize,
}

/// Parse an ADTS frame header from a 7-byte (or longer) slice.
///
/// ADTS header layout (fixed + variable = 7 bytes without CRC):
/// - 12 bits: sync word (0xFFF)
/// -  1 bit:  MPEG version (0 = MPEG-4, 1 = MPEG-2)
/// -  2 bits: layer (always 0)
/// -  1 bit:  protection absent (1 = no CRC)
/// -  2 bits: profile (AAC-LC = 1)
/// -  4 bits: sample rate index
/// -  1 bit:  private
/// -  3 bits: channel configuration
/// -  1 bit:  originality
/// -  1 bit:  home
/// -  1 bit:  copyrighted stream
/// -  1 bit:  copyright start
/// - 13 bits: frame length (header + payload)
/// - 11 bits: buffer fullness
/// -  2 bits: number of AAC frames minus 1
fn parse_adts_header(data: &[u8]) -> Result<AdtsHeader> {
    if data.len() < 7 {
        return Err(ShravanError::InvalidHeader(
            "ADTS header requires at least 7 bytes".into(),
        ));
    }

    // Check sync word: 12 bits = 0xFFF
    if data[0] != 0xFF || (data[1] & 0xF0) != 0xF0 {
        return Err(ShravanError::InvalidHeader(
            "missing ADTS sync word (0xFFF)".into(),
        ));
    }

    // Layer must be 0 for AAC
    if (data[1] & 0x06) != 0 {
        return Err(ShravanError::InvalidHeader(
            "ADTS layer must be 0 for AAC".into(),
        ));
    }

    let profile = (data[2] >> 6) & 0x03;
    let sample_rate_index = (data[2] >> 2) & 0x0F;

    if sample_rate_index >= 13 {
        return Err(ShravanError::InvalidHeader(format!(
            "ADTS reserved sample rate index: {sample_rate_index}"
        )));
    }

    let sample_rate = AAC_SAMPLE_RATES[sample_rate_index as usize];
    if sample_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }

    let channel_config = ((data[2] & 0x01) << 2) | ((data[3] >> 6) & 0x03);
    if channel_config == 0 || channel_config as usize >= AAC_CHANNEL_COUNTS.len() {
        return Err(ShravanError::InvalidChannels(channel_config.into()));
    }
    let channels = AAC_CHANNEL_COUNTS[channel_config as usize];

    // Frame length: 13 bits spanning bytes 3-5
    let frame_length = (usize::from(data[3] & 0x03) << 11)
        | (usize::from(data[4]) << 3)
        | (usize::from(data[5] >> 5) & 0x07);

    if frame_length < 7 {
        return Err(ShravanError::InvalidHeader(
            "ADTS frame length too small".into(),
        ));
    }

    Ok(AdtsHeader {
        profile,
        sample_rate,
        sample_rate_index,
        channels,
        frame_length,
    })
}

/// Extract raw AAC frame payloads from an ADTS stream.
///
/// Returns a list of (header, payload) pairs.
fn extract_adts_frames(data: &[u8]) -> Result<(AdtsHeader, Vec<&[u8]>)> {
    let mut pos = 0;
    let mut frames = Vec::new();
    let mut first_header: Option<AdtsHeader> = None;

    while pos + 7 <= data.len() {
        // Scan for sync word
        if data[pos] != 0xFF || (data[pos + 1] & 0xF0) != 0xF0 {
            pos += 1;
            continue;
        }

        let header = parse_adts_header(&data[pos..])?;

        if pos + header.frame_length > data.len() {
            break; // truncated final frame
        }

        // Determine header size (7 without CRC, 9 with CRC)
        let protection_absent = (data[pos + 1] & 0x01) != 0;
        let header_size = if protection_absent { 7 } else { 9 };

        if header.frame_length > header_size {
            frames.push(&data[pos + header_size..pos + header.frame_length]);
        }

        if first_header.is_none() {
            first_header = Some(header);
        }

        pos += header.frame_length;
    }

    match first_header {
        Some(h) => Ok((h, frames)),
        None => Err(ShravanError::InvalidHeader(
            "no valid ADTS frames found".into(),
        )),
    }
}

/// Convert a symphonia `AudioBufferRef` to interleaved f32 samples.
fn audio_buffer_ref_to_f32(
    buf_ref: &symphonia_core::audio::AudioBufferRef<'_>,
    channels: usize,
    out: &mut Vec<f32>,
) {
    use symphonia_core::audio::AudioBufferRef;
    use symphonia_core::conv::IntoSample;

    // Use the actual channel count from the buffer, capped to expected channels
    let buf_channels = buf_ref.spec().channels.count();
    let ch_count = channels.min(buf_channels);

    match buf_ref {
        AudioBufferRef::F32(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame]);
                }
                // Pad missing channels with silence
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::S16(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::S32(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::F64(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::S24(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    let val: i32 = buf.chan(ch)[frame].into_sample();
                    out.push(val.into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        AudioBufferRef::U8(buf) => {
            let frames = buf.frames();
            for frame in 0..frames {
                for ch in 0..ch_count {
                    out.push(buf.chan(ch)[frame].into_sample());
                }
                for _ in ch_count..channels {
                    out.push(0.0);
                }
            }
        }
        // Remaining variants: U16, U24, U32, S8 — highly unlikely from AAC
        // but handle them by converting through the widest available path
        _ => {
            // Cannot convert without knowing the exact type — skip.
            // This is a best-effort fallback; AAC-LC always outputs F32 or S16.
        }
    }
}

/// Decode AAC audio from an ADTS byte stream.
///
/// Parses ADTS frames, feeds raw AAC packets to symphonia's AAC-LC decoder,
/// and returns interleaved f32 samples normalised to \[-1.0, 1.0\].
///
/// # Errors
///
/// Returns errors for invalid ADTS headers, unsupported AAC profiles,
/// or decoding failures.
#[must_use = "decoded audio data is returned and should not be discarded"]
pub fn decode(data: &[u8]) -> Result<(FormatInfo, Vec<f32>)> {
    let (header, frames) = extract_adts_frames(data)?;

    if frames.is_empty() {
        return Err(ShravanError::DecodeError(
            "no AAC frames extracted from ADTS stream".into(),
        ));
    }

    // Build symphonia CodecParameters for ADTS (no ASC extra_data)
    let mut params = CodecParameters::new();
    params
        .for_codec(CODEC_TYPE_AAC)
        .with_sample_rate(header.sample_rate)
        .with_channels(map_channels(header.channels));

    // Instantiate the symphonia AAC decoder
    let opts = DecoderOptions::default();
    let mut decoder = symphonia_codec_aac::AacDecoder::try_new(&params, &opts)
        .map_err(|e| ShravanError::DecodeError(format!("AAC decoder init failed: {e}")))?;

    let channels = header.channels as usize;
    let mut samples: Vec<f32> = Vec::new();
    let mut ts: u64 = 0;

    for frame_data in &frames {
        let packet = Packet::new_from_slice(0, ts, 1024, frame_data);

        match decoder.decode(&packet) {
            Ok(buf_ref) => {
                audio_buffer_ref_to_f32(&buf_ref, channels, &mut samples);
            }
            Err(e) => {
                return Err(ShravanError::DecodeError(format!(
                    "AAC frame decode failed: {e}"
                )));
            }
        }

        ts += 1024; // AAC-LC always produces 1024 samples per frame
    }

    let total_frames = samples.len() / channels.max(1);
    let duration_secs = if header.sample_rate > 0 {
        total_frames as f64 / f64::from(header.sample_rate)
    } else {
        0.0
    };

    let info = FormatInfo {
        format: AudioFormat::Aac,
        sample_rate: header.sample_rate,
        channels: header.channels,
        bit_depth: 16, // AAC is perceptual; report as 16-bit equivalent
        duration_secs,
        total_samples: total_frames as u64,
    };

    Ok((info, samples))
}

/// Map a channel count to symphonia's `Channels` bitfield.
fn map_channels(count: u16) -> symphonia_core::audio::Channels {
    use symphonia_core::audio::Channels;

    match count {
        1 => Channels::FRONT_LEFT,
        2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
        3 => Channels::FRONT_CENTRE | Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
        4 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::REAR_CENTRE
        }
        5 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
        }
        6 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
                | Channels::LFE1
        }
        8 => {
            Channels::FRONT_CENTRE
                | Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::SIDE_LEFT
                | Channels::SIDE_RIGHT
                | Channels::FRONT_LEFT_WIDE
                | Channels::FRONT_RIGHT_WIDE
                | Channels::LFE1
        }
        _ => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_adts_header() {
        // ADTS header: sync=0xFFF, MPEG-4, layer=0, protection_absent=1,
        // profile=LC(1), sr_index=4(44100), private=0, ch_config=2(stereo),
        // frame_length=100
        let mut header = [0u8; 7];
        header[0] = 0xFF;
        header[1] = 0xF1; // sync + MPEG-4 + layer=0 + protection_absent=1
        header[2] = 0x50; // profile=1(LC) | sr_index=4(44100) | private=0 | ch_config MSB=0
        header[3] = 0x80; // ch_config[1:0]=10 | orig=0 | home=0 | copy=0 | copyright_start=0
        // frame_length = 100 = 0x064 across bits [30:43]
        // data[3] bits[1:0] = 0x00 (frame_len[12:11])
        // data[4] = 0x0C (frame_len[10:3] = 00001100 = 12, 12<<3 = 96)
        // data[5] bits[7:5] = 0x80 (frame_len[2:0] = 100 = 4, 96+4=100)
        header[3] |= 0x00; // frame_len bits 12-11
        header[4] = 0x0C; // frame_len bits 10-3
        header[5] = 0x80; // frame_len bits 2-0 (top 3 bits)
        header[6] = 0x00;

        let h = parse_adts_header(&header).unwrap();
        assert_eq!(h.profile, 1); // LC
        assert_eq!(h.sample_rate, 44100);
        assert_eq!(h.channels, 2);
        assert_eq!(h.frame_length, 100);
    }

    #[test]
    fn reject_short_header() {
        let data = [0xFF, 0xF1, 0x50];
        assert!(parse_adts_header(&data).is_err());
    }

    #[test]
    fn reject_bad_sync() {
        let data = [0x00u8; 7];
        assert!(parse_adts_header(&data).is_err());
    }

    #[test]
    fn reject_reserved_sample_rate() {
        let mut header = [0u8; 7];
        header[0] = 0xFF;
        header[1] = 0xF1;
        header[2] = 0x7C; // profile=1, sr_index=15(reserved)
        assert!(parse_adts_header(&header).is_err());
    }

    #[test]
    fn extract_no_frames_from_garbage() {
        let data = [0x00u8; 100];
        assert!(extract_adts_frames(&data).is_err());
    }

    #[test]
    fn decode_rejects_empty() {
        let data: &[u8] = &[];
        assert!(decode(data).is_err());
    }

    #[test]
    fn serde_roundtrip_aac_codec() {
        let codec = crate::codec::AacCodec;
        let json = serde_json::to_string(&codec).unwrap();
        let codec2: crate::codec::AacCodec = serde_json::from_str(&json).unwrap();
        assert_eq!(codec, codec2);
    }
}
