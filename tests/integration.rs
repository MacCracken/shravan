//! Integration tests for shravan.

use shravan::format::{AudioFormat, FormatInfo, detect_format};

#[cfg(feature = "pcm")]
use shravan::pcm::{self, PcmFormat};

#[cfg(feature = "wav")]
use shravan::wav;

#[cfg(feature = "resample")]
use shravan::resample::{self, ResampleQuality};

#[cfg(feature = "tag")]
use shravan::tag::AudioMetadata;

// --- WAV roundtrip tests ---

#[cfg(all(feature = "wav", feature = "pcm"))]
#[test]
fn wav_i16_roundtrip() {
    let samples: Vec<f32> = vec![0.0, 0.5, -0.5, 0.99, -0.99];
    let encoded = wav::encode(&samples, 44100, 1, PcmFormat::I16).unwrap();
    let (info, decoded) = wav::decode(&encoded).unwrap();

    assert_eq!(info.format, AudioFormat::Wav);
    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.channels, 1);
    assert_eq!(info.bit_depth, 16);
    assert_eq!(decoded.len(), samples.len());

    for (a, b) in samples.iter().zip(decoded.iter()) {
        assert!((a - b).abs() < 0.001, "i16 roundtrip mismatch: {a} vs {b}");
    }
}

#[cfg(all(feature = "wav", feature = "pcm"))]
#[test]
fn wav_f32_roundtrip() {
    let samples: Vec<f32> = vec![0.0, 0.25, -0.25, 0.99, -0.99];
    let encoded = wav::encode(&samples, 48000, 1, PcmFormat::F32).unwrap();
    let (info, decoded) = wav::decode(&encoded).unwrap();

    assert_eq!(info.format, AudioFormat::Wav);
    assert_eq!(info.sample_rate, 48000);
    assert_eq!(info.bit_depth, 32);

    for (a, b) in samples.iter().zip(decoded.iter()) {
        assert!(
            (a - b).abs() < f32::EPSILON,
            "f32 roundtrip mismatch: {a} vs {b}"
        );
    }
}

#[cfg(all(feature = "wav", feature = "pcm"))]
#[test]
fn wav_header_parsing() {
    let samples = vec![0.0f32; 44100]; // 1 second mono
    let encoded = wav::encode(&samples, 44100, 1, PcmFormat::I16).unwrap();
    let (info, _) = wav::decode(&encoded).unwrap();

    assert_eq!(info.sample_rate, 44100);
    assert_eq!(info.channels, 1);
    assert_eq!(info.bit_depth, 16);
    assert_eq!(info.total_samples, 44100);
    assert!((info.duration_secs - 1.0).abs() < 0.001);
}

// --- Format detection ---

#[test]
fn format_detection_wav() {
    let header = b"RIFF\x00\x00\x00\x00WAVE";
    assert_eq!(detect_format(header).unwrap(), AudioFormat::Wav);
}

#[test]
fn format_detection_flac() {
    let header = b"fLaC\x00\x00\x00\x22";
    assert_eq!(detect_format(header).unwrap(), AudioFormat::Flac);
}

#[test]
fn format_detection_unknown() {
    let header = b"\x00\x00\x00\x00extra";
    assert!(detect_format(header).is_err());
}

// --- PCM conversion tests ---

#[cfg(feature = "pcm")]
#[test]
fn pcm_i16_f32_roundtrip() {
    let original: Vec<i16> = vec![0, 16384, -16384, 32767, -32768];
    let f32s = pcm::i16_to_f32(&original);
    let back = pcm::f32_to_i16(&f32s);
    for (a, b) in original.iter().zip(back.iter()) {
        assert!((*a as i32 - *b as i32).abs() <= 1, "{a} != {b}");
    }
}

#[cfg(feature = "pcm")]
#[test]
fn pcm_i24_f32_roundtrip() {
    let original: Vec<i32> = vec![0, 4_194_304, -4_194_304, 8_388_607, -8_388_608];
    let f32s = pcm::i24_to_f32(&original);
    let back = pcm::f32_to_i24(&f32s);
    for (a, b) in original.iter().zip(back.iter()) {
        assert!((*a - *b).abs() <= 1, "{a} != {b}");
    }
}

#[cfg(feature = "pcm")]
#[test]
fn pcm_interleave_deinterleave_roundtrip() {
    let left = vec![1.0f32, 3.0, 5.0];
    let right = vec![2.0f32, 4.0, 6.0];
    let interleaved = pcm::interleave(&[&left, &right]);
    assert_eq!(interleaved, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);

    let planes = pcm::deinterleave(&interleaved, 2);
    assert_eq!(planes.len(), 2);
    assert_eq!(planes[0], left);
    assert_eq!(planes[1], right);
}

// --- Resample tests ---

#[cfg(feature = "resample")]
#[test]
fn resample_44100_48000_44100_roundtrip() {
    let sr = 44100u32;
    let frames = 4096;
    let samples: Vec<f32> = (0..frames)
        .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
        .collect();

    let rms_orig = rms(&samples);

    let up = resample::resample(&samples, 1, 44100, 48000, ResampleQuality::Good).unwrap();
    let back = resample::resample(&up, 1, 48000, 44100, ResampleQuality::Good).unwrap();

    let rms_back = rms(&back);
    assert!(
        (rms_back - rms_orig).abs() < rms_orig * 0.1,
        "Round-trip RMS: {rms_back} vs original: {rms_orig}"
    );
}

#[cfg(feature = "resample")]
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    libm::sqrt(sum / samples.len() as f64) as f32
}

// --- Serde roundtrip tests ---

#[test]
fn serde_roundtrip_format_info() {
    let info = FormatInfo {
        format: AudioFormat::Wav,
        sample_rate: 44100,
        channels: 2,
        bit_depth: 16,
        duration_secs: 3.5,
        total_samples: 154350,
    };
    let json = serde_json::to_string(&info).unwrap();
    let back: FormatInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn serde_roundtrip_audio_format() {
    for fmt in [AudioFormat::Wav, AudioFormat::Flac, AudioFormat::RawPcm] {
        let json = serde_json::to_string(&fmt).unwrap();
        let back: AudioFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, back);
    }
}

#[cfg(feature = "pcm")]
#[test]
fn serde_roundtrip_pcm_format() {
    for fmt in [
        PcmFormat::I8,
        PcmFormat::I16,
        PcmFormat::I24,
        PcmFormat::I32,
        PcmFormat::F32,
        PcmFormat::F64,
    ] {
        let json = serde_json::to_string(&fmt).unwrap();
        let back: PcmFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(fmt, back);
    }
}

#[cfg(feature = "tag")]
#[test]
fn serde_roundtrip_audio_metadata() {
    let meta = AudioMetadata {
        title: Some("Test".into()),
        artist: Some("Artist".into()),
        album: Some("Album".into()),
        track_number: Some("1".into()),
        year: Some("2025".into()),
        genre: Some("Rock".into()),
        comment: Some("A comment".into()),
    };
    let json = serde_json::to_string(&meta).unwrap();
    let back: AudioMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, back);
}

#[cfg(feature = "resample")]
#[test]
fn serde_roundtrip_resample_quality() {
    for q in [
        ResampleQuality::Draft,
        ResampleQuality::Good,
        ResampleQuality::Best,
    ] {
        let json = serde_json::to_string(&q).unwrap();
        let back: ResampleQuality = serde_json::from_str(&json).unwrap();
        assert_eq!(q, back);
    }
}

// --- Codec auto-detect ---

#[cfg(all(feature = "wav", feature = "pcm"))]
#[test]
fn codec_open_wav() {
    let samples = vec![0.5f32; 100];
    let encoded = wav::encode(&samples, 44100, 1, PcmFormat::I16).unwrap();
    let (info, decoded) = shravan::codec::open(&encoded).unwrap();
    assert_eq!(info.format, AudioFormat::Wav);
    assert_eq!(decoded.len(), 100);
}

#[test]
fn codec_open_unknown() {
    let data = vec![0u8; 100];
    assert!(shravan::codec::open(&data).is_err());
}

// --- Serde roundtrip: codec structs ---

#[cfg(feature = "wav")]
#[test]
fn serde_roundtrip_wav_codec() {
    use shravan::codec::WavCodec;
    let codec = WavCodec;
    let json = serde_json::to_string(&codec).unwrap();
    let back: WavCodec = serde_json::from_str(&json).unwrap();
    assert_eq!(codec, back);
}

#[cfg(feature = "flac")]
#[test]
fn serde_roundtrip_flac_codec() {
    use shravan::codec::FlacCodec;
    let codec = FlacCodec;
    let json = serde_json::to_string(&codec).unwrap();
    let back: FlacCodec = serde_json::from_str(&json).unwrap();
    assert_eq!(codec, back);
}

#[test]
fn serde_roundtrip_shravan_error() {
    use shravan::ShravanError;
    let errors = [
        ShravanError::UnsupportedFormat,
        ShravanError::InvalidHeader("bad header".into()),
        ShravanError::DecodeError("decode failed".into()),
        ShravanError::EncodeError("encode failed".into()),
        ShravanError::EndOfStream,
        ShravanError::InvalidSampleRate(0),
        ShravanError::InvalidChannels(0),
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: ShravanError = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{err}"), format!("{back}"));
    }
}

// --- WAV edge case tests ---

#[cfg(all(feature = "wav", feature = "pcm"))]
#[test]
fn wav_i8_roundtrip() {
    let samples = vec![0.0f32, 0.5, -0.5, -1.0];
    let encoded = wav::encode(&samples, 44100, 1, PcmFormat::I8).unwrap();
    let (info, decoded) = wav::decode(&encoded).unwrap();

    assert_eq!(info.bit_depth, 8);
    assert_eq!(decoded.len(), samples.len());
    for (a, b) in samples.iter().zip(decoded.iter()) {
        assert!((a - b).abs() < 0.02, "i8 roundtrip mismatch: {a} vs {b}");
    }
}

#[cfg(all(feature = "wav", feature = "pcm"))]
#[test]
fn wav_malformed_chunk_size() {
    // WAV with garbage chunk size should not panic
    let mut data = vec![0u8; 100];
    data[0..4].copy_from_slice(b"RIFF");
    data[4..8].copy_from_slice(&96u32.to_le_bytes());
    data[8..12].copy_from_slice(b"WAVE");
    // Fake chunk with max u32 size
    data[12..16].copy_from_slice(b"JUNK");
    data[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    // Should not panic, just return an error
    let _ = wav::decode(&data);
}

// --- PCM edge case tests ---

#[cfg(feature = "pcm")]
#[test]
fn pcm_deinterleave_empty_channels() {
    let planes = shravan::pcm::deinterleave(&[1.0, 2.0, 3.0], 0);
    assert!(planes.is_empty());
}

#[cfg(feature = "pcm")]
#[test]
fn pcm_interleave_empty() {
    let result = shravan::pcm::interleave(&[]);
    assert!(result.is_empty());
}

// --- Resample edge case tests ---

#[cfg(feature = "resample")]
#[test]
fn resample_source_rate_zero() {
    let samples = vec![0.0f32; 100];
    assert!(resample::resample(&samples, 1, 0, 44100, ResampleQuality::Draft).is_err());
}

// --- Codec struct serde tests for new types ---

#[cfg(feature = "ogg")]
#[test]
fn serde_roundtrip_ogg_codec() {
    use shravan::codec::OggCodec;
    let json = serde_json::to_string(&OggCodec).unwrap();
    let back: OggCodec = serde_json::from_str(&json).unwrap();
    assert_eq!(OggCodec, back);
}

#[cfg(feature = "aiff")]
#[test]
fn serde_roundtrip_aiff_codec() {
    use shravan::codec::AiffCodec;
    let json = serde_json::to_string(&AiffCodec).unwrap();
    let back: AiffCodec = serde_json::from_str(&json).unwrap();
    assert_eq!(AiffCodec, back);
}

#[cfg(feature = "mp3")]
#[test]
fn serde_roundtrip_mp3_codec() {
    use shravan::codec::Mp3Codec;
    let json = serde_json::to_string(&Mp3Codec).unwrap();
    let back: Mp3Codec = serde_json::from_str(&json).unwrap();
    assert_eq!(Mp3Codec, back);
}

#[cfg(feature = "opus")]
#[test]
fn serde_roundtrip_opus_codec() {
    use shravan::codec::OpusCodec;
    let json = serde_json::to_string(&OpusCodec).unwrap();
    let back: OpusCodec = serde_json::from_str(&json).unwrap();
    assert_eq!(OpusCodec, back);
}

// --- codec::open for new formats ---

#[cfg(all(feature = "aiff", feature = "pcm"))]
#[test]
fn codec_open_aiff() {
    let samples = vec![0.5f32; 100];
    let encoded = shravan::aiff::encode(&samples, 44100, 1, 16).unwrap();
    let (info, decoded) = shravan::codec::open(&encoded).unwrap();
    assert_eq!(info.format, shravan::AudioFormat::Aiff);
    assert_eq!(decoded.len(), 100);
}

// --- Tag edge case tests ---

#[cfg(feature = "tag")]
#[test]
fn tag_id3v2_empty_frames() {
    use shravan::tag;
    // Build an ID3v2 tag with zero-length text frames
    let mut data = Vec::new();
    data.extend_from_slice(b"ID3");
    data.push(3); // v2.3
    data.push(0);
    data.push(0);
    // size = 0 (no frames)
    data.extend_from_slice(&[0, 0, 0, 0]);
    let meta = tag::read_id3v2(&data).unwrap();
    assert!(meta.title.is_none());
}

#[cfg(feature = "tag")]
#[test]
fn tag_vorbis_empty_comments() {
    use shravan::tag;
    let mut data = Vec::new();
    data.extend_from_slice(&4u32.to_le_bytes()); // vendor len
    data.extend_from_slice(b"test");
    data.extend_from_slice(&0u32.to_le_bytes()); // 0 comments
    let meta = tag::read_vorbis_comment(&data).unwrap();
    assert!(meta.title.is_none());
}

// --- AIFF codec::open for AIFF-C ---

#[cfg(feature = "aiff")]
#[test]
fn format_detection_all_types() {
    use shravan::format::{AudioFormat, detect_format};

    assert_eq!(detect_format(b"RIFF____WAVE").unwrap(), AudioFormat::Wav);
    assert_eq!(detect_format(b"fLaC____").unwrap(), AudioFormat::Flac);
    assert_eq!(detect_format(b"OggS____").unwrap(), AudioFormat::Ogg);
    assert_eq!(detect_format(b"FORM____AIFF").unwrap(), AudioFormat::Aiff);
    assert_eq!(detect_format(b"FORM____AIFC").unwrap(), AudioFormat::Aiff);
    assert_eq!(detect_format(b"ID3_____").unwrap(), AudioFormat::Mp3);
    assert_eq!(
        detect_format(&[0xFF, 0xFB, 0x90, 0x00]).unwrap(),
        AudioFormat::Mp3
    );
    assert!(detect_format(b"\x00\x00\x00\x00").is_err());
}

// --- Streaming edge cases ---

#[cfg(all(feature = "streaming", feature = "wav", feature = "pcm"))]
#[test]
fn streaming_wav_double_flush() {
    use shravan::stream::{StreamDecoder, StreamEvent, WavStreamDecoder};
    let samples = vec![0.5f32; 100];
    let encoded = shravan::wav::encode(&samples, 44100, 1, shravan::pcm::PcmFormat::I16).unwrap();

    let mut dec = WavStreamDecoder::new();
    let _ = dec.feed(&encoded).unwrap();
    let _events1 = dec.flush().unwrap();
    let events2 = dec.flush().unwrap(); // double flush
    // Second flush should just return End or empty
    assert!(events2.is_empty() || events2.iter().all(|e| matches!(e, StreamEvent::End)));
}

#[cfg(all(feature = "streaming", feature = "wav", feature = "pcm"))]
#[test]
fn streaming_decode_file_nonexistent() {
    let result = shravan::stream::decode_file(std::path::Path::new("/nonexistent/file.wav"));
    assert!(result.is_err());
}
