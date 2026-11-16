#!/usr/bin/env python3
"""Compare FM1-NTT benchmark reports from different platforms or builds (Python 3 standard library only).

    scripts/bench-compare.py bench-A.txt bench-B.txt [bench-C.txt ...] [--baseline N] [--markdown] [--force]

Each file is what `fm1ntt --bench` (or TOOLS > Benchmark) wrote: a header (`format`, `workload`, `platform`, `cpu`, ...) and one
`BENCH id: median unit better=higher|lower min=.. max=.. n=..` line per measurement. The table has one row per benchmark and one
column per file; after the first number of each other column comes its ratio to the baseline column (the first file unless
--baseline), written so that **above 1.00x is always better than the baseline**, whether the benchmark is higher-is-better
(instructions a second) or lower-is-better (milliseconds). The last row is the geometric mean of those ratios.

Reports are only comparable when their `format` and `workload` numbers match: a mismatch is refused (--force compares anyway and
says so). A benchmark missing from a file shows `-`. Exit status: 0 compared, 1 a file could not be read or has no results,
2 usage or nothing comparable.
"""
import math
import sys

HEADER_KEYS = ("format", "workload", "app", "made", "platform", "os", "cpu", "cores", "max_khz", "ram_mb", "native_jit", "seconds_per_run", "runs")


class Report:
    def __init__(self, path):
        self.path = path
        self.head = {}
        self.bench = {}      # id -> dict(value, unit, better, min, max, n)
        self.firmware = ""
        self.complete = False
        self._read()

    def _read(self):
        with open(self.path, encoding="utf-8", errors="replace") as f:
            lines = f.read().splitlines()
        for line in lines:
            if line == "end":
                self.complete = True
            elif line.startswith("# firmware:"):
                self.firmware = line[len("# firmware:"):].strip()
            elif line.startswith("BENCH "):
                body = line[len("BENCH "):]
                key, _, rest = body.partition(": ")
                parts = rest.split()
                if len(parts) < 2:
                    continue
                try:
                    value = float(parts[0])
                except ValueError:
                    continue
                item = {"value": value, "unit": parts[1], "better": "higher", "min": value, "max": value, "n": 1}
                for p in parts[2:]:
                    k, _, v = p.partition("=")
                    if k == "better" and v in ("higher", "lower"):
                        item["better"] = v
                    elif k in ("min", "max"):
                        try:
                            item[k] = float(v)
                        except ValueError:
                            pass
                    elif k == "n" and v.isdigit():
                        item["n"] = int(v)
                self.bench[key] = item
            elif ": " in line and not line.startswith(("#", "==", "SKIP")):
                k, _, v = line.partition(": ")
                if k in HEADER_KEYS:
                    self.head[k] = v

    def label(self):
        cpu = self.head.get("cpu", "?")
        return "%s | %s | %s" % (self.head.get("platform", "?"), cpu[:28], self.head.get("made", "?")[:10])


def ratio(base, other):
    """other against base, above 1 meaning other is better; None when it cannot be said."""
    if base["unit"] != other["unit"] or base["better"] != other["better"]:
        return None
    b, o = base["value"], other["value"]
    if b <= 0 or o <= 0:
        return None
    return o / b if base["better"] == "higher" else b / o


def fmt(v):
    return "%.4g" % v


def compare(reports, baseline=0, markdown=False):
    base = reports[baseline]
    ids = []
    for r in reports:
        for k in r.bench:
            if k not in ids:
                ids.append(k)
    rows = []
    products = [[] for _ in reports]
    for k in ids:
        row = [k, next((r.bench[k]["unit"] for r in reports if k in r.bench), "")]
        for i, r in enumerate(reports):
            item = r.bench.get(k)
            if item is None:
                row.append("-")
            elif i == baseline or k not in base.bench:
                row.append(fmt(item["value"]))
            else:
                q = ratio(base.bench[k], item)
                row.append("%s (%s)" % (fmt(item["value"]), "n/a" if q is None else "%.2fx" % q))
                if q is not None:
                    products[i].append(q)
        rows.append(row)
    geo = ["geometric mean", "ratio"]
    for i in range(len(reports)):
        geo.append("baseline" if i == baseline else ("-" if not products[i] else "%.2fx" % math.exp(sum(math.log(q) for q in products[i]) / len(products[i]))))
    rows.append(geo)
    head = ["benchmark", "unit"] + ["%s%s" % (r.label(), "  [baseline]" if i == baseline else "") for i, r in enumerate(reports)]
    out = []
    if markdown:
        out.append("| " + " | ".join(head) + " |")
        out.append("|" + "---|" * len(head))
        out += ["| " + " | ".join(r) + " |" for r in rows]
    else:
        widths = [max(len(str(x)) for x in col) for col in zip(head, *rows)]
        out.append("  ".join(str(h).ljust(w) for h, w in zip(head, widths)))
        out.append("  ".join("-" * w for w in widths))
        out += ["  ".join(str(c).ljust(w) for c, w in zip(r, widths)) for r in rows]
    return "\n".join(out)


def main(argv):
    paths, baseline, markdown, force = [], 0, False, False
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--baseline" and i + 1 < len(argv):
            baseline = int(argv[i + 1]) - 1
            i += 1
        elif a == "--markdown":
            markdown = True
        elif a == "--force":
            force = True
        elif a in ("-h", "--help"):
            print(__doc__)
            return 0
        elif a.startswith("--"):
            print("bench-compare: unknown option %s" % a, file=sys.stderr)
            return 2
        else:
            paths.append(a)
        i += 1
    if len(paths) < 2 or not 0 <= baseline < len(paths):
        print("usage: bench-compare.py A.txt B.txt [...] [--baseline N] [--markdown] [--force]", file=sys.stderr)
        return 2
    reports = []
    for p in paths:
        try:
            r = Report(p)
        except OSError as e:
            print("bench-compare: cannot read %s: %s" % (p, e), file=sys.stderr)
            return 1
        if not r.bench:
            print("bench-compare: %s has no BENCH lines" % p, file=sys.stderr)
            return 1
        reports.append(r)
    notes = []
    for r in reports:
        if not r.complete:
            notes.append("%s has no `end` line: it was cut short (a crash?), so some benchmarks are missing" % r.path)
    for key in ("format", "workload"):
        vals = {r.head.get(key) for r in reports}
        if len(vals) > 1:
            msg = "the reports differ in `%s` (%s)" % (key, ", ".join(sorted(str(v) for v in vals)))
            if not force:
                print("bench-compare: %s: they are not comparable (--force compares anyway)" % msg, file=sys.stderr)
                return 2
            notes.append(msg + ": compared anyway (--force)")
    fws = {r.firmware.split(" fnv64=")[-1] if r.firmware else "" for r in reports}
    if len(fws - {""}) > 1:
        notes.append("the firmware differs between the reports: the fw.* rows are not the same program")
    print(compare(reports, baseline, markdown))
    for n in notes:
        print("note: " + n, file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
