//! # shravan
//!
//! **shravan** (Sanskrit: hearing / perception) — Audio codecs for the AGNOS ecosystem.
//!
//! Provides encode/decode for WAV, FLAC, AIFF, Ogg/Opus, AAC, ALAC, and MP3,
//! plus PCM sample format conversion, sinc resampling, and metadata tag reading.
//!
//! ## Feature flags
//!
//! | Feature     | Default | Description                              |
//! |-------------|---------|------------------------------------------|
//! | `std`       | yes     | Standard library support                  |
//! | `wav`       | yes     | WAV encode/decode                        |
//! | `flac`      | yes     | FLAC encode/decode                       |
//! | `pcm`       | yes     | PCM format conversions                   |
//! | `resample`  | no      | Sinc resampler                           |
//! | `tag`       | no      | ID3v2 / Vorbis Comment tag reading       |
//! | `ogg`       | no      | Ogg container parsing/muxing             |
//! | `aiff`      | no      | AIFF/AIFF-C encode/decode                |
//! | `mp3`       | no      | MP3 frame parsing (header only)          |
//! | `opus`      | no      | Opus header parsing + CELT-mode encode   |
//! | `aac`       | no      | AAC-LC decode (ADTS) + encode (requires `std`) |
//! | `alac`      | no      | Apple Lossless decode (raw frames)       |
//! | `simd`      | no      | SIMD-accelerated PCM conversion          |
//! | `dither`    | no      | Dithering for bit-depth reduction        |
//! | `streaming` | no      | Streaming decoders (requires `std`)      |
//! | `logging`   | no      | tracing instrumentation                  |
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
pub(crate) mod fft;
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

#[cfg(feature = "aac")]
pub mod aac;

#[cfg(feature = "alac")]
pub mod alac;

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
