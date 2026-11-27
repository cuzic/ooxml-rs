"""Small value objects shared by the binding (ADR-0023 §11.4, §12)."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Formula:
    """A formula with text (no ``=``). Round-trips through reads and writes."""

    text: str

    def __str__(self) -> str:
        return "=" + self.text


@dataclass(frozen=True)
class CellError:
    """A spreadsheet error value (``#DIV/0!`` and friends). A value object,
    not an exception."""

    code: str

    def __str__(self) -> str:
        return self.code


class _Clear:
    """Sentinel: clear the value (never creates a blank cell)."""

    _instance = None

    def __new__(cls) -> "_Clear":
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

    def __repr__(self) -> str:
        return "CLEAR"

    def __copy__(self) -> "_Clear":
        return self

    def __deepcopy__(self, _memo: object) -> "_Clear":
        return self


CLEAR = _Clear()
