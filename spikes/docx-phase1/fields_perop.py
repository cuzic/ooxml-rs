"""ADR-0007 §6 evidence: per-operation stale-field rules and the IF rule.

Usage: python3 -I fields_perop.py [--json out.json] [--list] <dir or file>...

For every file whose only Phase0 refusal would be StaleFieldRisk (W1, W3 and the open
refusals pass), the refused field keys are re-checked under ADR-0007's per-operation table:

  text    set_run_text, replace, set_cell_text, set_paragraph_text
  format  direct run and paragraph properties (the §4 subset)
  style   pStyle / rStyle
  append  append_paragraph (Phase0 operation; same additions as text, see ADR-0004 rev 3)

A key is allowed for an operation when it is Phase0-safe or in that operation's column.
IF is allowed (all operations except style) when every nested field is allowed and neither
side of its comparison is a bare identifier (a bookmark name). The tokenizer is the one
proposed in ADR-0007 §6.3.

Also reported: settings/w:mailMerge, existing w:updateFields, fldChar/@w:dirty.
"""
import collections
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import p1common as C  # noqa: E402

w = C.w

NEUTRAL = {'SEQ', 'TC', 'XE', 'NOTEREF'}                 # depend only on fields, instructions, note numbers
PARAGRAPH_COUNT = {'LISTNUM', 'AUTONUM', 'AUTONUMLGL', 'AUTONUMOUT'}  # count paragraphs / outline levels
STATISTICS_TEXT = {'NUMWORDS', 'NUMCHARS', 'INFO NUMWORDS', 'INFO NUMCHARS', 'DOCPROPERTY WORDS',
                   'DOCPROPERTY CHARACTERS', 'DOCPROPERTY CHARACTERSWITHSPACES'}
TABLE = {
    'text': NEUTRAL | PARAGRAPH_COUNT,
    'append': NEUTRAL | PARAGRAPH_COUNT,
    'format': NEUTRAL | PARAGRAPH_COUNT | STATISTICS_TEXT | {'='},
    'style': NEUTRAL,
}

TOKEN = re.compile(r'\s*(?:(?P<q>["“”„‟＂][^"“”„‟＂]*["“”„‟＂])'
                   r'|(?P<nested>\x01)|(?P<op><>|<=|>=|=|<|>)|(?P<switch>\\[*#@!]?\S*)|(?P<num>-?\d+(?:[.,]\d+)?)|(?P<word>[^\s"“”„‟＂<>=\\]+))')


def if_tokens(instr):
    s = instr.strip()
    assert s[:2].upper() == 'IF'
    s = s[2:]
    out, pos = [], 0
    while pos < len(s):
        m = TOKEN.match(s, pos)
        if not m or m.end() == pos:
            if s[pos:].strip() == '':
                break
            return None
        pos = m.end()
        out.append((m.lastgroup, m.group(m.lastgroup)))
    return out


def if_operands_ok(instr):
    """(b) of the IF rule: the comparison is `operand op operand`, and neither operand is a
    bare word (a bookmark reference). Quoted strings, numbers and nested fields are fine."""
    toks = if_tokens(instr)
    if toks is None:
        return False, 'untokenizable'
    toks = [t for t in toks if t[0] != 'switch']
    ops = [i for i, t in enumerate(toks) if t[0] == 'op']
    if not ops:
        return False, 'no comparison operator'
    i = ops[0]
    if i != 1 or len(toks) < 3:
        return False, 'operand shape'
    lhs, rhs = toks[0], toks[2]
    for side in (lhs, rhs):
        if side[0] == 'word':
            return False, 'bare identifier %r' % side[1]
    return True, 'ok'


def allowed(f, op, custom):
    if C.key_safe(f.key, custom):
        return all(allowed(c, op, custom) for c in f.children)
    if f.key in TABLE[op]:
        return all(allowed(c, op, custom) for c in f.children)
    if f.key == 'IF' and op != 'style':
        ok, _ = if_operands_ok(f.instr)
        return ok and all(allowed(c, op, custom) for c in f.children)
    return False


def main(argv):
    out_json, listing, args = None, False, []
    it = iter(argv)
    for a in it:
        if a == '--json':
            out_json = next(it)
        elif a == '--list':
            listing = True
        else:
            args.append(a)
    res = collections.Counter()
    keys = collections.Counter()
    ifs = collections.Counter()
    per_file = []
    for f in C.corpus_files(args):
        pkg = C.Pkg(f)
        g, detail = C.gate(pkg)
        if pkg.err:
            continue
        flags = C.settings_flags(pkg)
        res['mailMerge setting'] += flags['mailMerge']
        res['updateFields present'] += flags['updateFields'] is not None
        for kind, part, root in pkg.stories():
            for fc in root.iter(w('fldChar')):
                if fc.get(w('dirty')) is not None:
                    res['fldChar/@dirty present'] += 1
        if g != 'StaleFieldRisk':
            continue
        res['StaleFieldRisk files'] += 1
        custom = C.custom_props(pkg)
        fields = [fl for _, _, root in pkg.stories() for fl in C.scan_fields(root)]
        top = [fl for fl in fields if not any(fl in o.children for o in fields)]
        for fl in fields:
            if not C.key_safe(fl.key, custom):
                keys[fl.key] += 1
            if fl.key == 'IF':
                ok, why = if_operands_ok(fl.instr)
                ifs[why if not ok else 'operands ok'] += 1
                ifs['nested fields all Phase0-safe' if all(C.key_safe(c.key, custom) for c in fl.children) else 'a nested field is refused'] += 1
        row = {'file': pkg.name, 'refused': detail}
        for op in TABLE:
            ok = all(allowed(fl, op, custom) for fl in top)
            row[op] = ok
            res['unlocked for ' + op] += ok
        per_file.append(row)
    rep = {'summary': dict(res), 'refused_keys': dict(keys.most_common()), 'if_rule': dict(ifs)}
    if listing:
        rep['files'] = per_file
    print(json.dumps(rep, indent=1, ensure_ascii=False))
    if out_json:
        with open(out_json, 'w') as fh:
            json.dump(rep, fh, indent=1, ensure_ascii=False)


if __name__ == '__main__':
    main(sys.argv[1:])
