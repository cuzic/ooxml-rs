# ooxml-core

Format-agnostic foundation of ooxml-rs: the OPC package (Layer 1), the
lossless raw XML tree and typed lenses (Layer 2), and the OPC registries
(Layer 3) of [ADR-0001](../../docs/adr/0001-lossless-round-trip-architecture.md),
plus the runtime half of [ADR-0002](../../docs/adr/0002-schema-driven-code-generation.md).
Schema knowledge (`ElementSpec` markers, `*View`/`*ViewMut`) is generated
into [`ooxml-schema`](../ooxml-schema); format crates (`ooxml-xlsx`,
`ooxml-docx`, `ooxml-pptx`) build their domain API on both.

This page is the API tour for format-crate authors. Every item has rustdoc.

## Layer 1 — `Package`

```rust
let mut pkg = Package::open("book.xlsx")?;          // whole file read into memory
let wb = pkg.main_document_part().unwrap();          // "xl/workbook.xml" (officeDocument rel)
let rels = pkg.relationships(&wb)?;                  // xl/_rels/workbook.xml.rels
let sheet = rels.resolve(rels.by_id("rId1").unwrap()).unwrap();
let doc: &mut XmlDocument = pkg.xml_part_mut(&sheet)?; // Raw -> Parsed (once)
/* … edit through a LensMut … */
pkg.save("book.xlsx")?;                              // temp file + rename; same path is safe
```

| Item | Notes |
| --- | --- |
| `Package::open(path)` / `from_bytes(bytes)` | `Error::EncryptedPackage` for CFB/OLE files; `Error::StrictNotSupported` when a package relationship uses `http://purl.oclc.org/ooxml/…`; `Error::Package("duplicate part name …")` when two zip entries name the same OPC part (equal after ASCII case folding and percent-decoding; checked in linear time); `Error::TruncatedEntry` when an entry's stored data does not lie inside the archive (it could not be copied through unchanged); `Error::PackageTooLarge` when the declared decompressed sizes add up to more than `ParseOptions::max_package_bytes` (default 4 GiB). |
| `part_names()`, `contains(name)`, `state(name) -> PartStateKind` | Part names are zip entry names, no leading `/`. Original order is kept. |
| `read_part(name) -> Vec<u8>` | Current decompressed content (never changes the part's state). At most `ParseOptions::max_part_bytes` (default 1 GiB, `Error::PartTooLarge`); the declared size decides the refusal, never the allocation. |
| `xml_part(name)` / `xml_part_mut(name)` | Parse once into an `XmlDocument`. A part parsed but not edited is still raw-copied on save. |
| `parsed_part(name)` | `&self` access to a tree already parsed (`None` while `Raw` or `Replaced`); never parses. |
| `set_part(name, bytes)` | Replace an existing part wholesale (`Replaced`). New parts: `Error::NewPartUnsupported` (Phase0). |
| `remove_part(name, allowed_rel_types) -> RemovedPart`, `check_remove_part(&self, name, allowed_rel_types)` | Removes a part, every internal relationship resolving to it and every `Override` for it (never a `Default`); each removed registry element goes with the whitespace-only text right before it, nothing else in those parts changes. Refused with a typed `RemoveRefusal` (nothing changed, no part state changes) for registry parts, parts with their own `.rels`, a relationship type the caller did not allow (implicit references by type), any attribute of the source part equal to a removed `Id` after unescaping (`r:id`, VML `o:relid`, …), an unscannable source, or a registry that must change but was replaced or cannot be parsed. Part names follow OPC equivalence (ASCII case-insensitive and percent-decoded, for the part, sources, `_rels` folders, targets and `Override`s; XML whitespace around `Target`/`Id` is collapsed; percent-equivalent duplicate entries are refused at open); the safety net refuses when a relationship's target has the part's file name as its last segment (unescaped, percent-decoded, `\` counted as a separator) but resolves to no existing part (`UnresolvedMention`); only the root `_rels/.rels` holds package relationships. Both run one validation; after it, the removal cannot fail. The narrow interim exception to Layer 3's append-only rule (ADR-0001 note, ADR-0003 §4.3). |
| `content_types()`, `content_type(name)` | `[Content_Types].xml` (overrides, then extension defaults; ASCII case-insensitive). |
| `package_relationships()`, `relationships(part)`, `main_document_part()` | `opc::Relationships` with `by_id`, `by_type`, `resolve` (relative targets, `..`, absolute `/`). |
| `write_to(w)`, `to_bytes()`, `save(path)` | `Raw` and dirty-free `Parsed` parts go through `ZipWriter::raw_copy_file`: (name, method, CRC-32, compressed bytes) unchanged. |
| `parts::{PARTS, by_name, by_relationship_type, by_content_type}` | Generated part-kind table (content type, relationship type, root element/namespace, child parts). |

## Layer 2 — `XmlDocument`, `RawNode`

`XmlDocument::parse(bytes)` (UTF-8 only; a BOM is kept) builds a tree of
`RawNode`s. Each node has its resolved name (`XName`: namespace URI, local
name, original prefix), attributes in order (`xmlns` declarations included,
as attributes in `XMLNS_NS`), child items (`RawItem::{Element, Text, Markup}`),
its `ElementSpan` in the source, and `own_dirty` / `subtree_dirty`.
`serialize()` emits untouched subtrees verbatim (T1: no edits ⇒ identical
bytes). Any `<!DOCTYPE>` is refused (`Error::DtdNotAllowed`): no entity
expansion, no XXE. Constructs quick-xml would let through but that are not
well-formed, or would make a span inexact, are refused with
`Error::NotWellFormed { position, reason: WellFormedness }`: a second byte
order mark, an XML declaration not at the start, an empty element/attribute
name or prefix, U+0000, `xmlns:p=""`, and any reader position that does not
match the source.

Element nesting is limited. An element deeper than `ParseOptions::max_depth` makes the parse
fail with `Error::TooDeep { limit, position }` before the element is built. The default is
`DEFAULT_MAX_DEPTH` = 256, and the root is depth 1. The deepest real part measured is 33 (1,641
parts of POI, Open XML SDK, calamine, LibreOffice and python-pptx test files; median 9), so the
default leaves a 7x margin.

The limit is what bounds the recursive operations on a tree: `Clone`, `Drop`, `Debug`, dirty
serialization, `structurally_eq` and format-crate walks. At depth 1024, a debug-build `clone()`
needed about 1.7 MiB of stack. At 256, `tests/depth_limit.rs` runs every one of these operations
on a 1 MiB thread. Without a limit, a few kilobytes of nested tables cost gigabytes in
path-addressed format-crate code and overflow the stack (ADR-0004 round-2 review N1, PR #16
review M1).

- `XmlDocument::parse_with`, `Package::from_bytes_with` and `Package::open_with` take the
  options.
- **Raising the limit** raises stack use in proportion, by about 1.7 KiB per level for a
  debug-build clone. Above the default, run the tree operations on a thread with a larger stack.
- **Metadata:** `[Content_Types].xml` and `.rels` parts are never parsed with a limit below the
  default, so a lowered limit never breaks package metadata.
- **Edits:** a lens insert that would nest deeper than the document's limit returns `TooDeep`, so
  edited output always parses again with the same options.

Nodes are read-only from outside the crate; **all mutation goes through
`LensMut`**, which owns the dirty-flag duties. Test helpers:
`XmlDocument::force_rerender()` and `RawNode::structurally_eq()` (T1b).

## Lenses

`Lens<'a, E>` / `LensMut<'a, E>` are typed by an `ElementSpec` marker from
`ooxml-schema` (or a hand-written one). Generated `*View(Lens)` and
`*ViewMut(LensMut)` newtypes add typed accessors; `.0` reaches the generic
lens.

Reads (`Lens`): `root`, `raw`, `path`, `attr(q)`, `attr_or_default(q)`,
`attr_parsed(q, parse)`, `attr_parsed_or(q, default, parse)`,
`attr_enum::<V>(q) -> Option<EnumValue<V>>`, `attr_enum_or`, `text()`,
`xml_space_preserve()`, `children::<C>()`, `child::<C>()`, `cast::<F>()`.
Typed reads never coerce: a bad lexical value is `AttrError { attr, raw }`;
an unknown enum value is `EnumValue::Unknown(raw)`.

Navigation (`LensMut`): `node()`, `as_lens()`, `path()`, `child_mut::<C>()`,
`child_mut_by::<C>(pred)`, `nth_child_mut::<C>(n)`; consuming variants
`into_child_at::<C>(index)` / `into_child_by::<C>(pred)` keep the full
lifetime so helpers can return grandchild lenses. Child lenses are
reborrows: one at a time.

Write primitives (ADR-0002 §4.2):

| Primitive | Behaviour |
| --- | --- |
| `insert_by::<C>(before)` | Into `C`'s slot; `before(index_in_slot, existing)` picks the position inside the slot (callers own row/column order). Else before the first higher-ranked child, else immediately before the close tag. No whitespace is added. |
| `insert::<C>()` | Append at the end of the slot. |
| `insert_at::<C>(index)`, `check_insert_at::<C>(index)` | Insert at a child item index the caller computed (e.g. from its own row index): for a slot without limits, O(1) placement instead of `insert_by`'s scan of every child. Schema order is checked against the nearest listed neighbours only, every slot an ambiguous neighbour could take must be in order; slot limits are counted over all children exactly as `insert_by` does (and refuse next to any `mc:AlternateContent`); depth and prefix as `insert_by`; `check_insert_at` runs the same checks without changing anything, for validate-then-apply callers. `InsertIndexOutOfRange` for an index past the children. |
| `insert_in_slot::<C>(slot, before)` | For a QName that is in two slots (otherwise `AmbiguousSlot`). |
| `get_or_insert::<C>()` | First existing `C`, or insert. |
| `remove::<C>()`, `remove_at(index)` | Remove a child (whole span). |
| `set_attr(q, value)`, `remove_attr(q)` | Untyped; typed setters are generated. Setting the current value is not a change. |
| `set_text(value)` | `Content::Text` elements only; sets `xml:space="preserve"` for edge whitespace when the spec declares `xml:space`; never removes it. |
| `Lens::detach_as::<C>(skip)` → `Detached<C>` | Only for `Detachable` types (opted in by an allowlist `[detach]` section; never id-bearing subtrees). Owned, span-less, prefix-less copy, renamed to `C::QNAME`; `C: SameType<E> + Detachable`. Direct children named in `skip` are left out. Refuses (`NotDetachable { reason, name }`) MC content, relationship-id attributes (`name::RELATIONSHIP_ATTRIBUTES`), namespace declarations, namespaces an ancestor's `mc:Ignorable` lists, text under an inherited `xml:space="preserve"`, document-unique attributes with a non-default value (`Detachable::UNIQUE_ATTRS`, e.g. `smtId`), and extension lists (`extLst`) unless skipped. Use a lens that knows the ancestors (`XmlDocument::lens`, `Lens::at`). |
| `insert_detached[_by]::<C>(&d[, before])` | Places the copy like `insert_by::<C>`; every element/attribute prefix is chosen at the destination **before** the tree changes; created nodes are re-rendered. Same refusals as `insert_by` (incl. `TooDeep`, counted at the copy's deepest element), plus `NamespaceNotInScope` and the destination's `mc:Ignorable` / `xml:space`. `Detached::check_insert_under(doc, path, created)` checks those without changing anything (`WrongElement` for a bad path). |

Refusals: `AlreadyPresent` (slot `max_each`/`max_total`),
`UnresolvedSibling` (an `mc:AlternateContent` child, or existing children
out of schema order), `UnsupportedContentModel`, `NotAllowed`, `NotText`,
`IncompleteSpec` (spec with an unresolved SDK base), `NamespaceNotInScope`
(no usable prefix; Phase0 never declares namespaces), `NamespaceDeclaration`
(`set_attr`/`remove_attr` on `xmlns`/`xmlns:*`: declarations are managed by
core only), `InvalidXmlChar`
(`set_text`/`set_attr` value with a character outside XML 1.0 `Char`, e.g.
U+000B; checked before anything changes, so no primitive can write a part
that is not well-formed).

Dirty duties: every write marks `subtree_dirty` up to the root; attribute
writes and creation set `own_dirty`; a parent becomes `own_dirty` only if
it was self-closing and gains its first child; a parent with a close tag
keeps its original tags even when emptied.

Prefixes for new nodes: the parent's prefix if it binds the URI, else the
innermost binding in effect (default namespace preferred on the same
element); attributes need a non-empty prefix; `xml:` always works.

## Schema description types

`ChildOf` is generated between allowlisted entries, including the
SDK-listed children of a wildcard parent such as `a:graphicData` (ADR-0002
post-acceptance note). `QName` (static), `ElementSpec { QNAME, SDK_NAME, ATTRS, CONTENT,
ATTRS_INCOMPLETE }`, `ChildOf<P>` (compile-time child check with a readable
`on_unimplemented` message), `Detachable` (opted-in copy sources),
`SameType<S>` (same schema type: a renamed copy is schema-valid), `AttrSpec`, `SimpleType`, `Slot { names,
wildcard, max_each, max_total }`, `Wildcard`, `Content::{Empty, Text, Slots,
Unsupported}`, `EnumType`, `EnumValue<'a, E>`, and the `const fn` checks
(`assert_spec`, `assert_child`, …) used by generated `const _` items.
`value::*` has the lexical parsers/formatters generated accessors call.
Namespace constants the core acts on: `name::{MC_NS, R_NS, XML_NS, XMLNS_NS}`.

## Not yet (by design or later)

- Adding parts / adding `[Content_Types].xml` entries or relationships
  (the append-only rule applies when writes arrive). The only registry write
  so far is the removal done by `remove_part`.
- The `mc:AlternateContent` rank rule (ADR-0002 §4.3 "later rule").
- Copies across parts that carry relationships (`r:` attributes are
  refused by `detach_as`; copying them needs Layer 3 writes).
- Formula detection is format-specific and not in core. If a scan helper is
  added here it will match element names (namespace URI + local name),
  never byte patterns.
