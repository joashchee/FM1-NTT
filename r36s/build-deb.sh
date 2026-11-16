#!/bin/bash
# r36s/build-deb.sh
#
# The MIT app's package: build/pkg/FM1-NTT.deb, with the permissively licensed bundled firmware and NONE of the GPL pack (checked: check-bundled.py --no-pack).
# The copyleft firmware pack is its own repository, FM1-NTT Firmware Pack, with its own ./build-deb.sh.
#
# Builds FM1-NTT for the R36S on this computer: build/pkg/FM1-NTT.deb (package r36s-fm1ntt, arm64, Debian 12), which r3i6nos's
# flash-macos.sh copies onto the games card and deb_watcher installs at boot. Needs only Docker (Colima on an Apple chip: the VM is
# arm64, so everything builds natively). The work is r36s/package/build-deb.sh, run as root in rust:1.98-bookworm, so the binary
# links against Bookworm's SDL2 and glibc, the R36S rootfs's own; it also runs the self-test from the unpacked .deb.
# FM1NTT_VERSION passes through. No r3i6nos checkout is needed: the host draws its own pixels.
set -eu
die() { echo "build-deb: $*" >&2; exit 1; }
case "${1:-}" in -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac

REPO="$(cd "$(dirname "$0")/.." && pwd)"
command -v docker >/dev/null || die "Docker is not installed (brew install colima docker, then colima start)"
# Docker Desktop's leftover config names a credential helper that is not installed without it: pulls then fail. Use an empty config.
if [ -z "${DOCKER_CONFIG:-}" ] && grep -q '"credsStore"[[:space:]]*:[[:space:]]*"desktop"' "$HOME/.docker/config.json" 2>/dev/null \
    && ! command -v docker-credential-desktop >/dev/null; then
    if [ -z "${DOCKER_HOST:-}" ]; then
        DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}' 2>/dev/null || true)"
        [ -n "$DOCKER_HOST" ] && export DOCKER_HOST
    fi
    DOCKER_CONFIG="$(mktemp -d "${TMPDIR:-/tmp}/fm1ntt-docker.XXXXXX")"
    echo '{}' > "$DOCKER_CONFIG/config.json"
    export DOCKER_CONFIG
    trap 'rm -rf "$DOCKER_CONFIG"' EXIT
fi
docker info >/dev/null 2>&1 || die "Docker is not running (colima start)"

# the build stamp (short commit, +dirty, the commit's date) is made here: the container has no git identity for /src
if [ -z "${FM1NTT_BUILD:-}" ]; then
    COMMIT="$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)"
    [ -z "$(git -C "$REPO" status --porcelain 2>/dev/null)" ] || COMMIT="$COMMIT+dirty"
    FM1NTT_BUILD="$COMMIT $(git -C "$REPO" log -1 --format=%cs 2>/dev/null || echo undated)"
fi
export FM1NTT_BUILD
docker run --rm --platform linux/arm64 -v "$REPO":/src -w /src \
    -e FM1NTT_BUILD="$FM1NTT_BUILD" -e FM1NTT_VERSION="${FM1NTT_VERSION:-}" rust:1.98-bookworm \
    r36s/package/build-deb.sh --smoke
[ -s "$REPO/build/pkg/FM1-NTT.deb" ] || die "the build finished but there is no build/pkg/FM1-NTT.deb"
echo "[*] Built: $REPO/build/pkg/FM1-NTT.deb ($(du -h "$REPO/build/pkg/FM1-NTT.deb" | cut -f1))"
