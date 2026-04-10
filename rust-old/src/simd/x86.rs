//! x86_64 SIMD kernels — SSE2 (baseline) + AVX2 (runtime-detected).
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

pub fn i16_to_f32(src: &[i16], dst: &mut [f32]) {
    // SAFETY: SSE2 is always available on x86_64.
    unsafe { i16_to_f32_sse2(src, dst) };
}

pub fn f32_to_i16(src: &[f32], dst: &mut [i16]) {
    // SAFETY: SSE2 is always available on x86_64.
    unsafe { f32_to_i16_sse2(src, dst) };
}

pub fn weighted_sum(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    // SAFETY: SSE2 is always available on x86_64.
    unsafe { weighted_sum_sse2(samples, weights) }
}

// ── SSE2 (4 f32 per op) ────────────────────────────────────────────

// SAFETY: SSE2 is always available on x86_64.
#[target_feature(enable = "sse2")]
unsafe fn i16_to_f32_sse2(src: &[i16], dst: &mut [f32]) {
    let len = src.len().min(dst.len());
    let scale = _mm_set1_ps(1.0 / 32768.0);

    let chunks = len / 4;
    for i in 0..chunks {
        let off = i * 4;
        let s0 = src[off] as i32;
        let s1 = src[off + 1] as i32;
        let s2 = src[off + 2] as i32;
        let s3 = src[off + 3] as i32;
        // SAFETY: SSE2 intrinsics operating on scalars. Storing to slice with bounds checked.
        unsafe {
            let ints = _mm_set_epi32(s3, s2, s1, s0);
            let floats = _mm_cvtepi32_ps(ints);
            let scaled = _mm_mul_ps(floats, scale);
            _mm_storeu_ps(dst.as_mut_ptr().add(off), scaled);
        }
    }
    for i in (chunks * 4)..len {
        dst[i] = src[i] as f32 / 32768.0;
    }
}

// SAFETY: SSE2 is always available on x86_64.
#[target_feature(enable = "sse2")]
unsafe fn f32_to_i16_sse2(src: &[f32], dst: &mut [i16]) {
    let len = src.len().min(dst.len());
    let vmin = _mm_set1_ps(-1.0);
    let vmax = _mm_set1_ps(1.0);
    let scale = _mm_set1_ps(32767.0);

    let chunks = len / 4;
    for i in 0..chunks {
        let off = i * 4;
        // SAFETY: Loading/storing with bounds checked by loop range.
        unsafe {
            let a = _mm_loadu_ps(src.as_ptr().add(off));
            let clamped = _mm_min_ps(_mm_max_ps(a, vmin), vmax);
            let scaled = _mm_mul_ps(clamped, scale);
            let ints = _mm_cvtps_epi32(scaled);
            let packed = _mm_packs_epi32(ints, ints);
            dst[off] = _mm_extract_epi16(packed, 0) as i16;
            dst[off + 1] = _mm_extract_epi16(packed, 1) as i16;
            dst[off + 2] = _mm_extract_epi16(packed, 2) as i16;
            dst[off + 3] = _mm_extract_epi16(packed, 3) as i16;
        }
    }
    for i in (chunks * 4)..len {
        let clamped = src[i].clamp(-1.0, 1.0);
        dst[i] = (clamped * 32767.0) as i16;
    }
}

// SAFETY: SSE2 is always available on x86_64.
#[target_feature(enable = "sse2")]
unsafe fn weighted_sum_sse2(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    let len = samples.len().min(weights.len());
    let chunks = len / 4;
    let mut acc_sum = _mm_setzero_ps();
    let mut acc_wt = _mm_setzero_ps();

    for i in 0..chunks {
        let off = i * 4;
        let s = _mm_loadu_ps(samples.as_ptr().add(off));
        let w = _mm_loadu_ps(weights.as_ptr().add(off));
        acc_sum = _mm_add_ps(acc_sum, _mm_mul_ps(s, w));
        acc_wt = _mm_add_ps(acc_wt, w);
    }

    let sum = horizontal_sum_f32_sse2(acc_sum);
    let wt = horizontal_sum_f32_sse2(acc_wt);

    let mut total_sum = sum;
    let mut total_wt = wt;
    for i in (chunks * 4)..len {
        total_sum += samples[i] * weights[i];
        total_wt += weights[i];
    }
    (total_sum, total_wt)
}

// SAFETY: SSE2 is always available on x86_64.
#[target_feature(enable = "sse2")]
unsafe fn horizontal_sum_f32_sse2(v: __m128) -> f32 {
    let shuf = _mm_shuffle_ps(v, v, 0b_01_00_11_10);
    let sum1 = _mm_add_ps(v, shuf);
    let shuf2 = _mm_shuffle_ps(sum1, sum1, 0b_00_01_00_01);
    let sum2 = _mm_add_ps(sum1, shuf2);
    _mm_cvtss_f32(sum2)
}
