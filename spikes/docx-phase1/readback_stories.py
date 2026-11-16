"""ADR-0007 §9 (T5 analogue): which story parts docx-rs 0.4.22 reads back, and whether its text
equals an ElementTree reading of the same part.

Usage: python3 -I readback_stories.py <docx-rs json dir> <dir or file>...
       (json from `readback/`: cargo run --release -- <json dir> <files>)

Per story part of each file that passes the Phase0 gates:
  header/footer  docx-rs exposes only the parts referenced by the body-level w:sectPr
                 (header/firstHeader/evenHeader, footer/firstFooter/evenFooter). Reachable
                 parts are compared paragraph by paragraph (texts of w:t joined), in order.
  comments       compared per comment (w:id), paragraph by paragraph.
  footnotes/endnotes  docx-rs has no reader for them (its `footnotes` is writer-side only).
"""
import collections
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p1common as C  # noqa: E402

w, local = C.w, C.local
HF_KEYS = {'header': ('header', 'firstHeader', 'evenHeader'), 'footer': ('footer', 'firstFooter', 'evenFooter')}


def rs_texts(node, out):
    """Paragraph texts in document order from docx-rs JSON (tables, hyperlinks, ins...)."""
    if isinstance(node, dict):
        if node.get('type') == 'paragraph':
            buf = []
            collect(node.get('data'), buf)
            out.append(''.join(buf))
            return
        for v in node.values():
            rs_texts(v, out)
    elif isinstance(node, list):
        for v in node:
            rs_texts(v, out)


def collect(node, buf):
    if isinstance(node, dict):
        if node.get('type') == 'text':
            buf.append(node['data'].get('text', ''))
            return
        if node.get('type') == 'paragraph':  # nested (text boxes): own paragraphs, skip
            return
        for v in node.values():
            collect(v, buf)
    elif isinstance(node, list):
        for v in node:
            collect(v, buf)


def et_texts(container, kind='header'):
    """w:t texts per body-flow paragraph (the StoryWalk of p1common: tables and block-level
    content controls are descended, text boxes are not flow)."""
    out = []
    sw = C.StoryWalk(kind, '', container)
    for para in sw.paras:
        p = para.el
        buf = []
        stack = [iter(p)]
        while stack:
            for el in stack[-1]:
                if el.tag == w('p') or el.tag in C.NOT_FLOW or el.tag == w('pPr'):
                    continue
                if el.tag == w('t'):
                    buf.append(el.text or '')
                stack.append(iter(el))
                break
            else:
                stack.pop()
        out.append(''.join(buf))
    return out


def et_texts_note(c):
    out = []
    sw = C.StoryWalk('header', '', c)  # a comment's content is block-level, like a header
    for para in sw.paras:
        out.append(''.join(t.text or '' for t in para.el.iter(w('t')) if not inside_not_flow(para.el, t)))
    return out


def inside_not_flow(p, t):
    parent = {c: q for q in p.iter() for c in q}
    a = parent.get(t)
    while a is not None and a is not p:
        if a.tag in C.NOT_FLOW:
            return True
        a = parent.get(a)
    return False


def main(argv):
    jdir, args = argv[0], argv[1:]
    res = collections.Counter()
    miss = []
    for f in C.corpus_files(args):
        pkg = C.Pkg(f)
        if C.gate(pkg)[0]:
            continue
        jf = os.path.join(jdir, pkg.name + '.json')
        if not os.path.exists(jf):
            res['docx-rs failed on file'] += 1
            continue
        d = json.load(open(jf))
        sp = d['document'].get('sectionProperty', {})
        body = pkg.doc.find(w('body'))
        final = body.find(w('sectPr')) if body is not None else None
        rid2part = {rid: tgt for rid, _, tgt in pkg.rels}
        reachable = {}
        if final is not None:
            for kind, tag in (('header', 'headerReference'), ('footer', 'footerReference')):
                for ref in final.findall(w(tag)):
                    t = ref.get(w('type')) or 'default'
                    key = {'default': kind, 'first': 'first' + kind.title(), 'even': 'even' + kind.title()}.get(t)
                    if key:
                        reachable[rid2part.get(ref.get('{%s}id' % C.R))] = key
        for kind, part, root in pkg.stories():
            if kind in ('header', 'footer'):
                res[kind, 'parts'] += 1
                key = reachable.get(part)
                if key is None or key not in sp:
                    res[kind, 'not exposed by docx-rs'] += 1
                    continue
                res[kind, 'exposed'] += 1
                got = []
                rs_texts(sp[key][1] if isinstance(sp[key], list) else sp[key], got)
                exp = et_texts(root, kind)
                if got == exp:
                    res[kind, 'text equal'] += 1
                else:
                    res[kind, 'text differs'] += 1
                    if len(miss) < 10:
                        miss.append((pkg.name, part, exp[:3], got[:3]))
            elif kind == 'comments':
                res[kind, 'parts'] += 1
                rs = {c['id']: c for c in d.get('comments', {}).get('comments', [])}
                for c in root:
                    if local(c.tag) != 'comment':
                        continue
                    res[kind, 'comments'] += 1
                    cid = int(c.get(w('id')) or -1)
                    if cid not in rs:
                        res[kind, 'comment not exposed'] += 1
                        continue
                    got = []
                    rs_texts(rs[cid]['children'], got)
                    if got == et_texts_note(c):
                        res[kind, 'comment text equal'] += 1
                    else:
                        res[kind, 'comment text differs'] += 1
                        if len(miss) < 10:
                            miss.append((pkg.name, 'comment %d' % cid, et_texts_note(c)[:2], got[:2]))
            elif kind in ('footnotes', 'endnotes'):
                res[kind, 'parts (no docx-rs reader)'] += 1
    print(json.dumps({'/'.join(k) if isinstance(k, tuple) else k: v for k, v in sorted(res.items(), key=str)}, indent=1))
    for m in miss:
        print('DIFF', m)


if __name__ == '__main__':
    main(sys.argv[1:])
