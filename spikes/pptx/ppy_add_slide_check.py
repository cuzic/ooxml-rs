"""Does python-pptx's add_slide() collide with an existing slide part name?
(run with `python3 -I`; python-pptx is installed into a directory given as argv[1])

Usage: python3 -I ppy_add_slide_check.py <site-dir with python-pptx> <in.pptx> <out.pptx>

python-pptx names the new part /ppt/slides/slide{len(sldIdLst)+1}.xml
(parts/presentation.py `_next_slide_partname`). That would collide in a deck
whose only slide part is slide2.xml (LibreOffice's airplane-transition.pptx),
but python-pptx 1.0.2 avoids it by *renaming every slide part* to
slide1..slideN whenever `Presentation.slides` is accessed
(presentation.py -> `rename_slide_parts`). Observed result: slide2.xml is
renamed to slide1.xml and the new slide becomes slide2.xml; no duplicate entry.
ooxml-rs cannot rename parts (ADR-0001 Layer 1 passthrough), so it must pick
an unused name instead (as Apache POI does: `getUnusedPartIndex`, bug 55791).
"""
import sys, zipfile, collections
sys.path.insert(0, sys.argv[1])
from pptx import Presentation  # noqa: E402

src, out = sys.argv[2], sys.argv[3]
prs = Presentation(src)
before = [s.part.partname for s in prs.slides]
prs.slides.add_slide(prs.slide_layouts[0])
after = [s.part.partname for s in prs.slides]
print('slide partnames before:', before)
print('slide partnames after :', after)
try:
    prs.save(out)
    names = zipfile.ZipFile(out).namelist()
    dup = [n for n, c in collections.Counter(names).items() if c > 1]
    print('saved; duplicate zip entries:', dup)
except Exception as e:
    print('save failed:', type(e).__name__, e)
