//! # ooxml-schema
//!
//! Generated schema knowledge for ooxml-rs (ADR-0002): one uninhabited
//! marker type per allowlisted Open XML SDK entry (an
//! [`ElementSpec`](ooxml_core::ElementSpec)), the
//! [`ChildOf`](ooxml_core::ChildOf) relation between them, enumerations as
//! [`EnumType`](ooxml_core::EnumType)s, and typed `*View` / `*ViewMut`
//! newtypes over [`Lens`](ooxml_core::Lens) / [`LensMut`](ooxml_core::LensMut).
//!
//! Each namespace group is a cargo feature: `sml` (default), `wml`, `pml`,
//! `dml`. Everything under `generated/` is written by `cargo xtask codegen`
//! from the vendored inputs and `codegen/`; do not edit it by hand.
//!
//! ```
//! # #[cfg(feature = "sml")] {
//! use ooxml_core::XmlDocument;
//! use ooxml_schema::sml::{Cell, CellView, CellValues};
//! use ooxml_core::EnumValue;
//!
//! let doc = XmlDocument::parse(
//!     br#"<c xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" r="B2"/>"#,
//! ).unwrap();
//! let cell = CellView(doc.lens::<Cell>().unwrap());
//! assert_eq!(cell.style_index(), Ok(0)); // XSD default
//! assert_eq!(cell.data_type(), EnumValue::Known(CellValues::Number));
//! # }
//! ```
//!
//! Third-party notices for the generated content: `THIRD-PARTY-NOTICES`.
#![cfg_attr(
    all(feature = "pml", feature = "dml"),
    doc = r##"
## Detachable types (ADR-0002 note "detach/insert")

Only entries listed in an allowlist's `[detach]` section implement
[`Detachable`](ooxml_core::Detachable), so `detach_as` compiles for DrawingML
formatting, renamed through a generated `SameType` pair:

```
use ooxml_core::{Lens, XmlDocument};
use ooxml_schema::dml::{EndParagraphRunProperties, RunProperties};

let doc = XmlDocument::parse(br#"<a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:r><a:rPr lang="en"><a:latin typeface="X"/></a:rPr></a:r></a:p>"#).unwrap();
let rpr = Lens::<RunProperties>::at(doc.root(), &[0, 0]).unwrap();
let copy = rpr.detach_as::<EndParagraphRunProperties>(&[]).unwrap();
assert_eq!(copy.raw().name().local(), "endParaRPr");
```

but not for a shape, whose subtree carries a document-unique `cNvPr@id`
(copying it would duplicate the id):

```compile_fail
use ooxml_core::XmlDocument;
use ooxml_schema::pml::Shape;

let doc = XmlDocument::parse(br#"<p:sp xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#).unwrap();
let _ = doc.lens::<Shape>().unwrap().detach_as::<Shape>(&[]); // Shape: !Detachable
```

nor for a text body, a group or a shape tree (`p:txBody`, `p:grpSp`, `p:spTree`):

```compile_fail
use ooxml_core::XmlDocument;
use ooxml_schema::pml::GroupShape;

let doc = XmlDocument::parse(br#"<p:grpSp xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"/>"#).unwrap();
let _ = doc.lens::<GroupShape>().unwrap().detach_as::<GroupShape>(&[]);
```

A **same-part** copy (note "same-part copy") keeps relationship ids, so it is
another type, which `insert_detached` (the any-part insertion) does not take;
only `insert_same_part` does, and it checks the document at run time:

```
use ooxml_core::{SamePartOptions, XmlDocument};
use ooxml_schema::dml::{Paragraph, Run, RunProperties};

let mut doc = XmlDocument::parse(br#"<a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><a:r><a:rPr><a:hlinkClick r:id="rId2"/></a:rPr><a:t>x</a:t></a:r><a:r><a:t>y</a:t></a:r></a:p>"#).unwrap();
let rpr = doc.lens::<Paragraph>().unwrap().child::<Run>().unwrap().child::<RunProperties>().unwrap();
let copy = rpr.detach_same_part_as::<RunProperties>(&[], &SamePartOptions::default()).unwrap();
let mut p = doc.lens_mut::<Paragraph>().unwrap();
p.nth_child_mut::<Run>(1).unwrap().insert_same_part(&copy).unwrap();
```

```compile_fail
use ooxml_core::{SamePartOptions, XmlDocument};
use ooxml_schema::dml::{Paragraph, Run, RunProperties};

let mut doc = XmlDocument::parse(br#"<a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:r><a:rPr/></a:r><a:r/></a:p>"#).unwrap();
let rpr = doc.lens::<Paragraph>().unwrap().child::<Run>().unwrap().child::<RunProperties>().unwrap();
let copy = rpr.detach_same_part_as::<RunProperties>(&[], &SamePartOptions::default()).unwrap();
let mut p = doc.lens_mut::<Paragraph>().unwrap();
p.nth_child_mut::<Run>(1).unwrap().insert_detached(&copy); // SamePartDetached is not Detached
```
"##
)]

mod generated;

pub use generated::*;
