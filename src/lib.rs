//! # shravan
//!
//! **shravan** (Sanskrit: hearing / perception) — Audio codecs for the AGNOS ecosystem.
//!
//! Provides WAV and FLAC decoding/encoding, PCM sample format conversion,
//! sinc resampling, and audio metadata tag reading.
//!
//! ## Feature flags
//!
//! | Feature    | Default | Description                         |
//! |------------|---------|-------------------------------------|
//! | `std`      | yes     | Standard library support             |
//! | `wav`      | yes     | WAV encode/decode                   |
//! | `flac`     | yes     | FLAC decode/encode                  |
//! | `pcm`      | yes     | PCM format conversions              |
//! | `resample` | no      | Sinc resampler                      |
//! | `tag`      | no      | ID3v2 / Vorbis Comment tag reading  |
//! | `simd`     | no      | SIMD-accelerated PCM conversion     |
//! | `dither`   | no      | Dithering for bit-depth reduction   |
//! | `logging`  | no      | tracing instrumentation             |
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use shravan::format::detect_format;
//! use shravan::codec::open;
//!
//! // Auto-detect and decode (requires wav + pcm features)
//! let wav_bytes = shravan::wav::encode(
//!     &[0.0f32; 100], 44100, 1, shravan::pcm::PcmFormat::I16,
//! ).unwrap();
//! let (info, samples) = open(&wav_bytes).unwrap();
//! assert_eq!(info.sample_rate, 44100);
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_code, clippy::unwrap_used, clippy::panic)]
#![warn(missing_docs)]

extern crate alloc;

pub mod codec;
pub mod error;
pub mod format;

#[cfg(feature = "pcm")]
pub mod pcm;

#[cfg(feature = "wav")]
pub mod wav;

#[cfg(feature = "flac")]
pub mod flac;

#[cfg(feature = "resample")]
pub mod resample;

#[cfg(feature = "tag")]
pub mod tag;

#[cfg(feature = "ogg")]
pub mod ogg;

#[cfg(feature = "aiff")]
pub mod aiff;

#[cfg(feature = "mp3")]
pub mod mp3;

#[cfg(feature = "opus")]
pub mod opus;

#[cfg(feature = "simd")]
#[allow(unsafe_code)]
pub mod simd;

#[cfg(feature = "dither")]
pub mod dither;

#[cfg(feature = "streaming")]
pub mod stream;

// Re-exports for convenience.
pub use error::{Result, ShravanError};
pub use format::{AudioFormat, FormatInfo};
