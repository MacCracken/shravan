# shravan v1.1.0 — Rust vs Cyrius Benchmark Comparison

Date: 2026-04-09
Host: x86_64 Linux 6.18.19-1-lts
Rust: 1.89 (release, --all-features, criterion 0.8.2)
Cyrius: 3.3.4 (cc3, 55KB static ELF, manual timing via clock_gettime)

## Scope

Phase 1 port only: error, format, pcm, codec, wav.
Rust benchmarks cover the full crate (FLAC, Opus, resample, SIMD also included).
Cyrius benchmarks cover the ported subset (WAV decode, PCM conversion).

## Runtime Benchmarks

### WAV decode — 1 second mono 44100 Hz sine, i16

| Implementation | Time/iter | Notes |
|----------------|-----------|-------|
| **Rust** | **22.8 µs** | criterion, 222K iterations, LLVM -O3 |
| **Cyrius** | **1,130 µs** | 10K iterations, no optimizer |
| Ratio | ~50x | |

### PCM i16→float conversion — 4096 samples

| Implementation | Time/iter | Notes |
|----------------|-----------|-------|
| **Rust** (i16→f32) | **429 ns** | criterion, 12M iterations, LLVM auto-vectorized |
| **Cyrius** (i16→f64) | **95,500 ns** | 100K iterations, scalar f64 |
| Ratio | ~223x | |

### Full Rust benchmark suite (for future comparison)

| Benchmark | Rust time |
|-----------|-----------|
| wav_decode_1sec_i16 | 22.8 µs |
| pcm_i16_to_f32_4096 | 429 ns |
| simd_i16_to_f32_4096 | 1.32 µs |
| resample_4096_44100_to_48000 | 3.10 ms |
| flac_encode_1sec_16bit | 1.50 ms |
| flac_decode_1sec_16bit | 837 µs |
| opus_encode_1sec_mono_64k | 28.2 ms |

## Compile Time

| Metric | Rust | Cyrius |
|--------|------|--------|
| Full rebuild | **5.1s** (17.9s CPU, 392% parallel) | **25ms** |
| Incremental | ~1-2s | 25ms (no incremental, always full) |
| Ratio | ~200x slower | |

## Binary / Artifact Size

| Metric | Rust | Cyrius |
|--------|------|--------|
| Library artifact | 1,831,596 bytes (rlib, all features) | 55,976 bytes (static ELF) |
| Dependencies | 20 crates (serde, thiserror, symphonia, tracing, libm) | 0 (11 stdlib includes, no external deps) |
| Ratio | ~33x larger | |

## Source Size (Phase 1 modules only)

| Metric | Rust | Cyrius |
|--------|------|--------|
| Total lines (with tests) | 1,238 | 1,160 |
| Code lines (no comments/blanks) | ~900 | ~831 |
| Test assertions | 35 (unit tests) | 72 |

## Source Size (full crate vs Phase 1 port)

| Metric | Rust (full) | Cyrius (Phase 1) |
|--------|-------------|-------------------|
| Total lines | 10,265 | 1,160 |
| Code lines (no comments/blanks) | 7,629 | 831 |
| Modules | 16 | 5 (error, format, pcm, codec, wav) |

## Analysis

### Why Cyrius is slower

1. **No optimizer** — Cyrius cc3 emits direct x86_64 machine code with no optimization passes (no constant propagation beyond folding, no loop unrolling, no vectorization, no register allocation beyond the calling convention).

2. **f64 vs f32** — Cyrius uses f64 natively (SSE2 double-precision). Rust uses f32 for audio samples. Double precision is inherently slower for bulk data and doubles memory bandwidth requirements.

3. **No auto-vectorization** — Rust+LLVM auto-vectorizes the PCM conversion loop (SIMD `cvtdq2ps` etc.). Cyrius processes one sample at a time.

4. **Heap allocation per decode** — Each `vec_push` in Cyrius may trigger a grow+copy. Rust pre-allocates with `Vec::with_capacity` and uses zero-copy iterators.

5. **Function call overhead** — `sign_extend()`, `f64_clamp_sample()`, `read_u16_le()` are called per-sample in Cyrius. Rust inlines these.

### Why Cyrius compiles 200x faster

1. Single-pass compiler — lex → parse → emit in one traversal, no IR, no optimization passes.
2. 233KB compiler binary — fits in L2 cache, no LLVM/linker overhead.
3. Zero dependencies — no crate graph resolution, no proc macros.

### Potential Cyrius improvements

- **Inline asm** for hot loops (SSE2 `cvtsi2sd`/`mulsd` directly)
- **Bump allocator arena** per decode (avoid repeated vec_grow)
- **Loop unrolling** by hand (4x unroll for PCM conversion)
- **f32 path** via manual f32 bit manipulation (skip f64 entirely for PCM)
- Compiler improvements: inline small functions, dead store elimination on hot paths

### Verdict

Cyrius trades runtime performance for compilation speed, binary size, and sovereignty (zero external dependencies). For an audio codec library, the ~50x decode overhead is meaningful but may be acceptable for:
- Offline processing (batch conversion)
- Kernel-native audio (where avoiding Rust/LLVM toolchain matters)
- Embedded targets with no LLVM support

For real-time audio playback at 44.1 kHz, the 1.1ms WAV decode for 1 second of audio is well within budget (22ms per frame at 44.1 kHz / 1024 samples). The critical path would be FLAC/Opus decode, benchmarked when those are ported.
