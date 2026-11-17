#!/bin/bash
# Builds the core as WebAssembly for the web app (and for fm1tastik's Preview and its headless verify):
#   web/public/fm1ntt-<first 12 hex of its SHA-256>.wasm   the module, named by its hash so it can be cached for good
#   web/public/build-info.json                             which file is current, its full SHA-256, size, core version, commit, date
# Both are build output (gitignored). The page loads build-info.json (no-cache), then the hashed file, and checks the hash (fm1ntt.js: loadBuild).
#   web/build.sh
# Needs: cargo and the wasm32 target (rustup target add wasm32-unknown-unknown), and node (to check the module's imports and exports).
# wasm-opt (binaryen 133) is deliberately not used: tried 2026-11-17 on one firmware (-O3, -Os): raw -6%, gzipped +1.8%, JIT speed unchanged, interpreter -8% (docs/PERFORMANCE.md).
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PUB="$REPO/web/public"
die() { echo "web/build.sh: $*" >&2; exit 1; }

command -v cargo >/dev/null || die "cargo is not installed"
command -v node >/dev/null || die "node is not installed (it checks the module's imports and exports)"
libdir="$(rustc --print target-libdir --target wasm32-unknown-unknown 2>/dev/null || true)"
[ -n "$libdir" ] || die "the wasm32 target is not installed: rustup target add wasm32-unknown-unknown"
[ -d "$libdir" ] || die "the wasm32 target is not installed: rustup target add wasm32-unknown-unknown"

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

echo "== building the core for wasm32"
# cd into the crate: its .cargo/config.toml (export and grow the function table) is found from the working directory
(cd "$REPO/web/wasm" || exit 1; CARGO_TARGET_DIR="$REPO/build/web-wasm" cargo build --release --locked --target wasm32-unknown-unknown -q 2>&1 | grep -E "^(error|warning: unused)" -A8 || true)
built="$REPO/build/web-wasm/wasm32-unknown-unknown/release/fm1ntt_web.wasm"
[ -f "$built" ] || die "the build produced no module (run it without -q to see why: cd web/wasm && cargo build --release --target wasm32-unknown-unknown)"

echo "== checking the module's contract"
node - "$built" <<'JS' || die "the module does not match what web/public/fm1ntt.js expects"
const m = new WebAssembly.Module(require('fs').readFileSync(process.argv[2]));
const imp = WebAssembly.Module.imports(m).map((i) => `${i.module}.${i.name}:${i.kind}`);
const exp = new Set(WebAssembly.Module.exports(m).map((e) => `${e.name}:${e.kind}`));
const want = ['memory:memory', '__indirect_function_table:table', 'fm1ntt_wasm_alloc:function', 'fm1ntt_wasm_free:function', 'fm1ntt_new:function', 'fm1ntt_free:function',
  'fm1ntt_load_package:function', 'fm1ntt_loaded:function', 'fm1ntt_render:function', 'fm1ntt_screen:function', 'fm1ntt_key:function', 'fm1ntt_jit:function', 'fm1ntt_version:function', 'fm1ntt_error:function'];
const missing = want.filter((w) => !exp.has(w));
if (imp.length !== 1 || imp[0] !== 'env.fm1_jit_compile:function') { console.error('imports must be exactly env.fm1_jit_compile, got', imp); process.exit(1); }
if (missing.length) { console.error('missing exports:', missing); process.exit(1); }
JS

hash="$(sha256 "$built")"
name="fm1ntt-${hash:0:12}.wasm"
rm -f "$PUB"/fm1ntt-*.wasm
cp "$built" "$PUB/$name"
core_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/core/Cargo.toml" | head -1)"
commit="$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)"
tmp="$PUB/build-info.json.tmp"
printf '{\n  "format": 1,\n  "wasm": "%s",\n  "sha256": "%s",\n  "bytes": %s,\n  "core": "%s",\n  "commit": "%s",\n  "built": "%s"\n}\n' \
    "$name" "$hash" "$(wc -c <"$built" | tr -d ' ')" "$core_version" "$commit" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$tmp"
mv "$tmp" "$PUB/build-info.json"
gz="$(gzip -9 -c "$built" | wc -c | tr -d ' ')"
echo "web/public/$name  $(wc -c <"$built" | tr -d ' ') bytes ($gz gzipped)  sha256 ${hash:0:16}…  core $core_version  commit $commit"
