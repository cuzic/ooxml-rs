"""ADR-0013 §7 independent check of SmartArt text edits.

For every case (written by `cargo run --example smartart_cases` or the
corpus sweep's `--out`), this script reads input and output with zipfile
and lxml with entity resolution disabled (python-pptx does not model SmartArt; it is used just
to open the deck) and checks:
- the edited node's data paragraphs are the new lines;
- in the drawing cache, every shape showing the node (a presOf destination)
  has exactly the concatenation of its sources' paragraph texts (destOrd,
  then srcOrd; a source without dgm:t counts as one empty paragraph);
- every other node's text and every part but the data and drawing parts is
  unchanged.
Inputs are untrusted data: only parsed.

Usage: python -I smartart.py [--site DIR] <cases-dir> <fixtures-dir>
"""

import json
import os
import posixpath
import sys
import zipfile
from urllib.parse import unquote

if sys.argv[1:2] == ["--site"]:
    sys.path.insert(0, sys.argv[2])
    del sys.argv[1:3]
from lxml import etree as ET  # noqa: E402
from pptx import Presentation  # noqa: E402

NS = {
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "dgm": "http://schemas.openxmlformats.org/drawingml/2006/diagram",
    "dsp": "http://schemas.microsoft.com/office/drawing/2008/diagram",
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "ct": "http://schemas.openxmlformats.org/package/2006/content-types",
    "rel": "http://schemas.openxmlformats.org/package/2006/relationships",
}


DRAWING_REL = "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing"
CONTENT_TYPES = "[Content_Types].xml"


class DtdPresent(Exception):
    pass


def read_xml(z, name):
    parser = ET.XMLParser(resolve_entities=False, load_dtd=False, no_network=True)
    root = ET.fromstring(z.read(name), parser)
    if root.getroottree().docinfo.doctype:
        raise DtdPresent(name)
    return root


def rels_name(part):
    directory, basename = posixpath.split(part)
    return posixpath.join(directory, "_rels", basename + ".rels")


def part_key(path):
    return unquote(path).lstrip("/").lower()


def relationship_targets(z, name):
    # For both root and part relationships, the base is the parent of _rels.
    directory = posixpath.dirname(posixpath.dirname(name))
    for rel in read_xml(z, name).findall("rel:Relationship", NS):
        if rel.get("TargetMode") != "External":
            path = posixpath.join(directory, rel.get("Target").split("#", 1)[0])
            yield rel.get("Id"), posixpath.normpath(path).lstrip("/")


def rels(z, part):
    name = rels_name(part)
    return dict(relationship_targets(z, name)) if name in z.namelist() else {}


def para_text(p):
    s = ""
    for c in p:
        tag = c.tag.split("}")[1]
        if tag in ("r", "fld"):
            s += "".join(t.text or "" for t in c.findall("a:t", NS))
        elif tag == "br":
            s += "\v"
    return s


def parts_of(z, slide, frame):
    # slide by presentation order
    pres = read_xml(z, "ppt/presentation.xml")
    prels = rels(z, "ppt/presentation.xml")
    order = [prels[s.get("{%s}id" % NS["r"])] for s in pres.findall("p:sldIdLst/p:sldId", NS)]
    part = order[slide]
    srels = rels(z, part)
    root = read_xml(z, part)
    for gf in root.iter("{%s}graphicFrame" % NS["p"]):
        cnv = gf.find("p:nvGraphicFramePr/p:cNvPr", NS)
        if cnv is not None and cnv.get("id") == str(frame):
            ri = gf.find("a:graphic/a:graphicData/dgm:relIds", NS)
            data = srels[ri.get("{%s}dm" % NS["r"])]
            droot = read_xml(z, data)
            ext = droot.find("dgm:extLst/a:ext/dsp:dataModelExt", NS)
            rid = ext.get("relId") if ext is not None else None
            return part, data, rid, srels.get(rid)
    raise AssertionError(f"frame {frame} not found")


def model(z, data):
    root = read_xml(z, data)
    nodes = {}
    for pt in root.findall("dgm:ptLst/dgm:pt", NS):
        if pt.get("type", "node") in ("node", "asst"):
            t = pt.find("dgm:t", NS)
            ps = [para_text(p) for p in t.findall("a:p", NS)] if t is not None else []
            nodes[pt.get("modelId")] = ps or [""]
    pres_of = [
        (c.get("destId"), int(c.get("destOrd", 0)), int(c.get("srcOrd", 0)), c.get("srcId"))
        for c in root.findall("dgm:cxnLst/dgm:cxn", NS)
        if c.get("type") == "presOf"
    ]
    return nodes, pres_of


PROOFING = {"lang", "altLang", "dirty", "err", "smtClean"}


def text_runs(p):
    return [r for r in p.findall("a:r", NS) if "".join(t.text or "" for t in r.findall("a:t", NS))]


def pairs_ok(dp, cp):
    """Runs with text pair one to one; data a:rPr attributes (proofing
    aside) are on the cache run (ADR-0013 §4 invariant 3)."""
    dr, cr = text_runs(dp), text_runs(cp)
    if len(dr) != len(cr):
        return False
    for x, y in zip(dr, cr):
        xa = x.find("a:rPr", NS)
        ya = y.find("a:rPr", NS)
        for k, v in (xa.attrib.items() if xa is not None else []):
            if k in PROOFING:
                continue
            if ya is None or ya.get(k) != v:
                return False
    return True


def points(z, data):
    root = read_xml(z, data)
    return {pt.get("modelId"): pt for pt in root.findall("dgm:ptLst/dgm:pt", NS)}


def check_drop(zin, zout, slide, data, rid, drawing, label):
    names_in, names_out = set(zin.namelist()), set(zout.namelist())
    assert names_out <= names_in, f"{label}: added {names_out - names_in}"
    if drawing is None:
        assert rid is None, f"{label}: input cache reference did not resolve"
        assert names_in == names_out, f"{label}: parts removed without a cache"
        for name in names_in:
            assert zin.read(name) == zout.read(name), f"{label}: {name} changed"
        return

    gone = names_in - names_out
    assert drawing in gone, f"{label}: the drawing part is still there"
    drawing_rels = rels_name(drawing)
    assert drawing_rels not in names_out, f"{label}: the drawing relationships are still there"
    drawing_refs = {part_key(target) for target in rels(zin, drawing).values()}
    other_refs = {
        part_key(target)
        for name in names_in
        if name.lower().endswith(".rels") and name != drawing_rels
        for _, target in relationship_targets(zin, name)
    }
    for name in gone - {drawing, drawing_rels}:
        key = part_key(name)
        assert key.startswith("ppt/media/") and key in drawing_refs and key not in other_refs, (
            f"{label}: removed unrelated or shared part {name}"
        )

    slide_rels = rels_name(slide)
    expected_rels = read_xml(zin, slide_rels)
    match = [r for r in expected_rels.findall("rel:Relationship", NS) if r.get("Id") == rid]
    assert len(match) == 1 and match[0].get("Type") == DRAWING_REL, f"{label}: invalid input cache relationship"
    expected_rels.remove(match[0])
    assert same_xml(expected_rels, read_xml(zout, slide_rels)), f"{label}: unexpected slide relationship changes"

    expected_types = read_xml(zin, CONTENT_TYPES)
    gone_keys = {part_key(name) for name in gone}
    for override in expected_types.findall("ct:Override", NS):
        if part_key(override.get("PartName")) in gone_keys:
            expected_types.remove(override)
    assert same_xml(expected_types, read_xml(zout, CONTENT_TYPES)), f"{label}: unexpected content type changes"

    expected_data = read_xml(zin, data)
    ext_list = expected_data.find("dgm:extLst", NS)
    cache_ref = ext_list.find("a:ext/dsp:dataModelExt", NS)
    ext_list.remove(cache_ref.getparent())
    if len(ext_list) == 0:
        expected_data.remove(ext_list)
    assert same_xml(expected_data, read_xml(zout, data)), f"{label}: unexpected data model changes"
    for name in names_out - {data, slide_rels, CONTENT_TYPES}:
        assert zin.read(name) == zout.read(name), f"{label}: {name} changed"


def same_xml(before, after):
    # Namespace-expanded names and attributes must match. Ignore layout
    # whitespace between metadata elements when a deleted child leaves a gap.
    def structure(node):
        text = node.text
        if len(node) and text is not None and not text.strip():
            text = None
        return (
            node.tag, dict(node.attrib), text, node.tail if (node.tail or "").strip() else None,
            tuple(structure(child) for child in node),
        )

    return structure(before) == structure(after)


def check_case(c, zin, zout, label):
    op = c.get("op", "set_text")
    slide, data, rid, drawing_in = parts_of(zin, c["slide"], c["frame"])
    _, _, new_rid, drawing = parts_of(zout, c["slide"], c["frame"])
    names_in, names_out = set(zin.namelist()), set(zout.namelist())
    if op == "drop_drawing_cache":
        assert new_rid is None, f"{label}: the cache reference is still there"
        check_drop(zin, zout, slide, data, rid, drawing_in, label)
        return
    # no part added or removed (review m4)
    assert names_in == names_out, f"{label}: parts {names_in ^ names_out}"
    before, _ = model(zin, data)
    after, pres_of = model(zout, data)
    if op == "set_text":
        want = c["text"].split("\n")
        assert after[c["node"]] == want, f"{label}: data {after[c['node']]}"
    else:
        assert after[c["node"]][0].startswith(c["text"]) or after[c["node"]][0] == c["text"], f"{label}: data {after[c['node']]}"
    for k, v in before.items():
        if k != c["node"]:
            assert after[k] == v, f"{label}: node {k} changed"
    pt = points(zout, data)[c["node"]]
    ps = pt.find("dgm:prSet", NS)
    if ps is not None and any(after[c["node"]]):
        assert ps.get("phldr") not in ("1", "true"), f"{label}: phldr kept on a node with text"
    if drawing:
        din = read_xml(zin, drawing)
        dr = read_xml(zout, drawing)
        sps_in = {sp.get("modelId"): sp for sp in din.iter("{%s}sp" % NS["dsp"])}
        dpts = points(zout, data)
        for sp in dr.iter("{%s}sp" % NS["dsp"]):
            mid = sp.get("modelId")
            srcs = sorted((d, s, src) for dest, d, s, src in pres_of if dest == mid)
            if not any(src == c["node"] for _, _, src in srcs):
                # shapes not showing the node are untouched (review m4)
                assert ET.tostring(sp) == ET.tostring(sps_in[mid]), f"{label}: shape {mid} changed"
                continue
            shown = [para_text(p) for p in sp.findall("dsp:txBody/a:p", NS)]
            concat = [t for _, _, src in srcs for t in after.get(src, [""])]
            assert shown == concat, f"{label}: shape {mid} shows {shown}, sources {concat}"
            # run pairing and formats hold after the edit
            cps = sp.findall("dsp:txBody/a:p", NS)
            dps = [p for _, _, src in srcs for p in (dpts[src].findall("dgm:t/a:p", NS) if src in dpts and dpts[src].find("dgm:t", NS) is not None else [None])]
            for dp, cp in zip(dps, cps):
                if dp is not None:
                    assert pairs_ok(dp, cp), f"{label}: shape {mid}: runs do not pair"
    for info in zin.infolist():
        n = info.filename
        if n in (data, drawing):
            continue
        assert zin.read(n) == zout.read(n), f"{label}: {n} changed"


def main():
    cases_dir, fixtures = sys.argv[1], sys.argv[2]
    with open(os.path.join(cases_dir, "cases.json")) as manifest:
        cases = json.load(manifest)
    checked = 0
    for c in cases:
        op = c.get("op", "set_text")
        label = f"{c['input']} slide {c['slide']} frame {c['frame']} {op} {c.get('node', '')}"
        inp = os.path.join(fixtures, c["input"])
        out = os.path.join(cases_dir, c["output"])
        try:
            with zipfile.ZipFile(inp) as zin, zipfile.ZipFile(out) as zout:
                check_case(c, zin, zout, label)
        except DtdPresent as exc:
            print(f"{label}: skipped DTD in {exc}")
            continue
        Presentation(out)
        checked += 1
    print(f"smartart oracle: {checked} cases ok ({len(cases) - checked} skipped)")


if __name__ == "__main__":
    main()
