"""Exception mapping (ADR-0023 §11.3, oracle 11)."""

import errno
import os

import pytest

import ooxml_rs as R
from ooxml_rs._native import __kinds, __raise, __table_keys


def test_table_covers_every_kind_exactly():
    exported = {(crate, kind) for crate, kinds in __kinds().items() for kind in kinds}
    assert set(__table_keys()) == exported
    # ... once each (no duplicates hiding behind the set).
    total = sum(len(kinds) for kinds in __kinds().values())
    assert len(exported) == total == len(__table_keys())


def test_kinds_are_stable_strings():
    for crate, kinds in __kinds().items():
        assert crate in ("core", "xlsx", "docx", "pptx")
        for kind in kinds:
            assert kind and kind[0].isupper(), (crate, kind)


# (crate, kind) -> category, for every mapping the ADR names explicitly.
@pytest.mark.parametrize(
    ("crate", "kind", "category"),
    [
        ("xlsx", "MergedCell", R.RefusedError),
        ("xlsx", "ArrayResultCell", R.RefusedError),
        ("xlsx", "ArrayAnchor", R.RefusedError),
        ("xlsx", "RichValueCell", R.RefusedError),
        ("xlsx", "TableHeaderCell", R.RefusedError),
        ("xlsx", "Protected", R.RefusedError),
        ("xlsx", "FormulaScanFailed", R.RefusedError),
        ("xlsx", "UnknownCellMetadata", R.RefusedError),
        ("xlsx", "IterativeCalculation", R.RefusedError),
        ("xlsx", "SharedFormulaMaster", R.RefusedError),
        ("xlsx", "StructureBlocked", R.RefusedError),
        ("xlsx", "Poisoned", R.PoisonedError),
        ("xlsx", "NoSuchSheet", R.NotFoundError),
        ("xlsx", "NoSuchStyle", R.NotFoundError),
        ("xlsx", "InvalidReference", R.InvalidInputError),
        ("xlsx", "OutOfRange", R.InvalidInputError),
        ("xlsx", "InvalidNumber", R.InvalidInputError),
        ("xlsx", "StringTooLong", R.InvalidInputError),
        ("xlsx", "DateOutOfRange", R.InvalidInputError),
        ("xlsx", "DuplicateCellWrite", R.InvalidInputError),
        ("xlsx", "InvalidArgument", R.InvalidInputError),
        ("xlsx", "InvalidValue", R.MalformedFileError),
        ("xlsx", "Malformed", R.MalformedFileError),
        ("xlsx", "MissingReference", R.MalformedFileError),
        ("docx", "InvalidValue", R.InvalidInputError),
        ("core", "EncryptedPackage", R.UnsupportedFileError),
        ("core", "StrictNotSupported", R.UnsupportedFileError),
        ("core", "DtdNotAllowed", R.UnsupportedFileError),
        ("core", "Xml", R.MalformedFileError),
        ("core", "Package", R.MalformedFileError),
        ("core", "Io", R.OoxmlIOError),
    ],
)
def test_named_mappings(crate, kind, category):
    with pytest.raises(category) as caught:
        __raise(crate, kind)
    error = caught.value
    assert isinstance(error, R.OoxmlError)
    assert (error.crate, error.kind) == (crate, kind)
    assert str(error) == f"{crate}::{kind} (raised by the test hook)"


def test_not_found_is_a_key_error_and_invalid_input_a_value_error():
    with pytest.raises(KeyError):
        __raise("xlsx", "NoSuchSheet")
    with pytest.raises(ValueError):
        __raise("xlsx", "InvalidArgument")


def test_missing_path_raises_file_not_found(tmp_path):
    missing = tmp_path / "nope.xlsx"
    with pytest.raises(R.OoxmlFileNotFoundError) as caught:
        R.Workbook.open(missing)
    error = caught.value
    assert isinstance(error, FileNotFoundError)
    assert (error.crate, error.kind) == ("core", "Io")
    assert error.errno == errno.ENOENT
    assert error.filename == str(missing)


def test_garbage_bytes_raise_malformed():
    with pytest.raises(R.MalformedFileError) as caught:
        R.Workbook.from_bytes(b"not a zip")
    assert (caught.value.crate, caught.value.kind) == ("core", "Zip")
    with pytest.raises(R.MalformedFileError):
        R.Document.from_bytes(b"not a zip")
    with pytest.raises(R.MalformedFileError):
        R.Presentation.from_bytes(b"not a zip")


def test_save_into_missing_directory_raises_file_not_found(tmp_path):
    wb = R.Workbook.new()
    with pytest.raises(R.OoxmlFileNotFoundError) as caught:
        wb.save(tmp_path / "nope" / "out.xlsx")
    assert caught.value.filename == str(tmp_path / "nope" / "out.xlsx")


@pytest.mark.skipif(os.name != "posix", reason="POSIX permissions")
def test_unreadable_path_raises_permission_error(tmp_path):
    path = tmp_path / "locked.xlsx"
    path.write_bytes(R.Workbook.new().to_bytes())
    path.chmod(0o000)
    try:
        with pytest.raises(R.OoxmlPermissionError) as caught:
            R.Workbook.open(path)
        assert caught.value.errno == errno.EACCES
    finally:
        path.chmod(0o644)


def test_unknown_pair_falls_back_to_the_base():
    with pytest.raises(R.OoxmlError) as caught:
        __raise("xlsx", "NoSuchVariant")
    assert type(caught.value) is R.OoxmlError
