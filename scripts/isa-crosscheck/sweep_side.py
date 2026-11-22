# Run in the python:3.12-slim container (no network) with pi32v2-isa at /isa, the HIT lines of sweep.tsv as /w/hits.tsv; writes /w/hits_out.tsv.
import sys
sys.path.insert(0, "/isa")
from pi32v2isa import table, codec
t = table.load("/isa/isa/pi32v2.toml")
out = open("/w/hits_out.tsv", "w")
for line in open("/w/hits.tsv"):
    _, fw, at, h = line.rstrip("\n").split("\t")
    data = bytes.fromhex(h)
    got = codec.decode(t, data, 0)
    if got is None:
        out.write(f"{fw}\t{at}\t{h}\t-\t0\t-\n")
    else:
        insn, vals, n = got
        out.write(f"{fw}\t{at}\t{h}\t{insn.id}\t{n}\t{int(table.shown_by_objdump(insn, 'r3', ()))}\n")
