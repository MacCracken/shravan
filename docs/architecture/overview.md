# Architecture Overview

## Module Map

```
shravan/
  src/
    shravan.cyr     -- library: error, format, PCM, WAV, AIFF, ALAC, codec dispatch,
                       decode_file/reader (first [lib] bundle module)
    flac.cyr        -- FLAC decoder (all subframe types, SEEKTABLE, flac_verify), encoder (levels 0-8
                       as flac -0..-8: LPC, partitioned Rice, stereo, wasted bits, seek table)
    ogg.cyr         -- Ogg container parser/muxer (CRC-32, page extraction)
    mp3.cyr         -- MP3 decode -> PCM: MPEG-1/2/2.5 Layer I/II/III (reservoir, Huffman,
                       requant, IMDCT, polyphase synth) + ID3v2 skipping
    tag.cyr         -- ID3v2 + Vorbis Comment reading and writing; compressed ID3v2
                       frames inflated with sankoch
    fft.cyr         -- Mixed-radix FFT (2,3,5), cached radix-2 FFT, MDCT/IMDCT via an
                       N/4-point FFT (direct O(N^2) forms kept as fallback + reference)
    opus.cyr        -- Opus decode (CELT+SILK+hybrid, mono/stereo, every TOC configuration, all bandwidths,
                       bit-exact vs libopus; multistream, RFC 7845 families 0/1/2/255;
                       the CELT MDCTs over a 15 x 2^a FFT) +
                       RFC-6716 CELT encode (mono 20 ms, interop-verified); OpusHead/OpusTags parsing
    silk.cyr        -- Opus SILK-mode decode (NLSF/LTP/LPC synthesis, resampler, stereo);
                       the encoder's quantisers and bitstream writers
    silk_enc.cyr    -- SILK encoder (2.11.x, 2.12.0-2.12.3): input resampling (8-48 kHz to
                       the internal rate), VAD, pitch estimation (decimation,
                       three stages, the voicing decision), noise shaping, gains, LPC
                       analysis (Burg, NLSF interpolation search and quantisation),
                       long-term prediction (correlations, gain VQ, scaling by loss),
                       the noise shaping quantiser (NSQ) and its delayed-decision form
                       (complexity 2-10), the frame and its rate loop,
                       stereo (left/right to mid/side, the side's predictor, the rate
                       split and width, mid-only frames), the packet (one or two
                       channels, in-band FEC's LBRR frames, DTX, mono/stereo moves),
                       the pitch-tracking high-pass smoother, the per-packet control
                       (internal rate switching through the low-pass transition, the
                       resampler hand-over), silk_encoder_reset,
                       silk_encoder_new_channels / silk_encode / silk_encode_opus;
                       checked stage by stage against libopus, and whole packets byte
                       for byte, voiced and unvoiced, mono and stereo (its float stages
                       in libopus's single precision since 2.12.6, hostile input too)
    opus_enc.cyr    -- the Opus encoder layer (libopus opus_encode, 2.12.3), application
                       OPUS_APPLICATION_RESTRICTED_SILK: the 3 Hz high-pass, the bit rate,
                       stereo/mono and bandwidth decisions, FEC by loss, DTX on silence,
                       10-120 ms packets (80-120 repacketised), CBR padding, PLC packets,
                       libopus's ctls and getters; byte for byte libopus's
    celt_enc.cyr    -- libopus's CELT encoder (celt_encode_with_ec, 2.12.4) as its float build
                       computes it (typed f32 arithmetic, libopus's operation order): pre-emphasis,
                       tone and transient analysis, the pitch pre-filter, the forward MDCT (kiss
                       FFT), band energies, dynalloc, TF, coarse and fine energy, spreading, trim,
                       stereo decisions, VBR, the allocation, the bands (PVQ, the stereo theta
                       search with its resynthesis); celt_encoder_new / _encode / _encode_ec and
                       libopus's ctls; frame for frame libopus's portable C build's
    celt_enc_tables.cyr -- generated (scripts/gen-celt-tables.py): libopus's static float mode and
                       the CELT encoder's tables, parsed from its sources
    opus_legacy.cyr -- retired 2.5.x bespoke CELT encoder (test-only, EXCLUDED from [lib] bundle)
    aac.cyr         -- AAC-LC encoder/decoder (ADTS, Huffman, TNS, M/S, PNS, intensity, short windows;
                       decode: any channel layout, configurations or a PCE, in ffmpeg's order;
                       encoder psy + rate loop)
    mp4.cyr         -- MP4/M4A/QuickTime demux (box tree, sample table, esds/AudioSpecificConfig,
                       AAC -> _aac_decode_frames, ALAC -> alac_decode_frames)
    resample.cyr    -- Windowed sinc resampler (Draft/Good/Best)
    dither.cyr      -- TPDF + noise-shaped dithering
    simd.cyr        -- SIMD-style PCM conversions (unrolled scalar)
    stream.cyr      -- Incremental streaming decoders (WAV, AIFF, FLAC): any split,
                       buffer bounded to one chunk or frame, no length ceiling
    serde.cyr       -- JSON serialization of format/pcm/error/FormatInfo/... (bayan, #derive(Serialize))
    main.cyr        -- the test harness, every suite                    -> build/shravan
    *_tests.cyr     -- test suites (opus_rfc/opus_encoder/silk/opus_enc/celt_enc/interop) + opus_test_helpers;
                       NOT in [lib]
    interop_fixtures.cyr -- real ffmpeg/flac/lame/libopus output + reference decodes (generated
                       by scripts/gen-interop-fixtures.py); NOT in [lib]
    bench.cyr       -- benchmarks (clock_gettime timing), built on the library itself
  scripts/interop.sh, interop.py, interop/ -- reference sweep (CI job): interop-cli drives
                       shravan's decoders and encoders, opusref.c drives libopus, and
                       silkref/opusenc.c libopus's opus_encode (build/opusenc-ref)
  scripts/check-collisions.sh -- compiles dist/shravan.cyr beside its deps, the pinned stdlib
                       snapshot and abaco's newest tag; fails on a shared name (CI step)
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
               +-- mp4_decode()   --> default track --> demux --> AAC: _aac_decode_frames (layout from the
                                  AudioSpecificConfig), ALAC: alac_decode_frames
                                  --> gapless: stts durations, edit list or iTunSMPB --> f64
               +-- ogg_decode()   --> extract packets --> opus_decode_from_packets() --> per packet, its
                                  streams (self-delimited) --> a stream decoder each --> channels by the mapping
               +-- mp3_decode()   --> frame scan --> Layer I/II/III decode --> LAME tag delay/padding --> f64 PCM
               +-- aac_decode()   --> ADTS frames --> layout (configuration or PCE) --> per block, its
                                  elements (SCE/CPE/LFE) --> ICS/spectral/IMDCT --> channels in ffmpeg order --> f64
               +-- codec_open()   --> also handles FMT_ALAC, FMT_OPUS
                   (every one-shot decode refuses past shravan_set_max_samples: ERR_TOO_LARGE)

Byte chunks --> decode_reader_feed() --> wav/aiff/flac_stream_feed() --> events
               (HEADER, then SAMPLES per chunk_frames or per FLAC frame; ERROR; END on flush)

f64 samples --> wav_encode()   --> RIFF WAVE bytes
f64 samples --> flac_encode_level() --> FLAC bitstream, levels 0-8 as `flac -0..-8` (LPC,
               partitioned Rice, stereo decorrelation, wasted bits, seek table, CRC, MD5)
f64 samples --> aiff_encode()  --> AIFF bytes (big-endian)
f64 samples --> opus_encode()  --> Opus packets (OPUS_APPLICATION_RESTRICTED_SILK): 3 Hz
               high-pass, then per packet the rate, channels, bandwidth and FEC decisions,
               silk_Encode (silk_enc.cyr) at the internal rate they settle on, the TOC,
               DTX on silence, 80-120 ms as 2-5 frames repacketised, CBR padding
f64 samples --> aac_encode()   --> ADTS/AAC-LC bitstream: per frame, attack detection (one frame
               ahead) picks LONG / LONG_START / EIGHT_SHORT / LONG_STOP; MDCT; 3GPP TS 26.403
               thresholds (spreading, ATH, pre-echo control, tonality from a 2048-point FFT);
               TNS; PNS and intensity at low rates; M/S by perceptual entropy; a threshold-scale
               search with exact bit counts against the 3GPP bit demand and a reservoir;
               1024 samples of priming, so a decode is the input delayed by one frame
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
- **Bump allocator**: No individual free. Decoders presize their output from the declared length and reuse per-frame scratch (FLAC's buffer ring, the Opus and AAC frame arenas), so a one-shot decode allocates about its own output; the FLAC encoder keeps one block's scratch and hashes frame by frame, so it allocates nothing proportional to its input; a stream holds one unfinished chunk or frame, and the sample vecs it emits belong to the caller.
- **Lazy initialisation**: Consumers call only `alloc_init` and `shravan_init_constants`; every codec builds its own tables on first use. `fuzz/run.sh` runs cold-start processes because a warm process hides a table read that precedes its initialiser.
