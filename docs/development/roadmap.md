# Development Roadmap

## Completed

### v0.1.0 (2026-03-26) -- Initial Scaffold

- [x] WAV encoder/decoder (PCM 8/16/24/32, IEEE float 32)
- [x] FLAC decoder (Constant, Verbatim, Fixed subframes, Rice coding)
- [x] PCM format conversions (i16, i24, i32, f32)
- [x] Interleave/deinterleave utilities
- [x] Windowed sinc resampler (Draft/Good/Best)
- [x] ID3v2 tag reading (v2.2, v2.3, v2.4)
- [x] Vorbis Comment tag reading
- [x] Auto-detect codec via magic bytes
- [x] AudioCodec trait
- [x] Feature-gated modules
- [x] no_std support
- [x] Serde for all public types
- [x] Integration tests
- [x] Criterion benchmarks

### v0.2.0 (2026-03-28) -- FLAC Completeness

- [x] LPC subframe decoding (orders 1-32)
- [x] FLAC encoder (Fixed prediction, Rice coding, mid-side stereo, MD5)
- [x] CRC-8/CRC-16 verification (decode validation + encode emission)
- [x] Seeking support (`decode_range()`, SEEKTABLE parsing)

## Backlog

### v0.3.0 (2026-03-28) -- Extended Formats

- [x] Ogg container parsing (page demux, packet extraction, CRC-32)
- [x] AIFF decoder/encoder (FORM/AIFF, FORM/AIFC, 80-bit float, BE PCM)
- [x] MP3 frame sync (header parsing, bitrate/sample rate tables, ID3v2 skip)
- [x] Opus header parsing (OpusHead, OpusTags via Ogg)

### v0.4.0 -- Streaming

- [ ] Streaming decoder (chunk-at-a-time)
- [ ] Async I/O support (behind feature gate)
- [ ] Memory-mapped file support

### v0.2.1 (2026-03-28) -- SIMD & Dithering (ported from dhvani)

- [x] SIMD-accelerated PCM conversion (SSE2/NEON, `simd` feature)
- [x] TPDF and noise-shaped dithering (`dither` feature)
- [x] Extended PCM: f64, u8 conversions

## Future

- Multi-channel resampling optimizations
- SIMD-accelerated resampling (weighted_sum kernel ready)
- Metadata writing (ID3v2, Vorbis Comment)
- Waveform analysis utilities

## v1.0 Criteria

- All FLAC subframe types decoded
- WAV + FLAC encode/decode fully tested against reference implementations
- Streaming API stable
- Performance within 2x of C reference implementations
- 90%+ test coverage
- Published on crates.io
