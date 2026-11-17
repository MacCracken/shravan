# `_fft_pow2` clashes with abaco 2.8.0's and silently changes abaco's FFT at `n = 1`

**Status:** ✅ **RESOLVED** in 2.9.4 (2026-11-16). See Resolution at the end.
**Placement:** unpinned.
**Discovered:** 2026-11-08, moving dhvani to the newest dependency tags for its 2.3.1.
**Severity:** Low today, because dhvani is held at abaco 2.7.0 for another reason. Once that is lifted it is a silent change to a public abaco function on valid input.
**Affects:** shravan 2.9.3 (tag `2.9.3`, `cd577bb`, also `main`) with abaco 2.8.0 (`1ace4d7`), built with cyrius 6.7.18.

## Summary

abaco 2.8.0 adds `_fft_pow2(n)`, and shravan already has one. Bundles share one flat namespace. Both take one argument, so cycc does not refuse the pair. It warns, and the last definition wins for every caller in both bundles:

```
warning:lib/shravan.cyr:9943:1: duplicate fn '_fft_pow2' (last definition wins; first defined in abaco-2.8.0.cyr)
```

The two differ at `n = 1`:

| | `n < 1` | `n = 1` | other powers of two |
|---|---|---|---|
| abaco `_fft_pow2` | 0 | **1** | 1 |
| shravan `_fft_pow2` | 0 | **0** | 1 |

abaco treats 1 as a power of two: its `fft` and `fft_twiddles` accept it, and its tests cover it ("fft and ifft match the O(n^2) DFT, n = 1..256"). With abaco included first, shravan's copy replaces it and those calls are refused.

## Decision

abaco is the ecosystem's math library and takes priority (maintainer, 2026-11-08). abaco keeps the name, and shravan's is the one to rename.

## Where

`src/fft.cyr:305` is the definition, with three callers at `src/fft.cyr:177`, `:485` and `:694`. It is a private helper, so the rename changes no API.

## Measured

| Include order | Winning copy | abaco `fft_twiddles(t, 1)` and `fft(.., 1, t, 1)` | shravan `fft_compute(re, im, 1, 0)` |
|---|---|---|---|
| abaco alone | abaco's | Ok | not compiled |
| **abaco, then shravan** | **shravan's** | **Err** | not called |
| shravan alone | shravan's | not compiled | rc 0, value unchanged |
| shravan, then abaco | abaco's | not called | rc 0, value unchanged |

Only the order with abaco first does harm, and it is the order dhvani uses. shravan's own result is right in both orders: with abaco's copy, `n = 1` takes `_fft2` rather than the mixed-radix path and gives the same answer. The MDCT callers pass `n / 4` with `n % 8 == 0`, so they never ask about 1.

## Repro

From a dhvani 2.3.1 checkout, which has shravan in `lib/`, fetch abaco 2.8.0 beside it:

```sh
curl -sf https://raw.githubusercontent.com/MacCracken/abaco/2.8.0/dist/abaco.cyr -o abaco-2.8.0.cyr
```

```cyrius
include "abaco-2.8.0.cyr"
include "lib/sankoch.cyr"
include "lib/shravan.cyr"

fn run() {
    alloc_init();
    var t = alloc(64);
    var re = alloc(8);
    var im = alloc(8);
    store64(re, f64_from(7));
    store64(im, 0);
    var st, v = fft_twiddles(t, 1);
    println_int(st);                     # 1 (Err) here; 0 (Ok) with abaco alone
    var s2, v2 = fft(re, im, 1, t, 1);
    println_int(s2);                     # 1 (Err) here; 0 (Ok) with abaco alone
    return 0;
}
run();
syscall(60, 0);
```

```sh
cyrius build --no-deps repro.cyr repro && ./repro
```

It prints 1 and 1. Delete the two `lib/` includes and it prints 0 and 0.

## Fix direction

Rename shravan's helper, for example to `_shr_fft_pow2`, at the definition and its three callers. Keep its `n < 2` behaviour, since `fft_compute` relies on `n = 1` taking the other path.

## Regression

Add abaco's newest `dist/abaco.cyr` to whatever symbol-collision check shravan runs, so a same-arity clash fails there instead of surfacing as a warning in a consumer.

## Resolution (2.9.4)

- **Renamed** to `_shr_fft_pow2` at the definition (`src/fft.cyr`) and its three callers, `n < 2`
  behaviour kept. abaco's `_fft_pow2` is now the only one in a unit holding both bundles.
- **Measured** with the repro above against the 2.9.4 bundle: it prints 0 and 0, as abaco alone
  does, and no `_fft_pow2` warning remains. In both include orders, abaco's `fft_twiddles(t, 1)` and
  `fft(.., 1, t, 1)` return Ok, and shravan's `fft_compute(re, im, 1, 0)` returns 0 with the value
  unchanged.
- **Test**: `test_fft_size_1` pins the helper's contract (`_shr_fft_pow2(1) == 0`) and the size-1
  transform both ways. Giving the helper abaco's semantics fails exactly that assertion, while the
  size-1 values still agree, so both paths return the same result.
- **Regression**: shravan had no symbol-collision check, so 2.9.4 adds `scripts/check-collisions.sh`
  and a CI step. It compiles `dist/shravan.cyr` last, after its own deps, after every module of the
  pinned stdlib snapshot, and after abaco at its newest release tag. It fails on any duplicate or
  redeclaration located in shravan's bundle, except the `detect_format` wrapper (3.0 backlog). On
  the 2.9.3 bundle it fails twice: on this issue, and on a second clash it found. The stdlib's
  mabda 4.2.0 declares `const F64_255`, and shravan's `var F64_255` (never read) is a hard compile
  error beside it, so 2.9.4 deletes it, as phylax did at 1.3.0.

