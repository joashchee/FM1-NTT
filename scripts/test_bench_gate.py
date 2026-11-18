#!/usr/bin/env python3
"""Tests for bench-gate.py. Run: python3 scripts/test_bench_gate.py"""
import importlib.util
import io
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("bench_gate", os.path.join(HERE, "bench-gate.py"))
bg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bg)


def report(ips=30.0, boot=1.0, snd=0.5, workload=3, fmt=1, end=True):
    return """# fm1ntt benchmark
format: %d
workload: %d
platform: Linux-x86_64
cpu: Test
== results ==
BENCH core.nop.interp: %s Minsn/s better=higher min=1 max=9 n=3
BENCH core.boot: %s ms better=lower min=1 max=9 n=3
BENCH screen.draw.standard: 0.2 ms/frame better=lower min=1 max=9 n=3
BENCH sound.fm6.voices1.load: %s pct-of-real-time better=lower min=1 max=9 n=3
%s""" % (fmt, workload, ips, boot, snd, "end\n" if end else "")


class Gate(unittest.TestCase):
    def run_gate(self, base, cur, *extra):
        with tempfile.TemporaryDirectory() as d:
            paths = []
            for name, text in (("base.txt", base), ("cur.txt", cur)):
                p = os.path.join(d, name)
                if text is not None:
                    with open(p, "w") as f:
                        f.write(text)
                paths.append(p)
            out, err = io.StringIO(), io.StringIO()
            with redirect_stdout(out), redirect_stderr(err):
                rc = bg.main(paths + list(extra))
            return rc, out.getvalue() + err.getvalue()

    def test_same_passes(self):
        rc, out = self.run_gate(report(), report())
        self.assertEqual(rc, 0, out)
        self.assertIn("3 benchmarks", out)  # core.nop.interp, core.boot, screen.draw.standard (sound. is not gated by default)

    def test_a_halving_fails_in_both_directions(self):
        rc, out = self.run_gate(report(), report(ips=10.0))
        self.assertEqual(rc, 1)
        self.assertIn("REGRESSION core.nop.interp", out)
        rc, out = self.run_gate(report(), report(boot=3.0))        # a lower-is-better one three times slower
        self.assertEqual(rc, 1)
        self.assertIn("REGRESSION core.boot", out)

    def test_a_small_drop_and_an_improvement_pass(self):
        self.assertEqual(self.run_gate(report(), report(ips=20.0, boot=1.5))[0], 0)
        self.assertEqual(self.run_gate(report(), report(ips=90.0, boot=0.1))[0], 0)

    def test_the_limit_and_prefixes_are_options(self):
        self.assertEqual(self.run_gate(report(), report(ips=20.0), "--min-ratio", "0.9")[0], 1)
        rc, out = self.run_gate(report(), report(snd=5.0), "--prefix", "sound.")
        self.assertEqual((rc, "sound.fm6.voices1.load" in out), (1, True))
        self.assertEqual(self.run_gate(report(), report(snd=5.0))[0], 0, "sound. is not in the default set")

    def test_no_baseline_passes_and_says_so(self):
        rc, out = self.run_gate(None, report())
        self.assertEqual(rc, 0)
        self.assertIn("no baseline", out)

    def test_stale_and_broken_reports_fail(self):
        self.assertEqual(self.run_gate(report(), report(workload=4))[0], 1)
        self.assertEqual(self.run_gate(report(), report(fmt=2))[0], 1)
        self.assertEqual(self.run_gate(report(), report(end=False))[0], 1, "a cut-short run is not a pass")
        self.assertEqual(self.run_gate(report(), "nothing here\n")[0], 2)
        self.assertEqual(self.run_gate(report(), report(), "--min-ratio", "x")[0], 2)
        self.assertEqual(self.run_gate(report(), report(), "--min-ratio", "1.5")[0], 2)
        self.assertEqual(self.run_gate(report(), report(), "--nope")[0], 2)


if __name__ == "__main__":
    unittest.main()
