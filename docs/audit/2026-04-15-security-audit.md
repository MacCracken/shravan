# Security Audit — 2026-04-15

**Scope**: Full codebase (src/main.cyr + lib/*.cyr), stdlib dependencies, upstream compiler
**Focus**: CVE-class vulnerabilities exploitable via crafted audio files
**Reference CVEs**: libFLAC CVE-2007-4619, dr_flac CVE-2025-14369, stagefright, ffmpeg AAC stack overflow, Opus CVE-2013-0899

## Summary

| Severity | Found | Fixed | Deferred |
|----------|-------|-------|----------|
| **P0 Critical** | 7 | 7 | 0 |
| **P1 High** | 5 | 0 | 5 |
| **P2 Medium** | 6 | 0 | 6 |
| **P3 Low** | 3 | 0 | 3 |
| **Upstream** | 3 | 0 | 3 |

---

## P0 — Critical (Fixed)

### SEC-001: WAV chunk size integer overflow
**File**: `src/main.cyr:631-632`
**Vector**: Crafted WAV with `ck_size` near `INT64_MAX`. `pos = pos + padded + 8` wraps, skipping validation and enabling OOB access in subsequent chunk parsing.
**Impact**: OOB read/write, potential RCE
**Fix**: Added overflow guard `if (ck_size > len - pos - 8) { pos = len + 1; }` before pos advancement.

### SEC-002: AIFF SSND offset integer underflow
**File**: `src/main.cyr:1048`
**Vector**: AIFF with SSND chunk where `ssnd_offset > ck_size - 8`. `ssnd_len = ck_size - 8 - ssnd_offset` underflows to massive positive value. Subsequent decode reads past buffer end.
**Impact**: Heap overread (info leak), DoS (massive allocation), potential RCE
**CVE analog**: Stagefright-class — metadata-controlled buffer size
**Fix**: Added `if (ssnd_offset <= ck_size - 8)` guard and `pcm_start <= len` validation.

### SEC-003: AIFF chunk pos overflow
**File**: `src/main.cyr:1054`
**Vector**: Same pattern as SEC-001 but in AIFF chunk scanner.
**Fix**: Added overflow guard matching WAV fix.

### SEC-004: WAV extensible format OOB read
**File**: `src/main.cyr:615-617`
**Vector**: WAV with `WAVE_FORMAT_EXTENSIBLE` and `ck_size >= 40` but file truncated before `pos + 34`. `read_u16_le(data, pos + 32)` reads past buffer.
**Impact**: 2-byte heap overread
**Fix**: Added `if (pos + 34 <= len)` guard around extensible format parsing.

### SEC-005: FLAC unbounded block_size allocation
**File**: `lib/flac.cyr:449,460,473,518`
**Vector**: FLAC frame with block_size code 7 + 16-bit value. `alloc(block_size * 8)` with block_size up to 65536 = 512KB per subframe. Chained with multiple channels/frames = multi-GB allocation.
**Impact**: DoS (memory exhaustion), heap corruption if allocation wraps
**CVE analog**: dr_flac CVE-2025-14369 (integer overflow in frame count)
**Fix**: Added `if (v + 1 > 65535) { return err(ERR_DECODE); }` in `flac_decode_block_size()`.

### SEC-006: FLAC metadata block OOB
**File**: `lib/flac.cyr:262`
**Vector**: FLAC with metadata block_size (24-bit field, max 16MB) pointing past file end. `pos = pos + block_size` overshoots, causing subsequent reads from invalid memory.
**Impact**: OOB read, crash
**Fix**: Added `if (pos + block_size > len) { return err(ERR_INVALID_HEADER); }`.

### SEC-007: Ogg packet accumulation overflow
**File**: `lib/ogg.cyr:253-264`
**Vector**: Ogg stream with many continuation pages accumulating `cur_len` beyond reasonable bounds. Integer overflow in `cur_len + lacing_value`.
**Impact**: Heap overflow via oversized memcpy
**Fix**: Added overflow check (`cur_len + lacing_value < cur_len`) and hard cap (16MB).

---

## P1 — High (Deferred to v2.3.0)

### SEC-008: FLAC unary decode excessive iteration
**File**: `lib/flac.cyr:103-113`
**Vector**: Malformed FLAC bitstream with long runs of 0-bits. Unary decoder iterates up to 1M times before erroring.
**Impact**: DoS (CPU exhaustion per frame)
**Recommendation**: Reduce bound to `65535 * bps` (realistic maximum for Rice coding).

### SEC-009: ALAC arithmetic right shift INT64_MIN
**File**: `src/main.cyr:1409-1410`
**Vector**: ALAC prediction value of `INT64_MIN`. `0 - prediction` overflows (INT64_MIN negation = INT64_MIN).
**Impact**: Corrupted prediction, audio artifacts, potential downstream overflow
**Recommendation**: Guard with `if (prediction == 0 - (1 << 63)) { pred_shifted = 0; }`.

### SEC-010: MP3 frame_size scanner OOB
**File**: `lib/mp3.cyr:286`
**Vector**: MP3 with crafted frame header claiming frame_size near INT64_MAX. `pos = pos + fs` overflows.
**Impact**: OOB read in next iteration
**Recommendation**: Add `if (pos + fs > len) { break; }`.

### SEC-011: AAC frame_length accumulation
**File**: `lib/aac.cyr:116-120`
**Vector**: Stream with many ADTS frames at max length (8191 bytes). No cap on total accumulated allocation.
**Impact**: DoS (memory exhaustion for long streams)
**Recommendation**: Add per-decode total allocation budget.

### SEC-012: FLAC SEEKTABLE iteration DoS
**File**: `lib/flac.cyr:244-259`
**Vector**: SEEKTABLE with `block_size` = 16MB (max 24-bit). `num_points = block_size / 18` = 932,067 iterations. Each allocates 24 bytes = ~22MB of seek points.
**Impact**: DoS (CPU + memory)
**Recommendation**: Cap `num_points` at 1024 (reasonable for any real file).

---

## P2 — Medium (Deferred to v2.3.0)

### SEC-013: Vorbis Comment zero-length loop
**File**: `lib/tag.cyr:263-313`
**Vector**: Vorbis Comment block with `comment_len = 0` for all entries. `pos` doesn't advance.
**Impact**: Infinite loop (DoS)
**Recommendation**: Skip zero-length comments (`if (comment_len == 0) { ci = ci + 1; continue; }`).

### SEC-014: MDCT size validation
**File**: `lib/fft.cyr:291`
**Vector**: External caller passes non-power-of-2 or extremely large `n` to `fft_mdct`. Falls through to O(n^2) naive DFT or massive allocation.
**Impact**: DoS (CPU/memory)
**Recommendation**: Validate `n > 0 && n <= 8192 && n % 4 == 0`.

### SEC-015: Division truncation in frame count
**File**: `src/main.cyr:687,1070`
**Vector**: File with `sample_count % channels != 0`. Silent data loss.
**Impact**: Data integrity (samples silently dropped)
**Recommendation**: Error on non-exact division, or document as intended.

### SEC-016: Bitreader negative bits request
**File**: `src/main.cyr:1256`
**Vector**: Caller passes negative `bits` to `bitreader_read`. Loop runs `remaining > 0` which is true for negative values treated as large positive.
**Impact**: OOB read, hang
**Recommendation**: Add `if (bits < 0 || bits > 64) { return err(ERR_DECODE); }`.

### SEC-017: Allocation overflow helper missing
**Pattern**: Multiple locations use `alloc(a * b)` where both operands could be untrusted.
**Impact**: Integer overflow → tiny allocation → heap overflow
**Recommendation**: Add `_safe_alloc_mul(a, b)` helper with overflow check.

### SEC-018: Tag ID3v2 frame underflow
**File**: `lib/tag.cyr:201`
**Vector**: ID3v2 COMM frame with `frame_size < 4`. Subtraction underflows.
**Impact**: OOB read
**Recommendation**: Validate `frame_size >= 4` before subtraction.

---

## P3 — Low (Deferred)

### SEC-019: Extended float short buffer
**File**: `src/main.cyr:857-867`
**Vector**: `extended_to_f64()` reads 10 bytes without validating buffer length.
**Impact**: OOB read (2 bytes)
**Recommendation**: Document precondition or add length parameter.

### SEC-020: Integer division truncation (non-safety)
**File**: Various
**Impact**: Cosmetic — duration/frame count off by 1
**Recommendation**: Document or use ceiling division.

### SEC-021: CRC-32 collision (theoretical)
**File**: `lib/ogg.cyr:159-164`
**Impact**: Integrity — CRC-32 is not cryptographic. Crafted collision allows corrupted page to pass.
**Recommendation**: Acceptable for audio codec; document that CRC is not a security boundary.

---

## Upstream Issues (Cyrius Compiler)

### UPSTREAM-001: alloc() heap pointer overflow
**File**: `lib/alloc.cyr` (stdlib)
**Issue**: `_heap_ptr = _heap_ptr + size` has no overflow check. If `_heap_ptr + size > INT64_MAX`, wraps to negative, corrupting allocator state.
**Impact**: Heap corruption in any downstream project
**Recommendation**: Add `if (_heap_ptr + size < _heap_ptr) { return 0; }` — filed for Cyrius 5.0.1.

### UPSTREAM-002: vec capacity doubling overflow
**File**: `lib/vec.cyr:60` (stdlib)
**Issue**: `var new_cap = cap * 2` has no overflow check. At cap = 2^62, `new_cap * 8` overflows.
**Impact**: Tiny allocation for huge vec → heap overflow on next push
**Recommendation**: Add `if (cap > MAX_VEC_CAP) { abort; }` — filed for Cyrius 5.0.1.

### UPSTREAM-003: No allocation size cap
**File**: `lib/alloc.cyr` (stdlib)
**Issue**: No maximum allocation size. `alloc(0x7FFFFFFFFFFFFFFF)` attempts 8 EB allocation.
**Impact**: DoS via OOM in any codec processing untrusted input
**Recommendation**: Add configurable max (default 256MB) — filed for Cyrius 5.0.1.

---

## Methodology

- Manual code review of all 14,193 lines
- Pattern matching against known audio codec CVE classes
- Upstream stdlib review (alloc, vec, string, fmt)
- Focus areas: buffer boundary arithmetic, allocation size calculations, loop termination, integer overflow in size math
- All fixes verified: 520 tests passing, 0 failures
