//! quick-xml based parser that records element spans.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event, attributes::Attributes};
use quick_xml::reader::Reader;

use super::interner::Interner;
use super::node::{Bindings, Deferred, MAX_SPAN_OFFSET};
use super::{ElementSpan, MarkupKind, ParseOptions, RawAttr, RawItem, RawMarkup, RawNode, RawText, SStr, XmlDocument};
use crate::error::{Error, Result, WellFormedness};
use crate::name::{MC_ALTERNATE_CONTENT, QName, XML_NS, XMLNS_NS};

fn xml_err(position: usize, message: impl Into<String>) -> Error {
    Error::Xml {
        position,
        message: message.into(),
    }
}

fn not_wf(position: usize, reason: WellFormedness) -> Error {
    core::hint::cold_path();
    Error::NotWellFormed { position, reason }
}

/// Namespace bindings in scope, innermost last.
struct Scope<'s> {
    bindings: Vec<(Option<&'s str>, Arc<str>)>,
    interned: BTreeMap<Box<str>, Arc<str>>,
    empty: Arc<str>,
    xml: Arc<str>,
    xmlns: Arc<str>,
}

impl<'s> Scope<'s> {
    fn new() -> Self {
        let empty: Arc<str> = Arc::from("");
        let xml = Arc::<str>::from(XML_NS);
        let xmlns = Arc::<str>::from(XMLNS_NS);
        let interned = BTreeMap::from([
            ("".into(), empty.clone()),
            (XML_NS.into(), xml.clone()),
            (XMLNS_NS.into(), xmlns.clone()),
        ]);
        Self {
            bindings: Vec::new(),
            interned,
            empty,
            xml,
            xmlns,
        }
    }
    fn intern(&mut self, uri: &str) -> Arc<str> {
        if let Some(a) = self.interned.get(uri) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(uri);
        self.interned.insert(uri.into(), a.clone());
        a
    }
    fn snapshot(&self, src: &str, len: usize) -> Bindings {
        self.bindings[..len].iter().map(|(prefix, uri)| {
            let prefix = prefix.map(|prefix| {
                let span = src.substr_range(prefix).expect("prefix borrows the source");
                span.start..span.end
            });
            (prefix, uri.clone())
        }).collect()
    }

    fn lookup(&self, prefix: Option<&str>) -> Option<&Arc<str>> {
        self.bindings.iter().rev().find(|(p, _)| *p == prefix).map(|(_, u)| u)
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
    let (source, bom) = checked_source(bytes)?;
    parse_source(Arc::from(source), bom, options)
}

pub(super) fn parse_inflated(mut bytes: Vec<u8>, options: &ParseOptions) -> Result<XmlDocument> {
    // Keep the inflater's bounded, incremental allocation policy. Reduce
    // the overlap with the shared source, and free the buffer before building
    // any nodes. A ZIP header alone still cannot drive a large allocation.
    bytes.shrink_to_fit();
    let (source, bom) = checked_source(&bytes)?;
    let source = Arc::from(source);
    drop(bytes);
    parse_source(source, bom, options)
}

fn checked_source(bytes: &[u8]) -> Result<(&str, usize)> {
    if bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(&[0xFF, 0xFE]) {
        return Err(Error::UnsupportedEncoding("UTF-16 (byte order mark)".into()));
    }
    let source = std::str::from_utf8(bytes)
        .map_err(|e| Error::UnsupportedEncoding(format!("not valid UTF-8: {e}")))?;
    // Only a BOM at byte 0 is a BOM. quick-xml silently skips one at the
    // start of what it is given without counting it in its positions, so a
    // second one would shift every span by 3 bytes (fuzz finding F-1).
    let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
    if source[bom..].starts_with('\u{feff}') {
        return Err(not_wf(bom, WellFormedness::ByteOrderMark));
    }
    if let Some(p) = source.find('\0') {
        return Err(not_wf(p, WellFormedness::NullCharacter));
    }
    check_span_limit(source.len())?;
    Ok((source, bom))
}

fn parse_source(source: Arc<str>, bom: usize, options: &ParseOptions) -> Result<XmlDocument> {
    let mut parser = Parser::new(&source, bom..source.len(), *options);
    let root = if options.defer.is_empty() && options.count.is_empty() {
        parser.run::<false>()?
    } else {
        parser.run::<true>()?
    };
    let census = std::mem::take(&mut parser.census);
    drop(parser);
    Ok(XmlDocument {
        source,
        root,
        census,
        max_depth: options.max_depth,
        token: super::DocToken::new(),
    })
}

pub(super) fn expand(deferred: &Deferred, span: ElementSpan) -> Vec<RawItem> {
    let options = deferred.options.with_count(&[]);
    let mut parser = Parser::new(&deferred.source, span.start..span.end(), options);
    parser.ancestor_depth = deferred.depth as usize - 1;
    parser.expanding = true;
    parser.scope.bindings.extend(deferred.bindings.iter().map(|(prefix, uri)| {
        (prefix.as_ref().map(|range| &deferred.source[range.clone()]), uri.clone())
    }));
    parser.run::<true>().expect("deferred content was validated").into_parsed_children()
}

fn check_span_limit(len: usize) -> Result<()> {
    if len > MAX_SPAN_OFFSET as usize {
        return Err(Error::XmlPartTooLarge { len });
    }
    Ok(())
}

struct Parser<'s> {
    src: &'s Arc<str>,
    reader: Reader<&'s [u8]>,
    /// Absolute offset of the reader input in the shared source.
    base: usize,
    scope: Scope<'s>,
    names: Interner<'s>,
    /// Open elements with the scope length to restore on close.
    stack: Vec<(RawNode, usize)>,
    root: Option<RawNode>,
    // Keep growing buffers by depth, and transfer only their occupied
    // entries into completed nodes.
    children: Vec<Vec<RawItem>>,
    merged_text: Option<(String, Range<usize>)>,
    attrs: Vec<(Option<&'s str>, &'s str, Cow<'s, str>)>,
    options: ParseOptions,
    census: Box<[(QName, usize)]>,
    /// Namespace marks for skipped descendants. The owning deferred node
    /// remains at the top of `stack` until it closes.
    skipped: Vec<usize>,
    skipping: bool,
    /// The unchanged ancestor scope can be reused for consecutive siblings.
    snapshot: Option<Bindings>,
    ancestor_depth: usize,
    expanding: bool,
}

impl<'s> Parser<'s> {
    fn new(src: &'s Arc<str>, range: Range<usize>, options: ParseOptions) -> Self {
        let base = range.start;
        let mut reader = Reader::from_str(&src[range]);
        let cfg = reader.config_mut();
        cfg.trim_text(false);
        cfg.expand_empty_elements = false;
        cfg.check_end_names = true;
        Parser {
            src,
            reader,
            base,
            scope: Scope::new(),
            names: Interner::default(),
            stack: Vec::new(),
            root: None,
            children: Vec::new(),
            merged_text: None,
            attrs: Vec::new(),
            options,
            census: options.count.iter().map(|&name| (name, 0)).collect(),
            skipped: Vec::new(),
            skipping: false,
            snapshot: None,
            ancestor_depth: 0,
            expanding: false,
        }
    }

    fn run<const OPT: bool>(&mut self) -> Result<RawNode> {
        loop {
            let before = self.base + self.reader.buffer_position() as usize;
            let event = self
                .reader
                .read_event()
                .map_err(|e| xml_err(self.base + self.reader.error_position() as usize, e.to_string()))?;
            let after = self.base + self.reader.buffer_position() as usize;
            if !matches!(event, Event::Text(_) | Event::GeneralRef(_)) {
                self.flush_text();
            }
            match event {
                Event::Eof => break,
                Event::Decl(d) => {
                    // Only at the very start (after a BOM). quick-xml reports
                    // `<?xml …?>` anywhere; inside the root it would be
                    // dropped from the tree (fuzz finding F-4).
                    if before != self.base {
                        return Err(not_wf(before, WellFormedness::XmlDeclarationNotAtStart));
                    }
                    if let Some(enc) = d.encoding() {
                        let enc = enc.map_err(|e| xml_err(before, e.to_string()))?;
                        if !enc.eq_ignore_ascii_case("utf-8") && !enc.eq_ignore_ascii_case("utf8") {
                            return Err(Error::UnsupportedEncoding(enc.into_owned()));
                        }
                    }
                }
                Event::Start(e) => {
                    let mark = self.scope.bindings.len();
                    if let Some(node) = self.open::<OPT>(&e, before, after)? {
                        if OPT {
                            self.skipping = node.is_deferred();
                        }
                        self.stack.push((node, mark));
                        if self.children.len() < self.stack.len() {
                            self.children.push(Vec::new());
                        }
                    } else {
                        self.skipped.push(mark);
                    }
                }
                Event::Empty(e) => {
                    let mark = self.scope.bindings.len();
                    let node = self.open::<OPT>(&e, before, after)?;
                    self.restore_scope::<OPT>(mark);
                    if let Some(node) = node {
                        self.attach(RawItem::Element(node), before)?;
                    }
                }
                Event::End(_) => {
                    if !self.src.get(before..after).is_some_and(|t| t.starts_with("</")) {
                        return Err(not_wf(before, WellFormedness::PositionMismatch));
                    }
                    if OPT && let Some(mark) = self.skipped.pop() {
                        self.restore_scope::<OPT>(mark);
                        continue;
                    }
                    let (mut node, mark) = self.stack.pop().ok_or_else(|| xml_err(before, "unmatched close tag"))?;
                    self.restore_scope::<OPT>(mark);
                    node.close(before, after);
                    if OPT && self.skipping {
                        self.skipping = false;
                    } else {
                        let scratch = &mut self.children[self.stack.len()];
                        let mut children = Vec::with_capacity(scratch.len());
                        children.append(scratch);
                        node.finish_parsed_children(children);
                    }
                    self.attach(RawItem::Element(node), before)?;
                }
                Event::Text(t) => {
                    if OPT && self.skipping {
                        continue;
                    }
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
                    if !OPT || !self.skipping {
                        self.push_text(ch.encode_utf8(&mut [0; 4]), before..after);
                    }
                }
                Event::CData(c) => {
                    if (!OPT || !self.skipping) && !self.stack.is_empty() {
                        let index = self.stack.len() - 1;
                        let value = SStr::from_decoded(self.src, &c.xml_content(XmlVersion::Implicit1_0));
                        self.children[index].push(RawItem::Text(RawText::parsed(value, before..after, true)));
                    }
                }
                Event::Comment(_) if !OPT || !self.skipping => self.push_markup(MarkupKind::Comment, before, after),
                Event::PI(_) if !OPT || !self.skipping => self.push_markup(MarkupKind::ProcessingInstruction, before, after),
                Event::Comment(_) | Event::PI(_) => {}
                // OOXML never uses DTDs; refusing them outright rules out
                // entity-expansion and external-entity (XXE) tricks.
                Event::DocType(_) => return Err(Error::DtdNotAllowed { position: before }),
            }
        }
        if !self.stack.is_empty() {
            return Err(xml_err(self.src.len(), "unclosed element at end of input"));
        }
        self.root.take().ok_or_else(|| xml_err(0, "no root element"))
    }

    fn restore_scope<const OPT: bool>(&mut self, mark: usize) {
        if OPT && self.scope.bindings.len() != mark {
            self.snapshot = None;
        }
        self.scope.bindings.truncate(mark);
    }

    fn current_children(&mut self) -> Option<&mut Vec<RawItem>> {
        self.stack.len().checked_sub(1).map(|depth| &mut self.children[depth])
    }

    fn flush_text(&mut self) {
        if let Some((value, range)) = self.merged_text.take() {
            self.current_children()
                .expect("merged text is inside an element")
                .push(RawItem::Text(RawText::parsed(value.as_str().into(), range, false)));
        }
    }

    fn push_text(&mut self, value: &str, range: Range<usize>) {
        if self.stack.is_empty() {
            return; // outside the root: part of the verbatim prolog/epilog
        }
        if let Some((text, span)) = &mut self.merged_text
            && span.end == range.start
        {
            text.push_str(value);
            span.end = range.end;
            return;
        }
        self.flush_text();
        let source = self.src;
        let children = self.current_children().expect("inside an element");
        if let Some(RawItem::Text(prev)) = children.last()
            && !prev.cdata
            && let Some(span) = prev.span()
            && span.end == range.start
        {
            let mut text = prev.value().to_owned();
            text.push_str(value);
            let start = span.start;
            children.pop();
            self.merged_text = Some((text, start..range.end));
            return;
        }
        children.push(RawItem::Text(RawText::parsed(
            SStr::from_decoded(source, value),
            range,
            false,
        )));
    }

    fn push_markup(&mut self, kind: MarkupKind, before: usize, after: usize) {
        let source = self.src;
        if let Some(children) = self.current_children() {
            children.push(RawItem::Markup(RawMarkup {
                kind,
                raw: SStr::span(source, before, after),
            }));
        }
    }

    fn attach(&mut self, item: RawItem, at: usize) -> Result<()> {
        match self.current_children() {
            Some(children) => children.push(item),
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
    fn open<const OPT: bool>(&mut self, e: &BytesStart<'s>, before: usize, after: usize) -> Result<Option<RawNode>> {
        // Checked before anything is allocated for the element: the depth
        // of the new element is the number of open ancestors plus one.
        let ancestors = self.stack.len() + if OPT { self.ancestor_depth + self.skipped.len() } else { 0 };
        if ancestors >= self.options.max_depth {
            return Err(Error::TooDeep {
                limit: self.options.max_depth,
                position: before,
            });
        }
        if self.root.is_some() && self.stack.is_empty() {
            return Err(xml_err(before, "more than one root element"));
        }
        // `<` precedes the borrowed tag content; locate it via substr_range.
        // The two must agree, or every span recorded from the reader's
        // positions would be wrong (fuzz finding F-1); refuse instead.
        let start = self.src.substr_range(e).map_or(before, |r| r.start - 1);
        if start != before || !self.src.get(before..after).is_some_and(|t| t.starts_with('<')) {
            return Err(not_wf(before, WellFormedness::PositionMismatch));
        }
        let name_len = e.name().into_inner().len();
        let tag = &self.src[start + 1..start + 1 + e.len()];
        for a in Attributes::new(tag, name_len).with_checks(true) {
            let a = a.map_err(|err| xml_err(before, err.to_string()))?;
            let value = a
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|err| xml_err(before, err.to_string()))?;
            let (prefix, local) = split_qname(a.key.into_inner());
            if local.is_empty() || prefix == Some("") {
                return Err(not_wf(before, WellFormedness::EmptyName));
            }
            if prefix == Some("xmlns") && value.is_empty() {
                return Err(not_wf(before, WellFormedness::EmptyPrefixBinding));
            }
            self.attrs.push((prefix, local, value));
        }
        // Declarations first, so they apply to this element's own names.
        let marks = self.scope.bindings.len();
        for (prefix, local, value) in &self.attrs {
            match (prefix, *local) {
                (None, "xmlns") => {
                    let uri = self.scope.intern(value);
                    self.scope.bindings.push((None, uri));
                }
                (Some("xmlns"), p) => {
                    let uri = self.scope.intern(value);
                    self.scope.bindings.push((Some(p), uri));
                }
                _ => {}
            }
        }
        let building = !OPT || !self.skipping;
        let mut attrs = if building { Vec::with_capacity(self.attrs.len()) } else { Vec::new() };
        for (prefix, local, value) in self.attrs.drain(..) {
            let ns = match (prefix, local) {
                (None, "xmlns") | (Some("xmlns"), _) => &self.scope.xmlns,
                (None, _) => &self.scope.empty,
                (Some("xml"), _) => &self.scope.xml,
                (Some(p), _) => self
                    .scope
                    .lookup(Some(p))
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| xml_err(before, format!("unbound prefix {p:?}")))?,
            };
            if building {
                attrs.push(RawAttr {
                    name: self.names.get(ns, local, prefix),
                    value: SStr::from_decoded(self.src, &value),
                });
            }
        }
        let (prefix, local) = split_qname(&self.src[start + 1..start + 1 + name_len]);
        if local.is_empty() || prefix == Some("") {
            return Err(not_wf(before, WellFormedness::EmptyName));
        }
        let ns = match prefix {
            None => self.scope.lookup(None).unwrap_or(&self.scope.empty),
            Some("xml") => &self.scope.xml,
            Some(p) => self
                .scope
                .lookup(Some(p))
                .filter(|u| !u.is_empty())
                .ok_or_else(|| xml_err(before, format!("unbound prefix {p:?}")))?,
        };
        let matches = |q: QName| q.ns == ns.as_ref() && q.local == local;
        if OPT {
            for (requested, count) in &mut self.census {
                if matches(*requested) {
                    *count += 1;
                }
            }
            if !building {
                if self.skipped.is_empty() && matches(MC_ALTERNATE_CONTENT) {
                    self.stack.last_mut().expect("skipping a deferred element").0.mark_alternate_content();
                }
                if self.scope.bindings.len() != marks {
                    self.snapshot = None;
                }
                return Ok(None);
            }
        }
        let defer = OPT && !(self.expanding && self.stack.is_empty()) && self.options.defer.iter().copied().any(matches);
        let mut node = RawNode::parsed(self.names.get(ns, local, prefix), attrs.into_boxed_slice(), start, after);
        if OPT {
            // Capture the ancestor scope, before this element's own declarations.
            if defer {
                let bindings = self.snapshot.get_or_insert_with(|| self.scope.snapshot(self.src, marks)).clone();
                node.defer(Deferred::new(self.src, self.options, ancestors + 1, bindings));
            }
            if self.scope.bindings.len() != marks {
                self.snapshot = None;
            }
        }
        Ok(Some(node))
    }
}

#[cfg(test)]
#[test]
fn compact_span_limit_is_checked_before_narrowing() {
    let limit = 2_147_483_647_usize;
    assert_eq!(limit, MAX_SPAN_OFFSET as usize);
    assert!(check_span_limit(limit).is_ok());
    std::assert_matches!(check_span_limit(limit + 1), Err(Error::XmlPartTooLarge { len }) if len == limit + 1);
}
