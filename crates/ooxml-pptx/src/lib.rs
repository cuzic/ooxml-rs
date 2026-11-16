//! # ooxml-pptx
//!
//! PowerPoint (PresentationML) on top of [`ooxml_core`] and the generated
//! [`ooxml_schema`] `pml`/`dml` views: open and save any presentation
//! losslessly, walk slides and shapes, read text, and edit text **inside
//! existing text bodies** (Phase 0 and 0.1, [ADR-0005]).
//!
//! ```no_run
//! use ooxml_pptx::Presentation;
//!
//! let mut pres = Presentation::open("deck.pptx")?;
//! let mut slide = pres.slide_mut(0)?;
//! let path = slide.as_slide().placeholders()[0].path().clone();
//! slide.shape_mut(&path)?.text_body_mut()?.set_text("New title")?;
//! pres.save("deck.pptx")?;
//! # Ok::<(), ooxml_pptx::Error>(())
//! ```
//!
//! Every edit that would break a reference into the text (animations,
//! modern comments), touch a field, an `mc:AlternateContent` branch or a
//! merged cell, or emit a character XML cannot hold, is refused with a
//! typed [`Error`] **before** anything changes.
//!
//! [ADR-0005]: https://github.com/cuzic/ooxml-rs/blob/main/docs/adr/0005-pptx-phase0.md

mod builtin_styles;
mod error;
mod names;
mod presentation;
mod read;
mod refs;
mod text;
mod write;

pub use error::{Error, Result};
pub use presentation::Presentation;
pub use read::{
    CellRef, FieldRef, ParagraphRef, PlaceholderRef, RunRef, ShapeKind, ShapePath, ShapeRef, Slide, SlideInfo,
    TableRef, TextBodyRef, TextItem,
};
pub use write::{CellFill, Color, Edge, Line, ParagraphMut, RunMut, ShapeMut, Side, SlideMut, TableFlag, TableMut, TextBodyMut};
