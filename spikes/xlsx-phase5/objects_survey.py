"""Corpus survey of xlsx drawing-layer and object features (ADR-0017).

Reads untrusted packages with zipfile + xml.etree only; nothing is extracted
or executed. Run with `python3 -I` from the repository root:

    python3 -I spikes/xlsx-phase5/objects_survey.py [--summary] DIR...

One JSON line per package (stdout); with --summary, '#'-prefixed summary lines
follow. VML is not reliably well-formed XML, so it is scanned with regular
expressions.
"""
import collections
import hashlib
import json
import os
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

NS = {
    "s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "xdr": "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "c": "http://schemas.openxmlformats.org/drawingml/2006/chart",
    "mc": "http://schemas.openxmlformats.org/markup-compatibility/2006",
    "pr": "http://schemas.openxmlformats.org/package/2006/relationships",
    "tc": "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments",
    "x14": "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main",
    "xm": "http://schemas.microsoft.com/office/excel/2006/main",
}
R = "{%s}" % NS["r"]
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
T_DRAWING = REL + "drawing"
T_IMAGE = REL + "image"
T_CHART = REL + "chart"
T_COMMENTS = REL + "comments"
T_VML = REL + "vmlDrawing"
T_TABLE = REL + "table"
T_THREADED = "http://schemas.microsoft.com/office/2017/10/relationships/threadedComment"
T_PERSON = "http://schemas.microsoft.com/office/2017/10/relationships/person"
T_CHARTEX = "http://schemas.microsoft.com/office/2014/relationships/chartEx"
T_CHSTYLE = "http://schemas.microsoft.com/office/2011/relationships/chartStyle"
T_CHCOLORS = "http://schemas.microsoft.com/office/2011/relationships/chartColorStyle"
T_USERSHAPES = REL + "chartUserShapes"
T_PACKAGE = REL + "package"
T_SHEET_METADATA = REL + "sheetMetadata"
SPARK_URI = "{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"


def q(p, local):
    return "{%s}%s" % (NS[p], local)


def local(tag):
    return tag.rsplit("}", 1)[-1]


class Pkg:
    def __init__(self, z):
        self.z = z
        self.names = {n.lower(): n for n in z.namelist()}

    def read(self, name):
        n = self.names.get(name.lower())
        return None if n is None else self.z.read(n)

    def xml(self, name):
        b = self.read(name)
        if b is None:
            return None
        try:
            return ET.fromstring(b)
        except ET.ParseError:
            return None

    def rels(self, part):
        d, f = posixpath.split(part)
        x = self.xml(posixpath.join(d, "_rels", f + ".rels"))
        out = []
        if x is None:
            return out
        for r in x.findall(q("pr", "Relationship")):
            tgt = r.get("Target", "")
            ext = r.get("TargetMode") == "External"
            if not ext:
                tgt = tgt.lstrip("/") if tgt.startswith("/") else posixpath.normpath(posixpath.join(d, tgt))
            out.append((r.get("Id"), r.get("Type"), tgt, ext))
        return out


def sheets(p):
    wb = None
    for _, t, tgt, _ in p.rels(""):
        if t == REL + "officeDocument":
            wb = tgt
    if wb is None:
        return None, []
    rels = {i: tgt for i, t, tgt, _ in p.rels(wb)}
    x = p.xml(wb)
    if x is None:
        return wb, []
    out = []
    for s in x.iter(q("s", "sheet")):
        out.append((s.get("name"), rels.get(s.get(R + "id"))))
    return wb, out


def shared_strings(p, wb):
    for _, t, tgt, _ in p.rels(wb):
        if t == REL + "sharedStrings":
            x = p.xml(tgt)
            if x is not None:
                # si/t or si/r/t (phonetic rPh/t excluded)
                return ["".join(t_.text or "" for t_ in si.findall(q("s", "t")) + si.findall(q("s", "r") + "/" + q("s", "t")))
                        for si in x.findall(q("s", "si"))]
    return []


def col_num(letters):
    n = 0
    for ch in letters:
        n = n * 26 + ord(ch) - 64
    return n - 1


def header_cells(sx, ref, sst):
    """Values of the first row of `ref` ('=FORMULA' for a formula cell, None if absent)."""
    m = re.match(r"\$?([A-Z]+)\$?(\d+)(?::\$?([A-Z]+)\$?\d+)?$", ref)
    if not m:
        return []
    c0, row = col_num(m.group(1)), int(m.group(2))
    c1 = col_num(m.group(3)) if m.group(3) else c0
    vals = [None] * (c1 - c0 + 1)
    for r in sx.iter(q("s", "row")):
        if r.get("r") != str(row):
            continue
        for c in r.iter(q("s", "c")):
            mm = re.match(r"([A-Z]+)(\d+)$", c.get("r") or "")
            if not mm:
                continue
            k = col_num(mm.group(1)) - c0
            if not 0 <= k < len(vals):
                continue
            if c.find(q("s", "f")) is not None:
                vals[k] = "=FORMULA"
                continue
            v = c.find(q("s", "v"))
            t = c.get("t")
            if t == "s" and v is not None and (v.text or "").isdigit() and int(v.text) < len(sst):
                vals[k] = sst[int(v.text)]
            elif t == "inlineStr":
                vals[k] = "".join(x.text or "" for x in c.iter(q("s", "t")))
            elif v is not None:
                vals[k] = v.text or ""
    return vals


def survey(path):
    z = zipfile.ZipFile(path)
    p = Pkg(z)
    wb, shs = sheets(p)
    if wb is None or p.xml(wb) is None or p.xml(wb).tag != q("s", "workbook"):
        return None
    res = collections.defaultdict(lambda: 0)
    lists = collections.defaultdict(list)
    wbx = p.xml(wb)
    defined = {(d.get("name") or "").lower() for d in wbx.iter(q("s", "definedName"))}
    media_hash = collections.defaultdict(set)
    media_users = collections.defaultdict(set)
    table_ids, table_names = [], []
    idmaps = []
    all_drawing_ids = collections.Counter()
    sst = shared_strings(p, wb)
    for sname, part in shs:
        if not part:
            continue
        sx = p.xml(part)
        if sx is None:
            continue
        srels = p.rels(part)
        byid = {i: (t, tgt) for i, t, tgt, _ in srels}
        # ---- drawing
        for d in sx.findall(q("s", "drawing")):
            t, dp = byid.get(d.get(R + "id"), (None, None))
            if not dp:
                continue
            res["drawing_parts"] += 1
            dx = p.xml(dp)
            if dx is None:
                res["drawing_unparsable"] += 1
                continue
            drels = {i: (t, tgt, ext) for i, t, tgt, ext in p.rels(dp)}
            ids = []
            for anc in dx:
                k = local(anc.tag)
                if anc.tag == q("mc", "AlternateContent"):
                    res["drawing_ac_anchor"] += 1
                    continue
                res["anchor_" + k] += 1
                if anc.get("editAs"):
                    res["editAs_" + anc.get("editAs")] += 1
                for e in anc.iter():
                    if e.tag in (q("xdr", "cNvPr"),):
                        ids.append(e.get("id"))
                    if e.tag == q("mc", "AlternateContent"):
                        res["drawing_ac_inside"] += 1
                kinds = [local(c.tag) for c in anc if local(c.tag) not in ("from", "to", "pos", "ext", "clientData")]
                for kk in kinds:
                    res["obj_" + kk] += 1
                for gf in anc.iter(q("a", "graphicData")):
                    uri = gf.get("uri", "")
                    res["graphic_" + uri.rsplit("/", 1)[-1]] += 1
                for b in anc.iter(q("a", "blip")):
                    if b.get(R + "link"):
                        res["blip_link"] += 1
                    rid = b.get(R + "embed")
                    if rid and rid in drels and not drels[rid][2]:
                        m = drels[rid][1]
                        res["blip_embed"] += 1
                        media_users[m].add(dp)
                        bb = p.read(m)
                        if bb is not None:
                            media_hash[hashlib.sha256(bb).hexdigest()].add(m)
                            lists["media_ext"].append(posixpath.splitext(m)[1].lower())
                        # extLst svgBlip
                    for e in b.iter():
                        if local(e.tag) == "svgBlip":
                            res["svg_blip"] += 1
                if anc.find(".//" + q("a", "hlinkClick")) is not None:
                    res["pic_hlink"] += 1
            ints = [int(i) for i in ids if i and i.isdigit()]
            all_drawing_ids.update(set(ints))
            if len(ints) != len(set(ints)):
                res["drawings_dup_cnvpr"] += 1
            if 0 in ints:
                res["cnvpr_zero"] += 1
            if ints:
                lists["cnvpr_max_minus_count"].append(max(ints) - len(ints))
            lists["drawing_part_names"].append(re.sub(r"\d+", "N", dp))
            for i, (t, tgt, ext) in drels.items():
                if t == T_CHART:
                    res["chart_parts"] += 1
                    cx = p.xml(tgt)
                    crels = p.rels(tgt)
                    types = {tt for _, tt, _, _ in crels}
                    res["chart_with_style"] += T_CHSTYLE in types
                    res["chart_with_colors"] += T_CHCOLORS in types
                    res["chart_with_usershapes"] += T_USERSHAPES in types
                    res["chart_with_embedded_pkg"] += T_PACKAGE in types
                    if cx is not None:
                        res["chart_ac"] += sum(1 for _ in cx.iter(q("mc", "AlternateContent")))
                        res["chart_externalData"] += cx.find(q("c", "externalData")) is not None
                        fs = [f.text or "" for f in cx.iter(q("c", "f"))]
                        res["chart_f"] += len(fs)
                        res["chart_f_quoted"] += sum(1 for f in fs if f.startswith("'"))
                        res["chart_numCache"] += sum(1 for _ in cx.iter(q("c", "numCache")))
                        res["chart_numRef_nocache"] += sum(
                            1 for n in cx.iter(q("c", "numRef")) if n.find(q("c", "numCache")) is None)
                        res["chart_numLit"] += sum(1 for _ in cx.iter(q("c", "numLit")))
                        pa = cx.find(".//" + q("c", "plotArea"))
                        if pa is not None:
                            for ch in pa:
                                if local(ch.tag).endswith("Chart"):
                                    res["charttype_" + local(ch.tag)] += 1
                        if any(local(e.tag) == "ext" and e.get("uri", "").startswith("{02D57815") for e in cx.iter()):
                            res["chart_c15_filteredSeries"] += 1
                    lists["chart_part_names"].append(re.sub(r"\d+", "N", tgt))
                elif t == T_CHARTEX:
                    res["chartex_parts"] += 1
                elif t not in (T_IMAGE, T_CHART, REL + "hyperlink"):
                    lists["drawing_other_rel"].append(t.rsplit("/", 1)[-1])
        # ---- legacy comments + VML
        cpart = next((tgt for _, t, tgt, _ in srels if t == T_COMMENTS), None)
        vml_parts = [byid.get(e.get(R + "id"), (None, None))[1] for e in sx.findall(q("s", "legacyDrawing"))]
        if sx.find(q("s", "legacyDrawingHF")) is not None:
            res["legacyDrawingHF"] += 1
        refs = set()
        if cpart:
            cx = p.xml(cpart)
            res["comments_parts"] += 1
            if cx is not None:
                authors = [a.text or "" for a in cx.iter(q("s", "author"))]
                res["comment_authors"] += len(authors)
                if len(authors) != len(set(authors)):
                    res["comments_dup_authors"] += 1
                cl = list(cx.iter(q("s", "comment")))
                res["comments"] += len(cl)
                refs = {c.get("ref") for c in cl}
                res["comment_bad_authorId"] += sum(1 for c in cl if not (c.get("authorId") or "x").isdigit() or int(c.get("authorId")) >= len(authors))
                res["comment_threaded_fallback"] += sum(1 for c in cl if c.get(q("s", "x")) or any(local(k) == "uid" for k in c.attrib))
                res["comment_ac"] += sum(1 for _ in cx.iter(q("mc", "AlternateContent")))
            if not vml_parts:
                res["comments_without_vml"] += 1
        for vp in filter(None, vml_parts):
            b = p.read(vp)
            if b is None:
                continue
            res["vml_parts"] += 1
            v = b.decode("utf-8", "replace")
            try:
                ET.fromstring(b)
            except ET.ParseError:
                res["vml_not_wellformed"] += 1
            for m in re.finditer(r'<o:idmap[^>]*\bdata="([^"]*)"', v):
                idmaps.append(m.group(1))
            shapes = re.findall(r"<v:shape\b[^>]*>.*?</v:shape>", v, re.S)
            for sh in shapes:
                ot = re.search(r'ObjectType="([^"]+)"', sh)
                res["vml_obj_" + (ot.group(1) if ot else "none")] += 1
                sid = re.search(r'\bid="([^"]+)"', sh)
                if sid:
                    lists["vml_shape_id_form"].append(re.sub(r"\d+", "N", sid.group(1)))
                    mnum = re.search(r"_s(\d+)$", sid.group(1))
                    if mnum:
                        lists["vml_shape_num"].append(int(mnum.group(1)))
                if ot and ot.group(1) == "Note":
                    r_ = re.search(r"<x:Row>\s*(\d+)\s*</x:Row>", sh)
                    c_ = re.search(r"<x:Column>\s*(\d+)\s*</x:Column>", sh)
                    if r_ and c_:
                        col, row = int(c_.group(1)), int(r_.group(1))
                        name = ""
                        n = col + 1
                        while n:
                            n, rem = divmod(n - 1, 26)
                            name = chr(65 + rem) + name
                        if refs and f"{name}{row + 1}" not in refs:
                            res["vml_note_without_comment"] += 1
                    if "visibility:visible" in sh.replace(" ", ""):
                        res["vml_note_visible"] += 1
            res["vml_shapetypes"] += len(re.findall(r"<v:shapetype\b", v))
        # ---- threaded comments
        for _, t, tgt, _ in srels:
            if t == T_THREADED:
                res["threaded_parts"] += 1
                tx = p.xml(tgt)
                if tx is not None:
                    tl = list(tx.iter(q("tc", "threadedComment")))
                    res["threaded_comments"] += len(tl)
                    res["threaded_replies"] += sum(1 for t_ in tl if t_.get("parentId"))
                    lists["threaded_person_ids"] += [t_.get("personId") for t_ in tl]
                    res["threaded_ref_without_note"] += sum(1 for t_ in tl if not t_.get("parentId") and t_.get("ref") not in refs)
                    res["threaded_mentions"] += sum(1 for _ in tx.iter(q("tc", "mention")))
        # ---- tables
        for tp in sx.iter(q("s", "tablePart")):
            t, tgt = byid.get(tp.get(R + "id"), (None, None))
            tx = p.xml(tgt) if tgt else None
            if tx is None:
                continue
            res["tables"] += 1
            lists["table_part_names"].append(re.sub(r"\d+", "N", tgt))
            table_ids.append(tx.get("id"))
            table_names.append(((tx.get("name") or ""), (tx.get("displayName") or "")))
            if tx.get("name") != tx.get("displayName"):
                res["table_name_ne_displayName"] += 1
            if (tx.get("displayName") or "").lower() in defined:
                res["table_name_eq_definedname"] += 1
            if tx.get("headerRowCount") == "0":
                res["table_no_header"] += 1
            if tx.get("totalsRowCount") not in (None, "0"):
                res["table_totals"] += 1
            if tx.get("totalsRowShown") == "0":
                res["table_totalsRowShown0"] += 1
            if tx.get("tableType"):
                res["table_type_" + tx.get("tableType")] += 1
            cols = list(tx.iter(q("s", "tableColumn")))
            names = [(c.get("name") or "") for c in cols]
            if tx.get("headerRowCount") != "0" and tx.get("ref"):
                hdr = header_cells(sx, tx.get("ref"), sst)
                for i, n in enumerate(names):
                    v = hdr[i] if i < len(hdr) else None
                    if v is None:
                        res["table_header_cell_missing"] += 1
                    elif v == "=FORMULA":
                        res["table_header_cell_formula"] += 1
                    elif v != n:
                        res["table_header_mismatch"] += 1
                        lists["table_header_mismatch_examples"].append([n, v])
            if len({n.lower() for n in names}) != len(names):
                res["table_dup_colnames_ci"] += 1
            res["table_calculatedColumnFormula"] += sum(1 for _ in tx.iter(q("s", "calculatedColumnFormula")))
            res["table_totalsRowFormula"] += sum(1 for _ in tx.iter(q("s", "totalsRowFormula")))
            res["table_totalsRowFunction"] += sum(1 for c in cols if c.get("totalsRowFunction"))
            res["table_xr3_uid"] += sum(1 for c in cols if any(local(k) == "uid" for k in c.attrib))
            res["table_autoFilter"] += tx.find(q("s", "autoFilter")) is not None
            ids = [c.get("id") for c in cols]
            if ids != [str(i + 1) for i in range(len(ids))]:
                res["table_colids_not_1toN"] += 1
            res["table_ext"] += tx.find(q("s", "extLst")) is not None
        # ---- sparklines / ext
        for ext in sx.iter(q("s", "ext")):
            if ext.get("uri") == SPARK_URI:
                res["sparkline_ext"] += 1
                res["sparkline_groups"] += sum(1 for _ in ext.iter(q("x14", "sparklineGroup")))
                res["sparklines"] += sum(1 for _ in ext.iter(q("x14", "sparkline")))
        # ---- cells: cm / vm
        for c in sx.iter(q("s", "c")):
            if c.get("cm"):
                res["cells_cm"] += 1
                f = c.find(q("s", "f"))
                if f is not None and f.get("t") == "array":
                    res["cells_cm_array"] += 1
                if f is not None and "_xlfn" in (f.text or ""):
                    res["cells_cm_xlfn"] += 1
            if c.get("vm"):
                res["cells_vm"] += 1
        res["sheets_with_ac"] += any(ch.tag == q("mc", "AlternateContent") for ch in sx)
    # ---- workbook-level
    for d, ts in collections.Counter(table_ids).items():
        if ts > 1:
            res["table_id_dup"] += 1
    ti = sorted(int(i) for i in table_ids if i and i.isdigit())
    if ti:
        lists["table_id_gap"].append(ti[-1] - len(ti))
    dn = [d.lower() for _, d in table_names]
    if len(set(dn)) != len(dn):
        res["table_displayName_dup_ci"] += 1
    res["drawing_ids_repeated_across_drawings"] += sum(1 for v in all_drawing_ids.values() if v > 1)
    if len(idmaps) != len(set(idmaps)):
        res["vml_idmap_dup"] += 1
    lists["vml_idmaps"] = idmaps
    res["media_parts_shared"] += sum(1 for m, u in media_users.items() if len(u) > 1)
    res["media_dup_bytes"] += sum(len(v) - 1 for v in media_hash.values() if len(v) > 1)
    for n in z.namelist():
        ln = n.lower()
        if ln.endswith("persons/person.xml") or "/persons/" in ln:
            res["persons_part"] += 1
            px = p.xml(n)
            if px is not None:
                pids = {e.get("id") for e in px if local(e.tag) == "person"}
                lists["persons_ids"] = sorted(pids)
                res["threaded_unknown_person"] += sum(1 for i in lists["threaded_person_ids"] if i not in pids)
        if ln.startswith("xl/richdata/"):
            res["richdata_parts"] += 1
        if ln == "xl/metadata.xml":
            b = p.read(n) or b""
            res["metadata_part"] += 1
            res["metadata_XLDAPR"] += b"XLDAPR" in b
            res["metadata_XLRICHVALUE"] += b"XLRICHVALUE" in b
        if ln.startswith("xl/chartsheets/") and ln.endswith(".xml"):
            res["chartsheets"] += 1
        if ln.startswith("xl/charts/chartex"):
            res["chartex_files"] += 1
    return dict(res), {k: v for k, v in lists.items() if v and k not in ("threaded_person_ids",)}


def main(argv):
    summary = "--summary" in argv
    dirs = [a for a in argv if a != "--summary"]
    files = sorted(os.path.join(d, f) for d in dirs for f in os.listdir(d) if f.lower().endswith((".xlsx", ".xlsm")))
    tot = collections.Counter()
    nfiles = collections.Counter()
    agg = collections.defaultdict(collections.Counter)
    read = 0
    for f in files:
        try:
            r = survey(f)
        except (zipfile.BadZipFile, KeyError, OSError, RuntimeError, NotImplementedError, EOFError, ValueError):
            r = None
        if r is None:
            continue
        read += 1
        counts, lists = r
        print(json.dumps({"file": os.path.basename(f), **counts, **{k: v for k, v in lists.items() if k not in ("media_ext",)}}, sort_keys=True))
        for k, v in counts.items():
            tot[k] += v
            nfiles[k] += 1 if v else 0
        for k in ("drawing_part_names", "chart_part_names", "table_part_names", "vml_shape_id_form", "media_ext", "drawing_other_rel"):
            agg[k].update(lists.get(k, []))
        for k in ("cnvpr_max_minus_count", "table_id_gap"):
            agg[k].update(lists.get(k, []))
    if summary:
        print(f"# packages read: {read} of {len(files)}")
        for k in sorted(tot):
            print(f"# {k}\tfiles={nfiles[k]}\ttotal={tot[k]}")
        for k, c in agg.items():
            print(f"# {k}: {dict(c.most_common(12))}")


if __name__ == "__main__":
    main(sys.argv[1:])
