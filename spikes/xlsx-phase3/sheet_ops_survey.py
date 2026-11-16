"""Facts behind ADR-0009 (xlsx Phase3: sheet operations on existing files).

Read-only (zipfile + ElementTree). Untrusted input: parts with a DTD are
skipped, nothing is extracted or executed.

    python3 -I spikes/xlsx-phase3/sheet_ops_survey.py DIR...           # one JSON line per package
    python3 -I spikes/xlsx-phase3/sheet_ops_survey.py --summary DIR... # aggregate table

Per package:
- sheets: count, kinds (worksheet/chartsheet/dialogsheet/macrosheet), states;
- sheetId: max vs count (gaps), order, duplicates; part file number vs sheetId;
- workbook relationship ids: all `rIdN`? contiguous? non-standard ids;
- content types: Override per sheet part or Default;
- docProps/app.xml: HeadingPairs/TitlesOfParts presence, headings, and whether
  they agree with workbook.xml (counts, names, order);
- definedNames: count, sheet-local (`localSheetId`, a *position*), hidden,
  `_xlnm.*`, out-of-range `localSheetId`, `#REF!`;
- bookViews `activeTab`/`firstSheet`; `codeName`s; VBA project and signature;
- **rename impact**: for every sheet, which parts refer to it by name
  (cell formulas, 3-D references, defined names, chart `c:f`, pivot cache
  `worksheetSource/@sheet`, data validation and conditional formatting
  formulas incl. x14 `xm:f`, hyperlink `@location`, drawing/external `#Sheet!`
  targets, sparklines), plus string literals that name a sheet (e.g. inside
  `INDIRECT`) that no syntactic rewrite can see;
- **delete impact**: the relationship types of each sheet's own `.rels`
  (what a cascading delete must handle) and pivot caches sourced from it.

References are found with a regex over the XML text (sheet-name prefixes
`'…'!`, `Name!`, `A:B!`), so the counts are an approximation: good enough
for "how often", not a tokenizer.
"""
import json
import os
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict

SML = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
PKGREL = "http://schemas.openxmlformats.org/package/2006/relationships"
CT_NS = "http://schemas.openxmlformats.org/package/2006/content-types"
EP = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"
VT = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
C = "http://schemas.openxmlformats.org/drawingml/2006/chart"
STRICT = "http://purl.oclc.org/ooxml/"
SHEET_KINDS = {"worksheet": "worksheet", "chartsheet": "chartsheet", "dialogsheet": "dialogsheet",
               "xlMacrosheet": "macrosheet", "xlIntlMacrosheet": "macrosheet"}


def q(ns, local):
    return f"{{{ns}}}{local}"


def no_dtd(b):
    return b"<!DOCTYPE" not in b[:4096] and b"<!ENTITY" not in b


def parse(z, name):
    b = z.read(name)
    if not no_dtd(b):
        raise ValueError("DTD")
    return ET.fromstring(b)


def rels(z, part):
    d, n = posixpath.split(part)
    rp = posixpath.join(d, "_rels", n + ".rels")
    if rp not in z.namelist():
        return []
    out = []
    for e in parse(z, rp).iter(q(PKGREL, "Relationship")):
        t = e.get("Target", "")
        ext = e.get("TargetMode") == "External"
        if not ext:
            t = t[1:] if t.startswith("/") else posixpath.normpath(posixpath.join(d, t))
        out.append((e.get("Id"), e.get("Type", "").rsplit("/", 1)[-1], t, ext))
    return out


# sheet-name prefixes of references: 'quoted ''name'''! | Name! | A:B! | 'A:B'!
QUOTED = re.compile(r"'((?:[^']|'')+)'!")
PLAIN = re.compile(r"(?<![\w.'\]])([^\W\d][\w.]*(?::[^\W\d][\w.]*)?)!")
STRLIT = re.compile(r'"((?:[^"]|"")*)"')


def sheet_refs(text, names_ci):
    """Set of (sheet name, is_3d) referenced by a formula-like text."""
    out = set()
    if not text or "!" not in text:
        return out
    # drop string literals first; they are counted separately
    stripped = STRLIT.sub('""', text)
    for m in QUOTED.finditer(stripped):
        nm = m.group(1).replace("''", "'")
        if nm.startswith("["):  # external [n]Sheet
            continue
        parts = nm.split(":") if ":" in nm and nm.split(":")[0].lower() in names_ci else [nm]
        for p in parts:
            if p.lower() in names_ci:
                out.add((names_ci[p.lower()], len(parts) > 1))
    for m in PLAIN.finditer(stripped):
        nm = m.group(1)
        parts = nm.split(":")
        for p in parts:
            if p.lower() in names_ci:
                out.add((names_ci[p.lower()], len(parts) > 1))
    return out


def literal_mentions(text, names_ci):
    """Sheet names appearing inside string literals ("Sheet1!A1" for INDIRECT etc.)."""
    out = set()
    for m in STRLIT.finditer(text or ""):
        s = m.group(1)
        for k, v in names_ci.items():
            if k + "!" in s.lower() or "'" + k + "'!" in s.lower():
                out.add(v)
    return out


def one(path):
    try:
        z = zipfile.ZipFile(path)
        root = rels(z, "")
        wb = next(t for _, ty, t, _ in root if ty == "officeDocument")
        wbx = parse(z, wb)
    except Exception:  # noqa: BLE001 - untrusted input
        return None
    if wbx.tag.startswith("{" + STRICT):
        return {"file": path, "strict": True}
    out = {"file": path}
    names = z.namelist()
    wrels = rels(z, wb)
    by_id = {i: (ty, t, ext) for i, ty, t, ext in wrels}
    ids = [i for i, _, _, _ in wrels]
    nums = [int(i[3:]) for i in ids if i and re.fullmatch(r"rId\d+", i)]
    out["rels"] = {"n": len(ids), "all_rIdN": len(nums) == len(ids),
                   "contiguous": sorted(nums) == list(range(1, len(nums) + 1)) if nums else None,
                   "max": max(nums, default=0)}
    sheets = []
    for s in wbx.iter(q(SML, "sheet")):
        rid = s.get(q(R, "id"))
        ty, target, _ = by_id.get(rid, ("?", None, False))
        sheets.append({"name": s.get("name"), "sheetId": s.get("sheetId"), "state": s.get("state", "visible"),
                       "kind": SHEET_KINDS.get(ty, ty), "part": target, "rid": rid})
    out["n_sheets"] = len(sheets)
    out["kinds"] = dict(Counter(s["kind"] for s in sheets))
    out["states"] = dict(Counter(s["state"] for s in sheets))
    sids = [int(s["sheetId"]) for s in sheets if (s["sheetId"] or "").isdigit()]
    out["sheetId"] = {"max": max(sids, default=0), "gaps": max(sids, default=0) != len(sids),
                      "dups": len(sids) != len(set(sids)), "ascending": sids == sorted(sids)}
    # part file number vs sheetId
    num_eq = 0
    for s in sheets:
        m = re.search(r"(\d+)\.xml$", s["part"] or "")
        if m and m.group(1) == s["sheetId"]:
            num_eq += 1
    out["partnum_eq_sheetId"] = num_eq
    # content types
    try:
        ct = parse(z, "[Content_Types].xml")
        overrides = {e.get("PartName", "").lstrip("/").lower() for e in ct.iter(q(CT_NS, "Override"))}
        out["sheet_parts_with_override"] = sum(1 for s in sheets if (s["part"] or "").lower() in overrides)
    except Exception:  # noqa: BLE001
        pass
    # app.xml
    app = next((t for _, ty, t, _ in root if ty == "extended-properties"), None)
    a = {"present": bool(app and app in names)}
    if a["present"]:
        try:
            ax = parse(z, app)
            hp = ax.find(q(EP, "HeadingPairs"))
            tp = ax.find(q(EP, "TitlesOfParts"))
            a["HeadingPairs"] = hp is not None
            a["TitlesOfParts"] = tp is not None
            app_name = ax.find(q(EP, "Application"))
            a["application"] = (app_name.text or "").split()[0] if app_name is not None and app_name.text else None
            if hp is not None and tp is not None:
                vs = list(hp.iter(q(VT, "variant")))
                heads = []
                for i in range(0, len(vs) - 1, 2):
                    l = vs[i].find(q(VT, "lpstr"))
                    n = vs[i + 1].find(q(VT, "i4"))
                    heads.append(((l.text if l is not None else None), int(n.text) if n is not None and n.text else None))
                titles = [e.text or "" for e in tp.iter(q(VT, "lpstr"))]
                a["headings"] = [h for h, _ in heads]
                a["sum_matches_titles"] = sum(n or 0 for _, n in heads) == len(titles)
                # Excel groups titles by sheet kind (worksheets first) and leaves
                # veryHidden sheets out
                ws_names = [s["name"] for s in sheets if s["kind"] == "worksheet" and s["state"] != "veryHidden"]
                # the first heading's block should be the worksheet names in order
                first = heads[0][1] if heads else 0
                a["first_block_is_sheets_in_order"] = titles[:first or 0] == ws_names[:first or 0] and first == len(ws_names)
                a["mentions_all_sheet_names"] = all(s["name"] in titles for s in sheets if s["state"] != "veryHidden")
        except Exception as e:  # noqa: BLE001
            a["error"] = str(e)[:60]
    out["app"] = a
    # defined names
    dn = list(wbx.iter(q(SML, "definedName")))
    local = [d for d in dn if d.get("localSheetId") is not None]
    out["names"] = {"n": len(dn), "local": len(local), "hidden": sum(1 for d in dn if d.get("hidden") in ("1", "true")),
                    "builtin": sum(1 for d in dn if (d.get("name") or "").startswith("_xlnm.")),
                    "local_out_of_range": sum(1 for d in local if not (d.get("localSheetId") or "").isdigit()
                                              or int(d.get("localSheetId")) >= len(sheets)),
                    "ref_error": sum(1 for d in dn if "#REF!" in (d.text or ""))}
    bv = wbx.find(f"{q(SML, 'bookViews')}/{q(SML, 'workbookView')}")
    out["activeTab"] = int(bv.get("activeTab", "0")) if bv is not None and (bv.get("activeTab") or "0").isdigit() else 0
    out["firstSheet"] = int(bv.get("firstSheet", "0")) if bv is not None and (bv.get("firstSheet") or "0").isdigit() else 0
    wbpr = wbx.find(q(SML, "workbookPr"))
    out["wb_codeName"] = wbpr is not None and wbpr.get("codeName") is not None
    out["vba"] = any(ty == "vbaProject" for _, ty, _, _ in wrels)
    out["vba_signature"] = any(n.lower().endswith("vbaprojectsignature.bin") for n in names)
    out["calcChain"] = any(ty == "calcChain" for _, ty, _, _ in wrels)
    out["externalLinks"] = sum(1 for _, ty, _, _ in wrels if ty == "externalLink")

    # ---- reference census
    names_ci = {s["name"].lower(): s["name"] for s in sheets if s["name"]}
    touched = defaultdict(set)   # sheet -> categories referring to it from elsewhere
    cats_any = Counter()
    three_d = 0
    literal = set()
    def note(cat, text, own=None):
        nonlocal three_d
        for nm, is3d in sheet_refs(text, names_ci):
            if is3d:
                three_d += 1
            touched[nm].add(cat)
            cats_any[cat] += 1
        literal.update(literal_mentions(text, names_ci))
    for d in dn:
        note("definedName", d.text)
    codenames = 0
    own_rels = Counter()
    for s in sheets:
        p = s["part"]
        if not p or p not in names:
            continue
        try:
            x = parse(z, p)
        except Exception:  # noqa: BLE001
            continue
        pr = x.find(q(SML, "sheetPr"))
        codenames += pr is not None and pr.get("codeName") is not None
        for f in x.iter(q(SML, "f")):
            note("cell formula", f.text)
        for dv in x.iter(q(SML, "dataValidation")):
            for c in dv:
                note("data validation", c.text)
        for cf in x.iter(q(SML, "cfRule")):
            for c in cf.iter(q(SML, "formula")):
                note("conditional formatting", c.text)
        for h in x.iter(q(SML, "hyperlink")):
            loc = h.get("location")
            if loc:
                note("hyperlink @location", loc if "!" in loc else loc + "!")
        raw = z.read(p).decode("utf-8", "replace")
        for m in re.finditer(r"<xm:f>(.*?)</xm:f>", raw):
            note("x14 ext (xm:f: DV/CF/sparklines)", m.group(1))
        for rid, ty, t, ext in rels(z, p):
            own_rels[ty] += 1
            if ext and t.startswith("#"):
                note("hyperlink #target", t[1:])
            if ty == "drawing" and t in names:
                for _, ty2, t2, ext2 in rels(z, t):
                    if ext2 and t2.startswith("#"):
                        note("drawing hyperlink #target", t2[1:])
    out["codeName_sheets"] = codenames
    out["own_rel_types"] = dict(own_rels)
    for n in names:
        if re.match(r"xl/charts/chart\d*\.xml$", n, re.I) or re.match(r"xl/chartsheets/.*\.xml$", n, re.I):
            try:
                cx = parse(z, n)
            except Exception:  # noqa: BLE001
                continue
            for f in cx.iter(q(C, "f")):
                note("chart c:f", f.text)
        elif re.match(r"xl/pivotCache/pivotCacheDefinition\d*\.xml$", n, re.I):
            try:
                px = parse(z, n)
            except Exception:  # noqa: BLE001
                continue
            for ws in px.iter(q(SML, "worksheetSource")):
                if ws.get("sheet"):
                    note("pivot cache worksheetSource", "'" + ws.get("sheet").replace("'", "''") + "'!A1")
    out["refs_by_category"] = dict(cats_any)
    out["sheets_referenced_by_name"] = {k: sorted(v) for k, v in touched.items()}
    out["three_d_refs"] = three_d
    out["literal_mentions"] = len(literal)
    return out


def files(dirs):
    for d in dirs:
        for rt, _, fs in os.walk(d):
            for f in sorted(fs):
                if f.endswith((".xlsx", ".xlsm")):
                    yield os.path.join(rt, f)


def summary(rows):
    rows = [r for r in rows if r and not r.get("strict")]
    n = len(rows)
    pct = lambda a, b: f"{a}/{b} ({100 * a / b:.0f}%)" if b else f"{a}/0"
    out = [("packages (Transitional, readable)", n)]
    ns = sorted(r["n_sheets"] for r in rows)
    out.append(("sheets per workbook: median / p90 / max", f"{ns[n // 2]} / {ns[int(n * .9)]} / {ns[-1]}"))
    kinds = Counter()
    for r in rows:
        kinds.update({k: 1 for k in r["kinds"]})
    out.append(("packages having a sheet of kind", dict(kinds)))
    out.append(("packages with hidden / veryHidden sheets", f"{sum(1 for r in rows if r['states'].get('hidden'))} / {sum(1 for r in rows if r['states'].get('veryHidden'))}"))
    out.append(("sheetId gaps (max != count)", pct(sum(1 for r in rows if r["sheetId"]["gaps"]), n)))
    out.append(("sheetId not ascending in workbook order", sum(1 for r in rows if not r["sheetId"]["ascending"])))
    out.append(("sheetId duplicates", sum(1 for r in rows if r["sheetId"]["dups"])))
    out.append(("every sheet part number == its sheetId", pct(sum(1 for r in rows if r["partnum_eq_sheetId"] == r["n_sheets"]), n)))
    out.append(("workbook rel ids all rIdN", pct(sum(1 for r in rows if r["rels"]["all_rIdN"]), n)))
    out.append(("workbook rel ids rId1..rIdN contiguous", pct(sum(1 for r in rows if r["rels"]["contiguous"]), n)))
    out.append(("every sheet part has a CT Override", pct(sum(1 for r in rows if r.get("sheet_parts_with_override") == r["n_sheets"]), n)))
    apps = [r["app"] for r in rows]
    out.append(("app.xml present", pct(sum(1 for a in apps if a["present"]), n)))
    both = [a for a in apps if a.get("HeadingPairs") and a.get("TitlesOfParts")]
    out.append(("app.xml with HeadingPairs + TitlesOfParts", pct(len(both), n)))
    out.append(("  ... heading counts sum to TitlesOfParts", pct(sum(1 for a in both if a.get("sum_matches_titles")), len(both))))
    out.append(("  ... first block = visible+hidden worksheet names in workbook order", pct(sum(1 for a in both if a.get("first_block_is_sheets_in_order")), len(both))))
    out.append(("  ... TitlesOfParts mentions every non-veryHidden sheet", pct(sum(1 for a in both if a.get("mentions_all_sheet_names")), len(both))))
    heads = Counter(h for a in both for h in a.get("headings", []))
    out.append(("  ... heading labels", dict(heads.most_common(10))))
    prods_bad = Counter((a.get("application") or "?") for a in both if not a.get("first_block_is_sheets_in_order"))
    out.append(("  ... producers where first block != sheets", dict(prods_bad.most_common(6))))
    out.append(("app.xml without HeadingPairs/TitlesOfParts: producers", dict(Counter((a.get("application") or "?") for a in apps if a["present"] and not (a.get("HeadingPairs") and a.get("TitlesOfParts"))).most_common(6))))
    out.append(("defined names (packages with any / with local / with _xlnm)", f"{sum(1 for r in rows if r['names']['n'])} / {sum(1 for r in rows if r['names']['local'])} / {sum(1 for r in rows if r['names']['builtin'])}"))
    out.append(("localSheetId out of range", sum(1 for r in rows if r["names"]["local_out_of_range"])))
    out.append(("names containing #REF!", sum(1 for r in rows if r["names"]["ref_error"])))
    out.append(("activeTab > 0 / firstSheet > 0", f"{sum(1 for r in rows if r['activeTab'])} / {sum(1 for r in rows if r['firstSheet'])}"))
    out.append(("VBA project / signed / workbook codeName / sheets with codeName (packages)",
                f"{sum(1 for r in rows if r['vba'])} / {sum(1 for r in rows if r['vba_signature'])} / {sum(1 for r in rows if r['wb_codeName'])} / {sum(1 for r in rows if r['codeName_sheets'])}"))
    out.append(("calcChain / externalLinks (packages)", f"{sum(1 for r in rows if r['calcChain'])} / {sum(1 for r in rows if r['externalLinks'])}"))
    multi = [r for r in rows if r["n_sheets"] > 1]
    out.append(("multi-sheet packages", len(multi)))
    out.append(("  ... where some sheet is referenced by name from anywhere", pct(sum(1 for r in multi if r["sheets_referenced_by_name"]), len(multi))))
    cats = Counter()
    for r in rows:
        cats.update({k: 1 for k in r["refs_by_category"]})
    for k, v in cats.most_common():
        out.append((f"  packages with a by-name sheet reference in: {k}", v))
    out.append(("packages with 3-D references (Sheet1:Sheet3!)", sum(1 for r in rows if r["three_d_refs"])))
    out.append(("packages with sheet names inside string literals (INDIRECT etc.; not rewritable)", sum(1 for r in rows if r["literal_mentions"])))
    rt = Counter()
    for r in rows:
        rt.update({k: 1 for k in r["own_rel_types"]})
    out.append(("worksheet own-.rels types (packages)", dict(rt.most_common(14))))
    for k, v in out:
        print(f"| {k} | {v} |")


def main(argv):
    if argv and argv[0] == "--summary":
        summary([one(p) for p in files(argv[1:])])
        return
    for p in files(argv):
        r = one(p)
        if r is not None:
            print(json.dumps(r, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1:])
