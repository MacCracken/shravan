# shravan

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for the AGNOS ecosystem.

WAV and FLAC decoding/encoding, PCM sample format conversion, sinc resampling, and audio metadata tag reading. Zero dependencies on C libraries. Pure Rust with `no_std` support.

## Features

| Feature    | Default | Description                        |
|------------|---------|------------------------------------|
| `std`      | yes     | Standard library support            |
| `wav`      | yes     | WAV encode/decode                  |
| `flac`     | yes     | FLAC decode                        |
| `pcm`      | yes     | PCM format conversions             |
| `resample` | no      | Windowed sinc resampler            |
| `tag`      | no      | ID3v2 / Vorbis Comment tag reading |
| `logging`  | no      | tracing instrumentation            |

## Quick start

```rust
use shravan::codec::open;
use shravan::wav;
use shravan::pcm::PcmFormat;

// Encode samples as WAV
let samples = vec![0.0f32; 44100];
let wav_bytes = wav::encode(&samples, 44100, 1, PcmFormat::I16).unwrap();

// Auto-detect and decode
let (info, decoded) = open(&wav_bytes).unwrap();
assert_eq!(info.sample_rate, 44100);
```

## Modules

- **`wav`** -- RIFF WAVE encoder/decoder supporting PCM 8/16/24/32-bit and IEEE float 32-bit
- **`flac`** -- FLAC decoder with Constant, Verbatim, and Fixed subframes, Rice coding, channel decorrelation
- **`pcm`** -- Sample format conversion (i8/i16/i24/i32/f32/f64), interleave/deinterleave
- **`resample`** -- Windowed sinc interpolation with Draft/Good/Best quality levels
- **`tag`** -- ID3v2 and Vorbis Comment metadata tag reading
- **`codec`** -- Unified auto-detect interface

## License

GPL-3.0-only
