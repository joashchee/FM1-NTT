#!/usr/bin/env bash
# Rewrites core/tests/msfa/golden.txt from the vendored msfa (core/vendor/msfa) compiled as it is, with the integer
# kernel (not NEON). core/tests/fm6_golden.rs checks the Rust port against it. Needs clang++ or g++.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
CXX="${CXX:-$(command -v clang++ || command -v g++)}"
TMP="${TMPDIR:-/tmp}/msfa-oracle.$$"; mkdir -p "$TMP"; trap 'rm -rf "$TMP"' EXIT
"$CXX" -std=gnu++11 -O1 -w -U__aarch64__ -U__ARM_NEON__ -U__ARM_NEON -fno-fast-math -ffp-contract=off \
    -o "$TMP/oracle" "$REPO/core/tests/msfa/oracle.cc" \
    $(for f in sin exp2 freqlut lfo env pitchenv patch fm_op_kernel fm_core dx7note; do echo "$REPO/core/vendor/msfa/$f.cc"; done)
"$TMP/oracle" > "$TMP/golden.txt"
mv "$TMP/golden.txt" "$REPO/core/tests/msfa/golden.txt"
echo "wrote core/tests/msfa/golden.txt ($(wc -l < "$REPO/core/tests/msfa/golden.txt") lines)"
