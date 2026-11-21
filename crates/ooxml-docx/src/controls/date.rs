use chrono::{NaiveDateTime, Timelike};

use crate::{Error, Result};

pub(super) fn format_in_calendar(
    date: NaiveDateTime,
    picture: &str,
    language: &str,
    calendar: &str,
) -> Result<String> {
    if !matches!(
        calendar,
        "gregorian"
            | "gregorianArabic"
            | "gregorianMeFrench"
            | "gregorianUs"
            | "gregorianXlitEnglish"
            | "gregorianXlitFrench"
    ) {
        return Err(unsupported(calendar));
    }
    format(date, picture, language)
}

pub(crate) fn format(date: NaiveDateTime, picture: &str, language: &str) -> Result<String> {
    let mut remaining = picture;
    let mut display = String::new();
    while let Some(first) = remaining.chars().next() {
        let start = &remaining[first.len_utf8()..];
        if matches!(first, '\'' | '"') {
            let (literal, rest) = start
                .split_once(first)
                .ok_or_else(|| unsupported(remaining))?;
            display.push_str(literal);
            remaining = rest;
        } else if first == '\\' {
            let literal = start.chars().next().ok_or_else(|| unsupported(remaining))?;
            display.push(literal);
            remaining = &start[literal.len_utf8()..];
        } else if let Some(rest) = remaining.strip_prefix("AM/PM") {
            display.push_str(if date.hour() < 12 { "AM" } else { "PM" });
            remaining = rest;
        } else if first.is_ascii_alphabetic() {
            let end = start
                .find(|c| c != first)
                .map_or(remaining.len(), |i| i + first.len_utf8());
            let token = &remaining[..end];
            let english = language.eq_ignore_ascii_case("en")
                || language
                    .get(..3)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("en-"));
            let strftime = match token {
                "y" => "%-y",
                "yy" => "%y",
                "yyyy" => "%Y",
                "M" => "%-m",
                "MM" => "%m",
                "MMM" if english => "%b",
                "MMMM" if english => "%B",
                "d" => "%-d",
                "dd" => "%d",
                "H" => "%-H",
                "HH" => "%H",
                "h" => "%-I",
                "hh" => "%I",
                "m" => "%-M",
                "mm" => "%M",
                "s" => "%-S",
                "ss" => "%S",
                _ => return Err(unsupported(token)),
            };
            display.push_str(&date.format(strftime).to_string());
            remaining = &remaining[end..];
        } else {
            display.push(first);
            remaining = start;
        }
    }
    Ok(display)
}

fn unsupported(token: &str) -> Error {
    Error::UnsupportedDateFormat {
        token: token.into(),
    }
}

pub(super) fn mapped(date: NaiveDateTime, mapping: &str, display: &str) -> Result<String> {
    match mapping {
        "dateTime" => Ok(date.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        "date" => Ok(date.format("%Y-%m-%d").to_string()),
        "text" => Ok(display.into()),
        _ => Err(Error::UnsupportedBinding),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_tokens_and_mapping() {
        let date = "2004-03-02T17:06:07".parse().unwrap();
        assert_eq!(
            format(
                date,
                "y yy yyyy M MM MMM MMMM d dd H HH h hh m mm s ss AM/PM 'at' \"月\" \\e",
                "en-US"
            )
            .unwrap(),
            "4 04 2004 3 03 Mar March 2 02 17 17 5 05 6 06 7 07 PM at 月 e",
        );
        assert!(matches!(
            format(date, "ddd", "en"),
            Err(Error::UnsupportedDateFormat { .. })
        ));
        assert!(matches!(
            format(date, "MMMM", "ja-JP"),
            Err(Error::UnsupportedDateFormat { .. })
        ));
        assert_eq!(
            mapped(date, "dateTime", "x").unwrap(),
            "2004-03-02T17:06:07Z"
        );
        assert_eq!(mapped(date, "date", "x").unwrap(), "2004-03-02");
        assert_eq!(mapped(date, "text", "x").unwrap(), "x");
    }
}
