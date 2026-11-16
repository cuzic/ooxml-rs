//! SpreadsheetML's `_xHHHH_` escapes (ECMA-376 Part 1 §22.4.2.4, `ST_Xstring`).
//!
//! XML 1.0 cannot carry most C0 control characters, so Excel writes them as
//! `_xHHHH_` (UTF-16 code unit in hex). A literal `_xHHHH_` in the text is
//! protected by escaping its underscore as `_x005F_`.

/// Decodes `_xHHHH_` escapes. A high surrogate followed by a low surrogate
/// escape becomes one character; an unpaired surrogate becomes U+FFFD
/// (the same rule as calamine).
pub fn decode(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains("_x") {
        return s.into();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('_') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match unit_at(rest) {
            Some(hi @ 0xD800..=0xDBFF) => {
                rest = &rest[7..];
                match unit_at(rest) {
                    Some(lo @ 0xDC00..=0xDFFF) => {
                        let cp = 0x10000 + ((u32::from(hi) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
                        out.push(char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER));
                        rest = &rest[7..];
                    }
                    _ => out.push(char::REPLACEMENT_CHARACTER),
                }
            }
            Some(u) => {
                out.push(char::from_u32(u32::from(u)).unwrap_or(char::REPLACEMENT_CHARACTER));
                rest = &rest[7..];
            }
            None => {
                out.push('_');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.into()
}

fn unit_at(s: &str) -> Option<u16> {
    let b = s.as_bytes().get(..7)?;
    if b[0] != b'_' || b[1] != b'x' || b[6] != b'_' || !b[2..6].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    u16::from_str_radix(&s[2..6], 16).ok()
}

/// Characters XML 1.0 cannot carry (and U+FFFE/U+FFFF, which Excel escapes).
fn needs_escape(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// Encodes text for a `<t>`: characters XML cannot carry become `_xHHHH_`,
/// and the underscore of a literal `_xHHHH_` becomes `_x005F_`.
/// `decode(&encode(s)) == s` for every `s`.
pub fn encode(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.chars().any(needs_escape) && !s.contains("_x") {
        return s.into();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for (i, c) in s.char_indices() {
        if needs_escape(c) {
            out.push_str(&format!("_x{:04X}_", c as u32));
        } else if c == '_' && unit_at(&s[i..]).is_some() {
            out.push_str("_x005F_");
        } else {
            out.push(c);
        }
    }
    out.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for s in ["", "plain", "a_x000D_b", "_x005F_x0041_", "tab\there", "\u{1}\u{1F}\u{FFFF}", "_x_", "x_x12", "😜"] {
            assert_eq!(decode(&encode(s)), s, "{s:?}");
        }
        assert_eq!(encode("\u{1}"), "_x0001_");
        assert_eq!(encode("_x0041_"), "_x005F_x0041_");
        assert_eq!(decode("a_x000D_b"), "a\rb");
        assert_eq!(decode("_xD83D__xDE1C_"), "😜");
        assert_eq!(decode("_xD83D_x"), "\u{FFFD}x");
        assert_eq!(decode("_x00zz_"), "_x00zz_");
    }
}
