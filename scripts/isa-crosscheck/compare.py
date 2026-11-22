import re, collections
A = [l.rstrip("\n").split("\t") for l in open("isa_out.tsv")]
B = [l.rstrip("\n").split("\t") for l in open("fm1_out.tsv")]
def shape(s):
    s = s.lower()
    s = re.sub(r"\{[^}]*\}", "{L}", s)
    s = re.sub(r"\br\d+\b|\bsp\b|\br\b", "R", s)
    s = re.sub(r"#[a-z]\b|0x[0-9a-f]+|\b\d+\b", "N", s)
    s = re.sub(r"\s+", "", s)
    s = re.sub(r"\+-|-(?=N)", "+", s).replace("++", "+")
    return s.replace("(+", "(")

cats = collections.Counter(); only_a = collections.Counter(); only_b = collections.Counter(); lenmis = 0
for a, b in zip(A, B):
    alen, blen = int(a[2]), int(b[1])
    if alen and blen:
        lenmis += alen != blen; cats["both"] += 1
    elif alen:
        cats["only pi32v2-isa" + ("" if a[3] == "1" else " (needs instrev3 or another CPU)")] += 1; only_a[a[1]] += 1
    elif blen:
        cats["only fm1-isa"] += 1; only_b[b[2]] += 1
    else:
        cats["neither"] += 1
print("windows", len(A), dict(cats), "length mismatches among both:", lenmis)
print("only pi32v2-isa:", len(only_a), "distinct table ids; only fm1-isa:", dict(only_b))
diff = collections.Counter(); ex = {}; same = 0
for a, b in zip(A, B):
    aid, alen, atxt = a[1], int(a[2]), a[4]; blen, btxt = int(b[1]), b[2]
    if alen and blen:
        if shape(atxt) == shape(btxt): same += 1
        else:
            k = (aid, btxt); diff[k] += 1; ex.setdefault(k, (a[0], atxt))
tot = same + sum(diff.values())
print(f"both, same length: {tot}; same shape: {same} ({100*same/tot:.1f}%); different: {sum(diff.values())} in {len(diff)} groups")
for (aid, btxt), n in diff.most_common(60):
    print(f"  {n:5d} {aid:22s} {ex[(aid,btxt)][1][:46]:46s} | {btxt[:46]:46s} e.g. {ex[(aid,btxt)][0]}")
