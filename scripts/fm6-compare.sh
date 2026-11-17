#!/usr/bin/env bash
# Plays the same voices on Felucca (in the emulator, as data) and on the fm6 engine, and prints one comparison line each (docs/FM6.md).
#   scripts/fm6-compare.sh [FELUCCA.fwsc]
#   FM6_TUNING=felucca scripts/fm6-compare.sh      (fm6 tuned as Felucca measured: fm6::Tuning, docs/FM6.md; default msfa's; the PAD preset's DTUN macro, 30, is set)
# Felucca's part 2 is its FM6 engine (the ALGORITHM encoder turns to it); its five FM6 presets are the first five voices of its bank (TINE EP, BELL,
# FM BASS, BRASS, PAD), and the effects (chorus, delay, reverb) are turned to 0 so only the engine is heard. Needs the Firmware Pack beside this repository
# or a path. The firmware is read as data by the emulator and nothing of it is copied.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
FW="${1:-$REPO/../FM1-NTT-Firmware-Pack/felucca/felucca-1.8.2.fwsc}"
[ -f "$FW" ] || { echo "fm6-compare: no firmware at $FW (give the path to felucca-1.8.2.fwsc)" >&2; exit 2; }
export CARGO_TARGET_DIR="$REPO/build/target"
cargo build -q --release --manifest-path "$REPO/core/Cargo.toml" --example fm6cmp
X="$CARGO_TARGET_DIR/release/examples/fm6cmp"
FX=(--btn FX --enc KNOB2:-64 --enc KNOB3:-64 --enc KNOB4:-64 --btn HOME)
for p in 0 1 2 3 4; do
    for note in 36 60 84; do
        for vel in 40 100; do
            "$X" "$FW" --brief ${FM6_TUNING:+--tuning "$FM6_TUNING" --dtun "$([ "$p" = 4 ] && echo 30 || echo 0)"} --voice "$p" --enc ALGORITHM:1 --enc PRESETS:$((p - 4)) "${FX[@]}" --channel 2 --note "$note" --vel "$vel" | tail -1
        done
    done
done
