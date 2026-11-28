"""Fault-injection build: the hook arms and reads never trip it.

Runs against the fault-injection wheel only (U8's edits will trip faults;
U7 has no edits, so this pins the plumbing).
"""

import pytest

import ooxml_rs as R


@pytest.mark.fault_injection
def test_inject_fault_arms_and_reads_ignore_it():
    wb = R.Workbook.new()
    inject = getattr(wb, "__inject_fault")
    assert inject("Registries", False) is False
    assert inject("Target", False) is True
    assert wb.sheet_names() == ["Sheet1"]
    assert R.Workbook.from_bytes(wb.to_bytes()).sheet_names() == ["Sheet1"]


@pytest.mark.fault_injection
def test_inject_fault_rejects_unknown_steps():
    wb = R.Workbook.new()
    with pytest.raises(ValueError, match="unknown apply step"):
        getattr(wb, "__inject_fault")("Nope", False)
