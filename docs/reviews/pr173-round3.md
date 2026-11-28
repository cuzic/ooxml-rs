# PR #173 review round 3 (Opus via herdr-cocla)

- head: eefd512
- verdict: YES (n6 fixed alongside)
- job: 01a2203d-627a-76c7-bb1f-d15bdca4201e

# PR #173 review round 2 (re-check) @ eefd512 (209b476..eefd512)

## Conclusion
converged: YES. M1, m1, m2 and n1–n4 are all fixed, and the earlier round-2 follow-up (eefd512) also fixes m3 and n5. One new nit: a stale doc comment.

## Verification
- Exported the PR tree with git archive (probe directory deleted afterwards). The error tests for core and pptx (3 each) pass.
- Mutation: deleting the core `EncryptedPackage` sample fails `samples_cover_every_variant` ("a variant lacks a sample", error.rs:571).
- Mutation: deleting the pptx `NoTextBody` sample also fails it (error.rs:725).
- I did not build the wheel or run pytest.

## Status of round-1 findings
- **M1, FIXED.** All 14 entries match each variant's doc comment:
  - Refused: ReferenceDeleted, NotExpressible, ShowRangeByPosition, BuiltinNumFormatRedefined, NoStylesPart, NoSettingsPart, NoTextBody, NoNotesSlide, NoNotesBody, NoSectionProperties, TableStyleInline.
  - InvalidInput: MasterIndexOutOfRange.
  - MalformedFile: OutOfOrder, InvalidShapeId.

  `test_named_mappings` has 20 rows added in round 1, plus 6 more `Ambiguous*` rows in eefd512.
- **m1, FIXED; the approach is sound.**
  - **Windows:** the binding passes `winerror=raw_os_error()` and `errno=errno_from_kind(kind)`. For an integer `winerror`, CPython's `OSError` init on Windows overwrites errno with `winerror_to_errno(winerror)`, so real OS errors get the value CPython itself would report. The kind table only applies when there is no raw code.
  - **The table** after eefd512: NotFound=2, PermissionDenied=13, AlreadyExists=17, InvalidInput/InvalidData=22, Interrupted=4, BrokenPipe=32, everything else None. These are ENOENT/EACCES/EEXIST/EINVAL/EINTR/EPIPE, which have the same values in POSIX and in the MSVC CRT, so they are portable. TimedOut/ConnectionRefused were removed (round 2 n5).
  - **POSIX** is unchanged: `raw_os_error()` is used as is. The save test asserts ENOENT, and the Windows CI job runs it.
- **m2, FIXED.** In all four crates the `kind()` `=>` count read with `include_str!` equals the deduped sample kinds (format crates exclude Core on both sides). Both mutations above make the test fail.
- **n1, FIXED.** `category_class` is shared by `convert` and `__raise`.
- **n2, FIXED.**
  - The xlsx `InvalidArgument` Some/None samples are back, and `__surface_kinds` dedupes keeping the original order (HashSet).
  - The total is core 39 + xlsx 106 + docx 79 + pptx 79 = 303 unique. The format crates exclude Core, since it surfaces as core.
  - `test_table_covers_every_kind_exactly` checks `len(exported) == total == len(table)`.
- **n3, FIXED.** The AGENTS.md line moved after the oracle's code block.
- **n4, FIXED.** `check_manifest.py` checks that the pytest pins in requirements-ci.txt and ci.yml match.
- **Round 2 m3/n5, FIXED.** The 6 file-state `Ambiguous*` kinds became Refused, with pins: xlsx AmbiguousColumns and AmbiguousStyleName; docx AmbiguousStyle, AmbiguousStylesPart and AmbiguousSettings; pptx AmbiguousShapeId. docx `AmbiguousStyle` is "Two `w:style` elements have this id", which is file state, so Refused fits.

## New findings
- **n6 (nit):** the `TABLE` doc comment in `bindings/python/src/errors.rs` is stale. It still says "mixed caller/file image and name families (`ImageSize`, `NameInUse`, `Ambiguous*`) -> InvalidInput", but after eefd512 `Ambiguous*` is Refused. Drop `Ambiguous*` from that line and add "file-state ambiguity (`Ambiguous*`)" to the Refused examples.

## Verdict
YES. Fix n6 on CI.
