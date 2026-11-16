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
- `xlsx/tests/t.rs` — defaults via accessors on a local newtype, column-ordered insertion within
  a band, `maxOccurs=1` refusal, `mc:AlternateContent` sibling refusal, `str::substr_range`.

Negative checks run by hand on 2026-11-16 (rustc 1.99.1), results quoted in ADR-0002:
- `insert::<CellValue>()` on a `Row` → E0277 with the `#[diagnostic::on_unimplemented]` message.
- `impl Lens<'_, Cell> { … }` in a downstream crate → E0116.
- a bad enum default in `Cell::ATTRS` → E0080 from the module-level `const _` check.

## `py/` — data checks (`python3 -I`)

- `xsd_defaults.py <ECMA Transitional XSD dir> <Open-XML-SDK data dir>` — joins SDK attributes to
  XSD `@default` by (namespace, complexType, attribute).
- `slottable.py <Open-XML-SDK data/schemas>` — counts content models slot-based insertion can handle.
