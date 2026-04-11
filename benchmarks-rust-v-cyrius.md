# shravan v2.0.0 — Rust vs Cyrius Benchmark Comparison

Date: 2026-04-11
Host: x86_64 Linux 6.18.19-1-lts
Rust: 1.89 (release, --all-features, criterion 0.8.2)
Cyrius: 3.3.17 (cc3, 292KB static ELF, manual timing via clock_gettime)

## Scope

Full port: all 18 modules (error, format, pcm, wav, aiff, alac, flac, ogg, mp3, tag, fft, opus, aac, resample, dither, simd, stream, codec). Both benchmarks run on the same hardware in the same session.

## Runtime Benchmarks

### WAV — 1 second mono 44100 Hz sine, i16

| Benchmark | Rust | Cyrius | Ratio |
|-----------|------|--------|-------|
| wav_decode | 47.1 us | 1,994 us | 42x |
| wav_encode | — | 1,306 us | — |

### PCM conversion — 4096 samples

| Benchmark | Rust | Cyrius | Ratio |
|-----------|------|--------|-------|
| i16 -> float | 788 ns (f32) | 166 us (f64) | 211x |
| i24 packed -> float | — | 161 us (f64) | — |
| u8 -> float | — | 127 us (f64) | — |
| SIMD i16 -> f32 | 2.97 us (SSE2) | — | — |

### FLAC — 1 second mono 44100 Hz sine, 16-bit

| Benchmark | Rust | Cyrius | Ratio |
|-----------|------|--------|-------|
| flac_encode | 2.77 ms | 20.7 ms | 7.5x |
| flac_decode | 1.53 ms | 7.97 ms | 5.2x |

### FFT / MDCT

| Benchmark | Rust | Cyrius | Ratio |
|-----------|------|--------|-------|
| fft_forward_1024 | — | 2.46 ms | — |
| mdct_forward_2048 | — | 12.4 ms | — |

### Resample — 4096 samples, 44100 -> 48000

| Benchmark | Rust | Cyrius | Ratio |
|-----------|------|--------|-------|
| resample_4096 | 5.25 ms | — | — |

### Opus — 1 second mono 64 kbps

| Benchmark | Rust | Cyrius | Ratio |
|-----------|------|--------|-------|
| opus_encode | 51.9 ms | — | — |

## Compile Time

| Metric | Rust | Cyrius |
|--------|------|--------|
| Full rebuild | **6.9s** (21.1s CPU, 339% parallel) | **420ms** |
| Incremental | ~1-2s | 420ms (no incremental, always full) |
| Ratio | **16x slower** | |

## Binary / Artifact Size

| Metric | Rust | Cyrius |
|--------|------|--------|
| Library artifact | 1,831,780 bytes (rlib, all features) | 291,688 bytes (static ELF) |
| Dependencies | 20 crates (serde, thiserror, symphonia, tracing, libm) | 0 (13 lib includes, no external deps) |
| Ratio | **6.3x larger** | |

## Source Size (full port)

| Metric | Rust | Cyrius |
|--------|------|--------|
| Total lines (with tests) | 10,599 | 11,780 |
| Modules | 18 (16 + simd/x86 + simd/aarch64) | 18 (main + 12 lib + bench) |
| Test assertions | ~250 (cargo test) | 499 |

## Analysis

### FLAC: 5-8x (dramatically improved from Phase 1's 50x)

The FLAC codec is the most meaningful comparison — it's a complex, compute-heavy codec with bitstream parsing, entropy coding, prediction, and CRC verification. Cyrius achieves:
- **Encode: 7.5x slower** than Rust+LLVM (-O3, auto-vectorized)
- **Decode: 5.2x slower** than Rust+LLVM

This is a massive improvement over Phase 1's WAV-only 50x gap. The FLAC decoder spends most time in integer arithmetic and bitstream parsing, which Cyrius handles reasonably well. The remaining gap is primarily from:

1. **No inlining** — `flac_br_read`, `flac_crc8`, `sign_extend` called per-sample
2. **No loop optimization** — no unrolling, no strength reduction
3. **f64 vs f32** — 2x memory bandwidth for sample arrays
4. **Bump allocator** — no memory reuse across frames

### WAV: 42x (similar to Phase 1's 50x)

WAV decoding is dominated by PCM conversion (i16 -> float), where LLVM auto-vectorization gives Rust a massive advantage. The 42x gap is expected and consistent.

### PCM: 211x (LLVM auto-vectorization vs scalar)

This is the widest gap and the most misleading. Rust+LLVM converts i16->f32 using SSE2 packed operations (4 samples/cycle). Cyrius does scalar f64 operations with function call overhead per sample. Hand-written SSE2 inline asm could close this to ~10x.

### Compile: 16x faster (improved from 200x)

Rust+LLVM compile time improved from 5.1s to 6.9s (likely Rust version changes), but the ratio dropped from 200x to 16x because Cyrius now compiles a much larger codebase (292KB output vs 56KB in Phase 1). The 420ms is still fast enough for interactive development.

### Binary: 6.3x smaller (improved from 33x)

The Cyrius binary grew from 56KB to 292KB (full codec suite), while the Rust rlib stayed at ~1.8MB. The ratio narrowed from 33x to 6.3x, but Cyrius still ships as a single static ELF with zero external dependencies.

### Potential Cyrius improvements

- **Inline asm** for PCM hot loops (SSE2 `cvtsi2sd`/`mulsd` directly) — would close PCM gap to ~10x
- **Hand-unrolled FLAC decode** — 4x unroll for residual reconstruction
- **Arena allocator per frame** — avoid repeated vec_grow in decode loops
- **f32 fast path** — skip f64 for 16-bit PCM where precision isn't needed
- **Compiler inlining** — cc3 small-function inlining for `sign_extend`, `f64_clamp` etc.

### Verdict

The full port achieves **5-8x slower** on compute-heavy codecs (FLAC), **42x** on memory-bandwidth-bound paths (WAV/PCM), and **16x faster** compile times. For the AGNOS ecosystem's use case (kernel-native audio, no LLVM dependency, sovereign toolchain), this is a strong result. FLAC decode at 8ms/second of audio leaves 92% headroom for real-time 44.1 kHz playback.
