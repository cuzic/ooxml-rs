"""ADR-0012 §10 structural oracle and python-pptx read-back for Phase 2.

Independent of the Rust code: reads each case's input and output with
zipfile + xml.etree (structure) and python-pptx 1.0.2 (read-back), applies
the operation to a model of the input computed here, and compares.

Usage: python -I structure.py [--site DIR] <cases-dir> <fixtures-dir>
(<cases-dir>/cases.json is written by `cargo run --example structure_cases`).

The inputs are untrusted data: they are only parsed, never executed.
"""

import json
import posixpath
import sys
import zipfile
import xml.etree.ElementTree as ET

if sys.argv[1:2] == ["--site"]:
    sys.path.insert(0, sys.argv[2])
    del sys.argv[1:3]
from pptx import Presentation  # noqa: E402

NS = {
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "p14": "http://schemas.microsoft.com/office/powerpoint/2010/main",
    "rel": "http://schemas.openxmlformats.org/package/2006/relationships",
    "ct": "http://schemas.openxmlformats.org/package/2006/content-types",
}
R_ID = "{%s}id" % NS["r"]
SECTION_URI = "{521415D9-36F7-43E2-AB2F-B90AF26B5E84}"
SKIPPED_PH = {"dt", "ftr", "sldNum"}


def rels_name(part):
    d, f = posixpath.split(part)
    return posixpath.join(d, "_rels", f + ".rels")


def read_rels(z, part):
    """[(id, type, resolved target or None for external)]"""
    name = rels_name(part)
    if name not in z.namelist():
        return []
    out = []
    for r in ET.fromstring(z.read(name)).findall("rel:Relationship", NS):
        if r.get("TargetMode") == "External":
            out.append((r.get("Id"), r.get("Type"), None))
            continue
        t = r.get("Target")
        if t.startswith("/"):
            full = t[1:]
        else:
            full = posixpath.normpath(posixpath.join(posixpath.dirname(part), t))
        out.append((r.get("Id"), r.get("Type"), full))
    return out


def main_part(z):
    for _, t, target in read_rels(z, ""):
        if t.endswith("/officeDocument"):
            return target
    raise AssertionError("no officeDocument")


def slide_list(z):
    """[(sldId, slide part)] in order."""
    main = main_part(z)
    rels = {i: t for i, _, t in read_rels(z, main)}
    root = ET.fromstring(z.read(main))
    lst = root.find("p:sldIdLst", NS)
    return [(int(s.get("id")), rels.get(s.get(R_ID))) for s in ([] if lst is None else lst.findall("p:sldId", NS))]


def sections(z):
    root = ET.fromstring(z.read(main_part(z)))
    for ext in root.findall("p:extLst/p:ext", NS):
        if ext.get("uri") == SECTION_URI:
            return [[int(s.get("id")) for s in sec.findall("p14:sldIdLst/p14:sldId", NS)] for sec in ext.find("p14:sectionLst", NS)]
    return None


def check_structure(z, label):
    names = set(z.namelist())
    lower = {n.lower() for n in names}
    ct = ET.fromstring(z.read("[Content_Types].xml"))
    defaults = {d.get("Extension").lower() for d in ct.findall("ct:Default", NS)}
    overrides = {o.get("PartName").lower() for o in ct.findall("ct:Override", NS)}
    for n in names:
        if n.endswith("/") or n == "[Content_Types].xml":
            continue
        ext = n.rsplit(".", 1)[-1].lower()
        assert "/" + n.lower() in overrides or ext in defaults, f"{label}: {n} has no content type"
    for n in names:
        if not n.endswith(".rels"):
            continue
        d, f = n.rsplit("_rels/", 1)
        src = d + f[: -len(".rels")]
        for rid, typ, target in read_rels(z, src):
            if target is None or "#" in target:
                continue
            assert target.lower() in lower, f"{label}: {n} {rid} dangles ({target})"
    slides = slide_list(z)
    ids = [i for i, _ in slides]
    assert len(set(ids)) == len(ids), f"{label}: duplicate sldId"
    assert all(256 <= i < 2**31 for i in ids), f"{label}: sldId out of range"
    sec = sections(z)
    if sec is not None:
        assert sum(sec, []) == ids, f"{label}: sections {sec} != {ids}"
    # each notes slide: one back-relationship to a slide that points to it
    for _, part in slides:
        for _, typ, notes in read_rels(z, part):
            if typ.endswith("/notesSlide"):
                back = [t for _, ty, t in read_rels(z, notes) if ty.endswith("/slide")]
                assert back == [part], f"{label}: {notes} back-relationship {back} != {part}"


CASCADE = {
    "notesSlide", "comments", "image", "media", "audio", "video", "hdphoto", "chart", "package",
    "chartStyle", "chartColorStyle", "chartUserShapes", "diagramData", "diagramLayout",
    "diagramQuickStyle", "diagramColors", "diagramDrawing", "oleObject", "vmlDrawing", "tags",
    "themeOverride",
}
KEEP = {"slideLayout", "notesMaster", "hyperlink", "slide"}


def all_rels(z):
    """{source part: [(type local name, resolved target or None)]} over every .rels part."""
    out = {}
    for n in z.namelist():
        if not n.endswith(".rels"):
            continue
        d, f = n.rsplit("_rels/", 1)
        src = d + f[: -len(".rels")]
        try:
            out[src] = [(t.rsplit("/", 1)[-1], target) for _, t, target in read_rels(z, src)]
        except ET.ParseError:
            out[src] = []
    return out


def expected_cascade(z, slide):
    """ADR-0012 §6, computed independently: the slide and every part it
    reaches through cascade types, minus (to a fixed point) the parts a
    part outside the set refers to."""
    names = set(z.namelist())
    rels = all_rels(z)
    reach, queue = set(), [slide]
    while queue:
        p = queue.pop()
        for typ, t in rels.get(p, []):
            if t is None or typ in KEEP or "#" in t or t not in names:
                continue
            if t != slide and t not in reach:
                reach.add(t)
                queue.append(t)
    while True:
        inside = reach | {slide}
        keep = {c for c in reach if all(src in inside for src, lst in rels.items() for _, t in lst if t == c)}
        if keep == reach:
            break
        reach = keep
    return reach | {slide}


def unreachable(z):
    rels = all_rels(z)
    seen, queue = set(), [""]
    while queue:
        p = queue.pop()
        for _, t in rels.get(p, []):
            if t and t not in seen:
                seen.add(t)
                queue.append(t)
    return {n for n in z.namelist() if not n.endswith(".rels") and n != "[Content_Types].xml" and not n.endswith("/") and n not in seen}


def app_counts(z):
    if "docProps/app.xml" not in z.namelist():
        return None
    root = ET.fromstring(z.read("docProps/app.xml"))
    ep = "{http://schemas.openxmlformats.org/officeDocument/2006/extended-properties}"
    got = {k: (root.find(ep + k).text if root.find(ep + k) is not None else None) for k in ("Slides", "Notes", "HiddenSlides")}
    slides = slide_list(z)
    true = {
        "Slides": str(len(slides)),
        "Notes": str(sum(1 for _, p in slides if any(t.endswith("/notesSlide") for _, t, _ in read_rels(z, p)))),
        "HiddenSlides": str(sum(1 for _, p in slides if ET.fromstring(z.read(p)).get("show") in ("0", "false"))),
    }
    return got, true


def model(path):
    """python-pptx's view: [(slide_id, layout name, [shape texts])]."""
    prs = Presentation(path)
    out = []
    for s in prs.slides:
        texts = [sh.text_frame.text for sh in s.shapes if sh.has_text_frame]
        out.append((s.slide_id, s.slide_layout.name, texts))
    return out


def layout_placeholders(prs_path, layout_name):
    prs = Presentation(prs_path)
    for m in prs.slide_masters:
        for l in m.slide_layouts:
            if l.name == layout_name:
                out = []
                for ph in l.placeholders:
                    if ph._element.getparent().tag.endswith("}spTree") and ph._element.ph.get("type") not in SKIPPED_PH:
                        out.append(ph.placeholder_format.idx)
                return out
    raise AssertionError(layout_name)


def main():
    cases_dir, fixtures = sys.argv[1], sys.argv[2]
    cases = json.load(open(posixpath.join(cases_dir, "cases.json")))
    n = 0
    for c in cases:
        inp = posixpath.join(cases_dir, c["input"][1:]) if c["input"].startswith("@") else posixpath.join(fixtures, c["input"])
        out = posixpath.join(cases_dir, c["output"])
        label = f"{c['input']} {c['op']} {c['args']}"
        zin, zout = zipfile.ZipFile(inp), zipfile.ZipFile(out)
        check_structure(zout, label)
        before, after = model(inp), model(out)
        ids_in = [i for i, _ in slide_list(zin)]
        args = c["args"]
        exp = list(before)
        if c["op"] == "add":
            new_id = max(ids_in + [255]) + 1
            at = len(exp) if args["at"] is None else args["at"]
            exp.insert(at, (new_id, args["layout"], None))
        elif c["op"] == "move":
            x = exp.pop(args["from"])
            exp.insert(args["to"], x)
        elif c["op"] == "delete":
            exp.pop(args["index"])
        elif c["op"] == "duplicate":
            i = args["index"]
            at = i + 1 if args["at"] is None else args["at"]
            new_id = max(ids_in + [255]) + 1
            exp.insert(at, (new_id, exp[i][1], exp[i][2]))
        assert len(after) == len(exp), f"{label}: {len(after)} slides, expected {len(exp)}"
        for k, (a, e) in enumerate(zip(after, exp)):
            assert a[0] == e[0], f"{label}: slide {k} id {a[0]} != {e[0]}"
            assert a[1] == e[1], f"{label}: slide {k} layout {a[1]} != {e[1]}"
            if e[2] is None:
                assert all(t == "" for t in a[2]), f"{label}: new slide has text {a[2]}"
                got = [ph.placeholder_format.idx for ph in Presentation(out).slides[k].placeholders]
                assert got == layout_placeholders(out, e[1]), f"{label}: placeholders {got}"
            else:
                assert a[2] == e[2], f"{label}: slide {k} text differs"
        # delete: exactly the independently computed cascade goes (review m3)
        if c["op"] == "delete":
            slide = slide_list(zin)[args["index"]][1]
            want = expected_cascade(zin, slide)
            gone = {n for n in zin.namelist() if not n.endswith(".rels")} - set(zout.namelist())
            assert gone == want, f"{label}: removed {sorted(gone)} != expected {sorted(want)}"
        # no part becomes unreachable that was reachable before
        new_unreach = unreachable(zout) - unreachable(zin)
        assert not new_unreach, f"{label}: newly unreachable {sorted(new_unreach)}"
        # app.xml counts stay exact where they were exact
        ci, co = app_counts(zin), app_counts(zout)
        if ci and co:
            for k in ("Slides", "Notes", "HiddenSlides"):
                if ci[0][k] is not None and ci[0][k] == ci[1][k]:
                    assert co[0][k] == co[1][k], f"{label}: app.xml {k} {co[0][k]} != {co[1][k]}"
        # parts that are not touched by the op keep their bytes
        for info in zin.infolist():
            name = info.filename
            if name in c["changed"] or name not in zout.namelist():
                continue
            assert zin.read(name) == zout.read(name), f"{label}: {name} changed"
        n += 1
    print(f"structure oracle: {n} cases ok")


if __name__ == "__main__":
    main()
