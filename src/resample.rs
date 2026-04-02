//! Sinc resampling — windowed sinc interpolation with configurable quality.

use serde::{Deserialize, Serialize};

use crate::error::{Result, ShravanError};

/// Resampling quality level, controlling the sinc kernel width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResampleQuality {
    /// Fast, lower quality (4-point sinc kernel).
    Draft,
    /// Balanced quality (16-point sinc kernel).
    Good,
    /// Highest quality (64-point sinc kernel).
    Best,
}

impl ResampleQuality {
    /// Number of sinc lobes (half-width of the kernel in samples).
    #[must_use]
    #[inline]
    fn kernel_half_width(self) -> usize {
        match self {
            Self::Draft => 2,
            Self::Good => 8,
            Self::Best => 32,
        }
    }
}

/// Resample interleaved f32 audio using windowed sinc interpolation.
///
/// Higher quality levels use wider sinc kernels for better frequency
/// preservation at the cost of more computation.
///
/// # Arguments
///
/// * `samples` - Interleaved f32 sample data
/// * `channels` - Number of audio channels
/// * `source_rate` - Source sample rate in Hz
/// * `target_rate` - Target sample rate in Hz
/// * `quality` - Resampling quality level
///
/// # Errors
///
/// Returns [`ShravanError::InvalidSampleRate`] if `target_rate` is zero.
/// Returns [`ShravanError::InvalidChannels`] if `channels` is zero.
#[must_use = "resampled audio data is returned and should not be discarded"]
pub fn resample(
    samples: &[f32],
    channels: u16,
    source_rate: u32,
    target_rate: u32,
    quality: ResampleQuality,
) -> Result<Vec<f32>> {
    if source_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }
    if target_rate == 0 {
        return Err(ShravanError::InvalidSampleRate(0));
    }
    if channels == 0 {
        return Err(ShravanError::InvalidChannels(0));
    }
    if target_rate == source_rate {
        return Ok(samples.to_vec());
    }

    let ch = channels as usize;
    let frames = samples.len() / ch;
    if frames == 0 {
        return Ok(Vec::new());
    }

    // Multi-channel optimization: deinterleave for sequential memory access,
    // resample each channel independently, then reinterleave.
    if ch > 1 {
        let channel_bufs: Vec<Vec<f32>> = (0..ch)
            .map(|c| {
                let mut buf = Vec::with_capacity(frames);
                for f in 0..frames {
                    buf.push(samples[f * ch + c]);
                }
                buf
            })
            .collect();

        let mut out_channels: Vec<Vec<f32>> = Vec::with_capacity(ch);
        for buf in &channel_bufs {
            out_channels.push(resample_mono(buf, source_rate, target_rate, quality)?);
        }

        let new_frames = out_channels[0].len();
        let mut out = Vec::with_capacity(new_frames * ch);
        for f in 0..new_frames {
            for c_buf in &out_channels {
                out.push(if f < c_buf.len() { c_buf[f] } else { 0.0 });
            }
        }
        return Ok(out);
    }

    resample_mono(samples, source_rate, target_rate, quality)
}

/// Resample a single channel of f32 audio.
#[inline]
fn resample_mono(
    samples: &[f32],
    source_rate: u32,
    target_rate: u32,
    quality: ResampleQuality,
) -> Result<Vec<f32>> {
    let frames = samples.len();
    if frames == 0 {
        return Ok(Vec::new());
    }

    let ratio = target_rate as f64 / source_rate as f64;
    let new_frames = (frames as f64 * ratio).ceil() as usize;
    let half_width = quality.kernel_half_width();

    // For downsampling, scale the kernel to avoid aliasing
    let (filter_scale, kernel_scale) = if ratio < 1.0 {
        (ratio, ratio)
    } else {
        (1.0, 1.0)
    };

    let mut out = vec![0.0f32; new_frames];

    // Pre-allocate temp buffers for SIMD kernel accumulation
    #[cfg(feature = "simd")]
    let max_kernel_len = ((half_width as f64 / filter_scale).ceil() as usize) * 2 + 1;
    #[cfg(feature = "simd")]
    let mut kernel_samples = Vec::with_capacity(max_kernel_len);
    #[cfg(feature = "simd")]
    let mut kernel_weights = Vec::with_capacity(max_kernel_len);

    #[allow(clippy::needless_range_loop)]
    for frame in 0..new_frames {
        let src_pos = frame as f64 / ratio;
        let src_center = src_pos.floor() as i64;
        let frac = src_pos - src_center as f64;

        let scaled_half = (half_width as f64 / filter_scale).ceil() as i64;

        #[cfg(feature = "simd")]
        {
            kernel_samples.clear();
            kernel_weights.clear();

            for i in -scaled_half..=scaled_half {
                let src_idx = src_center + i;
                if src_idx < 0 || src_idx >= frames as i64 {
                    continue;
                }
                let x = (i as f64 - frac) * kernel_scale;
                let w = windowed_sinc(x, scaled_half as f64) as f32;
                kernel_samples.push(samples[src_idx as usize]);
                kernel_weights.push(w);
            }

            let (sum, weight_sum) = crate::simd::weighted_sum(&kernel_samples, &kernel_weights);
            if weight_sum.abs() > 1e-7 {
                out[frame] = sum / weight_sum;
            }
        }

        #[cfg(not(feature = "simd"))]
        {
            let mut sum = 0.0f64;
            let mut weight_sum = 0.0f64;

            for i in -scaled_half..=scaled_half {
                let src_idx = src_center + i;
                if src_idx < 0 || src_idx >= frames as i64 {
                    continue;
                }

                let x = (i as f64 - frac) * kernel_scale;
                let w = windowed_sinc(x, scaled_half as f64);
                let sample = samples[src_idx as usize] as f64;
                sum += sample * w;
                weight_sum += w;
            }

            if weight_sum.abs() > 1e-10 {
                out[frame] = (sum / weight_sum) as f32;
            }
        }
    }

    Ok(out)
}

/// Windowed sinc function using a Blackman-Harris window.
fn windowed_sinc(x: f64, half_width: f64) -> f64 {
    if x.abs() < 1e-10 {
        return 1.0;
    }

    let sinc = libm::sin(core::f64::consts::PI * x) / (core::f64::consts::PI * x);

    // Blackman-Harris window
    if x.abs() > half_width {
        return 0.0;
    }
    let t = (x / half_width + 1.0) * 0.5; // Normalize to [0, 1]
    let tau = core::f64::consts::TAU;
    let window = 0.35875 - 0.48829 * libm::cos(tau * t) + 0.14128 * libm::cos(2.0 * tau * t)
        - 0.01168 * libm::cos(3.0 * tau * t);

    sinc * window
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn same_rate_identity() {
        let samples: Vec<f32> = vec![0.5, -0.5, 0.3, -0.3];
        let out = resample(&samples, 1, 44100, 44100, ResampleQuality::Good).unwrap();
        assert_eq!(out.len(), samples.len());
        assert_eq!(out, samples);
    }

    #[test]
    fn zero_rate_rejected() {
        let samples = vec![0.0f32; 100];
        assert!(resample(&samples, 1, 44100, 0, ResampleQuality::Draft).is_err());
    }

    #[test]
    fn zero_channels_rejected() {
        let samples = vec![0.0f32; 100];
        assert!(resample(&samples, 0, 44100, 48000, ResampleQuality::Draft).is_err());
    }

    #[test]
    fn empty_input() {
        let out = resample(&[], 2, 44100, 48000, ResampleQuality::Good).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn upsample_increases_length() {
        let samples = vec![0.0f32; 1000];
        let out = resample(&samples, 1, 44100, 96000, ResampleQuality::Draft).unwrap();
        assert!(out.len() > 1000);
    }

    #[test]
    fn downsample_decreases_length() {
        let samples = vec![0.0f32; 1000];
        let out = resample(&samples, 1, 96000, 44100, ResampleQuality::Draft).unwrap();
        assert!(out.len() < 1000);
    }

    #[test]
    fn roundtrip_preserves_signal() {
        let sr = 44100u32;
        let frames = 4096;
        let samples: Vec<f32> = (0..frames)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / sr as f32))
            .collect();

        let rms_orig = rms(&samples);

        let up = resample(&samples, 1, 44100, 48000, ResampleQuality::Good).unwrap();
        let back = resample(&up, 1, 48000, 44100, ResampleQuality::Good).unwrap();

        let rms_back = rms(&back);
        assert!(
            (rms_back - rms_orig).abs() < rms_orig * 0.1,
            "Round-trip RMS: {rms_back} vs original: {rms_orig}"
        );
    }

    #[test]
    fn quality_levels_all_work() {
        let samples: Vec<f32> = (0..1024)
            .map(|i| libm::sinf(2.0 * core::f32::consts::PI * 440.0 * i as f32 / 44100.0))
            .collect();

        for quality in [
            ResampleQuality::Draft,
            ResampleQuality::Good,
            ResampleQuality::Best,
        ] {
            let out = resample(&samples, 1, 44100, 48000, quality).unwrap();
            assert!(!out.is_empty());
            assert!(out.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn stereo_resample_roundtrip() {
        let sr = 44100u32;
        let frames = 2048;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / sr as f32;
            samples.push(libm::sinf(2.0 * core::f32::consts::PI * 440.0 * t));
            samples.push(libm::sinf(2.0 * core::f32::consts::PI * 880.0 * t));
        }

        let up = resample(&samples, 2, 44100, 48000, ResampleQuality::Good).unwrap();
        assert_eq!(up.len() % 2, 0); // must be even (stereo)
        let back = resample(&up, 2, 48000, 44100, ResampleQuality::Good).unwrap();

        let rms_orig = rms(&samples);
        let rms_back = rms(&back);
        assert!(
            (rms_back - rms_orig).abs() < rms_orig * 0.15,
            "Stereo roundtrip RMS: {rms_back} vs original: {rms_orig}"
        );
    }

    #[test]
    fn multichannel_4ch() {
        let frames = 1024;
        let ch = 4;
        let samples: Vec<f32> = (0..frames * ch)
            .map(|i| libm::sinf(i as f32 * 0.1))
            .collect();
        let out = resample(&samples, ch as u16, 44100, 48000, ResampleQuality::Draft).unwrap();
        assert_eq!(out.len() % ch, 0);
        assert!(out.iter().all(|s| s.is_finite()));
    }

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        libm::sqrt(sum / samples.len() as f64) as f32
    }
}
