"""ADR-0007 §1 evidence: run boundaries, run groups, and a simulation of the proposed
cross-run replace over every story of every file that passes the Phase0 gates.

Usage: python3 -I replace_sim.py [--json out.json] [--examples N] <dir or file>...

Sections of the output:
 A. adjacent text-run pairs in one paragraph (all stories): what separates them, and how
    their rPr compare (identical / proofing-only: w:lang, w:noProof / rFonts@hint / different).
 B. 'boundary needles': for each such pair, the 6-character span centred on the boundary
    (3+3, fewer at short runs) is classified by the proposed rule. This is the cross-run case.
 C. 'word needles': every \\w{2,} token of every paragraph is classified (most are single-run).
 D. placeholder-shaped tokens ({{x}}, ${x}, «x», <<x>>, [[x]], {x}) and their outcomes.
 E. the differential oracle (prototype): each accepted match of B, C and D is applied with
    the rule (per-run new texts, replacement into the first run) and checked against a string
    replacement of the paragraph text, with per-character formatting.

Outcome names are ADR-0007's errors: Ok (single run), Ok (cross-run), MixedFormatting,
MatchCrossesBoundary:<what>, MarkerInsideMatch:<marker>, InsideField, InsideRevision,
UnsupportedContext:<ctx>, RunNotTextOnly.
"""
import collections
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p1common as C  # noqa: E402

w, local = C.w, C.local


class Stream:
    """The paragraph's text with, per character, the run that holds it, and between
    characters the separators the walk saw."""

    def __init__(self, para, phase1=False):
        self.chars = []      # (run_idx, offset in run text)
        self.text = []
        self.cuts = collections.defaultdict(list)   # position -> [separator kinds] before char at pos
        self.runs = para.runs()
        self.ref = []        # per run: refusal or None
        self.group = []      # per run: group id or None
        gid, cur_parent, last_settable = -1, None, False
        pending = []         # separators since the last run
        ri = 0
        toks = para.tokens
        skip = set()
        for ti, tok in enumerate(toks):
            if ti in skip:
                continue
            kind = tok[0]
            # a collapsed bookmark (start immediately followed by its end) marks a point; it is
            # transparent like proofErr and stays where it is (ADR-0007 §1.3)
            if (kind == 'marker' and tok[1] == w('bookmarkStart') and ti + 1 < len(toks)
                    and toks[ti + 1][0] == 'marker' and toks[ti + 1][1] == w('bookmarkEnd')
                    and toks[ti + 1][2].get(w('id')) == tok[2].get(w('id'))):
                pending.append('proof:point-bookmark')
                skip.add(ti + 1)
                continue
            if kind == 'run':
                r = tok[1]
                ref = r.refusal(phase1)
                self.ref.append(ref)
                hard = [s for s in pending if not s.startswith(('proof', 'marker:'))]
                if ref is None and last_settable and r.parent is cur_parent and not hard:
                    self.group.append(gid)
                else:
                    if ref is None:
                        gid += 1
                        self.group.append(gid)
                    else:
                        self.group.append(None)
                if ref is not None:
                    pending.append('run:' + ref)
                pos = len(self.text)
                self.cuts[pos].extend(pending)
                pending = [] if ref is None else []
                for i, ch in enumerate(r.text):
                    self.chars.append((ri, i)); self.text.append(ch)
                if ref is not None:
                    # a refused run is a separator on both sides
                    self.cuts[len(self.text)].append('run:' + ref)
                last_settable = ref is None
                cur_parent = r.parent if ref is None else None
                ri += 1
            elif kind == 'proof':
                pending.append('proof')
            elif kind == 'marker':
                pending.append('marker:' + local(tok[1]))
            elif kind in ('enter', 'exit'):
                pending.append('wrapper:' + local(tok[1]))
                last_settable = False
            else:
                pending.append('hard:' + local(tok[1]))
                last_settable = False
        self.cuts[len(self.text)].extend(pending)
        self.text = ''.join(self.text)

    def classify(self, s, e):
        """Outcome of replacing text[s:e] (s < e)."""
        runs = sorted({self.chars[i][0] for i in range(s, e)})
        refs = [self.ref[r] for r in runs if self.ref[r]]
        if refs:
            return refs[0].split(' ', 1)[1]
        groups = {self.group[r] for r in runs}
        for p in range(s + 1, e):
            for sep in self.cuts.get(p, ()):
                if sep.startswith(('wrapper:', 'hard:', 'run:')):
                    return 'MatchCrossesBoundary:' + sep.split(':', 1)[1].split(' ')[-1]
        if len(groups) > 1:
            return 'MatchCrossesBoundary:group'
        for p in range(s + 1, e):
            for sep in self.cuts.get(p, ()):
                if sep.startswith('marker:'):
                    return 'MarkerInsideMatch:' + sep[7:]
        if len(runs) == 1:
            return 'Ok (single run)'
        first = self.runs[runs[0]]
        worst = 'identical'
        order = ['identical', 'proofing', 'hint', 'different']
        for r in runs[1:]:
            c = C.rpr_class(first, self.runs[r])
            if order.index(c) > order.index(worst):
                worst = c
        if worst == 'different':
            return 'MixedFormatting'
        return 'Ok (cross-run, %s)' % worst

    def apply(self, s, e, repl):
        """The rule: per-run new texts. The run holding text[s] gets prefix + repl (+ its own
        tail if the match ends in it); later matched runs lose their matched prefix."""
        runs = sorted({self.chars[i][0] for i in range(s, e)})
        new = {}
        for r in runs:
            t = self.runs[r].text
            idx = [i for i in range(s, e) if self.chars[i][0] == r]
            a, b = self.chars[idx[0]][1], self.chars[idx[-1]][1] + 1
            new[r] = t[:a] + (repl if r == runs[0] else '') + t[b:]
        return new


def unpreserved_edge(run):
    for t in run.el.findall(w('t')):
        tx = t.text or ''
        if tx and (tx[0].isspace() or tx[-1].isspace()) and t.get('{%s}space' % C.XML_NS) != 'preserve':
            return True
    return False


def oracle(stream, s, e, repl):
    """Differential check: per-character (char, formatting) after == before with the match
    replaced by repl in the first matched run's formatting. Returns None or a message."""
    new = stream.apply(s, e, repl)
    before = []
    for i, ch in enumerate(stream.text):
        before.append((ch, stream.chars[i][0]))
    first = stream.chars[s][0]
    expect = before[:s] + [(ch, first) for ch in repl] + before[e:]
    after = []
    for r, run in enumerate(stream.runs):
        t = new.get(r, run.text)
        after.extend((ch, r) for ch in t)
    # formatting identity = run identity here; the Rust oracle compares canonical rPr
    if after != expect:
        return 'mismatch'
    if ''.join(c for c, _ in after) != stream.text[:s] + repl + stream.text[e:]:
        return 'text mismatch'
    return None


# template syntaxes: {{x}} {%x%} {#x#} (jinja/docxtpl), {x} {#x} {/x} {^x} {%x} {@x} (docxtemplater),
# ${x} (docx4j), <<x>> and [[x]]. Guillemets are left out: in the corpora they are quotation marks.
PLACEHOLDER = re.compile(r'\{\{[^{}]{1,80}\}\}|\{%[^%]{1,80}%\}|\{#[^#{}]{1,80}#\}|\$\{[^{}]{1,60}\}|<<[^<>]{1,60}>>'
                         r'|\[\[[^\[\]]{1,60}\]\]|\{[#/^%@~-]?[A-Za-z_][\w. ]{0,40}\}')
WORD = re.compile(r'\w{2,}')


def main(argv):
    out_json, nex, args = None, 0, []
    it = iter(argv)
    for a in it:
        if a == '--json':
            out_json = next(it)
        elif a == '--examples':
            nex = int(next(it))
        else:
            args.append(a)
    files = C.corpus_files(args)
    res = collections.Counter()
    sep_kind = collections.Counter()
    rpr_by_sep = collections.Counter()
    diff_children = collections.Counter()
    bnd = collections.Counter(); bnd_story = collections.Counter()
    word = collections.Counter()
    ph = collections.Counter(); ph_files = set(); ph_story = collections.Counter()
    ws_unpreserved = collections.Counter()
    oracle_res = collections.Counter()
    examples = collections.defaultdict(list)
    gated = collections.Counter()
    for f in files:
        pkg = C.Pkg(f)
        g, _ = C.gate(pkg)
        if g:
            gated[g.split(':')[0]] += 1
            continue
        gated['writable'] += 1
        for sw in C.iter_stories(pkg):
            for para in sw.paras:
                st = Stream(para)
                if not st.text:
                    continue
                res['paragraphs with text', sw.kind] += 1
                # A + B: adjacent runs that both contribute characters
                prev = None
                for i in range(1, len(st.text)):
                    ra, rb = st.chars[i - 1][0], st.chars[i][0]
                    if ra == rb:
                        continue
                    seps = st.cuts.get(i, [])
                    between = [x for x in seps]
                    if rb - ra > 1:
                        between.append('empty-run')
                    key = 'nothing' if not between else '+'.join(sorted({x.split(':')[0] if not x.startswith('marker:') else x for x in between}))
                    sep_kind[key] += 1
                    A, B = st.runs[ra], st.runs[rb]
                    if all(x.startswith(('proof', 'marker:', 'empty-run')) for x in between):
                        cls = C.rpr_class(A, B)
                        rpr_by_sep[key.split('+')[0] if key != 'nothing' else 'nothing', cls] += 1
                        if cls == 'different':
                            diff_children[tuple(C.rpr_diff_children(A, B))] += 1
                    s, e = max(0, i - 3), min(len(st.text), i + 3)
                    o = st.classify(s, e)
                    bnd[o] += 1; bnd_story[sw.kind, o.split(' (')[0].split(':')[0]] += 1
                    if o.startswith('Ok'):
                        oracle_res['boundary', oracle(st, s, e, 'X\tY') or 'ok'] += 1
                    if nex and len(examples[o]) < nex:
                        examples[o].append('%s %s %r' % (pkg.name, sw.part, st.text[s:e]))
                for m in WORD.finditer(st.text):
                    o = st.classify(m.start(), m.end())
                    word[o.split(':')[0] if not o.startswith('Ok') else o] += 1
                    if o.startswith('Ok (cross'):
                        oracle_res['word', oracle(st, m.start(), m.end(), 'replacement') or 'ok'] += 1
                for m in PLACEHOLDER.finditer(st.text):
                    o = st.classify(m.start(), m.end())
                    ph[o] += 1; ph_files.add(pkg.name)
                    if nex and len(examples['PH ' + o]) < nex:
                        examples['PH ' + o].append('%s %s %r' % (pkg.name, sw.part, m.group()))
                    if o.startswith('Ok'):
                        oracle_res['placeholder', oracle(st, m.start(), m.end(), '') or 'ok'] += 1
                        touched = sorted({st.chars[i][0] for i in range(m.start(), m.end())})
                        if any(unpreserved_edge(st.runs[r]) for r in touched):
                            ws_unpreserved['placeholder matches touching a run with unpreserved edge whitespace'] += 1
                    ph_story[sw.kind] += 1
                # xml:space: runs with edge whitespace in a w:t that lacks preserve
                for r in st.runs:
                    for t in r.el.findall(w('t')):
                        tx = t.text or ''
                        if tx and (tx[0].isspace() or tx[-1].isspace()) and t.get('{%s}space' % C.XML_NS) != 'preserve':
                            ws_unpreserved['w:t edge whitespace without preserve'] += 1
                            if r.refusal(True) is None:
                                ws_unpreserved['... in settable runs'] += 1
                        if '  ' in tx and t.get('{%s}space' % C.XML_NS) != 'preserve':
                            ws_unpreserved['w:t inner double space without preserve'] += 1
    rep = {
        'files': dict(gated),
        'paragraphs_with_text': {k[1]: v for k, v in res.items()},
        'A_separators_between_text_runs': dict(sep_kind.most_common()),
        'A_rpr_class_by_separator': {'%s/%s' % k: v for k, v in sorted(rpr_by_sep.items())},
        'A_top_differing_rpr_children': {'+'.join(k): v for k, v in diff_children.most_common(25)},
        'B_boundary_needles': dict(bnd.most_common()),
        'B_by_story': {'%s/%s' % k: v for k, v in sorted(bnd_story.items())},
        'C_word_needles': dict(word.most_common()),
        'D_placeholders': dict(ph.most_common()),
        'D_files_with_placeholders': len(ph_files),
        'D_placeholders_by_story': dict(ph_story),
        'E_oracle': {'%s/%s' % k: v for k, v in sorted(oracle_res.items())},
        'xml_space': dict(ws_unpreserved),
    }
    if nex:
        rep['examples'] = examples
    print(json.dumps(rep, indent=1, ensure_ascii=False))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump(rep, fh, indent=1, ensure_ascii=False)


if __name__ == '__main__':
    main(sys.argv[1:])
