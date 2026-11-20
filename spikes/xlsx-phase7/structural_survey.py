"""Corpus survey of reference-bearing places for xlsx structural edits (ADR-0018).

Insert/delete rows and columns (roadmap G1), copy a worksheet (G2) and table /
table-column rename must rewrite every place that holds a cell reference, a
range or a formula. This script counts how often each such place occurs, and
which reference *forms* occur in formula text, so that the ADR can decide what
to support and what to refuse with numbers.

Reads untrusted packages with zipfile + xml.etree only; parts that carry a DTD
or an entity declaration are skipped (no entity is ever resolved); nothing is
extracted or executed. VML is scanned with regular expressions (it is not
always well-formed XML). Run with `python3 -I` from the repository root:

    python3 -I spikes/xlsx-phase7/structural_survey.py [--summary] DIR...

One JSON line per package (stdout); with --summary, '#'-prefixed lines follow:
`# key<TAB>files=N<TAB>total=M` per counter, then a few distributions.

The formula lexer below is an approximation of ooxml-xlsx's Phase1 lexer
(`crates/ooxml-xlsx/src/formula/lexer.rs`): good enough for "how often", not a
tokenizer to rely on. Counters prefixed `lexfail_` count texts it gave up on.
"""
import collections
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
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "c": "http://schemas.openxmlformats.org/drawingml/2006/chart",
    "c15": "http://schemas.microsoft.com/office/drawing/2012/chart",
    "mc": "http://schemas.openxmlformats.org/markup-compatibility/2006",
    "pr": "http://schemas.openxmlformats.org/package/2006/relationships",
    "tc": "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments",
    "x14": "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main",
    "xm": "http://schemas.microsoft.com/office/excel/2006/main",
    "xr": "http://schemas.microsoft.com/office/spreadsheetml/2014/revision",
}
STRICT = "http://purl.oclc.org/ooxml/"
MAX_ROW, MAX_COL = 1048576, 16384
COPYABLE = {"printerSettings", "drawing", "vmlDrawing", "comments", "threadedComment", "table", "hyperlink", "image",
            "customProperty"}


def q(p, local):
    return "{%s}%s" % (NS[p], local)


def local(tag):
    return tag.rsplit("}", 1)[-1] if isinstance(tag, str) else ""


def ns_of(tag):
    return tag[1:].split("}", 1)[0] if isinstance(tag, str) and tag.startswith("{") else ""


def safe(b):
    head = b[:8192]
    return b"<!DOCTYPE" not in head and b"<!ENTITY" not in b


class Pkg:
    def __init__(self, z):
        self.z = z
        self.names = {n.lower(): n for n in z.namelist()}

    def read(self, name):
        n = self.names.get(name.lower())
        return None if n is None else self.z.read(n)

    def xml(self, name):
        b = self.read(name)
        if b is None or not safe(b):
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
        for e in x.iter(q("pr", "Relationship")):
            t = e.get("Target", "")
            ext = e.get("TargetMode") == "External"
            if not ext:
                t = t[1:] if t.startswith("/") else posixpath.normpath(posixpath.join(d, t))
            out.append((e.get("Id"), e.get("Type", "").rsplit("/", 1)[-1], t, ext))
        return out


# ---------------------------------------------------------------- formula lexer

COL = r"\$?[A-Za-z]{1,3}"
ROW = r"\$?[0-9]{1,7}"
AREA = re.compile(rf"(?:{COL}{ROW}(?::{COL}{ROW})?|{COL}:{COL}|{ROW}:{ROW})(?![A-Za-z0-9_.(\[!])")
CELL = re.compile(rf"({COL})({ROW})$")
NAME = re.compile(r"[A-Za-z_\\\u0080-￿][A-Za-z0-9_.\\?\u0080-￿]*")
ERRS = ("#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#GETTING_DATA", "#SPILL!", "#CALC!",
        "#FIELD!", "#BLOCKED!", "#CONNECT!", "#UNKNOWN!", "#BUSY!")
REF_FUNCS = {"OFFSET", "INDIRECT", "INDEX", "ADDRESS", "ROW", "ROWS", "COLUMN", "COLUMNS", "CELL", "AREAS"}


def col_no(s):
    n = 0
    for ch in s.upper().lstrip("$"):
        n = n * 26 + ord(ch) - 64
    return n


def valid_area(a):
    for part in a.split(":"):
        m = CELL.match(part)
        if m:
            if col_no(m.group(1)) > MAX_COL or not 1 <= int(m.group(2).lstrip("$")) <= MAX_ROW:
                return False
        elif re.fullmatch(COL, part):
            if col_no(part) > MAX_COL:
                return False
        elif re.fullmatch(ROW, part):
            if not 1 <= int(part.lstrip("$")) <= MAX_ROW:
                return False
    return True


def area_kind(a):
    p = a.split(":")
    if len(p) == 1:
        return "cell"
    if all(re.fullmatch(COL, x) for x in p):
        return "cols"
    if all(re.fullmatch(ROW, x) for x in p):
        return "rows"
    # A1:A1048576 / A1:XFD1 written as areas are full columns/rows in effect
    m1, m2 = CELL.match(p[0]), CELL.match(p[1])
    if m1 and m2:
        r1, r2 = int(m1.group(2).lstrip("$")), int(m2.group(2).lstrip("$"))
        c1, c2 = col_no(m1.group(1)), col_no(m2.group(1))
        if r1 == 1 and r2 == MAX_ROW:
            return "area_fullcol"
        if c1 == 1 and c2 == MAX_COL:
            return "area_fullrow"
    return "area"


def lex(text):
    """Yields (kind, detail) tokens. kind in: ref, name, func, sref, spill, referr, str, num, err, op.
    detail for ref: dict(prefix=None|'sheet'|'3d'|'ext', area=str, kind=...).
    Raises ValueError when it gives up."""
    i, n = 0, len(text)
    out = []
    if text.startswith("="):
        i = 1
    while i < n:
        ch = text[i]
        if ch in " \t\r\n":
            i += 1
            continue
        if ch == '"':
            j = i + 1
            while True:
                k = text.find('"', j)
                if k < 0:
                    raise ValueError("unterminated string")
                if k + 1 < n and text[k + 1] == '"':
                    j = k + 2
                    continue
                break
            out.append(("str", text[i + 1:k]))
            i = k + 1
            continue
        if ch == "{":
            k = text.find("}", i)
            if k < 0:
                raise ValueError("unterminated array")
            out.append(("array", text[i:k + 1]))
            i = k + 1
            continue
        if ch == "#":
            for e in ERRS:
                if text[i:i + len(e)].upper() == e:
                    out.append(("referr" if e == "#REF!" else "err", e))
                    i += len(e)
                    break
            else:
                raise ValueError("unknown # at %d" % i)
            continue
        # prefixes: [n] external, 'quoted'!, Name!, A:B!
        prefix = None
        j = i
        if ch == "[" and re.match(r"\[\d+\]", text[i:]):
            m = re.match(r"\[\d+\]", text[i:])
            j = i + m.end()
            prefix = "ext"
        if j < n and text[j] == "'":
            k = j + 1
            while True:
                k2 = text.find("'", k)
                if k2 < 0:
                    raise ValueError("unterminated quote")
                if k2 + 1 < n and text[k2 + 1] == "'":
                    k = k2 + 2
                    continue
                break
            inner = text[j + 1:k2].replace("''", "'")
            if k2 + 1 < n and text[k2 + 1] == "!":
                if inner.startswith("["):
                    prefix = "ext"
                elif prefix is None:
                    prefix = "3d" if ":" in inner else "sheet"
                j = k2 + 2
            else:
                raise ValueError("quoted without !")
        elif j < n:
            m = re.match(r"([A-Za-z_\\\u0080-￿][A-Za-z0-9_.\u0080-￿]*(?::[A-Za-z_\\\u0080-￿][A-Za-z0-9_.\u0080-￿]*)?)!", text[j:])
            if m and not re.fullmatch(rf"{COL}{ROW}", m.group(1)):
                if prefix is None:
                    prefix = "3d" if ":" in m.group(1) else "sheet"
                j += m.end()
            elif prefix == "ext" and j < n and text[j] == "!":
                j += 1
        if prefix is not None or j != i:
            rest = text[j:]
            if rest.upper().startswith("#REF!"):
                out.append(("referr", prefix))
                i = j + 5
                continue
        m = AREA.match(text, j)
        if m and valid_area(m.group(0)):
            area = m.group(0)
            out.append(("ref", {"prefix": prefix, "area": area, "kind": area_kind(area)}))
            i = m.end()
            if i < n and text[i] == "#":
                out[-1] = ("spill", out[-1][1])
                i += 1
            continue
        m = NAME.match(text, j)
        if m:
            word = m.group(0)
            k = m.end()
            if k < n and text[k] == "(":
                out.append(("func", (word.upper(), prefix)))
                i = k + 1
                out.append(("op", "("))
                continue
            if k < n and text[k] == "[":
                end = bracket_end(text, k)
                out.append(("sref", (word, text[k:end])))
                i = end
                continue
            if word.upper() in ("TRUE", "FALSE"):
                out.append(("bool", word))
            else:
                out.append(("name", (word, prefix)))
            i = k
            continue
        if j == i and ch == "[":
            end = bracket_end(text, i)
            out.append(("sref", (None, text[i:end])))
            i = end
            continue
        m = re.match(r"(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?", text[i:])
        if m:
            out.append(("num", m.group(0)))
            i += m.end()
            continue
        if ch in "+-*/^&=<>%:,;()@!":
            out.append(("op", ch))
            i += 1
            continue
        raise ValueError("unexpected %r at %d" % (ch, i))
    return out


def bracket_end(text, i):
    depth, k = 0, i
    while k < len(text):
        c = text[k]
        if c == "'" and k + 1 < len(text):
            k += 2
            continue
        if c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                return k + 1
        k += 1
    raise ValueError("unterminated [")


def formula_stats(text, cnt, loc, names_ci, tables_ci):
    """Counts reference forms of one formula text found at location `loc`."""
    cnt["fx_" + loc] += 1
    try:
        toks = lex(text)
    except ValueError:
        cnt["lexfail_" + loc] += 1
        return None
    forms = set()
    for kind, d in toks:
        if kind == "ref":
            forms.add("ref_" + d["kind"])
            if d["prefix"] == "sheet":
                forms.add("ref_sheet_prefixed")
            elif d["prefix"] == "3d":
                forms.add("ref_3d")
            elif d["prefix"] == "ext":
                forms.add("ref_external")
            if d["kind"] == "cell" and "$" not in d["area"]:
                forms.add("ref_relative_cell")
        elif kind == "spill":
            forms.add("ref_spill")
        elif kind == "referr":
            forms.add("ref_error")
            if d in ("sheet", "3d"):
                forms.add("ref_error_sheet_prefixed")
        elif kind == "sref":
            forms.add("structured_ref")
            tname, body = d
            if tname is None:
                forms.add("structured_ref_tableless")
            elif tname.lower() in tables_ci:
                forms.add("structured_ref_known_table")
            if "#this row" in body.lower() or "[@" in body or body.startswith("[@"):
                forms.add("structured_ref_this_row")
            if re.search(r"\]\s*:\s*\[", body):
                forms.add("structured_ref_column_range")
        elif kind == "name":
            word, prefix = d
            forms.add("name_use")
            if word.lower() in tables_ci:
                forms.add("bare_table_name")
            if prefix == "ext":
                forms.add("ref_external")
            if re.fullmatch(r"[Rr]\d*[Cc]\d*", word) or re.fullmatch(r"[Rr]\[-?\d+\][Cc](\[-?\d+\])?", word):
                forms.add("r1c1_like")
        elif kind == "func":
            fn, prefix = d
            if fn in REF_FUNCS or fn.endswith("." + "OFFSET"):
                forms.add("fn_" + fn.lower())
        elif kind == "str":
            if re.search(r"(?:'[^']+'|[A-Za-z_][\w.]*)![$A-Za-z]", d):
                forms.add("sheet_ref_in_string")
    for f in forms:
        cnt[loc + "." + f] += 1
    return toks


# ---------------------------------------------------------------- shared-group simulation

def cell_pos(r):
    m = CELL.match(r or "")
    return (int(m.group(2)) - 1, col_no(m.group(1)) - 1) if m else None


def comps(area):
    """[(axis, idx, abs)] for each component of an area text, axis 'r' or 'c'."""
    out = []
    for part in area.split(":"):
        m = CELL.match(part)
        if m:
            out.append(("c", col_no(m.group(1)) - 1, m.group(1).startswith("$")))
            out.append(("r", int(m.group(2).lstrip("$")) - 1, m.group(2).startswith("$")))
        elif re.fullmatch(COL, part):
            out.append(("c", col_no(part) - 1, part.startswith("$")))
        else:
            out.append(("r", int(part.lstrip("$")) - 1, part.startswith("$")))
    return out


def own_refs(text, sheet):
    """Areas of `text` on the formula's own sheet, as component lists; None if it does not lex
    or uses a form the simulation does not model (3-D, structured)."""
    try:
        toks = lex(text)
    except ValueError:
        return None
    out = []
    for kind, d in toks:
        if kind in ("ref", "spill"):
            if d["prefix"] in ("3d", "ext"):
                return None
            if d["prefix"] == "sheet":
                continue  # another sheet, or own sheet by name: unaffected in this simulation
            out.append(comps(d["area"]))
    return out


def translate(refs, dr, dc):
    out = []
    for area in refs:
        if area == "#REF!":
            out.append(area)
            continue
        a = []
        for axis, i, ab in area:
            if not ab:
                i += dr if axis == "r" else dc
            if i < 0 or i >= (MAX_ROW if axis == "r" else MAX_COL):
                return None
            a.append((axis, i, ab))
        out.append(a)
    return out


def edit_rows(refs, op, p):
    """Insert one row before 0-based row p, or delete row p (rows axis only).
    A single deleted cell becomes '#REF!'; ranges shrink/expand per the ADR's model."""
    out = []
    for area in refs:
        if area == "#REF!":
            out.append(area)
            continue
        rows = [i for axis, i, _ in area if axis == "r"]
        if op == "ins":
            new = [(axis, i + 1 if axis == "r" and i >= p else i, ab) for axis, i, ab in area]
        else:
            if rows and all(i == p for i in rows):
                out.append("#REF!")
                continue
            new = []
            ri = 0
            for axis, i, ab in area:
                if axis == "r":
                    first = ri == 0 and len(rows) == 2
                    ri += 1
                    if i > p or (i == p and first):
                        i -= 0 if (i == p and first) else 1
                new.append((axis, i, ab))
        out.append(new)
    return out


def simulate_shared(mpos, text, members, sheet, op, everywhere=False):
    """Does a one-row edit keep every surviving member of a shared group equal to the (edited)
    master text translated to it? `#REF!` tokens compare equal under translation (ADR-0018
    rev 2 §3.3.2). With everywhere=False the edit is in the middle of the group; with True,
    at every row from the lowest row the master refers to (or the first member) to one past
    the last member, and the group counts as split if any position splits it.
    Returns 'same' / 'split' / 'unmodelled' / 'single'."""
    cells = [mpos] + members
    rows = sorted({r for r, _ in cells})
    if len(rows) < 3:
        return "single"
    base = own_refs(text, sheet)
    if base is None:
        return "unmodelled"
    if everywhere:
        lo = min([rows[0]] + [i for area in base for axis, i, _ in area if axis == "r"])
        positions = range(lo, rows[-1] + 2)
    else:
        positions = [rows[len(rows) // 2]]
    for p in positions:
        r = one_edit(mpos, base, members, op, p)
        if r != "same":
            return r
    return "same"


def one_edit(mpos, base, members, op, p):
    newpos = lambda r: r + 1 if (op == "ins" and r >= p) else (r - 1 if op == "del" and r > p else r)
    mr, mc = mpos
    survivors = [m for m in [mpos] + members if not (op == "del" and m[0] == p)]
    if len(survivors) < 2:
        return "same"
    # master deleted: hand off to the first remaining member in row-major order (ADR-0003 §4.2)
    nm = min(survivors)
    mtext = translate(base, nm[0] - mr, nm[1] - mc)
    if mtext is None:
        return "unmodelled"
    m_new = edit_rows(mtext, op, p)
    for r, c in survivors:
        eff = translate(base, r - mr, c - mc)
        if eff is None:
            return "unmodelled"
        want = edit_rows(eff, op, p)
        got = translate(m_new, newpos(r) - newpos(nm[0]), c - nm[1])
        if got != want:
            return "split"
    return "same"


# ---------------------------------------------------------------- ranges

def sqref_ranges(s):
    return [x for x in (s or "").split() if x]


def survey(path):
    z = zipfile.ZipFile(path)
    p = Pkg(z)
    root_rels = p.rels("")
    wb = next((t for _, ty, t, ext in root_rels if ty == "officeDocument" and not ext), None)
    if wb is None:
        return None
    wbx = p.xml(wb)
    if wbx is None or wbx.tag.startswith("{" + STRICT):
        return None
    cnt = collections.Counter()
    lists = collections.defaultdict(list)
    vba = any(n.lower().endswith("vbaproject.bin") for n in z.namelist())
    wp = wbx.find(q("s", "workbookProtection"))
    if wp is not None and wp.get("lockStructure") in ("1", "true"):
        cnt["wb_lockStructure"] += 1
    if wbx.find(q("s", "oleSize")) is not None:
        cnt["wb_oleSize"] += 1
    wrels = {i: (ty, t) for i, ty, t, ext in p.rels(wb) if not ext}

    # tables (names first, for the lexer's classification)
    tables = {}
    for part in [n for n in z.namelist() if re.search(r"(^|/)tables/table\d*\.xml$", n, re.I)]:
        x = p.xml(part)
        if x is not None:
            tables[part] = x
    tables_ci = {(x.get("displayName") or x.get("name") or "").lower(): part for part, x in tables.items()}
    names_ci = set()

    # ---- workbook: defined names
    dn = wbx.find(q("s", "definedNames"))
    for d in (dn if dn is not None else []):
        nm = d.get("name", "")
        names_ci.add(nm.lower())
        cnt["definedName"] += 1
        if d.get("localSheetId") is not None:
            cnt["definedName_local"] += 1
        if nm.lower().startswith("_xlnm."):
            cnt["definedName_" + nm[6:].lower()] += 1
        toks = formula_stats(d.text or "", cnt, "name", names_ci, tables_ci)
        if toks is not None and toks and all(k in ("num", "str", "bool", "err", "op", "array") for k, _ in toks):
            cnt["definedName_constant"] += 1
        if toks and any(k == "ref" and "$" not in v["area"].replace(":", "") for k, v in toks):
            cnt["definedName_relative_ref"] += 1
        if toks and any(k == "ref" and re.search(r"(^|:)[A-Za-z]|(^|:)[0-9]", v["area"]) for k, v in toks):
            cnt["definedName_partly_relative_ref"] += 1
        if toks and any(k == "ref" and v["prefix"] is None for k, v in toks):
            cnt["definedName_unprefixed_ref"] += 1
    for v in wbx.iter(q("s", "customWorkbookView")):
        cnt["customWorkbookView"] += 1
    if wbx.find(q("s", "externalReferences")) is not None:
        cnt["externalReferences"] += 1
    if any(ty == "calcChain" for ty, _ in wrels.values()):
        cnt["calcChain"] += 1
    if any(ty == "revisionHeaders" for ty, _ in wrels.values()):
        cnt["shared_workbook_revisions"] += 1
    if any(ty == "connections" for ty, _ in wrels.values()):
        cnt["connections"] += 1
    for k in ("slicerCaches", "pivotCaches"):
        if wbx.find(q("s", k)) is not None:
            cnt["wb_" + k] += 1
    for ext in wbx.iter(q("s", "ext")):
        if ext.find(".//{http://schemas.microsoft.com/office/spreadsheetml/2009/9/main}slicerCaches") is not None:
            cnt["wb_x14_slicerCaches"] += 1

    # ---- sheets
    sheets = wbx.find(q("s", "sheets"))
    sheet_list = []
    for s in (sheets if sheets is not None else []):
        rid = s.get("{%s}id" % NS["r"])
        ty, t = wrels.get(rid, (None, None))
        sheet_list.append((s.get("name"), ty, t))
    nsheets = sum(1 for _, ty, _ in sheet_list if ty == "worksheet")
    cnt["worksheets"] += nsheets
    sheet_blockers = collections.Counter()

    for sname, ty, part in sheet_list:
        if ty != "worksheet" or part is None:
            continue
        b = p.read(part)
        if b is None or not safe(b):
            continue
        try:
            ws = ET.fromstring(b)
        except ET.ParseError:
            cnt["sheet_unparsed"] += 1
            continue
        srels = p.rels(part)
        rel_types = collections.Counter(ty for _, ty, _, _ in srels)
        for rt in rel_types:
            cnt["sheetrel_" + rt] += 1
        blockers = set()
        if rel_types.get("pivotTable"):
            cnt["sheet_with_pivot_table"] += 1
        # mc:AlternateContent anywhere in the sheet, and where
        for ac in ws.iter(q("mc", "AlternateContent")):
            cnt["ws_ac"] += 1
        for child in ws:
            if child.tag == q("mc", "AlternateContent"):
                inner = {local(e.tag) for ch in child for e in ch}
                for nm in inner:
                    cnt["ws_ac_toplevel." + nm] += 1
                blockers.add("ac_toplevel")
        # sheetData
        sd = ws.find(q("s", "sheetData"))
        rows_no_r = cells_no_r = 0
        maxrow = 0
        ncells = 0
        shared = {}
        masters = {}
        members = collections.defaultdict(list)
        for row in (sd if sd is not None else []):
            if row.tag == q("mc", "AlternateContent"):
                cnt["ac_in_sheetData"] += 1
                blockers.add("ac_in_sheetData")
                continue
            if row.get("r") is None:
                rows_no_r += 1
            else:
                maxrow = max(maxrow, int(row.get("r")))
            if row.get("spans"):
                cnt["row_spans"] += 1
            for c in row:
                if c.tag == q("mc", "AlternateContent"):
                    cnt["ac_in_row"] += 1
                    blockers.add("ac_in_row")
                    continue
                if c.tag != q("s", "c"):
                    continue
                ncells += 1
                if c.get("r") is None:
                    cells_no_r += 1
                if c.get("cm") is not None:
                    cnt["cell_cm"] += 1
                if c.get("vm") is not None:
                    cnt["cell_vm"] += 1
                f = c.find(q("s", "f"))
                if f is None:
                    continue
                t = f.get("t", "normal")
                cnt["cell_f_" + t] += 1
                if t == "shared":
                    si = f.get("si")
                    pos = cell_pos(c.get("r"))
                    if f.get("ref"):
                        cnt["shared_master"] += 1
                        shared[si] = f.get("ref")
                        if pos and f.text:
                            masters[si] = (pos, f.text)
                    elif pos:
                        members[si].append(pos)
                elif t == "array":
                    ref = f.get("ref") or ""
                    if ":" in ref and ref.split(":")[0] != ref.split(":")[1]:
                        cnt["array_multicell"] += 1
                elif t == "dataTable":
                    for a in ("r1", "r2"):
                        if f.get(a):
                            cnt["dataTable_" + a] += 1
                    if f.get("del1") == "1" or f.get("del2") == "1":
                        cnt["dataTable_deleted_input"] += 1
                if f.text:
                    formula_stats(f.text, cnt, "cell", names_ci, tables_ci)
        cnt["rows_without_r"] += rows_no_r
        cnt["cells_without_r"] += cells_no_r
        if rows_no_r or cells_no_r:
            blockers.add("rows_or_cells_without_r")
        if rows_no_r or cells_no_r:
            cnt["sheets_with_implicit_positions"] += 1
        cnt["cells"] += ncells
        lists["sheet_maxrow"].append(maxrow)
        if maxrow >= 10000:
            cnt["sheets_ge_10k_rows"] += 1
        sizes = []
        for si, ref in shared.items():
            a = ref.split(":")
            if len(a) == 2:
                m1, m2 = CELL.match(a[0]), CELL.match(a[1])
                if m1 and m2:
                    sizes.append((int(m2.group(2)) - int(m1.group(2)) + 1) * (col_no(m2.group(1)) - col_no(m1.group(1)) + 1))
                    if m1.group(1) != m2.group(1) and m1.group(2) != m2.group(2):
                        cnt["shared_2d_block"] += 1
        lists["shared_group_size"].extend(sizes)
        for si, (mpos, text) in masters.items():
            for op in ("ins", "del"):
                res = simulate_shared(mpos, text, members.get(si, []), sname, op)
                cnt["sim_shared_%s_%s" % (op, res)] += 1
                res = simulate_shared(mpos, text, members.get(si, []), sname, op, everywhere=True)
                cnt["sim_shared_all_%s_%s" % (op, res)] += 1
        # sheet-level reference places
        el = {
            "dimension": ws.find(q("s", "dimension")),
            "cols": ws.find(q("s", "cols")),
            "mergeCells": ws.find(q("s", "mergeCells")),
            "hyperlinks": ws.find(q("s", "hyperlinks")),
            "autoFilter": ws.find(q("s", "autoFilter")),
            "sortState": ws.find(q("s", "sortState")),
            "rowBreaks": ws.find(q("s", "rowBreaks")),
            "colBreaks": ws.find(q("s", "colBreaks")),
            "protectedRanges": ws.find(q("s", "protectedRanges")),
            "ignoredErrors": ws.find(q("s", "ignoredErrors")),
            "scenarios": ws.find(q("s", "scenarios")),
            "dataConsolidate": ws.find(q("s", "dataConsolidate")),
            "customSheetViews": ws.find(q("s", "customSheetViews")),
            "cellWatches": ws.find(q("s", "cellWatches")),
            "phoneticPr": ws.find(q("s", "phoneticPr")),
            "webPublishItems": ws.find(q("s", "webPublishItems")),
            "oleObjects": ws.find(q("s", "oleObjects")),
            "controls": ws.find(q("s", "controls")),
            "tableParts": ws.find(q("s", "tableParts")),
            "smartTags": ws.find(q("s", "smartTags")),
        }
        for k, e in el.items():
            if e is not None:
                cnt["ws_" + k] += 1
        if el["cols"] is not None:
            for c in el["cols"]:
                cnt["col_elements"] += 1
                if c.get("max") == "16384":
                    cnt["col_to_XFD"] += 1
        if el["mergeCells"] is not None:
            cnt["mergeCell"] += len(list(el["mergeCells"]))
        if el["hyperlinks"] is not None:
            for h in el["hyperlinks"]:
                cnt["hyperlink"] += 1
                if h.get("location"):
                    cnt["hyperlink_location"] += 1
                if ":" in (h.get("ref") or ""):
                    cnt["hyperlink_range_ref"] += 1
        for k in ("rowBreaks", "colBreaks"):
            if el[k] is not None:
                cnt[k[:-1] + "_brk"] += len(list(el[k]))
        if el["ignoredErrors"] is not None:
            cnt["ignoredError"] += len(list(el["ignoredErrors"]))
        for k in ("scenarios", "webPublishItems", "oleObjects", "controls"):
            if el[k] is not None:
                blockers.add(k)
        if el["dataConsolidate"] is not None:
            blockers.add("dataConsolidate")
        if el["customSheetViews"] is not None:
            for v in el["customSheetViews"]:
                if v.find(q("s", "selection")) is not None or v.find(q("s", "pane")) is not None:
                    cnt["customSheetView_selection_or_pane"] += 1
        # views
        for sv in ws.iter(q("s", "sheetView")):
            cnt["sheetView"] += 1
            if sv.get("topLeftCell"):
                cnt["sheetView_topLeftCell"] += 1
            for pn in sv.findall(q("s", "pane")):
                cnt["pane"] += 1
                if pn.get("state") in ("frozen", "frozenSplit"):
                    cnt["pane_frozen"] += 1
            for sel in sv.findall(q("s", "selection")):
                cnt["selection"] += 1
                if len(sqref_ranges(sel.get("sqref"))) > 1:
                    cnt["selection_multi"] += 1
        # CF / DV
        for cf in ws.findall(q("s", "conditionalFormatting")):
            cnt["cf"] += 1
            if len(sqref_ranges(cf.get("sqref"))) > 1:
                cnt["cf_multi_sqref"] += 1
            for rule in cf.findall(q("s", "cfRule")):
                cnt["cfRule"] += 1
                for f in rule.findall(q("s", "formula")):
                    formula_stats(f.text or "", cnt, "cf", names_ci, tables_ci)
                for v in rule.iter(q("s", "cfvo")):
                    if v.get("type") == "formula" or (v.get("val") and not re.fullmatch(r"-?[\d.]+(E[+-]?\d+)?", v.get("val"), re.I)):
                        cnt["cfvo_formula"] += 1
        dvs = ws.find(q("s", "dataValidations"))
        for dv in (dvs if dvs is not None else []):
            cnt["dv"] += 1
            if len(sqref_ranges(dv.get("sqref"))) > 1:
                cnt["dv_multi_sqref"] += 1
            for k in ("formula1", "formula2"):
                f = dv.find(q("s", k))
                if f is not None and f.text:
                    formula_stats(f.text, cnt, "dv", names_ci, tables_ci)
        # extLst: x14 CF / DV / sparklines / other
        ext_lst = ws.find(q("s", "extLst"))
        for ext in (ext_lst if ext_lst is not None else []):
            cnt["ws_ext." + (ext.get("uri") or "?")] += 1
            for e in ext.iter():
                ln = local(e.tag)
                if ln == "f" and ns_of(e.tag) == NS["xm"]:
                    formula_stats(e.text or "", cnt, "x14", names_ci, tables_ci)
                elif ln == "sqref" and ns_of(e.tag) == NS["xm"]:
                    cnt["xm_sqref"] += 1
                elif ln == "sparkline":
                    cnt["sparkline"] += 1
        # comments and VML anchors
        for _, rt, tgt, ext in srels:
            if ext:
                continue
            if rt == "comments":
                x = p.xml(tgt)
                if x is not None:
                    cnt["comment"] += sum(1 for _ in x.iter(q("s", "comment")))
            elif rt == "vmlDrawing":
                vb = p.read(tgt) or b""
                vt = vb.decode("utf-8", "replace")
                cnt["vml_x_Row"] += len(re.findall(r"<x:Row>", vt))
                cnt["vml_x_Column"] += len(re.findall(r"<x:Column>", vt))
                cnt["vml_x_Anchor"] += len(re.findall(r"<x:Anchor>", vt))
                cnt["vml_fmla"] += len(re.findall(r"<x:Fmla\w+>", vt))
                cnt["vml_fmla_position"] += len(re.findall(r"<x:Fmla(?:Link|Range|Txbx|Pict|Group)>", vt))
                cnt["vml_note"] += len(re.findall(r'ObjectType="Note"', vt))
                cnt["vml_control"] += len(re.findall(r'ObjectType="(?:Checkbox|Button|Drop|List|Radio|Spin|Scroll|Label|GBox|Edit|Dialog)"', vt))
            elif rt == "threadedComment":
                x = p.xml(tgt)
                if x is not None:
                    cnt["threadedComment"] += sum(1 for _ in x.iter(q("tc", "threadedComment")))
            elif rt == "drawing":
                x = p.xml(tgt)
                if x is None:
                    cnt["drawing_unparsed"] += 1
                    continue
                for a in x:
                    ln = local(a.tag)
                    if ln == "AlternateContent":
                        cnt["drawing_anchor_ac"] += 1
                        continue
                    cnt["anchor_" + ln] += 1
                    if ln == "twoCellAnchor":
                        cnt["anchor_editAs_" + (a.get("editAs") or "twoCell")] += 1
                    if a.find(".//" + q("xdr", "sp")) is not None:
                        for sp in a.iter(q("xdr", "sp")):
                            if sp.get("textlink"):
                                cnt["drawing_textlink"] += 1
                for _, rt2, tgt2, ext2 in p.rels(tgt):
                    if rt2 == "chart" and not ext2:
                        cnt["chart_on_worksheet"] += 1
            elif rt == "table":
                x = tables.get(tgt)
                if x is None:
                    continue
                cnt["table"] += 1
                if x.get("tableType") and x.get("tableType") != "worksheet":
                    cnt["table_type_" + x.get("tableType")] += 1
                if x.get("totalsRowCount") not in (None, "0"):
                    cnt["table_totals"] += 1
                if x.get("headerRowCount") == "0":
                    cnt["table_no_header"] += 1
                af = x.find(q("s", "autoFilter"))
                if af is not None:
                    cnt["table_autofilter"] += 1
                    if af.find(q("s", "sortState")) is not None or x.find(q("s", "sortState")) is not None:
                        cnt["table_sortState"] += 1
                for tc in x.iter(q("s", "tableColumn")):
                    cnt["tableColumn"] += 1
                    for k in ("calculatedColumnFormula", "totalsRowFormula"):
                        f = tc.find(q("s", k))
                        if f is not None:
                            formula_stats(f.text or "", cnt, "table_" + k, names_ci, tables_ci)
                    if tc.get("dataCellStyle") or tc.get("queryTableFieldId"):
                        pass
                if p.rels(tgt):
                    cnt["table_with_rels"] += 1
            elif rt == "pivotTable":
                x = p.xml(tgt)
                if x is not None:
                    cnt["pivotTable"] += 1
        # copy (G2): relationship types a copy duplicates or shares (ADR-0018 §4)
        copy_block = {rt for rt in rel_types if rt not in COPYABLE}
        if "ac_toplevel" in blockers:
            copy_block.add("ac_toplevel")
        if any(p.rels(t) for _, rt, t, ext in srels if rt == "table" and not ext):
            copy_block.add("table_with_rels")
        for k in copy_block:
            cnt["sheet_copy_blocker." + k] += 1
        if not copy_block:
            cnt["sheet_copyable"] += 1
        prot = ws.find(q("s", "sheetProtection"))
        if prot is not None and prot.get("sheet") in ("1", "true"):
            cnt["sheet_protected"] += 1
            for k in ("insertRows", "insertColumns", "deleteRows", "deleteColumns"):
                if prot.get(k) not in ("0", "false"):
                    cnt["sheet_protected_locks_" + k] += 1
        spr = ws.find(q("s", "sheetPr"))
        if spr is not None and spr.get("codeName"):
            cnt["sheet_codeName"] += 1
            if vba:
                cnt["sheet_codeName_in_vba_package"] += 1
        for e in ws.iter(q("s", "ext")):
            cnt["ext_uri_worksheet." + (e.get("uri") or "?")] += 1
        sheet_blockers.update(blockers)
        for k in blockers:
            cnt["sheet_blocker." + k] += 1
        if not blockers:
            cnt["sheet_no_blocker"] += 1

    # ---- charts (all chart parts)
    for part in z.namelist():
        if re.search(r"(^|/)charts/chart\d*\.xml$", part, re.I):
            x = p.xml(part)
            if x is None:
                cnt["chart_unparsed"] += 1
                continue
            cnt["chart"] += 1
            if any(True for _ in x.iter(q("mc", "AlternateContent"))):
                cnt["chart_with_ac"] += 1
            for f in x.iter(q("c", "f")):
                formula_stats(f.text or "", cnt, "chart", names_ci, tables_ci)
            for f in x.iter(q("c15", "f")):
                formula_stats(f.text or "", cnt, "chart_c15", names_ci, tables_ci)
            if x.find(".//" + q("c", "pivotSource")) is not None:
                cnt["chart_pivotSource"] += 1
        elif re.search(r"(^|/)charts/chartEx\d*\.xml$", part, re.I):
            cnt["chartEx"] += 1
    # ---- pivot caches
    for part in z.namelist():
        if re.search(r"pivotCacheDefinition\d*\.xml$", part, re.I):
            x = p.xml(part)
            if x is None:
                continue
            cnt["pivotCache"] += 1
            ws_src = x.find(".//" + q("s", "worksheetSource"))
            if ws_src is not None:
                if ws_src.get("ref"):
                    cnt["pivotCache_src_ref"] += 1
                if ws_src.get("name"):
                    cnt["pivotCache_src_name"] += 1
                    if ws_src.get("name").lower() in tables_ci:
                        cnt["pivotCache_src_table"] += 1
            if x.find(".//" + q("s", "consolidation")) is not None:
                cnt["pivotCache_consolidation"] += 1
    # ---- other parts with references
    for part in z.namelist():
        low = part.lower()
        if re.search(r"(^|/)ctrlprops/ctrlprop\d*\.xml$", low):
            x = p.xml(part)
            if x is not None:
                for k in ("fmlaLink", "fmlaRange", "fmlaTxbx", "fmlaGroup"):
                    if x.get(k):
                        cnt["ctrlProp_" + k] += 1
        elif re.search(r"(^|/)connections\.xml$", low):
            x = p.xml(part)
            if x is not None:
                cnt["connection_parameter_cell"] += sum(1 for e in x.iter(q("s", "parameter")) if e.get("cell"))
        elif re.search(r"(^|/)querytables/querytable\d*\.xml$", low):
            cnt["queryTable"] += 1
        elif re.search(r"(^|/)slicers/slicer\d*\.xml$", low):
            cnt["slicer"] += 1
        elif re.search(r"(^|/)externallinks/externallink\d*\.xml$", low):
            cnt["externalLink"] += 1
        elif re.search(r"(^|/)tables/tablesinglecells\d*\.xml$", low) or "singlexmlcells" in low:
            cnt["singleXmlCells"] += 1
        elif low.endswith("calcchain.xml"):
            x = p.xml(part)
            if x is not None:
                cnt["calcChain_c"] += sum(1 for _ in x.iter(q("s", "c")))
        elif re.search(r"(^|/)activex/activex\d*\.bin$", low):
            cnt["activeX_bin"] += 1
    # xr:uid on worksheet-level elements (copied sheets would duplicate them)
    for _, ty, part in sheet_list:
        if ty == "worksheet" and part:
            b = p.read(part) or b""
            cnt["xr_uid_in_sheet"] += len(re.findall(rb'xr:uid="', b))
    for part, x in tables.items():
        b = p.read(part) or b""
        cnt["xr_uid_in_table"] += len(re.findall(rb'xr\d*:uid="', b))
    return cnt, lists


def main(argv):
    summary = "--summary" in argv
    dirs = [a for a in argv if a != "--summary"]
    files = sorted(os.path.join(d, f) for d in dirs for f in os.listdir(d) if f.lower().endswith((".xlsx", ".xlsm")))
    tot = collections.Counter()
    nfiles = collections.Counter()
    maxrows = []
    shared_sizes = []
    read = 0
    for f in files:
        try:
            r = survey(f)
        except (zipfile.BadZipFile, KeyError, OSError, RuntimeError, NotImplementedError, EOFError, ValueError, ET.ParseError):
            r = None
        if r is None:
            continue
        read += 1
        counts, lists = r
        print(json.dumps({"file": os.path.basename(f), **counts}, sort_keys=True))
        for k, v in counts.items():
            tot[k] += v
            nfiles[k] += 1 if v else 0
        maxrows.extend(lists.get("sheet_maxrow", []))
        shared_sizes.extend(lists.get("shared_group_size", []))
    if summary:
        print(f"# packages read: {read} of {len(files)}")
        for k in sorted(tot):
            print(f"# {k}\tfiles={nfiles[k]}\ttotal={tot[k]}")
        if maxrows:
            maxrows.sort()
            print(f"# sheet max row: n={len(maxrows)} median={maxrows[len(maxrows) // 2]} p99={maxrows[int(len(maxrows) * 0.99)]} max={maxrows[-1]}")
        if shared_sizes:
            shared_sizes.sort()
            print(f"# shared group cells: n={len(shared_sizes)} median={shared_sizes[len(shared_sizes) // 2]} max={shared_sizes[-1]}")


if __name__ == "__main__":
    main(sys.argv[1:])
