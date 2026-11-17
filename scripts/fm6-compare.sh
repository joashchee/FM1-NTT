#!/usr/bin/env bash
# Plays the same voices on a Felucca-family firmware (in the emulator, as data) and on the fm6 engine, and prints one comparison line each (docs/FM6.md).
#   scripts/fm6-compare.sh [FIRMWARE.fwsc]
#   FM6_FIRMWARE=jangada|skiff scripts/fm6-compare.sh      which firmware's way of reaching its FM6 presets (default felucca)
#   FM6_TUNING=felucca scripts/fm6-compare.sh              fm6 tuned as Felucca measured: fm6::Tuning, docs/FM6.md; default msfa's. The PAD preset's DTUN macro (30) is set
# Part 2 of each is its FM6 engine (the ALGORITHM encoder turns to it). Its presets F1..F8 are the first eight voices of the firmware's bank (Felucca lists five of
# them, Jangada and Skiff eight), and the effects (chorus, delay, reverb) are turned to 0 so only the engine is heard. Needs the Firmware Pack beside this repository
# or a path. The firmware is read as data by the emulator and nothing of it is copied.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PACK="$REPO/../FM1-NTT-Firmware-Pack"
case "${FM6_FIRMWARE:-felucca}" in
    felucca) DEFAULT="$PACK/felucca/felucca-1.8.2.fwsc"; PRESETS=5 ;;
    jangada) DEFAULT="$PACK/jangada/jangada-1.0.1.fwsc"; PRESETS=8 ;;
    skiff)   DEFAULT="$PACK/skiff/skiff-2.6.fwsc"; PRESETS=8 ;;
    *) echo "fm6-compare: FM6_FIRMWARE is felucca, jangada or skiff" >&2; exit 2 ;;
esac
FW="${1:-$DEFAULT}"
[ -f "$FW" ] || { echo "fm6-compare: no firmware at $FW" >&2; exit 2; }
export CARGO_TARGET_DIR="$REPO/build/target"
cargo build -q --release --manifest-path "$REPO/core/Cargo.toml" --example fm6cmp
X="$CARGO_TARGET_DIR/release/examples/fm6cmp"
FX=(--btn FX --enc KNOB2:-64 --enc KNOB3:-64 --enc KNOB4:-64 --btn HOME)
for ((p = 0; p < PRESETS; p++)); do
    case "${FM6_FIRMWARE:-felucca}" in
        # Jangada's FM6 presets are 55 single clicks down its list from where part 2 starts (a big turn is not taken one for one)
        jangada) NAV=(--enc ALGORITHM:1 --encn PRESETS:$((55 + p))) ;;
        # Felucca and Skiff start part 2 on the PAD preset (F5): turn down to F1, up to F8
        *) NAV=(--enc ALGORITHM:1 --enc PRESETS:$((p - 4))) ;;
    esac
    DTUN=0
    [ "$p" = 4 ] && DTUN=30
    for note in 36 60 84; do
        for vel in 40 100; do
            "$X" "$FW" --brief ${FM6_TUNING:+--tuning "$FM6_TUNING" --dtun "$DTUN"} --voice "$p" "${NAV[@]}" "${FX[@]}" --channel 2 --note "$note" --vel "$vel" | tail -1
        done
    done
done
