//! Dithering for bit-depth reduction.
//!
//! TPDF (Triangular Probability Density Function) adds flat noise at ±1 LSB.
//! Noise-shaped dithering uses first-order error feedback for perceptual weighting.

use alloc::vec::Vec;

/// Apply TPDF dithering for bit-depth reduction.
///
/// `target_bits` is the target bit depth (e.g., 16 for f32 -> i16 conversion).
/// The dither noise amplitude is ±1 LSB of the target format.
#[must_use]
pub fn tpdf_dither(samples: &[f32], target_bits: u32) -> Vec<f32> {
    let quant_step = 1.0 / (1_u64 << (target_bits - 1)) as f32;
    let mut rng_state: u32 = 0x12345678;
    samples
        .iter()
        .map(|&s| {
            // Two uniform random numbers summed for triangular distribution
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 17;
            rng_state ^= rng_state << 5;
            let r1 = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 17;
            rng_state ^= rng_state << 5;
            let r2 = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let dither = (r1 + r2) * 0.5 * quant_step;
            s + dither
        })
        .collect()
}

/// Apply noise-shaped dithering with first-order error feedback.
///
/// Produces lower perceived noise than TPDF by shaping the noise spectrum
/// to frequencies where human hearing is less sensitive.
#[must_use]
pub fn noise_shaped_dither(samples: &[f32], target_bits: u32) -> Vec<f32> {
    let quant_step = 1.0 / (1_u64 << (target_bits - 1)) as f32;
    let mut rng_state: u32 = 0x12345678;
    let mut error: f32 = 0.0;
    samples
        .iter()
        .map(|&s| {
            // Add error feedback from previous sample
            let shaped = s - error;
            // Generate TPDF noise
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 17;
            rng_state ^= rng_state << 5;
            let r1 = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 17;
            rng_state ^= rng_state << 5;
            let r2 = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let dither = (r1 + r2) * 0.5 * quant_step;
            let dithered = shaped + dither;
            // Quantize to target bit depth
            let quantized = libm::roundf(dithered / quant_step) * quant_step;
            // Track error for next sample
            error = quantized - shaped;
            quantized
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn tpdf_preserves_length() {
        let input = alloc::vec![0.5; 1024];
        let output = tpdf_dither(&input, 16);
        assert_eq!(input.len(), output.len());
    }

    #[test]
    fn tpdf_noise_is_small() {
        let input = alloc::vec![0.5; 4096];
        let output = tpdf_dither(&input, 16);
        let max_diff = input
            .iter()
            .zip(&output)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        // TPDF noise for 16-bit should be within ~2 LSBs
        assert!(max_diff < 0.001, "max diff {max_diff} too large");
    }

    #[test]
    fn noise_shaped_preserves_length() {
        let input = alloc::vec![0.5; 1024];
        let output = noise_shaped_dither(&input, 16);
        assert_eq!(input.len(), output.len());
    }

    #[test]
    fn noise_shaped_noise_is_small() {
        let input = alloc::vec![0.5; 4096];
        let output = noise_shaped_dither(&input, 16);
        let max_diff = input
            .iter()
            .zip(&output)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 0.001, "max diff {max_diff} too large");
    }

    #[test]
    fn empty_input() {
        assert!(tpdf_dither(&[], 16).is_empty());
        assert!(noise_shaped_dither(&[], 16).is_empty());
    }

    #[test]
    fn serde_not_needed() {
        // Dither functions are pure — no types to serialize.
        // This test exists for coverage completeness.
        let input = alloc::vec![0.0f32; 10];
        let tpdf = tpdf_dither(&input, 16);
        let ns = noise_shaped_dither(&input, 16);
        assert_eq!(tpdf.len(), 10);
        assert_eq!(ns.len(), 10);
    }
}
