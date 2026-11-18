#!/bin/bash
# Builds the FM1-NTT host on this machine (a Mac or Linux with SDL2 and Rust): build/fm1ntt.
#   host/build.sh
# For the R36S itself (arm64 Debian 12) use r36s/build-deb.sh, in a container. No other checkout is needed: the host draws its own pixels.
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
OUT="$REPO/build"
mkdir -p "$OUT"
COMMIT="$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)"
[ -z "$(git -C "$REPO" status --porcelain 2>/dev/null)" ] || COMMIT="$COMMIT+dirty"
BUILD="${FM1NTT_BUILD:-$COMMIT $(git -C "$REPO" log -1 --format=%cs 2>/dev/null || echo undated)}"
echo "[*] libfm1ntt.a (cargo, release)..."
CARGO_TARGET_DIR="$OUT/target" cargo build -q --release --manifest-path "$REPO/core/Cargo.toml" 2>"$OUT/cargo.log" || { cat "$OUT/cargo.log"; echo "build.sh: the core did not build" >&2; exit 1; }
LIB="$OUT/target/release/libfm1ntt.a"
[ -f "$LIB" ] || { echo "build.sh: the core did not build" >&2; exit 1; }
case "$(uname -s)" in
    Darwin) SYS="-lm -framework IOKit -framework CoreFoundation -framework CoreMIDI" ;;
    *) SYS="-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc" ;;
esac
echo "[*] fm1ntt (C, SDL2)..."
# shellcheck disable=SC2046,SC2086  # flag lists
cc -std=gnu11 -Wall -Wextra -Wno-unused-parameter -O2 -DFM1NTT_BUILD=\""$BUILD"\" -I"$REPO/core" \
    -o "$OUT/fm1ntt" "$HERE/fm1ntt.c" "$LIB" $(sdl2-config --cflags --libs) $SYS
echo "[*] Done: $OUT/fm1ntt"
