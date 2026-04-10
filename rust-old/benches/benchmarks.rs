//! Criterion benchmarks for shravan.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

#[cfg(all(feature = "wav", feature = "pcm"))]
fn wav_decode_1sec(c: &mut Criterion) {
    use shravan::pcm::PcmFormat;
    use shravan::wav;

    // Generate 1 second of mono 44100 Hz sine wave
    let samples: Vec<f32> = (0..44100)
        .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 44100.0))
        .collect();
    let encoded = wav::encode(&samples, 44100, 1, PcmFormat::I16).unwrap();

    c.bench_function("wav_decode_1sec_i16", |b| {
        b.iter(|| wav::decode(black_box(&encoded)));
    });
}

#[cfg(feature = "pcm")]
fn pcm_i16_to_f32_4096(c: &mut Criterion) {
    use shravan::pcm;

    let samples: Vec<i16> = (0..4096).map(|i| (i % 65536 - 32768) as i16).collect();

    c.bench_function("pcm_i16_to_f32_4096", |b| {
        b.iter(|| pcm::i16_to_f32(black_box(&samples)));
    });
}

#[cfg(feature = "resample")]
fn resample_4096(c: &mut Criterion) {
    use shravan::resample::{self, ResampleQuality};

    let samples: Vec<f32> = (0..4096)
        .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 44100.0))
        .collect();

    c.bench_function("resample_4096_44100_to_48000", |b| {
        b.iter(|| resample::resample(black_box(&samples), 1, 44100, 48000, ResampleQuality::Good));
    });
}

#[cfg(feature = "simd")]
fn simd_i16_to_f32_4096(c: &mut Criterion) {
    use shravan::simd;

    let samples: Vec<i16> = (0..4096).map(|i| (i % 65536 - 32768) as i16).collect();
    let mut dst = vec![0.0f32; 4096];

    c.bench_function("simd_i16_to_f32_4096", |b| {
        b.iter(|| simd::i16_to_f32(black_box(&samples), black_box(&mut dst)));
    });
}

#[cfg(feature = "flac")]
fn flac_encode_1sec(c: &mut Criterion) {
    use shravan::flac;

    let samples: Vec<f32> = (0..44100)
        .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 44100.0))
        .collect();

    c.bench_function("flac_encode_1sec_16bit", |b| {
        b.iter(|| flac::encode(black_box(&samples), 44100, 1, 16));
    });
}

#[cfg(feature = "flac")]
fn flac_decode_1sec(c: &mut Criterion) {
    use shravan::flac;

    let samples: Vec<f32> = (0..44100)
        .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 44100.0))
        .collect();
    let encoded = flac::encode(&samples, 44100, 1, 16).unwrap();

    c.bench_function("flac_decode_1sec_16bit", |b| {
        b.iter(|| flac::decode(black_box(&encoded)));
    });
}

#[cfg(feature = "opus")]
fn opus_encode_1sec(c: &mut Criterion) {
    use shravan::opus;

    // 1 second mono sine at 48 kHz (Opus native rate)
    let samples: Vec<f32> = (0..48000)
        .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 48000.0))
        .collect();

    c.bench_function("opus_encode_1sec_mono_64k", |b| {
        b.iter(|| opus::encode(black_box(&samples), 48000, 1, 64000));
    });
}

// --- Benchmark groups ---
// Use a single function to collect all enabled benchmarks, avoiding
// the combinatorial explosion of cfg-gated criterion_main! macros.

fn all_benchmarks(c: &mut Criterion) {
    #[cfg(all(feature = "wav", feature = "pcm"))]
    wav_decode_1sec(c);

    #[cfg(feature = "pcm")]
    pcm_i16_to_f32_4096(c);

    #[cfg(feature = "simd")]
    simd_i16_to_f32_4096(c);

    #[cfg(feature = "resample")]
    resample_4096(c);

    #[cfg(feature = "flac")]
    {
        flac_encode_1sec(c);
        flac_decode_1sec(c);
    }

    #[cfg(feature = "opus")]
    opus_encode_1sec(c);
}

criterion_group!(benches, all_benchmarks);
criterion_main!(benches);
