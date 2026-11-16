//! # ooxml-docx
//!
//! Lossless editing of existing Word documents, Phase0 of
//! [ADR-0004](https://github.com/cuzic/ooxml-rs/blob/main/docs/adr/0004-docx-phase0.md):
//!
//! - read the paragraphs of the main document's body flow, their text and
//!   their runs, each with a [`RunContext`];
//! - replace the text of one existing run ([`Document::set_run_text`]);
//! - append a plain paragraph before the body's `w:sectPr`
//!   ([`Document::append_paragraph`]);
//! - refuse, with a typed [`Error`] and before any change, everything Phase0
//!   cannot do safely: encrypted, Strict or DTD-carrying packages; documents
//!   with track changes on, enforced protection or content-dependent fields;
//!   runs inside fields, revisions, content controls, smart tags, or with
//!   non-text content.
//!
//! Untouched parts are copied byte for byte, and an edit changes the main
//! part's bytes only inside the edited run (or at the insertion point).
//! Phase0 creates no IDs, styles, relationships or parts.
//!
//! ```no_run
//! use ooxml_docx::{Document, RunContext};
//!
//! let mut doc = Document::open("letter.docx")?;
//! doc.write_gate()?; // track changes, protection, stale-field risk
//! let target = doc.paragraphs().find_map(|p| {
//!     p.runs().find(|r| r.text() == "{{name}}" && r.check_settable().is_ok()).map(|r| (p.index(), r.index()))
//! });
//! if let Some((p, r)) = target {
//!     doc.set_run_text(p, r, "Ada Lovelace")?;
//! }
//! doc.append_paragraph("Regards,\tthe team")?;
//! doc.save("letter-filled.docx")?;
//! # Ok::<(), ooxml_docx::Error>(())
//! ```
//!
//! `Document::new()` (a document from scratch) is not part of Phase0.

mod body;
mod document;
mod error;
pub mod fields;
mod names;
mod text;

pub use document::{Document, Paragraph, Run};
pub use error::{Error, Result};

/// Where a run sits (ADR-0004 §3). Only [`RunContext::Direct`] and
/// [`RunContext::Hyperlink`] runs can be settable in Phase0.
///
/// When several wrappers enclose a run, the outermost one other than
/// `w:hyperlink` decides (a hyperlink inside a content control is
/// `ContentControl`). Complex fields are not a context; see
/// [`Run::in_field`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RunContext {
    /// A child of `w:p` (possibly inside tables).
    Direct,
    /// Inside `w:hyperlink` only.
    Hyperlink,
    /// Inside `w:ins`.
    Inserted,
    /// Inside `w:del`.
    Deleted,
    /// Inside `w:moveFrom`.
    MovedFrom,
    /// Inside `w:moveTo`.
    MovedTo,
    /// Inside `w:smartTag`.
    SmartTag,
    /// Inside a content control (`w:sdt`), at paragraph, row, cell or block
    /// level.
    ContentControl,
    /// Inside `w:fldSimple`.
    SimpleField,
    /// Inside `w:customXml`.
    CustomXml,
    /// Inside `w:dir` or `w:bdo`.
    Bidi,
    /// Ruby base text (`w:ruby/w:rubyBase`).
    Ruby,
}
