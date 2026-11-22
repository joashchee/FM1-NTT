import random
r = random.Random(20261122)
out = []
for w in range(0x10000):
    if w < 0xE000:
        pairs = [(0, 0), (0xFFFF, 0x5A5A)]
    else:
        pairs = [(0, 0), (0xFFFF, 0xFFFF)] + [(r.getrandbits(16), r.getrandbits(16)) for _ in range(30)]
    for a, b in pairs:
        out.append("".join(x.to_bytes(2, "little").hex() for x in (w, a, b)))
open("windows.txt", "w").write("\n".join(out) + "\n")
print(len(out), "windows")
