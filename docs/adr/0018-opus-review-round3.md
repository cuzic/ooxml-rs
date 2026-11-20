# ADR-0018 review (Opus, round 3)

対象: plan/xlsx-phase7 @ cd2177400a98c121dd1e4a0c60d595a09798bdbc

Scope: revision 3, focused on round 2's M1–M3. For the m1–m8 fixes and the new `lockStructure`
behaviour change, I raise only Blocker/Major issues. I checked against:

- the ADR text;
- `crates/ooxml-xlsx/src/sheet_ops/{scan.rs,mod.rs}` on main @ `1000645`;
- [MS-XLSX] §2.2.4.12 "Worksheet", the page fetched in round 2;
- the POI corpus in `/tmp/poi-xlsx`, with targeted probes (not committed).

## Conclusion first

- **Round-2 M2 (EXTENSIONS) is resolved.** All eleven URIs and their element mappings match
  MS-XLSX §2.2.4.12 character for character. `{EB79DEF2-…}` is gone, URIs compare
  case-insensitively, and reference-shaped content under a *known* URI at an unregistered path
  now refuses (L469-479).
- **Round-2 M1 is resolved for its target:**
  - every `Formula`/`Location` row that can occur in more than one part is now `(any)`,
    including L30, L42, L48, L56, L70 and L71;
  - a compile-time scope check exists;
  - the prefix scan accepts only dispatched places.
- But the new acceptance rule has a side effect (**M1** below).
- **Round-2 M3 is resolved structurally.** L71–L73 and the sheet-rename and sheet-delete
  columns exist, and the ADR-0009 ↔ `LOCATIONS` test is specified. The P7-pre acceptance
  criterion, however, contradicts the `lockStructure` change shipped in the same PR (**M2**
  below).

**Counts: 0 Blocker, 2 Major, 1 Minor, 1 Nit.**

---

## Major

### M1. "Accepted only in a place visited with a rule other than `–`" makes every G1 edit refuse on 31 sheets with print-area or print-titles names

**Problem.**

- The round-2 M1 fix (L451-460) accepts a sheet-prefix hit only in a registry place visited "with
  a rule other than `–`", or in a known non-reference.
- Several rows hold the sheet's name in prefix form **and** have `–` for G1, because a
  structural edit does not change them:

| Row | Place | G1 rule | Prefix-form content |
|---|---|---|---|
| L73 | `docProps/app.xml` "Named Ranges" titles | – | `Data!Print_Area`, `'My Sheet'!Print_Titles` |
| L49 | chart `c:pivotSource/c:name` | – (pivot rules) | `[file.xlsx]Pivot!PivotTable1` |
| L60 | `externalLink` (another workbook's names: `definedName/@refersTo="=Sheet1!$A$1"`) | – | `Sheet1!` when a local sheet has the same name |

- L637 excludes "L73's `app.xml` titles" from the known non-references. A G1 edit on any sheet
  named in an `app.xml` title therefore refuses with `UnrewritableReference`.
- **Probe:** 31 of 821 sheets (15 packages) are named in prefix form in `app.xml`. That is more
  than every XX blocker together (26, L199). These sheets are counted neither in the 787
  "no blocker" figure nor in Consequences.
- L49 adds the pivot-chart source sheet (1 file). Per L52 that sheet should refuse only edits
  that cross the pivot table.
- L60 is 0 in the corpus (probe: no external-link part contains a local sheet name in prefix
  form). But today's code classifies the whole externalLink part as `NonReference`
  (`sheet_ops/mod.rs` L1134-1137, "another workbook's names and sheets (review R2 nit)").
- The known non-references list (L636-637) does not carry that over. It also omits `persons`
  and threaded-comment text, which `scan.rs`'s `NonReference` covers (L33-35). So the switch
  can also refuse *sheet renames* that succeed today. The corpus acceptance would not catch
  this, because the corpus has 0 such cases.

Round 2's M1 asked for the scan to reject hits in places that were **not dispatched** for that
part. It did not ask for rejection of places dispatched with an "unaffected" rule. The
revision uses `–` for both meanings.

**Fix.**

1. Accept a hit iff its `(row, part)` was in the operation's dispatch set, **whatever the
   rule**, and refuse it only when that rule is X/XX. Dispatch L49, L60 and L73 with scope
   `any` and a no-op rule. Rename that rule to "– (dispatched, unchanged)", or add a rule
   `K` = kept.
2. Copy `scan.rs`'s `NonReference` set into the known non-references list (L636-637):
   - shared strings;
   - comments and threaded comments;
   - persons;
   - `docProps` except L73;
   - `externalLink` parts.

   Then the ADR-0009 ↔ `LOCATIONS` test can assert it too.
3. Spike or oracle 7: count G1 refusals by reason on the corpus. A refusal that names a `–`
   place must be 0.

### M2. P7-pre's acceptance criterion contradicts the `lockStructure` refusal shipped in the same PR

**Problem.** P7-pre (L1345-1366) contains two items that cannot both hold:

- "every ADR-0009 rename and delete that succeeds before the switch gives byte-identical
  output after it", and "0 refusals in 838 renames";
- "the `lockStructure` refusal for ADR-0009's sheet operations".

Consequences (L1420-1422) confirms that these operations "start refusing with `Protected` in
the 4 corpus workbooks with `lockStructure`, where they succeed today". **Probe:** those 4
workbooks hold 35 sheets:

- `CVLKRA-KYC_Download_File_Structure_V3.1.xlsx` 30;
- `workbookProtection-workbook_password_user_range-2010.xlsx` 3;
- `workbookProtection-workbook_password-2013.xlsx` 1;
- `workbookProtection_workbook_structure_protected.xlsx` 1.

So roughly 35 of the 838 corpus renames go from success to `Protected`. Both acceptance bullets
fail as written.

The author's statement "P7-pre turns those refusals into the R of this table, and **nothing
else changes**" (L543-545) is therefore also false. Two output changes are intended, not one:

1. refusal → rewrite, for the places in ADR-0009's implementation note;
2. success → `Protected`, under `lockStructure`.

Change (1) does not contradict "byte-identical for every rename/delete that succeeds today".
Those renames refused before, so they are outside the set. Change (2) does contradict it.

**Fix.** Make the criterion well-defined and checkable:

- **(a)** For every rename and delete that succeeds today **in a workbook without
  `lockStructure`**: byte-identical output, and `RewrittenLocation` lists that are equal.
- **(b)** In a workbook with `lockStructure`: `Err(Protected)`, with the output byte-identical
  to the input (35 renames plus the deletes and moves in the 4 files).
- **(c)** For every operation that refuses today: it either still refuses with the same error,
  or succeeds and every changed place is a registry place listed in ADR-0009's implementation
  note, verified by oracle 7's per-place check.
- **(d)** "0 refusals in 838 renames" becomes "0 refusals other than `Protected` (35)".

Alternatively, land the `lockStructure` refusal as a separate commit after the switch and
measure each separately. Record the behaviour change as a post-acceptance note on ADR-0009,
since it changes an Accepted ADR.

---

## Minor

- **m1 (scope-check consistency).**
  - The compile-time check (L496-497) requires every `Formula`/`Location` row to be `any`.
  - L32 (`definedName`, `Formula(none)`, part "workbook") and L73 (`Location`, `app.xml`) are
    not marked `(any)`. Both are single-part kinds, so the marking changes nothing, but the
    check as written fails on them.
  - Either mark them `(any)` or limit the check to part kinds that can occur more than once.

## Nit

- **n1.** The ADR has no recorded corpus count of sheets named in `app.xml` titles. M1's fix
  should add that count (31 / 15) to the Context table, so that a regression shows up in the
  spike.

---

## Round-2 findings: status

| Finding | Status | Evidence |
|---|---|---|
| R2-M1 (X/XX formula rows; scan accepted any registry place) | **Resolved**, with a new side effect (M1) | L30, L42, L48, L56, L70 and L71 are `(any)`; scope check L496-497; scan rule L451-460 |
| R2-M2 (EXTENSIONS) | **Resolved** | L502-524 match MS-XLSX §2.2.4.12 (11 URIs, element mappings, `cfRule`/`sortState`/`filterColumn` extensions); known-URI rule L469-479; swapped-mapping unit case L522-524; L74 added |
| R2-M3 (registry ⊇ ADR-0009 §6) | **Resolved** in the table; acceptance contradicts the `lockStructure` change (M2) | L71 (`#Sheet!` targets), L72 (`@sheet`), L73 (`app.xml`); two sheet columns; ADR-0009 ↔ `LOCATIONS` test L498-500 |
| R2-m1–m8, n1–n2 | No Blocker/Major found except M2 (from m8) | – |

## 判定

**Not accepted** (0 Blocker, 2 Major).

- M1 needs a one-rule change to the scan's acceptance (dispatched vs. "rule ≠ `–`") and the
  non-reference list copied from `scan.rs`.
- M2 needs the P7-pre acceptance criterion split into (a)–(d).

Both are local text fixes with no design change. Round 4 can be limited to checking them, and
it can accept the ADR if they land as described.
