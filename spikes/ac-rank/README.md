# AC-rank spike (evidence for ADR-0020)

Evidence for [ADR-0020](../../docs/adr/0020-alternatecontent-rank.md): the rank of an
`mc:AlternateContent` (AC) sibling and inserts next to it. This is a throwaway script, not
production code.

- **Untrusted input.** Packages are read with `zipfile` + `xml.etree` only, under `python3 -I`,
  from the repository root (not from the corpus directory). Parts with a DTD or an entity
  declaration are skipped; no entity is resolved; nothing is extracted or executed. Packages are
  de-duplicated by SHA-256.
- **Ranks are the library's.** The slot tables and `impl ChildOf<P> for C` lines are parsed out of
  `crates/ooxml-schema/src/generated/*.rs`, so ranks, limits and "which typed inserts exist" are
  exactly what `ooxml-core`'s lens uses today (ADR-0002 §4.1). A parent with no generated spec is
  reported as `not-generated` (no typed insert exists there today).

| Script | Usage | Result |
|---|---|---|
| `ac_rank.py` | `python3 -I spikes/ac-rank/ac_rank.py crates/ooxml-schema/src/generated DIR…` | [`results/ac-rank.txt`](results/ac-rank.txt) |

The module docstring defines every counter and the rule (ADR-0020 revision 2, §3–§5). In short:

- `parents_with_ac`, `ac_children`: elements with an AC **direct child**, and the ACs; the
  per-parent table gives the parent QName, the generated spec status, and the branch contents
  (`C:` = `mc:Choice`, `F:` = `mc:Fallback`).
- Rank evidence, for ACs whose parent has a generated slot table: `ac_single_rank`,
  `ac_branch_spans_slots`, `ac_branches_disagree`, `ac_unranked` (a non-empty branch with no listed
  member), `ac_malformed` (§3.1, with `Requires` prefixes checked **in scope** at each `mc:Choice`),
  `ac_disorder`, `ac_all_branches_empty`, `branch_empty`, `branch_nonempty_unlisted_only`,
  `branch_starts_unlisted`, `branch_with_nested_ac`, `requires=…`, `requires_prefix_not_in_scope`,
  `parent_correlated_acs` (two or more sibling ACs with the same `Requires` set, §3.2; listed with
  their outcomes). A counter that does not appear is 0.
- `pair_*`: for every (parent instance with an AC child, typed child `C` with `ChildOf<parent>`),
  the outcome of a **plain append** of `C` under ADR-0020: `ok`, `limit` (`AlreadyPresent` from
  direct children alone), `limit_ac` (`AlreadyPresent` only because a branch already holds that
  child), `straddle`, `disorder`, `unranked`, `malformed`. `pair_ok_same_slot_ac` counts the `ok`
  pairs where an AC is in `C`'s slot (or an AC has only empty branches): the callback and positional forms (`insert_by`,
  `insert_at`, …) refuse those (§4.2). Today every pair is `Err(UnresolvedSibling)`.
  `pair_old_rule_wrong` counts `ok` appends that ADR-0002 §4.3's point rule would place out of
  order. Wildcard-only parents (`a:graphicData`) are counted apart (`wildcard_parent_pair_*`).

## Corpora (not committed except `reference/fixtures`)

| Label | Source | Pin |
|---|---|---|
| POI xlsx | Apache POI `test-data/spreadsheet/*.{xlsx,xlsm}` | `47dcff769bb395034dddfd59c4609659b6dbc6a4` (as `spikes/xlsx-phase7`) |
| POI docx | Apache POI `test-data/document/*.docx` | same commit (as `spikes/docx-phase*`) |
| POI pptx | Apache POI `test-data/slideshow/*.pptx` | same commit (as `spikes/pptx`) |
| fixtures | `reference/fixtures/{xlsx,docx,pptx}` | this branch |

Reproducing (Apache-2.0 files, fetched to a scratch directory outside the repository):

```sh
SHA=47dcff769bb395034dddfd59c4609659b6dbc6a4; T=$(mktemp -d)
gh api "repos/apache/poi/git/trees/$SHA?recursive=1" --jq '.tree[].path' > "$T/tree.txt"
for k in "spreadsheet xlsx|xlsm poi-xlsx" "document docx poi-docx" "slideshow pptx poi-pptx"; do
  set -- $k; mkdir -p "$T/$3"
  grep -E "^test-data/$1/[^/]*\.($2)\$" "$T/tree.txt" | while read -r p; do
    curl -sfL -o "$T/$3/$(basename "$p")" "https://raw.githubusercontent.com/apache/poi/$SHA/$p"; done
done
python3 -I spikes/ac-rank/ac_rank.py crates/ooxml-schema/src/generated \
  "$T/poi-xlsx" "$T/poi-docx" "$T/poi-pptx" reference/fixtures > results/ac-rank.txt
```

(The committed result was made with the xlsx set at `/tmp/poi-xlsx`, fetched by the
`spikes/xlsx-phase7` recipe: 366 files. The directory order does not change the counts; a file
that is both in POI and in `reference/fixtures` is counted once.)

Measured on 2026-11-22 with Python 3.15, on `main` at `d066ff1` (the generated tables of that
commit); revision 3 of the script (after ADR-0020 review round 2: interval statistics from the
real greedy `cur`, and an all-empty AC counted as same-slot for the positional forms). The
output is identical to revision 2's. Runs in about 1 s.

## Core placement verification

```sh
python3 -I spikes/ac-rank/check_core.py reference/fixtures
python3 -I spikes/ac-rank/check_core.py "$T/poi-xlsx" "$T/poi-docx" "$T/poi-pptx" reference/fixtures
```

The verifier uses the same corpus filtering and typed pairs as the spike. It
compares each simulated outcome with the Rust append and callback checks, and
checks whether any item index admits a positional insertion. It also executes
the append and checks byte equality on refusal. A scope declaration supplies
the typed child's namespace when needed so the comparison measures placement.
The temporary probe builds under the clone's `target/ac-rank` directory.

## Results (`results/ac-rank.txt`)

| | xlsx | docx | pptx |
|---|---:|---:|---:|
| packages read (distinct) | 378 | 123 | 102 |
| packages with an AC | 89 | 10 | 25 |
| ACs (direct children of some element) | 466 | 34 | 51 |
| of them under a parent with a generated slot table | 276 | 23 | 43 |
| ranked ACs whose branches **disagree** in rank | 0 | 0 | 0 |
| ranked ACs with a branch spanning **several slots** | 0 | 0 | 0 |
| malformed ACs (incl. `Requires` prefix not bound in scope) | 0 | 0 | 0 |
| non-empty branches with no listed member / starting with an unlisted member | 0 / 0 | 0 / 0 | 0 / 0 |
| ACs with an **empty** branch | 171 | 2 | 0 |
| ACs without `mc:Fallback` | 248 | 0 | 0 |
| ACs with a nested AC directly in a branch | 0 | 0 | 0 |
| parents with ≥ 2 ACs sharing a `Requires` set | 6 | 0 | 0 |
| typed inserts at AC parents (today all refused) | 622 | 440 | 97 |
| … placed by a plain append (`ok`) | 256 | 411 | 41 |
| … of those with an AC in the slot (positional forms refuse) | 21 | 411 | 6 |
| … `AlreadyPresent` from direct children | 366 | 27 | 38 |
| … `AlreadyPresent` only through a branch (`limit_ac`) | 0 | 2 | 18 |
| … straddle / disorder / unranked / malformed | 0 | 0 | 0 |
| … where the recorded point rule would mis-place | 0 | 0 | 0 |

Parents with an AC child (instances; what the branches hold):

- **xlsx:** `x:workbook` 79 (`x15ac:absPath`, Choice only); `xdr:wsDr` 5 parents / 170 ACs
  (`xdr:twoCellAnchor`, Fallback empty); `x:worksheet` 2 (`x:controls`, Choice only;
  `62629_target.xlsm`, `mv-calculator-final-2-20-2013.xlsm`); `x:fonts`/`x:cellXfs`/`x:dxfs` 1
  each (`style-alternate-content.xlsx`); `a:p` 1 (`a14:m`). Not generated: `c:chartSpace` 16
  (`c:style`), `x:controls` 2 / 164 ACs (`x:control`), `x:oleObjects` 4 / 7 ACs, and 3 Strict
  workbooks.
- **docx:** `w:r` 20 (`w:drawing` / `w:pict`), `w:body` 1 (`w:p`, Fallback empty;
  `sdk-HelloO14.docx`), `w:lvl` 1 (`w:numFmt`), `pic:pic` 1 (`pic:blipFill`). Not generated:
  `c:chartSpace` 6, `wp:anchor` 4 (`wp:positionH`), `w:fonts` 1.
- **pptx:** `a:graphicData` 23 (`p:oleObj`), `p:sld` 12 / `p:sldLayout` 4 / `p:sldMaster` 2
  (`p:transition` in both branches), `p:pic` 1 (`p:blipFill`), `p:spTree` 1 (Choice
  `p:graphicFrame`, Fallback `p:pic`; `Of16-01.pptx`). Not generated: `c:chartSpace` 8.

Notable typed inserts (outcome under ADR-0020):

- `x:workbook` + `x:definedNames` (ADR-0014 `set_autofilter`): 59 ok, 20 already present.
- `x:worksheet` (AC wrapping `x:controls`) + any of its 23 typed children (`x:autoFilter`,
  `x:hyperlinks`, `x:tableParts`, `x:legacyDrawing`, …): each is `ok` or a genuine
  `AlreadyPresent` (both sheets already have `legacyDrawing`, `drawing`, `extLst`).
- `w:body` + `w:p` (`append_paragraph` on `sdk-HelloO14.docx`, a plain `insert`): ok; the AC is
  in the `w:p` slot, so positional body inserts stay refused.
- `w:r` + `w:t`/`w:br`/`w:tab`/… on the 20 runs that wrap a drawing: ok as a plain append only
  (every docx pair is same-slot: `pair_ok_same_slot_ac` = 411).
- `p:sld` + `p:extLst` (ADR-0019 `commentRel`): 12 ok; `p:timing` 9 ok, 3 already present.
- `p:sld`/`p:sldLayout`/`p:sldMaster` + `p:transition`: 18 `limit_ac`: the transition exists only
  inside the AC, so an insert that ignored the branches would write a second one.
