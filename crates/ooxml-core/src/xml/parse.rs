//! quick-xml based parser that records element spans.

use std::collections::BTreeMap;
use std::sync::Arc;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use super::{ElementSpan, ParseOptions, MarkupKind, RawAttr, RawItem, RawMarkup, RawNode, RawText, XmlDocument};
use crate::error::{Error, Result};
use crate::name::{XML_NS, XMLNS_NS, XName};

fn xml_err(position: usize, message: impl Into<String>) -> Error {
    Error::Xml { position, message: message.into() }
}

/// Namespace bindings in scope, innermost last.
struct Scope {
    bindings: Vec<(Option<Box<str>>, Arc<str>)>,
    interned: BTreeMap<Box<str>, Arc<str>>,
    empty: Arc<str>,
}

impl Scope {
    fn new() -> Self {
        let empty: Arc<str> = Arc::from("");
        let mut s = Scope { bindings: Vec::new(), interned: BTreeMap::new(), empty: empty.clone() };
        s.interned.insert("".into(), empty);
        s
    }
    fn intern(&mut self, uri: &str) -> Arc<str> {
        if let Some(a) = self.interned.get(uri) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(uri);
        self.interned.insert(uri.into(), a.clone());
        a
    }
    fn lookup(&self, prefix: Option<&str>) -> Option<Arc<str>> {
        self.bindings.iter().rev().find(|(p, _)| p.as_deref() == prefix).map(|(_, u)| u.clone())
    }
}

/// Splits `p:l` into (`Some(p)`, `l`).
fn split_qname(raw: &str) -> (Option<&str>, &str) {
    match raw.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, raw),
    }
}

pub(super) fn parse(bytes: &[u8], options: &ParseOptions) -> Result<XmlDocument> {
    if bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(&[0xFF, 0xFE]) {
        return Err(Error::UnsupportedEncoding("UTF-16 (byte order mark)".into()));
    }
    let source: Box<str> = std::str::from_utf8(bytes)
        .map_err(|e| Error::UnsupportedEncoding(format!("not valid UTF-8: {e}")))?
        .into();
    let root = Parser::new(&source, options.max_depth).run()?;
    Ok(XmlDocument { source, root, max_depth: options.max_depth, token: super::DocToken::new() })
}

struct Parser<'s> {
    src: &'s str,
    reader: Reader<&'s [u8]>,
    /// Length of a UTF-8 BOM the reader does not see (positions are offset by it).
    base: usize,
    scope: Scope,
    /// Open elements with the scope length to restore on close.
    stack: Vec<(RawNode, usize)>,
    root: Option<RawNode>,
    /// Maximum element depth (the root is depth 1).
    max_depth: usize,
}

impl<'s> Parser<'s> {
    fn new(src: &'s str, max_depth: usize) -> Self {
        // The BOM stays in the verbatim prolog; the reader starts after it.
        let base = if src.starts_with('\u{feff}') { 3 } else { 0 };
        let mut reader = Reader::from_str(&src[base..]);
        let cfg = reader.config_mut();
        cfg.trim_text(false);
        cfg.expand_empty_elements = false;
        cfg.check_end_names = true;
        Parser { src, reader, base, scope: Scope::new(), stack: Vec::new(), root: None, max_depth }
    }

    fn run(mut self) -> Result<RawNode> {
        loop {
            let before = self.base + self.reader.buffer_position() as usize;
            let event = self
                .reader
                .read_event()
                .map_err(|e| xml_err(self.base + self.reader.error_position() as usize, e.to_string()))?;
            let after = self.base + self.reader.buffer_position() as usize;
            match event {
                Event::Eof => break,
                Event::Decl(d) => {
                    if let Some(enc) = d.encoding() {
                        let enc = enc.map_err(|e| xml_err(before, e.to_string()))?;
                        if !enc.eq_ignore_ascii_case("utf-8") && !enc.eq_ignore_ascii_case("utf8") {
                            return Err(Error::UnsupportedEncoding(enc.into_owned()));
                        }
                    }
                }
                Event::Start(e) => {
                    let node = self.open(&e, before, after, false)?;
                    let mark = self.scope_len_before_push(&node);
                    self.stack.push((node, mark));
                }
                Event::Empty(e) => {
                    let node = self.open(&e, before, after, true)?;
                    let mark = self.scope_len_before_push(&node);
                    self.scope.bindings.truncate(mark);
                    self.attach(RawItem::Element(node), before)?;
                }
                Event::End(_) => {
                    let (mut node, mark) =
                        self.stack.pop().ok_or_else(|| xml_err(before, "unmatched close tag"))?;
                    self.scope.bindings.truncate(mark);
                    if let Some(span) = &mut node.span {
                        span.close = Some((before, after));
                    }
                    self.attach(RawItem::Element(node), before)?;
                }
                Event::Text(t) => {
                    let range = self.src.substr_range(&t).map_or(before..after, |r| r.start..r.end);
                    self.push_text(t.xml_content(XmlVersion::Implicit1_0).as_ref(), range);
                }
                Event::GeneralRef(r) => {
                    let ch = match r.resolve_char_ref().map_err(|e| xml_err(before, e.to_string()))? {
                        Some(c) => c,
                        None => match &*r {
                            "lt" => '<',
                            "gt" => '>',
                            "amp" => '&',
                            "apos" => '\'',
                            "quot" => '"',
                            other => return Err(xml_err(before, format!("undeclared entity &{other};"))),
                        },
                    };
                    self.push_text(ch.encode_utf8(&mut [0; 4]), before..after);
                }
                Event::CData(c) => {
                    if let Some((parent, _)) = self.stack.last_mut() {
                        parent.children.push(RawItem::Text(RawText {
                            value: c.xml_content(XmlVersion::Implicit1_0).into_owned(),
                            span: Some(before..after),
                            cdata: true,
                        }));
                    }
                }
                Event::Comment(_) => self.push_markup(MarkupKind::Comment, before, after),
                Event::PI(_) => self.push_markup(MarkupKind::ProcessingInstruction, before, after),
                // OOXML never uses DTDs; refusing them outright rules out
                // entity-expansion and external-entity (XXE) tricks.
                Event::DocType(_) => return Err(Error::DtdNotAllowed { position: before }),
            }
        }
        if !self.stack.is_empty() {
            return Err(xml_err(self.src.len(), "unclosed element at end of input"));
        }
        self.root.ok_or_else(|| xml_err(0, "no root element"))
    }

    /// Scope length before this node's own declarations were pushed.
    fn scope_len_before_push(&self, node: &RawNode) -> usize {
        self.scope.bindings.len() - node.attrs.iter().filter(|a| a.name.is_namespace_declaration()).count()
    }

    fn push_text(&mut self, value: &str, range: std::ops::Range<usize>) {
        let Some((parent, _)) = self.stack.last_mut() else {
            return; // outside the root: part of the verbatim prolog/epilog
        };
        if let Some(RawItem::Text(prev)) = parent.children.last_mut()
            && !prev.cdata
            && let Some(span) = &mut prev.span
            && span.end == range.start
        {
            prev.value.push_str(value);
            span.end = range.end;
            return;
        }
        parent.children.push(RawItem::Text(RawText { value: value.into(), span: Some(range), cdata: false }));
    }

    fn push_markup(&mut self, kind: MarkupKind, before: usize, after: usize) {
        if let Some((parent, _)) = self.stack.last_mut() {
            parent.children.push(RawItem::Markup(RawMarkup { kind, raw: self.src[before..after].into() }));
        }
    }

    fn attach(&mut self, item: RawItem, at: usize) -> Result<()> {
        match self.stack.last_mut() {
            Some((parent, _)) => parent.children.push(item),
            None => {
                if self.root.is_some() {
                    return Err(xml_err(at, "more than one root element"));
                }
                let RawItem::Element(n) = item else { unreachable!() };
                self.root = Some(n);
            }
        }
        Ok(())
    }

    /// Builds a node from a start tag, pushing its namespace declarations
    /// onto the scope.
    fn open(&mut self, e: &BytesStart<'_>, before: usize, after: usize, empty: bool) -> Result<RawNode> {
        // Checked before anything is allocated for the element: the depth
        // of the new element is the number of open ancestors plus one.
        if self.stack.len() >= self.max_depth {
            return Err(Error::TooDeep { limit: self.max_depth, position: before });
        }
        if self.root.is_some() && self.stack.is_empty() {
            return Err(xml_err(before, "more than one root element"));
        }
        // `<` precedes the borrowed tag content; locate it via substr_range.
        let start = self.src.substr_range(e).map_or(before, |r| r.start - 1);
        debug_assert_eq!(start, before);
        let xmlns: Arc<str> = self.scope.intern(XMLNS_NS);
        let mut raw_attrs: Vec<(Option<&str>, &str, String)> = Vec::new();
        for a in e.attributes().with_checks(true) {
            let a = a.map_err(|err| xml_err(before, err.to_string()))?;
            let value = a.normalized_value(XmlVersion::Implicit1_0).map_err(|err| xml_err(before, err.to_string()))?;
            let (prefix, local) = split_qname(a.key.into_inner());
            raw_attrs.push((prefix, local, value.into_owned()));
        }
        // Declarations first, so they apply to this element's own names.
        for (prefix, local, value) in &raw_attrs {
            match (prefix, *local) {
                (None, "xmlns") => {
                    let uri = self.scope.intern(value);
                    self.scope.bindings.push((None, uri));
                }
                (Some("xmlns"), p) => {
                    let uri = self.scope.intern(value);
                    self.scope.bindings.push((Some(p.into()), uri));
                }
                _ => {}
            }
        }
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        for (prefix, local, value) in raw_attrs {
            let ns = match (prefix, local) {
                (None, "xmlns") | (Some("xmlns"), _) => xmlns.clone(),
                (None, _) => self.scope.empty.clone(),
                (Some("xml"), _) => self.scope.intern(XML_NS),
                (Some(p), _) => self
                    .scope
                    .lookup(Some(p))
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| xml_err(before, format!("unbound prefix {p:?}")))?,
            };
            attrs.push(RawAttr { name: XName::new(ns, local, prefix), value });
        }
        let (prefix, local) = split_qname(e.name().into_inner());
        let ns = match prefix {
            None => self.scope.lookup(None).unwrap_or_else(|| self.scope.empty.clone()),
            Some("xml") => self.scope.intern(XML_NS),
            Some(p) => self
                .scope
                .lookup(Some(p))
                .filter(|u| !u.is_empty())
                .ok_or_else(|| xml_err(before, format!("unbound prefix {p:?}")))?,
        };
        Ok(RawNode {
            name: XName::new(ns, local, prefix),
            attrs,
            children: Vec::new(),
            span: Some(ElementSpan { start, open_end: after, close: if empty { None } else { Some((after, after)) } }),
            own_dirty: false,
            subtree_dirty: false,
        })
    }
}
