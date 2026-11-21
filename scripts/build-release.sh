#!/bin/sh
# Builds a release FM1-NTT.app that runs on any Mac someone downloads it to: universal (Apple Silicon and Intel), signed with the
# Developer ID and the hardened runtime, notarized by Apple and stapled, then zipped for the release. The maintainer's Mac only,
# never CI (a Mac app made anywhere else is "damaged" on any other Mac). Coupler's scripts/build-release.sh is the reference.
#   scripts/build-release.sh
# Published as ANSI Apps (com.ansiapps.fm1ntt), signed and notarized by Scoreless Music (the only Apple account). There is no
# Mac App Store build (ROADMAP section 6); check-bundled.py --no-pack stays as the guard that the app carries no GPL firmware.
#
# Signing and notarization need, once:
#   1. A "Developer ID Application" certificate in the login keychain (found by itself; with more than one, name it in
#      APPLE_SIGNING_IDENTITY).
#   2. notarytool's credentials under the profile every Scoreless Music app shares, set up in a real Terminal (not with !):
#        xcrun notarytool store-credentials scorelessmusic --apple-id you@example.com --team-id TEAMID
#      Another profile name: FM1NTT_NOTARY_PROFILE.
# FM1NTT_UNSIGNED=1 builds signed ad hoc and not notarized: for trying the build only, never for a release.
# FM1NTT_NO_INSTALL=1 (or CI) skips the copy into ~/Applications at the end.
#
# SDL2 is built here, once, as a universal static library from a pinned source release (the Homebrew one is one architecture
# only), so the app needs nothing installed. The core's JIT maps executable memory (MAP_JIT), which the hardened runtime allows
# only with the com.apple.security.cs.allow-jit entitlement: it is the only one asked for.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

PROFILE="${FM1NTT_NOTARY_PROFILE:-scorelessmusic}"
UNSIGNED="${FM1NTT_UNSIGNED:-}"
SDL_VER=2.32.10
SDL_SHA256=5f5993c530f084535c65a6879e9b26ad441169b3e25d789d83287040a9ca5165   # of the release tarball, taken when it was first fetched (2026-11-21)
MIN_MACOS=11.0
BUNDLE_ID=com.ansiapps.fm1ntt

[ "$(uname -s)" = Darwin ] || { echo "build-release.sh: a Mac is needed." >&2; exit 1; }

# The signing identity and the notary profile, checked before the long build.
if [ -n "$UNSIGNED" ]; then
  APPLE_SIGNING_IDENTITY="-"
  echo "FM1NTT_UNSIGNED: signed ad hoc and not notarized; not for release."
elif [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  APPLE_SIGNING_IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -n 1)
  if [ -z "$APPLE_SIGNING_IDENTITY" ]; then
    echo "No Developer ID Application certificate in the keychain. See this script's header, or set FM1NTT_UNSIGNED=1 to try the build unsigned." >&2
    exit 1
  fi
fi
if [ -z "$UNSIGNED" ]; then
  echo "Signing as: $APPLE_SIGNING_IDENTITY"
  # Notarization is done below with the keychain profile: no Apple credential is ever in the environment, the repo or CI.
  unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH 2>/dev/null || true
  if ! xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1; then
    echo "No notarytool credentials under the profile \"$PROFILE\". See this script's header." >&2
    exit 1
  fi
fi

for t in cmake cargo lipo codesign ditto iconutil sips python3; do
  command -v "$t" >/dev/null 2>&1 || { echo "build-release.sh: $t is needed and was not found." >&2; exit 1; }
done
INSTALLED_TARGETS=$(rustup target list --installed 2>/dev/null || true)
for T in aarch64-apple-darwin x86_64-apple-darwin; do
  if ! printf '%s\n' "$INSTALLED_TARGETS" | grep -qx "$T"; then
    echo "build-release.sh: both Rust targets are needed: rustup target add aarch64-apple-darwin x86_64-apple-darwin" >&2
    exit 1
  fi
done

# What goes in the app is checked first: the permissive tier only, every hash and licence in order, no GPL firmware anywhere.
python3 scripts/check-bundled.py

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' core/Cargo.toml | head -n 1)
[ -n "$VERSION" ] || { echo "build-release.sh: no version in core/Cargo.toml" >&2; exit 1; }
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
  if [ -n "$UNSIGNED" ]; then
    COMMIT="$COMMIT+dirty"
  else
    echo "build-release.sh: the working tree is not clean; commit first (a release is a commit)." >&2
    exit 1
  fi
fi
BUILD="$COMMIT $(git log -1 --format=%cs 2>/dev/null || echo undated)"

OUT="$ROOT/build/release"
mkdir -p "$OUT"

# Without the build machine's paths in it: rustc and the C compiler bake source paths into the binary (panic messages, __FILE__),
# which would publish the home folder and user name. The more specific prefix comes after $HOME (the last matching one wins).
export MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=~ --remap-path-prefix=$ROOT=."
MAPS="-ffile-prefix-map=$HOME=~ -ffile-prefix-map=$ROOT=."

# SDL2: a universal static library, built once and kept.
SDL="$OUT/sdl2-$SDL_VER"
if [ ! -f "$SDL/lib/libSDL2.a" ]; then
  echo "[*] SDL2 $SDL_VER (universal, static)..."
  TARBALL="$OUT/SDL2-$SDL_VER.tar.gz"
  if [ ! -f "$TARBALL" ]; then
    curl -fsSL -o "$TARBALL" "https://github.com/libsdl-org/SDL/releases/download/release-$SDL_VER/SDL2-$SDL_VER.tar.gz"
  fi
  [ "$(shasum -a 256 "$TARBALL" | cut -d' ' -f1)" = "$SDL_SHA256" ] || { echo "build-release.sh: SDL2's tarball does not match its pinned SHA-256." >&2; rm -f "$TARBALL"; exit 1; }
  rm -rf "$OUT/sdl2-src" "$OUT/sdl2-build" "$SDL"
  mkdir -p "$OUT/sdl2-src"
  tar -xzf "$TARBALL" -C "$OUT/sdl2-src" --strip-components=1
  cmake -S "$OUT/sdl2-src" -B "$OUT/sdl2-build" -G "Unix Makefiles" -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_ARCHITECTURES="arm64;x86_64" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$MIN_MACOS" -DSDL_SHARED=OFF -DSDL_STATIC=ON -DSDL_TEST=OFF -DSDL_INSTALL_TESTS=OFF \
    -DCMAKE_C_FLAGS="$MAPS" -DCMAKE_INSTALL_PREFIX="$SDL" >"$OUT/sdl2-cmake.log" 2>&1 || { cat "$OUT/sdl2-cmake.log"; exit 1; }
  cmake --build "$OUT/sdl2-build" -j "$(sysctl -n hw.ncpu 2>/dev/null || echo 4)" >>"$OUT/sdl2-cmake.log" 2>&1 || { tail -40 "$OUT/sdl2-cmake.log"; exit 1; }
  cmake --install "$OUT/sdl2-build" >>"$OUT/sdl2-cmake.log" 2>&1 || { tail -40 "$OUT/sdl2-cmake.log"; exit 1; }
  mkdir -p "$SDL/share/licenses" && cp "$OUT/sdl2-src/LICENSE.txt" "$SDL/share/licenses/SDL2-LICENSE.txt"
fi
SDL_LIBS=$("$SDL/bin/sdl2-config" --static-libs)
SDL_CFLAGS=$("$SDL/bin/sdl2-config" --cflags)

# The core and the host, one architecture at a time, joined by lipo.
for ARCH in arm64 x86_64; do
  case "$ARCH" in arm64) TRIPLE=aarch64-apple-darwin ;; x86_64) TRIPLE=x86_64-apple-darwin ;; esac
  echo "[*] $ARCH: the core (cargo, release)..."
  CARGO_TARGET_DIR="$OUT/target" cargo build -q --release --target "$TRIPLE" --manifest-path "$ROOT/core/Cargo.toml" 2>"$OUT/cargo-$ARCH.log" || { cat "$OUT/cargo-$ARCH.log"; exit 1; }
  echo "[*] $ARCH: the host (C, SDL2)..."
  # shellcheck disable=SC2086  # flag lists
  cc -arch "$ARCH" -mmacosx-version-min="$MIN_MACOS" -std=gnu11 -Wall -Wextra -Wno-unused-parameter -O2 $MAPS -DFM1NTT_BUILD=\""$BUILD"\" \
    -I"$ROOT/core" $SDL_CFLAGS -o "$OUT/fm1ntt-$ARCH" "$ROOT/host/fm1ntt.c" "$OUT/target/$TRIPLE/release/libfm1ntt.a" \
    $SDL_LIBS -framework IOKit -framework CoreFoundation -framework CoreMIDI -Wl,-S   # -S: no debug map, which names the object files by their full paths
done
lipo -create -output "$OUT/fm1ntt" "$OUT/fm1ntt-arm64" "$OUT/fm1ntt-x86_64"

# The bundle.
APP="$OUT/FM1-NTT.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$OUT/fm1ntt" "$APP/Contents/MacOS/fm1ntt"
# The permissive firmware (the app finds it beside its resources as bundled-firmware), with its licences, sources notes and NOTICE.
rsync -a --exclude=.DS_Store bundled-firmware/ "$APP/Contents/Resources/bundled-firmware/"
mkdir -p "$APP/Contents/Resources/licenses"
cp LICENSE NOTICE "$APP/Contents/Resources/licenses/"
cp "$SDL/share/licenses/SDL2-LICENSE.txt" "$APP/Contents/Resources/licenses/"
# The icon, from the PWA's art (original, drawn by scripts/build-icons.ts): an .icns holding the 512 x 512 PNG as the 512 and as the 256 at 2x
# (ic09 and ic14); macOS scales it for the smaller places. Written here, not by iconutil, so it is the same bytes every time.
python3 - web/public/icon-512.png "$APP/Contents/Resources/AppIcon.icns" <<'PY'
import struct, sys
png = open(sys.argv[1], "rb").read()
chunks = b"".join(t + struct.pack(">I", 8 + len(png)) + png for t in (b"ic09", b"ic14"))
open(sys.argv[2], "wb").write(b"icns" + struct.pack(">I", 8 + len(chunks)) + chunks)
PY
cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>FM1-NTT</string>
  <key>CFBundleDisplayName</key><string>FM1-NTT</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>fm1ntt</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>$MIN_MACOS</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>MIT licence. Firmware inside keeps its own licences.</string>
</dict>
</plist>
PLIST
ENT="$OUT/fm1ntt.entitlements"
cat >"$ENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
</dict>
</plist>
PLIST

# The rule: no GPL firmware, no source archive, no pack folder in what is signed.
python3 scripts/check-bundled.py --no-pack "$APP"

# No build machine paths in the app or in either architecture's bare binary.
LEAKS=$(grep -r -l -a -F "$HOME" "$APP" "$OUT/fm1ntt-arm64" "$OUT/fm1ntt-x86_64" 2>/dev/null || true)
if [ -n "$LEAKS" ]; then
  echo "The build still contains $HOME in:" >&2
  echo "$LEAKS" >&2
  exit 1
fi
echo "Checked: no $HOME paths in the release build."

ARCHS=$(lipo -archs "$APP/Contents/MacOS/fm1ntt")
case "$ARCHS" in
  *arm64*x86_64* | *x86_64*arm64*) echo "Checked: universal ($ARCHS)." ;;
  *) echo "Not universal: $ARCHS" >&2; exit 1 ;;
esac

# Signed with the hardened runtime and the one entitlement (ad hoc has no timestamp).
if [ -n "$UNSIGNED" ]; then
  codesign --force --options runtime --entitlements "$ENT" --sign - "$APP"
else
  codesign --force --options runtime --timestamp --entitlements "$ENT" --sign "$APPLE_SIGNING_IDENTITY" "$APP"
fi
codesign --verify --deep --strict "$APP"
if [ -z "$UNSIGNED" ]; then
  codesign -dvv "$APP" 2>&1 | grep -q "Authority=Developer ID Application" || { echo "Not signed with a Developer ID." >&2; exit 1; }
  codesign -dvv "$APP" 2>&1 | grep -q "flags=.*runtime" || { echo "Not signed with the hardened runtime." >&2; exit 1; }
  echo "Checked: signed with the Developer ID and the hardened runtime."

  # Apple scans a zip of the app; the ticket is then stapled to the app so it opens offline.
  SUBMIT="$OUT/FM1-NTT-notarize.zip"
  rm -f "$SUBMIT"
  ditto -c -k --keepParent "$APP" "$SUBMIT"
  RESULT=$(xcrun notarytool submit "$SUBMIT" --keychain-profile "$PROFILE" --wait --output-format json)
  STATUS=$(printf '%s' "$RESULT" | python3 -c 'import json,sys; j=json.load(sys.stdin); print(j.get("status",""), j.get("id",""))')
  case "$STATUS" in
    Accepted*) echo "Notarized." ;;
    *)
      echo "Notarization failed ($STATUS). Apple's log:" >&2
      xcrun notarytool log "${STATUS#* }" --keychain-profile "$PROFILE" >&2 || true
      exit 1
      ;;
  esac
  rm -f "$SUBMIT"
  xcrun stapler staple "$APP"
  spctl --assess --type execute -vv "$APP" 2>&1 | grep -q "source=Notarized Developer ID" || { echo "Gatekeeper doesn't accept the app." >&2; exit 1; }
  echo "Checked: Gatekeeper accepts it as notarized."
fi

# The zip to upload to the version's release (gh release upload, by hand), the stapled app inside; ditto keeps the signature.
ZIP="$OUT/FM1-NTT_${VERSION}_universal.app.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "For the release: $ZIP"
( cd "$OUT" && shasum -a 256 "$(basename "$ZIP")" )

if [ -d "$APP" ] && [ -z "${CI:-}" ] && [ -z "${FM1NTT_NO_INSTALL:-}" ]; then
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/FM1-NTT.app"
  ditto "$APP" "$HOME/Applications/FM1-NTT.app"
  echo "Installed: ~/Applications/FM1-NTT.app"
fi
