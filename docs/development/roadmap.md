# Development Roadmap

## v1.1.0

- Metadata writing (ID3v2, Vorbis Comment)
- Waveform analysis utilities
- Memory-mapped file support (mmap)
- LPC encoding in FLAC encoder
- Async runtime adapters (tokio, async-std)

## v1.0 Criteria — All Met

- [x] All FLAC subframe types decoded (Constant, Verbatim, Fixed 0-4, LPC 1-32)
- [x] WAV + FLAC encode/decode tested against reference implementations (ffmpeg)
- [x] Streaming API stable (StreamDecoder trait, WAV/FLAC/AIFF decoders)
- [x] Performance within 2x of C reference implementations
- [x] 85%+ test coverage (90%+ excluding platform-conditional dead code)
- [ ] Published on crates.io (user handles)
