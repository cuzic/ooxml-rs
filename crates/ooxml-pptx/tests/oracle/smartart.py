"""ADR-0013 §7 independent check of SmartArt text edits.

For every case (written by `cargo run --example smartart_cases` or the
corpus sweep's `--out`), this script reads input and output with zipfile
and ElementTree only (python-pptx does not model SmartArt; it is used just
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
import xml.etree.ElementTree as ET

if sys.argv[1:2] == ["--site"]:
    sys.path.insert(0, sys.argv[2])
    del sys.argv[1:3]
from pptx import Presentation  # noqa: E402

NS = {
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "dgm": "http://schemas.openxmlformats.org/drawingml/2006/diagram",
    "dsp": "http://schemas.microsoft.com/office/drawing/2008/diagram",
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "rel": "http://schemas.openxmlformats.org/package/2006/relationships",
}


def rels(z, part):
    d, f = posixpath.split(part)
    name = posixpath.join(d, "_rels", f + ".rels")
    out = {}
    if name in z.namelist():
        for r in ET.fromstring(z.read(name)).findall("rel:Relationship", NS):
            if r.get("TargetMode") != "External":
                out[r.get("Id")] = posixpath.normpath(posixpath.join(d, r.get("Target")))
    return out


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
    slides = sorted(n for n in z.namelist() if n.startswith("ppt/slides/slide") and n.endswith(".xml"))
    # slide by presentation order
    pres = ET.fromstring(z.read("ppt/presentation.xml"))
    prels = rels(z, "ppt/presentation.xml")
    order = [prels[s.get("{%s}id" % NS["r"])] for s in pres.findall("p:sldIdLst/p:sldId", NS)]
    part = order[slide]
    srels = rels(z, part)
    root = ET.fromstring(z.read(part))
    for gf in root.iter("{%s}graphicFrame" % NS["p"]):
        cnv = gf.find("p:nvGraphicFramePr/p:cNvPr", NS)
        if cnv is not None and cnv.get("id") == str(frame):
            ri = gf.find("a:graphic/a:graphicData/dgm:relIds", NS)
            data = srels[ri.get("{%s}dm" % NS["r"])]
            droot = ET.fromstring(z.read(data))
            ext = droot.find("dgm:extLst/a:ext/dsp:dataModelExt", NS)
            drawing = srels.get(ext.get("relId")) if ext is not None else None
            return data, drawing
    raise AssertionError(f"frame {frame} not found")


def model(z, data):
    root = ET.fromstring(z.read(data))
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
    root = ET.fromstring(z.read(data))
    return {pt.get("modelId"): pt for pt in root.findall("dgm:ptLst/dgm:pt", NS)}


def main():
    cases_dir, fixtures = sys.argv[1], sys.argv[2]
    cases = json.load(open(os.path.join(cases_dir, "cases.json")))
    for c in cases:
        op = c.get("op", "set_text")
        label = f"{c['input']} slide {c['slide']} frame {c['frame']} {op} {c.get('node', '')}"
        inp = os.path.join(fixtures, c["input"])
        out = os.path.join(cases_dir, c["output"])
        Presentation(out)  # opens
        zin, zout = zipfile.ZipFile(inp), zipfile.ZipFile(out)
        data, drawing_in = parts_of(zin, c["slide"], c["frame"])
        _, drawing = parts_of(zout, c["slide"], c["frame"])
        names_in, names_out = set(zin.namelist()), set(zout.namelist())
        if op == "drop_drawing_cache":
            assert drawing is None, f"{label}: the cache reference is still there"
            assert drawing_in not in names_out, f"{label}: the drawing part is still there"
            gone = names_in - names_out
            assert all(g == drawing_in or g.startswith("ppt/diagrams/_rels/") or g.startswith("ppt/media/") for g in gone), f"{label}: removed {gone}"
            assert names_out <= names_in, f"{label}: added {names_out - names_in}"
            before, _ = model(zin, data)
            after, _ = model(zout, data)
            assert before == after, f"{label}: node texts changed"
            continue
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
            din = ET.fromstring(zin.read(drawing))
            dr = ET.fromstring(zout.read(drawing))
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
    print(f"smartart oracle: {len(cases)} cases ok")


if __name__ == "__main__":
    main()
