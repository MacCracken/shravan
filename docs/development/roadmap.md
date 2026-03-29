# Development Roadmap

## Backlog

### v0.4.0 -- Streaming

- [ ] Streaming decoder (chunk-at-a-time)
- [ ] Async I/O support (behind feature gate)
- [ ] Memory-mapped file support

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
