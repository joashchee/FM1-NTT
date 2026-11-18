#!/usr/bin/env python3
"""A benchmark regression gate (Python 3 standard library only).

    scripts/bench-gate.py BASELINE.txt CURRENT.txt [--min-ratio 0.5] [--prefix core. --prefix screen. ...]

Both files are what `fm1ntt --bench` wrote. For every benchmark whose id starts with one of the prefixes (default `core.`, `screen.`, `io.`) and is in both,
the ratio of CURRENT to BASELINE is taken the way bench-compare.py takes it (**above 1.00x is better**, whatever the unit), and the gate FAILS when any ratio is
below --min-ratio (default 0.5: a shared CI machine is noisy, so only a halving is a regression worth stopping for). `format` and `workload` must match: a
changed workload is a stale baseline, said so and failed (regenerate the baseline). A missing baseline passes with a note, so the first run can make one.
Exit status: 0 passed (or no baseline), 1 a regression or a stale baseline, 2 usage or an unreadable CURRENT.
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("bench_compare", os.path.join(HERE, "bench-compare.py"))
bc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bc)

DEFAULT_PREFIXES = ("core.", "screen.", "io.")


def gate(baseline, current, min_ratio, prefixes):
    """(failures, checked): failures are (id, ratio) pairs below min_ratio."""
    failures, checked = [], 0
    for k, cur in current.bench.items():
        if not k.startswith(tuple(prefixes)) or k not in baseline.bench:
            continue
        q = bc.ratio(baseline.bench[k], cur)
        if q is None:
            continue
        checked += 1
        if q < min_ratio:
            failures.append((k, q))
    return failures, checked


def main(argv):
    paths, min_ratio, prefixes = [], 0.5, []
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--min-ratio" and i + 1 < len(argv):
            try:
                min_ratio = float(argv[i + 1])
            except ValueError:
                print("bench-gate: --min-ratio needs a number", file=sys.stderr)
                return 2
            i += 1
        elif a == "--prefix" and i + 1 < len(argv):
            prefixes.append(argv[i + 1])
            i += 1
        elif a in ("-h", "--help"):
            print(__doc__)
            return 0
        elif a.startswith("--"):
            print("bench-gate: unknown option %s" % a, file=sys.stderr)
            return 2
        else:
            paths.append(a)
        i += 1
    if len(paths) != 2 or not 0 < min_ratio <= 1:
        print("usage: bench-gate.py BASELINE.txt CURRENT.txt [--min-ratio 0.5] [--prefix P ...]", file=sys.stderr)
        return 2
    try:
        cur = bc.Report(paths[1])
    except OSError as e:
        print("bench-gate: cannot read %s: %s" % (paths[1], e), file=sys.stderr)
        return 2
    if not cur.bench:
        print("bench-gate: %s has no BENCH lines" % paths[1], file=sys.stderr)
        return 2
    if not os.path.exists(paths[0]):
        print("bench-gate: no baseline at %s yet: nothing to compare (keep %s as the first one)" % (paths[0], paths[1]))
        return 0
    base = bc.Report(paths[0])
    if not base.bench:
        print("bench-gate: the baseline %s has no BENCH lines" % paths[0])
        return 1
    for key in ("format", "workload"):
        if base.head.get(key) != cur.head.get(key):
            print("bench-gate: STALE BASELINE: `%s` is %s in the baseline and %s now; regenerate the baseline" % (key, base.head.get(key), cur.head.get(key)))
            return 1
    if not cur.complete:
        print("bench-gate: %s has no `end` line: the benchmark was cut short" % paths[1])
        return 1
    failures, checked = gate(base, cur, min_ratio, prefixes or DEFAULT_PREFIXES)
    if checked == 0:
        print("bench-gate: nothing comparable between the two reports")
        return 1
    for k, q in sorted(failures, key=lambda x: x[1]):
        print("bench-gate: REGRESSION %s is %.2fx of the baseline (limit %.2fx)" % (k, q, min_ratio))
    if failures:
        return 1
    print("bench-gate: %d benchmarks within %.2fx of the baseline (%s)" % (checked, min_ratio, base.label()))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
