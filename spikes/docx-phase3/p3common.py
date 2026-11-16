"""Shared helpers for the Word Phase2 spike (ADR-0011); copied from spikes/docx-phase1/p1common.py (PR #25). Throwaway code, not production.

Inputs are untrusted .docx files. They are only read with zipfile + ElementTree
(expat, no external entity resolution); any part containing a DOCTYPE is skipped, as
ooxml-core refuses it (ADR-0004 O2). Run every script with `python3 -I` from a
directory other than the inputs' directory. Scripts import this module by inserting
their own (trusted) directory into sys.path, because -I disables that.

Model (what ADR-0007 proposes, simplified where noted):
- stories: main document, headers, footers, footnotes, endnotes, comments, found
  through the main part's relationships (any part name).
- the block walk descends body/hdr/ftr/footnote/endnote/comment -> tbl/tr/tc,
  block-level sdt/sdtContent and customXml (ADR-0004 §3.1); drawings, VML, OLE,
  mc:AlternateContent, math and text boxes are not flow.
- the paragraph walk yields a token stream: ('run', RunInfo) | ('marker', tag) |
  ('enter', tag, el) | ('exit', tag, el) | ('hard', tag) for anything else.
- field state is one counter per story in document order (ADR-0004 R1).
"""
import collections
import os
import posixpath
import re
import zipfile
import xml.etree.ElementTree as ET

W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
WS = 'http://purl.oclc.org/ooxml/wordprocessingml/main'
MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'
M = 'http://schemas.openxmlformats.org/officeDocument/2006/math'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
PR = 'http://schemas.openxmlformats.org/package/2006/relationships'
XML_NS = 'http://www.w3.org/XML/1998/namespace'
RT = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'


def w(l):
    return '{%s}%s' % (W, l)


def local(tag):
    return tag.rsplit('}', 1)[-1]


def ns(tag):
    return tag[1:].split('}', 1)[0] if tag.startswith('{') else ''


STORY_TYPES = {RT + 'header': 'header', RT + 'footer': 'footer', RT + 'footnotes': 'footnotes',
               RT + 'endnotes': 'endnotes', RT + 'comments': 'comments'}

# run content that Phase0's set_run_text accepts (R4), and its text mapping
TEXT_CHILD = {w('t'), w('tab'), w('br'), w('cr'), w('noBreakHyphen'), w('softHyphen'),
              w('lastRenderedPageBreak')}
REVISION_WRAPPERS = {w('ins'), w('del'), w('moveFrom'), w('moveTo')}
# paragraph-level wrappers the walk descends into (ADR-0004 §3.1)
RUN_WRAPPERS = {w('hyperlink'), w('ins'), w('del'), w('moveFrom'), w('moveTo'), w('smartTag'), w('sdt'),
                w('sdtContent'), w('customXml'), w('fldSimple'), w('dir'), w('bdo')}
# paragraph-level elements that carry no text and mark a position
PROOF = {w('proofErr')}
ANCHORS = {w('bookmarkStart'), w('bookmarkEnd'), w('commentRangeStart'), w('commentRangeEnd'),
           w('permStart'), w('permEnd'), w('moveFromRangeStart'), w('moveFromRangeEnd'),
           w('moveToRangeStart'), w('moveToRangeEnd'), w('customXmlInsRangeStart'),
           w('customXmlInsRangeEnd'), w('customXmlDelRangeStart'), w('customXmlDelRangeEnd'),
           w('customXmlMoveFromRangeStart'), w('customXmlMoveFromRangeEnd'),
           w('customXmlMoveToRangeStart'), w('customXmlMoveToRangeEnd')}
NOT_FLOW = {w('drawing'), w('pict'), w('object'), '{%s}AlternateContent' % MC, w('txbxContent')}
BLOCK_DESCEND = {w('tbl'), w('tr'), w('tc'), w('sdt'), w('sdtContent'), w('customXml')}


def max_depth(root):
    best, stack = 0, [(root, 1)]
    while stack:
        el, d = stack.pop()
        best = max(best, d)
        if best > 256:
            return best
        stack.extend((c, d + 1) for c in el)
    return best


def has_dtd(b):
    return b'<!DOCTYPE' in b or b'<!doctype' in b


class Pkg:
    """A .docx opened read-only, with its stories located by relationship."""

    def __init__(self, path):
        self.path = path
        self.name = os.path.basename(path)
        self.err = None
        self.z = None
        try:
            with open(path, 'rb') as fh:
                head = fh.read(8)
            if head.startswith(b'\xd0\xcf\x11\xe0'):
                self.err = 'EncryptedPackage'; return
            self.z = zipfile.ZipFile(path)
            self.names = set(self.z.namelist())
        except Exception:
            self.err = 'NotZip'; return
        try:
            for n in self.names:
                if n.endswith(('.xml', '.rels')) and has_dtd(self.z.read(n)):
                    self.err = 'DtdNotAllowed'; return
            self.main = self._main()
            if self.main is None:
                self.err = 'MainPartMissing'; return
            mb = self.z.read(self.main)
            if WS.encode() in mb[:4096]:
                self.err = 'StrictNotSupported'; return
            self.rels = self._rels(self.main)
            self.doc = ET.fromstring(mb)
            if max_depth(self.doc) > 256:  # ADR-0004 O5 (core's default limit)
                self.err = 'TooDeep'; return
        except Exception as e:  # corrupt zip entries, bad XML
            self.err = 'Corrupt:' + type(e).__name__; return

    def _rels(self, part):
        d, b = posixpath.split(part)
        rp = posixpath.join(d, '_rels', b + '.rels')
        out = []
        if rp not in self.names:
            return out
        for r in ET.fromstring(self.z.read(rp)):
            if r.get('TargetMode') == 'External':
                continue
            t = r.get('Target') or ''
            tgt = t.lstrip('/') if t.startswith('/') else posixpath.normpath(posixpath.join(d, t))
            out.append((r.get('Id'), r.get('Type'), tgt))
        return out

    def _main(self):
        if '_rels/.rels' not in self.names:
            return None
        for r in ET.fromstring(self.z.read('_rels/.rels')):
            if (r.get('Type') or '').endswith('/officeDocument'):
                t = (r.get('Target') or '').lstrip('/')
                return t if t in self.names else None
        return None

    def part(self, rtype_suffix):
        for _, t, tgt in self.rels:
            if t == RT + rtype_suffix and tgt in self.names:
                return tgt
        return None

    def parts(self, rtype_suffix):
        return [tgt for _, t, tgt in self.rels if t == RT + rtype_suffix and tgt in self.names]

    def xml(self, name):
        return ET.fromstring(self.z.read(name))

    def stories(self):
        """[(kind, part name, root element)], main first, then by relationship order."""
        out = [('main', self.main, self.doc)]
        seen = {self.main}
        for _, t, tgt in self.rels:
            k = STORY_TYPES.get(t)
            if k and tgt in self.names and tgt not in seen:
                seen.add(tgt)
                try:
                    root = self.xml(tgt)
                except ET.ParseError:
                    continue
                if max_depth(root) > 256:
                    self.err = 'TooDeep'
                    return []
                out.append((k, tgt, root))
        return out


# ---------------------------------------------------------------- settings gates

OFF = {'false', '0', 'off'}


def onoff_on(el):
    return el is not None and el.get(w('val')) not in OFF


def settings_flags(pkg):
    f = {'track': False, 'protect': None, 'updateFields': None, 'mailMerge': False}
    for sp in pkg.parts('settings'):
        s = pkg.xml(sp)
        for el in s.findall(w('trackRevisions')):
            f['track'] |= onoff_on(el)
        for dp in s.findall(w('documentProtection')):
            enf = dp.get(w('enforcement')) or dp.get(w('enforcment'))
            if enf not in OFF | {None} and dp.get(w('edit')) not in (None, 'none'):
                f['protect'] = f['protect'] or dp.get(w('edit'))
        for el in s.findall(w('updateFields')):
            f['updateFields'] = onoff_on(el)
        if s.find(w('mailMerge')) is not None:
            f['mailMerge'] = True
    return f


# ---------------------------------------------------------------- fields (ADR-0004 W2, simplified port)

SAFE = set('''PAGE NUMPAGES SECTIONPAGES SECTION DATE TIME CREATEDATE SAVEDATE PRINTDATE EDITTIME AUTHOR TITLE
SUBJECT KEYWORDS COMMENTS FILENAME LASTSAVEDBY TEMPLATE DOCVARIABLE USERNAME USERINITIALS USERADDRESS REVNUM
HYPERLINK INCLUDEPICTURE INCLUDETEXT LINK EMBED MERGEFIELD MERGEREC MERGESEQ NEXT NEXTIF SKIPIF GREETINGLINE
ADDRESSBLOCK FORMTEXT FORMCHECKBOX FORMDROPDOWN ASK FILLIN SET EQ SYMBOL MACROBUTTON GOTOBUTTON ADVANCE PRIVATE
BARCODE DISPLAYBARCODE'''.split())
DOCPROPERTY_SAFE = set('''AUTHOR CATEGORY COMMENTS COMPANY CREATETIME HYPERLINKBASE KEYWORDS LASTPRINTED
LASTSAVEDBY LASTSAVEDTIME MANAGER NAMEOFAPPLICATION PAGES REVISIONNUMBER SECURITY SUBJECT TEMPLATE TITLE
TOTALEDITINGTIME'''.split())
QUOTES = '"“”„‟＂'


def field_code(s):
    s = s.strip()
    if s.startswith('='):
        return '='
    if not s:
        return ''
    return re.split(r'[\s\\]', s, 1)[0].upper()


def first_arg(s):
    s = s.strip()
    parts = re.split(r'\s+', s, 1)
    if len(parts) < 2:
        return None
    rest = parts[1].strip()
    if rest[:1] in QUOTES:
        m = re.match('[%s]([^%s]*)[%s]' % (QUOTES, QUOTES, QUOTES), rest)
        return m.group(1).upper() if m else None
    if rest.startswith('\\'):
        return None
    return re.split(r'\s', rest, 1)[0].upper()


def field_key(instr, nested=False):
    c = field_code(instr)
    if c in ('INFO', 'DOCPROPERTY'):
        a = first_arg(instr)
        return c + (' ' + a if a else '')
    if c == 'BARCODE' and re.search(r'\\\s*b(\s|$)', instr, re.I):
        return 'BARCODE \\b'
    if c in ('INCLUDETEXT', 'INCLUDEPICTURE', 'LINK') and (nested or not first_arg(instr)):
        return c + ' (non-literal source)'
    return c


def key_safe(k, custom=()):
    if k in SAFE:
        return True
    if k.startswith('INFO '):
        return k[5:] in SAFE
    if k.startswith('DOCPROPERTY '):
        a = k[12:]
        return a in DOCPROPERTY_SAFE or a in custom
    return False


class Field:
    __slots__ = ('instr', 'separated', 'revised', 'nested', 'children', 'key', 'story')

    def __init__(self, story):
        self.instr = ''; self.separated = False; self.revised = False; self.nested = False
        self.children = []; self.key = None; self.story = story


def scan_fields(root, story=''):
    """All fields of one story, every nesting level: [Field], with .children (nested fields
    in the instruction) and .key. Simplified port of ADR-0004 W2's stack scan."""
    out, stack = [], []
    for el in root.iter():
        if el.tag == w('fldSimple'):
            f = Field(story); f.instr = el.get(w('instr')) or ''; f.key = field_key(f.instr); out.append(f)
            if stack and not stack[-1].separated:
                stack[-1].children.append(f); stack[-1].nested = True; stack[-1].instr += ' \x01 '
        elif el.tag == w('fldChar'):
            t = el.get(w('fldCharType'))
            if t == 'begin':
                f = Field(story)
                if stack and not stack[-1].separated:
                    stack[-1].children.append(f); stack[-1].nested = True
                    stack[-1].instr += ' \x01 '  # placeholder for the nested field's result
                stack.append(f)
            elif t == 'separate' and stack:
                stack[-1].separated = True
            elif t == 'end' and stack:
                f = stack.pop(); f.key = '<revised instruction>' if f.revised else field_key(f.instr, f.nested)
                out.append(f)
            else:
                out.append(_malformed(story))
        elif el.tag in (w('instrText'), w('delInstrText')):
            if stack and not stack[-1].separated:
                stack[-1].instr += el.text or ''
                if el.tag == w('delInstrText'):
                    stack[-1].revised = True
                    # nested-field placeholder so the IF tokenizer sees it
            elif not stack:
                out.append(_malformed(story))
    for f in stack:
        f.key = '<revised instruction>' if f.revised else field_key(f.instr, f.nested); out.append(f)
    return out


def _malformed(story):
    f = Field(story); f.key = '<malformed field>'; return f


def custom_props(pkg):
    """docProps/custom.xml names without linkTarget (upper-cased)."""
    out = set()
    for n in pkg.names:
        if n.endswith('docProps/custom.xml'):
            try:
                for p in pkg.xml(n):
                    if p.get('linkTarget') is None and p.get('name'):
                        out.add(p.get('name').upper())
            except ET.ParseError:
                pass
    return out


def gate(pkg):
    """Phase0 document gate: (name or None, detail)."""
    if pkg.err:
        return pkg.err, None
    f = settings_flags(pkg)
    if f['track']:
        return 'TrackRevisionsOn', None
    if f['protect']:
        return 'DocumentProtected', f['protect']
    custom = custom_props(pkg)
    bad = set()
    for kind, name, root in pkg.stories():
        for fl in scan_fields(root, kind):
            if not key_safe(fl.key, custom):
                bad.add(fl.key)
    if pkg.err:
        return pkg.err, None
    if bad:
        return 'StaleFieldRisk', sorted(bad)
    return None, None


# ---------------------------------------------------------------- walking stories

class RunInfo:
    __slots__ = ('el', 'wrappers', 'in_field', 'text', 'kinds', 'para', 'parent')

    def __init__(self, el, wrappers, in_field, parent):
        self.el = el; self.wrappers = wrappers; self.in_field = in_field; self.parent = parent
        self.kinds = [k.tag for k in el if k.tag != w('rPr')]
        self.text = run_text(el)
        self.para = None

    def rpr(self):
        return self.el.find(w('rPr'))

    def text_only(self):
        for k in self.el:
            if k.tag == w('rPr'):
                continue
            if k.tag not in TEXT_CHILD:
                return False
            if k.tag == w('br') and (k.get(w('type')) not in (None, 'textWrapping') or k.get(w('clear')) not in (None, 'none')):
                return False
        return True

    def has_text(self):
        return any(k.tag == w('t') for k in self.el)

    def has_fldchar(self):
        return any(k.tag in (w('fldChar'), w('instrText'), w('delInstrText')) for k in self.el)

    def context(self):
        """Phase0 RunContext name (outermost wrapper other than hyperlink)."""
        for wr in self.wrappers:
            if wr.tag != w('hyperlink'):
                return local(wr.tag)
        return 'hyperlink' if self.wrappers else 'direct'

    def refusal(self, phase1=False):
        """None if settable, else the Phase0 refusal (R1..R4). phase1=True also accepts
        runs whose only non-hyperlink wrapper is w:smartTag (ADR-0007 §1.6)."""
        if self.in_field or self.has_fldchar() or any(x.tag == w('fldSimple') for x in self.wrappers):
            return 'R1 InsideField'
        if any(x.tag in REVISION_WRAPPERS for x in self.wrappers):
            return 'R2 InsideRevision'
        ok = {w('hyperlink'), w('smartTag')} if phase1 else {w('hyperlink')}
        bad = [x for x in self.wrappers if x.tag not in ok]
        if bad:
            return 'R3 UnsupportedContext:' + local(bad[0].tag)
        if not self.text_only():
            return 'R4 RunNotTextOnly'
        rp = self.rpr()
        if rp is not None and rp.find(w('rPrChange')) is not None:
            return 'R2 InsideRevision:rPrChange'
        return None


def run_text(r):
    out = []
    for k in r:
        t = k.tag
        if t in (w('t'), w('delText')):
            out.append(k.text or '')
        elif t == w('tab'):
            out.append('\t')
        elif t in (w('br'), w('cr')):
            out.append('\n')
        elif t == w('noBreakHyphen'):
            out.append('‑')
        elif t == w('softHyphen'):
            out.append('­')
    return ''.join(out)


class Para:
    __slots__ = ('el', 'tokens', 'story', 'part', 'cell', 'note')

    def __init__(self, el, story, part, cell, note):
        self.el = el; self.tokens = []; self.story = story; self.part = part; self.cell = cell; self.note = note

    def runs(self):
        return [t[1] for t in self.tokens if t[0] == 'run']


class StoryWalk:
    """Walks one story: paragraphs (with token streams) and cells, field depth in document order."""

    def __init__(self, kind, part, root):
        self.kind, self.part, self.root = kind, part, root
        self.paras = []
        self.cells = []  # (tc element, [Para])
        self.depth = 0
        self._walk()

    def _walk(self):
        if self.kind == 'main':
            body = self.root.find(w('body'))
            tops = [(body, None)] if body is not None else []
        elif self.kind in ('header', 'footer'):
            tops = [(self.root, None)]
        else:  # footnotes/endnotes/comments: one container per note/comment
            tops = [(c, c) for c in self.root if local(c.tag) in ('footnote', 'endnote', 'comment')]
        for top, note in tops:
            self._blocks(top, None, note)

    def _blocks(self, el, cell, note):
        for c in el:
            if c.tag == w('p'):
                self._para(c, cell, note)
            elif c.tag == w('tc'):
                paras_before = len(self.paras)
                self._blocks(c, c, note)
                self.cells.append((c, self.paras[paras_before:]))
            elif c.tag in BLOCK_DESCEND:
                self._blocks(c, cell, note)
            # everything else at block level (sectPr, AC, markers) is not flow

    def _para(self, p, cell, note):
        para = Para(p, self.kind, self.part, cell, note)
        self.paras.append(para)
        self._inline(p, [], para)

    def _inline(self, el, wrappers, para):
        for c in el:
            t = c.tag
            if t == w('pPr'):
                continue
            if t == w('r'):
                ri = RunInfo(c, list(wrappers), self.depth > 0, el)
                ri.para = para
                for k in c:
                    if k.tag == w('fldChar'):
                        ft = k.get(w('fldCharType'))
                        if ft == 'begin':
                            self.depth += 1
                        elif ft == 'end':
                            self.depth = max(0, self.depth - 1)
                ri.in_field = ri.in_field or self.depth > 0
                para.tokens.append(('run', ri))
                # ruby base runs are a context of their own (R3); not modelled here
            elif t in RUN_WRAPPERS:
                para.tokens.append(('enter', t, c))
                self._inline(c, wrappers + [c] if t != w('sdtContent') else wrappers, para)
                para.tokens.append(('exit', t, c))
            elif t in PROOF:
                para.tokens.append(('proof', t))
            elif t in ANCHORS:
                para.tokens.append(('marker', t, c))
            elif t == w('sdtPr') or t == w('sdtEndPr') or t == w('smartTagPr') or t == w('customXmlPr'):
                continue
            else:
                para.tokens.append(('hard', t))


def iter_stories(pkg):
    for kind, part, root in pkg.stories():
        yield StoryWalk(kind, part, root)


# ---------------------------------------------------------------- run properties comparison

def canon(el):
    """Canonical form of an element: (tag, sorted attrs, children); ignores prefixes,
    attribute order and whitespace-only text."""
    if el is None:
        return None
    kids = tuple(canon(k) for k in el)
    txt = (el.text or '').strip()
    return (el.tag, tuple(sorted(el.attrib.items())), kids, txt)


PROOFING_RPR = {w('lang'), w('noProof')}


def canon_drop(el, drop, drop_attrs=()):
    if el is None:
        return None
    kids = []
    for k in el:
        if k.tag in drop:
            continue
        a = tuple(sorted((n, v) for n, v in k.attrib.items() if (k.tag, n) not in drop_attrs))
        kids.append((k.tag, a, tuple(canon(x) for x in k), (k.text or '').strip()))
    kids.sort()  # order inside rPr is schema-fixed; sorting only guards against producer variance
    return tuple(kids)


def rpr_bytes(pkg_bytes_cache, run):
    return ET.tostring(run.rpr(), encoding='unicode') if run.rpr() is not None else ''


def _rpr_norm(el, drop_children, drop_attrs):
    if el is None:
        return ()
    kids = []
    for k in el:
        if k.tag in drop_children:
            continue
        a = tuple(sorted((n, v) for n, v in k.attrib.items() if (k.tag, n) not in drop_attrs))
        sub = tuple(canon(x) for x in k)
        if not a and not sub and any(k.tag == t for t, _ in drop_attrs) and k.attrib:
            continue  # the element carried only dropped attributes
        kids.append((k.tag, a, sub, (k.text or '').strip()))
    kids.sort()
    return tuple(kids)


INVISIBLE_ATTRS = {(w('lang'), w('val')), (w('rFonts'), w('hint'))}


def rpr_class(a, b):
    """How two runs' rPr compare:
    'identical'  canonical equal (prefixes, attribute order, whitespace ignored);
    'proofing'   equal after dropping w:noProof and w:lang/@w:val (the Latin proofing
                 language; w:lang/@w:eastAsia and @w:bidi still count, they can change glyphs);
    'hint'       equal after also dropping w:rFonts/@w:hint;
    'different'  anything else."""
    ra, rb = a.rpr(), b.rpr()
    if _rpr_norm(ra, set(), set()) == _rpr_norm(rb, set(), set()):
        return 'identical'
    pa = {(w('lang'), w('val'))}
    if _rpr_norm(ra, {w('noProof')}, pa) == _rpr_norm(rb, {w('noProof')}, pa):
        return 'proofing'
    if _rpr_norm(ra, {w('noProof')}, INVISIBLE_ATTRS) == _rpr_norm(rb, {w('noProof')}, INVISIBLE_ATTRS):
        return 'hint'
    return 'different'


def rpr_diff_children(a, b):
    """Names of rPr children (with attrs) that differ between two runs."""
    def d(el):
        out = {}
        if el is None:
            return out
        for k in el:
            out[local(k.tag)] = (tuple(sorted(k.attrib.items())), tuple(canon(x) for x in k))
        return out
    da, db = d(a.rpr()), d(b.rpr())
    return sorted(k for k in set(da) | set(db) if da.get(k) != db.get(k))


def corpus_files(dirs):
    out = []
    for d in dirs:
        if os.path.isfile(d):
            out.append(d); continue
        for n in sorted(os.listdir(d)):
            if n.lower().endswith(('.docx', '.docm', '.dotx', '.dotm')):
                out.append(os.path.join(d, n))
    return out
