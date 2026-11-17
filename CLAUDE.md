# shravan -- Claude Code Instructions

## Project Identity

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for AGNOS

- **Language**: Cyrius
- **License**: GPL-3.0-only
- **Genesis repo**: [agnosticos](https://github.com/MacCracken/agnosticos)
- **Philosophy**: [AGNOS Philosophy & Intention](https://github.com/MacCracken/agnosticos/blob/main/docs/philosophy.md)
- **Standards**: [First-Party Standards](https://github.com/MacCracken/agnosticos/blob/main/docs/development/applications/first-party-standards.md)
- **Recipes**: [zugot](https://github.com/MacCracken/zugot) -- takumi build recipes
- **Ported from**: Rust (10,265 Rust lines -> 11,780 Cyrius lines *at the v2.0.0 port*; src/ has since grown well past that). Rust source removed at v2.0.0, preserved at tag `1.1.0` in git history. See `benchmarks-rust-v-cyrius.md`.

## Architecture

All shravan source lives in `src/`; `lib/` holds only the vendored Cyrius
stdlib snapshot (via `cyrius lib sync`). `src/shravan.cyr` is the **library**
(error, format, PCM, WAV, AIFF, ALAC, codec dispatch, decode_file/reader); the
codec modules (FLAC, Ogg, MP3, Opus, AAC, …) are `src/*.cyr`. The **test harness
is one entry point**, `src/main.cyr`, running the whole 15,550-assertion suite
(core codecs, Opus decode, the CELT and SILK encoders, SILK golden vectors, the real-file
interop fixtures). It was split into three until 2.9.3 for a ~3.15 MB code buffer
that has been a growable 64 MiB region since Cyrius 6.4.49. `cyrius distlib`
concatenates the `[lib]` modules into `dist/shravan.cyr`, the **self-contained bundle
consumers include** (they supply stdlib + sankoch from their own manifest, and
`cyrius deps` pulls `bayan-json` in through shravan's own `[deps.bayan]`);
none of the test files are in `[lib]`.

```
cyrius.cyml      -- manifest (toolchain pin, [deps].stdlib, [lib] bundle, version = ${file:VERSION})
cyrius.lock      -- per-file stdlib hash lock (committed)
src/shravan.cyr  -- the library (core + codec dispatch); first [lib] module
src/*.cyr        -- codec modules (flac, ogg, mp3, tag, fft, opus, silk, silk_enc, opus_legacy, aac, mp4, resample, dither, simd, stream, serde)
src/main.cyr     -- the test harness: every suite                  -> build/shravan
src/*_tests.cyr / opus_test_helpers.cyr -- test suites (opus_rfc_tests, opus_encoder_tests, silk_tests, interop_tests); NOT in [lib]
src/interop_fixtures.cyr -- real ffmpeg/flac/lame output with reference decodes (generated); NOT in [lib]
src/silk_enc_vectors.cyr -- libopus SILK encoder stage inputs/outputs and whole packets (generated); NOT in [lib]
src/bench.cyr    -- benchmarks (clock_gettime timing; includes the library itself)
dist/shravan.cyr -- distlib bundle for consumers (committed; regenerate via `cyrius distlib`)
dist/shravan.deps -- stdlib leaf sidecar `cyrius deps` consumes (emitted alongside the bundle)
lib/             -- vendored Cyrius stdlib snapshot ONLY (cyrius lib sync)
build/           -- compiled binaries (gitignored)
scripts/         -- test-all.sh, bench-history.sh, version-bump.sh,
                    check-write-lengths.py (CI guard: syscall write byte counts),
                    check-collisions.sh (CI guard: dist/shravan.cyr compiled beside its
                    deps, the pinned stdlib and abaco's newest tag; bundles share one
                    namespace, so a shared fn name is replaced for both),
                    gen-interop-fixtures.py (regenerates src/interop_fixtures.cyr),
                    gen-silk-vectors.py + silkref/ (silkdump.h, patch.py, silkref.c,
                    nsqprobe.c): libopus's SILK encoder instrumented to record each
                    stage (stereo included), which regenerates src/silk_enc_vectors.cyr (and checks
                    shravan's packets against libopus's when build/interop-cli exists),
                    interop.sh + interop.py + interop/ (cli.cyr, opusref.c,
                    build-libopus.sh): the reference sweep against ffmpeg/flac/
                    mpg123/libopus (CI job)
fuzz/            -- fuzz_codecs.cyr (stubbed lib), fuzz_decode.cyr (real lib:
                    codec_open/WAV/AIFF/ALAC/FLAC/MP3/Ogg/MP4/AAC/streams/tags,
                    and the FLAC, AAC and SILK encoders on hostile f64 input),
                    fuzz_opus.cyr (damaged libopus packets), run.sh
docs/            -- architecture, roadmap
```

## Build

Toolchain pinned in `cyrius.cyml [package].cyrius` (currently **6.7.18**). The
cyrius toolchain rolls fast, so the pin is a deliberate stability anchor — hold it;
bump only on explicit instruction. Bumping the pin: edit `[package].cyrius` (and
`[deps.NAME].tag`), then run `cyrius deps --relock`, which moves all of `lib/`. Do not
run `cyrius lib sync` first: since 6.7.13 it refuses a pin move while `lib/` holds
transitive leaves outside the declared set, and its suggested `--full` would dump the
whole snapshot into `lib/`. Relock only when the pin really moved; a lock/snapshot
disagreement under an unchanged pin means the snapshot changed. `lib sync` + `deps`
stay correct under an unchanged pin, which is what CI runs. New transitive leaves can
appear in `lib/` untracked and must be added.

```sh
cyrius lib sync                             # vendor [deps].stdlib from the pin (after a pin bump)
cyrius deps                                 # resolve git deps + refresh cyrius.lock
./scripts/test-all.sh                       # build + run the suite (15,550 assertions)
cyrius build src/main.cyr build/shravan    # the same by hand (Cyrius 6.7.18)
./build/shravan
./scripts/interop.sh                        # reference sweep: ffmpeg, flac, mpg123, libopus (needs libopus-dev)
OPUS_PREFIX=<dir> ./scripts/interop.sh      # ... against a default libopus build (scripts/interop/build-libopus.sh <dir>):
                                            # this machine's libopus is deep-PLC, so wideband Opus loss needs it, and
                                            # scripts/gen-interop-fixtures.py refuses to run without one
cyrius build src/bench.cyr build/bench     # compile benchmarks
./build/bench                               # run benchmarks
./fuzz/run.sh 90000                         # hostile-input gate: >= 90K calls per harness + cold starts
./fuzz/run.sh 90000 --poison                # the same under the allocator poison mode (redzones)
./scripts/check-write-lengths.py            # verify syscall write byte counts (--fix to repair)
./scripts/check-collisions.sh               # bundle namespace vs deps, stdlib, abaco (fetches abaco's newest tag)
```

## Modules

| Module | Location | Description |
|--------|----------|-------------|
| error | src/shravan.cyr | ShravanErr enum, packed Result helpers |
| format | src/shravan.cyr | AudioFormat enum, FormatInfo struct, format detection |
| pcm | src/shravan.cyr | Sample format conversion (u8/i16/i24/i32/f32/f64), interleave/deinterleave |
| wav | src/shravan.cyr | RIFF WAVE encode/decode: PCM 1–32-bit (whole-byte containers), IEEE float 32/64, EXTENSIBLE. Header parse (`_wav_header`) and sample layouts (`_pcm_into`) shared with the stream. f32 encode rounds to nearest even |
| aiff | src/shravan.cyr | AIFF/AIFF-C encode/decode (big-endian, 80-bit extended sample rate; AIFF-C NONE/twos/sowt/fl32/fl64, other compressions refused) |
| alac | src/shravan.cyr | Apple Lossless decoder: adaptive Rice and LPC, stereo unmix, shifted bytes, surround channel order. `alac_decode_frames` takes container frames; MP4/QuickTime route to it. Bit-exact vs ffmpeg |
| codec | src/shravan.cyr | Auto-detect format and dispatch to decoder. One-shot decodes stop at `shravan_set_max_samples` (default 2^26 interleaved samples) with `ERR_TOO_LARGE`, presized from the declared length |
| flac | src/flac.cyr | FLAC decode (all subframe types to 32-bit, Rice coding, channel decorrelation; one frame decoder, `_flac_frame`, shared by one-shot, range and stream; `flac_verify` checks the STREAMINFO MD5) and encode: `flac_encode_level` 0-8, the levels of `flac -0` to `-8` (LPC to order 12 over tukey/subdivide_tukey windows, partitioned Rice, stereo decorrelation, wasted bits, CONSTANT/VERBATIM, a seek table every 10 s; fixed scratch, nothing proportional to the input), `flac_encode` = level 5. Decode bit-exact vs the flac CLI; encode sizes within 0.1% of `flac -L`, libFLAC and ffmpeg decode it exactly |
| ogg | src/ogg.cyr | Ogg container parse/mux (CRC32, page extraction, lacing) |
| mp3 | src/mp3.cyr | MP3 decode → PCM: MPEG-1/2/2.5 Layer III (reservoir, Huffman, requant, IMDCT, polyphase synth) + Layer II + Layer I (subband PCM), free format, LAME/Xing/VBRI frames skipped, the LAME tag's delay and padding trimmed (gapless, as ffmpeg plays it; `shravan_set_gapless(0)` keeps every frame whole). ISO conformance vectors within 1 LSB; Layer III exact vs ffmpeg at all 9 rates |
| tag | src/tag.cyr | ID3v2 and Vorbis Comment metadata tag reading and writing. ID3v2 de-unsynchronisation, v2.3 and v2.4 frame flags, zlib-compressed frames inflated with sankoch (per-frame and per-tag limits) |
| fft | src/fft.cyr | Mixed-radix FFT (2,3,5); cached radix-2 FFT for powers of two; MDCT/IMDCT through an N/4-point FFT (the O(N²) direct forms kept as fallback and test reference) |
| opus | src/opus.cyr | RFC-6716 Opus **decode** → PCM: CELT + SILK + hybrid + stereo, **10 ms and 20 ms** frames, all bandwidths (`opus_decode_from_packets` dispatch, bit-exact vs libopus). Multistream (RFC 7845 families 0, 1, 2, 255): self-delimited stream packets (RFC 6716 Appendix B), a stream decoder each; family 1 in ffmpeg's channel order, 2 and 255 in mapping order. Both CELT MDCTs run an N/4-point FFT (`_celt_dft`: 15 × 2^a by Cooley-Tukey, a Good-Thomas 15-point kernel, radix 2); the direct DFT it replaced in 2.10.4 was 95% of a CELT decode. RFC-6716 CELT **encode** (`celt_encode_frame_ec`: fwd MDCT → energy → allocation → PVQ → range coder) — a real interoperable CELT frame (mono, 20 ms; libopus decodes it sample-identically). The range encoder (`ec_enc_new`/`ec_enc_init`, `ec_enc_patch_initial_bits`) is shared with the SILK encoder. Hybrid encode = 2.12. |
| silk | src/silk.cyr | Opus SILK-mode decode: NLSF/LTP/LPC synthesis + resampler + stereo (mid/side), 10/20 ms, bit-exact vs libopus — part of the RFC-6716 Opus decode path. Also the encoder's quantisers and writers (gains, NLSF, indices, pulses) |
| silk_enc | src/silk_enc.cyr | SILK encoder, an f64 port of libopus's float encoder (2.11.x, 2.12.0–2.12.2). 2.11.0: LPC analysis — `silk_burg_modified_flp`, `silk_find_lpc_flp` (NLSF interpolation search), `silk_process_nlsfs`, `silk_residual_energy_flp`, the unvoiced `silk_find_pred_coefs_flp`. 2.11.1: `silk_encode_do_vad` (the VAD, fixed point, exact), `silk_find_pitch_lags_flp` up to its voicing decision, `silk_noise_shape_analysis_flp` (warped and voiced branches included), `silk_process_gains_flp`, and the parameters (`silk_enc_setup_complexity`, `silk_control_snr`). 2.11.2: `silk_nsq` (the noise shaping quantiser, fixed point, bit-exact: pulses and whole state) and `silk_nsq_wrapper_flp` (`_sk_nsq_convert`, exact). 2.11.3: `silk_encode_frame_flp` (the stages in order, the gainMult rate loop with its snapshots, damage control), the packet layer (`_silk_encode_packet` = mono `silk_Encode` at the internal rate: FLOAT2INT16, the equal-rate resampler's delay, flags patched in, per-frame targets and the reservoir), and the public encoder: `silk_encoder_new(fs_hz, ms, bitrate, complexity 0-1, cbr)`, `silk_encode` (bare payload), `silk_encode_opus` (Opus packet: TOC, CBR padding, the PLC frame). 2.12.0: the voiced path — `silk_pitch_analysis_core_flp` (decimation through `silk_resampler_down2`/`down2_3` in fixed point, three stages, estimator complexity 0–2), the voicing decision in `silk_find_pitch_lags_flp` (`_sk_pitch_threshold`), `silk_find_ltp_flp`, `silk_quant_ltp_gains` + `silk_vq_wmat_ec` (fixed point, exact), `silk_ltp_scale_ctrl_flp`, `silk_ltp_analysis_filter_flp`, the voiced `silk_find_pred_coefs_flp(st, ctrl, res_pitch, x, cond)`, `silk_hp_variable_cutoff` (state for the Opus layer), and `silk_encoder_set_packet_loss`. 2.12.1: input resampling (`silk_enc_resampler_new`/`silk_enc_resample`: copy or AR2 + down-FIR at 3/4 to 1/6, exact), in-band FEC (`silk_lbrr_encode_flp`, LBRR frames written in the next packet, the LBRR bit accounting), DTX, and `silk_encoder_new_api(api_hz, fs_hz, ...)`, `silk_encoder_set_fec`, `silk_encoder_set_dtx`. 2.12.2: stereo — `silk_stereo_lr_to_ms` (mid/side, the side's predictors from the mid's low and high bands, the rate split, width smoothing, collapse and mid-only frames; fixed point, exact, its hot loops inline), `silk_stereo_find_predictor`, `silk_stereo_quant_pred`, `silk_stereo_encode_pred`/`_mid_only`; the two-channel packet layer (the side's reset after mid-only frames, mono/stereo moves, stereo input coded mono, DTX of both); `silk_encoder_new_channels(api_hz, fs_hz, channels, ...)`, `silk_encoder_set_channels`, `silk_encoder_set_to_mono`, `silk_encoder_set_bitrate`, `silk_encoder_stereo_width`; `silk_enc_state_reset` (in place). Each stage is checked on libopus's own inputs (`src/silk_enc_vectors.cyr`, zlib-compressed fields the suite inflates once): float stages to float precision, fixed-point ones exactly; whole packets byte for byte (2,537: voiced and unvoiced, every input rate pair, FEC, DTX; on inputs `_skk_signal` builds: kind 6, noise through a comb, is voiced but well-conditioned), stereo packets by length and FNV-1a hash (2,724 on `_skk_stereo`'s inputs), `silk_stereo_lr_to_ms` on 15 of libopus's frames. Float→fixed rounds as `lrintf` (`_sk_lrint`) |
| opus_legacy | src/opus_legacy.cyr | Retired 2.5.x bespoke CELT encoder/decoder + old Opus encoder framework — **off the RFC path**, kept only for its tests, included by the test harnesses but **excluded from `[lib]`** (holds `opus.cyr` under the 256 KB distlib cap) |
| aac | src/aac.cyr | AAC-LC encode/decode (ADTS), ISO/IEC 14496-3. Decode: SCE + CPE + LFE, any layout: channel configurations 1–7 and 11–14, or a PCE (in the ADTS stream or the MP4 AudioSpecificConfig); elements placed and channels ordered as ffmpeg does (`_aac_che_slot` = ff_aac_get_che, `_aac_layout_finish` = sniff_channel_order); `_aac_decode_frames` is the core behind ADTS and MP4. All 13 sample rates (`_aac_select_swb`), long/short windows, sine+KBD, full TNS, PNS, intensity, per-band M/S, ISO codebooks 1--11, independent windows. Exact vs ffmpeg; PNS bands at exactly the transmitted energy (decode energy within 1e-5 of ffmpeg's). Encode: 3GPP TS 26.403 psychoacoustics (spreading, ATH, pre-echo control, tonality-adjusted SMR), block switching (LONG_START / EIGHT_SHORT / LONG_STOP, groups by energy), TNS with thresholds in the filtered domain, a rate loop over the threshold scale with exact bit counts and a reservoir that starts full, M/S by perceptual entropy, PNS (at most 24 kbit/s a channel) and intensity (at most 48). ffmpeg and faad decode it at every rate, and `scripts/interop.py` holds it within 0.02 of ffmpeg's own encoder |
| mp4 | src/mp4.cyr | MP4/M4A and QuickTime demux: box tree, sample table, 64-bit sizes and offsets, sample entry v0/v1/v2, esds/AudioSpecificConfig (GASpecificConfig: configurations 11–14, a PCE; 960-sample frames refused). AAC access units → `_aac_decode_frames` directly, ALAC → alac_decode_frames, HE-AAC reported unsupported. Every audio track by index (`mp4_track_count`, `mp4_track_info`, `mp4_decode_track`); `mp4_decode` takes ffmpeg's default (enabled, then most channels, then first, of the tracks shravan decodes). Gapless: the sample table's durations, the edit list (sample-exact, several edits concatenated, empty edits add nothing) and, on AAC, iTunSMPB |
| resample | src/resample.cyr | Windowed sinc interpolation (Draft/Good/Best quality) |
| dither | src/dither.cyr | Dithering for sample depth reduction |
| simd | src/simd.cyr | SIMD-optimized inner loops |
| stream | src/stream.cyr | Incremental streaming decoders (WAV/AIFF/FLAC; `decode_reader` in shravan.cyr dispatches): sample-identical to one-shot however the input is split, buffer bounded to one chunk or frame, no length ceiling, ERROR event on corrupt input |
| serde | src/serde.cyr | JSON serialization — full type surface: format/pcm/error + AlacConfig/Mp3FrameInfo/OpusHead/AudioMetadata + mp3/resample enums + codec markers (`#derive(Serialize)` + bayan) |
| sankoch | dep (cyrius.cyml) | Compression (LZ4, DEFLATE, zlib, gzip). shravan's call site: `zlib_decompress_capped` for compressed ID3v2 frames (src/tag.cyr) |

## Consumer Map

| Consumer | Usage |
|----------|-------|
| tarang | media framework -- full codec suite |
| jalwa | media player -- decode + playback |
| dhvani | audio engine -- PCM, resample, stream |
| shruti | DAW -- full codec suite |

## Development Process

### P(-1): Scaffold Hardening (before any new features)

0. Read roadmap, CHANGELOG, and open issues -- know what was intended before auditing what was built
1. Test + benchmark sweep of existing code
2. Cleanliness check: `./scripts/test-all.sh`, verify all 15,550 assertions pass
3. Get baseline benchmarks (`./scripts/bench-history.sh`)
4. Internal deep review (performance, memory, correctness, edge cases)
5. External research -- audio codec specs (WAV, FLAC, AIFF, Ogg, MP3, Opus, AAC, ALAC), PCM standards
6. Cleanliness check -- must be clean after review
7. Additional tests/benchmarks from findings
8. Post-review benchmarks -- prove the wins
9. Repeat if heavy
10. Documentation audit -- ADRs, source citations, guides, examples

### Work Loop (continuous)

1. Work phase -- new codecs, improvements, optimizations
2. Cleanliness check: `./scripts/test-all.sh`, all tests pass
3. Test + benchmark additions for new code
4. Run benchmarks (`./scripts/bench-history.sh`)
5. Internal review -- correctness, performance, memory safety
6. Cleanliness check -- must be clean after review
7. Deeper tests/benchmarks from review observations
8. Run benchmarks again -- prove the wins
9. If review heavy -> return to step 5
10. Documentation -- update CHANGELOG, roadmap, docs
11. Version check -- `VERSION` is the source of truth (`cyrius.cyml` derives it via `${file:VERSION}`); keep VERSION + CHANGELOG in sync
12. Return to step 1

### Key Principles

- **Never skip benchmarks.** Numbers don't lie. The CSV history is the proof.
- **Tests + benchmarks are the way.**
- **All samples are f64 internally** -- Cyrius native, higher precision than f32.
- **Zero panics** -- return error codes, never abort.
- **Caller-provided buffers** where possible -- avoid heap allocation in hot paths.
- **A round-trip through shravan's own encoder proves nothing.** Decoders are judged by maximum sample error against a reference on real encoder output, encoders by what that reference does with their output. New decode paths need a fixture from `scripts/gen-interop-fixtures.py`, and a fix is proven by reverting it and watching that fixture fail.
- **Every codec initialises its own tables, lazily.** Consumers call only `alloc_init` and `shravan_init_constants`. A table read before its initialiser runs is invisible in a warm process (the suite, a long fuzz batch); `fuzz/run.sh`'s cold starts exist to catch it.
- **All tests must pass** before any changes are considered complete.

## Conventions

- Error encoding: packed Result -- ok >= 0, err < 0, code via `err_code(r)`. `is_err`/`err_code` come from the stdlib (`syscalls`, via `io`) — byte-identical; shravan's own positive check is `res_ok(r)` (not `is_ok`, which the stdlib's `result.cyr` owns for tagged Results)
- Error codes: integer enum `ShravanErr`, messages via `err_print(code)` helper
- f64 constants: `F64_ONE`/`F64_TWO`/`F64_HALF` come from `math.cyr`; shravan inits the PCM-specific ones at startup via `shravan_init_constants()`, never hardcoding bit patterns
- f64 arithmetic: builtins `f64_add`, `f64_mul`, `f64_div`, `f64_sub`, `f64_neg`, etc.
- `>>` is logical (unsigned) shift -- use conditional subtraction for sign extension
- Entry point is top-level statements, not `fn main()`
- Large buffers via `alloc()`, not stack arrays (code buffer limit)
- Feature-gate modules via `#ifdef` / `-D` flags
- Structs: heap-allocated via `alloc()`, accessed via `store64`/`load64` at fixed offsets
- Benchmarks: `clock_gettime` timing, CSV history via `scripts/bench-history.sh`

## DO NOT

- **Do not commit or push** -- the user handles all git operations (commit, push, tag)
- **NEVER use `gh` CLI** -- use `curl` to GitHub API only
- Do not add unnecessary dependencies -- keep it lean
- Do not skip benchmarks before claiming performance improvements
- Do not commit `build/` directory
- Do not break backward compatibility without a major version bump
- Do not use reserved keywords as variable names (`match`, `default`, `shared`, `in`, `secret`)

## Documentation Structure

```
Root files (required):
  README.md, CHANGELOG.md, CLAUDE.md, CONTRIBUTING.md, SECURITY.md, CODE_OF_CONDUCT.md, LICENSE

docs/ (required):
  architecture/overview.md -- module map, data flow, consumers
  development/roadmap.md -- completed, backlog, future, v1.0 criteria

docs/ (when earned):
  adr/ -- architectural decision records
  guides/ -- usage guides, integration patterns
  examples/ -- worked examples
  standards/ -- external spec conformance
  sources.md -- source citations for algorithms/formulas
```
