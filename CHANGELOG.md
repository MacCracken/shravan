# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-03-28

### Added

- **SIMD-accelerated PCM conversion**: SSE2 (x86_64) + NEON (aarch64) kernels for i16/f32 conversion and weighted dot product, behind `simd` feature gate
- **Dithering module**: TPDF and noise-shaped dithering for bit-depth reduction, behind `dither` feature gate
- **Extended PCM conversions**: `f64_to_f32`, `f32_to_f64`, `u8_to_f32`, `f32_to_u8`

## [0.2.0] - 2026-03-28

### Added

- **FLAC encoder**: Fixed prediction (orders 0-4) with automatic order selection, Rice entropy coding with optimal parameter selection, mid-side stereo channel decorrelation, MD5 signature computation
- **LPC subframe decoding**: Full support for LPC orders 1-32 with quantized coefficients and variable precision
- **CRC verification**: CRC-8 (frame header) and CRC-16 (full frame) validation on decode, correct CRC emission on encode
- **FLAC seeking**: `decode_range()` for sample-accurate seeking, SEEKTABLE metadata block parsing, range-based decoding with start/end sample positions
- **BitWriter**: MSB-first bitstream writer for FLAC frame construction
- FLAC encode/decode benchmarks (Criterion)

### Fixed

- `resample()` now rejects `source_rate=0` instead of panicking with capacity overflow
- WAV chunk parser uses saturating arithmetic to prevent overflow on malicious chunk sizes

## [0.1.0] - 2026-03-26

### Added

- **WAV codec**: RIFF WAVE encoder/decoder supporting PCM 8/16/24/32-bit integer and IEEE float 32-bit
- **FLAC decoder**: STREAMINFO parsing, frame sync, Constant/Verbatim/Fixed subframes, Rice entropy coding, channel decorrelation (independent, left-side, right-side, mid-side)
- **PCM conversions**: i16/i24/i32 to f32 and back, interleave/deinterleave, packed 24-bit support
- **Sinc resampler**: Windowed sinc interpolation with Blackman-Harris window, Draft/Good/Best quality levels
- **Tag reading**: ID3v2 (v2.2/v2.3/v2.4) and Vorbis Comment metadata parsing
- **Codec trait**: Unified `AudioCodec` trait and auto-detect `open()` function
- **Format detection**: Magic byte detection for WAV and FLAC
- Feature-gated modules: `wav`, `flac`, `pcm`, `resample`, `tag`, `logging`
- `no_std` support (with `alloc`)
- Serde serialization for all public types
- Comprehensive test suite with integration tests
- Criterion benchmarks for WAV decode, PCM conversion, and resampling
