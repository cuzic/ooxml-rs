"""How text is split across runs in slide paragraphs (run with `python3 -I`).

Usage: python3 -I runs.py <pptx files/dirs...>

For each a:p in ppt/slides/*.xml with >= 2 text-bearing children (a:r/a:fld):
  - 'same_rPr'      : all a:r have identical rPr once the volatile attributes
                      lang/altLang/dirty/err/smtClean/smtId/noProof are dropped
                      (split for proofing/IME reasons; merging loses nothing visible)
  - 'diff_rPr'      : runs differ in visible formatting (merging would lose it)
  - 'has_fld'       : paragraph contains a:fld (slide number, date: cached text)
  - 'has_br'        : paragraph contains a:br (soft line break)
Also counts text that matches a search string only across a run boundary,
using every 2..3-word window of the paragraph text as a probe (rough proxy for
"find/replace that spans runs").
"""
import sys, os, re, zipfile, collections
import xml.etree.ElementTree as ET

A = '{http://schemas.openxmlformats.org/drawingml/2006/main}'
VOLATILE = {'lang', 'altLang', 'dirty', 'err', 'smtClean', 'smtId', 'noProof'}

def norm(rpr):
    if rpr is None:
        return ('',)
    attrs = tuple(sorted((k, v) for k, v in rpr.attrib.items() if k not in VOLATILE))
    kids = ET.tostring(rpr, encoding='unicode')
    kids = re.sub(r'<[^>]*rPr[^>]*>', '', kids)  # drop the rPr tag itself
    return (attrs, kids)

def main(argv):
    files = []
    for a in argv:
        files += [os.path.join(a, n) for n in sorted(os.listdir(a)) if n.endswith('.pptx')] if os.path.isdir(a) else [a]
    c = collections.Counter()
    for p in files:
        try:
            z = zipfile.ZipFile(p)
        except Exception:
            continue
        for n in z.namelist():
            if not re.match(r'ppt/slides/slide\d+\.xml$', n):
                continue
            try:
                root = ET.fromstring(z.read(n))
            except Exception:
                continue
            for para in root.iter(A + 'p'):
                kids = [k for k in para if k.tag in (A + 'r', A + 'fld')]
                if not kids:
                    continue
                c['para_with_text'] += 1
                if any(k.tag == A + 'br' for k in para):
                    c['has_br'] += 1
                if any(k.tag == A + 'fld' for k in kids):
                    c['has_fld'] += 1
                runs = [k for k in kids if k.tag == A + 'r']
                if len(kids) < 2:
                    continue
                c['multi'] += 1
                if len({norm(r.find(A + 'rPr')) for r in runs}) <= 1:
                    c['multi_same_rPr'] += 1
                else:
                    c['multi_diff_rPr'] += 1
                texts = [(r.findtext(A + 't') or '') for r in runs]
                full = ''.join(texts)
                bounds = set(); pos = 0
                for t in texts[:-1]:
                    pos += len(t); bounds.add(pos)
                words = [(m.start(), m.end()) for m in re.finditer(r'\S+', full)]
                for w in (2, 3):
                    for i in range(len(words) - w + 1):
                        s, e = words[i][0], words[i + w - 1][1]
                        c['probe_%d' % w] += 1
                        if any(s < b < e for b in bounds):
                            c['probe_%d_crosses_run' % w] += 1
    for k in sorted(c):
        print('%-24s %d' % (k, c[k]))

if __name__ == '__main__':
    main(sys.argv[1:])
