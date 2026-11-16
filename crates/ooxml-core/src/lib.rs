//! # ooxml-core
//!
//! The format-agnostic foundation of ooxml-rs (ADR-0001, ADR-0002):
//!
//! - **Layer 1** — [`Package`]: an OPC zip whose untouched parts are copied
//!   through with their original compressed bytes.
//! - **Layer 2** — [`XmlDocument`] / [`RawNode`]: a lossless XML tree with
//!   per-element source spans and dirty flags, so untouched subtrees are
//!   written back byte-for-byte; [`Lens`] / [`LensMut`]: typed views and the
//!   schema-aware write primitives.
//! - **Layer 3** — [`opc`]: `[Content_Types].xml` and relationship
//!   registries (read-only for now).
//!
//! Schema knowledge ([`ElementSpec`] implementations) is generated into the
//! `ooxml-schema` crate; see `README.md` for the API tour.

pub mod error;
pub mod lens;
pub mod name;
pub mod opc;
pub mod package;
pub mod parts;
pub mod schema;
pub mod value;
pub mod xml;

pub use error::{DetachRefusal, Error, Result};
pub use lens::{Detached, Lens, LensMut};
pub use name::{QName, XName};
pub use package::{Package, PartState, PartStateKind};
pub use schema::{AttrError, AttrSpec, ChildOf, Content, Detachable, SameType, ElementSpec, EnumType, EnumValue, SimpleType, Slot, Wildcard};
pub use xml::{ElementSpan, RawAttr, RawItem, RawNode, RawText, XmlDocument};
