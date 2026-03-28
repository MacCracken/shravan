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

## Backlog

### v0.2.0 -- FLAC Completeness

- [ ] LPC subframe decoding
- [ ] FLAC encoder
- [ ] CRC-8/CRC-16 verification
- [ ] Seeking support

### v0.3.0 -- Extended Formats

- [ ] Ogg container parsing
- [ ] AIFF decoder
- [ ] MP3 frame sync (decode via external crate)
- [ ] Opus header parsing

### v0.4.0 -- Streaming

- [ ] Streaming decoder (chunk-at-a-time)
- [ ] Async I/O support (behind feature gate)
- [ ] Memory-mapped file support

## Future

- SIMD-accelerated PCM conversion
- Multi-channel resampling optimizations
- Metadata writing (ID3v2, Vorbis Comment)
- Waveform analysis utilities

## v1.0 Criteria

- All FLAC subframe types decoded
- WAV + FLAC encode/decode fully tested against reference implementations
- Streaming API stable
- Performance within 2x of C reference implementations
- 90%+ test coverage
- Published on crates.io
