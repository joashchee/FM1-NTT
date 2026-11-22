import sys
sys.path.insert(0, "/isa")
from pi32v2isa import table, codec
t = table.load("/isa/isa/pi32v2.toml")
ins = t.insns
# index by first word: candidates whose first word matches
def first_ok(i, w):
    n = len(i.words)
    sh = 16 * (n - 1)
    return ((w << sh) & i.mask) == (i.value & (0xFFFF << sh))
cache = {}
out = open("/w/isa_out.tsv", "w")
for line in open("/w/windows.txt"):
    h = line.strip()
    data = bytes.fromhex(h)
    w = data[0] | (data[1] << 8)
    cands = cache.get(w)
    if cands is None:
        cache = {w: [i for i in ins if first_ok(i, w)]}
        cands = cache[w]
    got = None
    for i in cands:
        v = codec._words(data, 0, len(i.words))
        if v is not None and v & i.mask == i.value:
            got = i
            break
    if got is None:
        out.write(f"{h}\t-\t0\t-\t-\n")
    else:
        vals = codec.fields_of(got, data, 0)
        try:
            txt = codec.format(got, vals)
        except Exception as e:
            txt = "FORMATERR"
        base = table.shown_by_objdump(got, "r3", ())
        out.write(f"{h}\t{got.id}\t{2*len(got.words)}\t{int(base)}\t{txt}\n")
