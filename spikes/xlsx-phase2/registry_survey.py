"""Shared-string and style registry facts over a corpus, for ADR-0006.

Read-only (zipfile + ElementTree). Untrusted input: parts with a DTD are
skipped, nothing is extracted or executed.

    python3 -I spikes/xlsx-phase2/registry_survey.py DIR...          # one JSON line per package
    python3 -I spikes/xlsx-phase2/registry_survey.py --summary DIR... # aggregate table

Per package it reports, for `styles.xml`:
- table sizes and whether each `count` attribute matches the children;
- the mandatory fills (0 = none, 1 = gray125);
- custom number formats: ids below 164, codes equal to a built-in code,
  duplicate codes;
- features a value-typed `Format` (ADR-0006 §3) cannot express, per entry;
- "projection collisions": two existing `cellXfs` entries that a `Format`
  projection maps to the same value although their XML differs (what a
  dedup-against-existing-entries policy would silently merge);
- vendor extensions (`mc:AlternateContent`, `x14ac:knownFonts`, `extLst`).

and for `sharedStrings.xml`:
- `count`/`uniqueCount` against the `si` children and against the actual
  `t="s"` references in every worksheet;
- rich (`r`) and phonetic (`rPh`, `phoneticPr`) items;
- duplicates by flattened text, and whether a duplicate group mixes plain
  and rich/phonetic items (a by-text dedup would pick one silently);
- items whose text has outer whitespace without `xml:space="preserve"`.
"""
import json
import posixpath
import sys
import zipfile
import xml.etree.ElementTree as ET
from collections import Counter, defaultdict

SML = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
PKGREL = "http://schemas.openxmlformats.org/package/2006/relationships"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
X14AC = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/ac"
XML_NS = "http://www.w3.org/XML/1998/namespace"
CT_WS = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"
STRICT = "http://purl.oclc.org/ooxml/"


def q(local):
    return f"{{{SML}}}{local}"


# ECMA-376 Part 1 §18.8.30 built-in number formats (locale-independent ids).
BUILTIN = {
    0: "General", 1: "0", 2: "0.00", 3: "#,##0", 4: "#,##0.00",
    9: "0%", 10: "0.00%", 11: "0.00E+00", 12: "# ?/?", 13: "# ??/??",
    14: "mm-dd-yy", 15: "d-mmm-yy", 16: "d-mmm", 17: "mmm-yy",
    18: "h:mm AM/PM", 19: "h:mm:ss AM/PM", 20: "h:mm", 21: "h:mm:ss",
    22: "m/d/yy h:mm", 37: "#,##0 ;(#,##0)", 38: "#,##0 ;[Red](#,##0)",
    39: "#,##0.00;(#,##0.00)", 40: "#,##0.00;[Red](#,##0.00)",
    45: "mm:ss", 46: "[h]:mm:ss", 47: "mmss.0", 48: "##0.0E+0", 49: "@",
}
BUILTIN_CODES = set(BUILTIN.values())

# What the proposed value `Format` (ADR-0006 §3.2) can express. Anything else
# in an existing entry is "unmodelable" for that entry.
FONT_CHILDREN = {"b", "i", "strike", "u", "vertAlign", "sz", "color", "name", "rFont",
                 "family", "charset", "scheme", "condense", "extend"}
FONT_UNMODELED = {"outline", "shadow"}
XF_ATTRS = {"numFmtId", "fontId", "fillId", "borderId", "xfId", "applyNumberFormat",
            "applyFont", "applyFill", "applyBorder", "applyAlignment", "applyProtection",
            "quotePrefix"}
ALIGN_ATTRS = {"horizontal", "vertical", "textRotation", "wrapText", "indent", "shrinkToFit",
               "readingOrder"}
BORDER_SIDES = ["left", "right", "top", "bottom", "diagonal"]
COLOR_ATTRS = {"rgb", "theme", "tint", "indexed", "auto"}


# The tint strings rust_xlsxwriter 0.102.0 writes for Color::Theme(color, shade)
# (src/color.rs, `attributes`).
RX_TINTS = {"-4.9989318521683403E-2", "-0.14999847407452621", "-0.249977111117893",
            "-0.34998626667073579", "-0.499984740745262", "0.499984740745262",
            "0.34998626667073579", "0.249977111117893", "0.14999847407452621",
            "4.9989318521683403E-2", "-9.9978637043366805E-2", "-0.749992370372631",
            "-0.89999084444715716", "0.79998168889431442", "0.59999389629810485",
            "0.39997558519241921"}


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
        return {}
    out = {}
    for e in parse(z, rp).iter(f"{{{PKGREL}}}Relationship"):
        t = e.get("Target", "")
        if e.get("TargetMode") != "External":
            t = t[1:] if t.startswith("/") else posixpath.normpath(posixpath.join(d, t))
        out[e.get("Id")] = (e.get("Type", "").rsplit("/", 1)[-1], t)
    return out


def local(tag):
    return tag.rsplit("}", 1)[-1]


def canon(e):
    """Canonical text of an element (attributes sorted), for byte-level comparison."""
    return ET.tostring(e, encoding="unicode") if e is not None else ""


def color_issue(c):
    """Problems a Color { Rgb | Theme{idx,tint} | Indexed | Auto } cannot represent."""
    a = set(c.attrib)
    kinds = a & {"rgb", "theme", "indexed", "auto"}
    out = []
    if len(kinds) > 1:
        out.append("color:several kinds")
    if "tint" in a and "theme" not in a:
        out.append("color:tint on non-theme")
    if a - COLOR_ATTRS:
        out.append("color:other attr")
    return out


def font_issues(f):
    out = []
    for ch in f:
        n = local(ch.tag)
        if n in FONT_UNMODELED:
            out.append(f"font:{n}")
        elif n not in FONT_CHILDREN:
            out.append(f"font:unknown {n}")
        elif n == "color":
            out += color_issue(ch)
    return out


def fill_issues(f):
    out = []
    for ch in f:
        n = local(ch.tag)
        if n == "gradientFill":
            out.append("fill:gradient")
        elif n == "patternFill":
            for c in ch:
                if local(c.tag) in ("fgColor", "bgColor"):
                    out += color_issue(c)
        else:
            out.append(f"fill:unknown {n}")
    return out


def border_issues(b):
    out = []
    for ch in b:
        n = local(ch.tag)
        if n in BORDER_SIDES:
            for c in ch:
                if local(c.tag) == "color":
                    out += color_issue(c)
        elif n in ("start", "end", "vertical", "horizontal"):
            out.append(f"border:{n}")
        else:
            out.append(f"border:unknown {n}")
    if b.get("outline") not in (None, "1", "true"):
        out.append("border:outline=0")
    return out


def xf_issues(xf):
    out = []
    for a in xf.attrib:
        if local(a) not in XF_ATTRS | {"pivotButton"}:
            out.append(f"xf:attr {local(a)}")
    if xf.get("pivotButton") in ("1", "true"):
        out.append("xf:pivotButton")
    if xf.get("xfId", "0") != "0":
        out.append("xf:xfId!=0 (named style parent)")
    for ch in xf:
        n = local(ch.tag)
        if n == "alignment":
            for a in ch.attrib:
                if a not in ALIGN_ATTRS:
                    out.append(f"align:{a}")
        elif n == "protection":
            pass
        elif n == "extLst":
            out.append("xf:extLst")
        else:
            out.append(f"xf:unknown {n}")
    return out


def projection(xf, fonts, fills, borders, numfmts):
    """What a value Format would see of a cellXfs entry: resolved parts, no apply flags."""
    def at(lst, i):
        try:
            return canon(lst[int(i)])
        except (ValueError, IndexError, TypeError):
            return f"<missing {i}>"
    nid = int(xf.get("numFmtId", "0") or 0)
    code = numfmts.get(nid, BUILTIN.get(nid, f"<builtin {nid}>"))
    al = xf.find(q("alignment"))
    pr = xf.find(q("protection"))
    return (code, at(fonts, xf.get("fontId", "0")), at(fills, xf.get("fillId", "0")),
            at(borders, xf.get("borderId", "0")),
            tuple(sorted(al.attrib.items())) if al is not None else (),
            tuple(sorted(pr.attrib.items())) if pr is not None else (),
            xf.get("quotePrefix", "0") in ("1", "true"))


def flat_text(si):
    parts = []
    for t in si.iter(q("t")):
        # skip phonetic text
        parts.append(t.text or "")
    # rPh/t would be included by iter; rebuild without them
    ph = sum((len(list(r.iter(q("t")))) for r in si.iter(q("rPh"))), 0)
    if ph:
        parts = []
        for ch in si:
            n = local(ch.tag)
            if n == "t":
                parts.append(ch.text or "")
            elif n == "r":
                t = ch.find(q("t"))
                parts.append((t.text or "") if t is not None else "")
    return "".join(parts)


def styles_facts(z, part):
    r = {}
    st = parse(z, part)
    raw = z.read(part)
    r["mc_alternate_content"] = f"{{{MC}}}AlternateContent".encode() in raw or b"AlternateContent" in raw
    r["x14ac_knownFonts"] = b"knownFonts" in raw
    r["mc_ignorable"] = st.get(f"{{{MC}}}Ignorable")
    tables = {}
    for name, child in [("numFmts", "numFmt"), ("fonts", "font"), ("fills", "fill"),
                        ("borders", "border"), ("cellStyleXfs", "xf"), ("cellXfs", "xf"),
                        ("cellStyles", "cellStyle"), ("dxfs", "dxf"), ("tableStyles", "tableStyle")]:
        e = st.find(q(name))
        if e is None:
            continue
        kids = e.findall(q(child))
        cnt = e.get("count")
        tables[name] = {"n": len(kids), "count_ok": cnt is None or cnt == str(len(kids)),
                        "count_absent": cnt is None}
    r["tables"] = tables
    r["styleSheet_extLst_uris"] = sorted(x.get("uri", "") for x in st.findall(f"{q('extLst')}/{q('ext')}"))
    r["indexedColors"] = st.find(f"{q('colors')}/{q('indexedColors')}") is not None
    r["mruColors"] = st.find(f"{q('colors')}/{q('mruColors')}") is not None
    fonts = st.findall(f"{q('fonts')}/{q('font')}")
    fills = st.findall(f"{q('fills')}/{q('fill')}")
    borders = st.findall(f"{q('borders')}/{q('border')}")
    xfs = st.findall(f"{q('cellXfs')}/{q('xf')}")
    numfmts = {}
    nf_ids_below_164 = 0
    nf_builtin_code = 0
    for n in st.findall(f"{q('numFmts')}/{q('numFmt')}"):
        try:
            i = int(n.get("numFmtId"))
        except (TypeError, ValueError):
            continue
        code = n.get("formatCode", "")
        numfmts[i] = code
        nf_ids_below_164 += i < 164
        nf_builtin_code += code in BUILTIN_CODES
    codes = Counter(numfmts.values())
    r["numfmt"] = {"custom": len(numfmts), "ids_below_164": nf_ids_below_164,
                   "code_equals_builtin": nf_builtin_code,
                   "duplicate_codes": sum(c - 1 for c in codes.values() if c > 1),
                   "max_id": max(numfmts, default=None)}
    def pt(f):
        p = f.find(q("patternFill"))
        return p.get("patternType") if p is not None else ("gradient" if f.find(q("gradientFill")) is not None else None)
    r["fill0_none"] = len(fills) > 0 and pt(fills[0]) == "none"
    r["fill1_gray125"] = len(fills) > 1 and pt(fills[1]) == "gray125"
    issues = Counter()
    entry_bad = {"font": 0, "fill": 0, "border": 0, "xf": 0}
    font_bad = [bool(font_issues(f)) for f in fonts]
    fill_bad = [bool(fill_issues(f)) for f in fills]
    border_bad = [bool(border_issues(b)) for b in borders]
    for f in fonts:
        i = font_issues(f)
        issues.update(set(i))
        entry_bad["font"] += bool(i)
    for f in fills:
        i = fill_issues(f)
        issues.update(set(i))
        entry_bad["fill"] += bool(i)
    for b in borders:
        i = border_issues(b)
        issues.update(set(i))
        entry_bad["border"] += bool(i)
    xf_unmodelable = 0
    for xf in xfs:
        i = xf_issues(xf)
        issues.update(set(i))
        def bad(lst, k):
            try:
                return lst[int(xf.get(k, "0"))]
            except (ValueError, IndexError):
                return True
        deep = bool(i) or bad(font_bad, "fontId") or bad(fill_bad, "fillId") or bad(border_bad, "borderId")
        xf_unmodelable += deep
        entry_bad["xf"] += bool(i)
    r["entries"] = {"fonts": len(fonts), "fills": len(fills), "borders": len(borders), "cellXfs": len(xfs)}
    r["entries_unmodelable"] = entry_bad
    r["cellXfs_unmodelable_deep"] = xf_unmodelable
    r["issues"] = dict(issues)
    # projection collisions
    groups = defaultdict(list)
    for idx, xf in enumerate(xfs):
        groups[projection(xf, fonts, fills, borders, numfmts)].append(idx)
    dup_groups = [g for g in groups.values() if len(g) > 1]
    differing = 0
    diff_kind = Counter()
    for g in dup_groups:
        if len({canon(xfs[i]) for i in g}) > 1:
            differing += 1
            xfids = {xfs[i].get("xfId", "0") for i in g}
            applies = {tuple(sorted((k, v) for k, v in xfs[i].attrib.items() if k.startswith("apply"))) for i in g}
            if len(xfids) > 1:
                diff_kind["xfId"] += 1
            elif len(applies) > 1:
                diff_kind["apply* flags only"] += 1
            else:
                diff_kind["other"] += 1
    r["xf_projection_diff_kind"] = dict(diff_kind)
    r["xf_projection_dup_groups"] = len(dup_groups)
    r["xf_projection_dup_groups_xml_differs"] = differing
    r["xf_exact_dup_groups"] = sum(1 for c in Counter(canon(x) for x in xfs).values() if c > 1)
    r["font_exact_dups"] = sum(c - 1 for c in Counter(canon(f) for f in fonts).values() if c > 1)
    # tints
    tints = Counter()
    for c in st.iter():
        if local(c.tag) in ("color", "fgColor", "bgColor") and c.get("tint"):
            tints[c.get("tint")] += 1
    r["tints"] = dict(tints)
    # named styles
    cs = st.findall(f"{q('cellStyles')}/{q('cellStyle')}")
    names = Counter(c.get("name") for c in cs)
    r["cellStyles"] = {"n": len(cs), "custom": sum(1 for c in cs if c.get("builtinId") is None),
                       "dup_names": sum(v - 1 for v in names.values() if v > 1)}
    return r


def sst_facts(z, part, sheets):
    r = {}
    sst = parse(z, part)
    sis = sst.findall(q("si"))
    r["si"] = len(sis)
    r["count_attr"] = sst.get("count")
    r["uniqueCount_attr"] = sst.get("uniqueCount")
    r["uniqueCount_ok"] = sst.get("uniqueCount") in (None, str(len(sis)))
    refs = 0
    for s in sheets:
        try:
            ws = parse(z, s)
        except Exception:  # noqa: BLE001
            continue
        refs += sum(1 for c in ws.iter(q("c")) if c.get("t") == "s")
    r["s_refs"] = refs
    r["count_matches_refs"] = sst.get("count") is None or sst.get("count") == str(refs)
    rich = sum(1 for si in sis if si.find(q("r")) is not None)
    ph = sum(1 for si in sis if si.find(q("rPh")) is not None or si.find(q("phoneticPr")) is not None)
    r["rich"] = rich
    r["phonetic"] = ph
    by_text = defaultdict(list)
    ws_no_preserve = 0
    for si in sis:
        txt = flat_text(si)
        by_text[txt].append(si)
        for t in si.iter(q("t")):
            v = t.text or ""
            if v != v.strip(" \t\r\n") and t.get(f"{{{XML_NS}}}space") != "preserve":
                ws_no_preserve += 1
    groups = [g for g in by_text.values() if len(g) > 1]
    r["dup_text_groups"] = len(groups)
    r["dup_text_groups_mixed"] = sum(
        1 for g in groups
        if len({(si.find(q("r")) is not None, si.find(q("rPh")) is not None or si.find(q("phoneticPr")) is not None) for si in g}) > 1
        or len({canon(si) for si in g}) > 1)
    r["outer_ws_without_preserve"] = ws_no_preserve
    return r


def one(path):
    try:
        z = zipfile.ZipFile(path)
        root = rels(z, "")
        wb = next(t for ty, t in root.values() if ty == "officeDocument")
        wbx = parse(z, wb)
    except Exception:  # noqa: BLE001 - untrusted input
        return None
    if wbx.tag.startswith("{" + STRICT):
        return {"file": path, "strict": True}
    out = {"file": path}
    wbpr = wbx.find(q("workbookPr"))
    out["date1904"] = wbpr is not None and wbpr.get("date1904") in ("1", "true")
    app = "docProps/app.xml"
    if app in z.namelist():
        try:
            a = parse(z, app)
            out["app"] = next((e.text for e in a.iter() if local(e.tag) == "Application"), None)
        except Exception:  # noqa: BLE001
            pass
    wrels = rels(z, wb)
    sheets = [t for ty, t in wrels.values() if ty == "worksheet" and t in z.namelist()]
    st = next((t for ty, t in wrels.values() if ty == "styles"), None)
    ss = next((t for ty, t in wrels.values() if ty == "sharedStrings"), None)
    out["theme"] = any(ty == "theme" for ty, _ in wrels.values())
    try:
        if st and st in z.namelist():
            out["styles"] = styles_facts(z, st)
    except Exception as e:  # noqa: BLE001
        out["styles_error"] = str(e)[:80]
    try:
        if ss and ss in z.namelist():
            out["sst"] = sst_facts(z, ss, sheets)
    except Exception as e:  # noqa: BLE001
        out["sst_error"] = str(e)[:80]
    return out


def files(dirs):
    import os
    for d in dirs:
        for root, _, fs in os.walk(d):
            for f in sorted(fs):
                if f.endswith((".xlsx", ".xlsm")):
                    yield os.path.join(root, f)


def summary(rows):
    rows = [r for r in rows if r and not r.get("strict")]
    st = [r["styles"] for r in rows if "styles" in r]
    ss = [r["sst"] for r in rows if "sst" in r]
    pct = lambda a, b: f"{a}/{b} ({100 * a / b:.0f}%)" if b else f"{a}/0"
    out = []
    out.append(("packages (Transitional, readable)", len(rows)))
    out.append(("with styles.xml", len(st)))
    out.append(("styles parse errors", sum(1 for r in rows if "styles_error" in r)))
    out.append(("fills[0]=none and fills[1]=gray125", pct(sum(1 for s in st if s["fill0_none"] and s["fill1_gray125"]), len(st))))
    for t in ["numFmts", "fonts", "fills", "borders", "cellStyleXfs", "cellXfs", "cellStyles", "dxfs"]:
        has = [s["tables"][t] for s in st if t in s["tables"]]
        out.append((f"{t}: present / count attr wrong / count absent",
                    f"{len(has)} / {sum(1 for h in has if not h['count_ok'])} / {sum(1 for h in has if h['count_absent'])}"))
    sizes = sorted(s["entries"]["cellXfs"] for s in st)
    if sizes:
        out.append(("cellXfs entries: median / p90 / max", f"{sizes[len(sizes) // 2]} / {sizes[int(len(sizes) * .9)]} / {sizes[-1]}"))
    out.append(("custom numFmt with id < 164 (packages)", sum(1 for s in st if s["numfmt"]["ids_below_164"])))
    out.append(("custom numFmt whose code equals a built-in (packages)", sum(1 for s in st if s["numfmt"]["code_equals_builtin"])))
    out.append(("duplicate custom numFmt codes (packages)", sum(1 for s in st if s["numfmt"]["duplicate_codes"])))
    tot_xf = sum(s["entries"]["cellXfs"] for s in st)
    bad_xf = sum(s["cellXfs_unmodelable_deep"] for s in st)
    out.append(("cellXfs entries a value Format cannot express (incl. via font/fill/border)", pct(bad_xf, tot_xf)))
    out.append(("packages with >= 1 such entry", pct(sum(1 for s in st if s["cellXfs_unmodelable_deep"]), len(st))))
    iss = Counter()
    for s in st:
        iss.update(s["issues"].keys())
    for k, v in iss.most_common():
        out.append((f"  feature (packages): {k}", v))
    out.append(("xf projection-duplicate groups (packages with any)", pct(sum(1 for s in st if s["xf_projection_dup_groups"]), len(st))))
    out.append(("  ... where the XML of the group differs (silent-merge risk)", sum(1 for s in st if s["xf_projection_dup_groups_xml_differs"])))
    out.append(("  ... total differing groups", sum(s["xf_projection_dup_groups_xml_differs"] for s in st)))
    dk = Counter()
    for s in st:
        dk.update(s["xf_projection_diff_kind"])
    out.append(("  ... differing groups by cause (groups)", dict(dk)))
    out.append(("exact-duplicate cellXfs (packages)", sum(1 for s in st if s["xf_exact_dup_groups"])))
    out.append(("exact-duplicate fonts (packages)", sum(1 for s in st if s["font_exact_dups"])))
    out.append(("styles.xml with mc:AlternateContent", sum(1 for s in st if s["mc_alternate_content"])))
    out.append(("styles.xml with x14ac:knownFonts", sum(1 for s in st if s["x14ac_knownFonts"])))
    out.append(("styles.xml with mc:Ignorable", sum(1 for s in st if s["mc_ignorable"])))
    uris = Counter(u for s in st for u in s["styleSheet_extLst_uris"])
    out.append(("styleSheet/extLst ext uris (packages)", dict(uris.most_common(8))))
    out.append(("custom indexedColors palette", sum(1 for s in st if s["indexedColors"])))
    out.append(("named styles: packages with custom cellStyle (no builtinId)", sum(1 for s in st if s["cellStyles"]["custom"])))
    out.append(("named styles: packages with duplicate names", sum(1 for s in st if s["cellStyles"]["dup_names"])))
    tints = Counter()
    for s in st:
        tints.update(s["tints"].keys())
    out.append(("distinct tint lexical values", len(tints)))
    out.append(("  ... not in rust_xlsxwriter's Theme(color, shade) table", sorted(t for t in tints if t not in RX_TINTS)))
    out.append(("date1904 workbooks", sum(1 for r in rows if r.get("date1904"))))
    out.append(("with sharedStrings.xml", len(ss)))
    out.append(("sst uniqueCount != number of si", sum(1 for s in ss if not s["uniqueCount_ok"])))
    out.append(("sst count absent / uniqueCount absent", f"{sum(1 for s in ss if s['count_attr'] is None)} / {sum(1 for s in ss if s['uniqueCount_attr'] is None)}"))
    out.append(("sst count != actual t=\"s\" references", pct(sum(1 for s in ss if not s["count_matches_refs"]), len(ss))))
    out.append(("sst with rich items / phonetic items", f"{sum(1 for s in ss if s['rich'])} / {sum(1 for s in ss if s['phonetic'])}"))
    out.append(("sst with duplicate flattened text", pct(sum(1 for s in ss if s["dup_text_groups"]), len(ss))))
    out.append(("  ... groups mixing different XML (rich/phonetic/space)", sum(1 for s in ss if s["dup_text_groups_mixed"])))
    out.append(("sst items: outer whitespace without preserve (packages)", sum(1 for s in ss if s["outer_ws_without_preserve"])))
    apps = Counter((r.get("app") or "?").split()[0] for r in rows if "styles" in r and r["styles"]["cellXfs_unmodelable_deep"])
    out.append(("producers of packages with unmodelable xfs", dict(apps.most_common(6))))
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
