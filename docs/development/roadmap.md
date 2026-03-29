# Development Roadmap

## Backlog

### v0.5.0 -- Async & Performance

- [ ] Async I/O support (behind feature gate)
- [ ] SIMD-accelerated resampling (weighted_sum kernel ready)
- [ ] Multi-channel resampling optimizations

## Future

- Metadata writing (ID3v2, Vorbis Comment)
- Waveform analysis utilities
- Memory-mapped file support (mmap)

## v1.0 Criteria

- All FLAC subframe types decoded
- WAV + FLAC encode/decode fully tested against reference implementations
- Streaming API stable
- Performance within 2x of C reference implementations
- 90%+ test coverage
- Published on crates.io
