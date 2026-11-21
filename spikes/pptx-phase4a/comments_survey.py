#!/usr/bin/env python3
"""Comment survey for ADR-0019 (PowerPoint Phase 4a, modern comment writes).

Throwaway spike, not production code. Reads untrusted .pptx files with
zipfile + xml.etree only: nothing is extracted or executed, parts that carry
a DTD or an entity declaration are skipped, no entity is resolved. Run with
`python3 -I` from a directory that does not contain the files:

    python3 -I comments_survey.py <dirs/files...>

Packages are de-duplicated by SHA-256. Output: one `counter value` line per
measurement (sorted), with up to three example file names for rare keys.
"""

import hashlib
import os
import posixpath
import re
import sys
import zipfile
from collections import Counter, defaultdict
from xml.etree import ElementTree as ET

NS = {
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "p14": "http://schemas.microsoft.com/office/powerpoint/2010/main",
    "p15": "http://schemas.microsoft.com/office/powerpoint/2012/main",
    "p188": "http://schemas.microsoft.com/office/powerpoint/2018/8/main",
    "ac": "http://schemas.microsoft.com/office/drawing/2013/main/command",
    "pc": "http://schemas.microsoft.com/office/powerpoint/2013/main/command",
    "a16": "http://schemas.microsoft.com/office/drawing/2014/main",
    "mc": "http://schemas.openxmlformats.org/markup-compatibility/2006",
    "rel": "http://schemas.openxmlformats.org/package/2006/relationships",
    "ct": "http://schemas.openxmlformats.org/package/2006/content-types",
    "strict_p": "http://purl.oclc.org/ooxml/presentationml/main",
}
PREFIX = {v: k for k, v in NS.items()}
OD = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
REL_SLIDE = OD + "slide"
REL_LEGACY = OD + "comments"
REL_LEGACY_AUTHORS = OD + "commentAuthors"
REL_MODERN = "http://schemas.microsoft.com/office/2018/10/relationships/comments"
REL_AUTHORS = "http://schemas.microsoft.com/office/2018/10/relationships/authors"
REL_OFFICE_DOC = OD + "officeDocument"
COMMENT_REL_URI = "{6950BFC3-D8DA-4A85-94F7-54DA5524770B}"
CREATION_ID_URI = "{BB962C8B-B14F-4D97-AF65-F5344CB8AC3E}"
GUID = re.compile(r"^\{([0-9A-Fa-f]{8})-([0-9A-Fa-f]{4})-([0-9A-Fa-f]{4})-([0-9A-Fa-f]{4})-([0-9A-Fa-f]{12})\}$")
MODERN_NAME = re.compile(r"^modernComment_([0-9A-Fa-f]+)_([0-9A-Fa-f]+)\.xml$")

C = Counter()
EX = defaultdict(list)


def hit(key, name=None, n=1):
    C[key] += n
    if name and len(EX[key]) < 3 and name not in EX[key]:
        EX[key].append(name)


def q(tag):
    """Clark name -> prefix:local for printing."""
    if tag.startswith("{"):
        ns, local = tag[1:].split("}", 1)
        return f"{PREFIX.get(ns, ns)}:{local}"
    return tag


def T(prefix, local):
    return "{%s}%s" % (NS[prefix], local)


def parse(z, name):
    try:
        data = z.read(name)
    except KeyError:
        return None
    head = data[:4096]
    if b"<!DOCTYPE" in head or b"<!ENTITY" in data:
        hit("skipped.dtd_part")
        return None
    try:
        return ET.fromstring(data)
    except ET.ParseError:
        hit("skipped.unparsable_part")
        return None


def raw(z, name):
    try:
        return z.read(name)
    except KeyError:
        return None


def rels_of(z, part):
    d, b = posixpath.split(part)
    root = parse(z, posixpath.join(d, "_rels", b + ".rels"))
    out = []
    if root is None:
        return out
    for r in root.findall(T("rel", "Relationship")):
        tgt = r.get("Target", "")
        ext = r.get("TargetMode") == "External"
        res = tgt.lstrip("/") if tgt.startswith("/") else posixpath.normpath(posixpath.join(d, tgt))
        out.append((r.get("Id"), r.get("Type"), res, ext))
    return out


def guid_form(v, kind, fname):
    m = GUID.match(v or "")
    if not m:
        hit(f"{kind}.id_form=not_braced_guid", fname)
        return
    hit(f"{kind}.id_form=braced_guid")
    body = v[1:-1]
    hit(f"{kind}.id_case=" + ("upper" if body == body.upper() else "lower" if body == body.lower() else "mixed"), fname)
    ver = m.group(3)[0].upper()
    var = m.group(4)[0].upper()
    hit(f"{kind}.id_version_nibble={ver}", fname)
    hit(f"{kind}.id_variant=" + ("rfc4122" if var in "89AB" else "other"), fname)


def date_form(v, kind, fname):
    if v is None:
        hit(f"{kind}.date=absent", fname)
        return
    m = re.match(r"^\d{4}-\d\d-\d\dT\d\d:\d\d(:\d\d)?(\.(\d+))?(Z|[+-]\d\d:\d\d)?$", v)
    if not m:
        hit(f"{kind}.date=other_form", fname)
        return
    frac = len(m.group(3)) if m.group(3) else 0
    zone = "Z" if m.group(4) == "Z" else ("offset" if m.group(4) else "none")
    hit(f"{kind}.date=frac{frac}_zone_{zone}", fname)


def classify_user_id(uid, name):
    if uid is None:
        return "absent"
    if uid == name:
        return "equals_name"
    if uid.startswith("S::"):
        return "S::upn::guid"
    if "@" in uid:
        return "email_like"
    if re.fullmatch(r"[0-9a-fA-F]{16}", uid):
        return "hex16"
    if GUID.match(uid) or re.fullmatch(r"[0-9a-fA-F-]{36}", uid):
        return "guid"
    return "other"


def descendants(el):
    for c in el:
        yield c
        yield from descendants(c)


def survey_package(path, data, fname):
    try:
        z = zipfile.ZipFile(__import__("io").BytesIO(data))
        z.namelist()
    except zipfile.BadZipFile:
        hit("packages.not_zip", fname)
        return
    pkg_rels = rels_of(z, "")
    main = next((t for _, ty, t, e in pkg_rels if ty == REL_OFFICE_DOC and not e), None)
    if main is None:
        # Strict relationship type
        main = next((t for _, ty, t, e in pkg_rels if ty and ty.endswith("/officeDocument") and not e), None)
    pres = parse(z, main) if main else None
    if pres is None:
        hit("packages.no_presentation", fname)
        return
    if pres.tag == T("strict_p", "presentation"):
        hit("packages.strict", fname)
        return
    if pres.tag != T("p", "presentation"):
        hit("packages.other_root", fname)
        return
    hit("packages.read")
    app = raw(z, "docProps/app.xml") or b""
    m = re.search(rb"<Application>([^<]*)</Application>", app)
    producer = m.group(1).decode("utf-8", "replace") if m else "(none)"

    # content types
    ct_root = parse(z, "[Content_Types].xml")
    overrides = {}
    if ct_root is not None:
        for o in ct_root.findall(T("ct", "Override")):
            overrides[o.get("PartName", "").lstrip("/")] = o.get("ContentType")

    prels = rels_of(z, main)
    rid_to_slide = {i: t for i, ty, t, e in prels if ty == REL_SLIDE and not e}
    sld_ids = {}  # part -> sldId
    for s in pres.iter(T("p", "sldId")):
        rid = s.get(T("r", "id"))
        if rid in rid_to_slide:
            sld_ids[rid_to_slide[rid]] = s.get("id")
    hit("slides", n=len(sld_ids))

    # presentation extLst URIs
    ext = pres.find(T("p", "extLst"))
    for e in (ext if ext is not None else []):
        hit("presentation.ext_uri=" + (e.get("uri") or "(none)"))
    for e in pres.iter(T("mc", "AlternateContent")):
        hit("presentation.mc_AlternateContent", fname)
    viewprops = [t for _, ty, t, e in prels if ty == OD + "viewProps" and not e]
    for v in viewprops:
        vr = parse(z, v)
        if vr is not None and vr.get("showComments") is not None:
            hit("viewProps.showComments=" + vr.get("showComments"), fname)

    legacy_author_parts = [t for _, ty, t, e in prels if ty == REL_LEGACY_AUTHORS and not e]
    modern_author_parts = [t for _, ty, t, e in prels if ty == REL_AUTHORS and not e]
    if len(legacy_author_parts) > 1:
        hit("presentation.multiple_commentAuthors_rels", fname)
    if len(modern_author_parts) > 1:
        hit("presentation.multiple_authors_rels", fname)

    # --- legacy authors
    legacy_author_ids = set()
    for ap in legacy_author_parts:
        hit("legacy.authors_part", fname)
        hit("legacy.authors_part_name=" + ap)
        hit("legacy.authors_ct=" + str(overrides.get(ap)))
        r = parse(z, ap)
        if r is None:
            continue
        for a in r.findall(T("p", "cmAuthor")):
            hit("legacy.author")
            legacy_author_ids.add(a.get("id"))
            for att in ("id", "name", "initials", "lastIdx", "clrIdx"):
                hit(f"legacy.author.@{att}=" + ("present" if a.get(att) is not None else "absent"), fname)
            ci = a.get("clrIdx")
            if ci is not None and a.get("id") is not None:
                hit("legacy.author.clrIdx_" + ("equals_id" if ci == a.get("id") else "differs_from_id"), fname)
            el = a.find(T("p", "extLst"))
            for e in (el if el is not None else []):
                hit("legacy.author.ext_uri=" + str(e.get("uri")), fname)
                for pi in e.iter(T("p15", "presenceInfo")):
                    hit("legacy.author.p15_presenceInfo.providerId=" + str(pi.get("providerId")), fname)
                    hit("legacy.author.p15_presenceInfo.userId=" + classify_user_id(pi.get("userId"), a.get("name")), fname)

    # --- modern authors
    modern_author_ids = {}
    for ap in modern_author_parts:
        hit("modern.authors_part", fname)
        hit("modern.authors_part_name=" + ap)
        hit("modern.authors_ct=" + str(overrides.get(ap)))
        r = parse(z, ap)
        if r is None:
            continue
        hit("modern.authors_root=" + q(r.tag))
        for c in r:
            if c.tag != T("p188", "author"):
                hit("modern.authors_other_child=" + q(c.tag), fname)
                continue
            hit("modern.author", fname)
            aid = c.get("id")
            if aid in modern_author_ids:
                hit("modern.author.duplicate_id", fname)
            modern_author_ids[aid] = c.get("name")
            guid_form(aid, "modern.author", fname)
            for att in ("id", "name", "initials", "userId", "providerId"):
                hit(f"modern.author.@{att}=" + ("present" if c.get(att) is not None else "absent"), fname)
            for att in c.attrib:
                if att not in ("id", "name", "initials", "userId", "providerId"):
                    hit("modern.author.other_attr=" + q(att), fname)
            if c.get("providerId") is not None:
                hit("modern.author.providerId=" + c.get("providerId"), fname)
            hit("modern.author.userId=" + classify_user_id(c.get("userId"), c.get("name")), fname)
            if len(c):
                hit("modern.author.has_children", fname)
        for e in r.iter(T("mc", "AlternateContent")):
            hit("modern.authors_mc_AlternateContent", fname)

    deck_has_legacy = deck_has_modern = False
    all_modern_ids = []
    for part, sld_id in sld_ids.items():
        srels = rels_of(z, part)
        slide = parse(z, part)
        if slide is None:
            continue
        cid = None
        for c in slide.iter(T("p14", "creationId")):
            cid = c.get("val")
        hit("slides.with_p14_creationId" if cid else "slides.without_p14_creationId")
        # shapes: id -> a16 creationId ; top-level vs nested
        shapes = {}
        tree = slide.find(T("p", "cSld") + "/" + T("p", "spTree"))
        kinds = {T("p", k) for k in ("sp", "pic", "cxnSp", "graphicFrame", "grpSp", "contentPart")}

        def walk_tree(node, depth):
            for ch in node:
                if ch.tag in kinds:
                    hit(f"shapes.depth={'top' if depth == 0 else 'nested'}")
                    nv = next((x for x in ch if x.tag.endswith("Pr") and x.tag.startswith("{" + NS["p"] + "}nv")), None)
                    cnv = nv.find(T("p", "cNvPr")) if nv is not None else None
                    if cnv is not None:
                        a16 = next((x.get("id") for x in cnv.iter(T("a16", "creationId"))), None)
                        shapes[cnv.get("id")] = (a16, depth, q(ch.tag))
                        hit("shapes.with_a16_creationId" if a16 else "shapes.without_a16_creationId")
                    if ch.tag == T("p", "grpSp"):
                        walk_tree(ch, depth + 1)
                elif ch.tag == T("mc", "AlternateContent"):
                    for br in ch:
                        walk_tree(br, depth)

        if tree is not None:
            walk_tree(tree, 0)

        lrels = [(i, t) for i, ty, t, e in srels if ty == REL_LEGACY and not e]
        mrels = [(i, t) for i, ty, t, e in srels if ty == REL_MODERN and not e]
        if lrels and mrels:
            hit("slides.with_legacy_and_modern", fname)
        if len(mrels) > 1:
            hit("slides.multiple_modern_rels", fname)
        if len(lrels) > 1:
            hit("slides.multiple_legacy_rels", fname)
        # slide extLst
        sext = slide.find(T("p", "extLst"))
        uris = [e.get("uri") for e in (sext if sext is not None else [])]
        if mrels:
            hit("slides.with_modern_comments", fname)
            for u in uris:
                hit("modern.slide_ext_uri=" + str(u), fname)
            crs = list(slide.iter(T("p188", "commentRel")))
            hit(f"modern.slide.commentRel_count={len(crs)}", fname)
            for cr in crs:
                if cr.get(T("r", "id")) not in [i for i, _ in mrels]:
                    hit("modern.slide.commentRel_rid_mismatch", fname)
            for e in slide.iter(T("mc", "AlternateContent")):
                hit("modern.slide.has_mc_AlternateContent", fname)
            if sext is not None:
                pos = [q(c.tag) for c in slide].index("p:extLst")
                hit(f"modern.slide.extLst_is_last={pos == len(slide) - 1}")
        else:
            for cr in slide.iter(T("p188", "commentRel")):
                hit("slides.commentRel_without_modern_rel", fname)
        if lrels:
            for u in uris:
                hit("legacy.slide_ext_uri=" + str(u), fname)

        for rid, lp in lrels:
            deck_has_legacy = True
            hit("legacy.comment_part", fname)
            hit("legacy.comment_ct=" + str(overrides.get(lp)))
            hit("legacy.part_name_form=" + ("commentN" if re.fullmatch(r"ppt/comments/comment\d+\.xml", lp) else "other"), fname)
            r = parse(z, lp)
            if r is None:
                continue
            for cm in r.findall(T("p", "cm")):
                hit("legacy.cm")
                if cm.get("authorId") not in legacy_author_ids:
                    hit("legacy.cm.author_unresolved", fname)
                date_form(cm.get("dt"), "legacy.cm", fname)
                hit("legacy.cm.@idx=" + ("present" if cm.get("idx") else "absent"))
                hit("legacy.cm.pos=" + ("present" if cm.find(T("p", "pos")) is not None else "absent"))
                el = cm.find(T("p", "extLst"))
                for e in (el if el is not None else []):
                    hit("legacy.cm.ext_uri=" + str(e.get("uri")), fname)
                for pc in cm.iter(T("p15", "parentCm")):
                    hit("legacy.cm.reply(p15:parentCm)", fname)
                for e in cm.iter(T("mc", "AlternateContent")):
                    hit("legacy.cm.mc_AlternateContent", fname)

        for rid, mp in mrels:
            deck_has_modern = True
            hit("modern.comment_part", fname)
            hit("modern.comment_ct=" + str(overrides.get(mp)), fname)
            base = posixpath.basename(mp)
            mm = MODERN_NAME.match(base)
            if not mm:
                hit("modern.part_name=other_form", fname)
            else:
                hex_sld, hex_cid = mm.group(1), mm.group(2)
                ok_sld = sld_id is not None and int(hex_sld, 16) == int(sld_id)
                ok_cid = cid is not None and int(hex_cid, 16) == int(cid)
                hit(f"modern.part_name=sldIdHex_{'match' if ok_sld else 'differs'}_cIdHex_{'match' if ok_cid else ('differs' if cid else 'slide_has_none')}", fname)
                hit("modern.part_name_hex_case=" + ("upper" if base == base.replace(hex_sld, hex_sld.upper()).replace(hex_cid, hex_cid.upper()) else "lower_or_mixed"), fname)
            r = parse(z, mp)
            if r is None:
                continue
            hit("modern.comment_root=" + q(r.tag))
            rawb = raw(z, mp) or b""
            decl = re.search(rb"<[^?!][^>]*>", rawb)
            if decl:
                for pfx in re.findall(rb'xmlns:(\w+)=', decl.group(0)):
                    hit("modern.comment_root_declares=" + pfx.decode())
            for pfx in re.findall(rb'<(?:ac|pc):\w+ xmlns:(\w+)=', rawb):
                hit("modern.comment_local_decl_on_moniker=" + pfx.decode())
            for e in r.iter(T("mc", "AlternateContent")):
                hit("modern.comment_part.mc_AlternateContent", fname)
            for cm in r:
                if cm.tag != T("p188", "cm"):
                    hit("modern.cmLst_other_child=" + q(cm.tag), fname)
                    continue
                hit("modern.cm", fname)
                all_modern_ids.append(cm.get("id"))
                guid_form(cm.get("id"), "modern.cm", fname)
                if cm.get("authorId") not in modern_author_ids:
                    hit("modern.cm.author_unresolved", fname)
                date_form(cm.get("created"), "modern.cm", fname)
                hit("modern.cm.status=" + str(cm.get("status")), fname)
                for att in cm.attrib:
                    if att not in ("id", "authorId", "created", "status"):
                        hit("modern.cm.other_attr=" + q(att), fname)
                order = [q(c.tag) for c in cm]
                anchors = [c for c in cm if q(c.tag) not in ("p188:pos", "p188:replyLst", "p188:txBody", "p188:extLst")]
                hit("modern.cm.child_sequence=" + ",".join(
                    "ANCHOR" if c in anchors else q(c.tag).split(":")[1] for c in cm), fname)
                hit(f"modern.cm.anchor_count={len(anchors)}", fname)
                for a in anchors:
                    hit("modern.cm.anchor=" + q(a.tag), fname)
                    mon = [q(c.tag) for c in a]
                    hit("modern.cm.anchor_monikers=" + q(a.tag) + "[" + ",".join(mon) + "]", fname)
                    for d in descendants(a):
                        if len(d):
                            hit("modern.cm.anchor_nested_children_under=" + q(d.tag), fname)
                    for sm in a.iter(T("pc", "sldMk")):
                        hit("modern.sldMk.sldId_" + ("matches_slide" if sm.get("sldId") == sld_id else "differs"), fname)
                        if sm.get("cId") is None:
                            hit("modern.sldMk.cId=absent", fname)
                        else:
                            hit("modern.sldMk.cId_" + ("matches_slide" if sm.get("cId") == cid else "differs"), fname)
                    for d in a.iter():
                        if d.tag.startswith("{" + NS["ac"] + "}") and d.tag.endswith("Mk") and d.get("id") is not None:
                            s = shapes.get(d.get("id"))
                            if s is None:
                                hit("modern.ac_moniker.id_not_a_shape_on_slide", fname)
                                continue
                            hit(f"modern.ac_moniker={q(d.tag)}_on_{s[2]}_{'top' if s[1] == 0 else 'nested'}", fname)
                            ci = d.get("creationId")
                            if ci is None:
                                hit("modern.ac_moniker.creationId=absent", fname)
                            elif ci == s[0]:
                                hit("modern.ac_moniker.creationId=matches_a16", fname)
                            elif ci == "{00000000-0000-0000-0000-000000000000}":
                                hit("modern.ac_moniker.creationId=all_zero", fname)
                            else:
                                hit("modern.ac_moniker.creationId=differs", fname)
                        if d.get("cp") is not None or d.get("len") is not None:
                            hit("modern.anchor.text_offsets", fname)
                        if q(d.tag) == "ac:context":
                            hit("modern.anchor.ac_context(hash)", fname)
                hit("modern.cm.pos=" + ("present" if cm.find(T("p188", "pos")) is not None else "absent"), fname)
                rl = cm.find(T("p188", "replyLst"))
                replies = list(rl) if rl is not None else []
                hit(f"modern.cm.replies={len(replies)}", fname)
                for rp in replies:
                    if rp.tag != T("p188", "reply"):
                        hit("modern.replyLst_other_child=" + q(rp.tag), fname)
                        continue
                    hit("modern.reply")
                    all_modern_ids.append(rp.get("id"))
                    guid_form(rp.get("id"), "modern.reply", fname)
                    date_form(rp.get("created"), "modern.reply", fname)
                    if rp.get("authorId") not in modern_author_ids:
                        hit("modern.reply.author_unresolved", fname)
                    hit("modern.reply.status=" + str(rp.get("status")), fname)
                    if rp.get("created") and cm.get("created") and rp.get("created") < cm.get("created"):
                        hit("modern.reply.created_before_parent", fname)
                    hit("modern.reply.child_sequence=" + ",".join(q(c.tag).split(":")[1] for c in rp), fname)
                for body in [cm.find(T("p188", "txBody"))] + [rp.find(T("p188", "txBody")) for rp in replies]:
                    if body is None:
                        hit("modern.txBody=absent", fname)
                        continue
                    hit("modern.txBody.children=" + ",".join(sorted({q(c.tag) for c in body})))
                    for p in body.findall(T("a", "p")):
                        hit("modern.txBody.p")
                        for c in p:
                            hit("modern.txBody.p_child=" + q(c.tag), fname)
                            if c.tag == T("a", "r"):
                                rpr = c.find(T("a", "rPr"))
                                if rpr is not None:
                                    hit("modern.txBody.rPr_attrs=" + ",".join(sorted(q(x) for x in rpr.attrib)))
                                    for x in rpr:
                                        hit("modern.txBody.rPr_child=" + q(x.tag), fname)
                ex = cm.find(T("p188", "extLst"))
                for e in (ex if ex is not None else []):
                    hit("modern.cm.ext_uri=" + str(e.get("uri")), fname)
                    for x in e:
                        hit("modern.cm.ext_child=" + q(x.tag), fname)
    if deck_has_legacy and deck_has_modern:
        hit("decks.with_legacy_and_modern", fname)
    if deck_has_legacy:
        hit("decks.with_legacy_comments", fname)
        hit("decks.legacy.producer=" + producer, fname)
    if deck_has_modern:
        hit("decks.with_modern_comments", fname)
        hit("decks.modern.producer=" + producer, fname)
    if legacy_author_parts and not deck_has_legacy:
        hit("decks.commentAuthors_without_legacy_comments", fname)
    if modern_author_parts and not deck_has_modern:
        hit("decks.authors_without_modern_comments", fname)
    up = [str(x).upper() for x in all_modern_ids + list(modern_author_ids)]
    if len(up) != len(set(up)):
        hit("decks.modern.duplicate_guid", fname)


def main(argv):
    files = []
    for a in argv:
        if os.path.isdir(a):
            for root, _, names in os.walk(a):
                files += [os.path.join(root, n) for n in names if n.lower().endswith(".pptx")]
        else:
            files.append(a)
    seen = set()
    for f in sorted(files):
        with open(f, "rb") as fh:
            data = fh.read()
        h = hashlib.sha256(data).hexdigest()
        hit("files.listed")
        if h in seen:
            hit("files.duplicate_sha256")
            continue
        seen.add(h)
        if data[:8] == b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1":
            hit("packages.cfb_encrypted_or_binary", os.path.basename(f))
            continue
        survey_package(f, data, os.path.basename(f))
    for k in sorted(C):
        ex = f"   e.g. {', '.join(EX[k])}" if EX[k] and C[k] <= 5 else ""
        print(f"{k:<100} {C[k]}{ex}")


if __name__ == "__main__":
    main(sys.argv[1:])
