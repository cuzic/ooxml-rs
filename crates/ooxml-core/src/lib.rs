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
//!   registries, written only by `Package`'s append-only primitives
//!   (`add_part`, `add_relationship`, `set_content_type`) and by
//!   `remove_part`.
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

pub use error::{Error, Result};
pub use lens::{Lens, LensMut};
pub use name::{QName, XName};
pub use package::{
    IncomingRelationship, Package, PartContent, PartNameRequest, PartState, PartStateKind, RegistryPlan, RegistryRefusal, RemoveRefusal,
    RemovedPart, RemovedParts, Target,
};
pub use schema::{AttrError, AttrSpec, ChildOf, Content, ElementSpec, EnumType, EnumValue, SimpleType, Slot, Wildcard};
pub use xml::{DEFAULT_MAX_DEPTH, ElementSpan, ParseOptions, RawAttr, RawItem, RawNode, RawText, XmlDocument};
