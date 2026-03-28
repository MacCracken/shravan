//! PCM sample format conversion and layout utilities.

use serde::{Deserialize, Serialize};

/// PCM sample format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PcmFormat {
    /// Signed 8-bit integer.
    I8,
    /// Signed 16-bit integer.
    I16,
    /// Signed 24-bit integer (stored in 3 bytes).
    I24,
    /// Signed 32-bit integer.
    I32,
    /// 32-bit IEEE 754 float.
    F32,
    /// 64-bit IEEE 754 float.
    F64,
}

impl PcmFormat {
    /// Bytes per sample for this format.
    #[must_use]
    #[inline]
    pub const fn bytes_per_sample(self) -> u16 {
        match self {
            Self::I8 => 1,
            Self::I16 => 2,
            Self::I24 => 3,
            Self::I32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    /// Bit depth for this format.
    #[must_use]
    #[inline]
    pub const fn bit_depth(self) -> u16 {
        match self {
            Self::I8 => 8,
            Self::I16 => 16,
            Self::I24 => 24,
            Self::I32 | Self::F32 => 32,
            Self::F64 => 64,
        }
    }
}

/// Convert signed 16-bit integer samples to f32 in [-1.0, ~1.0].
#[must_use]
#[inline]
pub fn i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| s as f32 / 32768.0).collect()
}

/// Convert f32 samples to signed 16-bit integers with clamping.
#[must_use]
#[inline]
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped * 32767.0) as i16
        })
        .collect()
}

/// Convert signed 32-bit integer samples to f32 in [-1.0, ~1.0].
#[must_use]
#[inline]
pub fn i32_to_f32(samples: &[i32]) -> Vec<f32> {
    samples
        .iter()
        .map(|&s| s as f32 / 2_147_483_648.0)
        .collect()
}

/// Convert f32 samples to signed 32-bit integers with clamping.
#[must_use]
#[inline]
pub fn f32_to_i32(samples: &[f32]) -> Vec<i32> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped as f64 * 2_147_483_647.0) as i32
        })
        .collect()
}

/// Convert signed 24-bit integers (stored as i32, lower 24 bits) to f32.
///
/// Range: \[-8388608, 8388607\] maps to \[-1.0, ~1.0).
#[must_use]
#[inline]
pub fn i24_to_f32(samples: &[i32]) -> Vec<f32> {
    samples
        .iter()
        .map(|&s| {
            // Sign-extend from 24 bits
            let extended = (s << 8) >> 8;
            extended as f32 / 8_388_608.0
        })
        .collect()
}

/// Convert f32 samples to signed 24-bit integers (stored as i32).
///
/// Input clamped to \[-1.0, 1.0\].
#[must_use]
#[inline]
pub fn f32_to_i24(samples: &[f32]) -> Vec<i32> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped * 8_388_607.0) as i32
        })
        .collect()
}

/// Convert 24-bit packed bytes (3 bytes per sample, little-endian) to f32.
#[must_use]
#[inline]
pub fn i24_packed_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(3)
        .map(|chunk| {
            let raw =
                i32::from(chunk[0]) | (i32::from(chunk[1]) << 8) | (i32::from(chunk[2]) << 16);
            // Sign-extend from 24 bits
            let extended = (raw << 8) >> 8;
            extended as f32 / 8_388_608.0
        })
        .collect()
}

/// Convert f32 samples to 24-bit packed bytes (3 bytes per sample, little-endian).
#[must_use]
#[inline]
pub fn f32_to_i24_packed(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 3);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let val = (clamped * 8_388_607.0) as i32;
        out.push(val as u8);
        out.push((val >> 8) as u8);
        out.push((val >> 16) as u8);
    }
    out
}

/// Convert 64-bit float samples to 32-bit float.
#[must_use]
#[inline]
pub fn f64_to_f32(samples: &[f64]) -> Vec<f32> {
    samples.iter().map(|&s| s as f32).collect()
}

/// Convert 32-bit float samples to 64-bit float.
#[must_use]
#[inline]
pub fn f32_to_f64(samples: &[f32]) -> Vec<f64> {
    samples.iter().map(|&s| f64::from(s)).collect()
}

/// Convert unsigned 8-bit PCM samples (0..255, center at 128) to f32 in \[-1.0, ~1.0\].
#[must_use]
#[inline]
pub fn u8_to_f32(samples: &[u8]) -> Vec<f32> {
    samples
        .iter()
        .map(|&s| (f32::from(s) - 128.0) / 128.0)
        .collect()
}

/// Convert f32 samples to unsigned 8-bit PCM (0..255, center at 128).
#[must_use]
#[inline]
pub fn f32_to_u8(samples: &[f32]) -> Vec<u8> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            ((clamped * 128.0) + 128.0).clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Interleave separate channel buffers into a single interleaved buffer.
///
/// All channels must have the same length.
#[must_use]
pub fn interleave(channels: &[&[f32]]) -> Vec<f32> {
    if channels.is_empty() {
        return Vec::new();
    }
    let frames = channels[0].len();
    let ch_count = channels.len();
    let mut out = Vec::with_capacity(frames * ch_count);
    for frame in 0..frames {
        for ch in channels {
            if frame < ch.len() {
                out.push(ch[frame]);
            }
        }
    }
    out
}

/// Deinterleave an interleaved buffer into separate channel buffers.
#[must_use]
pub fn deinterleave(samples: &[f32], channels: u16) -> Vec<Vec<f32>> {
    let ch = channels as usize;
    if ch == 0 {
        return Vec::new();
    }
    let frames = samples.len() / ch;
    let mut out: Vec<Vec<f32>> = (0..ch).map(|_| Vec::with_capacity(frames)).collect();
    for frame in 0..frames {
        for (c, plane) in out.iter_mut().enumerate() {
            plane.push(samples[frame * ch + c]);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn i16_f32_roundtrip() {
        let original: Vec<i16> = vec![0, 16384, -16384, 32767, -32768];
        let f32s = i16_to_f32(&original);
        let back = f32_to_i16(&f32s);
        for (a, b) in original.iter().zip(back.iter()) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "{a} != {b}");
        }
    }

    #[test]
    fn i32_f32_roundtrip() {
        let original: Vec<i32> = vec![0, 1_073_741_824, -1_073_741_824];
        let f32s = i32_to_f32(&original);
        let back = f32_to_i32(&f32s);
        for (a, b) in original.iter().zip(back.iter()) {
            let tolerance = 256;
            assert!((*a as i64 - *b as i64).abs() <= tolerance, "{a} != {b}");
        }
    }

    #[test]
    fn f32_to_i16_clamps() {
        let samples = vec![2.0, -2.0, 0.5];
        let result = f32_to_i16(&samples);
        assert_eq!(result[0], 32767);
        assert_eq!(result[1], -32767);
    }

    #[test]
    fn i24_f32_roundtrip() {
        let original: Vec<i32> = vec![0, 4_194_304, -4_194_304, 8_388_607, -8_388_608];
        let f32s = i24_to_f32(&original);
        let back = f32_to_i24(&f32s);
        for (a, b) in original.iter().zip(back.iter()) {
            assert!((*a - *b).abs() <= 1, "{a} != {b}");
        }
    }

    #[test]
    fn i24_packed_roundtrip() {
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        let packed = f32_to_i24_packed(&samples);
        assert_eq!(packed.len(), samples.len() * 3);
        let back = i24_packed_to_f32(&packed);
        for (a, b) in samples.iter().zip(back.iter()) {
            assert!((a - b).abs() < 0.001, "{a} != {b}");
        }
    }

    #[test]
    fn interleave_deinterleave_roundtrip() {
        let left = vec![1.0f32, 3.0, 5.0];
        let right = vec![2.0f32, 4.0, 6.0];
        let interleaved = interleave(&[&left, &right]);
        assert_eq!(interleaved, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);

        let planes = deinterleave(&interleaved, 2);
        assert_eq!(planes.len(), 2);
        assert_eq!(planes[0], left);
        assert_eq!(planes[1], right);
    }

    #[test]
    fn deinterleave_empty() {
        let planes = deinterleave(&[], 0);
        assert!(planes.is_empty());
    }

    #[test]
    fn pcm_format_bytes() {
        assert_eq!(PcmFormat::I8.bytes_per_sample(), 1);
        assert_eq!(PcmFormat::I16.bytes_per_sample(), 2);
        assert_eq!(PcmFormat::I24.bytes_per_sample(), 3);
        assert_eq!(PcmFormat::I32.bytes_per_sample(), 4);
        assert_eq!(PcmFormat::F32.bytes_per_sample(), 4);
        assert_eq!(PcmFormat::F64.bytes_per_sample(), 8);
    }

    #[test]
    fn pcm_format_bit_depth() {
        assert_eq!(PcmFormat::I16.bit_depth(), 16);
        assert_eq!(PcmFormat::I24.bit_depth(), 24);
        assert_eq!(PcmFormat::F32.bit_depth(), 32);
    }

    #[test]
    fn f64_f32_roundtrip() {
        let original = vec![0.0f64, 0.5, -0.5, 1.0, -1.0];
        let f32s = f64_to_f32(&original);
        let back = f32_to_f64(&f32s);
        for (a, b) in original.iter().zip(back.iter()) {
            assert!((*a - *b).abs() < 1e-6, "{a} != {b}");
        }
    }

    #[test]
    fn u8_f32_roundtrip() {
        let original: Vec<u8> = vec![0, 64, 128, 192, 255];
        let f32s = u8_to_f32(&original);
        let back = f32_to_u8(&f32s);
        for (a, b) in original.iter().zip(back.iter()) {
            assert!((*a as i16 - *b as i16).abs() <= 1, "{a} != {b}");
        }
    }

    #[test]
    fn u8_center_is_zero() {
        let f32s = u8_to_f32(&[128]);
        assert!((f32s[0]).abs() < f32::EPSILON);
    }
}
