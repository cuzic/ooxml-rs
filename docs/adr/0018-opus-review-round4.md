# ADR-0018 review (Opus, round 4)

対象: plan/xlsx-phase7 @ 5efd18d3d6e1fa2ede95f1e7284069b2004b32eb

Scope: whether round 3's M1 and M2 are resolved, and whether revision 4 introduces any new
Blocker or Major.

I reran `python3 -I spikes/xlsx-phase7/structural_survey.py --summary /tmp/poi-xlsx
reference/fixtures/xlsx/calamine`. The new counters reproduce:

- `sheet_named_in_app_titles` 25 / 15;
- `wb_lockStructure` 4;
- `wb_lockStructure_sheets` 35;
- `sheet_no_blocker` 787 (unchanged).

## Conclusion first

Both Majors are resolved, and I found no new Blocker or Major.

**Counts: 0 Blocker, 0 Major, 0 Minor, 2 Nit.**

## Round-3 findings

### M1: resolved

- **Acceptance by dispatch.** A sheet-prefix hit is now accepted iff its `(row, part)` is in the
  operation's dispatch set, whatever the rule (§2.2). This is the rule round 3 asked for.
- **Rule `–` and the new rule K.** `–` now means "not dispatched", and the new rule **K**
  ("dispatched, kept") covers rows that hold the name but do not change.
- **Rows using K.** L49 (pivot-chart source), L60 (external links) and L73 (`app.xml` titles)
  are `(any)` and use K for G1. In the other columns their rules are unchanged:
  - L73 is R for sheet rename and R for delete;
  - L49 is R/X for rename and X for delete;
  - L60 is `–` but also a known non-reference.

  So sheet rename and delete keep today's behaviour.
- **Known non-references.** The list now copies `scan.rs`'s `NonReference` classification
  (`sheet_ops/mod.rs` L1129-1137): `docProps/` except L73, `sharedStrings`, comments, threaded
  comments, persons and `externalLink`. It adds the in-part non-references (`c/v`, `c/is/t`,
  `c/is/r/t`, formula string literals, external `[n]` references). That matches the code on
  main, so the switch to `LOCATIONS` no longer narrows what today's scan accepts.
- **Other `–` rows.** I checked the remaining rows that are `–` for G1 and found none that can
  hold the sheet name in prefix form:
  - raw names without `!`: L38, L51, L72;
  - pure positions: L1–L3, L33;
  - XX elsewhere: L58, L59.

  Oracle 7's new check ("a refusal naming a K or `–` place must not occur") would catch any I
  missed.
- **Scope check.** L32 and L73 are now `(any)`, so the compile-time check has no exemptions.
  This settles round 3's m1.

### M2: resolved

The P7-pre acceptance in Delivery 0 now has four clauses:

1. Byte-identical output and an equal `RewrittenLocation` list, for every rename and delete that
   succeeds today in a workbook without `lockStructure`.
2. `Protected` with bytes unchanged in the 4 `lockStructure` workbooks (35 sheets, listed by
   file).
3. Every operation that refuses today either still refuses with the same error, or succeeds and
   changes only the places listed in ADR-0009's implementation note, checked place by place.
4. 0 refusals other than `Protected` (35).

These clauses no longer conflict. The two intended changes are named in §2.2:

- refusal → rewrite (clause 3);
- success → `Protected` (clause 2).

Clause 1 covers everything else. The criterion is now well-defined and achievable. The
ADR-0009 post-acceptance note is planned.

## The `app.xml` count: 25 or 31

**Use 25 / 15**, the spike's count. The two methods agree file by file. The difference is in
the corpus, not in the matcher:

- My round-3 probe walked all 366 POI files. It included
  `clusterfuzz-testcase-minimized-POIXSSFFuzzer-5937385319563264.xlsx` (7 matching sheets),
  which is a fuzzing case outside the spike's 343 readable packages.
- My probe missed
  `craftonhills.edu_programreview_report.aspx_goalpriorityreport_0011d159-….xlsx` (1), which
  the spike reads.
- 31 − 7 + 1 = 25.

The ADR's note "31 / 15 with a broader matcher" (Context table) should therefore say "31
including one fuzzing case outside the 343 readable packages" (n1). Noting both is fine once
the reason is correct.

## Nit

- **n1.** Context table, `app.xml` row: the difference from 31 is corpus membership (the
  `clusterfuzz-…5937385319563264` fuzz case: 7; `craftonhills…`: −1), not a broader matcher.
- **n2.** §2.2's "A hit in a dispatched place whose rule is X or XX refuses because of that
  rule" should say "applies that rule". X refuses only when the place *would change*.
  - Example: a cross-sheet ctrlProps link (L56) to a cell above `p` must not refuse an insert
    at `p`.
  - Read literally, the sentence refuses on any hit. That fails closed, but it is stricter than
    the rule table.

## 判定

**Accept.** There are no Blockers or Majors (0 Blocker, 0 Major, 0 Minor, 2 Nit). The Nits can go
to a follow-up or into the acceptance commit without another review round (`docs/process.md`,
"Adversarial review rounds"). The ADR can be marked Accepted at the head that carries them, or
at `5efd18d` if they are deferred.
