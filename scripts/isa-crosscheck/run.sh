#!/bin/sh
# Compares fm1-isa's decoder with pi32v2-isa's table on the same instruction windows.
# Usage: scripts/isa-crosscheck/run.sh PI32V2_ISA_CHECKOUT WORKDIR
# PI32V2_ISA_CHECKOUT is a clone of https://github.com/jvitkauskas/pi32v2-isa (MIT-0 code, CC0 table); WORKDIR is an empty
# folder for the results. pi32v2-isa needs Python 3.11+, so its side runs in a python:3.12-slim container with no network.
# Reads nothing of any GPL project. See docs/ISA-CROSSCHECK.md.
set -eu
[ $# -eq 2 ] || { echo "usage: $0 PI32V2_ISA_CHECKOUT WORKDIR" >&2; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
isa=$(cd "$1" && pwd)
mkdir -p "$2"
work=$(cd "$2" && pwd)
cp "$here/gen.py" "$here/isa_side.py" "$here/compare.py" "$work/"
(cd "$work" && python3 gen.py)
c=$(docker create --network none -w /w python:3.12-slim python3 -I /w/isa_side.py)
docker cp "$isa" "$c:/isa"
docker cp "$work/." "$c:/w/"
docker start -a "$c"
docker cp "$c:/w/isa_out.tsv" "$work/isa_out.tsv"
docker rm -f "$c" >/dev/null
(cd "$here" && cargo run --release -q -- "$work/windows.txt" "$work/fm1_out.tsv")
(cd "$work" && python3 compare.py)
