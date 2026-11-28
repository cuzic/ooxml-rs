# PR #173 review round 1 (Opus via herdr-cocla)

- base: main cd24426, head: 209b476
- verdict: NO (M1 table mislabels + m1/m2/n1-n4)
- job: 01a2201d-0f94-7278-8a41-ebeee7f6ebfb

# PR #173 (U7 binding foundation) review round 1 @ 209b476

## Conclusion
converged: NO. Major M1: the category table mislabels several kinds, against its own stated rule. Workspace separation, the handle/GIL rule, OSError plumbing, the CI job and the pins are sound.

## Verification
Static review only: I read the full diff. I did not build the wheel (pyo3/maturin build); the PR body reports clippy, two maturin builds and pytest 47+2 passing locally.
- Sample counts before and after the hoist: core 39/39, docx 80/80, pptx 80/80, xlsx 108 → 107 (see n2).
- The table has 303 entries, equal to the `kind()` arm count: 39 + 107 + 80 + 80.

## Per-scope check
1. **Workspace: OK.**
   - Root `exclude = ["bindings/python"]`; its own `[workspace]` and Cargo.lock.
   - `pyo3 = "=0.30.1"`, `maturin==1.16.0`, `pytest==9.2.0`.
   - `check_manifest.py` checks 6 keys and the pyo3 exact pin.
   - CI: no path filter, ubuntu and windows, `shell: bash` with Scripts/bin switched by OS. The fault-injection build repeats the abi3 pair because `--features` overrides `tool.maturin.features`, which is correct.
2. **Table:** see M1.
3. **Handles: OK.** `#[pyclass(frozen)]` with `Mutex<Slot<T>>` and `Condvar`, no `unsendable`. `run` locks only inside `py.detach`. Arguments are converted to owned values before detach, and results are owned. A poisoned mutex raises `PoisonedError("binding","HandlePoisoned")`. No test hits the panic path; U7 has no edits that can trip it, and U8 will test it.
4. **Error conversion: OK.** Core errors wrapped by format crates are reported as `crate="core"`; `Io` is split by `io.kind()` into NotFound, Permission or the base class; errno, strerror and filename are passed through. Windows caveat in m1.
5. **pytest: OK.**
   - Coverage is exact (set equality plus a duplicate check).
   - All 31 mappings the ADR names are pinned.
   - FileNotFound on open and on save into a missing directory, Permission (POSIX), garbage bytes → Malformed for all 3 formats, an unknown pair → base class.
   - Fault-injection plumbing.
6. **`__error_samples` hoist:** the kind tests and `__surface_kinds` share one function, so they cannot drift apart. But see m2 and n2.

## Major
**M1: the table contradicts its own rule.** The `TABLE` comment in `bindings/python/src/errors.rs` says "`Unsupported*` and exhaustion are refusals (the operation cannot proceed, the file is not broken)". These entries do not follow that rule, judging by each variant's own doc comment.

Refusals of an operation on a valid file, mapped to `MalformedFileError`:
- `("xlsx","ReferenceDeleted")`: "A structural edit would delete a required reference".
- `("xlsx","NotExpressible")`: "cannot be expressed without losing content the registry does not model".
- `("pptx","ShowRangeByPosition")`: a valid presProps blocks the delete.

A valid file that lacks an optional part this library does not create yet, also mapped to `MalformedFileError`; by the stated rule these should be `RefusedError`:
- `("xlsx","NoStylesPart")`: "creating the part is out of scope".
- `("docx","NoSettingsPart")`: "creating one is Phase2".
- `("pptx","NoTextBody")`: for example a picture shape; "creating one is out of Phase 0 scope".
- `("pptx","NoNotesSlide")`: "creating one is not part of Phase 1".

Wrong direction in the caller/file split:
- `("pptx","MasterIndexOutOfRange")`: the caller passed an index. It is `MalformedFileError`; it should be `InvalidInputError` (or `NotFoundError`).
- `("xlsx","OutOfOrder")`: the rows or cells in the file are out of order (`index.rs:184`/`193`). It is `InvalidInputError`, which is also a `ValueError`; it should be `MalformedFileError` or `RefusedError`.

Why it matters: the exception classes are the main contract of the Python API. RPA flows branch on "the file is broken" versus "the caller's mistake" versus "the operation can't be done". With these entries, a valid file is reported as corrupt, and a file problem is reported as a caller `ValueError`. Changing a class after release is a breaking change, so U7, where the table is reviewed, is the place to fix it.

Fix:
- Correct the entries above.
- Re-check the other `Malformed` and `InvalidInput` entries against each variant's doc comment, for example `pptx NoNotesBody`, `docx NoSectionProperties` and `docx InconsistentGrid`.
- Pin the judgement-call kinds in `test_named_mappings` as well.
- Record the rule in the ADR §11.3 note or the PR body.

## Minor
**m1: on Windows, `errno` receives a Win32 code.** `errors.rs` `convert` sets `errno = io.raw_os_error()`, and `_errors.py` passes it as `OSError.__init__(self, errno or 0, ...)`. On Windows `raw_os_error()` is a Win32 error code, so saving into a missing directory (`ERROR_PATH_NOT_FOUND`=3) gives `errno=3` (ESRCH). The class is chosen correctly from `io.kind()`; only the attribute is wrong. The existing tests do not catch it: on open, `ERROR_FILE_NOT_FOUND`=2 happens to equal ENOENT, and the save test does not check errno.
- Fix: on Windows, pass the code as `winerror` (the 4th argument of `OSError`) and let Python map errno.
- Test: assert `errno == errno.ENOENT` in the save test as well.

**m2: a variant without a sample silently drops out of the table check.** `__surface_kinds` is derived from `__error_samples`. When someone adds a variant, `kind()` is forced at compile time, but the sample is not. A variant added without a sample is missing from both `__kinds` and `__table_keys`, the coverage test still passes, and at runtime the error falls into the `OoxmlError` base class. ADR §11.3 says "a new Rust variant cannot fall silently into the base class", and that does not hold as built.
- Fix: in each crate, add a test that checks the number of samples equals the number of `kind()` arms, for example by counting them from the source with `include_str!`. Alternatively, generate the variant list with a macro.

## nit
- **n1: `__raise` repeats `convert`'s class selection.** It has its own copy of the `match` from `convert`, so `test_named_mappings` verifies the copy, not `convert`. A mistake in `convert`'s class arms (other than Io) would pass the tests. Factoring out a shared `category_class(cat, io_kind)` would fix this.
- **n2: a cell() sample was removed.** To keep `__surface_kinds` free of duplicates, the xlsx `InvalidArgument { cell: Some("B2") }` sample was deleted, so the `Some` path of `cell()` is no longer in the sample test. phase2 and others still cover it, but it is better to keep the sample and dedupe inside `__surface_kinds`.
- **n3: the AGENTS.md line is in the wrong place.** The new `python` job line went between the python-pptx oracle bullet and its "手元で回す場合" continuation, so the oracle's local commands now read as if they belonged to the python job. Move it after the code block.
- **n4: the pytest pin is duplicated.** It is written in both the CI yaml and `requirements-ci.txt`; the PR body says they are kept in sync by hand. Having `check_manifest.py` compare them would be safer.
- **Info:** the U6 kinds (`NoSuchName`, `AmbiguousName`, `NameNotARange`) are absent because U6 is not on main yet. When the PRs are restacked, the coverage test will force them in; in ADR §11.3 they are NotFound/InvalidInput/InvalidInput.

## Verdict
NO. Fix M1 (correct the entries, pin the judgement calls, record the rule). m1 and m2 should go in the same change.
