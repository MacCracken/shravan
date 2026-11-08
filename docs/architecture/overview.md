# Architecture Overview

## Module Map

```
shravan/
  src/
    shravan.cyr     -- library: error, format, PCM, WAV, AIFF, ALAC, codec dispatch,
                       decode_file/reader (first [lib] bundle module)
    flac.cyr        -- FLAC decoder (all subframe types, SEEKTABLE, flac_verify), encoder (Fixed)
    ogg.cyr         -- Ogg container parser/muxer (CRC-32, page extraction)
    mp3.cyr         -- MP3 decode -> PCM: MPEG-1/2/2.5 Layer I/II/III (reservoir, Huffman,
                       requant, IMDCT, polyphase synth) + ID3v2 skipping
    tag.cyr         -- ID3v2 + Vorbis Comment reading and writing; compressed ID3v2
                       frames inflated with sankoch
    fft.cyr         -- Mixed-radix FFT (2,3,5), cached radix-2 FFT, MDCT/IMDCT via an
                       N/4-point FFT (direct O(N^2) forms kept as fallback + reference)
    opus.cyr        -- Opus decode (CELT+SILK+hybrid, mono/stereo, 10/20 ms, all bandwidths,
                       bit-exact vs libopus) + RFC-6716 CELT encode (mono 20 ms, interop-verified);
                       OpusHead/OpusTags parsing
    silk.cyr        -- Opus SILK-mode decode (NLSF/LTP/LPC synthesis, resampler, stereo)
    opus_legacy.cyr -- retired 2.5.x bespoke CELT encoder (test-only, EXCLUDED from [lib] bundle)
    aac.cyr         -- AAC-LC encoder/decoder (ADTS, Huffman, M/S, short windows)
    mp4.cyr         -- MP4/M4A/QuickTime demux (box tree, sample table, esds, AAC -> aac_decode, ALAC -> alac_decode_frames)
    resample.cyr    -- Windowed sinc resampler (Draft/Good/Best)
    dither.cyr      -- TPDF + noise-shaped dithering
    simd.cyr        -- SIMD-style PCM conversions (unrolled scalar)
    stream.cyr      -- Incremental streaming decoders (WAV, AIFF, FLAC): any split,
                       buffer bounded to one chunk or frame, no length ceiling
    serde.cyr       -- JSON serialization of format/pcm/error/FormatInfo/... (bayan, #derive(Serialize))
    main.cyr        -- the test harness, every suite                    -> build/shravan
    *_tests.cyr     -- test suites (opus_rfc/opus_encoder/silk/interop) + opus_test_helpers;
                       NOT in [lib]
    interop_fixtures.cyr -- real ffmpeg/flac/lame/libopus output + reference decodes (generated
                       by scripts/gen-interop-fixtures.py); NOT in [lib]
    bench.cyr       -- benchmarks (clock_gettime timing), built on the library itself
  scripts/interop.sh, interop.py, interop/ -- reference sweep (CI job): interop-cli drives
                       shravan's decoders and encoders, opusref.c drives libopus
  fuzz/             -- fuzz_codecs (stubbed lib), fuzz_decode, fuzz_opus; run.sh (seeded
                       batches, cold starts, --poison / --bounds builds)
  dist/
    shravan.cyr     -- distlib bundle (all src [lib] modules concatenated; consumers include this)
  lib/              -- vendored Cyrius stdlib ONLY (alloc, vec, str, math, ganita, io,
                       syscalls, fmt, string, args, assert, tagged, thread, fnptr, …)
                       version-matched to the cyrius.cyml pin, plus the git deps
                       (sankoch, bayan-json) at their tags
```

## Data Flow

```
Raw bytes --> detect_format() --> AudioFormat enum
Raw bytes --> codec_open()    --> decode_result (FormatInfo + samples vec)
               |
               +-- wav_decode()   --> parse RIFF/fmt/data --> PCM to f64
               +-- flac_decode()  --> parse STREAMINFO/frames --> subframes --> f64
               +-- aiff_decode()  --> parse FORM/COMM/SSND --> PCM to f64
               +-- mp4_decode()   --> demux --> AAC: aac_decode, ALAC: alac_decode_frames --> f64
               +-- ogg_decode()   --> extract packets --> opus_decode_from_packets()
               +-- mp3_decode()   --> frame scan --> Layer I/II/III decode --> f64 PCM
               +-- aac_decode()   --> ADTS frames --> ICS/spectral/IMDCT --> f64
               +-- codec_open()   --> also handles FMT_ALAC, FMT_OPUS
                   (every one-shot decode refuses past shravan_set_max_samples: ERR_TOO_LARGE)

Byte chunks --> decode_reader_feed() --> wav/aiff/flac_stream_feed() --> events
               (HEADER, then SAMPLES per chunk_frames or per FLAC frame; ERROR; END on flush)

f64 samples --> wav_encode()   --> RIFF WAVE bytes
f64 samples --> flac_encode()  --> FLAC bitstream (Fixed prediction, CRC, MD5)
f64 samples --> aiff_encode()  --> AIFF bytes (big-endian)
f64 samples --> opus_encode()  --> Ogg/Opus bitstream (CELT mode)
f64 samples --> aac_encode()   --> ADTS/AAC-LC bitstream
f64 samples --> resample()     --> f64 samples at new rate
f64 samples --> dither_tpdf()  --> f64 samples with dither noise

tag_read_id3v2()  --> AudioMetadata struct
tag_write_id3v2() --> ID3v2 bytes
tag_read_vorbis() --> AudioMetadata struct
tag_write_vorbis()--> Vorbis Comment bytes
```

## Consumers

| Consumer | Usage |
|----------|-------|
| **tarang** | media framework -- full codec suite |
| **jalwa** | media player -- decode + playback |
| **dhvani** | audio engine -- PCM, resample, stream |
| **shruti** | DAW -- full codec suite |

## Design Decisions

- **f64 internally**: Cyrius native SSE2 double-precision. Higher precision than f32, simpler code (no f32 bit manipulation needed except for WAV IEEE float I/O).
- **Distlib bundle**: No separate compilation. Consumers `include "dist/shravan.cyr"` (the `cyrius distlib` bundle of `src/shravan.cyr` + codec modules) and get all codecs; they supply stdlib + sankoch from their own manifest, `cyrius deps` pulls `bayan-json` in through shravan's `[deps.bayan]`, and they call `shravan_init_constants()` at startup.
- **Packed Result**: Negative values are errors (bit 63 set). No heap allocation for error paths. `is_err(r)` is a single comparison.
- **Caller-provided buffers**: Encode functions take an output buffer pointer. Decode functions return a vec (heap-allocated via bump allocator).
- **Bump allocator**: No individual free. Decoders presize their output from the declared length and reuse per-frame scratch (FLAC's buffer ring, the Opus and AAC frame arenas), so a one-shot decode allocates about its own output; a stream holds one unfinished chunk or frame, and the sample vecs it emits belong to the caller.
- **Lazy initialisation**: Consumers call only `alloc_init` and `shravan_init_constants`; every codec builds its own tables on first use. `fuzz/run.sh` runs cold-start processes because a warm process hides a table read that precedes its initialiser.
