# shravan -- Claude Code Instructions

## Project Identity

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for AGNOS

- **Language**: Cyrius
- **License**: GPL-3.0-only
- **Genesis repo**: [agnosticos](https://github.com/MacCracken/agnosticos)
- **Philosophy**: [AGNOS Philosophy & Intention](https://github.com/MacCracken/agnosticos/blob/main/docs/philosophy.md)
- **Standards**: [First-Party Standards](https://github.com/MacCracken/agnosticos/blob/main/docs/development/applications/first-party-standards.md)
- **Recipes**: [zugot](https://github.com/MacCracken/zugot) -- takumi build recipes
- **Ported from**: Rust (10,265 lines -> 9,958 lines Cyrius). Rust source archived in `rust-old/`, preserved in git history. See `benchmarks-rust-v-cyrius.md`.

## Architecture

Single-file library with codec modules in `lib/`. `src/main.cyr` is the entry point containing error, format, PCM, WAV, AIFF, ALAC, and codec dispatch inline, with codec modules (FLAC, Ogg, MP3, Opus, AAC, etc.) included from `lib/`. Stdlib vendored in `lib/`. Consumers include the entry point and get all codecs.

```
src/main.cyr     -- library + test harness (entry point)
src/bench.cyr    -- benchmarks (clock_gettime timing)
lib/             -- codec modules + vendored Cyrius stdlib
build/           -- compiled binaries (gitignored)
rust-old/        -- archived Rust implementation (reference only)
scripts/         -- bench-history.sh, version-bump.sh
docs/            -- architecture, roadmap
```

## Build

```sh
cyrius build src/main.cyr build/shravan    # compile
./build/shravan                             # run tests (119 assertions)
cyrius build src/bench.cyr build/bench     # compile benchmarks
./build/bench                               # run benchmarks
```

## Modules

| Module | Location | Description |
|--------|----------|-------------|
| error | src/main.cyr | ShravanErr enum, packed Result helpers |
| format | src/main.cyr | AudioFormat enum, FormatInfo struct, format detection |
| pcm | src/main.cyr | Sample format conversion (u8/i16/i24/i32/f32/f64), interleave/deinterleave |
| wav | src/main.cyr | RIFF WAVE encode/decode (PCM 8/16/24/32-bit, IEEE float 32-bit) |
| aiff | src/main.cyr | AIFF encode/decode (big-endian, 80-bit extended sample rate) |
| alac | src/main.cyr | Apple Lossless Audio Codec decoder |
| codec | src/main.cyr | Auto-detect format and dispatch to decoder |
| flac | lib/flac.cyr | FLAC encode/decode (all subframe types, Rice coding, channel decorrelation) |
| ogg | lib/ogg.cyr | Ogg container parse/mux (CRC32, page extraction, lacing) |
| mp3 | lib/mp3.cyr | MP3 header parse, frame scanning, ID3v2 skip, decode stub |
| tag | lib/tag.cyr | ID3v2 and Vorbis Comment metadata tag reading |
| fft | lib/fft.cyr | Mixed-radix FFT for MDCT |
| opus | lib/opus.cyr | Opus CELT-mode encoder (FFT-based MDCT) |
| aac | lib/aac.cyr | AAC-LC encoder/decoder (ADTS) |
| resample | lib/resample.cyr | Windowed sinc interpolation (Draft/Good/Best quality) |
| dither | lib/dither.cyr | Dithering for sample depth reduction |
| simd | lib/simd.cyr | SIMD-optimized inner loops |
| stream | lib/stream.cyr | Streaming decoder interface (WAV/FLAC/AIFF) |

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
2. Cleanliness check: `cyrius build src/main.cyr build/shravan`, verify all 119 assertions pass
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
11. Version check -- VERSION, cyrius.toml in sync
12. Return to step 1

### Key Principles

- **Never skip benchmarks.** Numbers don't lie. The CSV history is the proof.
- **Tests + benchmarks are the way.**
- **All samples are f64 internally** -- Cyrius native, higher precision than f32.
- **Zero panics** -- return error codes, never abort.
- **Caller-provided buffers** where possible -- avoid heap allocation in hot paths.
- **All tests must pass** before any changes are considered complete.

## Conventions

- Error encoding: packed Result -- ok >= 0, err < 0 (bit 63 set), error code via `err_code(r)`
- Error codes: integer enum `ShravanErr`, messages via `err_print(code)` helper
- f64 constants: initialized at startup via `shravan_init_constants()`, never hardcode bit patterns
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
- Do not modify anything in `rust-old/` -- it is archived reference only
- Do not break backward compatibility without a major version bump
- Do not use reserved keywords as variable names (`match`, `default`, `shared`, `in`)

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
