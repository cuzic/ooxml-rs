"""Check the workbook produced by the Rust image read-back test with openpyxl."""

from pathlib import Path
import sys
from zipfile import ZipFile

from openpyxl import load_workbook


def check(path):
    with ZipFile(path) as package:
        for name in package.namelist():
            if name.endswith((".xml", ".rels")) and b"<!DOCTYPE" in package.read(name):
                raise ValueError(f"DTD in {name}")
    book = load_workbook(path)
    try:
        anchors = {type(picture.anchor).__name__: picture for picture in book.active._images}
        assert len(book.active._images) == len(anchors) == 3
        first, second, third = (anchors[name] for name in ("OneCellAnchor", "TwoCellAnchor", "AbsoluteAnchor"))
        assert (first.anchor._from.row, first.anchor._from.col) == (1, 2)
        assert (first.anchor.ext.cx, first.anchor.ext.cy) == (152400, 114300)
        assert (second.anchor._from.row, second.anchor._from.col) == (3, 1)
        assert (second.anchor.to.row, second.anchor.to.col) == (5, 3)
        assert second.anchor.editAs == "absolute"
        assert (third.anchor.pos.x, third.anchor.pos.y) == (0, 1)
        assert (third.anchor.ext.cx, third.anchor.ext.cy) == (200, 300)
        fixtures = Path(__file__).parent
        blue, orange = ((fixtures / f"{name}.png").read_bytes() for name in ("blue", "orange"))
        assert [picture._data() for picture in (first, second, third)] == [blue, orange, blue]
    finally:
        book.close()


if __name__ == "__main__":
    check(sys.argv[1])
