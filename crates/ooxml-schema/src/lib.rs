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

mod generated;

pub use generated::*;
