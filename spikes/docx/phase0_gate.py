"""Apply ADR-0004's proposed Phase0 open-time gates and per-run checks to real files.

Usage: python3 -I phase0_gate.py <dir or file>... [--list]

Quantifies how much of a corpus Phase0 could edit, mirroring ADR-0001's
"formula-free workbooks only" consequence. Read-only (zipfile + ElementTree);
inputs are untrusted data and never executed.

Document-level gates (first that trips wins):
  EncryptedPackage   - not a zip; CFB magic D0 CF 11 E0 (password-protected .docx)
  NotZip             - neither zip nor CFB
  DtdNotAllowed      - any .xml/.rels part contains <!DOCTYPE
  StrictNotSupported - main part root in the Strict WML namespace
  TrackRevisionsOn   - settings.xml w:trackRevisions with ST_OnOff true
  StaleFieldRisk     - any story part has a field whose cached result depends on
                       document content (TOC, REF, PAGEREF, NOTEREF, SEQ, STYLEREF,
                       =, IF, NUMWORDS, NUMCHARS, INDEX, TOA, LISTNUM, or a code we
                       cannot classify)
Per run (document.xml, direct w:r children of a w:p whose ancestors up to w:body
are only p/hyperlink/tbl/tr/tc): settable if its children are only rPr, t, tab,
br (no type or textWrapping), cr, noBreakHyphen, softHyphen, lastRenderedPageBreak,
and it is not between fldChar begin and end.
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
        'INCLUDEPICTURE', 'INCLUDETEXT', 'LINK', 'EMBED', 'AUTONUM', 'AUTONUMLGL', 'AUTONUMOUT', 'BARCODE',
        'DISPLAYBARCODE', 'ASK', 'FILLIN', 'SET', 'NUMBERING', 'INFO', 'EDITTIME', 'REVNUM', 'DOCPROPERTY'}
ON = {None, 'true', '1', 'on'}
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
        if n.endswith(('.xml', '.rels')) and b'<!DOCTYPE' in z.read(n)[:4096]: return 'DtdNotAllowed', None
    if 'word/document.xml' not in names: return 'NoMainPart', None
    if WS.encode() in z.read('word/document.xml')[:4096]: return 'StrictNotSupported', None
    if 'word/settings.xml' in names:
        s = ET.fromstring(z.read('word/settings.xml'))
        tr = s.find(w('trackRevisions'))
        if tr is not None and tr.get(w('val')) in ON: return 'TrackRevisionsOn', None
    codes = collections.Counter()
    for n in names:
        if not STORY.match(n): continue
        root = ET.fromstring(z.read(n))
        cur = None
        for el in root.iter():
            if el.tag == w('fldSimple'): codes[field_code(el.get(w('instr')) or '')] += 1
            elif el.tag == w('fldChar'):
                t = el.get(w('fldCharType'))
                if t == 'begin': cur = ''  # outermost-first is enough to classify nested codes too
                elif t in ('separate', 'end') and cur is not None: codes[field_code(cur)] += 1; cur = None
            elif el.tag == w('instrText') and cur is not None: cur += el.text or ''
    bad = sorted(c for c in codes if c in CONTENT_DEPENDENT or (c not in SAFE))
    if bad: return 'StaleFieldRisk', bad
    return 'Editable', runs_stats(z)


def runs_stats(z):
    root = ET.fromstring(z.read('word/document.xml'))
    parent = {c: p for p in root.iter() for c in p}
    st = collections.Counter()
    body = root.find(w('body'))
    if body is not None and any(k.tag == '{%s}AlternateContent' % MC for k in body): st['append_refused_AC_in_body'] += 1
    for p in root.iter(w('p')):
        in_field = 0
        for r in p:
            if r.tag != w('r'): continue
            kids = list(r)
            for k in kids:
                if k.tag == w('fldChar'):
                    t = k.get(w('fldCharType'))
                    in_field += 1 if t == 'begin' else -1 if t == 'end' else 0
            if not any(k.tag == w('t') for k in kids): continue
            st['text_runs'] += 1
            a = parent.get(p); ok_ctx = True
            while a is not None and a.tag != w('body'):
                if a.tag not in OK_ANCESTORS: ok_ctx = False; break
                a = parent.get(a)
            if not ok_ctx: st['refused: context (sdt/ins/del/smartTag/txbx/...)'] += 1; continue
            if in_field > 0: st['refused: inside complex field'] += 1; continue
            if all(k.tag in TEXT_ONLY or (k.tag == w('br') and k.get(w('type')) in (None, 'textWrapping')) for k in kids):
                st['settable'] += 1
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
