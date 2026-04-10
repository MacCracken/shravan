# shravan -- Claude Code Instructions

## Project Identity

**shravan** (Sanskrit: hearing / perception) -- Audio codecs for AGNOS

- **Language**: Cyrius (ported from Rust)
- **Type**: Library (include-based)
- **License**: GPL-3.0
- **Version**: SemVer 1.1.0
- **Compiler**: Cyrius 3.3.4+ (cc3)

## Layout

```
src/main.cyr         — library + test harness (entry point)
src/bench.cyr        — benchmarks (clock_gettime timing)
lib/                 — Cyrius stdlib modules (alloc, vec, str, io, etc.)
build/               — compiled binaries
rust-old/            — archived Rust implementation (reference only)
cyrius.toml          — build configuration
benchmarks-rust-v-cyrius.md — performance comparison baseline
```

## Build & Test

```sh
cyrius build src/main.cyr build/shravan    # build + test harness
./build/shravan                             # run tests (72 assertions)
cyrius build src/bench.cyr build/bench     # build benchmarks
./build/bench                               # run benchmarks
```

## Consumers

tarang (media framework), jalwa (media player), dhvani (audio engine), shruti (DAW), and any AGNOS component needing audio codec support.

## Development Process

### P(-1): Scaffold Hardening (before any new features)

0. Read roadmap, CHANGELOG, and open issues
1. Test + benchmark sweep of existing code
2. Cleanliness check: `cyrius build`, `cyrius check`, verify all tests pass
3. Get baseline benchmarks
4. Internal deep review
5. External research -- audio codec specs (WAV, FLAC, AIFF, Ogg, MP3, Opus), PCM standards
6. Cleanliness check -- must be clean after review
7. Additional tests/benchmarks from findings
8. Post-review benchmarks
9. Repeat if heavy

### Work Loop (continuous)

1. Work phase
2. Cleanliness check
3. Test + benchmark additions
4. Run benchmarks
5. Internal review
6. Cleanliness check
7. Deeper tests/benchmarks
8. Benchmarks again
9. If review heavy -> return to step 5
10. Documentation -- CHANGELOG, roadmap, docs
11. Version check
12. Return to step 1

### Key Principles

- Never skip benchmarks
- All samples are f64 internally (Cyrius native, higher precision than f32)
- Error handling via packed Result (negative = error, bit 63 set)
- Initialize f64 constants via f64_from() at startup (never hardcode bit patterns)
- `>>` is logical (unsigned) shift -- use conditional subtraction for sign extension
- Entry point is top-level statements, not fn main()
- Large buffers via alloc(), not stack arrays (code buffer limit)
- Feature-gate modules via #ifdef / -D flags
- Zero panics -- return error codes
- All tests must pass before any changes are considered complete

## Cyrius Idioms (Rust translation guide)

| Rust | Cyrius |
|------|--------|
| `Result<T, E>` | Packed result: ok >= 0, err < 0 (bit 63) |
| `&[u8]` | pointer + length args |
| `Vec<f32>` | vec of f64 bit patterns (i64 storage) |
| `enum { A(String) }` | integer enum + `err_print(code)` helper |
| `struct { field: T }` | heap alloc + `load64`/`store64` accessors |
| `trait Codec` | concrete functions, convention dispatch |
| `#[cfg(feature)]` | `#ifdef` / `cyrius build -D FLAG` |
| `impl Display` | `format_name(enum_val)` function |
| `f32` arithmetic | f64 builtins: `f64_add`, `f64_mul`, `f64_div`, etc. |

## DO NOT

- **Do not commit or push** -- the user handles all git operations
- **NEVER use `gh` CLI** -- use `curl` to GitHub API only
- Do not add unnecessary dependencies
- Do not break backward compatibility without a major version bump
- Do not skip benchmarks before claiming performance improvements
- Do not modify anything in rust-old/ -- it is archived reference only
