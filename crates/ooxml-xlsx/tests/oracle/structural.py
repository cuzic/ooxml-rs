"""Read Phase7 workbooks with openpyxl and optionally LibreOffice.

Use python -I. With --case, verify the precise values and formula in the
synthetic case. With --corpus, read every output of rows_and_columns_corpus.
"""
import argparse
from collections import Counter
import os
from pathlib import Path
import subprocess
import tempfile
import warnings
import zipfile

os.environ["OPENPYXL_LXML"] = "False"

from openpyxl import load_workbook


def has_dtd(path):
    with zipfile.ZipFile(path) as book:
        for info in book.infolist():
            if info.filename.endswith((".xml", ".rels", ".vml")):
                data = book.read(info).replace(b"\0", b"")
                if b"<!DOCTYPE" in data:
                    return True
    return False


def read_workbook(path):
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        book = load_workbook(path, read_only=False, keep_links=False)
        try:
            assert book.sheetnames
        finally:
            book.close()


def verify_case(path):
    assert not has_dtd(path)
    book = load_workbook(path, read_only=True, keep_links=False)
    try:
        assert book["Data"]["B4"].value == 20
        assert book["Data"]["A2"].value is None
        assert book["Data"]["C4"].value == "=$B$4"
        assert book["Other"]["A1"].value == "=Data!B4"
    finally:
        book.close()


def libreoffice(paths, executable, scratch):
    output = scratch / "output"
    output.mkdir()
    profile = scratch / "profile"
    (profile / "user").mkdir(parents=True)
    (profile / "user" / "registrymodifications.xcu").write_text(
        '<?xml version="1.0"?>'
        '<oor:items xmlns:oor="http://openoffice.org/2001/registry">'
        '<item oor:path="/org.openoffice.Office.Common/Security/Scripting">'
        '<prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop>'
        '</item><item oor:path="/org.openoffice.Office.Calc/Content/Update">'
        '<prop oor:name="Link" oor:op="fuse"><value>2</value></prop>'
        '</item></oor:items>',
        encoding="utf-8",
    )
    result = Counter()
    for start in range(0, len(paths), 25):
        batch = paths[start:start + 25]
        subprocess.run(
            [executable, f"-env:UserInstallation={profile.as_uri()}", "--headless",
             "--convert-to", "xlsx", "--outdir", str(output), *batch],
            check=False,
            timeout=180,
        )
        for path in batch:
            exported = output / path.with_suffix(".xlsx").name
            if exported.is_file():
                result["success"] += 1
                exported.unlink()
            else:
                result["failed"] += 1
                print(f"LibreOffice failed to export {path.name}", flush=True)
    return dict(result)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", type=Path)
    parser.add_argument("--corpus", type=Path)
    parser.add_argument("--soffice")
    parser.add_argument("--scratch", type=Path)
    args = parser.parse_args()
    if args.case:
        verify_case(args.case)
        print("openpyxl structural case passed", flush=True)
    if args.corpus:
        counts = Counter()
        safe = []
        for path in sorted(args.corpus.iterdir()):
            if path.suffix not in (".xlsx", ".xlsm"):
                continue
            if has_dtd(path):
                counts["DTD skipped"] += 1
                continue
            safe.append(path.resolve())
            try:
                read_workbook(path)
                counts["success"] += 1
            except Exception as error:
                counts[type(error).__name__] += 1
                print(f"openpyxl {path.name}: {error}", flush=True)
        print(f"openpyxl {dict(counts)}", flush=True)
        if args.soffice:
            with tempfile.TemporaryDirectory(dir=args.scratch) as directory:
                print(f"LibreOffice {libreoffice(safe, args.soffice, Path(directory))}", flush=True)


if __name__ == "__main__":
    main()
