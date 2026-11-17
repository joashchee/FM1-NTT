#!/bin/bash
# Everything that can be checked without an R36S or an FM-1: the core's tests, a build of the host and its headless
# self-test, the web shell's PWA test and the wasm build's tests, and the scripts' syntax and shellcheck. Same as CI.
#   scripts/check.sh
# The arm64 package is r36s/build-deb.sh (needs Docker).
set -eu
REPO="$(cd "$(dirname "$0")/.." && pwd)"
echo "== core tests"
mkdir -p "$REPO/build"
CARGO_TARGET_DIR="$REPO/build/target" cargo test -q --release --manifest-path "$REPO/core/Cargo.toml" 2>&1 | grep -E "^test result|FAILED|panicked" || true
CARGO_TARGET_DIR="$REPO/build/target" cargo test -q --release --manifest-path "$REPO/core/Cargo.toml" >/dev/null 2>&1 || { echo "core tests FAILED"; exit 1; }
if command -v cargo-clippy >/dev/null 2>&1 || cargo clippy --version >/dev/null 2>&1; then
    CARGO_TARGET_DIR="$REPO/build/target" cargo clippy -q --no-deps --release --manifest-path "$REPO/core/Cargo.toml" -- -D warnings >"$REPO/build/clippy.log" 2>&1 || { grep -B2 -A12 "^error" "$REPO/build/clippy.log" | head -60; echo "clippy FAILED"; exit 1; }
    echo "clippy ok"
else echo "clippy NOT INSTALLED: not run"; fi
echo "== host"
mkdir -p "$REPO/build"
"$REPO/host/build.sh" >"$REPO/build/host-build.log" 2>&1 || { cat "$REPO/build/host-build.log"; echo "host build FAILED"; exit 1; }
out="$(SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy "$REPO/build/fm1ntt" --selftest 2>&1)" || { echo "$out"; echo "self-test FAILED"; exit 1; }
echo "$out" | tail -1
echo "== web"
# the wasm build (web/build.sh): needs the wasm32 target; without it the wasm tests are SKIPPED and this says so
if rustc --print target-libdir --target wasm32-unknown-unknown >/dev/null 2>&1 && [ -d "$(rustc --print target-libdir --target wasm32-unknown-unknown)" ]; then
    "$REPO/web/build.sh" >"$REPO/build/web-build.log" 2>&1 || { cat "$REPO/build/web-build.log"; echo "web build FAILED"; exit 1; }
    tail -1 "$REPO/build/web-build.log"
else echo "wasm32 target NOT INSTALLED: web/build.sh not run, the wasm tests will be skipped (rustup target add wasm32-unknown-unknown)"; fi
if command -v node >/dev/null; then node --test "$REPO/web/" 2>&1 | grep -E "^ℹ (pass|fail)" ; node --test "$REPO/web/" >/dev/null 2>&1 || { echo "web test FAILED"; exit 1; }
else echo "node NOT INSTALLED: web test not run"; fi
echo "== bundled-firmware checker"
if python3 "$REPO/scripts/test_check_bundled.py" >"$REPO/build/check-bundled.log" 2>&1; then tail -1 "$REPO/build/check-bundled.log"; else cat "$REPO/build/check-bundled.log"; echo "check-bundled tests FAILED"; exit 1; fi
echo "== firmware update checker"
if python3 "$REPO/scripts/test_check_firmware_updates.py" >"$REPO/build/check-updates.log" 2>&1; then tail -1 "$REPO/build/check-updates.log"; else cat "$REPO/build/check-updates.log"; echo "check-firmware-updates tests FAILED"; exit 1; fi
echo "== bench-compare"
if python3 "$REPO/scripts/test_bench_compare.py" >"$REPO/build/bench-compare.log" 2>&1; then tail -1 "$REPO/build/bench-compare.log"; else cat "$REPO/build/bench-compare.log"; echo "bench-compare tests FAILED"; exit 1; fi
echo "== scripts"
for f in "$REPO"/scripts/check.sh "$REPO"/host/build.sh "$REPO"/web/build.sh "$REPO"/r36s/build-deb.sh "$REPO"/r36s/package/*.sh; do bash -n "$f"; done
if command -v shellcheck >/dev/null; then
    shellcheck "$REPO"/scripts/check.sh "$REPO"/host/build.sh "$REPO"/web/build.sh "$REPO"/r36s/build-deb.sh "$REPO"/r36s/package/*.sh || { echo "shellcheck FAILED"; exit 1; }
    echo "shellcheck ok"
else echo "shellcheck NOT INSTALLED: not run"; fi
echo "== bundled firmware"
python3 "$REPO/scripts/check-bundled.py" "$REPO" || { echo "bundled firmware check FAILED"; exit 1; }
# the FM1-NTT Firmware Pack is its own repository: when it is beside this one (or still inside it, before it is moved out) check it too
for PACK in "$REPO/../FM1-NTT-Firmware-Pack" "$REPO/FM1-NTT-Firmware-Pack"; do
    if [ -f "$PACK/MANIFEST.tsv" ]; then
        python3 "$PACK/scripts/check-bundled.py" --pack "$PACK" || { echo "the firmware pack repository FAILED its check"; exit 1; }
        break
    fi
done
echo "fm1ntt check: ok"
