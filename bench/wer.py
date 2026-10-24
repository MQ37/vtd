import json, re, sys, unicodedata
def norm(s, strip=False):
    s = unicodedata.normalize("NFC", s.lower()).replace("’", "'")
    if strip: s = "".join(c for c in unicodedata.normalize("NFD", s) if unicodedata.category(c) != "Mn")
    return re.sub(r"[^\w\s']", " ", s).split()
def ed(a, b):
    d = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        p, d[0] = d[0], i
        for j, y in enumerate(b, 1):
            p, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, p + (x != y))
    return d[-1]
ref = open(sys.argv[1]).read()
rows = []
for f in sys.argv[2:]:
    for line in open(f):
        r = json.loads(line)
        w = ed(norm(ref), norm(r["text"])) / len(norm(ref)) * 100
        w2 = ed(norm(ref, True), norm(r["text"], True)) / len(norm(ref, True)) * 100
        rows.append((w, w2, r))
for w, w2, r in sorted(rows, key=lambda x: x[0]):
    print(f"{w:5.1f}% {w2:5.1f}%  {r['model']:24s} {r['variant']:10s} {r['seconds']:6.1f}s")
