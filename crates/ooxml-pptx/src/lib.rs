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
//! [`Presentation::find`] / [`Presentation::replace_all`] search and replace
//! across runs ([ADR-0008] §4), in slides and notes. [`Presentation::notes_mut`]
//! edits an existing notes slide's body with the same text API (§2).
//!
//! Every edit that would break a reference into the text (animations,
//! modern comments), touch a field, an `mc:AlternateContent` branch or a
//! merged cell, or emit a character XML cannot hold, is refused with a
//! typed [`Error`] **before** anything changes.
//!
//! [ADR-0005]: https://github.com/cuzic/ooxml-rs/blob/main/docs/adr/0005-pptx-phase0.md
//! [ADR-0008]: https://github.com/cuzic/ooxml-rs/blob/main/docs/adr/0008-pptx-phase1-deck-reading.md

mod builtin_styles;
mod deck;
mod deckread;
mod error;
mod names;
mod new_deck;
mod notes;
mod presentation;
mod read;
mod refs;
mod slides;
mod replace;
mod resolve;
mod text;
mod write;

pub use deck::{ChainLink, Deck, DeckSlide, PartRef, Sheet, SheetKind};
pub use deckread::{
    Anchor, Author, Branch, Build, Comment, Effect, Link, LinkColor, LinkEvent, LinkLocation, LinkTarget, PartLinks, Section, Sections, ShowJump, Target,
    Timing, Transition, UNDERSTOOD_NAMESPACES,
};
pub use error::{Error, Result};
pub use new_deck::NewOptions;
pub use resolve::{
    Autofit, BodyProps, Bullet, ClrMap, Color, Family, Fill, Font, Lookup, MapSource, MatchKind, Outline, ParaProps, PlaceholderMatch,
    Reason, Rect, Resolved, Role, RunItem, RunProps, Source, Spacing, TextStyleKind, WinnerAttr, Xfrm, script_for_lang,
};
pub use notes::{NotesMut, NotesRef};
pub use presentation::Presentation;
pub use slides::LayoutRef;
pub use read::{
    CellRef, FieldRef, ParagraphRef, PlaceholderRef, RunRef, ShapeKind, ShapePath, ShapeRef, Slide, SlideInfo,
    TableRef, TextBodyRef, TextItem,
};
pub use replace::{
    FindOptions, Match, MixedFormat, OnRefusal, PROOFING_ATTRIBUTES, ReplaceOptions, ReplaceReport, Scope, TextLocation,
};
pub use write::{FieldMut, GroupMut, LinkSpec, ShapeFill, SlideLinkTarget};
pub use write::{AutoFitMode, BulletSize, BulletSpec, ColorSpec, ColorTransform, FontRef, FormatChange, LineEnd, LineSpec, PresetColor, RunFill, RunFormat, SchemeColor, Script, SystemColor, TabStop, ThemeFont, UnderlineFill, UnderlineLine, WriteColor, CacheReport, CellFill, Edge, Line, ParagraphMut, RunMut, ShapeMut, Side, SlideMut, SmartArtMut, SmartArtNode, TableColor, TableFlag, TableMut, TextBodyMut};

pub use write::{
    StyleId, TablePartStyle, TableStyleBorders, TableStyleCell, TableStyleDef, TableStyleParts,
    TableStyleText, ThemeColor, ThemeColorSlot, ThemeFonts, ThemeMut, ThemeOptions,
};
