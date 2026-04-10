//! SIMD-accelerated audio processing kernels.
//!
//! Provides platform-specific SIMD implementations for hot-path PCM operations.
//! Falls back to scalar code on unsupported platforms.
//!
//! - **x86_64**: SSE2 (baseline, 4 f32/op)
//! - **aarch64**: NEON (baseline, 4 f32/op)

#[cfg(target_arch = "x86_64")]
#[allow(clippy::needless_range_loop)]
mod x86;

#[cfg(target_arch = "aarch64")]
mod aarch64;

// ── Platform dispatch ───────────────────────────────────────────────

/// Convert signed 16-bit integer samples to f32 using SIMD.
///
/// Output buffer must be at least as long as input.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn i16_to_f32(src: &[i16], dst: &mut [f32]) {
    x86::i16_to_f32(src, dst)
}
/// Convert signed 16-bit integer samples to f32 using SIMD.
#[cfg(target_arch = "aarch64")]
#[inline]
pub fn i16_to_f32(src: &[i16], dst: &mut [f32]) {
    aarch64::i16_to_f32(src, dst)
}
/// Convert signed 16-bit integer samples to f32 (scalar fallback).
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[inline]
pub fn i16_to_f32(src: &[i16], dst: &mut [f32]) {
    i16_to_f32_scalar(src, dst)
}

/// Convert f32 samples to signed 16-bit integers using SIMD.
///
/// Output buffer must be at least as long as input.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn f32_to_i16(src: &[f32], dst: &mut [i16]) {
    x86::f32_to_i16(src, dst)
}
/// Convert f32 samples to signed 16-bit integers using SIMD.
#[cfg(target_arch = "aarch64")]
#[inline]
pub fn f32_to_i16(src: &[f32], dst: &mut [i16]) {
    aarch64::f32_to_i16(src, dst)
}
/// Convert f32 samples to signed 16-bit integers (scalar fallback).
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[inline]
pub fn f32_to_i16(src: &[f32], dst: &mut [i16]) {
    f32_to_i16_scalar(src, dst)
}

/// Weighted dot product: `sum(samples[i] * weights[i])` for pre-computed sinc kernels.
///
/// Returns `(weighted_sum, weight_sum)` for normalization.
#[cfg(target_arch = "x86_64")]
#[must_use]
#[inline]
pub fn weighted_sum(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    x86::weighted_sum(samples, weights)
}
/// Weighted dot product using SIMD.
#[cfg(target_arch = "aarch64")]
#[must_use]
#[inline]
pub fn weighted_sum(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    aarch64::weighted_sum(samples, weights)
}
/// Weighted dot product (scalar fallback).
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[must_use]
#[inline]
pub fn weighted_sum(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    weighted_sum_scalar(samples, weights)
}

// ── Scalar fallbacks ────────────────────────────────────────────────

#[allow(dead_code)]
fn i16_to_f32_scalar(src: &[i16], dst: &mut [f32]) {
    let len = src.len().min(dst.len());
    for i in 0..len {
        dst[i] = src[i] as f32 / 32768.0;
    }
}

#[allow(dead_code)]
fn f32_to_i16_scalar(src: &[f32], dst: &mut [i16]) {
    let len = src.len().min(dst.len());
    for i in 0..len {
        dst[i] = (src[i].clamp(-1.0, 1.0) * 32767.0) as i16;
    }
}

#[allow(dead_code)]
fn weighted_sum_scalar(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    let len = samples.len().min(weights.len());
    let mut sum = 0.0f32;
    let mut weight_sum = 0.0f32;
    for i in 0..len {
        sum += samples[i] * weights[i];
        weight_sum += weights[i];
    }
    (sum, weight_sum)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn i16_f32_roundtrip() {
        let src_i16: Vec<i16> = vec![0, 16384, -16384, 32767, -32768];
        let mut f32_buf = vec![0.0f32; 5];
        i16_to_f32(&src_i16, &mut f32_buf);
        let mut back_i16 = vec![0i16; 5];
        f32_to_i16(&f32_buf, &mut back_i16);
        for (a, b) in src_i16.iter().zip(back_i16.iter()) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "{a} != {b}");
        }
    }

    #[test]
    fn various_buffer_sizes() {
        for size in [0, 1, 3, 4, 7, 8, 15, 16, 17] {
            let src: Vec<i16> = (0..size).map(|i| (i * 1000) as i16).collect();
            let mut dst = vec![0.0f32; size];
            i16_to_f32(&src, &mut dst);
            for (i, (&s, &d)) in src.iter().zip(dst.iter()).enumerate() {
                let expected = s as f32 / 32768.0;
                assert!(
                    (d - expected).abs() < f32::EPSILON,
                    "size={size} i={i}: {d} != {expected}"
                );
            }
        }
    }

    #[test]
    fn weighted_sum_basic() {
        let samples = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let weights = vec![0.5, 0.5, 0.5, 0.5, 0.5];
        let (sum, wt) = weighted_sum(&samples, &weights);
        assert!((sum - 7.5).abs() < 0.001);
        assert!((wt - 2.5).abs() < 0.001);
    }

    #[test]
    fn weighted_sum_empty() {
        let (sum, wt) = weighted_sum(&[], &[]);
        assert_eq!(sum, 0.0);
        assert_eq!(wt, 0.0);
    }
}
