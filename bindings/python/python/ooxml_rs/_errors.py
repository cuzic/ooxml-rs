"""Exception hierarchy (ADR-0023 §11.3).

Every error from the native module arrives as one of these. ``crate`` is
``"core"``, ``"xlsx"``, ``"docx"``, ``"pptx"`` or ``"binding"``; ``kind`` is
the Rust ``kind()`` string (or a binding kind); ``cell`` is an A1 address or
``None``; ``sheet`` is set when the call had one. The message is the Rust
``Display`` text.
"""

from __future__ import annotations


class OoxmlError(Exception):
    """Base of every ooxml-rs error."""

    def __init__(
        self,
        message: str = "",
        *,
        crate: str,
        kind: str,
        cell: str | None = None,
        sheet: str | None = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.crate = crate
        self.kind = kind
        self.cell = cell
        self.sheet = sheet

    def __str__(self) -> str:
        return self.message

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}({self.message!r}, crate={self.crate!r}, "
            f"kind={self.kind!r}, cell={self.cell!r}, sheet={self.sheet!r})"
        )


class RefusedError(OoxmlError):
    """The edit was not made; the handle is unchanged and usable."""


class PoisonedError(OoxmlError):
    """A bug poisoned the handle (or a panic did); reopen from the last save."""


class NotFoundError(OoxmlError, KeyError):
    """A name, sheet or style was not found."""


class InvalidInputError(OoxmlError, ValueError):
    """Caller input (not the file) is invalid."""


class UnsupportedFileError(OoxmlError):
    """Encrypted, Strict or DTD-carrying packages."""


class MalformedFileError(OoxmlError):
    """The file itself is broken."""


class OoxmlIOError(OoxmlError, OSError):
    """OS I/O failure, carrying ``errno``/``strerror``/``filename``."""

    def __init__(
        self,
        message: str = "",
        *,
        crate: str,
        kind: str,
        cell: str | None = None,
        sheet: str | None = None,
        errno: int | None = None,
        strerror: str | None = None,
        filename: str | None = None,
    ) -> None:
        OoxmlError.__init__(
            self, message, crate=crate, kind=kind, cell=cell, sheet=sheet
        )
        OSError.__init__(self, errno or 0, strerror or message, filename)
        # OSError.__init__ overwrote args; keep the Rust message visible.
        self.args = (message,)


class OoxmlFileNotFoundError(OoxmlIOError, FileNotFoundError):
    """The path does not exist."""


class OoxmlPermissionError(OoxmlIOError, PermissionError):
    """Permission denied."""
