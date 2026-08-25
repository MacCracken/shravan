#!/usr/bin/env python3
"""Verify every syscall write literal declares its true UTF-8 byte length.

shravan writes to stdout/stderr with raw `syscall(1, fd, "literal", N)`, where N
is the byte count handed to write(2). Nothing checks that N matches the literal,
and both directions of mismatch are real defects:

  N too small  -> the tail is silently clipped (banners lost their closing ")\\n")
  N too large  -> write(2) reads past the literal into adjacent .rodata and emits
                  the stray bytes; in practice a NUL, which turns the test
                  transcript into a *binary* file for grep/diff/CI log scrapers

v2.6.8 fixed 12 such sites. The mistake is easy to repeat because the em-dash
(U+2014) that shravan uses in banners is three bytes but reads as one character,
so this runs in CI to keep the count honest.

Usage:
  scripts/check-write-lengths.py            # check, exit 1 on any mismatch
  scripts/check-write-lengths.py --fix      # rewrite counts to the true length
"""

import codecs
import glob
import re
import sys

# syscall(1|2, fd, "literal", N)  — the write syscall with an explicit length
PATTERN = re.compile(
    r'(syscall\(\s*(?:1|2)\s*,\s*\d+\s*,\s*")((?:[^"\\]|\\.)*)("\s*,\s*)(\d+)(\s*\))'
)

SOURCE_GLOBS = ["src/*.cyr", "fuzz/*.cyr", "tests/**/*.cyr"]


def literal_bytes(raw: str) -> int:
    """True UTF-8 byte length of a Cyrius string literal's runtime value."""
    try:
        # Resolve \n, \t, \" etc. without mangling the already-UTF-8 source text.
        decoded = codecs.decode(raw, "unicode_escape").encode("latin-1").decode("utf-8")
    except (UnicodeDecodeError, UnicodeEncodeError):
        decoded = (
            raw.replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace('\\"', '"')
            .replace("\\\\", "\\")
        )
    return len(decoded.encode("utf-8"))


def collect_files():
    files = []
    for pattern in SOURCE_GLOBS:
        files.extend(glob.glob(pattern, recursive=True))
    return sorted(set(files))


def main() -> int:
    fix = "--fix" in sys.argv
    files = collect_files()
    if not files:
        print("check-write-lengths: no source files found — run from the repo root")
        return 1

    checked = 0
    mismatches = []

    for path in files:
        with open(path, encoding="utf-8") as handle:
            original = handle.read()

        # Report with line numbers; rewrite over the whole text if --fix.
        for lineno, line in enumerate(original.splitlines(), 1):
            for match in PATTERN.finditer(line):
                checked += 1
                declared = int(match.group(4))
                actual = literal_bytes(match.group(2))
                if declared != actual:
                    mismatches.append((path, lineno, match.group(2), declared, actual))

        if fix:
            def rewrite(match):
                actual = literal_bytes(match.group(2))
                return (
                    match.group(1) + match.group(2) + match.group(3)
                    + str(actual) + match.group(5)
                )

            updated = PATTERN.sub(rewrite, original)
            if updated != original:
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write(updated)

    print(f"check-write-lengths: {checked} write literals across {len(files)} files")

    if not mismatches:
        print("  all byte counts correct")
        return 0

    if fix:
        print(f"  corrected {len(mismatches)} byte count(s):")
        for path, lineno, literal, declared, actual in mismatches:
            print(f"    {path}:{lineno}  {declared} -> {actual}")
        return 0

    print(f"  {len(mismatches)} MISMATCH(ES):")
    for path, lineno, literal, declared, actual in mismatches:
        kind = "SHORT — output truncated" if actual > declared else "LONG — reads past literal"
        preview = literal if len(literal) <= 60 else literal[:57] + "..."
        print(f'    {path}:{lineno}  declared={declared} actual={actual}  {kind}')
        print(f'        "{preview}"')
    print("\n  fix with: scripts/check-write-lengths.py --fix")
    return 1


if __name__ == "__main__":
    sys.exit(main())
