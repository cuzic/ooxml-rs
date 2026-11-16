"""Cross-run find/replace: corpus evidence and a reference model of the
ADR-0008 §4 rules (run with `python3 -I`).

Usage: python3 -I findreplace.py <pptx files/dirs...>

For every slide paragraph (shapes, group members, table cells; not inside
mc:AlternateContent) the paragraph text is the concatenation of its items
(a:r text, a:br = "\\v", a:fld cached text). Probes: every 1-, 2- and
3-word window of the paragraph text, searched literally (left to right,
non-overlapping, as str.replace). Each match is classified:

  single-run | cross-run same format | cross-run proofing-only difference
  | cross-run visible difference | crosses a:fld (refused) | crosses a:br
  (refused) | crosses runs with different a:hlinkClick

Then the reference model applies the rules to the item list (replacement
into the run where the match starts, keeping its a:rPr; fully covered runs
removed; partly covered runs trimmed; a run left empty is removed unless it
is the paragraph's only run) and checks two invariants for every applied
replacement:
  T  the new paragraph text == str.replace on the old text;
  F  every character keeps its old a:rPr, and every inserted character has
     the a:rPr of the run where its match started.
"""
import sys, os, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import xml.etree.ElementTree as ET
from common import Pkg, q, qn, iter_files, walk_shapes, sptree

PROOF = {'lang', 'altLang', 'err', 'dirty', 'smtClean', 'smtId', 'noProof', 'bmk'}
C = collections.Counter()


def canon(e, drop=()):
    if e is None:
        return ''
    a = sorted((k, v) for k, v in e.attrib.items() if k not in drop)
    return '%s%s[%s]' % (qn(e.tag), a, ''.join(canon(c) for c in e))


def link(rpr):
    if rpr is None:
        return None
    h = rpr.find(q('a:hlinkClick'))
    return None if h is None else (h.get(q('r:id')), h.get('action'))


def items_of(p):
    out = []
    for c in p:
        if c.tag == q('a:r'):
            t = c.find(q('a:t'))
            out.append({'k': 'r', 't': (t.text or '') if t is not None else '', 'rpr': c.find(q('a:rPr'))})
        elif c.tag == q('a:br'):
            out.append({'k': 'br', 't': '\v', 'rpr': c.find(q('a:rPr'))})
        elif c.tag == q('a:fld'):
            t = c.find(q('a:t'))
            out.append({'k': 'fld', 't': (t.text or '') if t is not None else '', 'rpr': c.find(q('a:rPr'))})
    return out


def text_bodies(root):
    for s, _d, _g in walk_shapes(sptree(root)):
        b = s.find(q('p:txBody'))
        if b is not None:
            yield b
        for tc in (s.iter(q('a:tc')) if s.tag == q('p:graphicFrame') else []):
            b = tc.find(q('a:txBody'))
            if b is not None:
                yield b


def spans(items):
    off, out = 0, []
    for i, it in enumerate(items):
        out.append((off, off + len(it['t']), i))
        off += len(it['t'])
    return out


def matches(text, pat):
    i, out = 0, []
    while pat:
        j = text.find(pat, i)
        if j < 0:
            break
        out.append((j, j + len(pat)))
        i = j + len(pat)
    return out


def covered(sp, s, e):
    return [i for a, b, i in sp if a < e and b > s or (a == b == s and False)]


def classify(items, sp, s, e, count=False):
    cov = covered(sp, s, e)
    kinds = {items[i]['k'] for i in cov}
    if 'fld' in kinds:
        return 'refused: crosses a:fld'
    if 'br' in kinds:
        return 'refused: crosses a:br'
    if len(cov) == 1:
        return 'single run'
    fm = {canon(items[i]['rpr']) for i in cov}
    fp = {canon(items[i]['rpr'], PROOF) for i in cov}
    lk = {link(items[i]['rpr']) for i in cov}
    tag = 'cross-run same format' if len(fm) == 1 else 'cross-run proofing-only difference' if len(fp) == 1 else 'cross-run visible difference'
    if len(lk) > 1 and count:
        C['match: cross-run, different hlinkClick (subset of the above)'] += 1
    return tag


LINKS = (q('a:hlinkClick'), q('a:hlinkMouseOver'))


def without_links(rpr):
    """The Phase 0.1 copy of a format source: the subtree without links."""
    if rpr is None:
        return None
    c = ET.Element(rpr.tag, dict(rpr.attrib))
    c.extend(ch for ch in rpr if ch.tag not in LINKS)
    return c


def apply(items, pat, repl):
    """Reference model of ADR-0008 §4 (revision 2). Returns (new items,
    expected text, expected per-char rPr) or None when any match is refused.
    A `\v` in `repl` becomes a:br + a run whose a:rPr is the start run's
    without links; a match that ends inside a linked start run with a `\v`
    in the replacement is refused (LinkSplitByBreak, review M5), because the
    kept suffix would otherwise move into that link-less run."""
    text = ''.join(it['t'] for it in items)
    ms = matches(text, pat)
    sp = spans(items)
    for s, e in ms:
        if classify(items, sp, s, e).startswith('refused'):
            return None
        if '\v' in repl:
            cov = covered(sp, s, e)
            start = items[cov[0]]
            if link(start['rpr']) is not None and e < sp[cov[0]][1]:
                C['replace with \\v: refused LinkSplitByBreak (match ends inside a linked run)'] += 1
                return None
    # expected per-character format, computed independently of the edit
    fmt = [None] * len(text)
    for a, b, i in sp:
        for k in range(a, b):
            fmt[k] = canon(items[i]['rpr'])
    exp_text, exp_fmt, pos = [], [], 0
    for s, e in ms:
        start_item = [i for a, b, i in sp if a <= s < b][0]
        exp_text.append(text[pos:s]); exp_fmt += fmt[pos:s]
        exp_text.append(repl)
        srpr = items[start_item]['rpr']
        seg0 = repl.split('\v')[0]
        exp_fmt += [canon(srpr)] * len(seg0)
        for ch in repl[len(seg0):]:
            exp_fmt.append(canon(None) if ch == '\v' else canon(without_links(srpr)))
        pos = e
    exp_text.append(text[pos:]); exp_fmt += fmt[pos:]
    # the edit itself, right to left
    new = [dict(it) for it in items]
    touched = set()
    for s, e in reversed(ms):
        sp = spans(new)
        cov = covered(sp, s, e)
        first, last = cov[0], cov[-1]
        a0, al = sp[first][0], sp[last][0]
        tail = new[last]['t'][e - al:]
        segs = repl.split('\v')
        suffix = tail if first == last else ''
        new[first]['t'] = new[first]['t'][:s - a0] + segs[0] + (suffix if len(segs) == 1 else '')
        touched.add(id(new[first]))
        if len(segs) > 1:
            added = []
            for k, seg in enumerate(segs[1:], 1):
                t = seg + (suffix if k == len(segs) - 1 else '')
                added.append({'k': 'br', 't': '\v', 'rpr': None})
                if t:
                    added.append({'k': 'r', 't': t, 'rpr': without_links(new[first]['rpr'])})
            new[first + 1:first + 1] = added
        if first != last:
            shift = len(added) if len(segs) > 1 else 0
            new[last + shift]['t'] = tail
            touched.add(id(new[last + shift]))
            for i in cov[1:-1]:
                new[i + shift]['t'] = None          # fully covered: removed
            if tail == '':
                new[last + shift]['t'] = None       # fully covered last run: removed
        new = [it for it in new if it['t'] is not None]
    # a touched run left empty is removed, unless no run would remain
    keep = [it for it in new if not (it['k'] == 'r' and it['t'] == '' and id(it) in touched)]
    if any(it['k'] == 'r' for it in keep) or not any(it['k'] == 'r' for it in new):
        new = keep
    else:
        first_empty = next(it for it in new if it['k'] == 'r' and id(it) in touched)
        new = [it for it in new if not (it['k'] == 'r' and it['t'] == '' and id(it) in touched and it is not first_empty)]
    return new, ''.join(exp_text), exp_fmt


def check(new, exp_text, exp_fmt):
    t = ''.join(it['t'] for it in new)
    f = [canon(it['rpr']) for it in new for _ in it['t']]
    return t == exp_text, f == exp_fmt


def main(argv):
    for path in iter_files(argv):
        try:
            pk = Pkg(path)
        except Exception:
            continue
        for sid, part in pk.slides():
            root = pk.xml(part) if part else None
            if root is None:
                continue
            for body in text_bodies(root):
                for p in body.findall(q('a:p')):
                    items = items_of(p)
                    text = ''.join(it['t'] for it in items)
                    if not text.strip():
                        continue
                    C['paragraphs with text'] += 1
                    if sum(1 for it in items if it['k'] in ('r', 'fld')) > 1:
                        C['paragraphs with >= 2 runs/fields'] += 1
                    words = text.split()
                    pats = set()
                    for n in (1, 2, 3):
                        for i in range(len(words) - n + 1):
                            w = words[i:i + n]
                            j = text.find(w[0])
                            # use the literal text between the first and last word
                            k = text.find(w[-1], j + len(w[0]) - (len(w[-1]) if n == 1 else 0))
                            if j < 0 or k < 0:
                                continue
                            pats.add(text[j:k + len(w[-1])])
                    sp = spans(items)
                    for pat in pats:
                        for s, e in matches(text, pat):
                            C['match: ' + classify(items, sp, s, e, count=True)] += 1
                            C['matches'] += 1
                        r = apply(items, pat, 'X' + pat[::-1])
                        if r is None:
                            C['replace_all: refused (paragraph unchanged)'] += 1
                            continue
                        new, et, ef = r
                        okt, okf = check(new, et, ef)
                        C['replace_all: applied'] += 1
                        C['invariant T (text == str.replace) %s' % ('holds' if okt else 'FAILS')] += 1
                        C['invariant F (per-char format) %s' % ('holds' if okf else 'FAILS')] += 1
                        # replacement with a line break (review M5)
                        for repl in ('X\v' + pat[::-1], '\v'):
                            r = apply(items, pat, repl)
                            if r is None:
                                continue
                            new, et, ef = r
                            okt, okf = check(new, et, ef)
                            C['replace with \\v: applied, invariants %s' % ('hold' if okt and okf else 'FAIL')] += 1
                            if any(link(it['rpr']) for it in items if it['k'] == 'r'):
                                C['replace with \\v: applied in a paragraph with a linked run'] += 1
                        # deletion
                        r = apply(items, pat, '')
                        if r is not None:
                            new, et, ef = r
                            okt, okf = check(new, et, ef)
                            C['delete_all: invariants %s' % ('hold' if okt and okf else 'FAIL')] += 1
                            if not any(it['k'] == 'r' for it in new) and any(it['k'] == 'r' for it in items):
                                C['delete_all: no run left (paragraph keeps a:endParaRPr only)'] += 1
    tot = C['matches'] or 1
    for k in sorted(C):
        extra = ' (%.1f %%)' % (100.0 * C[k] / tot) if k.startswith('match: ') else ''
        print('%-72s %d%s' % (k, C[k], extra))


if __name__ == '__main__':
    main(sys.argv[1:])
