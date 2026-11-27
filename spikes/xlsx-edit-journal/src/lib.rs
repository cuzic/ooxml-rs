//! ADR-0024 Edit Journal: the J0/J1 keyed-op slice, as a spike over
//! `ooxml-xlsx`'s public API. See README.md.

pub mod commit;
pub mod exceljs;
pub mod fixture;
pub mod journal;
pub mod normalize;
pub mod op;
pub mod project;
pub mod shadow;

pub use journal::{CommitReport, Fault, JError, JResult, Journal, Mode, NormCell};
pub use op::{Api, CellKey, Content, EchoField, Op, Patch, RawInput, Start};
