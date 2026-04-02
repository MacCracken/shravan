# Development Roadmap

## v1.1.0

- High-resolution audio support (88.2/96/176.4/192/352.8/384 kHz sample rates, 32-bit integer, 64-bit float)
- Metadata writing (ID3v2, Vorbis Comment)
- Waveform analysis utilities
- Memory-mapped file support (mmap)
- LPC encoding in FLAC encoder
- Async runtime adapters (tokio, async-std)

## Future

### Codec gaps (needed by tarang to drop remaining C deps)

- **Opus encode** — needed to drop `opus` (libopus FFI) dep in tarang
- **AAC decode** — needed to drop `fdk-aac` dep in tarang (decode path)
- **AAC encode** — needed to drop `fdk-aac` dep in tarang (encode path)
- **ALAC decode** — Apple Lossless; previously via symphonia, currently unsupported in tarang

### Other

- DSD support (DSD64/DSD128/DSD256, DoP)

## v1.0 Criteria — All Met

- [x] All FLAC subframe types decoded (Constant, Verbatim, Fixed 0-4, LPC 1-32)
- [x] WAV + FLAC encode/decode tested against reference implementations (ffmpeg)
- [x] Streaming API stable (StreamDecoder trait, WAV/FLAC/AIFF decoders)
- [x] Performance within 2x of C reference implementations
- [x] 85%+ test coverage (90%+ excluding platform-conditional dead code)
- [ ] Published on crates.io (user handles)
