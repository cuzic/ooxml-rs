//! ECMA-376 Transitional XSDs: a minimal DOM plus the two plain lookups
//! ADR-0002 uses — attribute defaults (§1.2, port of `join3.py`) and the
//! XSD particle order for the cross-check (§4.1, port of `slottable.py`'s
//! `xlow`). Not a general XSD processor.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::Res;

pub const XS: &str = "http://www.w3.org/2001/XMLSchema";
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Debug)]
pub struct Node {
    /// (namespace URI, local name)
    pub ns: String,
    pub local: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn is_xs(&self, local: &str) -> bool {
        self.ns == XS && self.local == local
    }
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str)
    }
    /// Pre-order traversal including `self` (Python's `Element.iter()`).
    pub fn iter(&self) -> Vec<&Node> {
        let mut out = vec![self];
        for c in &self.children {
            out.extend(c.iter());
        }
        out
    }
}

pub struct XsdFile {
    pub root: Node,
    /// Every prefix declared anywhere in the file (last declaration wins).
    pub nsmap: BTreeMap<String, String>,
    pub tns: String,
    pub qualified: bool,
}

fn parse_file(name: &str, text: &str) -> Res<XsdFile> {
    let mut reader = Reader::from_str(text);
    let mut nsmap = BTreeMap::new();
    let mut scopes: Vec<Vec<(String, String)>> = Vec::new();
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let resolve = |scopes: &Vec<Vec<(String, String)>>, p: &str| -> String {
        if p == "xml" {
            return XML_NS.into();
        }
        for s in scopes.iter().rev() {
            if let Some((_, u)) = s.iter().rev().find(|(k, _)| k == p) {
                return u.clone();
            }
        }
        String::new()
    };
    loop {
        let ev = reader.read_event().map_err(|e| format!("{name}: {e}"))?;
        let (start, empty) = match &ev {
            Event::Start(e) => (Some(e.clone()), false),
            Event::Empty(e) => (Some(e.clone()), true),
            Event::End(_) => {
                let n = stack.pop().ok_or("unbalanced")?;
                scopes.pop();
                match stack.last_mut() {
                    Some(p) => p.children.push(n),
                    None => root = Some(n),
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let e = start.unwrap();
        let mut decls = Vec::new();
        let mut attrs = BTreeMap::new();
        for a in e.attributes() {
            let a = a.map_err(|e| format!("{name}: {e}"))?;
            let k = a.key.into_inner();
            let v = a.normalized_value(quick_xml::XmlVersion::Implicit1_0)?.into_owned();
            if k == "xmlns" {
                decls.push((String::new(), v.clone()));
                nsmap.insert(String::new(), v);
            } else if let Some(p) = k.strip_prefix("xmlns:") {
                decls.push((p.to_owned(), v.clone()));
                nsmap.insert(p.to_owned(), v);
            } else {
                attrs.insert(k.to_owned(), v);
            }
        }
        scopes.push(decls);
        let qn = e.name().into_inner();
        let (p, l) = qn.split_once(':').unwrap_or(("", qn));
        let node = Node { ns: resolve(&scopes, p), local: l.to_owned(), attrs, children: Vec::new() };
        if empty {
            scopes.pop();
            match stack.last_mut() {
                Some(par) => par.children.push(node),
                None => root = Some(node),
            }
        } else {
            stack.push(node);
        }
    }
    let root = root.ok_or("empty xsd")?;
    let tns = root.attr("targetNamespace").unwrap_or_default().to_owned();
    let qualified = root.attr("attributeFormDefault") == Some("qualified");
    Ok(XsdFile { root, nsmap, tns, qualified })
}

/// A default value found in the XSDs.
#[derive(Clone, Debug)]
pub struct XDefault {
    pub value: String,
}

/// Key of a default: (target namespace, complexType name, (attr ns, attr local)).
pub type DefaultKey = (String, String, (String, String));

pub struct Xsd {
    pub files: Vec<XsdFile>,
    /// (tns, name) → (file index, path to node as child indices of root)
    pub cts: BTreeMap<(String, String), (usize, usize)>,
    pub groups: BTreeMap<(String, String), (usize, usize)>,
    pub ags: BTreeMap<(String, String), (usize, usize)>,
    pub gattrs: BTreeMap<(String, String), Option<String>>,
    pub defaults: BTreeMap<DefaultKey, XDefault>,
}

impl Xsd {
    pub fn load(dir: &Path) -> Res<Self> {
        let mut names: Vec<_> = std::fs::read_dir(dir)?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_, _>>()?;
        names.retain(|n| n.ends_with(".xsd"));
        names.sort();
        let mut files = Vec::new();
        for n in &names {
            let text = std::fs::read_to_string(dir.join(n))?;
            files.push(parse_file(n, &text)?);
        }
        let mut x = Xsd {
            files,
            cts: BTreeMap::new(),
            groups: BTreeMap::new(),
            ags: BTreeMap::new(),
            gattrs: BTreeMap::new(),
            defaults: BTreeMap::new(),
        };
        for (fi, f) in x.files.iter().enumerate() {
            for (ci, c) in f.root.children.iter().enumerate() {
                let Some(name) = c.attr("name") else { continue };
                let key = (f.tns.clone(), name.to_owned());
                if c.is_xs("group") {
                    x.groups.insert(key, (fi, ci));
                } else if c.is_xs("complexType") {
                    x.cts.insert(key, (fi, ci));
                } else if c.is_xs("attributeGroup") {
                    x.ags.insert(key, (fi, ci));
                } else if c.is_xs("attribute") {
                    x.gattrs.insert(key, c.attr("default").map(String::from));
                }
            }
        }
        x.defaults = x.compute_defaults();
        Ok(x)
    }

    pub fn node(&self, at: (usize, usize)) -> &Node {
        &self.files[at.0].root.children[at.1]
    }

    /// Resolves `p:l` with a file's namespace map.
    fn res(&self, q: &str, file: usize) -> (String, String) {
        let f = &self.files[file];
        let (p, l) = match q.rsplit_once(':') {
            Some((p, l)) => (p, l),
            None => ("", q),
        };
        let ns = if p.is_empty() {
            f.nsmap.get("").cloned().unwrap_or_else(|| f.tns.clone())
        } else {
            match f.nsmap.get(p) {
                Some(u) if !u.is_empty() => u.clone(),
                _ if p == "xml" => XML_NS.into(),
                _ => format!("UNBOUND:{p}"),
            }
        };
        (ns, l.to_owned())
    }

    /// `direct()` of `join2/join3.py`: defaults declared under `node`
    /// (all descendants), following attribute-group refs and global attribute refs.
    fn direct(&self, node: &Node, file: usize) -> Vec<((String, String), XDefault)> {
        let f = &self.files[file];
        let mut out: Vec<((String, String), XDefault)> = Vec::new();
        let set = |out: &mut Vec<((String, String), XDefault)>, k: (String, String), v: XDefault| {
            match out.iter_mut().find(|(kk, _)| *kk == k) {
                Some(e) => e.1 = v,
                None => out.push((k, v)),
            }
        };
        for a in node.iter().into_iter().filter(|n| n.is_xs("attribute")) {
            if let Some(name) = a.attr("name") {
                let ns = if f.qualified || a.attr("form") == Some("qualified") { f.tns.clone() } else { String::new() };
                if let Some(d) = a.attr("default") {
                    set(&mut out, (ns, name.into()), XDefault { value: d.into() });
                }
            } else if let Some(r) = a.attr("ref") {
                let key = self.res(r, file);
                if let Some(d) = a.attr("default") {
                    set(&mut out, key, XDefault { value: d.into() });
                } else if let Some(Some(d)) = self.gattrs.get(&key) {
                    set(&mut out, key, XDefault { value: d.clone() });
                }
            }
        }
        for g in node.iter().into_iter().filter(|n| n.is_xs("attributeGroup")) {
            let Some(r) = g.attr("ref") else { continue };
            let k = self.res(r, file);
            let Some(&at) = self.ags.get(&k) else { continue };
            for (kk, v) in self.direct(self.node(at), at.0) {
                if !out.iter().any(|(x, _)| *x == kk) {
                    out.push((kk, v));
                }
            }
        }
        out
    }

    fn compute_defaults(&self) -> BTreeMap<DefaultKey, XDefault> {
        let mut xdef = BTreeMap::new();
        for ((tns, name), &at) in &self.cts {
            let ct = self.node(at);
            let mut d = self.direct(ct, at.0);
            for e in ct.iter() {
                if e.is_xs("extension")
                    && let Some(base) = e.attr("base")
                {
                    let k = self.res(base, at.0);
                    if let Some(&bat) = self.cts.get(&k) {
                        for (kk, v) in self.direct(self.node(bat), bat.0) {
                            if !d.iter().any(|(x, _)| *x == kk) {
                                d.push((kk, v));
                            }
                        }
                    }
                }
            }
            for (kk, v) in d {
                xdef.insert((tns.clone(), name.clone(), kk), v);
            }
        }
        xdef
    }

    pub fn namespaces(&self) -> BTreeSet<&str> {
        self.cts.keys().map(|(t, _)| t.as_str()).collect()
    }

    /// `xlow` of `slottable.py`: ordered slots of local names for an XSD
    /// particle node.
    pub fn xlow(&self, n: &Node, tns: &str) -> Vec<BTreeSet<String>> {
        let name_of = |s: &str| s.rsplit(':').next().unwrap_or(s).to_owned();
        if n.is_xs("element") {
            let nm = n.attr("name").or(n.attr("ref")).unwrap_or_default();
            return vec![BTreeSet::from([name_of(nm)])];
        }
        if n.is_xs("any") {
            return vec![BTreeSet::from(["*ANY*".to_owned()])];
        }
        if n.is_xs("group") {
            let r = name_of(n.attr("ref").unwrap_or_default());
            return match self.groups.get(&(tns.to_owned(), r)) {
                Some(&at) => {
                    let g = self.node(at);
                    let gt = self.files[at.0].tns.clone();
                    g.children.iter().flat_map(|c| self.xlow(c, &gt)).collect()
                }
                None => Vec::new(),
            };
        }
        if n.is_xs("sequence") || n.is_xs("all") {
            let out: Vec<_> = n.children.iter().flat_map(|c| self.xlow(c, tns)).collect();
            if n.is_xs("all") && !out.is_empty() {
                return vec![out.into_iter().flatten().collect()];
            }
            return out;
        }
        if n.is_xs("choice") {
            let s: BTreeSet<String> = n.children.iter().flat_map(|c| self.xlow(c, tns)).flatten().collect();
            return vec![s];
        }
        if n.is_xs("complexContent") || n.is_xs("extension") || n.is_xs("restriction") {
            let mut out = Vec::new();
            if n.is_xs("extension")
                && let Some(base) = n.attr("base")
                && let Some(&at) = self.cts.get(&(tns.to_owned(), name_of(base)))
            {
                let bt = self.files[at.0].tns.clone();
                out.extend(self.node(at).children.iter().flat_map(|c| self.xlow(c, &bt)));
            }
            out.extend(n.children.iter().flat_map(|c| self.xlow(c, tns)));
            return out;
        }
        Vec::new()
    }

    /// XSD slots of complex type (tns, name), if it exists.
    pub fn ct_slots(&self, tns: &str, name: &str) -> Option<Vec<BTreeSet<String>>> {
        let &at = self.cts.get(&(tns.to_owned(), name.to_owned()))?;
        let t = self.files[at.0].tns.clone();
        Some(self.node(at).children.iter().flat_map(|c| self.xlow(c, &t)).collect())
    }
}
