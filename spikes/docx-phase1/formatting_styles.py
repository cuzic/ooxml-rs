"""ADR-0007 §4 and §5 evidence: direct run/paragraph properties as producers write them, and
style references.

Usage: python3 -I formatting_styles.py [--json out.json] <dir or file>...

Over files that open (gates are not applied: formatting facts are about producers):
 rpr / ppr  child frequency under w:r/w:rPr and w:p/w:pPr in all stories; lexical forms of
            the proposed subset (on/off val, u, color, highlight, sz/szCs parity, vertAlign,
            rFonts attributes, jc, spacing and ind attributes); pairing of b/bCs, i/iCs, sz/szCs.
 styles     styles.xml presence; styles per type; default styles; styleId vs w:name
            (identical, case/space-folded, unrelated e.g. Japanese Word's 'a', '1');
            references (pStyle, rStyle, tblStyle, basedOn, next, link, numbering styleLink)
            to an undefined id or to a style of the wrong type; duplicate ids (exact and
            case-insensitive); latent styles without a definition.
"""
import collections
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p1common as C  # noqa: E402

w, local = C.w, C.local
ONOFF_PROPS = ['b', 'bCs', 'i', 'iCs', 'strike', 'dstrike', 'caps', 'smallCaps', 'vanish', 'keepNext', 'keepLines',
               'pageBreakBefore', 'widowControl', 'contextualSpacing', 'snapToGrid', 'noProof']


def lex(v):
    if v is None:
        return '<absent>'
    return v


def main(argv):
    out_json, args = None, []
    it = iter(argv)
    for a in it:
        if a == '--json':
            out_json = next(it)
        else:
            args.append(a)
    rpr_kids, ppr_kids = collections.Counter(), collections.Counter()
    forms = collections.Counter()
    pairs = collections.Counter()
    sty = collections.Counter()
    refs = collections.Counter()
    examples = collections.defaultdict(list)
    nfiles = 0
    for f in C.corpus_files(args):
        pkg = C.Pkg(f)
        if pkg.err:
            continue
        nfiles += 1
        stories = pkg.stories()
        if pkg.err:
            continue
        for kind, part, root in stories:
            for r in root.iter(w('r')):
                rpr = r.find(w('rPr'))
                if rpr is None:
                    continue
                kids = {local(k.tag): k for k in rpr}
                for k in kids:
                    rpr_kids[k] += 1
                for p in ONOFF_PROPS:
                    if p in kids:
                        forms['onoff val', lex(kids[p].get(w('val')))] += 1
                for a, b in (('b', 'bCs'), ('i', 'iCs'), ('sz', 'szCs')):
                    if a in kids:
                        pairs[a, 'with ' + b if b in kids else 'without ' + b] += 1
                        if b in kids and kids[a].get(w('val')) != kids[b].get(w('val')):
                            pairs[a, b + ' with a different val'] += 1
                    elif b in kids:
                        pairs[b, 'without ' + a] += 1
                if 'u' in kids:
                    forms['u val', lex(kids['u'].get(w('val')))] += 1
                if 'color' in kids:
                    v = kids['color'].get(w('val')) or '<absent>'
                    shape = 'auto' if v == 'auto' else 'RRGGBB upper' if re.fullmatch('[0-9A-F]{6}', v) else \
                        'rrggbb lower/mixed' if re.fullmatch('[0-9A-Fa-f]{6}', v) else 'other:' + v[:12]
                    forms['color val', shape] += 1
                    if kids['color'].get(w('themeColor')):
                        forms['color', 'with themeColor'] += 1
                if 'highlight' in kids:
                    forms['highlight val', lex(kids['highlight'].get(w('val')))] += 1
                if 'sz' in kids:
                    v = kids['sz'].get(w('val')) or ''
                    forms['sz val', 'integer' if v.isdigit() else 'other:' + v[:8]] += 1
                if 'vertAlign' in kids:
                    forms['vertAlign val', lex(kids['vertAlign'].get(w('val')))] += 1
                if 'rFonts' in kids:
                    for at in kids['rFonts'].attrib:
                        forms['rFonts attr', local(at)] += 1
            for p in root.iter(w('p')):
                ppr = p.find(w('pPr'))
                if ppr is None:
                    continue
                for k in ppr:
                    ppr_kids[local(k.tag)] += 1
                jc = ppr.find(w('jc'))
                if jc is not None:
                    forms['jc val', lex(jc.get(w('val')))] += 1
                sp = ppr.find(w('spacing'))
                if sp is not None:
                    for at in sp.attrib:
                        forms['spacing attr', local(at)] += 1
                ind = ppr.find(w('ind'))
                if ind is not None:
                    for at in ind.attrib:
                        forms['ind attr', local(at)] += 1
        # ------------------------------------------------------------ inherited superseding attributes
        # (ADR-0007 rev 2, review m3): direct values written below an inherited theme font
        # or character-unit indent
        sp = pkg.part('styles')
        if sp is not None:
            sr = pkg.xml(sp)
            dd = sr.find(w('docDefaults'))
            drf = dd.find('.//' + w('rFonts')) if dd is not None else None
            if drf is not None and drf.get(w('asciiTheme')):
                forms['inherited', 'files: docDefaults rFonts with asciiTheme'] += 1
                for kind, part, root in stories:
                    for rf in root.iter(w('rFonts')):
                        if rf.get(w('ascii')) and not rf.get(w('asciiTheme')):
                            forms['inherited', 'direct rFonts/@ascii without asciiTheme under a docDefaults asciiTheme'] += 1
            chars_styles = {st.get(w('styleId')) for st in sr.findall(w('style'))
                            if any(st.find('.//' + w('ind')) is not None and st.find('.//' + w('ind')).get(w(a)) not in (None, '0')
                                   for a in ('firstLineChars', 'leftChars', 'hangingChars'))}
            for kind, part, root in stories:
                for p_ in root.iter(w('p')):
                    ppr = p_.find(w('pPr'))
                    ind = ppr.find(w('ind')) if ppr is not None else None
                    if ind is None:
                        continue
                    for a in ('firstLineChars', 'leftChars', 'hangingChars'):
                        if ind.get(w(a)) == '0':
                            forms['inherited', 'direct ind/@%s="0" (cancels an inherited value)' % a] += 1
                    ps = ppr.find(w('pStyle'))
                    if ps is not None and ps.get(w('val')) in chars_styles and not any(ind.get(w(a)) for a in ('firstLineChars', 'leftChars', 'hangingChars')):
                        forms['inherited', 'paragraph: style has *Chars, direct ind has twips only'] += 1
        # ------------------------------------------------------------ styles
        sp = pkg.part('styles')
        if sp is None:
            sty['files without a styles part'] += 1
            ids = {}
        else:
            sty['files with a styles part'] += 1
            sroot = pkg.xml(sp)
            ids = {}
            lower = collections.Counter()
            for s in sroot.findall(w('style')):
                sid = s.get(w('styleId'))
                t = s.get(w('type')) or '<absent>'
                sty['style type', t] += 1
                if sid is None:
                    sty['style without styleId'] += 1
                    continue
                if sid in ids:
                    sty['duplicate styleId (exact)'] += 1
                ids[sid] = t
                lower[sid.lower()] += 1
                nm = s.find(w('name'))
                name = nm.get(w('val')) if nm is not None else None
                if name is None:
                    sty['styleId vs name', 'no name'] += 1
                elif name == sid:
                    sty['styleId vs name', 'identical'] += 1
                elif re.sub(r'\s', '', name).lower() == sid.lower():
                    sty['styleId vs name', 'equal after removing spaces and case'] += 1
                else:
                    sty['styleId vs name', 'unrelated'] += 1
                    if len(examples['unrelated id/name']) < 12:
                        examples['unrelated id/name'].append('%s: %r -> %r' % (pkg.name, sid, name))
                if s.get(w('default')) in ('1', 'true', 'on'):
                    sty['default style', t] += 1
            sty['case-insensitive duplicate styleIds'] += sum(1 for v in lower.values() if v > 1)
            defined_names = set()
            for s in sroot.findall(w('style')):
                nm = s.find(w('name'))
                if nm is not None:
                    defined_names.add((nm.get(w('val')) or '').lower())
            ls = sroot.find(w('latentStyles'))
            if ls is not None:
                for e in ls.findall(w('lsdException')):
                    if (e.get(w('name')) or '').lower() not in defined_names:
                        sty['latent styles without a definition'] += 1
                    else:
                        sty['latent styles with a definition'] += 1
            for s in sroot.findall(w('style')):
                for tag in ('basedOn', 'next', 'link'):
                    e = s.find(w(tag))
                    if e is not None:
                        v = e.get(w('val'))
                        refs[tag, 'defined' if v in ids else 'UNDEFINED'] += 1
        # references from stories (and numbering styleLink)
        expect = {'pStyle': 'paragraph', 'rStyle': 'character', 'tblStyle': 'table'}
        for kind, part, root in stories:
            for tag, typ in expect.items():
                for e in root.iter(w(tag)):
                    v = e.get(w('val'))
                    if sp is None:
                        refs[tag, 'no styles part'] += 1
                    elif v not in ids:
                        refs[tag, 'UNDEFINED'] += 1
                        if len(examples['undefined ' + tag]) < 8:
                            examples['undefined ' + tag].append('%s %s %r' % (pkg.name, part, v))
                    elif ids[v] not in (typ, '<absent>' if typ == 'paragraph' else None):
                        refs[tag, 'wrong type: ' + ids[v]] += 1
                    else:
                        refs[tag, 'defined, right type'] += 1
    rep = {
        'files': nfiles,
        'rpr_children_top': dict(rpr_kids.most_common(40)),
        'ppr_children_top': dict(ppr_kids.most_common(30)),
        'forms': {'%s/%s' % k: v for k, v in sorted(forms.items())},
        'pairs': {'%s/%s' % k: v for k, v in sorted(pairs.items())},
        'styles': {('%s/%s' % k if isinstance(k, tuple) else k): v for k, v in sorted(sty.items(), key=lambda x: str(x[0]))},
        'style_refs': {'%s/%s' % k: v for k, v in sorted(refs.items())},
        'examples': examples,
    }
    print(json.dumps(rep, indent=1, ensure_ascii=False))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump(rep, fh, indent=1, ensure_ascii=False)


if __name__ == '__main__':
    main(sys.argv[1:])
