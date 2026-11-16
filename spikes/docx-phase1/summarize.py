"""Aggregate the JSON outputs of replace_sim.py and stories_cells.py into the numbers ADR-0007
quotes. Usage: python3 -I summarize.py <results dir> <corpus name>...
(expects <dir>/rs_<name>.json and <dir>/sc_<name>.json)."""
import json
import os
import sys


def pct(a, b):
    return '%d / %d (%.1f%%)' % (a, b, 100.0 * a / b) if b else '0 / 0'


def main(argv):
    d, names = argv[0], argv[1:]
    for n in names:
        rs = json.load(open(os.path.join(d, 'rs_%s.json' % n)))
        sc = json.load(open(os.path.join(d, 'sc_%s.json' % n)))
        print('==', n, 'writable files', rs['files'].get('writable'))
        b = rs['B_boundary_needles']
        tot = sum(b.values())
        ok_id = sum(v for k, v in b.items() if k == 'Ok (cross-run, identical)')
        ok_inv = sum(v for k, v in b.items() if k.startswith('Ok (cross-run'))
        mixed = b.get('MixedFormatting', 0)
        print('  boundary needles: identical-only', pct(ok_id, tot), '| default rule (identical+proofing+hint)', pct(ok_inv, tot),
              '| +FirstRunFormatting', pct(ok_inv + mixed, tot))
        ph = rs['D_placeholders']
        pt = sum(ph.values())
        if pt:
            ok = sum(v for k, v in ph.items() if k.startswith('Ok'))
            cross = sum(v for k, v in ph.items() if k.startswith('Ok (cross'))
            print('  placeholders: accepted', pct(ok, pt), '| of which cross-run', cross, '| files', rs['D_files_with_placeholders'])
        wd = rs['C_word_needles']
        wt = sum(wd.values())
        print('  word needles accepted', pct(sum(v for k, v in wd.items() if k.startswith('Ok')), wt))
        print('  oracle', rs['E_oracle'])
        cells = {k[4:]: v for k, v in sc['cells'].items() if k.startswith('all/')}
        ct = sum(cells.values())
        cok = sum(v for k, v in cells.items() if k.startswith('Ok'))
        cont = cells.get('MergedContinuation', 0)
        print('  cells settable', pct(cok, ct), '| excluding merged continuations', pct(cok, ct - cont))
        st = sc['stories']
        for k in ('header', 'footer', 'footnotes', 'endnotes', 'comments', 'main'):
            print('  %-9s settable text runs %s' % (k, pct(st.get(k + '/settable text runs', 0), st.get(k + '/text runs', 0))))


if __name__ == '__main__':
    main(sys.argv[1:])
