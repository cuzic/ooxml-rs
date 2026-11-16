# ADR-0002 spike

Throwaway evidence for [ADR-0002](../../docs/adr/0002-schema-driven-code-generation.md).
Not part of the workspace; nothing here is production code.

## `ws/` — Rust 1.99.1 compile/behaviour checks

```sh
cd ws && cargo test
```

- `core` stands in for `ooxml-core` (specs, `ChildOf`, toy raw tree, slot insertion).
- `schema` stands in for the generated `ooxml-schema` crate (`cfg_select!`-gated `sml` module,
  hand-written in the generated shape).
- `xlsx/tests/t.rs` (revision-3 rules) — defaults and raw-preserving `AttrError` via accessors on a
  local newtype; column-ordered insertion within a slot; path-based `LensMut` marking every ancestor
  `subtree_dirty`; `own_dirty` only for an originally self-closing parent gaining its first child;
  sequential child-view reborrow; `maxOccurs=1` refusal; `mc:AlternateContent` sibling refusal;
  `str::substr_range` (returns `core::range::Range`).

Negative checks run by hand on 2026-11-16 (rustc 1.99.1), re-run after the revision-3 changes,
results quoted in ADR-0002:
- `insert::<CellValue>()` on a `Row` → E0277 with the `#[diagnostic::on_unimplemented]` message.
- `impl Lens<'_, Cell> { … }` in a downstream crate → E0116.
- a bad enum default in `Cell::ATTRS` → E0080 from the module-level `const _` check.

## `py/` — data checks (`python3 -I`)

- `join3.py <ECMA Transitional XSD dir> <Open-XML-SDK data dir>` — namespace-correct default join
  (attributeFormDefault/form, attributeGroup, global-attribute `ref`, one extension level): 1,511.
- `typecheck.py <same args>` — joined defaults outside the SDK type's lexical space: 39 (none in SML).
  (`join2.py` is its helper.) `join*.py`/`typecheck.py` were written by the round-2 reviewer.
- `slottable.py <Open-XML-SDK data dir> <ECMA Transitional XSD dir>` — §4.1 slot lowering counts and
  the SDK-vs-ECMA order cross-check.
