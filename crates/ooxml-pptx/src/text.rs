//! Character rules shared by every text setter (ADR-0005 §3 "Text
//! arguments", review B2). Checked before anything changes.
//!
//! | API | `\v` | `\n` | `\r` | other non-XML characters |
//! |---|---|---|---|---|
//! | `RunMut::set_text`, `ParagraphMut::add_run` | `LineBreakInRun` | `LineBreakInRun` | `LineBreakInRun` | `InvalidXmlChar` |
//! | `ParagraphMut::set_text`, `TextBodyMut::add_paragraph` | `a:br` | `LineBreakInParagraph` | `LineBreakInParagraph` | `InvalidXmlChar` |
//! | `TextBodyMut::set_text` | `a:br` | new paragraph | `\r\n` / `\r` → `\n` | `InvalidXmlChar` |

use ooxml_core::value::is_xml_char;

use crate::error::{Error, Result};

/// Text for one `a:t`: no line breaks of any kind.
pub(crate) fn check_run_text(s: &str) -> Result<()> {
    for (offset, ch) in s.char_indices() {
        match ch {
            '\n' | '\r' | '\u{b}' => return Err(Error::LineBreakInRun),
            c if !is_xml_char(c) => return Err(Error::InvalidXmlChar { ch: c, offset }),
            _ => {}
        }
    }
    Ok(())
}

/// Text for one paragraph: split on `\v` (each `\v` becomes an `a:br`).
pub(crate) fn paragraph_segments(s: &str) -> Result<Vec<&str>> {
    for (offset, ch) in s.char_indices() {
        match ch {
            '\n' | '\r' => return Err(Error::LineBreakInParagraph),
            '\u{b}' => {}
            c if !is_xml_char(c) => return Err(Error::InvalidXmlChar { ch: c, offset }),
            _ => {}
        }
    }
    Ok(s.split('\u{b}').collect())
}

/// Text for a whole body: `\r\n` and `\r` become `\n`, then one entry per
/// paragraph, each split into `\v` segments.
pub(crate) fn body_lines(s: &str) -> Result<Vec<Vec<String>>> {
    // Report offsets in the caller's string, before normalisation.
    for (offset, ch) in s.char_indices() {
        if !matches!(ch, '\n' | '\r' | '\u{b}') && !is_xml_char(ch) {
            return Err(Error::InvalidXmlChar { ch, offset });
        }
    }
    let norm = s.replace("\r\n", "\n").replace('\r', "\n");
    norm.split('\n')
        .map(|line| Ok(paragraph_segments(line)?.into_iter().map(String::from).collect()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    #[test]
    fn rules() {
        assert!(check_run_text("a\tb").is_ok());
        for s in ["a\nb", "a\rb", "a\u{b}b"] {
            assert_matches!(check_run_text(s), Err(Error::LineBreakInRun));
        }
        assert_matches!(check_run_text("a\u{1}"), Err(Error::InvalidXmlChar { ch: '\u{1}', offset: 1 }));
        assert_eq!(paragraph_segments("a\u{b}b\u{b}").unwrap(), ["a", "b", ""]);
        assert_matches!(paragraph_segments("a\nb"), Err(Error::LineBreakInParagraph));
        assert_matches!(paragraph_segments("a\rb"), Err(Error::LineBreakInParagraph));
        assert_matches!(paragraph_segments("\u{FFFF}"), Err(Error::InvalidXmlChar { .. }));
        assert_eq!(body_lines("a\r\nb\rc\nd\u{b}e").unwrap(), [vec!["a"], vec!["b"], vec!["c"], vec!["d", "e"]]);
        assert_eq!(body_lines("").unwrap(), [vec![""]]);
        assert_matches!(body_lines("ok\n\u{0}"), Err(Error::InvalidXmlChar { offset: 3, .. }));
    }
}
