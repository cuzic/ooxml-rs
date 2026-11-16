"""Per-fixture facts that the Phase0 acceptance tests (docs/roadmap/xlsx.md §5)
depend on, and the verdict ADR-0001/0002 predict for a value edit.

Read-only inspection of untrusted files (zipfile + ElementTree, no execution).
Run with `python3 -I spikes/xlsx/phase0_facts.py reference/fixtures/xlsx/poi reference/fixtures/xlsx/calamine`.

Prints one JSON object per file, then a Markdown summary table.
"""
import io
import json
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET
from pathlib import Path

SML = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
STRICT = "http://purl.oclc.org/ooxml/spreadsheetml/main"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
PKGREL = "http://schemas.openxmlformats.org/package/2006/relationships"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
WS_TYPE = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"


def rels(z, part):
    d, n = posixpath.split(part)
    rp = posixpath.join(d, "_rels", n + ".rels")
    out = {}
    if rp in z.namelist():
        for e in ET.fromstring(z.read(rp)).iter(f"{{{PKGREL}}}Relationship"):
            t = e.get("Target", "")
            if e.get("TargetMode") != "External":
                t = t[1:] if t.startswith("/") else posixpath.normpath(posixpath.join(d, t))
            out[e.get("Id")] = (e.get("Type", ""), t)
    return out


def col_num(ref):
    n = 0
    for ch in re.match(r"[A-Z]+", ref).group(0):
        n = n * 26 + ord(ch) - 64
    return n


def facts(p):
    raw = p.read_bytes()
    f = {"file": f"{p.parent.name}/{p.name}", "bytes": len(raw)}
    if raw[:8] == CFB_MAGIC:
        f["verdict"] = "Err(EncryptedPackage) at open (CFB container)"
        return f
    z = zipfile.ZipFile(io.BytesIO(raw))
    infos = z.infolist()
    f["parts"] = len(infos)
    f["data_descriptor_entries"] = sum(1 for i in infos if i.flag_bits & 0x08)
    f["calcChain"] = any(n.endswith("calcChain.xml") for n in z.namelist())
    root = rels(z, "")
    wb = next(t for ty, t in root.values() if ty.endswith("/officeDocument"))
    f["workbook_part"] = wb
    wbx = ET.fromstring(z.read(wb))
    wns = wbx.tag[1:].split("}")[0]
    f["strict"] = wns == STRICT
    wr = rels(z, wb)
    sheets = []
    for s in wbx.iter(f"{{{wns}}}sheet"):
        rid = next(v for k, v in s.attrib.items() if k.endswith("}id"))
        ty, t = wr[rid]
        sheets.append({"name": s.get("name"), "type": ty.rsplit("/", 1)[-1], "part": t,
                       "state": s.get("state", "visible")})
    f["sheets"] = sheets
    # Owner decision 2026-11-16 (ADR-0001 note): the first *visible worksheet*.
    visible = [s for s in sheets if s["type"] == "worksheet" and s["state"] == "visible"]
    if not visible:
        f["verdict"] = "Err(NoVisibleWorksheet)"
        return f
    first = visible[0]
    f["first_sheet"] = f"{first['name']} -> {first['part']} ({first['type']})"
    # Formula presence, by QName (SML f) vs. the naive byte prefix `<f`.
    sml_f = 0
    naive = 0
    for s in sheets:
        if s["type"] != "worksheet":
            continue
        data = z.read(s["part"])
        naive += len(re.findall(rb"<(?:[A-Za-z_][\w.-]*:)?f[\s>/]", data))
        naive_prefix = len(re.findall(rb"<f", data))
        f.setdefault("naive_byte_prefix_lt_f", 0)
        f["naive_byte_prefix_lt_f"] += naive_prefix
        for _, el in ET.iterparse(io.BytesIO(data)):
            if el.tag in (f"{{{SML}}}f", f"{{{STRICT}}}f"):
                sml_f += 1
    f["sml_f_elements"] = sml_f
    f["any_prefix_local_f"] = naive
    if f["strict"]:
        f["verdict"] = "Err(StrictNotSupported) at open"
        return f
    data = z.read(first["part"])
    ws = ET.fromstring(data)
    f["worksheet_children"] = [
        (("mc:" if el.tag.startswith(f"{{{MC}}}") else "") + el.tag.rsplit("}", 1)[-1])
        for el in ws]
    sd = ws.find(f"{{{SML}}}sheetData")
    rows = list(sd) if sd is not None else []
    cells = [c for r in rows for c in r if c.tag == f"{{{SML}}}c"]
    f["first_sheet_rows"] = len(rows)
    f["rows_without_r"] = sum(1 for r in rows if r.get("r") is None)
    f["cells_without_r"] = sum(1 for c in cells if c.get("r") is None)
    types = {}
    for c in cells:
        t = c.get("t", "n")
        if len(c) == 0:
            t = "empty(" + t + ")"
        elif t == "n" and c.find(f"{{{SML}}}f") is not None:
            t = "n+f"
        types[t] = types.get(t, 0) + 1
    f["first_sheet_cell_types"] = types
    f["selfclosing_c"] = len(re.findall(rb"<(?:\w+:)?c\b[^>]*/>", data))
    f["selfclosing_row"] = len(re.findall(rb"<(?:\w+:)?row\b[^>]*/>", data))
    f["selfclosing_sheetData"] = bool(re.search(rb"<(?:\w+:)?sheetData\s*/>", data))
    f["vm_cells"] = sum(1 for c in cells if c.get("vm") is not None)
    f["has_tableParts"] = ws.find(f"{{{SML}}}tableParts") is not None
    f["merge_refs"] = [m.get("ref") for m in ws.iter(f"{{{SML}}}mergeCell")]
    dim = ws.find(f"{{{SML}}}dimension")
    f["dimension"] = dim.get("ref") if dim is not None else None
    f["worksheet_has_AC_child"] = any(el.tag == f"{{{MC}}}AlternateContent" for el in ws)
    m = re.search(rb"<(?:\w+:)?worksheet\b[^>]*>", data)
    f["mc_ignorable"] = bool(m and b"Ignorable=" in m.group(0))
    f["xml_space_preserve_in_sheet"] = data.count(b'xml:space="preserve"')
    if "verdict" not in f:
        if sml_f:
            f["verdict"] = "every value edit: Err(FormulaPresent)"
        elif f["rows_without_r"] or f["cells_without_r"]:
            f["verdict"] = "reads+writes in affected rows: Err(MissingReference)"
        else:
            v = "value edits Ok"
            if f["merge_refs"]:
                v += "; cells of merged ranges (top-left included) Err(MergedCell) (Q3)"
            if f["vm_cells"]:
                v += f"; {f['vm_cells']} vm cell(s) Err(RichValueCell) (Q4)"
            if f["has_tableParts"]:
                v += "; table header cells Err(TableHeaderCell) (Q5)"
            f["verdict"] = v
    return f


def main(dirs):
    out = []
    for d in dirs:
        for p in sorted(Path(d).iterdir()):
            if p.suffix.lower() in (".xlsx", ".xlsm"):
                out.append(facts(p))
    for f in out:
        print(json.dumps(f, ensure_ascii=False))
    print()
    print("| fixture | first sheet | cells (first sheet) | `x:f` | naive `<f` | verdict |")
    print("|---|---|---|---:|---:|---|")
    for f in out:
        print(f"| `{f['file']}` | {f.get('first_sheet', '-')} | {f.get('first_sheet_cell_types', '-')} "
              f"| {f.get('sml_f_elements', '-')} | {f.get('naive_byte_prefix_lt_f', '-')} | {f['verdict']} |")


if __name__ == "__main__":
    main(sys.argv[1:])
