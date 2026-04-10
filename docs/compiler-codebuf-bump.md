# Compiler Code Buffer Bump: 256KB → 512KB

## Problem

shravan's full binary (all 16 modules + serde + SIMD + streaming + tests) generates ~270KB of machine code, exceeding the Cyrius compiler's 262144-byte (256KB) code buffer. The compiler either errors with "output too large" or silently produces a broken binary when the overflow corrupts internal state.

## Required Changes in cyrius (3 files, 6 edits)

### 1. src/main.cyr — Heap map comment + output_buf offset

Update the heap map documentation. The codebuf grows from 262144 to 524288 (0x40000 → 0x80000). This shifts everything after codebuf down by 0x40000.

**Before:**
```
#   0x20000  codebuf       [262144]      256KB generated machine code
#   0xDA000  output_buf    [262144]      256KB ELF output
```

**After:**
```
#   0x20000  codebuf       [524288]      512KB generated machine code
#   0x11A000 output_buf    [524288]      512KB ELF output
```

Note: Every offset between `0x60000` and `0xDA000` shifts by +0x40000. The tok_names, struct tables, compiler state, fixup table, fn tables, and var tables all need their base addresses bumped. This is a significant change to the heap map.

**Simpler alternative:** Only bump the output_buf size check (the ELF output limit) and the codebuf overflow check, WITHOUT moving any heap regions. This works if the code buffer can overlap into unused padding between regions, or if the compiler is rebuilt with the new layout.

### 2. src/backend/x86/emit.cyr line 7 — Code buffer overflow check

**Before:**
```
    if (cp >= 262144) {
```

**After:**
```
    if (cp >= 524288) {
```

Also update the error message string on line 10:
```
    syscall(SYS_WRITE, 2, "/524288 bytes) — program too large for single compilation\n", 58);
```

### 3. src/backend/x86/fixup.cyr lines 150 and 397 — ELF output size checks

**Line 150 (ELF output):**
```
    if (filesz > 524288) {
```
Update error message on line 153.

**Line 397 (.o output):**
```
    if (total_size > 524288) {
```

## Why This Is Needed

shravan v2.0.0 has 16 audio codec modules totaling ~10K lines of Cyrius. With all features enabled (serde, SIMD batch ops, streaming decoders), the generated code exceeds 256KB. The project will only grow as codecs are optimized and new features added (DSD, SILK mode Opus, native AAC decode).

Other AGNOS projects approaching the limit: vidya (reference library), agnosys (20 kernel modules).

## Impact

- Self-hosting: cc3 must recompile itself with the new limits (two-step bootstrap)
- Memory: Total heap grows by ~512KB (codebuf + output_buf expansion)
- All programs < 256KB: No change in behavior
