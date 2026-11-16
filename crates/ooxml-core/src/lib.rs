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

pub use error::{DetachRefusal, Error, MoveRefusal, Result, WellFormedness};
pub use lens::{Detached, Lens, LensMut, MoveSource, NsDecl};
pub use name::{QName, XName};
pub use package::{Package, PartState, PartStateKind, RemoveRefusal, RemovedPart};
pub use schema::{AttrError, AttrSpec, ChildOf, Content, Detachable, ElementSpec, EnumType, EnumValue, SameType, SimpleType, Slot, UniqueAttr, Wildcard};
pub use xml::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_PACKAGE_BYTES, DEFAULT_MAX_PART_BYTES, ElementSpan, ParseOptions, RawAttr, RawItem, RawNode, RawText, XmlDocument};
