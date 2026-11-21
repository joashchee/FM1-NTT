#!/bin/bash
# scripts/build-linux.sh
#
# The desktop Linux build: build/linux/FM1-NTT_<version>_linux-<arch>.tar.gz (x86_64 on CI and on an Intel Mac's Docker, aarch64 on an Apple chip's),
# with the program, the permissively licensed bundled firmware and its licences, and NONE of the GPL pack (checked: check-bundled.py --no-pack).
# Built in Debian 12 (rust:1.98-bookworm), so it links against glibc 2.36 and runs on Debian 12, Ubuntu 22.04 and newer; it needs only the system's SDL2
# (apt install libsdl2-2.0-0). The program finds the firmware in the folder beside it. --smoke-style: the self-test runs from the unpacked archive with SDL's
# dummy drivers. Needs only Docker. FM1NTT_VERSION and FM1NTT_BUILD pass through. Windows is not built: the host uses fork, signals and POSIX directories.
set -eu
die() { echo "build-linux: $*" >&2; exit 1; }
REPO="$(cd "$(dirname "$0")/.." && pwd)"

if [ "${1:-}" != "--inside" ]; then
    command -v docker >/dev/null || die "Docker is not installed (brew install colima docker, then colima start)"
    # Docker Desktop's leftover config names a credential helper that is not installed without it: pulls then fail. Use an empty config.
    if [ -z "${DOCKER_CONFIG:-}" ] && grep -q '"credsStore"[[:space:]]*:[[:space:]]*"desktop"' "$HOME/.docker/config.json" 2>/dev/null \
        && ! command -v docker-credential-desktop >/dev/null; then
        if [ -z "${DOCKER_HOST:-}" ]; then
            DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}' 2>/dev/null || true)"
            if [ -n "$DOCKER_HOST" ]; then export DOCKER_HOST; fi
        fi
        DOCKER_CONFIG="$(mktemp -d "${TMPDIR:-/tmp}/fm1ntt-docker.XXXXXX")"
        echo '{}' > "$DOCKER_CONFIG/config.json"
        export DOCKER_CONFIG
        trap 'rm -rf "$DOCKER_CONFIG"' EXIT
    fi
    docker info >/dev/null 2>&1 || die "Docker is not running (colima start)"
    if [ -z "${FM1NTT_BUILD:-}" ]; then
        COMMIT="$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)"
        if [ -n "$(git -C "$REPO" status --porcelain 2>/dev/null)" ]; then COMMIT="$COMMIT+dirty"; fi
        FM1NTT_BUILD="$COMMIT $(git -C "$REPO" log -1 --format=%cs 2>/dev/null || echo undated)"
    fi
    # mounted at a path that cannot occur in the code's own strings, so the leak check below means something
    docker run --rm -v "$REPO":/work/fm1ntt-build -w /work/fm1ntt-build -e FM1NTT_BUILD="$FM1NTT_BUILD" -e FM1NTT_VERSION="${FM1NTT_VERSION:-}" rust:1.98-bookworm scripts/build-linux.sh --inside
    ls "$REPO"/build/linux/*.tar.gz >/dev/null 2>&1 || die "the build finished but there is no archive in build/linux"
    ls -l "$REPO"/build/linux/*.tar.gz
    exit 0
fi

# ---- inside the container ----
[ "$(id -u)" -eq 0 ] || die "--inside runs as root in the container"
grep -q '^12' /etc/debian_version 2>/dev/null || die "needs Debian 12 (Bookworm)"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends pkg-config libsdl2-dev file python3 >/dev/null
ARCH="$(uname -m)"
VERSION="${FM1NTT_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/core/Cargo.toml" | head -1)}"
[ -n "$VERSION" ] || die "no version"
OUT="$REPO/build/linux"
NAME="FM1-NTT_${VERSION}_linux-${ARCH}"
rm -rf "${OUT:?}/${NAME:?}" "${OUT:?}/${NAME:?}.tar.gz"
mkdir -p "$OUT/$NAME/licenses"

export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$REPO=."          # no build folder in the binary (panic messages carry source paths)
echo "[*] libfm1ntt.a (Rust $(rustc --version | cut -d' ' -f2), release)..."
CARGO_TARGET_DIR="$REPO/build/linux-target-$ARCH" cargo build -q --release --manifest-path "$REPO/core/Cargo.toml"
LIB="$REPO/build/linux-target-$ARCH/release/libfm1ntt.a"
echo "[*] fm1ntt ($ARCH)..."
# shellcheck disable=SC2046  # pkg-config prints separate flags
gcc -std=gnu11 -Wall -Wextra -Wno-unused-parameter -Wno-format-truncation -O2 -DFM1NTT_BUILD=\""${FM1NTT_BUILD:-unstamped}"\" -I"$REPO/core" \
    -ffile-prefix-map="$REPO"=. -o "$OUT/$NAME/fm1ntt" "$REPO/host/fm1ntt.c" "$LIB" $(pkg-config --cflags --libs sdl2) -lgcc_s -lpthread -lm -ldl -lc -Wl,-S
strip "$OUT/$NAME/fm1ntt"
needed="$(readelf -d "$OUT/$NAME/fm1ntt" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | sort -u | tr '\n' ' ')"
echo "    needs: $needed"
for lib in $needed; do
    case "$lib" in
        libSDL2-2.0.so.0|libm.so.6|libc.so.6|libdl.so.2|libpthread.so.0|libgcc_s.so.1|librt.so.1|libutil.so.1|ld-linux-aarch64.so.1|ld-linux-x86-64.so.2) ;;
        *) die "unexpected shared library $lib: only SDL2 and the C library may be needed" ;;
    esac
done
glibc="$(readelf -V "$OUT/$NAME/fm1ntt" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
echo "    newest glibc symbol: $glibc (Debian 12 has 2.36)"
case "$glibc" in GLIBC_2.3[7-9]*|GLIBC_2.[4-9]*) die "needs $glibc: newer than Debian 12's glibc 2.36" ;; esac
if grep -a -q -F "$REPO" "$OUT/$NAME/fm1ntt"; then die "the binary names the build folder $REPO"; fi

echo "[*] the bundled firmware (checked first)..."
python3 "$REPO/scripts/check-bundled.py" "$REPO" || die "the bundled firmware failed its check"
cp -R "$REPO/bundled-firmware" "$OUT/$NAME/bundled-firmware"
find "$OUT/$NAME" -name '.DS_Store' -delete
cp "$REPO/LICENSE" "$REPO/NOTICE" "$OUT/$NAME/licenses/"
cat > "$OUT/$NAME/README.txt" <<TXT
FM1-NTT $VERSION (linux-$ARCH), build $FM1NTT_BUILD

Run ./fm1ntt. It needs SDL2: on Debian and Ubuntu, sudo apt install libsdl2-2.0-0. Built on Debian 12 (glibc 2.36).
Your firmware goes in the folder the app shows on its FILES panel (FM1NTT_DIR sets it). The firmware in bundled-firmware/ ships under its own
permissive licences (see its NOTICE and licence files); the app itself is MIT (licenses/). Nothing here has run on a real FM-1.
TXT
python3 "$REPO/scripts/check-bundled.py" --no-pack "$OUT/$NAME" "$REPO" || die "the archive contains something from the copyleft pack"

echo "[*] the self-test from the unpacked archive (SDL dummy drivers)..."
SMOKE="$(mktemp -d)"
tar -C "$OUT" -czf "$OUT/$NAME.tar.gz" "$NAME"
tar -C "$SMOKE" -xzf "$OUT/$NAME.tar.gz"
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy FM1NTT_DIR="$SMOKE/data" "$SMOKE/$NAME/fm1ntt" --selftest 2>&1 | tail -3
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy FM1NTT_DIR="$SMOKE/data" "$SMOKE/$NAME/fm1ntt" --selftest >/dev/null 2>&1 || die "the self-test failed from the unpacked archive"
rm -rf "$SMOKE"
( cd "$OUT" && sha256sum "$NAME.tar.gz" )
echo "[*] Built: $OUT/$NAME.tar.gz"
