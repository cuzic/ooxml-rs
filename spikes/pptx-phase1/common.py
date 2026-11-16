"""Shared helpers for the Phase 1 spikes (ADR-0008). Not production code.

The .pptx files are untrusted: they are read with zipfile + xml.etree only
(no external entities, nothing executed). Run every script with
`python3 -I` from a directory that does not contain the files; the scripts
add their own directory to sys.path explicitly to import this module.
"""
import os, posixpath, re, zipfile
import xml.etree.ElementTree as ET

NS = {
    'p': 'http://schemas.openxmlformats.org/presentationml/2006/main',
    'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
    'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
    'mc': 'http://schemas.openxmlformats.org/markup-compatibility/2006',
    'p14': 'http://schemas.microsoft.com/office/powerpoint/2010/main',
    'p15': 'http://schemas.microsoft.com/office/powerpoint/2012/main',
    'p159': 'http://schemas.microsoft.com/office/powerpoint/2015/09/main',
    'p188': 'http://schemas.microsoft.com/office/powerpoint/2018/8/main',
    'ac': 'http://schemas.microsoft.com/office/drawing/2013/main/command',
    'pc': 'http://schemas.microsoft.com/office/powerpoint/2013/main/command',
}
PFX = {v: k for k, v in NS.items()}
PKG_REL = '{http://schemas.openxmlformats.org/package/2006/relationships}'
RT = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
RT_OFFICE_DOC = RT + 'officeDocument'
RT_LAYOUT = RT + 'slideLayout'
RT_MASTER = RT + 'slideMaster'
RT_THEME = RT + 'theme'
RT_NOTES = RT + 'notesSlide'
RT_NOTES_MASTER = RT + 'notesMaster'
RT_SLIDE = RT + 'slide'
RT_HLINK = RT + 'hyperlink'
RT_COMMENTS = RT + 'comments'
RT_COMMENT_AUTHORS = RT + 'commentAuthors'
RT_MODERN_COMMENTS = 'http://schemas.microsoft.com/office/2018/10/relationships/comments'
RT_AUTHORS = 'http://schemas.microsoft.com/office/2018/10/relationships/authors'


def q(t):
    p, l = t.split(':')
    return '{%s}%s' % (NS[p], l)


def qn(tag):
    """'{uri}local' -> 'pfx:local' (unknown URIs keep the Clark form)."""
    if tag.startswith('{'):
        uri, l = tag[1:].split('}')
        return '%s:%s' % (PFX[uri], l) if uri in PFX else tag
    return tag


def iter_files(args):
    out = []
    for a in args:
        if os.path.isdir(a):
            out += [os.path.join(a, n) for n in sorted(os.listdir(a)) if n.lower().endswith(('.pptx', '.pptm', '.potx', '.ppsx'))]
        else:
            out.append(a)
    return out


class Pkg:
    """A package opened read-only. `xml(part)` caches parsed parts; parse
    failures return None and are counted by the caller."""

    def __init__(self, path):
        self.path = path
        self.z = zipfile.ZipFile(path)
        self.names = set(self.z.namelist())
        self._xml = {}
        self.pres = None
        for _id, ty, tgt, ext in self.rels(''):
            if ty == RT_OFFICE_DOC and not ext:
                self.pres = tgt

    def xml(self, part):
        if part not in self._xml:
            try:
                self._xml[part] = ET.fromstring(self.z.read(part)) if part in self.names else None
            except Exception:
                self._xml[part] = None
        return self._xml[part]

    def rels(self, part):
        """[(Id, Type, absolute target or URL, external?)]"""
        d, b = posixpath.split(part)
        rp = posixpath.join(d, '_rels', b + '.rels') if part else '_rels/.rels'
        if rp not in self.names:
            return []
        try:
            root = ET.fromstring(self.z.read(rp))
        except Exception:
            return []
        out = []
        for r in root:
            ext = r.get('TargetMode') == 'External'
            t = r.get('Target') or ''
            if not ext:
                t = posixpath.normpath(t.lstrip('/')) if t.startswith('/') else posixpath.normpath(posixpath.join(d, t))
            out.append((r.get('Id'), r.get('Type'), t, ext))
        return out

    def rel_targets(self, part, rtype):
        return [t for _i, ty, t, ext in self.rels(part) if ty == rtype and not ext]

    def one(self, part, rtype):
        t = self.rel_targets(part, rtype)
        return t[0] if len(t) == 1 and t[0] in self.names else None

    def slides(self):
        """Slide parts in p:sldIdLst order: [(sldId, part or None)]."""
        root = self.xml(self.pres) if self.pres else None
        if root is None:
            return []
        rid = {i: t for i, ty, t, ext in self.rels(self.pres) if not ext}
        lst = root.find(q('p:sldIdLst'))
        out = []
        for s in (lst if lst is not None else []):
            t = rid.get(s.get(q('r:id')))
            out.append((s.get('id'), t if t in self.names else None))
        return out

    def chain(self, slide):
        """(layout, master, theme) part names for a slide (None where missing)."""
        lay = self.one(slide, RT_LAYOUT)
        mst = self.one(lay, RT_MASTER) if lay else None
        thm = self.one(mst, RT_THEME) if mst else None
        return lay, mst, thm


SHAPES = ('p:sp', 'p:pic', 'p:graphicFrame', 'p:cxnSp', 'p:grpSp', 'p:contentPart')
SHAPE_TAGS = {q(t) for t in SHAPES}


def walk_shapes(sptree, depth=0, in_group=False):
    """Yields (shape element, depth, inside_group) for the shape tree in
    document order; groups before members; AC content is skipped (Phase 0
    treats an AC as one opaque entry)."""
    if sptree is None:
        return
    for c in sptree:
        if c.tag in SHAPE_TAGS:
            yield c, depth, in_group
            if c.tag == q('p:grpSp'):
                yield from walk_shapes(c, depth + 1, True)


def ph_of(shape):
    """The p:ph element of a shape (first nv*Pr/p:nvPr/p:ph), or None."""
    for nv in shape:
        if nv.tag.startswith('{%s}nv' % NS['p']):
            nvpr = nv.find(q('p:nvPr'))
            return nvpr.find(q('p:ph')) if nvpr is not None else None
    return None


def ph_key(ph):
    return ph.get('type', 'obj'), int(ph.get('idx', '0'))


def sptree(root):
    if root is None:
        return None
    csld = root.find(q('p:cSld'))
    return csld.find(q('p:spTree')) if csld is not None else None
