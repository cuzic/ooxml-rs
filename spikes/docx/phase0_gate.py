"""Apply ADR-0004's (revision 2) Phase0 open-time gates and per-run checks to real files.

Usage: python3 -I phase0_gate.py <dir or file>... [--list]

Quantifies how much of a corpus Phase0 could edit, mirroring ADR-0001's
"formula-free workbooks only" consequence. Read-only (zipfile + ElementTree);
inputs are untrusted data and never executed.

Document-level gates (first that trips wins):
  EncryptedPackage   - not a zip; CFB magic D0 CF 11 E0 (password-protected .docx)
  NotZip             - neither zip nor CFB
  DtdNotAllowed      - any .xml/.rels part contains <!DOCTYPE (approximation of the
                       parser-level check in ooxml-core)
  StrictNotSupported - main part root in the Strict WML namespace
  TrackRevisionsOn   - settings.xml w:trackRevisions with ST_OnOff true (absent val = on)
  DocumentProtected  - settings.xml w:documentProtection with enforcement on and
                       edit other than none
  StaleFieldRisk     - any story part has a field whose cached result depends on
                       document content, or a code we cannot classify. Fields are
                       tracked with a STACK in document order; every nesting level is
                       classified (rev 2, review M1).
Per run (every w:r reachable in the body flow of document.xml, review m3): settable if
every ancestor up to w:body is p/hyperlink/tbl/tr/tc, its children are only rPr, t,
tab, br (no type other than textWrapping, no clear), cr, noBreakHyphen, softHyphen,
lastRenderedPageBreak, and it is not inside a complex field. Field state runs over the
whole body in document order (review m2), across paragraphs and wrappers.
Append: refused if w:body has an mc:AlternateContent child.
"""
import sys, os, re, zipfile, collections
import xml.etree.ElementTree as ET

W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
WS = 'http://purl.oclc.org/ooxml/wordprocessingml/main'
MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'


def w(l): return '{%s}%s' % (W, l)


STORY = re.compile(r'word/(document|header\d*|footer\d*|footnotes|endnotes|comments|footer|header)\.xml$')
CONTENT_DEPENDENT = {'TOC', 'REF', 'PAGEREF', 'NOTEREF', 'SEQ', 'STYLEREF', '=', 'IF', 'NUMWORDS', 'NUMCHARS',
                     'INDEX', 'TOA', 'LISTNUM', 'TC', 'XE', 'RD', 'QUOTE', 'COMPARE', 'BIBLIOGRAPHY', 'CITATION'}
SAFE = {'PAGE', 'NUMPAGES', 'SECTIONPAGES', 'SECTION', 'DATE', 'TIME', 'CREATEDATE', 'SAVEDATE', 'PRINTDATE',
        'AUTHOR', 'TITLE', 'SUBJECT', 'KEYWORDS', 'COMMENTS', 'FILENAME', 'FILESIZE', 'LASTSAVEDBY', 'TEMPLATE',
        'DOCPROPERTY', 'DOCVARIABLE', 'USERNAME', 'USERINITIALS', 'USERADDRESS', 'HYPERLINK', 'MERGEFIELD',
        'MERGEREC', 'MERGESEQ', 'NEXT', 'NEXTIF', 'SKIPIF', 'GREETINGLINE', 'ADDRESSBLOCK', 'FORMTEXT',
        'FORMCHECKBOX', 'FORMDROPDOWN', 'EQ', 'SYMBOL', 'MACROBUTTON', 'GOTOBUTTON', 'ADVANCE', 'PRIVATE',
        'INCLUDEPICTURE', 'INCLUDETEXT', 'LINK', 'EMBED', 'BARCODE',
        'DISPLAYBARCODE', 'ASK', 'FILLIN', 'SET', 'INFO', 'EDITTIME', 'REVNUM'}
# rev 2: AUTONUM/AUTONUMLGL/AUTONUMOUT moved out of SAFE (they count paragraphs, like SEQ),
# NUMBERING removed (not a Word field code). Neither occurs in the corpora.
CONTENT_DEPENDENT |= {'AUTONUM', 'AUTONUMLGL', 'AUTONUMOUT'}
OFF = {'false', '0', 'off'}  # ST_OnOff; any other value (absent, true, 1, on, invalid) is on
REVISED_INSTR = '<revised instruction>'  # delInstrText in a field's own instruction: fail closed
TEXT_ONLY = {w(x) for x in ('rPr', 't', 'tab', 'cr', 'noBreakHyphen', 'softHyphen', 'lastRenderedPageBreak')}
OK_ANCESTORS = {w(x) for x in ('p', 'hyperlink', 'tbl', 'tr', 'tc', 'body')}


def field_code(s):
    s = s.strip()
    if s.startswith('='): return '='
    return s.split(' ')[0].split('\\')[0].upper() if s else ''


def gate(path):
    with open(path, 'rb') as fh: head = fh.read(8)
    if head.startswith(b'\xd0\xcf\x11\xe0'): return 'EncryptedPackage', None
    try: z = zipfile.ZipFile(path); names = z.namelist()
    except Exception: return 'NotZip', None
    for n in names:
        if n.endswith(('.xml', '.rels')) and b'<!DOCTYPE' in z.read(n): return 'DtdNotAllowed', None
    if 'word/document.xml' not in names: return 'NoMainPart', None
    if WS.encode() in z.read('word/document.xml')[:4096]: return 'StrictNotSupported', None
    if 'word/settings.xml' in names:
        s = ET.fromstring(z.read('word/settings.xml'))
        tr = s.find(w('trackRevisions'))
        if tr is not None and tr.get(w('val')) not in OFF: return 'TrackRevisionsOn', None
        dp = s.find(w('documentProtection'))
        if dp is not None and dp.get(w('enforcement')) not in OFF | {None} and dp.get(w('edit')) not in (None, 'none'):
            return 'DocumentProtected', [dp.get(w('edit'))]
    codes = collections.Counter()
    for n in names:
        if not STORY.match(n): continue
        codes.update(field_codes(ET.fromstring(z.read(n))))
    bad = sorted(c for c in codes if c in CONTENT_DEPENDENT or (c not in SAFE))
    if bad: return 'StaleFieldRisk', bad
    return 'Editable', runs_stats(z)


def field_codes(root):
    """Every field's code, all nesting levels (review M1). A level's code is the
    instrText seen while that level is the innermost open field and before its own
    separate; a nested field's result (written by Word as instrText) therefore stays
    out of the outer code."""
    codes = collections.Counter()
    stack = []  # [instruction, separated, revised]
    for el in root.iter():
        if el.tag == w('fldSimple'): codes[field_code(el.get(w('instr')) or '')] += 1
        elif el.tag == w('fldChar'):
            t = el.get(w('fldCharType'))
            if t == 'begin': stack.append(['', False, False])
            elif t == 'separate' and stack: stack[-1][1] = True
            elif t == 'end' and stack:
                ins, _, rev = stack.pop(); codes[REVISED_INSTR if rev else field_code(ins)] += 1
        elif el.tag in (w('instrText'), w('delInstrText')) and stack and not stack[-1][1]:
            stack[-1][0] += el.text or ''
            if el.tag == w('delInstrText'): stack[-1][2] = True
    for ins, _, rev in stack:  # unterminated fields are still classified
        codes[REVISED_INSTR if rev else field_code(ins)] += 1
    return codes


def runs_stats(z):
    root = ET.fromstring(z.read('word/document.xml'))
    parent = {c: p for p in root.iter() for c in p}
    st = collections.Counter()
    body = root.find(w('body'))
    if body is None: return st
    if any(k.tag == '{%s}AlternateContent' % MC for k in body): st['append_refused_AC_in_body'] += 1
    skip = set()  # subtrees outside the body flow (drawings, VML, OLE, AC)
    depth = 0
    for el in body.iter():
        if el in skip: continue
        if el.tag in (w('drawing'), w('pict'), w('object'), '{%s}AlternateContent' % MC):
            skip.update(el.iter()); continue
        if el.tag != w('r'): continue
        kids = list(el)
        before = depth
        for k in kids:
            if k.tag == w('fldChar'):
                t = k.get(w('fldCharType'))
                if t == 'begin': depth += 1
                elif t == 'end': depth = max(0, depth - 1)
        if not any(k.tag == w('t') for k in kids): continue
        st['text_runs'] += 1
        a = parent[el]; ok_ctx = True
        while a is not body:
            if a.tag not in OK_ANCESTORS: ok_ctx = False; break
            a = parent[a]
        if not ok_ctx: st['refused: context (sdt/ins/del/smartTag/...)'] += 1; continue
        if before > 0 or depth > 0 or any(k.tag == w('fldChar') for k in kids): st['refused: inside complex field'] += 1; continue
        if all(k.tag in TEXT_ONLY or (k.tag == w('br') and k.get(w('type')) in (None, 'textWrapping')
                                      and k.get(w('clear')) in (None, 'none')) for k in kids):
            st['settable'] += 1
            if parent[el].tag == w('hyperlink'): st['settable (in w:hyperlink)'] += 1
        else:
            st['refused: run has non-text child'] += 1
    return st


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    files = []
    for a in args:
        if os.path.isdir(a): files += [os.path.join(a, f) for f in sorted(os.listdir(a)) if f.lower().endswith(('.docx', '.docm', '.dotx'))]
        else: files.append(a)
    res = collections.Counter(); runs = collections.Counter(); why = collections.Counter()
    for f in files:
        try: g, extra = gate(f)
        except Exception as e: g, extra = 'CorruptPackage', None  # bad CRC, truncated part, malformed XML
        res[g] += 1
        if g == 'Editable': runs.update(extra)
        if g == 'StaleFieldRisk':
            for c in extra: why[c] += 1
        if '--list' in sys.argv: print('%-22s %s %s' % (g, os.path.basename(f), extra if g != 'Editable' else dict(extra)))
    print('files: %d' % len(files))
    for k, v in res.most_common(): print('  %-22s %5d' % (k, v))
    print('StaleFieldRisk codes (files):', dict(why.most_common()))
    print('text runs in Editable files:')
    for k, v in sorted(runs.items()): print('  %-50s %7d' % (k, v))


if __name__ == '__main__':
    main()
