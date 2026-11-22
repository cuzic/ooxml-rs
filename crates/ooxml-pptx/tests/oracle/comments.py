#!/usr/bin/env python3
"""Check added modern comments against their input packages and SDK schema."""
from collections import Counter
from datetime import datetime
import json
from pathlib import Path
import posixpath
import re
import sys
from xml.parsers import expat
import xml.etree.ElementTree as ET
from zipfile import ZipFile

NS = {
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "p188": "http://schemas.microsoft.com/office/powerpoint/2018/8/main",
    "pc": "http://schemas.microsoft.com/office/powerpoint/2013/main/command",
    "ac": "http://schemas.microsoft.com/office/drawing/2013/main/command",
    "p14": "http://schemas.microsoft.com/office/powerpoint/2010/main",
    "a16": "http://schemas.microsoft.com/office/drawing/2014/main",
    "ct": "http://schemas.openxmlformats.org/package/2006/content-types",
}
REL = "http://schemas.microsoft.com/office/2018/10/relationships/"
COMMENT_CT = "application/vnd.ms-powerpoint.comments+xml"
AUTHOR_CT = "application/vnd.ms-powerpoint.authors+xml"
GUID = re.compile(r"\{[0-9A-F]{8}-[0-9A-F]{4}-4[0-9A-F]{3}-[89AB][0-9A-F]{3}-[0-9A-F]{12}\}")
EXT = "{6950BFC3-D8DA-4A85-94F7-54DA5524770B}"


def tag(name):
    prefix, local = name.split(":")
    return f"{{{NS[prefix]}}}{local}"


def reject_doctype(*_):
    raise ValueError("DTD in XML part")


def parse(data):
    parser = expat.ParserCreate()
    parser.StartDoctypeDeclHandler = reject_doctype
    parser.Parse(data, True)
    return ET.fromstring(data)


def read(package, part):
    return parse(package.read(part))


def rels_part(source):
    directory, name = posixpath.split(source)
    return f"{directory}/_rels/{name}.rels"


def relationships(package, source):
    directory = posixpath.dirname(source)
    part = rels_part(source)
    if part not in package.namelist():
        return {}
    return {
        edge.attrib["Id"]: (
            edge.attrib["Type"],
            posixpath.normpath(posixpath.join(directory, edge.attrib["Target"])).lstrip("/"),
        )
        for edge in read(package, part)
        if edge.get("TargetMode") != "External"
    }


def overrides(package):
    return {
        element.attrib["PartName"].lstrip("/"): element.attrib["ContentType"]
        for element in read(package, "[Content_Types].xml").findall("ct:Override", NS)
    }


def ids(package, types):
    return Counter(
        node.attrib["id"].upper()
        for name, ct in types.items() if ct in (COMMENT_CT, AUTHOR_CT)
        for node in read(package, name).iter()
        if node.tag in {tag("p188:cm"), tag("p188:reply"), tag("p188:author")} and "id" in node.attrib
    )


def key(author):
    provider = author.get("providerId")
    if provider == "None":
        return (provider, author.get("name"))
    if provider:
        return (provider, author.get("userId"))
    return None


def slides(package):
    main = "ppt/presentation.xml"
    rels = relationships(package, main)
    return {
        rels[node.attrib[tag("r:id")]][1]: (
            int(node.attrib["id"]),
            read(package, rels[node.attrib[tag("r:id")]][1]),
        )
        for node in read(package, main).findall("p:sldIdLst/p:sldId", NS)
    }


def creation_id(slide):
    node = slide.find("p:cSld/p:extLst/p:ext/p14:creationId", NS)
    return int(node.attrib["val"]) if node is not None else None


def structure(deck):
    from pptx import Presentation
    return [
        [(shape.shape_id, shape.shape_type, shape.name) for shape in slide.shapes]
        for slide in Presentation(str(deck)).slides
    ]


def sdk_order(particle):
    slots = []
    for child in particle.get("Items", []):
        if "Name" in child:
            name = child["Name"].split("/")[-1].replace("oac:", "ac:", 1)
            slots.append({tag(name)})
        elif child.get("Kind") == "Choice":
            slots.append(set().union(*sdk_order(child)))
        else:
            slots.extend(sdk_order(child))
    return slots


def check_sdk(element, schemas):
    for item in element.iter():
        for name, schema in schemas.items():
            if item.tag == tag(name):
                required = [
                    a["QName"].lstrip(":") for a in schema.get("Attributes", [])
                    if any(v.get("Name") == "RequiredValidator" for v in a.get("Validators", []))
                ]
                assert all((tag(a) if ":" in a else a) in item.attrib for a in required), name
                if "Particle" in schema:
                    slots = sdk_order(schema["Particle"])
                    ranks = [next(i for i, names in enumerate(slots) if child.tag in names) for child in item]
                    assert ranks == sorted(ranks), name


def check(before, after, schemas):
    old_types, new_types = overrides(before), overrides(after)
    new_ids = ids(after, new_types) - ids(before, old_types)
    assert all(count == 1 and GUID.fullmatch(value) for value, count in new_ids.items()), new_ids
    assert not (new_ids.keys() & ids(before, old_types).keys())
    assert set(new_types) <= set(after.namelist()), "dangling Override"
    auth = [part for kind, part in relationships(after, "ppt/presentation.xml").values() if kind == REL + "authors"]
    [authors_part] = auth
    assert new_types[authors_part] == AUTHOR_CT
    authors = read(after, authors_part)
    assert authors.tag == tag("p188:authorLst")
    allowed = {"[Content_Types].xml", authors_part, rels_part("ppt/presentation.xml")}
    seen_keys = set()
    first_by_key = {}
    for author in authors:
        author_key = key(author)
        if author.get("id", "").upper() in new_ids:
            assert author_key and author_key not in seen_keys
            assert {"id", "name", "userId", "providerId"} <= author.attrib.keys()
            check_sdk(author, schemas)
        if author_key:
            seen_keys.add(author_key)
            first_by_key.setdefault(author_key, author.attrib["id"])
    author_by_id = {node.attrib["id"]: node for node in authors}
    old_slides, new_slides = slides(before), slides(after)
    cid_count = Counter(creation_id(slide) for _, slide in new_slides.values())
    new_threads = 0
    for slide_part, (slide_id, slide) in new_slides.items():
        cid = creation_id(slide)
        if slide_part not in old_slides or cid != creation_id(old_slides[slide_part][1]):
            assert cid is not None and cid_count[cid] == 1
        for rid, (kind, part) in relationships(after, slide_part).items():
            if kind != REL + "comments":
                continue
            assert new_types[part] == COMMENT_CT
            root = read(after, part)
            assert root.tag == tag("p188:cmLst")
            if part not in old_types:
                assert part == f"ppt/comments/modernComment_{slide_id:X}_{cid:X}.xml"
                [ref] = slide.findall(f"p:extLst/p:ext[@uri='{EXT}']/p188:commentRel", NS)
                assert ref.attrib[tag("r:id")] == rid
            for thread in root:
                if thread.get("id", "").upper() not in new_ids:
                    continue
                new_threads += 1
                allowed.add(part)
                if part not in old_types:
                    allowed.update((slide_part, rels_part(slide_part)))
                check_sdk(thread, schemas)
                assert {"id", "authorId", "created"} <= thread.attrib.keys()
                author = author_by_id[thread.attrib["authorId"]]
                assert first_by_key[key(author)] == author.attrib["id"]
                created = thread.attrib["created"]
                assert re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}", created)
                assert datetime.fromisoformat(created).year >= 1970
                assert thread.get("status", "active") in {"active", "resolved", "closed"}
                anchor, body = thread
                assert body.tag == tag("p188:txBody")
                assert [element.tag for element in body[:2]] == [tag("a:bodyPr"), tag("a:lstStyle")]
                assert len(body) >= 3 and all(p.tag == tag("a:p") for p in body[2:])
                assert anchor.tag in {tag("pc:sldMkLst"), tag("ac:deMkLst")}
                doc_mk, slide_mk = anchor[:2]
                assert doc_mk.tag == tag("pc:docMk") and slide_mk.tag == tag("pc:sldMk")
                assert int(slide_mk.attrib["sldId"]) == slide_id and int(slide_mk.attrib["cId"]) == cid
                if anchor.tag == tag("pc:sldMkLst"):
                    assert len(anchor) == 2
                    continue
                assert len(anchor) == 3
                moniker = anchor[2]
                kinds = {
                    tag(f"ac:{kind}Mk"): tag(f"p:{kind}")
                    for kind in ("sp", "pic", "cxnSp", "graphicFrame", "grpSp")
                }
                shapes = [
                    props for shape in slide.iter(kinds[moniker.tag])
                    for nv in shape
                    for props in nv.findall("p:cNvPr", NS)
                    if props.get("id") == moniker.attrib["id"]
                ]
                [shape] = shapes
                creation = shape.find("a:extLst/a:ext/a16:creationId", NS)
                assert moniker.get("creationId") == (None if creation is None else creation.get("id"))
    assert new_threads == 1
    old_parts, new_parts = set(before.namelist()), set(after.namelist())
    assert not old_parts - new_parts
    assert new_parts - old_parts <= allowed
    for part in old_parts - allowed:
        assert before.read(part) == after.read(part), part


def main(out, fixtures):
    sdk = Path(__file__).resolve().parents[4] / "reference/openxml-sdk-data/schemas"
    data = json.loads((sdk / "schemas_microsoft_com_office_powerpoint_2018_8_main.json").read_text())
    schemas = {item["Name"].split("/")[-1]: item for item in data["Types"] if "/" in item["Name"]}
    cases = json.loads((out / "comments.json").read_text())
    for name, fixture, adds_slide in cases:
        input_file = out / f"{name}.input.pptx" if adds_slide else fixtures / fixture
        output_file = out / f"{name}.pptx"
        with ZipFile(input_file) as before, ZipFile(output_file) as after:
            check(before, after, schemas)
        previous, current = structure(input_file), structure(output_file)
        assert previous == current, name
    print(f"{len(cases)} modern comment cases passed")


if __name__ == "__main__":
    main(*map(Path, sys.argv[1:]))
