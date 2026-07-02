# shravan -- Claude Code Instructions

## Project Identity

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for AGNOS

- **Language**: Cyrius
- **License**: GPL-3.0-only
- **Genesis repo**: [agnosticos](https://github.com/MacCracken/agnosticos)
- **Philosophy**: [AGNOS Philosophy & Intention](https://github.com/MacCracken/agnosticos/blob/main/docs/philosophy.md)
- **Standards**: [First-Party Standards](https://github.com/MacCracken/agnosticos/blob/main/docs/development/applications/first-party-standards.md)
- **Recipes**: [zugot](https://github.com/MacCracken/zugot) -- takumi build recipes
- **Ported from**: Rust (10,265 lines -> 11,780 lines Cyrius). Rust source removed at v2.0.0, preserved at tag `1.1.0` in git history. See `benchmarks-rust-v-cyrius.md`.

## Architecture

All shravan source lives in `src/`; `lib/` holds only the vendored Cyrius
stdlib snapshot (via `cyrius lib sync`). `src/shravan.cyr` is the **library**
(error, format, PCM, WAV, AIFF, ALAC, codec dispatch, decode_file/reader); the
codec modules (FLAC, Ogg, MP3, Opus, AAC, …) are `src/*.cyr`. `src/main.cyr` is
the **test harness** — it `include`s the library + codecs and runs the 796-assertion
suite. `cyrius distlib` concatenates the `[lib]` modules into `dist/shravan.cyr`,
the **self-contained bundle consumers include** (they supply stdlib + bayan +
sankoch from their own manifest).

```
cyrius.cyml      -- manifest (toolchain pin, [deps].stdlib, [lib] bundle, version = ${file:VERSION})
cyrius.lock      -- per-file stdlib hash lock (committed)
src/shravan.cyr  -- the library (core + codec dispatch); first [lib] module
src/*.cyr        -- codec modules (flac, ogg, mp3, tag, fft, opus, aac, resample, dither, simd, stream, serde)
src/main.cyr     -- test harness entry (includes shravan.cyr + codecs, runs tests)
src/bench.cyr    -- benchmarks (clock_gettime timing)
dist/shravan.cyr -- distlib bundle for consumers (committed; regenerate via `cyrius distlib`)
lib/             -- vendored Cyrius stdlib snapshot ONLY (cyrius lib sync)
build/           -- compiled binaries (gitignored)
scripts/         -- bench-history.sh, version-bump.sh
docs/            -- architecture, roadmap
```

## Build

Toolchain pinned in `cyrius.cyml [package].cyrius` (currently **6.3.25**). Bumping
the pin requires re-vendoring stdlib: `cyrius lib sync`.

```sh
cyrius lib sync                             # vendor [deps].stdlib from the pin (after a pin bump)
cyrius deps                                 # resolve git deps + refresh cyrius.lock
cyrius build src/main.cyr build/shravan    # compile (Cyrius 6.3.25)
./build/shravan                             # run tests (727 assertions)
cyrius build src/bench.cyr build/bench     # compile benchmarks
./build/bench                               # run benchmarks
```

## Modules

| Module | Location | Description |
|--------|----------|-------------|
| error | src/shravan.cyr | ShravanErr enum, packed Result helpers |
| format | src/shravan.cyr | AudioFormat enum, FormatInfo struct, format detection |
| pcm | src/shravan.cyr | Sample format conversion (u8/i16/i24/i32/f32/f64), interleave/deinterleave |
| wav | src/shravan.cyr | RIFF WAVE encode/decode (PCM 8/16/24/32-bit, IEEE float 32-bit) |
| aiff | src/shravan.cyr | AIFF encode/decode (big-endian, 80-bit extended sample rate) |
| alac | src/shravan.cyr | Apple Lossless Audio Codec decoder |
| codec | src/shravan.cyr | Auto-detect format and dispatch to decoder |
| flac | src/flac.cyr | FLAC encode/decode (all subframe types, Rice coding, channel decorrelation) |
| ogg | src/ogg.cyr | Ogg container parse/mux (CRC32, page extraction, lacing) |
| mp3 | src/mp3.cyr | MP3 header parse, frame scanning, ID3v2 skip, decode stub |
| tag | src/tag.cyr | ID3v2 and Vorbis Comment metadata tag reading |
| fft | src/fft.cyr | Mixed-radix FFT for MDCT |
| opus | src/opus.cyr | Opus CELT-mode encoder + encoder framework (mode/bandwidth select, RFC 6716 TOC byte, dispatch); SILK/hybrid = 2.5.x |
| aac | src/aac.cyr | AAC-LC encoder/decoder (ADTS) |
| resample | src/resample.cyr | Windowed sinc interpolation (Draft/Good/Best quality) |
| dither | src/dither.cyr | Dithering for sample depth reduction |
| simd | src/simd.cyr | SIMD-optimized inner loops |
| stream | src/stream.cyr | Streaming decoder interface (WAV/FLAC/AIFF) |
| serde | src/serde.cyr | JSON serialization — full type surface: format/pcm/error + AlacConfig/Mp3FrameInfo/OpusHead/AudioMetadata + mp3/resample enums + codec markers (`#derive(Serialize)` + bayan) |
| sankoch | dep (cyrius.cyml) | Compression (LZ4, DEFLATE, zlib, gzip) — `compress()`/`decompress()` |

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
2. Cleanliness check: `cyrius build src/main.cyr build/shravan`, verify all 727 assertions pass
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
2. Cleanliness check: `cyrius build src/main.cyr build/shravan`, all tests pass
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
