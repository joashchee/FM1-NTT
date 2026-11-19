#!/bin/bash
# build-deb.sh [--smoke]
#
# Builds the R36S package: build/pkg/FM1-NTT.deb (package r36s-fm1ntt, arm64, Debian 12), which r3i6nos's deb_watcher installs from
# the games card's native_deb folder like any native app (binary usr/games/fm1ntt, licences in usr/share/doc/fm1ntt).
# Run as root in Rust's official Debian 12 image, so the binary links against Bookworm's SDL2 and glibc 2.36, the R36S rootfs's own
# (r36s/build-deb.sh does this); no other checkout is needed. On arm64 (Colima on an Apple chip) everything builds natively; on x86_64 Rust and gcc cross-compile
# to aarch64 and --smoke runs under qemu-user. --smoke runs `fm1ntt --selftest` from the unpacked .deb with SDL's dummy drivers.
# The bundled firmware (bundled-firmware/) is installed in usr/share/fm1ntt/firmware, its licences in usr/share/doc/fm1ntt/bundled.
# FM1NTT_VERSION sets the version (default: core/Cargo.toml's); FM1NTT_BUILD is the build line the splash shows.
set -eu

SMOKE=""; [ "${1:-}" = "--smoke" ] && SMOKE=1
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
OUT="$REPO/build/pkg"
TARGET=aarch64-unknown-linux-gnu
HOST=aarch64-linux-gnu
die() { echo "build-deb: $*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root (it installs packages); see r36s/build-deb.sh"
grep -q '^12' /etc/debian_version 2>/dev/null || die "needs Debian 12 (Bookworm); see r36s/build-deb.sh"
command -v cargo >/dev/null || die "needs Rust (the rust:1.98-bookworm image has it)"

NATIVE=""; [ "$(uname -m)" = aarch64 ] && NATIVE=1
export DEBIAN_FRONTEND=noninteractive
pkgs="pkg-config dpkg-dev file python3"
if [ -n "$NATIVE" ]; then
    pkgs="$pkgs libsdl2-dev"
else
    dpkg --add-architecture arm64
    pkgs="$pkgs crossbuild-essential-arm64 libsdl2-dev:arm64"
    [ -n "$SMOKE" ] && pkgs="$pkgs qemu-user-static"
fi
apt-get update -qq
# shellcheck disable=SC2086  # a list of package names
apt-get install -y -qq --no-install-recommends $pkgs >/dev/null
if [ -n "$NATIVE" ]; then
    CC=gcc STRIP=strip READELF=readelf
else
    rustup target add "$TARGET" >/dev/null
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$HOST-gcc" PKG_CONFIG_LIBDIR="/usr/lib/$HOST/pkgconfig:/usr/share/pkgconfig" PKG_CONFIG_PATH=
    CC="$HOST-gcc" STRIP="$HOST-strip" READELF="$HOST-readelf"
fi

VERSION="${FM1NTT_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/core/Cargo.toml" | head -1)}"
rm -rf "$OUT/root" "$OUT/FM1-NTT.deb"      # only this package: the firmware pack's .deb lives beside it
mkdir -p "$OUT"
pkg="$OUT/root"
mkdir -p "$pkg/DEBIAN" "$pkg/usr/games" "$pkg/usr/share/doc/fm1ntt"

echo "[*] libfm1ntt.a (Rust $(rustc --version | cut -d' ' -f2), $TARGET, release)..."
CARGO_TARGET_DIR="$REPO/build/linux-target" cargo build -q --release --target "$TARGET" --manifest-path "$REPO/core/Cargo.toml"
LIB="$REPO/build/linux-target/$TARGET/release/libfm1ntt.a"

echo "[*] Compiling fm1ntt (arm64)..."
# shellcheck disable=SC2046  # pkg-config prints separate flags
"$CC" -std=gnu11 -Wall -Wextra -Wno-unused-parameter -Wno-format-truncation -O2 -DFM1NTT_BUILD=\""${FM1NTT_BUILD:-unstamped}"\" -I"$REPO/core" \
    -o "$pkg/usr/games/fm1ntt" "$REPO/host/fm1ntt.c" "$LIB" $(pkg-config --cflags --libs sdl2) -lgcc_s -lpthread -lm -ldl -lc
"$STRIP" "$pkg/usr/games/fm1ntt"
file "$pkg/usr/games/fm1ntt" | grep -q 'ARM aarch64' || die "fm1ntt is not an arm64 binary"
# The rootfs installs no dependencies: the binary may need only what it already has (SDL2, glibc, libgcc).
needed="$("$READELF" -d "$pkg/usr/games/fm1ntt" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | sort -u | tr '\n' ' ')"
echo "    needs: $needed"
for lib in $needed; do
    case "$lib" in
        libSDL2-2.0.so.0|libm.so.6|libc.so.6|libdl.so.2|libpthread.so.0|libgcc_s.so.1|librt.so.1|libutil.so.1|ld-linux-aarch64.so.1) ;;
        *) die "unexpected shared library $lib: the rootfs may not have it" ;;
    esac
done
echo "$needed" | grep -q 'libSDL2-2.0.so.0' || die "fm1ntt does not link SDL2"
glibc="$("$READELF" -V "$pkg/usr/games/fm1ntt" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
echo "    newest glibc symbol: $glibc (the rootfs has 2.36)"
case "$glibc" in GLIBC_2.3[7-9]*|GLIBC_2.[4-9]*) die "needs $glibc: newer than the rootfs's glibc 2.36" ;; esac

echo "[*] The bundled firmware (checked first: scripts/check-bundled.py)..."
python3 "$REPO/scripts/check-bundled.py" "$REPO" || die "the bundled firmware failed its check"
mkdir -p "$pkg/usr/share/fm1ntt/firmware" "$pkg/usr/share/doc/fm1ntt/bundled"
for fam in "$REPO"/bundled-firmware/*/; do
    n="$(basename "$fam")"
    mkdir -p "$pkg/usr/share/fm1ntt/firmware/$n" "$pkg/usr/share/doc/fm1ntt/bundled/$n"
    find "$fam" -maxdepth 1 -type f \( -name '*.fwsc' -o -name '*.fm1i' \) -exec cp {} "$pkg/usr/share/fm1ntt/firmware/$n/" \;
    find "$fam" -maxdepth 1 -type f ! -name '*.fwsc' ! -name '*.fm1i' ! -name '.*' -exec cp {} "$pkg/usr/share/doc/fm1ntt/bundled/$n/" \;
    [ -d "$fam/docs" ] && cp -R "$fam/docs" "$pkg/usr/share/doc/fm1ntt/bundled/$n/docs"
    find "$pkg/usr/share" -name '.DS_Store' -delete
done
cp "$REPO/bundled-firmware/MANIFEST.tsv" "$pkg/usr/share/doc/fm1ntt/bundled/"
ls "$pkg"/usr/share/fm1ntt/firmware/*/*.fwsc >/dev/null || die "no bundled firmware was packed"
python3 "$REPO/scripts/check-bundled.py" --no-pack "$pkg" "$REPO" || die "the MIT app's package contains something from the copyleft pack"

echo "[*] Licences..."
cp "$HERE/copyright" "$pkg/usr/share/doc/fm1ntt/copyright"
cp "$REPO/LICENSE" "$REPO/NOTICE" "$pkg/usr/share/doc/fm1ntt/"
cp "$REPO/core/vendor/fm1-emu/LICENSE" "$pkg/usr/share/doc/fm1ntt/LICENSE.fm1-emu"
cp "$REPO/core/vendor/msfa/COPYING" "$pkg/usr/share/doc/fm1ntt/LICENSE.msfa-Apache-2.0"

size_kb="$(du -sk "$pkg/usr" | cut -f1)"
cat > "$pkg/DEBIAN/control" <<CONTROL
Package: r36s-fm1ntt
Version: $VERSION
Architecture: arm64
Maintainer: ANSI Apps <noreply@ansiapps.com>
Installed-Size: $size_kb
Depends: libsdl2-2.0-0
Section: games
Priority: optional
Description: FM1-NTT: a sandbox for FM-1 firmware (native ARM64, SDL2)
 Runs the M-VAVE FM-1's community firmware images in an emulator, to audition them before flashing a real FM-1.
 Includes five permissively licensed firmwares (Haunted FM-1, MIT with MPL-2.0 and Apache-2.0 parts; see /usr/share/doc/fm1ntt/bundled);
 bring your own for any other. Free and MIT-licensed.
CONTROL

echo "[*] Packing FM1-NTT.deb..."
find "$pkg" -exec touch -h -d '2026-01-01 00:00:00 UTC' {} +      # same bytes for the same source
dpkg-deb --root-owner-group -Zxz --build "$pkg" "$OUT/FM1-NTT.deb"
dpkg-deb -I "$OUT/FM1-NTT.deb" | sed -n 1,9p
echo "[*] Done: $OUT/FM1-NTT.deb ($(du -k "$OUT/FM1-NTT.deb" | cut -f1) KiB)"

if [ -n "$SMOKE" ]; then
    tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
    dpkg-deb -x "$OUT/FM1-NTT.deb" "$tmp"
    if [ -n "$NATIVE" ]; then run=(); else run=(qemu-aarch64-static -L /); fi
    echo "[*] Smoke test (the installed layout, dummy video and audio)..."
    # under qemu-user (CI: x86_64 emulating arm64) the self-test is light: the showcase's real 15 s runs and workers are many minutes of emulation; the native CI job and a native arm64 build run them
    light=(); [ -n "$NATIVE" ] || light=(FM1NTT_SELFTEST_LIGHT=1)
    out="$(cd "$tmp" && env SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy ${light[@]+"${light[@]}"} timeout 300 ${run[@]+"${run[@]}"} "$tmp/usr/games/fm1ntt" --selftest 2>&1)" || { echo "$out"; die "smoke test FAILED"; }
    echo "$out" | tail -3
    echo "$out" | grep -q 'selftest: ok' || die "smoke test printed no ok line"
    echo "build-deb: smoke ok"
fi
