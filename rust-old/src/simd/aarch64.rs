//! aarch64 SIMD kernels — NEON (baseline, 4 f32 per op).

// NEON i16_to_f32 and f32_to_i16 are scalar on aarch64 (same as dhvani).

pub fn i16_to_f32(src: &[i16], dst: &mut [f32]) {
    let len = src.len().min(dst.len());
    for i in 0..len {
        dst[i] = src[i] as f32 / 32768.0;
    }
}

pub fn f32_to_i16(src: &[f32], dst: &mut [i16]) {
    let len = src.len().min(dst.len());
    for i in 0..len {
        let clamped = src[i].clamp(-1.0, 1.0);
        dst[i] = (clamped * 32767.0) as i16;
    }
}

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

pub fn weighted_sum(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    // SAFETY: NEON is always available on aarch64.
    unsafe { weighted_sum_neon(samples, weights) }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn weighted_sum_neon(samples: &[f32], weights: &[f32]) -> (f32, f32) {
    let len = samples.len().min(weights.len());
    let chunks = len / 4;
    // SAFETY: NEON intrinsics; NEON is always available on aarch64.
    let mut acc_sum = unsafe { vdupq_n_f32(0.0) };
    let mut acc_wt = unsafe { vdupq_n_f32(0.0) };

    for i in 0..chunks {
        let off = i * 4;
        // SAFETY: Loading from slice with bounds checked by loop range.
        unsafe {
            let s = vld1q_f32(samples.as_ptr().add(off));
            let w = vld1q_f32(weights.as_ptr().add(off));
            acc_sum = vmlaq_f32(acc_sum, s, w);
            acc_wt = vaddq_f32(acc_wt, w);
        }
    }

    // SAFETY: NEON horizontal sum intrinsic.
    let mut total_sum = unsafe { vaddvq_f32(acc_sum) };
    let mut total_wt = unsafe { vaddvq_f32(acc_wt) };
    for i in (chunks * 4)..len {
        total_sum += samples[i] * weights[i];
        total_wt += weights[i];
    }
    (total_sum, total_wt)
}
