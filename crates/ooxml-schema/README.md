# ooxml-schema

Generated schema knowledge for ooxml-rs
([ADR-0002](../../docs/adr/0002-schema-driven-code-generation.md)):
`ElementSpec` markers, `ChildOf` impls, enums, and typed `*View` /
`*ViewMut` newtypes over `ooxml-core`'s lenses.

- Features: `sml` (default), `wml`, `pml`, `dml` — one per allowlist file
  `codegen/allowlist/<group>.toml` (`[write]` / `[read]` sections of SDK
  `Name`s). Children in another group get `ChildOf` under
  `cfg(all(feature = …, feature = …))`.
- `src/generated/` is written by `cargo xtask codegen` and checked by
  `cargo xtask codegen --check`. Do not edit it; change the allowlist,
  `codegen/overrides.toml`, or the generator, and review the generated diff
  together with `codegen/report.txt`.
- Notices for the embedded Open XML SDK and ECMA-376 content:
  `THIRD-PARTY-NOTICES`.

Naming: marker = SDK class name (`Cell`, `Row`, `CellValue`; prefixed with the
namespace prefix on a clash within a group), views `CellView` /
`CellViewMut`, attribute name constants `Cell::ATTR_S`, attribute getters
from the SDK property name (`style_index()`; absent → XSD default), setters
`set_*` / `remove_*`, child accessors `cell_value()` (max 1) or
`cell_iter()` (repeating), and on `*ViewMut` `get_or_insert_*`,
`insert_*` / `insert_*_by`, `*_mut`, `*_mut_by`, `remove_*`.
