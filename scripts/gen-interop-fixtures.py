#!/usr/bin/env python3
"""Generate src/interop_fixtures.cyr: real encoder output with reference decodes.

shravan's round-trip tests decode what shravan encoded, so they cannot see a bug the
encoder and decoder share, or a path shravan's encoder never takes. 2.9.2 found FLAC
LPC, ALAC, AAC at most sample rates and MP3 count1 table B all broken behind green
suites that way. These fixtures come from ffmpeg, flac and lame, and the expected
values come from ffmpeg's decoders, so src/interop_tests.cyr compares shravan against
an independent reference.

Each fixture asserts it exercises the path it exists for. If a tool changes and stops
producing that path, this script fails rather than writing a test that proves nothing.

Needs ffmpeg (with libmp3lame) and flac on PATH. The output is checked in, so running
the tests does not need either tool. Regenerate with:

    ./scripts/gen-interop-fixtures.py
"""
import array
import base64
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "src", "interop_fixtures.cyr")
FNV_OFFSET = 1469598103934665603
FNV_PRIME = 0x100000001B3
MASK64 = (1 << 64) - 1

# Bursts at different times in each channel: transients force short blocks, and
# different times force the channels onto independent windows (common_window = 0).
STEREO_TRANSIENT = (
    "aevalsrc=0.45*exp(-30*mod(t\\,0.11))*sin(2*PI*(%d/9)*t)"
    "+0.05*exp(-90*mod(t+0.04\\,0.09))*random(0)"
    "|0.4*exp(-25*mod(t+0.05\\,0.13))*sin(2*PI*(%d/7)*t):s=%d:d=%s"
)
TONE_NOISE = (
    "aevalsrc=0.4*sin(2*PI*440*t)+0.05*random(0)"
    "|0.3*sin(2*PI*660*t)+0.05*random(1):s=%d:d=%s"
)


def run(*args):
    return subprocess.run(args, check=True, capture_output=True)


def ffmpeg_make(out, lavfi, *codec_args):
    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", lavfi, *codec_args, out)


def reference_f64(path):
    """Decode with ffmpeg, keeping the file's own priming so frames line up."""
    keep = ("-ignore_editlist", "1") if path.endswith((".m4a", ".mov")) else ()
    p = run("ffmpeg", "-v", "error", *keep, "-i", path,
            "-f", "f64le", "-acodec", "pcm_f64le", "-")
    a = array.array("d")
    a.frombytes(p.stdout[: len(p.stdout) // 8 * 8])
    return a


def fnv_hash(ints):
    h = FNV_OFFSET
    for v in ints:
        h = ((h ^ (v & 0xFFFFFFFF)) * FNV_PRIME) & MASK64
    return h


def b64_literal(name, data):
    return 'var %s = "%s";\n' % (name, base64.b64encode(data).decode("ascii"))


def exact_fixture(name, path, bps):
    """Lossless: every sample must match, so a hash of all of them is enough."""
    ref = reference_f64(path)
    scale = 1 << (bps - 1)
    ints = [int(round(x * scale)) for x in ref]
    text = b64_literal("_FX_%s_B64" % name, open(path, "rb").read())
    text += "var _FX_%s_COUNT = %d;\n" % (name, len(ints))
    text += "var _FX_%s_HASH = %d;\n" % (name, fnv_hash(ints))
    return text


def lossy_fixture(name, path, step, skip_frames=0):
    """Lossy: compare every step-th sample as a 16-bit value, allowing for rounding.

    ffmpeg's float decoders and shravan's f64 ones agree to about 1e-6, far inside
    the tolerance. Random decode damage lands on nearly every sampled point.
    """
    ref = reference_f64(path)
    picks = [int(round(ref[i] * 32768)) for i in range(0, len(ref), step)]
    text = b64_literal("_FX_%s_B64" % name, open(path, "rb").read())
    text += "var _FX_%s_COUNT = %d;\n" % (name, len(ref))
    text += "var _FX_%s_STEP = %d;\n" % (name, step)
    text += 'var _FX_%s_REF = "%s";\n' % (name, ",".join(map(str, picks)))
    return text


def flac_analysis(path):
    return run("flac", "--analyze", "--stdout", "-s", path).stdout.decode()


def require(cond, what):
    if not cond:
        sys.exit("fixture does not exercise its path: " + what)


def mdat_payload(path):
    """First sample of a one-chunk MP4: the bytes after the mdat header."""
    data = open(path, "rb").read()
    pos = 0
    while pos + 8 <= len(data):
        size = int.from_bytes(data[pos:pos + 4], "big")
        kind = data[pos + 4:pos + 8]
        if kind == b"mdat":
            return data[pos + 8:]
        pos += size
    sys.exit("no mdat in " + path)


def alac_shift_bits(path):
    """bytes_shifted from the first ALAC frame header: tag(3) instance(4) unused(12)
    partial(1) shifted(2) escape(1)."""
    head = int.from_bytes(mdat_payload(path)[:4], "big")
    return (head >> (32 - 22)) & 3


def adts_first_frames(path, wanted):
    """Walk ADTS frames, reporting (frame count, how many start a CPE with
    common_window = 0, how many are EIGHT_SHORT)."""
    data = open(path, "rb").read()
    pos = frames = independent = 0
    while pos + 7 <= len(data):
        length = ((data[pos + 3] & 3) << 11) | (data[pos + 4] << 3) | (data[pos + 5] >> 5)
        bits = int.from_bytes(data[pos + 7:pos + 9], "big")
        element = bits >> 13
        common = (bits >> 8) & 1
        if element == 1 and common == 0:
            independent += 1
        frames += 1
        pos += length
    require(frames >= wanted, "too few ADTS frames")
    return frames, independent


def main():
    parts = [
        "# interop_fixtures.cyr -- real encoder output with reference decodes.\n",
        "# Generated by scripts/gen-interop-fixtures.py. Do not edit.\n\n",
    ]
    with tempfile.TemporaryDirectory() as tmp:
        p = lambda f: os.path.join(tmp, f)

        # FLAC: LPC subframes and stereo decorrelation, which shravan's Fixed-only
        # encoder never writes.
        ffmpeg_make(p("s16.wav"), TONE_NOISE % (44100, "0.12"), "-c:a", "pcm_s16le")
        run("flac", "-s", "-f", "--best", p("s16.wav"), "-o", p("flac16.flac"))
        text = flac_analysis(p("flac16.flac"))
        require("type=LPC" in text, "FLAC 16-bit LPC")
        require("MID_SIDE" in text or "LEFT_SIDE" in text or "RIGHT_SIDE" in text,
                "FLAC stereo decorrelation")
        parts.append(exact_fixture("FLAC16", p("flac16.flac"), 16))

        ffmpeg_make(p("s8.wav"), TONE_NOISE % (8000, "0.2"), "-ac", "1", "-c:a", "pcm_u8")
        run("flac", "-s", "-f", "--best", p("s8.wav"), "-o", p("flac8.flac"))
        require("type=LPC" in flac_analysis(p("flac8.flac")), "FLAC 8-bit LPC")
        parts.append(exact_fixture("FLAC8", p("flac8.flac"), 8))

        # ALAC in MP4: adaptive LPC and stereo unmixing at 16 bits, plus the shifted
        # low bytes at 24.
        ffmpeg_make(p("alac16.m4a"), TONE_NOISE % (44100, "0.06"),
                    "-c:a", "alac", "-sample_fmt", "s16p")
        parts.append(exact_fixture("ALAC16", p("alac16.m4a"), 16))
        ffmpeg_make(p("alac24.m4a"), TONE_NOISE % (48000, "0.05"),
                    "-c:a", "alac", "-sample_fmt", "s32p")
        require(alac_shift_bits(p("alac24.m4a")) > 0, "ALAC shifted low bytes")
        parts.append(exact_fixture("ALAC24", p("alac24.m4a"), 24))

        # ALAC in QuickTime at 96 kHz: a version 2 sample entry with the config
        # nested in a wave box, and a rate that does not fit the 16.16 field.
        ffmpeg_make(p("alac96.mov"), TONE_NOISE % (96000, "0.03"),
                    "-c:a", "alac", "-sample_fmt", "s16p")
        data = open(p("alac96.mov"), "rb").read()
        at = data.find(b"stsd") + 12
        require(int.from_bytes(data[at + 16:at + 18], "big") == 2, "QuickTime v2 entry")
        require(b"wave" in data[at:at + 200], "QuickTime wave box")
        parts.append(exact_fixture("ALACMOV", p("alac96.mov"), 16))

        # AAC at 22.05 kHz, a band layout other than 44.1/48, with TNS and with
        # channels on independent windows. PNS stays off: its noise is defined only
        # by energy, so it can never match a reference sample for sample.
        ffmpeg_make(p("aac22.aac"), STEREO_TRANSIENT % (22050, 22050, 22050, "0.6"),
                    "-c:a", "aac", "-b:a", "64k", "-aac_pns", "0", "-f", "adts")
        frames, independent = adts_first_frames(p("aac22.aac"), 8)
        require(independent > 0, "AAC common_window = 0")
        parts.append(lossy_fixture("AAC22", p("aac22.aac"), 53))

        # AAC in MP4 at 96 kHz: the rate only exists in the esds, and 96 kHz has its
        # own band layout.
        ffmpeg_make(p("aac96.m4a"), STEREO_TRANSIENT % (96000, 96000, 96000, "0.08"),
                    "-c:a", "aac", "-b:a", "128k", "-aac_pns", "0")
        parts.append(lossy_fixture("AAC96", p("aac96.m4a"), 61))

        # MP3 at 8 kHz: short blocks with the 72-line region boundary. And at 44.1:
        # count1 table B, which LAME picks for high-frequency bursts.
        ffmpeg_make(p("mp38.mp3"), STEREO_TRANSIENT % (8000, 8000, 8000, "0.9"),
                    "-ac", "1", "-c:a", "libmp3lame", "-b:a", "16k",
                    "-write_xing", "0", "-id3v2_version", "0")
        parts.append(lossy_fixture("MP38", p("mp38.mp3"), 41))
        ffmpeg_make(p("mp344.mp3"), STEREO_TRANSIENT % (44100, 44100, 44100, "0.3"),
                    "-ac", "1", "-c:a", "libmp3lame", "-b:a", "64k",
                    "-write_xing", "0", "-id3v2_version", "0")
        parts.append(lossy_fixture("MP344", p("mp344.mp3"), 97))

    with open(OUT, "w") as f:
        f.write("".join(parts))
    print("wrote %s (%d bytes)" % (OUT, os.path.getsize(OUT)))


if __name__ == "__main__":
    main()
