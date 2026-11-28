"""The binding cannot inherit [workspace.package]; its metadata must match the root.

Usage: python -I ci/check_manifest.py ../../Cargo.toml Cargo.toml
Checks: rust-version, edition, license, version, authors, repository.
"""

import os
import re
import sys


def section(text: str, name: str) -> dict[str, str]:
    """Minimal TOML: top-level [section] key/value pairs (strings and arrays)."""
    out: dict[str, str] = {}
    in_section = False
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("["):
            in_section = line == f"[{name}]"
            continue
        if in_section and "=" in line and not line.startswith("#"):
            key, _, value = line.partition("=")
            out[key.strip()] = value.strip()
    return out


def main(root_path: str, own_path: str) -> int:
    root = section(open(root_path).read(), "workspace.package")
    own = section(open(own_path).read(), "package")
    keys = ["version", "edition", "rust-version", "license", "repository", "authors"]
    failed = False
    for key in keys:
        want, got = root.get(key), own.get(key)
        if want is None:
            print(f"root has no workspace.package.{key}")
            failed = True
        elif want != got:
            print(f"{key}: root {want} != binding {got}")
            failed = True
    if not re.search(r'^pyo3\s*=\s*"=', open(own_path).read(), re.M):
        print("pyo3 is not exactly pinned (=x.y.z)")
        failed = True
    # The CI yaml mirrors the pytest pin by hand; keep them in sync.
    here = os.path.dirname(os.path.abspath(own_path))
    req = open(os.path.join(here, "requirements-ci.txt")).read()
    ci = open(os.path.join(here, "..", "..", ".github", "workflows", "ci.yml")).read()
    want = re.search(r"^pytest==(.*)$", req, re.M)
    got = set(re.findall(r'"pytest==([^"]+)"', ci))
    if want is None:
        print("requirements-ci.txt has no pytest pin")
        failed = True
    elif got != {want.group(1)}:
        print(f"pytest pin: requirements-ci.txt {want.group(1)} != ci.yml {sorted(got)}")
        failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1], sys.argv[2]))
