//! Criterion benchmarks for shravan.

use criterion::{Criterion, black_box, criterion_group, criterion_main};

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
        b.iter(|| {
            resample::resample(
                black_box(&samples),
                1,
                44100,
                48000,
                ResampleQuality::Good,
            )
        });
    });
}

#[cfg(all(feature = "wav", feature = "pcm"))]
criterion_group!(wav_benches, wav_decode_1sec);

#[cfg(feature = "pcm")]
criterion_group!(pcm_benches, pcm_i16_to_f32_4096);

#[cfg(feature = "resample")]
criterion_group!(resample_benches, resample_4096);

// Combine all enabled benchmark groups.
#[cfg(all(feature = "wav", feature = "pcm", feature = "resample"))]
criterion_main!(wav_benches, pcm_benches, resample_benches);

#[cfg(all(feature = "wav", feature = "pcm", not(feature = "resample")))]
criterion_main!(wav_benches, pcm_benches);

#[cfg(all(not(feature = "wav"), feature = "pcm", feature = "resample"))]
criterion_main!(pcm_benches, resample_benches);

#[cfg(all(not(feature = "wav"), feature = "pcm", not(feature = "resample")))]
criterion_main!(pcm_benches);

#[cfg(all(feature = "wav", not(feature = "pcm"), feature = "resample"))]
criterion_main!(resample_benches);

// Fallback: if nothing is enabled, still need a main
#[cfg(not(any(feature = "pcm", all(feature = "wav", feature = "pcm"), feature = "resample")))]
fn main() {}
