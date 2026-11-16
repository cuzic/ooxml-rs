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


def main():
    cases_dir, fixtures = sys.argv[1], sys.argv[2]
    cases = json.load(open(os.path.join(cases_dir, "cases.json")))
    for c in cases:
        label = f"{c['input']} slide {c['slide']} frame {c['frame']} node {c['node']}"
        inp = os.path.join(fixtures, c["input"])
        out = os.path.join(cases_dir, c["output"])
        Presentation(out)  # opens
        zin, zout = zipfile.ZipFile(inp), zipfile.ZipFile(out)
        data, drawing = parts_of(zout, c["slide"], c["frame"])
        before, _ = model(zin, data)
        after, pres_of = model(zout, data)
        want = c["text"].split("\n")
        assert after[c["node"]] == want, f"{label}: data {after[c['node']]}"
        for k, v in before.items():
            if k != c["node"]:
                assert after[k] == v, f"{label}: node {k} changed"
        if drawing:
            dr = ET.fromstring(zout.read(drawing))
            for sp in dr.iter("{%s}sp" % NS["dsp"]):
                mid = sp.get("modelId")
                srcs = sorted((d, s, src) for dest, d, s, src in pres_of if dest == mid)
                if not any(src == c["node"] for _, _, src in srcs):
                    continue
                shown = [para_text(p) for p in sp.findall("dsp:txBody/a:p", NS)]
                concat = [t for _, _, src in srcs for t in after.get(src, [""])]
                assert shown == concat, f"{label}: shape {mid} shows {shown}, sources {concat}"
        for info in zin.infolist():
            n = info.filename
            if n in (data, drawing) or n not in zout.namelist():
                continue
            assert zin.read(n) == zout.read(n), f"{label}: {n} changed"
    print(f"smartart oracle: {len(cases)} cases ok")


if __name__ == "__main__":
    main()
