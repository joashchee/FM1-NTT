#!/usr/bin/env python3
"""Tests for bench-compare.py: parsing, the better-is-above-one ratio, refusals. Run: python3 scripts/test_bench_compare.py"""
import importlib.util
import io
import os
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("bench_compare", os.path.join(HERE, "bench-compare.py"))
bc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bc)


def report(platform, ips, boot, workload=1, end=True, fw=""):
    return """# fm1ntt benchmark
format: 1
workload: %d
app: FM1-NTT 0.2.0 (abc 2026-11-16)
made: 2026-11-16 12:00:00
platform: %s
os: x
cpu: Test CPU
cores: 4
== results ==
BENCH core.nop.interp: %s Minsn/s better=higher min=1 max=9 n=3
BENCH core.boot: %s ms better=lower min=1 max=9 n=3
%s%s""" % (workload, platform, ips, boot, fw, "end\n" if end else "")


class Compare(unittest.TestCase):
    def write(self, text):
        f = tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False)
        f.write(text)
        f.close()
        self.addCleanup(os.unlink, f.name)
        return f.name

    def run_main(self, args):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            rc = bc.main(args)
        return rc, out.getvalue(), err.getvalue()

    def test_parse(self):
        r = bc.Report(self.write(report("Darwin-arm64", 33.5, 0.2)))
        self.assertEqual(r.head["platform"], "Darwin-arm64")
        self.assertEqual(r.bench["core.nop.interp"]["value"], 33.5)
        self.assertEqual(r.bench["core.boot"]["better"], "lower")
        self.assertTrue(r.complete)

    def test_ratio_is_above_one_when_better_either_way(self):
        a = bc.Report(self.write(report("A", 10, 4)))
        b = bc.Report(self.write(report("B", 20, 2)))
        self.assertAlmostEqual(bc.ratio(a.bench["core.nop.interp"], b.bench["core.nop.interp"]), 2.0)   # twice the speed
        self.assertAlmostEqual(bc.ratio(a.bench["core.boot"], b.bench["core.boot"]), 2.0)               # half the time
        self.assertAlmostEqual(bc.ratio(b.bench["core.boot"], a.bench["core.boot"]), 0.5)

    def test_table_and_geometric_mean(self):
        rc, out, err = self.run_main([self.write(report("A", 10, 4)), self.write(report("B", 20, 2))])
        self.assertEqual(rc, 0)
        self.assertIn("2.00x", out)
        self.assertIn("geometric mean", out)
        self.assertIn("[baseline]", out)

    def test_baseline_option_and_markdown(self):
        rc, out, _ = self.run_main([self.write(report("A", 10, 4)), self.write(report("B", 20, 2)), "--baseline", "2", "--markdown"])
        self.assertEqual(rc, 0)
        self.assertIn("0.50x", out)
        self.assertTrue(out.startswith("| benchmark"))

    def test_different_workload_is_refused_unless_forced(self):
        a, b = self.write(report("A", 10, 4, workload=1)), self.write(report("B", 20, 2, workload=2))
        rc, _, err = self.run_main([a, b])
        self.assertEqual(rc, 2)
        self.assertIn("not comparable", err)
        rc, out, err = self.run_main([a, b, "--force"])
        self.assertEqual(rc, 0)
        self.assertIn("compared anyway", err)

    def test_a_cut_short_report_is_noted_and_a_missing_benchmark_is_a_dash(self):
        short = report("B", 20, 2, end=False).replace("BENCH core.boot: 2 ms better=lower min=1 max=9 n=3\n", "")
        rc, out, err = self.run_main([self.write(report("A", 10, 4)), self.write(short)])
        self.assertEqual(rc, 0)
        self.assertIn("cut short", err)
        self.assertRegex(out, r"core\.boot\s+ms\s+4\s+-")

    def test_usage_and_unreadable(self):
        self.assertEqual(self.run_main(["only-one.txt"])[0], 2)
        self.assertEqual(self.run_main(["/nonexistent/a.txt", "/nonexistent/b.txt"])[0], 1)
        empty = self.write("# fm1ntt benchmark\nformat: 1\nend\n")
        self.assertEqual(self.run_main([empty, empty])[0], 1)

    def test_different_firmware_is_noted(self):
        a = self.write(report("A", 10, 4, fw="# firmware: a.bin 10 bytes fnv64=1111\n"))
        b = self.write(report("B", 20, 2, fw="# firmware: a.bin 10 bytes fnv64=2222\n"))
        rc, _, err = self.run_main([a, b])
        self.assertEqual(rc, 0)
        self.assertIn("firmware differs", err)


if __name__ == "__main__":
    unittest.main(verbosity=1)
