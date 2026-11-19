//! Run content: reading it as text, the text-only check (R4), and the
//! mapping from a Rust string to run content (ADR-0004 §3, rev 2).

use std::borrow::Cow;

use ooxml_core::{RawItem, RawNode};
use ooxml_schema::wml::{
    Break, CarriageReturn, DeletedText, LastRenderedPageBreak, NoBreakHyphen, RunProperties, SoftHyphen, TabChar,
    Text,
};

use crate::error::{Error, Result};
use crate::names::{display_name, is};

/// One item of run content as Phase0 writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    /// `w:t`
    Text(String),
    /// `w:tab`
    Tab,
    /// `w:br` (text wrapping)
    Br,
}

/// Maps text to run content.
///
/// - maximal substrings without `\t`, `\n`, `\r` → `w:t`;
/// - `\t` → `w:tab`;
/// - `\n`, `\r\n` and a lone `\r` → `w:br` (XML would normalize a literal
///   CR to LF on re-reading, so CR is never written into `w:t`);
/// - characters XML 1.0 cannot carry → [`Error::InvalidXmlChar`].
pub(crate) fn map_text(s: &str) -> Result<Vec<Piece>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let piece = match c {
            '\t' => Piece::Tab,
            '\n' => Piece::Br,
            '\r' => {
                chars.next_if_eq(&'\n');
                Piece::Br
            }
            c if c < ' ' || c == '\u{FFFE}' || c == '\u{FFFF}' => return Err(Error::InvalidXmlChar { ch: c }),
            c => {
                cur.push(c);
                continue;
            }
        };
        if !cur.is_empty() {
            out.push(Piece::Text(std::mem::take(&mut cur)));
        }
        out.push(piece);
    }
    if !cur.is_empty() {
        out.push(Piece::Text(cur));
    }
    Ok(out)
}

/// Does a `w:t` with this text need `xml:space="preserve"`: leading or
/// trailing space, or two consecutive spaces? (`w:t` never holds tabs or
/// line breaks after [`map_text`].)
pub(crate) fn needs_preserve(s: &str) -> bool {
    s.starts_with(' ') || s.ends_with(' ') || s.contains("  ")
}

fn is_text_wrapping_br(node: &RawNode) -> bool {
    matches!(node.attr(Break::ATTR_W_TYPE), None | Some("textWrapping"))
        && matches!(node.attr(Break::ATTR_W_CLEAR), None | Some("none"))
}

/// The run's text: `w:t` and `w:delText` as written, `w:tab` → `\t`,
/// `w:br`/`w:cr` → `\n`, `w:noBreakHyphen` → U+2011, `w:softHyphen` →
/// U+00AD. Other content (drawings, field codes, symbols, …) adds nothing.
pub(crate) fn run_text(run: &RawNode) -> String {
    let mut s = String::new();
    for c in run.elements() {
        s.push_str(&run_child_text(c));
    }
    s
}

pub(crate) fn run_child_text(node: &RawNode) -> Cow<'_, str> {
    Cow::Borrowed(match () {
        () if is::<Text>(node) || is::<DeletedText>(node) => return node.text(),
        () if is::<TabChar>(node) => "\t",
        () if is::<Break>(node) || is::<CarriageReturn>(node) => "\n",
        () if is::<NoBreakHyphen>(node) => "\u{2011}",
        () if is::<SoftHyphen>(node) => "\u{AD}",
        () => "",
    })
}

/// R4: is the run text-only? Whitespace-only text between children (pretty
/// printing) is not content.
pub(crate) fn check_text_only(run: &RawNode) -> Result<()> {
    for item in run.children() {
        let ok = match item {
            RawItem::Element(c) => {
                is::<RunProperties>(c)
                    || is::<Text>(c)
                    || is::<TabChar>(c)
                    || (is::<Break>(c) && is_text_wrapping_br(c))
                    || is::<CarriageReturn>(c)
                    || is::<NoBreakHyphen>(c)
                    || is::<SoftHyphen>(c)
                    || is::<LastRenderedPageBreak>(c)
            }
            RawItem::Text(t) => t.is_whitespace(),
            RawItem::Markup(_) => false,
        };
        if !ok {
            let child = match item {
                RawItem::Element(c) if is::<Break>(c) => format!("{} with type/clear", display_name(c)),
                RawItem::Element(c) => display_name(c),
                RawItem::Text(_) => "text".into(),
                RawItem::Markup(m) => match m.kind() {
                    ooxml_core::xml::MarkupKind::Comment => "comment".into(),
                    ooxml_core::xml::MarkupKind::ProcessingInstruction => "processing instruction".into(),
                },
            };
            return Err(Error::RunNotTextOnly { child });
        }
    }
    Ok(())
}

/// Does a `w:t` of the run need `xml:space="preserve"` (by
/// [`needs_preserve`]) without carrying it itself? Word drops the edge
/// spaces of such text (review round 2, m6).
pub(crate) fn lacks_needed_preserve(run: &RawNode) -> bool {
    run.elements().filter(|c| is::<Text>(c)).any(|t| needs_preserve(&t.text()) && t.xml_space_preserve() != Some(true))
}

/// The run's content as [`Piece`]s, if it is already in the form Phase0
/// writes (only `w:t`, `w:tab`, text-wrapping `w:br` besides `w:rPr` and
/// whitespace). Used to make writing the same content a no-op.
pub(crate) fn current_pieces(run: &RawNode) -> Option<Vec<Piece>> {
    run.elements()
        .filter(|c| !is::<RunProperties>(c))
        .map(|c| match () {
            () if is::<Text>(c) => Some(Piece::Text(c.text().into_owned())),
            () if is::<TabChar>(c) => Some(Piece::Tab),
            () if is::<Break>(c) && c.attrs().iter().all(|a| a.name().is_namespace_declaration()) => Some(Piece::Br),
            () => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        use Piece::*;
        let t = |s: &str| Text(s.into());
        assert_eq!(map_text("").unwrap(), vec![]);
        assert_eq!(map_text("a").unwrap(), vec![t("a")]);
        assert_eq!(map_text("a\tb\nc").unwrap(), vec![t("a"), Tab, t("b"), Br, t("c")]);
        assert_eq!(map_text("a\r\nb\rc").unwrap(), vec![t("a"), Br, t("b"), Br, t("c")]);
        assert_eq!(map_text("\t\t").unwrap(), vec![Tab, Tab]);
        assert_eq!(map_text("\r\n\r\n").unwrap(), vec![Br, Br]);
        assert_eq!(map_text("x\u{2011}y\u{AD}").unwrap(), vec![t("x\u{2011}y\u{AD}")]);
        for bad in ['\0', '\u{8}', '\u{B}', '\u{C}', '\u{1F}', '\u{FFFE}', '\u{FFFF}'] {
            assert!(matches!(map_text(&format!("a{bad}")), Err(Error::InvalidXmlChar { ch }) if ch == bad));
        }
        assert!(needs_preserve(" a") && needs_preserve("a ") && needs_preserve("a  b"));
        assert!(!needs_preserve("a b") && !needs_preserve(""));
    }
}
