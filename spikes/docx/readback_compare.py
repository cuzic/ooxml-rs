"""Compare candidate readers' text against an ElementTree ground truth.

Usage: python3 -I readback_compare.py <out dir of readback spike> <file.docx>...

Ground truth per file: every w:t in word/document.xml's w:body, split into
(context, text) where context is the nearest ancestor among ins/del/hyperlink/
sdtContent/smartTag/fldSimple/txbxContent/AlternateContent-Fallback, else 'plain'.
A reader "covers" a w:t if its whitespace-stripped text occurs in the reader's
whitespace-stripped output. Reported per context, so we see which wrappers a
reader drops. Read-only; the .docx files are untrusted data and only parsed.
"""
import sys, os, json, zipfile, collections, re
import xml.etree.ElementTree as ET

W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'
CTX = {'{%s}%s' % (W, x): x for x in ('ins', 'del', 'hyperlink', 'sdtContent', 'smartTag', 'fldSimple', 'txbxContent', 'customXml')}
CTX['{%s}Fallback' % MC] = 'mc:Fallback'
CTX['{%s}Choice' % MC] = 'mc:Choice'


def ground(path):
    z = zipfile.ZipFile(path)
    raw = z.read('word/document.xml')
    if b'<!DOCTYPE' in raw[:4096]: return None
    root = ET.fromstring(raw)
    parent = {c: p for p in root.iter() for c in p}
    out = []
    for t in root.iter('{%s}t' % W):
        if not (t.text or '').strip(): continue
        ctx = 'plain'; p = parent.get(t)
        while p is not None:
            if p.tag in CTX: ctx = CTX[p.tag]; break
            p = parent.get(p)
        out.append((ctx, re.sub(r'\s+', '', t.text)))
    return out


def json_texts(o, acc):
    if isinstance(o, dict):
        for k, v in o.items():
            if k == 'text' and isinstance(v, str): acc.append(v)
            else: json_texts(v, acc)
    elif isinstance(o, list):
        for v in o: json_texts(v, acc)


def main():
    out = sys.argv[1]
    tot = {'docx-rs': collections.Counter(), 'docx-rust': collections.Counter()}
    hit = {'docx-rs': collections.Counter(), 'docx-rust': collections.Counter()}
    per_file = []
    for f in sys.argv[2:]:
        name = os.path.basename(f)
        try: g = ground(f)
        except Exception: continue
        if not g: continue
        texts = {}
        pj = os.path.join(out, name + '.docx-rs.json')
        if os.path.exists(pj):
            acc = []; json_texts(json.load(open(pj)), acc); texts['docx-rs'] = re.sub(r'\s+', '', ''.join(acc))
        pt = os.path.join(out, name + '.docx-rust.txt')
        if os.path.exists(pt): texts['docx-rust'] = re.sub(r'\s+', '', open(pt).read())
        row = [name]
        for lib in ('docx-rs', 'docx-rust'):
            if lib not in texts: row.append('%s=n/a' % lib); continue
            h = 0
            for ctx, s in g:
                tot[lib][ctx] += 1
                if s in texts[lib]: hit[lib][ctx] += 1; h += 1
            row.append('%s=%d/%d' % (lib, h, len(g)))
        per_file.append(row)
    if len(sys.argv) <= 40:
        for r in per_file: print('\t'.join(r))
    for lib in tot:
        print('\n%s coverage of w:t by context (hit/total):' % lib)
        for ctx in sorted(tot[lib]): print('  %-14s %6d / %6d' % (ctx, hit[lib][ctx], tot[lib][ctx]))


if __name__ == '__main__':
    main()
