//! Integration tests for shravan.

use shravan::format::{detect_format, AudioFormat, FormatInfo};

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
        assert!(
            (a - b).abs() < 0.001,
            "i16 roundtrip mismatch: {a} vs {b}"
        );
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
