"""Lossless xlsx/docx/pptx editing for RPA hosts (ADR-0023).

Index conventions (stated once, ADR-0023 §11.4): rows and columns are
**1-based** and A1 strings are accepted wherever a coordinate or range is;
**sheets** are ``str`` (name) or ``int`` (**0-based** position in
``sheet_names()``, so ``wb.sheet_names()[i]`` and ``sheet=i`` agree).
"""

from ._errors import (
    InvalidInputError,
    MalformedFileError,
    NotFoundError,
    OoxmlError,
    OoxmlFileNotFoundError,
    OoxmlIOError,
    OoxmlPermissionError,
    PoisonedError,
    RefusedError,
    UnsupportedFileError,
)
from ._values import CLEAR, CellError, Formula

__all__ = [
    "CLEAR",
    "CellError",
    "Formula",
    "InvalidInputError",
    "MalformedFileError",
    "NotFoundError",
    "OoxmlError",
    "OoxmlFileNotFoundError",
    "OoxmlIOError",
    "OoxmlPermissionError",
    "PoisonedError",
    "RefusedError",
    "UnsupportedFileError",
]

__version__ = "0.1.0"
