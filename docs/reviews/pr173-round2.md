# PR #173 review round 2 (Opus via herdr-cocla)

- head: c7bb5f2
- verdict: YES (m3/n5 fixed alongside)
- job: 01a22034-4f26-7f5f-a4b3-1bd0f2d17d7d

# PR #173 review round 2 @ c7bb5f2 (209b476..c7bb5f2)

## Conclusion
converged: YES. M1, m1, m2 and n1–n4 are all fixed. No new Blocker or Major. One Minor (m3: the `Ambiguous*` bucket includes file-state kinds) and one nit.

## Verification
Static review: I read the full diff. I did not build the wheel; CI's python job, on both OSes, confirms it.

## Status of round-1 findings
- **M1, FIXED.** All 14 entries were checked against each variant's doc comment, and I agree with every one.
  - To Refused, an operation blocked on a valid file: ReferenceDeleted, NotExpressible, ShowRangeByPosition and BuiltinNumFormatRedefined. For BuiltinNumFormatRedefined the file legitimately redefines a built-in id, so this is a refusal, not a caller error.
  - To Refused, an optional part this library does not create yet: NoStylesPart, NoSettingsPart, NoTextBody, NoNotesSlide, NoNotesBody, NoSectionProperties, and also TableStyleInline (it is not replaced implicitly).
  - To InvalidInput: MasterIndexOutOfRange (the caller's index).
  - To MalformedFile: OutOfOrder (row/cell order in the file) and InvalidShapeId (`cNvPr@id` is missing or not a number).
  - `test_named_mappings` gains 20 rows: the 14, plus Random, InconsistentGrid, ImageSize, NameInUse, AmbiguousColumns and NotSmartArt. The rule is written into the TABLE doc comment.
- **m1, FIXED; the approach is sound.** On Windows the binding passes `winerror=raw_os_error()` and `errno=errno_from_kind(kind)`. CPython's `OSError.__init__`, when given an integer `winerror` on Windows, computes `errno` itself with `winerror_to_errno` and overwrites the passed value. So for real OS errors `errno` comes out correct, as `winerror` (ERROR_PATH_NOT_FOUND=3 → ENOENT); the hand table only applies when there is no raw code. The save test asserts ENOENT, and the Windows CI job checks it. The new `winerror` argument in `_errors.py` falls back to 3 arguments when it is None, so POSIX is unaffected.
- **m2, FIXED.** All four crates have `samples_cover_every_variant`: the `=>` arms in `kind()` are counted with `include_str!` and compared to the deduped sample kinds (in format crates, both sides exclude Core). Remove one sample and `kinds.len()` drops by one, so the assert fails. `kind()` is an exhaustive match with no nested match, so the `=>` count equals the variant count.
- **n1, FIXED.** `category_class` is shared by `convert` and `__raise`, so the pins now check the real selection, except that `__raise` passes `io_kind` as None and therefore only checks the OoxmlIOError base class.
- **n2, FIXED.** The xlsx `InvalidArgument { cell: Some("B2") }` sample is back, and `__surface_kinds` dedupes with a HashSet that keeps the original order. `test_table_covers_every_kind_exactly` checks `len(exported) == total` against the deduped lists, so it stays free of duplicates. Only xlsx has two samples of one kind, and docx/pptx/core have no duplicates.
- **n3, FIXED.** The AGENTS.md line moved after the oracle's code block.
- **n4, FIXED.** `check_manifest.py` compares the pytest pin in `requirements-ci.txt` with every `"pytest==…"` in ci.yml.

## New findings
**m3 (Minor): the `Ambiguous*` bucket includes kinds that are about the file.** The TABLE doc comment puts "mixed caller/file … name families (`ImageSize`, `NameInUse`, `Ambiguous*`)" into InvalidInput (a ValueError), and `AmbiguousColumns` is pinned there. By their own doc comments, these are file states, not caller input:
- xlsx `AmbiguousColumns`: "more than one `cols` element, overlapping `col` ranges";
- docx `AmbiguousStylesPart`: more than one styles part;
- docx `AmbiguousSettings`;
- pptx `AmbiguousShapeId`: "more than one shape on the slide carries this id";
- xlsx `AmbiguousStyleName`: several cell styles in the file have the same name.

This is the same category as `OutOfOrder`, which this round moved to MalformedFile. Under this PR's rule (an operation that cannot proceed on a valid file is Refused), these should be Refused, or MalformedFile where the file breaks a format invariant. Because the decision is documented and the impact is small (it is still an OoxmlError), I leave it at Minor: it can be fixed by CI or recorded as an ADR §11.3 note.

**n5 (nit):** `errno_from_kind` uses Linux values for `TimedOut`=110 and `ConnectionRefused`=111; on Windows, CPython's ETIMEDOUT is 138 and ECONNREFUSED is 107. It only matters when there is no raw code (a synthesized io::Error), so file operations essentially never hit it. Returning None for those kinds would be enough.

## Verdict
YES. m3 and n5 can go in a follow-up, accepted on CI.
