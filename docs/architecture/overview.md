# Architecture Overview

## Module Map

```
shravan/
  src/
    lib.rs          -- crate root, feature gates, re-exports
    error.rs        -- ShravanError enum, Result type alias
    format.rs       -- AudioFormat enum, FormatInfo struct, magic byte detection
    pcm.rs          -- PCM format conversions (i16/i24/i32/f32), interleave/deinterleave
    wav.rs          -- WAV (RIFF WAVE) encoder and decoder
    flac.rs         -- FLAC decoder (STREAMINFO, frames, subframes, Rice coding)
    resample.rs     -- Windowed sinc resampler with configurable quality
    codec.rs        -- AudioCodec trait, auto-detect open() function
    tag.rs          -- ID3v2 and Vorbis Comment metadata tag reading
```

## Data Flow

```
Raw bytes --> detect_format() --> AudioFormat
Raw bytes --> codec::open()   --> (FormatInfo, Vec<f32>)
              |
              +-- wav::decode()  --> parse RIFF/fmt/data --> PCM to f32
              +-- flac::decode() --> parse STREAMINFO/frames --> subframes --> f32

f32 samples --> wav::encode() --> RIFF WAVE bytes
f32 samples --> resample()    --> f32 samples at new rate
i16/i24/i32 --> pcm::*_to_f32() --> f32 samples
f32 samples --> pcm::f32_to_*()  --> integer samples
```

## Consumers

- **tarang**: Uses shravan for WAV/FLAC decoding in the media pipeline
- **jalwa**: Uses shravan via tarang for music playback
- **dhvani**: Uses shravan for audio buffer format conversion
- **shruti**: Uses shravan for DAW audio file I/O

## Feature Gates

All codec modules are feature-gated so consumers pull only what they need:

| Feature    | Modules enabled              |
|------------|------------------------------|
| `wav`      | `wav`                        |
| `flac`     | `flac`                       |
| `pcm`      | `pcm`                        |
| `resample` | `resample`                   |
| `tag`      | `tag`                        |
| `logging`  | tracing instrumentation      |
| `std`      | std-dependent functionality  |
