//! Shared FFT and MDCT infrastructure for audio encoders.
//!
//! Provides a mixed-radix FFT (factors of 2, 3, 5) and an FFT-based forward
//! MDCT used by the Opus CELT encoder and AAC-LC encoder.

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Complex arithmetic
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(crate) struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    #[inline]
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    #[inline]
    pub fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    #[inline]
    pub fn add(self, other: Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    #[inline]
    #[allow(dead_code)]
    pub fn sub(self, other: Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }

    /// exp(i * theta)
    #[inline]
    pub fn from_angle(theta: f64) -> Self {
        Self {
            re: libm::cos(theta),
            im: libm::sin(theta),
        }
    }
}

// ---------------------------------------------------------------------------
// Mixed-radix FFT (supports factors of 2, 3, 5)
// ---------------------------------------------------------------------------

/// In-place mixed-radix DIT FFT.
/// `buf` length must factor entirely into 2, 3, and 5.
pub(crate) fn fft(buf: &mut [Complex]) {
    let n = buf.len();
    if n <= 1 {
        return;
    }

    let radix = if n.is_multiple_of(2) {
        2
    } else if n.is_multiple_of(3) {
        3
    } else if n.is_multiple_of(5) {
        5
    } else {
        dft_naive(buf);
        return;
    };

    let m = n / radix;

    // Decimation-in-time: reorder into `radix` sub-sequences of length `m`
    let mut tmp = vec![Complex::ZERO; n];
    for r in 0..radix {
        for j in 0..m {
            tmp[r * m + j] = buf[j * radix + r];
        }
    }

    // Recurse on each sub-FFT of length m
    for r in 0..radix {
        fft(&mut tmp[r * m..(r + 1) * m]);
    }

    // Combine: generic radix-R butterfly
    let angle_base = -2.0 * core::f64::consts::PI / n as f64;
    for (k, out) in buf.iter_mut().enumerate() {
        let km = k % m;
        let mut sum = Complex::ZERO;
        for r in 0..radix {
            let tw = Complex::from_angle(angle_base * (r * k) as f64);
            sum = sum.add(tmp[r * m + km].mul(tw));
        }
        *out = sum;
    }
}

/// Naive DFT for small prime sizes (fallback).
pub(crate) fn dft_naive(buf: &mut [Complex]) {
    let n = buf.len();
    let tmp: Vec<Complex> = buf.to_vec();
    let angle = -2.0 * core::f64::consts::PI / (n as f64);
    for (k, out) in buf.iter_mut().enumerate() {
        let mut sum = Complex::ZERO;
        for (j, &inp) in tmp.iter().enumerate() {
            let w = Complex::from_angle(angle * (k * j) as f64);
            sum = sum.add(inp.mul(w));
        }
        *out = sum;
    }
}

// ---------------------------------------------------------------------------
// MDCT via 2N-point FFT
// ---------------------------------------------------------------------------

/// Compute forward MDCT of `input` (length N) producing N/2 spectral coefficients.
///
/// MDCT: X\[k\] = Σ x\[n\] cos(π/N (n + 0.5 + N/4)(k + 0.5))
///
/// Uses a 2N-point FFT with pre/post twiddle. Complexity: O(N log N).
/// N must factor into 2, 3, and 5 after doubling (2N factorable).
pub(crate) fn mdct_forward(input: &[f32], output: &mut [f32]) {
    let n = input.len();
    let n2 = n / 2;
    let n4 = n / 4;
    let pi = core::f64::consts::PI;
    let n0 = 0.5 + n4 as f64;
    let nn = 2 * n;

    // Pre-twiddle + zero-pad to 2N
    let mut z = vec![Complex::ZERO; nn];
    for (i, &x) in input.iter().enumerate() {
        let tw = Complex::from_angle(-pi * i as f64 / nn as f64);
        z[i] = Complex::new(f64::from(x), 0.0).mul(tw);
    }

    fft(&mut z);

    // Post-twiddle: X[k] = Re(exp(-j π n₀ (2k+1) / (2N)) * Z[k])
    for (k, out) in output.iter_mut().enumerate().take(n2) {
        let angle = -pi * n0 * (2 * k + 1) as f64 / nn as f64;
        let tw = Complex::from_angle(angle);
        *out = z[k].mul(tw).re as f32;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn fft_basic_impulse() {
        let mut buf = vec![Complex::ZERO; 4];
        buf[0] = Complex::new(1.0, 0.0);
        fft(&mut buf);
        for z in &buf {
            assert!((z.re - 1.0).abs() < 1e-10);
            assert!(z.im.abs() < 1e-10);
        }
    }

    #[test]
    fn fft_size_6() {
        let mut buf = vec![Complex::ZERO; 6];
        buf[0] = Complex::new(1.0, 0.0);
        fft(&mut buf);
        for (i, z) in buf.iter().enumerate() {
            assert!(
                (z.re - 1.0).abs() < 1e-10,
                "FFT size 6 bin {i}: re={}",
                z.re
            );
            assert!(z.im.abs() < 1e-10, "FFT size 6 bin {i}: im={}", z.im);
        }
    }

    #[test]
    fn fft_vs_dft_size_30() {
        let n = 30;
        let mut buf_fft = Vec::with_capacity(n);
        let mut buf_dft = Vec::with_capacity(n);
        for i in 0..n {
            let val = Complex::new(libm::sin(i as f64 * 0.7), libm::cos(i as f64 * 1.3));
            buf_fft.push(val);
            buf_dft.push(val);
        }

        fft(&mut buf_fft);
        dft_naive(&mut buf_dft);

        let mut max_diff = 0.0f64;
        for (a, b) in buf_fft.iter().zip(buf_dft.iter()) {
            max_diff = max_diff.max((a.re - b.re).abs()).max((a.im - b.im).abs());
        }
        assert!(max_diff < 1e-6, "FFT vs DFT size 30 max_diff={max_diff}");
    }

    #[test]
    fn fft_size_240() {
        let mut buf = vec![Complex::ZERO; 240];
        buf[0] = Complex::new(1.0, 0.0);
        fft(&mut buf);
        assert!((buf[0].re - 1.0).abs() < 1e-10);
    }

    /// Naive O(N²) MDCT for correctness validation.
    fn mdct_naive(input: &[f32], output: &mut [f32]) {
        let n = input.len();
        let n2 = n / 2;
        for (k, out) in output.iter_mut().enumerate().take(n2) {
            let mut sum = 0.0f64;
            for (i, &inp) in input.iter().enumerate().take(n) {
                let phase = core::f64::consts::PI / (n as f64)
                    * (f64::from(i as u32) + 0.5 + (n as f64) / 4.0)
                    * (f64::from(k as u32) + 0.5);
                sum += f64::from(inp) * libm::cos(phase);
            }
            *out = sum as f32;
        }
    }

    #[test]
    fn mdct_produces_output() {
        let input = vec![1.0f32; 960];
        let mut output = vec![0.0f32; 480];
        mdct_forward(&input, &mut output);
        let energy: f32 = output.iter().map(|x| x * x).sum();
        assert!(energy > 0.0);
    }

    #[test]
    fn mdct_fft_matches_naive() {
        let input: Vec<f32> = (0..960)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 48000.0))
            .collect();

        let mut fft_output = vec![0.0f32; 480];
        let mut naive_output = vec![0.0f32; 480];

        mdct_forward(&input, &mut fft_output);
        mdct_naive(&input, &mut naive_output);

        let mut max_diff = 0.0f32;
        for (f, n) in fft_output.iter().zip(naive_output.iter()) {
            max_diff = max_diff.max((f - n).abs());
        }
        assert!(
            max_diff < 0.01,
            "FFT-based MDCT diverges from naive: max_diff={max_diff}"
        );
    }

    #[test]
    fn mdct_small_matches_naive() {
        let input: Vec<f32> = (0..16).map(|i| (i as f32) / 16.0).collect();
        let mut fft_out = vec![0.0f32; 8];
        let mut naive_out = vec![0.0f32; 8];
        mdct_forward(&input, &mut fft_out);
        mdct_naive(&input, &mut naive_out);
        let max_diff: f32 = fft_out
            .iter()
            .zip(naive_out.iter())
            .map(|(f, n)| (f - n).abs())
            .fold(0.0, f32::max);
        assert!(max_diff < 0.01, "Small MDCT max_diff={max_diff}");
    }
}
