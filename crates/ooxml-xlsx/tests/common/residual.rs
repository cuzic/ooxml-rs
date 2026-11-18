//! ADR-0009 oracle 4a, the residual-mention oracle: a scan of **every**
//! output part (XML text and attribute values decoded; other parts as
//! UTF-8 and UTF-16LE text) for a sheet name in sheet-prefix form. Written
//! independently of the implementation's location table: it knows only the
//! places that are never references.

use ooxml_core::Package;
use quick_xml::{Reader, XmlVersion};
use quick_xml::events::Event;

/// One scanned value: part, the element path (local names) and attribute
/// (`None` for text), and the decoded value.
#[derive(Debug, Clone)]
pub struct Value {
    pub part: String,
    pub path: Vec<String>,
    pub attr: Option<String>,
    pub text: String,
}

impl Value {
    fn el(&self, k: usize) -> &str {
        self.path.len().checked_sub(k + 1).map_or("", |i| self.path[i].as_str())
    }

    /// A place that is never a sheet reference (ADR-0009 §6 "known
    /// non-reference"): document properties, shared strings, comments,
    /// another workbook (external links), cell values and inline strings.
    pub fn non_reference(&self) -> bool {
        let p = self.part.to_ascii_lowercase();
        p.starts_with("docprops/")
            || p.contains("sharedstrings")
            || p.contains("comments")
            || p.contains("/persons/")
            || p.contains("externallinks/")
            || (self.attr.is_none() && self.el(0) == "v" && self.el(1) == "c")
            || (self.attr.is_none() && self.el(0) == "t" && (self.el(1) == "is" || self.el(2) == "is"))
    }

    /// `hyperlink/@location` or a `#…` relationship target: a delete leaves
    /// these (ADR-0009 §5).
    pub fn location(&self) -> bool {
        matches!(self.attr.as_deref(), Some("location")) && self.el(0) == "hyperlink"
            || matches!(self.attr.as_deref(), Some("Target")) && self.text.trim_start().starts_with('#')
    }
}

/// Every value of every part of the package.
pub fn values(bytes: &[u8]) -> Vec<Value> {
    let pkg = Package::from_bytes(bytes.to_vec()).unwrap();
    let mut out = Vec::new();
    for part in pkg.part_names().map(str::to_owned).collect::<Vec<_>>() {
        let b = pkg.read_part(&part).unwrap().to_vec();
        if !xml_values(&part, &b, &mut out) {
            let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            for text in [String::from_utf8_lossy(&b).into_owned(), String::from_utf16_lossy(&units)] {
                out.push(Value { part: part.clone(), path: vec![], attr: None, text });
            }
        }
    }
    out
}

/// Pushes the values of an XML part; `false` when it is not XML.
fn xml_values(part: &str, b: &[u8], out: &mut Vec<Value>) -> bool {
    let mut r = Reader::from_reader(b);
    let mut path: Vec<String> = Vec::new();
    let mut vals = Vec::new();
    let mut text = String::new();
    let local = |n: &str| n.rsplit(':').next().unwrap_or_default().to_owned();
    loop {
        match r.read_event() {
            Err(_) => return false,
            Ok(Event::Eof) => break,
            Ok(ev @ (Event::Start(_) | Event::Empty(_))) => {
                let (Event::Start(e) | Event::Empty(e)) = &ev else { unreachable!() };
                if !text.is_empty() {
                    vals.push(Value { part: part.into(), path: path.clone(), attr: None, text: std::mem::take(&mut text) });
                }
                path.push(local(e.name().as_ref()));
                for a in e.attributes().flatten() {
                    let Ok(v) = a.normalized_value(XmlVersion::Implicit1_0) else { return false };
                    vals.push(Value { part: part.into(), path: path.clone(), attr: Some(local(a.key.as_ref())), text: v.into_owned() });
                }
                if matches!(ev, Event::Empty(_)) {
                    path.pop();
                }
            }
            Ok(Event::End(_)) => {
                if !text.is_empty() {
                    vals.push(Value { part: part.into(), path: path.clone(), attr: None, text: std::mem::take(&mut text) });
                }
                path.pop();
            }
            Ok(Event::Text(t)) => text.push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Ok(Event::CData(t)) => text.push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Ok(Event::GeneralRef(g)) => match g.resolve_char_ref() {
                Ok(Some(c)) => text.push(c),
                _ => text.push_str(match &*g {
                    "lt" => "<",
                    "gt" => ">",
                    "amp" => "&",
                    "apos" => "'",
                    "quot" => "\"",
                    _ => "",
                }),
            },
            Ok(_) => {}
        }
    }
    out.extend(vals);
    true
}

/// Removes formula string literals (`"…"`, `""` escaped): a sheet name
/// inside one is not a reference (ADR-0009 §6 "Not rewritten").
fn without_literals(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut in_lit = false;
    for c in t.chars() {
        if c == '"' {
            in_lit = !in_lit; // `""` toggles twice
            continue;
        }
        if !in_lit {
            out.push(c);
        }
    }
    out
}

/// Sheet-prefix mentions of `sheet`: `Name!`, `'Name'!`, or a 3-D endpoint
/// (`Name:`, `:Name!`); not after `[n]` (another workbook).
pub fn mentions(text: &str, sheet: &str) -> bool {
    let t = without_literals(text).to_lowercase();
    let n = sheet.to_lowercase();
    let q = n.replace('\'', "''");
    [format!("{n}!"), format!("'{q}'!"), format!("{n}:"), format!(":{n}!"), format!("'{q}:"), format!(":{q}'!")].iter().any(|p| {
        t.match_indices(p.as_str()).any(|(i, _)| {
            let before = t[..i].chars().next_back();
            if p.starts_with(['\'', ':']) {
                return true;
            }
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ']' || c == '\'')
        })
    })
}

/// Values (outside the never-reference places) that mention `sheet`.
pub fn residual<'a>(vals: &'a [Value], sheet: &str) -> Vec<&'a Value> {
    vals.iter().filter(|v| !v.non_reference() && mentions(&v.text, sheet)).collect()
}
