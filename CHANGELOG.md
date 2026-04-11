# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.1.0] - 2026-04-11

### Added

- **AAC-LC decoder**: from-scratch implementation — bitreader, ICS/section/scale factor parsing, escape-pair spectral decode, inverse quantization (|q|^4/3), IMDCT via fft.cyr, sine window overlap-add. Encode/decode roundtrip verified.
- **AAC M/S stereo**: CPE (Channel Pair Element) parsing, common window flag, per-band and all-band M/S mask, stereo reconstruction (L=M+S, R=M-S). Stereo roundtrip tested.
- **AAC scale factor Huffman codebook**: ISO 14496-3 standard codebook (121 entries). Both encoder and decoder now use real Huffman codes instead of simplified 7-bit fixed encoding.
- **Metadata writing**: `tag_write_id3v2()` and `tag_write_vorbis()` — write/read roundtrip tested for title, artist, album, year, genre fields.
- **FLAC incremental streaming**: `flac_stream_feed` now decodes frame-by-frame via `flac_decode_range` with sample offset tracking, emitting SAMPLES events incrementally instead of buffering everything.
- **FLAC SEEKTABLE parsing**: `flac_parse_metadata` now parses SEEKTABLE blocks (type=3) into SeekPoint structs for fast seeking.
- **FLAC decode_range()**: sample-accurate seeking with SEEKTABLE support.
- **Opus decode path**: `opus_decode_from_packets` parses OpusHead/OpusTags, scans granule positions for duration. `ogg_decode` dispatches to Opus on OpusHead magic.
- **Opus parse_tags()**: reads Vorbis Comment metadata from OpusTags packets.
- **codec_open ALAC/Opus dispatch**: `codec_open` now dispatches all 8 formats including ALAC and Opus.
- **PCM i24 unpacked conversions**: `i24_to_f64()` and `f64_to_i24()` for unpacked i32-stored 24-bit samples.
- **Stream chunk_size**: WAV and AIFF streaming decoders now use `chunk_frames` parameter — emit multiple SAMPLES events of controlled size.
- 21 new test assertions (520 total, 0 failures)

### Changed

- **Compiler requirement**: cc3 >= 3.4.3 (tok_names overflow fix)
- **Rust source removed**: `rust-old/` deleted, preserved at git tag `1.1.0`
- **CI workflows**: bumped to Cyrius 3.4.3, added format check, security scan jobs
- **Benchmarks**: 9 benchmarks (WAV encode/decode, PCM i16/i24/u8, FFT 1024, MDCT 2048, FLAC encode/decode)

### Performance

- FLAC decode 1sec i16: 8.0ms (vs Rust 1.53ms — 5.2x)
- FLAC encode 1sec i16: 20.7ms (vs Rust 2.77ms — 7.5x)
- WAV decode 1sec i16: 2.0ms (vs Rust 47µs — 42x)
- Compile: 420ms full rebuild (vs Rust 6.9s — 16x faster)
- Binary: 313KB (vs Rust 1.83MB — 6x smaller)

## [2.0.0] - 2026-04-10

Full rewrite from Rust to Cyrius. Zero external dependencies.

### Changed

- **Language**: Rust → Cyrius (self-hosting systems language, zero LLVM/crate dependency)
- **Sample format**: f32 → f64 internally (native Cyrius SSE2, higher precision)
- **Error handling**: thiserror enum → packed Result (negative = error, bit 63 set)
- **Build system**: Cargo → `cyrius build` (211ms full compile, 253KB static ELF)
- **Architecture**: Library crate → include-based modules (`lib/*.cyr`)
- Rust implementation archived in `rust-old/` (removed in v2.1.0, preserved at tag `1.1.0`)

### Preserved

- All 16 modules ported: error, format, pcm, codec, wav, aiff, alac, flac, ogg, mp3, tag, fft, opus, aac, resample, dither, simd, stream
- Full encode/decode for WAV, FLAC, AIFF, Opus, AAC
- Decode-only for ALAC, MP3 (header parsing)
- Ogg container parsing/muxing, metadata tag reading (ID3v2, Vorbis Comment)
- Mixed-radix FFT, MDCT/IMDCT, sinc resampler, TPDF + noise-shaped dithering
- Streaming decoders for WAV, FLAC, AIFF
- 415 test assertions, all passing

### Performance

- WAV decode 1sec i16: 1.1ms (vs Rust 22.8µs — ~50x, expected without LLVM optimizer)
- PCM i16→f64 4096: 93µs (vs Rust 429ns — ~217x, scalar vs auto-vectorized)
- Compile: 211ms (vs Rust 5.1s — 24x faster)
- Binary: 253KB (vs Rust 1.8MB — 7x smaller)

## [1.1.0] - 2026-04-02

Four new codecs to eliminate tarang's C FFI audio codec dependencies.

### Added

- **AAC-LC decoder**: ADTS parsing + symphonia-codec-aac backend, mono through 7.1 channels. Feature: `aac` (requires `std`)
- **AAC-LC encoder**: From-scratch MDCT-based encoder producing ADTS bitstream. Per-band quantization, scale factor Huffman coding, escape-pair spectral coding. Mono/stereo, all standard AAC sample rates, CBR. Feature: `aac`
- **Opus CELT encoder**: From-scratch CELT-mode encoder producing valid Ogg/Opus files. Mono/stereo, 48 kHz, CBR 32-256 kbps, 20ms frames. Feature: `opus`
- **ALAC decoder**: From-scratch Apple Lossless decoder for raw frames from MP4. 16/20/24/32-bit, mono/stereo, LPC prediction, adaptive Rice-Golomb coding, stereo de-matrixing. `no_std` compatible. Feature: `alac`
- **Ogg muxer**: Page construction with CRC-32, lacing, BOS/EOS flags
- **Mixed-radix FFT** (`src/fft.rs`): Shared O(N log N) FFT (factors of 2, 3, 5) and MDCT, used by Opus and AAC encoders
- `AudioFormat::Aac`, `AudioFormat::Alac` variants with ADTS format detection
- `AacCodec`, `AlacCodec` structs implementing `AudioCodec` trait
- `AlacConfig` for parsing ALACSpecificConfig from MP4 extradata
- `#[must_use]` on all 22 public `Result`-returning functions
- Opus encode benchmark

### Fixed

- ADTS/MP3 format detection correctly distinguishes AAC (layer=0) from MP3 (layer!=0)
- Opus encoder TOC byte reflects mono-coded bitstream regardless of input channel count

## [1.0.1] - 2026-03-28

### Fixed
- `streaming` feature now compiles without requiring `flac` or `aiff` features — `FlacStreamDecoder` and `AiffStreamDecoder` are properly gated behind their respective feature flags

## [1.0.0] - 2026-03-28

### Added

- **Reference implementation tests**: Validate WAV, FLAC, and AIFF decode against ffmpeg-generated reference files. Cross-format consistency checks (WAV vs FLAC vs AIFF from same source).
- **WAVE_FORMAT_EXTENSIBLE support**: WAV decoder now handles format code 0xFFFE with SubFormat GUID extraction, enabling 24-bit and multi-channel WAV files from professional tools.
- Performance validated within 2x of C reference implementations (libFLAC, dr_wav)
- Test coverage: 85%+ line coverage (90%+ excluding platform-conditional dead code)

## [0.5.0] - 2026-03-28

### Changed

- **SIMD-accelerated resampling**: Inner kernel loop uses `simd::weighted_sum()` when `simd` feature is enabled, replacing manual f64 accumulation with vectorized f32 path
- **Multi-channel resampling optimization**: Deinterleave → per-channel resample → reinterleave for sequential memory access. Significant improvement for stereo and multi-channel audio
- **Async I/O**: `StreamDecoder::feed()` is async-compatible by design (non-blocking, caller-driven). No runtime dependency needed — callers use their async runtime to drive the streaming trait.

## [0.4.0] - 2026-03-28

### Added

- **Streaming decoders**: `StreamDecoder` trait with chunk-at-a-time `feed()`/`flush()` API, `StreamEvent` enum (`Header`, `Samples`, `End`)
- **WavStreamDecoder**: Streaming WAV decoder with configurable chunk size, state-machine header parsing, incremental PCM conversion
- **FlacStreamDecoder**: Streaming FLAC decoder using `decode_range()` with sample offset tracking to avoid duplicate emission
- **AiffStreamDecoder**: Streaming AIFF/AIFF-C decoder with big-endian PCM, `sowt` little-endian support
- **`decode_reader()`**: Read entire `std::io::Read` stream and auto-detect/decode (std-only)
- **`decode_file()`**: Read file from path and auto-detect/decode (std-only)
- Feature gate: `streaming` (requires `std`)

## [0.3.0] - 2026-03-28

### Added

- **Ogg container parser**: Page parsing, packet extraction, CRC-32 verification, continuation page handling, codec detection (delegates to Opus)
- **AIFF decoder/encoder**: FORM/AIFF and FORM/AIFC parsing, COMM chunk with 80-bit extended float sample rate, SSND chunk, big-endian PCM 8/16/24/32-bit decode and encode
- **MP3 frame sync**: Frame header parsing (MPEG 1/2/2.5, Layer I/II/III), bitrate and sample rate tables, frame size calculation, ID3v2 tag skipping, multi-frame scanning
- **Opus header parsing**: OpusHead identification header, OpusTags comment header (via Ogg container), duration from granule position
- Format detection for Ogg (`OggS`), AIFF (`FORM....AIFF`), AIFF-C (`FORM....AIFC`), MP3 (ID3v2 or MPEG sync word)
- Codec structs: `OggCodec`, `AiffCodec`, `Mp3Codec`, `OpusCodec` (all with Serialize/Deserialize)
- Feature gates: `ogg`, `aiff`, `mp3`, `opus` (opus depends on ogg)

### Fixed

- MP3 frame size calculation for Layer II with MPEG-2/2.5 (used wrong samples-per-frame divisor)
- Dither functions now clamp `target_bits` to 1..=32 instead of panicking on 0

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
