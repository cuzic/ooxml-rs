"""Compare two phase7_probe logs (ADR-0018 Delivery 0)."""

import argparse
from collections import Counter
from pathlib import Path

# The changed sites are exercised by the independent XML value oracle in
# phase7_pre.rs.
NEWLY_SUPPORTED = {
    ("56644.xlsx", 1, "rename"),
    ("56644.xlsx", 1, "delete"),
    ("61652.xlsx", 1, "rename"),
    ("61652.xlsx", 1, "delete"),
    ("LIBRE_OFFICE-128382-0.xlsx", 0, "rename"),
    ("StructuredRefs-lots-with-lookups.xlsx", 1, "rename"),
    ("StructuredRefs-lots-with-lookups.xlsx", 1, "delete"),
    ("WithChartSheet.xlsx", 5, "rename"),
    ("dataValidationTableRange.xlsx", 0, "rename"),
}


def read_log(path):
    operations = {}
    for line in path.read_text().splitlines():
        if line.startswith("P7\t"):
            _, file, index, operation, *result = line.split("\t")
            key = file, int(index), operation
            if key in operations:
                raise ValueError(f"{path}: repeated operation {key}")
            operations[key] = result
    return operations


def compare(before, after):
    problems = []
    counts = Counter()
    if before.keys() != after.keys():
        problems.append("the two runs did not cover the same operations")
    if not before or not after:
        problems.append("the runs must contain operations")

    for key in sorted(before.keys() & after.keys()):
        operation = key[2]
        old, new = before[key], after[key]
        counts[operation, "tested"] += 1
        if new[0] == "ERR" and new[1].startswith("Protected {"):
            counts[operation, "protected"] += 1
        elif new == old:
            counts[operation, f"same {new[0]}"] += 1
            if operation == "rename" and new[0] == "OK" and int(new[-1]):
                counts[operation, "retargeted relationships"] += 1
        elif key in NEWLY_SUPPORTED and old[0] == "ERR" and new[0] == "OK":
            counts[operation, "new success"] += 1
        else:
            problems.append(f"unexpected change in {key}")

    for (operation, outcome), count in sorted(counts.items()):
        print(f"{operation:6} {outcome:26} {count:4}")
    if counts["rename", "tested"] != 838 or counts["rename", "same ERR"] != 0:
        problems.append("clause 4 requires 838 renames and no non-protection refusals")
    if counts["rename", "protected"] != 35 or counts["delete", "protected"] != 35:
        problems.append("expected 35 protected sheets")
    if counts["rename", "retargeted relationships"] != 18:
        problems.append("expected 18 relationship retargets")
    for problem in problems:
        print(problem)
    return int(bool(problems))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    args = parser.parse_args()
    raise SystemExit(compare(read_log(args.before), read_log(args.after)))
