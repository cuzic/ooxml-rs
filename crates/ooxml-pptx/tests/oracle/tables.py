"""ADR-0013 §7 independent read-back of Phase 1b table edits.

For every case written by `cargo run --example table_cases`, python-pptx
1.0.2 reads the input and the output table; this script applies the
operation to its own model of the input (cell texts by grid position,
merge origins and spans) and compares. Inputs are untrusted: only parsed.

Usage: python -I tables.py [--site DIR] <cases-dir> <fixtures-dir>
"""

import json
import os
import sys

if sys.argv[1:2] == ["--site"]:
    sys.path.insert(0, sys.argv[2])
    del sys.argv[1:3]
from pptx import Presentation  # noqa: E402


def table_of(path, slide, shape_id):
    prs = Presentation(path)
    for sh in prs.slides[slide].shapes:
        if sh.shape_id == shape_id and sh.has_table:
            return sh.table
    raise AssertionError(f"{path}: no table {shape_id}")


def model(t):
    """[[(text, origin, span_h, span_w, spanned)]] by grid position."""
    out = []
    for r in range(len(t.rows)):
        row = []
        for c in range(len(t.columns)):
            cell = t.cell(r, c)
            row.append((cell.text, cell.is_merge_origin, cell.span_height, cell.span_width, cell.is_spanned))
        out.append(row)
    return out


def origins(m):
    """{(r, c): (h, w)} of merge origins."""
    return {(r, c): (x[2], x[3]) for r, row in enumerate(m) for c, x in enumerate(row) if x[1]}


def expected(m, op, a):
    """Texts and merge rectangles after the op, computed independently."""
    texts = [[x[0] for x in row] for row in m]
    rects = dict(origins(m))
    rows, cols = len(m), len(m[0])
    if op == "insert_row":
        at = a["at"]
        texts.insert(at, ["" for _ in range(cols)])
        new = {}
        for (r, c), (h, w) in rects.items():
            if r < at < r + h:
                new[(r, c)] = (h + 1, w)
            elif r >= at:
                new[(r + 1, c)] = (h, w)
            else:
                new[(r, c)] = (h, w)
        rects = new
    elif op == "delete_row":
        at = a["at"]
        texts.pop(at)
        new = {}
        for (r, c), (h, w) in rects.items():
            if r == at:
                continue  # a merge inside the deleted row goes with it
            if r < at < r + h:
                h -= 1
            elif r > at:
                r -= 1
            if h * w > 1:
                new[(r, c)] = (h, w)
        rects = new
    elif op == "insert_column":
        at = a["at"]
        for row in texts:
            row.insert(at, "")
        new = {}
        for (r, c), (h, w) in rects.items():
            if c < at < c + w:
                new[(r, c)] = (h, w + 1)
            elif c >= at:
                new[(r, c + 1)] = (h, w)
            else:
                new[(r, c)] = (h, w)
        rects = new
    elif op == "delete_column":
        at = a["at"]
        for row in texts:
            row.pop(at)
        new = {}
        for (r, c), (h, w) in rects.items():
            if c == at:
                continue  # a merge inside the deleted column goes with it
            if c < at < c + w:
                w -= 1
            elif c > at:
                c -= 1
            if h * w > 1:
                new[(r, c)] = (h, w)
        rects = new
    elif op == "merge":
        r1, c1, r2, c2 = a["r1"], a["c1"], a["r2"], a["c2"]
        parts = [texts[r][c] for r in range(r1, r2 + 1) for c in range(c1, c2 + 1)]
        nonempty = [p for p in parts[1:] if p != ""]
        origin = parts[0]
        moved = nonempty if origin == "" else [origin] + nonempty
        joined = "\n".join(moved) if nonempty else origin
        for r in range(r1, r2 + 1):
            for c in range(c1, c2 + 1):
                texts[r][c] = ""
        texts[r1][c1] = joined
        rects[(r1, c1)] = (r2 - r1 + 1, c2 - c1 + 1)
    return texts, rects


def main():
    cases_dir, fixtures = sys.argv[1], sys.argv[2]
    cases = json.load(open(os.path.join(cases_dir, "cases.json")))
    for c in cases:
        label = f"{c['input']} slide {c['slide']} table {c['shape_id']} {c['op']} {c['args']}"
        before = model(table_of(os.path.join(fixtures, c["input"]), c["slide"], c["shape_id"]))
        after = model(table_of(os.path.join(cases_dir, c["output"]), c["slide"], c["shape_id"]))
        if c["op"] == "border":
            assert after == before, label
            continue
        texts, rects = expected(before, c["op"], c["args"])
        got_texts = [[x[0] for x in row] for row in after]
        if c["op"] == "merge":
            # spanned cells' text is hidden; compare visible cells only
            for r, row in enumerate(after):
                for col, x in enumerate(row):
                    if not x[4]:
                        assert x[0] == texts[r][col], f"{label}: text at ({r},{col}) {x[0]!r} != {texts[r][col]!r}"
        else:
            assert got_texts == texts, f"{label}: texts\n{got_texts}\n{texts}"
        assert origins(after) == rects, f"{label}: merges {origins(after)} != {rects}"
    print(f"table oracle: {len(cases)} cases ok")


if __name__ == "__main__":
    main()
