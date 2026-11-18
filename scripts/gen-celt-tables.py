#!/usr/bin/env python3
"""Generate src/celt_enc_tables.cyr from libopus's static float mode.

    scripts/gen-celt-tables.py <opus-source-dir>

The CELT encoder (src/celt_enc.cyr) reproduces libopus's float build, which
reads the 48 kHz / 960-sample mode from celt/static_modes_float.h: the
120-sample window, the MDCT's twiddles, the FFT's twiddles, bit-reversal
tables and scales. Those literals are not the values their formulas give in
single precision (195 of the 2,880 floats differ by an ulp from
(float)cos(...) and friends), so they are copied, not computed: each float as
its bit pattern, two to a 64-bit word. The encoder's other tables come from
libopus's sources too, parsed rather than retyped: transient_analysis's
inv_table, the intensity thresholds and hysteresis, remove_doubling's
second_check, quant_band's bit (de)interleave tables, eMeans, the energy
predictor's coefficients, the comb filter's gains, the FFT states' factors and
the pre-emphasis coefficient. Run against libopus v1.6.1.
"""
import os
import re
import struct
import sys
from fractions import Fraction

ROOT = sys.argv[1]
src = open(os.path.join(ROOT, "celt", "static_modes_float.h")).read()


def source(rel):
    return open(os.path.join(ROOT, "celt", rel)).read()


def ints_of(text, name):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    m = re.search(r"%s\[[^\]]*\](?:\[[^\]]*\])?\s*=\s*\{(.*?)\};" % re.escape(name), text, re.S)
    if not m:
        sys.exit("gen-celt-tables: no table " + name)
    return [int(x) for x in re.findall(r"-?\d+", re.sub(r"/\*.*?\*/", "", m.group(1), flags=re.S))]


def block(name):
    m = re.search(r"%s\[\d+\] = \{(.*?)\};" % re.escape(name), src, re.S)
    if not m:
        sys.exit("gen-celt-tables: no table " + name)
    return m.group(1)


def f32(lit):
    """The float nearest the decimal literal, as C reads it (not through a
    double, whose second rounding can land one ulp off)."""
    sign = 0x80000000 if lit.startswith("-") else 0     # -0.0f keeps its sign
    mag = lit.lstrip("+-")
    exact = Fraction(mag)
    b = struct.unpack("<I", struct.pack("<f", float(mag)))[0]
    best = None
    for c in (b - 1, b, b + 1):
        if c < 0:
            continue
        v = Fraction(struct.unpack("<f", struct.pack("<I", c))[0])
        key = (abs(v - exact), c & 1)
        if best is None or key < best[0]:
            best = (key, c)
    return best[1] | sign


def floats(txt):
    return [f32(x.rstrip("f")) for x in re.findall(r"[-+]?\d*\.\d+(?:e[-+]?\d+)?f", txt)]


def bits(v):
    return v


w = block("window120")
if "#else" in w:                       # the float (not QEXT) variant
    w = w.split("#else")[1].split("#endif")[0]
window = floats(w)
fft_tw = floats(block("fft_twiddles48000_960"))
trig = floats(block("mdct_twiddles960"))
assert len(window) == 120 and len(fft_tw) == 960 and len(trig) == 1800
bitrev = {}
for n in (480, 240, 120, 60):
    bitrev[n] = [int(x) for x in re.findall(r"-?\d+", block("fft_bitrev%d" % n))]
    assert len(bitrev[n]) == n
scales = []
for i in range(4):
    m = re.search(r"fft_state48000_960_%d = \{\s*(\d+),\s*/\* nfft \*/\s*([-\d.e]+)f," % i, src)
    if not m:
        sys.exit("gen-celt-tables: no fft state %d" % i)
    scales.append(f32(m.group(2)))

enc = source("celt_encoder.c")
inv_table = ints_of(enc, "inv_table")
int_thr = ints_of(enc, "intensity_thresholds")
int_hyst = ints_of(enc, "intensity_histeresis")
second_check = ints_of(source("pitch.c"), "second_check")
bands = source("bands.c")
bit_il = ints_of(bands, "bit_interleave_table")
bit_deil = [int(x, 16) for x in re.findall(r"0x[0-9A-Fa-f]+", re.search(
    r"bit_deinterleave_table\[16\]=\{(.*?)\};", bands, re.S).group(1))]
qb = source("quant_bands.c")
em = re.search(r"#else\s*/\* Mean energy in each band quantized in Q4 and converted back to float \*/\s*"
               r"const opus_val16 eMeans\[25\] = \{(.*?)\};", qb, re.S)
if not em:
    em = re.search(r"Mean energy in each band quantized in Q4 and converted back to float \*/\s*"
                   r"const opus_val16 eMeans\[25\] = \{(.*?)\};", qb, re.S)
emeans = floats(em.group(1))
def kratios(name):
    """The float build's k/32768. initialisers (the fixed-point build's come first)."""
    m = re.search(r"%s\[4\] = \{([^}]*/32768\.[^}]*)\};" % name, qb)
    return [int(a) for a in re.findall(r"(\d+)/32768\.", m.group(1))]
pred_num = kratios("pred_coef")
beta_num = kratios("beta_coef")
beta_intra_num = int(re.search(r"beta_intra = (\d+)/32768\.", qb).group(1))
celt_c = source("celt.c")
gm = re.search(r"gains\[3\]\[3\] = \{(.*?)\};", celt_c, re.S)
comb = [f32(x) for x in re.findall(r"QCONST16\(([-\d.]+)f, 15\)", gm.group(1))]
factors = []
for i in range(4):
    m = re.search(r"fft_state48000_960_%d = \{.*?\{([\d, ]+)\},\s*/\* factors \*/" % i, src, re.S)
    factors.append([int(x) for x in m.group(1).replace(" ", "").strip(",").split(",")][:10])
pm = re.search(r"mode48000_960_120 = \{.*?\{([-\d.ef, ]+)\},\s*/\* preemph \*/", src, re.S)
preemph0 = f32(pm.group(1).split(",")[0].strip().rstrip("f"))
assert len(inv_table) == 128 and len(int_thr) == 21 and len(int_hyst) == 21 and len(second_check) == 16
assert len(bit_il) == 16 and len(bit_deil) == 16 and len(emeans) == 25 and len(comb) == 9
assert len(pred_num) == 4 and len(beta_num) == 4

out = []
out.append("# celt_enc_tables.cyr -- generated by scripts/gen-celt-tables.py from libopus")
out.append("# v1.6.1's celt/static_modes_float.h (the 48 kHz / 960-sample mode). Do not edit.")
out.append("# Single-precision floats as bit patterns, two to a word (the lower first).")
out.append("")
out.append("var _cet_window = 0;     # window120: 120 floats")
out.append("var _cet_fft_tw = 0;     # fft_twiddles48000_960: 480 complex (r, i)")
out.append("var _cet_trig = 0;       # mdct_twiddles960: 1800 floats (N = 1920, 960, 480, 240)")
out.append("var _cet_bitrev = 0;     # fft_bitrev480/240/120/60, 8-byte words, at 0, 480, 720, 840")
out.append("var _cet_fft_scale = 0;  # the four FFT states' scale (floats)")
out.append("var _cet_fft_factors = 0; # the four FFT states' factors, 10 words each")
out.append("var _cet_inv_table = 0;  # transient_analysis's inv_table[128] (words)")
out.append("var _cet_int_thr = 0;    # intensity_thresholds[21]")
out.append("var _cet_int_hyst = 0;   # intensity_histeresis[21]")
out.append("var _cet_second_check = 0; # remove_doubling's second_check[16]")
out.append("var _cet_bit_il = 0;     # bit_interleave_table[16]")
out.append("var _cet_bit_deil = 0;   # bit_deinterleave_table[16]")
out.append("var _cet_emeans = 0;     # eMeans[25] (floats)")
out.append("var _cet_pred_coef = 0;  # pred_coef[4]: k/32768 as floats")
out.append("var _cet_beta_coef = 0;  # beta_coef[4]")
out.append("var _cet_beta_intra = 0; # beta_intra (a float pattern)")
out.append("var _cet_comb_gains = 0; # comb_filter's gains[3][3] (floats)")
out.append("var _cet_preemph0 = 0;   # the static mode's preemph[0] (a float pattern)")
out.append("")


def packed(name, vals):
    lines = ["    %s = alloc(%d);" % (name, len(vals) * 4 + 8)]
    row = []
    for i in range(0, len(vals), 2):
        lo = bits(vals[i])
        hi = bits(vals[i + 1]) if i + 1 < len(vals) else 0
        row.append("store64(%s + %d, 0x%08X%08X);" % (name, i * 4, hi, lo))
        if len(row) == 6:
            lines.append("    " + " ".join(row))
            row = []
    if row:
        lines.append("    " + " ".join(row))
    return lines


out.append("fn _cet_init() {")
out += packed("_cet_window", window)
out += packed("_cet_fft_tw", fft_tw)
out += packed("_cet_trig", trig)
out += packed("_cet_fft_scale", scales)
out += packed("_cet_emeans", emeans)
out += packed("_cet_comb_gains", comb)


def ratio_bits(k):
    return struct.unpack("<I", struct.pack("<f", k / 32768.0))[0]   # exact: k < 2^15


out += packed("_cet_pred_coef", [ratio_bits(k) for k in pred_num])
out += packed("_cet_beta_coef", [ratio_bits(k) for k in beta_num])
out.append("    _cet_beta_intra = 0x%08X;" % ratio_bits(beta_intra_num))
out.append("    _cet_preemph0 = 0x%08X;" % preemph0)


def words(name, vals):
    lines = ["    %s = alloc(%d);" % (name, len(vals) * 8)]
    row = []
    for i, v in enumerate(vals):
        row.append("store64(%s + %d, %d);" % (name, i * 8, v))
        if len(row) == 8:
            lines.append("    " + " ".join(row))
            row = []
    if row:
        lines.append("    " + " ".join(row))
    return lines


out += words("_cet_fft_factors", [v for f in factors for v in (f + [0] * 10)[:10]])
out += words("_cet_inv_table", inv_table)
out += words("_cet_int_thr", int_thr)
out += words("_cet_int_hyst", int_hyst)
out += words("_cet_second_check", second_check)
out += words("_cet_bit_il", bit_il)
out += words("_cet_bit_deil", bit_deil)
allrev = bitrev[480] + bitrev[240] + bitrev[120] + bitrev[60]
out.append("    _cet_bitrev = alloc(%d);" % (len(allrev) * 8))
row = []
for i, v in enumerate(allrev):
    row.append("store64(_cet_bitrev + %d, %d);" % (i * 8, v))
    if len(row) == 8:
        out.append("    " + " ".join(row))
        row = []
if row:
    out.append("    " + " ".join(row))
out.append("    return 0;")
out.append("}")
dest = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "celt_enc_tables.cyr")
open(dest, "w").write("\n".join(out) + "\n")
print("wrote %s (window %d, fft twiddles %d, MDCT twiddles %d, bit reversals %d)" % (
    os.path.relpath(dest), len(window), len(fft_tw) // 2, len(trig), len(allrev)))
