# ADR-0002: Schema-driven code generation for the typed lens

- Status: **Accepted** (revision 5; `0002-opus-review-round3.md` approved acceptance at revision 4. Revision 5 corrects the slot rules after the Word/PowerPoint check and settles the XSD licence. `0002-opus-review-round4.md` reviewed that delta, found Minor issues only and approved keeping it Accepted; its fixes are folded in.)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md)

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (after `0002-opus-review-round1.md`): attribute
  defaults now come from a second input, the ECMA-376 Transitional XSDs,
  because the SDK data has none (B1); generated accessors move onto
  local view newtypes in a new `ooxml-schema` crate, because inherent
  impls on `ooxml-core`'s `Lens` don't compile downstream (B2); the
  single `insert` becomes slot-based insertion with caller ordering
  inside a slot, plus `get_or_insert`/`remove`/attribute writes and
  explicit dirty-flag duties (B3); `mc:AlternateContent` siblings make
  insertion fail safe (M1); the generation unit is the SDK's
  (type, element) pair with base-class inheritance resolved (M2); the
  `ooxmlsdk` assessment is corrected (M3); the Phase0 allowlist gains
  the parts needed to find sheet1 and read rich/shared strings (M4); an
  overrides format is defined (M5); plus m1–m5.
- **Revision 3** (after `0002-opus-review-round2.md`): slot lowering
  now keeps per-QName and per-slot limits, refuses exclusive choices
  whose alternatives repeat, and cross-checks the SDK's order against
  the ECMA XSDs (R2-M1); the default join is namespace-correct, follows
  global-attribute `ref`s, and drops defaults that don't fit the SDK
  type instead of failing the build (R2-M2); prefix rules for new nodes
  (R2-m1); node id = child-index path, and `own_dirty` only when an
  originally self-closing element gains its first child (R2-m2);
  `insert_by` sees the index within the slot (R2-m3); `mergeCells` and
  the read-only parse scope are added to Phase0 (R2-m4); plus
  R2-m5–m8. The spike was updated to the revision-3 rules.
- **Revision 4** (after `0002-opus-review-round3.md`, Minor only):
  repetition lifts limits by multiplication and the choice rules cover
  alternatives that lower to one slot (R3-m1); top-level repeating
  multi-slot sequences are unsupported, recounted (R3-m2); `set_text`
  primitive with its dirty and `xml:space` duties (R3-m3);
  `MissingReference` applies when any sibling lacks `r`, for reads too
  (R3-m4); prefix-rule wording and an ADR-0001 note on `RawNode` names
  (R3-m5); `set_type` override (R3-m6).
- **Revision 5** (after checking Word/PowerPoint applicability,
  `docs/research/2026-11-16-word-powerpoint-applicability.md`): the
  exclusive-choice rule now considers enclosing repetition, which had
  wrongly made `w:p`/`w:tbl`/`w:tr`/`w:tc` unsupported; a repeated
  sequence of optional parts is one free slot; a QName in two slots
  makes only that QName's insertion need an explicit slot instead of
  the whole model being unsupported. Unsupported models went from 66 to
  12. Slot lowering was checked against 197 real .docx/.pptx files (0
  order violations). The Ecma licence question (former open question 3)
  is settled, and overrides on abstract base entries are inherited.
  Round-4 Minor fixes: the free-slot rules apply only to unbounded
  repetition (R4-m1); existing siblings with a QName in two slots are
  assigned greedily (R4-m2); leftover wording (R4-m3); the real-file
  claim is narrowed to what was checked (R4-m4); licence premise,
  `.gitattributes` for unmodified XSDs, full notice in
  `THIRD-PARTY-NOTICES` (R4-m5); override inheritance keyed on the
  complex-type part, 191 entries, precedence (R4-m6).
- **Post-acceptance note (2026-11-16, implementation layout — no change
  to the decisions):** the first implementation (`foundation-v0`) fixes
  three layout details this ADR left open or stated differently.
  (1) The allowlist is one file per feature group,
  `codegen/allowlist/{sml,wml,pml,dml}.toml`, instead of a single
  `codegen/allowlist.toml`, so each format workstream edits only its own
  file. Each file has a `[write]` and a `[read]` section (`names = [...]`):
  write entries get `*View` + `*ViewMut`, read entries `*View` only.
  `ChildOf` impls exist between any two allowlisted entries, so the generic
  `LensMut` primitives still accept read-only children; only the generated
  typed write surface depends on `[write]`. The feature group of an entry is
  the file it is listed in. (2) Generated files are one per feature group
  (`src/generated/<group>.rs` plus a generated `mod.rs` with the
  `cfg_select!` switches), not one per namespace. (3) The §6 parts table is
  generated into `ooxml-core` (`src/generated/parts.rs`), because
  `ooxml-core::Package` consumes it and cannot depend on `ooxml-schema`.
  The generator also writes `codegen/report.txt` (lowering and default-join
  totals, all diagnostics), checked by `--check` like the code.
- **Post-acceptance note (2026-11-16, two additive rules — no change to
  the decisions; from the ADR-0005 round-1 review, M5 and B2):**
  (1) *Wildcard parents.* A parent whose content model has a wildcard slot
  (`a:graphicData` is `xsd:any`) has no named children, so the rule
  "`ChildOf` between allowlisted entries" produced none. The generator now
  also treats the SDK `Children` list of such a parent as children when the
  wildcard admits their namespace; `ChildOf` is then generated for the
  allowlisted ones (e.g. `impl ChildOf<GraphicData> for Table`). Named
  content models are unchanged. Note: `Lens::cast` only re-types a node
  with the *same* QName and does not replace this. (2) *XML characters.*
  `LensMut::set_text` and `LensMut::set_attr` refuse a value containing a
  character outside XML 1.0 `Char` (`Error::InvalidXmlChar`), so no
  primitive can write a part that is not well-formed (U+000B, other C0
  controls, U+FFFE/U+FFFF). Format crates still own their own line-break
  rules; this is the backstop.

Every claim marked *(spike)* was checked by running code in
`spikes/adr-0002/` on rustc 1.99.1; see its README.

## Context

ADR-0001's Layer 2 puts a *typed lens* over a lossless `RawNode` tree.
For any element, the lens needs schema knowledge the raw tree does not
carry:

- which complex type an element has *in this parent* — the same local
  name maps to different types in different contexts (`x:dimension` is
  `CT_SheetDimension` under `<worksheet>` but `CT_PivotDimension`
  elsewhere; `x:row`/`x:sheetData` have external-link variants);
- its attributes: names, simple types, **defaults** (a `<c>` with no
  `t` is numeric; with no `s` it uses style 0 — Phase0 reads depend on
  this);
- its content model (XSD *particle*): allowed children, cardinality,
  and **schema order**. A child inserted out of schema order is the
  classic cause of Excel's repair dialog.

Schema order is not the only order that matters. ADR-0001's T2 oracle
requires insertion points to respect **row/column order** — an order
*within* one schema position (`row*` inside `sheetData`, `c*` inside
`row`) that the schema says nothing about. This ADR keeps the two
apart (§4).

Hand-writing this is a non-starter. In the Open XML SDK schema data,
SpreadsheetML has 445 element declarations in its base namespace plus
474 across Microsoft extension namespaces (45 schema files, 35 of them
with element declarations); WordprocessingML 685 + 166;
PresentationML 248 + 178; shared DrawingML/charts/diagrams add roughly
1,300 more. Across all schema files, 2,122 (type, element) entries
carry a content model.

ADR-0001's Alternative 5 deferred a dedicated evaluation of `ooxmlsdk`,
which generates Rust from the same data. That evaluation was done for
this ADR (source read at v0.15.0); its result is under Alternatives.

## Decision

### 1. Inputs: two vendored, pinned data sets plus an overrides file

1. **Structure** — the .NET Open XML SDK's machine-readable metadata
   (`data/schemas/*.json`, `data/parts/*.json`, `data/namespaces.json`).
   It already resolves XSD groups, extensions and substitutions into one
   particle tree per (type, element), and it covers the Office
   2010–Microsoft 365 extension namespaces that ECMA-376 lacks.
   Examined at `dotnet/Open-XML-SDK@431ab05c` (2026-08-18), MIT
   (.NET Foundation).
2. **Attribute defaults** — the SDK data has **no** default values (all
   8,510 attribute entries carry only `QName`, `Type`,
   `PropertyComments`, `PropertyName`, `Validators`, `Version`). The
   ECMA-376 Transitional XSD set has 1,236 `default=` attributes, 775 in
   `sml.xsd` alone, including `CT_Cell`'s `s="0"`, `t="n"`, `cm="0"`,
   `vm="0"`, `ph="false"`. Defaults are extracted by a plain lookup,
   **not** a general XSD processor:
   - key = (target namespace, `complexType/@name`, attribute QName),
     where the attribute's namespace is decided by
     `attributeFormDefault`/`form` (WordprocessingML is `qualified`,
     so `w:color` is matched in the `w` namespace, not as `color`);
   - `attributeGroup` references and a single `extension/@base` level
     are followed, and so are `ref=` uses of global attribute
     declarations (`o:insetmode`, `r:dm`, …), whose default lives on the
     global declaration;
   - names are compared by namespace URI, never by prefix text.

   *(spike: `join3.py` joins 1,511 SDK attribute entries this way,
   CT_Cell's five included.)*

   **A default that does not fit the SDK's type is not adopted.** 39
   joined defaults fall outside the lexical space of the SDK type for
   the same attribute (`typecheck.py`), e.g. DrawingML percentages
   (`a:xfrm/@sx default="100%"` vs. SDK `Int32Value`) and
   `c:CT_SplitType/@val default="auto"` (missing from the SDK enum).
   None is in SpreadsheetML. The generator drops such a default and
   emits a `DefaultMismatch` diagnostic. It is adopted only via a
   `set_default` override with evidence. The underlying cause, a
   Transitional union type (`ST_Percentage`) that the SDK models as an
   integer, is fixed where it matters by giving `ooxml-core` the union
   simple types, never by failing the build. For reads, the same
   "never lossy" rule as for enums applies to every simple type: a value
   outside the type's lexical space is returned as an `AttrError`
   carrying the raw string. It is never coerced or dropped.

   **Source and licence.** The Transitional XSDs are the
   `OfficeOpenXML-XMLSchema-Transitional.zip` inside Ecma's official
   ECMA-376 Part 4 download (5th edition, December 2016;
   `ECMA-376-4_5th_edition_december_2016.zip`, SHA-256
   `bd25da1109f73762356596918bf5ff8b74a1331642dba5f1c1d1dfc6bed34ecd`;
   inner zip `d34187520749998af306faf1b730e568b0ca6d88ad24638a407c0a9bb4ca04fc`).
   `ooxmlsdk`'s copy, on which the counts above were measured, is
   identical except for CRLF line endings, so the counts stand. This
   edition carries no copyright page, and the XSDs carry no header.
   The policy that governs it is Ecma's text copyright policy (version 3,
   December 2025). Its FAQ says the default notice has been included in
   every new standard since 2009, and that the same principle applies in
   practice where a notice was not inserted. This 2016 edition is such a
   case. That notice permits copying and distribution with the notice attached, and
   permits implementing the specification in conformant products,
   including by copy and paste. It forbids modifying the document itself,
   and the FAQ asks that republications be unchanged and up to date. The
   XSDs are therefore vendored **unmodified**, with Ecma's default notice
   in full and the provenance above. Line endings are kept by a
   `.gitattributes` rule (`reference/ecma-376-transitional-xsd/** -text`),
   and checksums are verified by `--check`. Updating to a newer Ecma
   edition is a reviewed input bump like any other. Corrections live only in `overrides.toml`. The same notice goes
   into `ooxml-schema`'s `THIRD-PARTY-NOTICES` (§2). Ecma's BSD software
   licence is not relied on, since this edition's copyright page does not
   carry it. Extension-namespace
   defaults (x14, x15, …) are not in ECMA-376 and come only from overrides citing the relevant [MS-XLSX]/[MS-DOCX]/
   [MS-PPTX] section.
3. **Overrides** — `codegen/overrides.toml`, for places where the data
   disagrees with what Office actually writes. Allowed operations, each
   entry requiring `reason` and `evidence` (a fixture path or a spec
   section):
   - `add_attribute { on, name, type, default? }`
   - `add_child { parent, name, after | before }`
   - `add_enum_value { enum, value }`
   - `set_default { on, attribute, value }`
   - `mce_wrapped { parent, children }` — these children appear in real
     files inside `mc:AlternateContent`, not directly (§4.3).
   - `set_content { on, slots }` — replace a content model whose SDK
     lowering disagrees with the ECMA order (§4.1).
   - `set_type { on, attribute, type }` — e.g. swap an SDK `Int32Value`
     for the core's Transitional union type (`ST_Percentage`), so that a
     valid `sx="50%"` reads as a value instead of an `AttrError`. Needed
     before DrawingML/PresentationML/charts enter the allowlist; not for
     Phase0.
   `ooxmlsdk`'s own overrides (`sdk_data/schema_extensions/`) already
   need every one of these operations for SpreadsheetML alone (e.g.
   `xr:uid`, `xr:revisionPtr`, `SheetStateValues::show`, MCE-wrapped
   `legacyDrawing`/`controls`), which is why the set is fixed now. An
   override on an abstract base entry (`Name` ending in `/`) is inherited
   by every entry whose `Name` has the same complex-type part, across
   schema files. It is keyed on that type part (`w:CT_OnOff`), not on
   the .NET `BaseClass` name: `OnOffType` is shared by the unrelated `w`,
   `m` and `w14` bases. An override on the derived entry itself takes
   precedence over the inherited one. Example: one `set_default` of
   `val` to `true` on `w:CT_OnOff/` gives WordprocessingML's on/off
   semantics (`<w:b/>` means on, which the XSD leaves to prose) to all
   191 `w:CT_OnOff/…` entries. The `m:` (30) and `w14:` (4) on/off types
   need their own entries. If an override's content is taken from
   `ooxmlsdk`'s overrides, their licence (MIT OR Apache-2.0) is honoured
   in the notices (§2).

Inputs 1 and 2 are vendored under `reference/openxml-sdk-data/` and
`reference/ecma-376-transitional-xsd/`, each with upstream's licence
text and a `PROVENANCE.md` (source URL, commit or edition, SHA-256 of
every file). Updating any input is an explicit, reviewed commit (bump →
regenerate → review the generated diff); nothing is fetched at build
time.

### 2. Generator and crate layout

- A workspace member `xtask` (`publish = false`) provides
  `cargo xtask codegen`. It writes ordinary Rust source into a new
  crate, **`ooxml-schema`**, under `src/generated/<namespace>.rs`, each
  file headed `// @generated by cargo xtask codegen — do not edit`.
  Generated files are committed.
- Crate graph: `ooxml-core` (OPC package, raw tree, lens runtime;
  hand-written) ← `ooxml-schema` (generated tables and views, one
  cargo feature per namespace group) ← `ooxml-xlsx` / later
  `ooxml-docx`, `ooxml-pptx` (hand-written domain API). Shared
  schema namespaces (DrawingML `a:` and friends) live in `ooxml-schema`
  once, behind their own features, so no format crate owns them. The
  namespace URIs the core runtime itself acts on — `mc` (AC detection,
  §4.3), `r` (relationship-id attributes such as `r:id`, used by OPC
  traversal), `xml` — are hand-written constants in `ooxml-core`, not
  feature-gated.
- `cargo xtask codegen --check` regenerates in memory and fails on any
  diff, covering all three inputs; CI runs it. Output is deterministic:
  inputs are read in sorted order, all maps are `BTreeMap`, and
  formatting is done inside the generator with a pinned `prettyplease`
  (no dependency on the host's `rustfmt`).
- `ooxml-schema`'s package ships a `THIRD-PARTY-NOTICES` file (in the
  crate's `include`) with the .NET Foundation MIT notice, Ecma's
  default copyright notice in full with the provenance of §1.2, and — if any override was taken from `ooxmlsdk` —
  its MIT/Apache-2.0 notice. Generated code embeds names and summaries
  from these sources. A `LICENSE` file under `reference/` alone would not
  reach crates.io.
- **Not used:** `build.rs` generation (cost on every downstream build,
  generated code invisible to review) and procedural macros for the
  schema surface (compile cost at thousands of types, poor
  rust-analyzer visibility). `macro_rules!` may be emitted inside
  generated files for repetitive bodies.

### 3. Shape of generated code

All behaviour is hand-written once in `ooxml-core`; generated code is
constant data, trait impls, and one-line view methods.

**Generation unit.** One marker per SDK `Name`, i.e. per
(complex type, element) pair such as `x:CT_Cell/x:c` — not per complex
type. A complex type can back many elements (`CT_Xstring` backs 15:
`t`, `v`, `author`, `formula`, …; `CT_Cell` backs `c` and `oc`), and
`ElementSpec` has exactly one `QNAME`. The allowlist (§5) is written in
the same unit, so "allowlist diff = generated diff" holds.

**Inheritance.** Attributes and `AdditionalElements` often live on an
abstract base entry: `x:CT_Cell/x:c` has no attributes of its own
(`BaseClass: CellType`); `r`/`s`/`t`/`cm`/`vm`/`ph` are on the abstract
`x:CT_Cell/` entry, and `xml:space` for `t`/`v` is on `x:CT_Xstring/`.
The generator resolves `BaseClass` (a .NET class name) **within the same
schema file** to the abstract entry (`Name` ending in `/`) and merges
its attributes and elements. Class names repeat across files (e.g.
`Text` exists in dml, math, chartex and sml), so cross-file lookup is
never attempted. Ten WordprocessingML entries name a base that is not in
the data at all (`CustomXmlElement` ×5, `SdtElement` ×5, hand-written
classes in the .NET SDK). Such an entry is still generated, but marked
`attrs_incomplete`: its typed attribute accessors are omitted and
writes to it return `Err(IncompleteSpec)` until an `add_attribute`
override supplies the base's attributes. The generator does not stop.

**QName identity.** A `QName` is (namespace URI, local name); prefixes
are per-document and never compared. The raw tree records each element
and attribute name's resolved namespace URI at parse time, next to its
original prefix (which is still needed when a dirty node is
re-rendered). Choosing a prefix for something newly written:

- `xml` (`http://www.w3.org/XML/1998/namespace`) is always bound and is
  never declared (Namespaces in XML 1.0 §3), so
  `xml:space="preserve"` on a new `<t>` always works.
- A namespaced **attribute** needs a non-empty prefix bound to its URI.
  The default namespace never applies to attributes. An attribute with
  no namespace (SML's `r`, `s`, `t`) is written unprefixed.
- A new **element** uses the parent's own prefix when that prefix binds
  the element's URI. Otherwise it uses the innermost in-scope binding
  for the URI. If one element declares both the default namespace and a
  prefix for the URI, the default namespace (empty prefix) is used.
  Only bindings that are **in effect** at the insertion point count: a
  prefix that an inner element re-declares to another URI is excluded.
- If no usable binding is in effect, the write returns
  `Err(NamespaceNotInScope)`. For an attribute this includes the case
  where the URI is bound only as the default namespace. Phase0 never
  declares namespaces.

Hand-written (`ooxml-core`), shown in the shape the spike compiled
*(spike: `ws/core`)*:

```rust
pub struct QName { pub ns: &'static str, pub local: &'static str }

pub trait ElementSpec: 'static {
    const QNAME: QName;
    const ATTRS: &'static [AttrSpec];   // name, simple type, default (§1.2)
    const CONTENT: Content;              // §4
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an allowed child of `{P}`",
    note = "allowed children are listed in `{P}::CONTENT`")]
pub trait ChildOf<P: ElementSpec>: ElementSpec {}

pub struct Slot { names: &'static [QName], max_each: Option<u32>, max_total: Option<u32> }
pub enum Content { Empty, Text(SimpleType), Slots(&'static [Slot]),
                   Unsupported(&'static [QName]) /* every allowed child name */ }

pub struct Lens<'a, E: ElementSpec> { /* &'a part root, node id */ }
pub struct LensMut<'a, E: ElementSpec> { /* &'a mut part root, node id */ }
// generic reads/writes/insertion on Lens/LensMut: §4
```

Generated (`ooxml-schema`, per allowlisted entry):

```rust
/// `x:CT_Cell/x:c` — Cell.
pub enum Cell {}                                   // uninhabited marker
impl ElementSpec for Cell {
    const QNAME: QName = qn!(SML, "c");
    const ATTRS: &'static [AttrSpec] = &[          // merged from CellType
        attr!("r", Str), attr!("s", U32, default = "0"),
        attr!("t", Enum(&CELL_VALUES), default = "n"), /* cm, vm, ph */
    ];
    const CONTENT: Content = Content::Slots(&[
        one!(SML:"f"), one!(SML:"v"), one!(SML:"is"), one!(SML:"extLst"),
    ]);
}
impl ChildOf<Row> for Cell {}   // under cfg(all(..)) if Row and Cell are in different features
const _: () = assert!(content_contains(Row::CONTENT, Cell::QNAME));

/// Typed view. A local newtype, so inherent methods are legal here.
pub struct CellView<'a>(pub Lens<'a, Cell>);
impl<'a> CellView<'a> {
    pub fn style_index(&self) -> Result<u32, AttrError> { /* absent → default 0 */ }
}
pub struct CellViewMut<'a>(pub LensMut<'a, Cell>);
impl<'a> CellViewMut<'a> {
    // child views are reborrowed from the parent view, one at a time
    pub fn value_mut(&mut self) -> Result<CellValueViewMut<'_>, Error> { /* get_or_insert */ }
}
```

Writing the view methods as `impl Lens<'_, Cell> { … }` in a crate
other than `ooxml-core` is rejected by rustc (E0116) *(spike)*; local
`*View`/`*ViewMut` newtypes are the chosen fix. Domain types in
`ooxml-xlsx` (ADR-0001's `CellRef`/`CellMut`) wrap these views; the
`View` suffix keeps the names from colliding.

**Enumerated values are never lossy.** A typed read of an enum
attribute yields `EnumValue::Known(variant)` or `EnumValue::Unknown(raw)`
(see Resolved questions);
the enum is not made fallible in a way that tempts callers into
rewriting the attribute. Writing an arbitrary string is always possible
through the untyped `set_attr`, so a value the pinned data lacks (e.g.
`show` before an override adds it) can still be written. Untouched
attributes are never re-rendered (ADR-0001 Layer 2).

**Leaf text.** `Content::Text(ty)` uses the simple type from the SDK
`Validators` where present (187 of 344 leaf text entries); the rest are
`Str`.

**Compile-time checks.** Each generated file ends with module-level
`const _: () = …;` items, which are always evaluated (unlike an inline
`const {}` in a function body, which is only guaranteed to run when
reached). They check: every `impl ChildOf<P> for C` has `C::QNAME` in
`P::CONTENT` (searching `Slots` and, for unlowerable models, the
`Unsupported` name list); no QName appears twice within one slot (a
QName in two different slots is allowed, §4.1); every default is
valid for its type. Stable const evaluation can do this with
hand-written byte loops (`&str == &str` is not const; `const_trait_impl`
is unstable as of 1.99) and `u32::from_str_radix` (const since 1.82)
*(spike: a bad enum default fails the build with E0080)*. For `double`
defaults (10 in `sml.xsd`) only the lexical form is checked, since
`f64::from_str` is not const. The generator runs the same checks first,
with readable messages.

What fails at **compile time** is only *which element type may be a
child of which* (`ChildOf` + `on_unimplemented`; *(spike)*: inserting a
`CellValue` under a `Row` gives "`CellValue` is not an allowed child of
`Row`", and rustc also points at the existing
`ChildOf<Cell> for CellValue`). Order, cardinality and unsupported
content models are **run-time** errors (§4).

**Features and cross-group children.** `impl ChildOf<P> for C` is
generated only when both entries are allowlisted, and is wrapped in
`#[cfg(all(feature = "<P's group>", feature = "<C's group>"))]` when the
two belong to different feature groups. 20 SpreadsheetML-namespace
entries have DrawingML children, e.g. `xdr14:cNvPr → a:hlinkClick`.
Phase0's allowlist is entirely within `sml` plus the core constants of
§2, so `ooxml-xlsx` needs only the `sml` feature.

### 4. Content models and the write primitives

#### 4.1 Slots

The SDK particle kinds across all schemas are 11,278 `Element`, 2,234
`Sequence`, 1,227 `Choice`, 1,150 `Group`, 142 `Any`, 6 `All`. They are
lowered to an **ordered list of slots**. A slot is a set of element
QNames sharing one schema rank, with two limits: `max_each` (per QName)
and `max_total` (the whole slot). Insertion checks both.

- Each element of a sequence is its own slot, with its own `maxOccurs`.
- Below, an *alternative* is any choice branch that itself lowers to
  exactly one slot (a single element, an `Any`, or a nested
  sequence/group/choice that lowers to one slot).
- A **repeating** `Choice` (`maxOccurs = r > 1`) is one slot containing
  all its alternatives' names. Its children may appear in any order
  relative to each other.
- A **non-repeating** `Choice` whose alternatives all allow at most one
  occurrence is one slot with `max_total = 1`.
- A **non-repeating** `Choice` where two or more alternatives exist and
  any of them may repeat means "a run of one kind only". This is not
  representable as a slot, so the model is **unsupported**
  (`p188:CT_Comment`, `x:CT_SortState`, …). *Non-repeating* means
  **effectively** non-repeating: a choice nested, up to the slot
  boundary, inside a particle whose `maxOccurs` is **unbounded** is a
  repeating choice. (WordprocessingML nests `Group* > Choice > Group* >
  Choice …` throughout `w:p`/`w:body`/`w:tc`.) A finite enclosing bound
  (e.g. `{0..2}`) does not count, because `(a* | b*){0..2}` is not
  `(a | b)*`. Such models stay unsupported. None occurs in the current
  data.
- A sequence or group repeated **without bound** whose parts are **all
  optional** is one free slot holding all its names: `(a? b? …)*` and
  `(a | b | …)*` accept the same child lists. With a finite bound the
  two differ, so the model is unsupported. Examples are SDK-inserted `w14`
  extension pairs inside paragraph content and the top-level
  `x:CT_RPrElt/x:rPr`, whose ECMA definition is an unbounded choice
  anyway.
- A nested `Sequence`/`Group` with `maxOccurs ≤ 1` that is not inside
  an unbounded repetition is spliced in place. Inside one, the
  free-slot rule above applies instead.
- **Repetition multiplies.** When a particle with upper bound `r` lowers
  to a single slot (a repeating choice, or a repeating group/sequence
  around one slot), each `max_each` and `max_total` inside it is
  multiplied by `r`; unbounded × anything = unbounded. For example,
  `a:duotone` (`Group{2..2}[color choice]`) allows exactly 2 colours,
  and `w:sectPr` allows up to 6 header/footer references. A repeating
  particle that lowers to **more than one** slot is unsupported, at any
  depth including the top level, unless the free-slot rule above
  applies.
- `All` becomes one slot with `max_each = 1` and `max_total` unbounded.
- `Any` becomes a wildcard slot. The SDK data gives a `Namespace` for
  only 6 of 142 `Any` particles; a missing one is read as `##any`. Even
  so, `mc:AlternateContent` is governed by §4.3, never by a wildcard.

A content model is also **unsupported** if a multi-slot sequence/group
with a required part repeats, or if a `Choice` alternative is itself a
multi-slot sequence. Then `Content::Unsupported(names)` is generated:
typed reads work, and insertion returns `Err(UnsupportedContentModel)`.

A QName that lands in **two slots** does not make the model
unsupported. In `w:tbl`, for example, 20 range-marker elements are
allowed both before `tblPr` and among the rows. Only inserting that
QName is ambiguous: plain `insert`/`insert_by` for it returns
`Err(AmbiguousSlot)`, and `insert_in_slot(slot_index, before)` picks the
slot explicitly. All other children, `w:tr` included, insert normally.
The compile-time duplicate check (§3) applies within a slot only.

To position anything, each **existing** child with such a QName is
first assigned a slot **greedily, left to right**: the lowest of its
candidate slots that is not before the slot of the preceding listed
sibling. A leading `bookmarkStart` in `w:tbl` is therefore in slot 0,
before `tblPr`. Inserting `tblGrid` then lands after `tblPr`, not in
front of the marker. A child list that admits no such assignment
violates the schema, and insertion into that parent returns
`Err(UnresolvedSibling)`.

**Cross-check against ECMA order.** The SDK data is not always the
ECMA particle. For example, `x:CT_Filters` is `Choice[x14:filter*,
x:filter*, x:dateGroupItem*]` in the SDK, but `sequence(filter*,
dateGroupItem*)` in `sml.xsd`. For every entry whose complex type also
exists in the ECMA XSDs, the generator lowers the XSD particle the same
way. If the SDK slots allow an order the XSD forbids (two names share an
SDK slot but have distinct XSD ranks, or their order is inverted), the
entry is generated as unsupported and a diagnostic is emitted. A
`set_content` override can restore it. *(spike: 8 such entries, none in
Phase0: `xne:macrosheet`, `x:fieldGroup`, four WordprocessingML
border/margin types with `left`/`start`, `w:sdtPr`, `w:fieldMapData`.
`CT_Filters` is already unsupported by the exclusive-choice rule.)*

Measured on the data *(spike: `slottable.py`; a heuristic reading of
`Occurs`, where a present `Occurs` without `Max` is taken as
unbounded)*: of 2,122 content models, 1,573 lower to single-QName slots
only, 537 to slots that include multi-QName (choice) slots (1 of them,
`w:tbl`, with an ambiguous QName), and **12 are unsupported** (4
effectively exclusive choices of repeating alternatives, 8 multi-slot
alternatives), before the 8 ECMA-order disagreements above. By format
group, WordprocessingML and PresentationML base namespaces have 0
unsupported.

**Checked against real files** *(spike: `corpus_check.py`)*: 197
.docx/.pptx files from Apache POI's test data were scanned. In 171,537
instances of 18 key parents (`w:body`, `w:p`, `w:r`, `w:tbl`, `w:tr`,
`w:tc`, `w:pPr`, `w:rPr`, `w:sectPr`, `p:sld`, `p:cSld`, `p:spTree`,
`p:sp`, `p:pic`, `p:txBody`, `a:p`, `a:r`, `p:presentation`), the
*listed* children always appear in slot order under the greedy
assignment above: 0 violations. What this does **not** check: unlisted
children and `mc:AlternateContent` are skipped, and counts against
`max_each`/`max_total` are not tested. `w:rPr` is checked as `CT_RPr`
everywhere; a review re-check against the context-correct type
(`CT_ParaRPr` under `w:pPr`) also found 0. Where particle `Occurs` differ by Office
version (3 entries, none in Phase0), the most permissive one is used.

All Phase0 entries are flat sequences or leaves (verified against the
data): `worksheet` (38 elements), `sheetData` (`row*`), `row`
(`c*`, `extLst?`), `c` (`f?`, `v?`, `is?`, `extLst?`), `is` (`t?`,
`r*`, `rPh*`, `phoneticPr?`); `f`, `v`, `t`, `dimension` are leaves.

#### 4.2 Primitives (all in `ooxml-core`, generic over `ElementSpec`)

| Primitive | Behaviour |
| --- | --- |
| `insert_by::<C>(before)` | Find `C`'s slot. `before(index_in_slot, existing)` is called on the existing children of that slot in document order; the new node goes before the first one for which it returns `true`, else before the first child of a higher-ranked slot, else after the last child. If `max_each` or `max_total` is reached → `Err(AlreadyPresent)`. |
| `insert::<C>()` | `insert_by` that never returns `true`: append at the end of the slot. |
| `insert_in_slot::<C>(slot, before)` | As `insert_by`, with the slot given explicitly; required only for a QName that appears in two slots (§4.1). |
| `get_or_insert::<C>()` | Existing child of `C`'s QName, or `insert`. The normal path for `maxOccurs = 1` children. |
| `remove::<C>()` / `remove_at` | Remove the child node and its whole span. |
| `set_attr` / `remove_attr` | Typed and untyped (by QName) variants. |
| `set_text(value)` | Only on `Content::Text` elements. Replaces the element's text content. If `value` has leading or trailing whitespace and the element's `ATTRS` declare `xml:space`, it also sets `xml:space="preserve"` (ADR-0001 R2-M5). It never removes an existing `xml:space`. |

Callers own the within-slot order. `ooxml-xlsx` compares row numbers
when inserting into `sheetData` and column numbers when inserting into
`row`, which is what ADR-0001's T2 checks *(spike: B1, D1 + insert A1,
C1 → A1, B1, C1, D1, extLst)*. Because `before` also receives the index
within the slot, a sheet whose `row`/`c` elements omit `r` (legal,
position-implied) can be handled later. Phase0 does not handle it: if
**any** `row` in `sheetData`, or any `c` in the row concerned, lacks
`r`, then both writes **and reads** there return
`Err(MissingReference)`. A read that silently returned `None` for a
cell with a value would break ADR-0001's fail-honestly stance. Rows and
cells created by ooxml-rs always carry `r`, and the from-scratch
template's `<sheetData/>` has no children, so neither path is affected.

**Node id.** A node is identified by its path of child indices from the
part root, which keeps ADR-0001's nested `RawNode` (no parent links).
`LensMut` holds the part root by `&mut` plus that path. A child view is
a reborrow of its parent view, so only one child view exists at a time
*(spike)*. Inserting or removing a child invalidates the indices of its
later siblings. That is why primitives return the new child's view
instead of handing out paths.

**Dirty-flag duties are the primitives'**, never generated code's:

- every insert, remove, attribute write and `set_text` sets
  `subtree_dirty` on the changed node and on every ancestor along the
  path, up to the part root (ADR-0001 Layer 2);
- an attribute write sets `own_dirty` on that node;
- a parent gets `own_dirty` from a child change **only** when its
  original span was self-closing (`span.close == None`) and it gains its
  first child, element or text (ADR-0001's R2-B2 case; e.g. `<t/>`
  receiving text through `set_text`);
- a parent that had a close tag keeps its original open and close tags
  verbatim even when it loses its last child (`<row …></row>` is
  well-formed), so the byte diff stays local;
- an originally self-closing element that gains and then loses children
  is already `own_dirty` and is re-rendered self-closing.

Primitives create nodes immediately. ADR-0001's "no node until a value
is written" is the domain layer's job: `CellMut` calls `get_or_insert`
only from inside `set_string`/`set_number`.

**Whitespace.** A new element goes immediately before the child it was
positioned before (so after any whitespace text that precedes that
child), or immediately before the parent's close tag if appended. No
whitespace is ever added, so the byte diff stays at the insertion point.

#### 4.3 Unlisted siblings and Markup Compatibility

A child that is in no slot (and matches no wildcard) is *unlisted*: a
vendor extension, a comment, a processing instruction. It is treated as
attached to the listed sibling before it. It never decides a position
and is never moved.

`mc:AlternateContent` cannot be treated that way. Its branches carry
listed elements (real worksheets wrap `legacyDrawing` and `controls`
like this), so it *has* a schema rank, and ignoring it gives wrong
output. Counterexample: children `[pageMargins, drawing, legacyDrawing,
AC{controls}, extLst]`; inserting `picture` (rank 32) would land after
the AC and give `controls(34), picture(32)` once MC is processed.
Therefore:

- **Phase0 rule:** an insertion into a parent that has any
  `mc:AlternateContent` child returns `Err(UnresolvedSibling)`
  *(spike)*. No Phase0 write targets such a parent in the fixture or
  template; in a real file, a `dimension` insert into a `worksheet` with
  an AC child is the one case that would now error rather than risk a
  corrupt file.
- **Later rule** (recorded so it isn't reinvented): an AC's rank is the
  minimum slot rank of the first listed element across its
  `mc:Choice`/`mc:Fallback` branches, with a diagnostic if branches
  disagree.
- The SDK data lists some MCE-only children as direct children (e.g.
  `x15ac:absPath` at position 3 of `CT_Workbook`). These get a
  `mce_wrapped` override (§1.3) so the generated `CONTENT` does not
  offer a direct insert Excel would never write.

### 5. Scope control

- An explicit allowlist of SDK `Name`s per format, in
  `codegen/allowlist.toml`. Growing the typed surface means growing the
  allowlist and reviewing the generated diff.
- **Phase0 allowlist:**
  - write: `x:CT_SheetData/x:sheetData`, `x:CT_Row/x:row`,
    `x:CT_Cell/x:c`, `x:CT_CellFormula/x:f` (read only; formula writes
    are `Err` per ADR-0001), `x:CT_Xstring/x:v`, `x:CT_Rst/x:is`,
    `x:CT_Xstring/x:t`, `x:CT_SheetDimension/x:dimension`,
    `x:CT_Worksheet/x:worksheet`;
  - read only: `x:CT_Workbook/x:workbook`, `x:CT_Sheets/x:sheets`,
    `x:CT_Sheet/x:sheet` (to locate sheet1), `x:CT_RElt/x:r` (inline
    strings made only of rich runs), `x:CT_Sst/x:sst`, `x:CT_Rst/x:si`
    (shared strings, so `cell()` can return text),
    `x:CT_MergeCells/x:mergeCells`, `x:CT_MergeCell/x:mergeCell`
    (ADR-0001 refuses writes into merged ranges).
- **Parse scope.** These read-only entries mean Phase0 parses
  `workbook.xml` and `sharedStrings.xml` into Layer 2 trees, not only
  `sheet1.xml`. This widens ADR-0001's "`RawNode` tree for `sheet1.xml`
  only" for **reads**; edits are still confined to the worksheet part.
  The extra parts stay dirty-free `Parsed` and are copied through raw,
  per ADR-0001's "parsing for inspection must not change the output"
  rule (its M2). ADR-0001 gets a post-acceptance note saying so.
- Each namespace group is a cargo feature of `ooxml-schema` (`sml`,
  `wml`, `pml`, `dml`, …), switched in generated code with
  `cfg_select!` (Rust 1.95). `ooxml-xlsx` enables `sml` (§3, features).
- The SDK's `InitialVersion` is carried as data and has no behavioural
  effect in Phase0.

### 6. Package parts

`data/parts/*.json` (128 part kinds: content type, relationship type,
allowed child parts) is generated into a constant table used by
`ooxml-core::Package`'s Layer 3 registries. Phase0 uses it read-only to
locate "sheet1", meaning the **first sheet in workbook order**:

1. the package's `officeDocument` relationship gives the workbook part;
2. the first `<sheet>` in its `<sheets>` gives an `r:id`;
3. that id is looked up in the workbook part's relationships part, whose
   name is derived from the workbook part's name
   (`<dir>/_rels/<name>.rels`, not a fixed path);
4. that relationship gives the worksheet part.

In ADR-0001's fixture and template this part is `xl/worksheets/sheet1.xml`.
In a workbook whose sheets were reordered, the part name can differ, and
workbook order is what users mean by "the first sheet". The relationship
type alone does not identify it in a multi-sheet workbook. (`WorkbookPart`
has no fixed content type in the data, since `.xlsx`/`.xlsm`/`.xltx`
differ; it is identified by relationship type.)

## Alternatives considered

1. **Depend on `ooxmlsdk` (v0.15.0) as the typed layer.** It generates
   Rust from the same SDK data, models every part, reuses original
   compressed bytes for unloaded parts (matching ADR-0001 Layer 1), and
   reports 5,477 of 5,477 corpus files passing across five corpora:
   5,366 round-tripped, the rest open-only or expected-invalid. Rejected as the
   Layer 2 foundation because a *loaded* part is fully re-serialized
   from typed structs, in schema order with canonical lexemes. Its
   round-trip suite opens parts eagerly and compares after XML
   canonicalization with 47 normalization functions (child reordering,
   duplicate removal, numeric lexemes;
   `ooxmlsdk-test-suite/crates/ooxmlsdk-corpus-test-support/src/roundtrip.rs`).
   That is strong semantic equivalence, but not ADR-0001's byte-exact
   untouched siblings. Unknown content is kept as raw bytes where the
   schema has an `Any` slot (e.g. inside `<ext>`, so a foreign
   `<extLst>` payload survives), but an unknown child of a parent with
   no `Any` slot (directly under `worksheet` or `row`) is skipped on read
   (`ooxmlsdk-derive/src/sdk_type.rs`, `build_unmatched_child_tokens`,
   used only when the type has no any-dispatch). It remains useful as a
   reference, as a test oracle (does ooxml-rs output parse there?), and
   for its overrides data.
2. **`ooxmlsdk`-style typed structs + derive, self-built.** Same
   storage model as (1), i.e. ADR-0001's alternative 1b. Rejected for
   Phase0 for the same reason; generating *tables over a raw tree* keeps
   storage lossless and makes typing a view.
3. **Read the XSDs for everything.** The XSDs remain authoritative for
   the standard namespaces, but structure from XSD needs a real
   processor (groups, extensions, substitution groups, redefines), and
   the extension namespaces real files are full of are not in ECMA-376.
   Adopted narrowly instead: XSDs supply only defaults, by plain lookup
   (§1.2). (`ooxmlsdk-build` regenerates SDK-style JSON from XSDs and
   diffs it against the SDK data, `sdk_data/xsd_schema_gen.rs` and
   `oracle.rs`; if the SDK data proves unreliable beyond what overrides
   can fix, that is the path to study.)
4. **`build.rs` generation.** Rejected (§2).
5. **Procedural macros over hand-written declarations.** Moves cost to
   every compile and hides the result from rust-analyzer. Reconsider for
   small hand-written API surfaces once `macro_rules!` derive macros
   (RFC 3698, `macro_derive`) are stable; they are not as of Rust 1.99.
6. **Extension traits instead of view newtypes** (`trait CellLensExt`
   implemented for `Lens<'_, Cell>`, re-exported via a prelude). Also
   legal. Not chosen: every call site needs the trait in scope, and
   rustdoc lists the methods under traits instead of on a type.
   **Generating into `ooxml-core` itself** (so inherent impls on `Lens`
   are legal) was rejected to keep the hand-written core small and its
   compile time independent of how much schema is enabled.
7. **serde-based XML mapping or XSD→serde generators.** Rejected: the
   serde data model loses sibling order and unknown content by
   construction.

## Consequences

- Positive: Phase0's lens for `sheetData`/`row`/`c` is the first
  consumer of the generated shape; broadening coverage is "extend the
  allowlist, review the diff".
- Positive: child-type mistakes are compile errors with readable
  messages; ordering and cardinality are enforced in one place, the
  core primitives, and ADR-0001's row/column ordering has an explicit
  hook (`insert_by`).
- Positive: slot lowering supports insertion for 2,110 of 2,122 content
  models (minus the 8 ECMA-order disagreements until overridden, 6 of
  which are WordprocessingML border/margin and SDT/mail-merge types). No
  WordprocessingML or PresentationML base-namespace model is
  unsupported by lowering, and the listed-child order agrees with
  171,537 real-file instances.
- Negative: two upstream data sets plus overrides to keep pinned,
  attributed and in sync. A future SDK bump can change both structure
  and the default join, so the generated diff must be read, not skimmed.
- Negative: 12 content models (exclusive choices of repeating
  alternatives, multi-slot choice alternatives) plus the 8 ECMA-order
  disagreements stay read-only until overrides or a later ADR cover
  them. Any parent with
  an `mc:AlternateContent` child is insert-refused until the AC-rank
  rule is implemented.
- Negative: ooxml-rs inherits the SDK's reading of the schemas,
  mistakes included; overrides are the escape hatch, each with a
  recorded reason and evidence.
- Negative: the generated output's size is unmeasured until the
  generator exists (expected to be mostly constant tables); it will be
  recorded then. (`ooxmlsdk`'s generated `schemas/` alone is about 112k
  lines, for comparison of order of magnitude only.)

## Resolved questions (decided by the project owner, 2026-11-16)

1. **Vendoring scope: everything.** All SDK `data/schemas`, `data/parts`,
   `namespaces.json` and all Transitional XSDs are vendored, about 12 MB.
   Growing the allowlist then never needs a data refresh, and the
   generator's checks (default join, ECMA-order cross-check) run over the
   whole schema, not just the allowlisted part.
2. **Unknown enum values: a wrapper type.** Typed enum reads return
   `EnumValue<'a, E> { Known(E), Unknown(&'a str) }`. Each generated `E`
   lists only known values, so a `match` on `E` stays exhaustive, and the
   type forces callers to decide what to do with unknown values.
3. **Ecma redistribution terms:** settled in revision 5 (§1.2).

## Sources

- Open XML SDK schema data: https://github.com/dotnet/Open-XML-SDK/tree/main/data
  (examined at commit `431ab05cf160248cc3885a4a766026d4f8243792`, 2026-08-18; MIT)
- ECMA-376 Transitional XSDs as vendored by `ooxmlsdk`
  (`schemas/OfficeOpenXML-XMLSchema-Transitional/`); ECMA-376 editions:
  https://ecma-international.org/publications-and-standards/standards/ecma-376/;
  Apache POI's note on bundling them (Ecma by-laws §9.4)
- ooxmlsdk v0.15.0: https://github.com/KaiserY/ooxmlsdk — `crates/ooxmlsdk-derive/src/sdk_type.rs`,
  `crates/ooxmlsdk/src/schemas/`, `sdk_data/schema_extensions/`, `crates/ooxmlsdk-build/src/sdk_data/`
- ooxmlsdk round-trip harness: https://github.com/KaiserY/ooxmlsdk-test-suite —
  `crates/ooxmlsdk-corpus-test-support/src/roundtrip.rs`
- Rust: release notes 1.89–1.99 (https://github.com/rust-lang/rust/blob/stable/RELEASES.md);
  RFC 3698 (https://rust-lang.github.io/rfcs/3698-declarative-derive-macros.html);
  E0116 (https://doc.rust-lang.org/error_codes/E0116.html)
- Spike: `spikes/adr-0002/` (rustc 1.99.1, 2026-11-16)
- Word/PowerPoint applicability: `docs/research/2026-11-16-word-powerpoint-applicability.md`;
  Apache POI test data https://github.com/apache/poi/tree/trunk/test-data
- Ecma text copyright policy version 3:
  https://ecma-international.org/wp-content/uploads/Ecma_text_copyright_policy_version_3.pdf
- Reviews: `docs/adr/0002-opus-review-round1.md`, `docs/adr/0002-opus-review-round2.md`,
  `docs/adr/0002-opus-review-round3.md`, `docs/adr/0002-opus-review-round4.md`
