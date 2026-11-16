"""Formula-related statistics over a corpus, for the ADR-0003 decision.

Read-only (zipfile + ElementTree). `python3 -I spikes/xlsx/formula_stats.py DIR...`

For every readable, non-Strict package: does any worksheet contain an SML
<f> element; calcChain presence; calcPr attributes; shared/array formulas;
and which formula texts would defeat a purely syntactic "which cells does
this formula read" analysis (volatile reference functions, defined names,
structured references, external references, 3-D references).
"""
import io
import json
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path

SML = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
PKGREL = "http://schemas.openxmlformats.org/package/2006/relationships"
F = f"{{{SML}}}f"

VOLATILE_REF = re.compile(r"\b(INDIRECT|OFFSET)\s*\(", re.I)
EXTERNAL = re.compile(r"\[\d+\]")
STRUCTURED = re.compile(r"[A-Za-z_][\w.]*\[(#|\[|[^\]\d])")
SHEET_REF = re.compile(r"(?:'[^']+'|[A-Za-z_][\w.]*)!")
CELL = re.compile(r"\$?[A-Z]{1,3}\$?\d+")


def rels(z, part):
    d, n = posixpath.split(part)
    rp = posixpath.join(d, "_rels", n + ".rels")
    if rp not in z.namelist():
        return {}
    out = {}
    for e in ET.fromstring(z.read(rp)).iter(f"{{{PKGREL}}}Relationship"):
        t = e.get("Target", "")
        if e.get("TargetMode") != "External":
            t = t[1:] if t.startswith("/") else posixpath.normpath(posixpath.join(d, t))
        out[e.get("Id")] = (e.get("Type", "").rsplit("/", 1)[-1], t)
    return out


def one(p):
    try:
        return _one(p)
    except Exception:  # noqa: BLE001 - corrupt/fuzzed inputs are skipped
        return None


def _one(p):
    try:
        z = zipfile.ZipFile(p)
        root = rels(z, "")
        wb = next(t for ty, t in root.values() if ty == "officeDocument")
        wbx = ET.fromstring(z.read(wb))
    except Exception:  # noqa: BLE001 - untrusted input
        return None
    if not wbx.tag.startswith(f"{{{SML}}}"):
        return None
    r = {"file": p.name, "calcChain": any(n.endswith("calcChain.xml") for n in z.namelist())}
    cp = wbx.find(f"{{{SML}}}calcPr")
    r["calcPr"] = dict(cp.attrib) if cp is not None else None
    names = [d.get("name") for d in wbx.iter(f"{{{SML}}}definedName")]
    wr = rels(z, wb)
    c = Counter()
    for _, (ty, t) in wr.items():
        if ty != "worksheet" or t not in z.namelist():
            continue
        for _, el in ET.iterparse(io.BytesIO(z.read(t))):
            if el.tag != F:
                continue
            c["f"] += 1
            kind = el.get("t", "normal")
            c["t_" + kind] += 1
            txt = el.text or ""
            if VOLATILE_REF.search(txt):
                c["volatile_ref"] += 1
            if EXTERNAL.search(txt):
                c["external"] += 1
            if STRUCTURED.search(txt):
                c["structured"] += 1
            if SHEET_REF.search(txt):
                c["cross_sheet"] += 1
            if any(re.search(rf"(?<![\w.]){re.escape(n)}(?![\w(])", txt) for n in names if n and not n.startswith("_xlnm")):
                c["defined_name"] += 1
            if "_xlfn." in txt:
                c["xlfn"] += 1
    r["counts"] = dict(c)
    return r


def main(dirs):
    res = [x for d in dirs for p in sorted(Path(d).iterdir())
           if p.suffix.lower() in (".xlsx", ".xlsm") and (x := one(p))]
    for x in res:
        print(json.dumps(x))
    n = len(res)
    withf = [x for x in res if x["counts"].get("f")]
    agg = Counter()
    for x in withf:
        for k, v in x["counts"].items():
            if v:
                agg[k] += 1
    print(json.dumps({
        "packages": n,
        "with_formulas": len(withf),
        "with_formulas_and_calcChain": sum(1 for x in withf if x["calcChain"]),
        "with_formulas_no_calcChain": sum(1 for x in withf if not x["calcChain"]),
        "calcChain_without_formulas": sum(1 for x in res if x["calcChain"] and not x["counts"].get("f")),
        "fullCalcOnLoad": sum(1 for x in res if x["calcPr"] and x["calcPr"].get("fullCalcOnLoad") in ("1", "true")),
        "files_with_feature": dict(agg),
    }, indent=1))


if __name__ == "__main__":
    main(sys.argv[1:])
