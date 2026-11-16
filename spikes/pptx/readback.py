"""Independent read-back oracle candidate for ADR-0005 (T3): dump the text of
every shape on every slide with python-pptx, as JSON.
(run with `python3 -I`; python-pptx is installed into a directory given as argv[1])

Usage: python3 -I readback.py <site-dir with python-pptx 1.0.2> <file.pptx>

Output: {"slides": [{"slide_id": 256, "partname": "...", "shapes": [
           {"shape_id": 2, "name": "Title 1", "paragraphs": [["run", "texts"], ...]}, ...]}]}
Group shapes are walked recursively; table cells are emitted as
"name[r,c]". Graphic frames other than tables are skipped. The Rust test
would compare this JSON with ooxml-pptx's own read of the same output file.
Note: python-pptx renames slide parts on access (see ppy_add_slide_check.py);
this script only reads and never saves, so that does not matter here.
"""
import sys, json
sys.path.insert(0, sys.argv[1])
from pptx import Presentation  # noqa: E402
from pptx.shapes.group import GroupShape  # noqa: E402

def paras(tf):
    return [[r.text for r in p.runs] for p in tf.paragraphs]

def walk(shapes, out):
    for sh in shapes:
        if isinstance(sh, GroupShape):
            walk(sh.shapes, out)
        elif getattr(sh, 'has_table', False) and sh.has_table:
            for ri, row in enumerate(sh.table.rows):
                for ci, cell in enumerate(row.cells):
                    out.append({'shape_id': sh.shape_id, 'name': '%s[%d,%d]' % (sh.name, ri, ci),
                                'paragraphs': paras(cell.text_frame)})
        elif sh.has_text_frame:
            out.append({'shape_id': sh.shape_id, 'name': sh.name, 'paragraphs': paras(sh.text_frame)})

prs = Presentation(sys.argv[2])
res = {'slides': []}
for s in prs.slides:
    shapes = []
    walk(s.shapes, shapes)
    res['slides'].append({'slide_id': s.slide_id, 'shapes': shapes})
json.dump(res, sys.stdout, ensure_ascii=False, indent=1)
