# Development Roadmap

## Future

- Metadata writing (ID3v2, Vorbis Comment)
- Waveform analysis utilities
- Memory-mapped file support (mmap)
- LPC encoding in FLAC encoder
- Async runtime adapters (tokio, async-std)

## v1.0 Criteria

- All FLAC subframe types decoded
- WAV + FLAC encode/decode fully tested against reference implementations
- Streaming API stable
- Performance within 2x of C reference implementations
- 90%+ test coverage
- Published on crates.io
