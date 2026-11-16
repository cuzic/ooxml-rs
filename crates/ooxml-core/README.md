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
| `set_part(name, bytes)` | Replace an existing part wholesale (`Replaced`). A name that is not a part: `Error::NewPartUnsupported`; new parts go through the registry plan below. |
| `RegistryPlan::new(&pkg)` + steps + `pkg.apply(plan)` | **Layer 3 writes as one transaction.** Steps are validated against `&Package` plus the earlier steps (a relationship may target a part added earlier; a second relationship from one source gets the next `Id`; parts removed earlier are gone). A refused step returns `Error::RegistryRefused` and leaves the plan as it was. `apply` cannot fail except `StalePlan` (another package, or this one changed — `set_part`, `xml_part_mut`, a removal, another `apply` — since the plan started). Building a plan and dropping it is the validate-only twin of a whole multi-step edit. |
| `plan.add_part(&pkg, PartNameRequest, content_type, content)` | `Exact(name)` or `Numbered("xl/worksheets/sheet{}.xml")` (**max + 1** over existing parts, `Override`s, relationship targets incl. dangling ones, names removed in this session or plan; gaps are not filled). Content: bytes (XML content types must be well-formed) or an `XmlDocument` (stays editable). Content type: nothing when a `Default` (existing or planned) gives exactly the type, else an `Override`. Refused: invalid OPC name (Part 2 §6.2.2), registry name, name in use (part under OPC equivalence, segment prefix, `Override`, relationship target, **a left-over `.rels` for the name**), malformed XML, `[Content_Types].xml` replaced/unparseable. New parts are appended after the original zip entries, Deflated. |
| `plan.add_default_content_type(&pkg, ext, content_type)` | Appends a `Default`; `Ok(false)` if exactly that one exists. Refused if the extension has a `Default` with another type, or if an existing/planned part with the extension has no `Override` (its type would change). |
| `plan.add_relationship(&pkg, source, rel_type, Target)` | `Target::Part(name)` (written relative to the source) or `Target::External(uri)`. `Id` = `rId{max N + 1}` over the source's `rIdN` Ids (prefix ASCII case-insensitive), skipping Ids in use under case folding; other Ids (`R44c8…`) are left alone; never reuses a removed Id in the same plan. A missing `.rels` part is created and registered. |
| `plan.remove_relationship(&pkg, source, id)` | Removes one relationship (with the whitespace before it). Refused when the source part refers to `id` in any attribute (PR #14's any-attribute rule) or cannot be scanned. |
| `plan.set_content_type(&pkg, name, content_type)` | Rewrites the `ContentType` of the part's `Override`s, or appends an `Override` when a `Default` covered it; a no-op when unchanged. |
| `plan.remove_parts(&pkg, names, allowed_rel_types) -> RemovedParts` | Removes a **closed set** of parts, each with its own `.rels`: references inside the set do not block. Relationships from staying parts to the set must have an allowed type and must not be referenced by their source (PR #14 rules: `RemoveRefusal` inside `RegistryRefusal::Remove`); `Override`s of everything removed go; `Default`s never. Returns the kept internal targets of the removed parts, so a caller can cascade (`incoming_relationships(name)` tells whether a part is an orphan). |
| `plan.stage_part(&pkg, name, content)`, `plan.copy_relationships(&pkg, src, dst, target_map)`, `plan.content_type(&pkg, name)` | New content for an existing part, validated with the batch; a duplicated part's relationships with the same `Id`s (internal targets re-relativised or mapped); the type a name has in the plan's view. |
| One-step conveniences | `add_part`, `add_related_part`, `add_relationship`, `add_default_content_type`, `remove_relationship`, `set_content_type`, `remove_parts`, each with a `check_*` twin; `free_part_name(request)`, `part_name_in_use(name)`, `incoming_relationships(name)`. |
| `remove_part(name, allowed_rel_types) -> RemovedPart`, `check_remove_part(&self, name, allowed_rel_types)` | Removes a part, every internal relationship resolving to it and every `Override` for it (never a `Default`); each removed registry element goes with the whitespace-only text right before it, nothing else in those parts changes. Refused with a typed `RemoveRefusal` (nothing changed, no part state changes) for registry parts, parts with their own `.rels`, a relationship type the caller did not allow (implicit references by type), any attribute of the source part equal to a removed `Id` after unescaping (`r:id`, VML `o:relid`, …), an unscannable source, or a registry that must change but was replaced or cannot be parsed. Part names follow OPC equivalence (ASCII case-insensitive and percent-decoded, for the part, sources, `_rels` folders, targets and `Override`s; XML whitespace around `Target`/`Id` is collapsed; percent-equivalent duplicate entries are refused at open); the safety net refuses when a relationship's target has the part's file name as its last segment (unescaped, percent-decoded, `\\` counted as a separator) but resolves to no existing part (`UnresolvedMention`); only the root `_rels/.rels` holds package relationships. Both run one validation; after it, the removal cannot fail. The narrow interim exception to Layer 3's append-only rule (ADR-0001 note, ADR-0003 §4.3). |
| `content_types()`, `content_type(name)` | `[Content_Types].xml` (overrides, then extension defaults; ASCII case-insensitive). |
| `package_relationships()`, `relationships(part)`, `main_document_part()` | `opc::Relationships` with `by_id`, `by_type`, `resolve` (relative targets, `..`, absolute `/`). |
| `write_to(w)`, `to_bytes()`, `save(path)` | `Raw` and dirty-free `Parsed` parts go through `ZipWriter::raw_copy_file`: (name, method, CRC-32, compressed bytes) unchanged. |
| `parts::{PARTS, by_name, by_relationship_type, by_content_type}` | Generated part-kind table (content type, relationship type, root element/namespace, child parts). |

### Layer 3 write rules (single source of truth for ADR-0009, ADR-0011, ADR-0012)

| Rule | Decision | Why |
| --- | --- | --- |
| Transaction | `RegistryPlan` (owned, no borrow of the package) collects steps; each step is validated against the package **plus the earlier steps**, and a refused step leaves the plan unchanged; `Package::apply` applies all of it and cannot fail except `StalePlan`. Dropping a plan is the validate-only twin of a whole batch. | Dependent multi-part edits (part + relationship to it; content edit + removal of what it stops naming) must validate together (ADR-0009 B2, ADR-0011 B1, ADR-0012 B1). |
| Content of existing parts | `stage_part(name, bytes \| XmlDocument)`: staged content is the source that later reference scans read; content staged after a relationship removal may not name the removed `Id`. Edit a clone (`pkg.xml_part(name)?.clone()`) with a lens, then stage it. | A lens edit and a registry edit go in one batch without holding `&mut Package`. |
| Part names | `Exact(name)` or `Numbered(template)`: **max N + 1** per template (ASCII case-insensitive, percent-decoded), over existing parts, `Override`s, relationship targets (dangling ones included), names removed in the session or the plan, and names planned earlier. Gaps are never filled. | A name that existed may still be named somewhere this crate cannot see (a dangling relationship elsewhere, another tool's cache, a removed part's leftover references); max + 1 never reuses one, so a part name identifies one part across sessions. ADR-0011's "smallest free n" also avoids collisions but can resurrect such a name; the corpora have no case where max + 1 is worse than a gap. |
| A name is taken when | a part has an OPC-equivalent name, or one is a segment prefix of the other; an `Override` names it; a relationship target resolves to it (dangling included); **a `.rels` part for it is left over** (ADR-0009 M6); it was removed in the plan. | Each would give the new part a meaning or a type it was not given. |
| Relationship `Id`s | `rId{N + 1}`, N = largest number among the source's `rId<digits>` Ids (prefix ASCII case-insensitive) and the Ids planned for it; an `Id` used under ASCII case folding is skipped. Other Ids (`R44c8…`) are left alone. An Id removed in the plan or earlier in the session (`remove_part` included) is not reused: the high-water is per relationships part and per session. `copy_relationships` keeps the source's Ids (only into a part without relationships). | Matches Office and POI output (`rIdN` in 99% of corpus files); gaps may be relationships another tool removed while a stale `r:id` survived. |
| Content types | A new part gets **nothing** when a `Default` (existing or planned) gives exactly its type; otherwise an **`Override`**. A `Default` is added only by `add_default_content_type`, and only when no existing or planned part with that extension lacks an `Override` (else its type would change). `Default`s are never changed or removed. A left-over `Override` for the name makes the name taken. | Media: call `add_default_content_type(ext)` first when the extension has none (the Office convention, ADR-0011), then `add_part`; when a `Default` for the extension has another type (e.g. `bin` = printer settings), the part gets an `Override` (ADR-0009). |
| Changing entries | Only: the `ContentType` of one named part's `Override`s (`set_content_type`); removing a relationship nobody refers to (`remove_relationship`); removing the entries that register removed parts (`remove_parts`). | Layer 3 append-only invariant, generalised (ADR-0001 note). |
| Removal | `remove_parts(set, allowed_rel_types)`: a **closed set**, each part with its own `.rels`; references inside the set do not block; a relationship from a staying part must have an allowed type (parts found by type are referenced implicitly) and must not be named by its source (any attribute, any namespace, after unescaping; staged content counts); `Override`s go, `Default`s never; returns kept targets; `incoming_relationships(part)` answers "is it an orphan now". | PR #14's safety conditions, extended to sets (ADR-0009 B1, ADR-0012 §6). |
| Registry parts | `[Content_Types].xml` and `.rels` parts are written only by these operations; one that was replaced wholesale (`set_part`) or does not parse refuses any step that must change it. New entries are appended after a copy of the whitespace before the current last entry. | Byte locality: untouched registry parts stay raw (L1); touched ones change only at the end. |
| New parts in the zip | Appended after every original entry, Deflated; `[Content_Types].xml` keeps its position. | L1 for every original entry. |

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

- Declaring a namespace on a created element (`LensMut::declare_namespace`,
  ADR-0011 §1.3) is a lens change, not a registry one; not in this crate yet.
- The `mc:AlternateContent` rank rule (ADR-0002 §4.3 "later rule").
- Copies across parts that carry relationships (`r:` attributes are
  refused by `detach_as`; copying them needs Layer 3 writes).
- Formula detection is format-specific and not in core. If a scan helper is
  added here it will match element names (namespace URI + local name),
  never byte patterns.
