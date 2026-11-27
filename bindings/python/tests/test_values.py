"""Value objects (ADR-0023 §11.4, §12)."""

import copy

import ooxml_rs as R


def test_formula_renders_with_equals():
    f = R.Formula("A1*2")
    assert f.text == "A1*2"
    assert str(f) == "=A1*2"
    assert f == R.Formula("A1*2")
    assert hash(f) == hash(R.Formula("A1*2"))


def test_cell_error_is_a_value():
    e = R.CellError("#DIV/0!")
    assert e.code == "#DIV/0!"
    assert str(e) == "#DIV/0!"
    assert not isinstance(e, Exception)
    assert e == R.CellError("#DIV/0!")


def test_clear_is_a_singleton():
    assert R.CLEAR is copy.copy(R.CLEAR)
    assert R.CLEAR is copy.deepcopy(R.CLEAR)
    assert repr(R.CLEAR) == "CLEAR"
