//! Resolves SDK entries: base-class inheritance, attribute types and XSD
//! defaults, overrides, and the lowered content model (ADR-0002 §1, §3, §4).

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{Override, SlotOverride};
use crate::lower::{self, LSlot, Lim};
use crate::sdk::{Particle, Sdk, SdkAttr, SdkEnum, SdkType};
use crate::xsd::{XML_NS, Xsd};

/// .NET framework base classes (not schema entries).
const FRAMEWORK_BASES: &[&str] =
    &["OpenXmlCompositeElement", "OpenXmlLeafElement", "OpenXmlLeafTextElement", "OpenXmlPartRootElement"];

/// Simple type, by `ooxml_core::SimpleType` variant name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ty {
    Simple(&'static str),
    /// (schema file index, enum index)
    Enum(usize, usize),
}

#[derive(Clone, Debug)]
pub struct Attr {
    /// (namespace URI, local name)
    pub ns: String,
    pub local: String,
    /// Prefix as written in the SDK data (`""` for none).
    pub prefix: String,
    pub ty: Ty,
    pub default: Option<String>,
    pub property: Option<String>,
    pub comment: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ContentModel {
    Empty,
    Text(Ty),
    Slots(Vec<LSlot>),
    Unsupported { names: BTreeSet<String>, reason: String },
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub file: usize,
    pub class_name: String,
    pub summary: Option<String>,
    /// Element prefix (`x`) and local name (`c`).
    pub prefix: String,
    pub local: String,
    pub attrs: Vec<Attr>,
    pub attrs_incomplete: bool,
    pub content: ContentModel,
    /// SDK Names of children reachable through the content model.
    pub child_names: BTreeSet<String>,
    /// Child SDK Name → property name (from the SDK `Children` list).
    pub child_props: BTreeMap<String, String>,
}

#[derive(Default, Debug)]
pub struct Diagnostics {
    pub lines: Vec<String>,
}

impl Diagnostics {
    pub fn push(&mut self, s: String) {
        self.lines.push(s);
    }
}

/// The whole input: SDK + XSD + overrides, indexed.
pub struct Model<'a> {
    pub sdk: &'a Sdk,
    pub xsd: &'a Xsd,
    pub overrides: &'a [Override],
    /// SDK Name → (file, type index)
    pub index: BTreeMap<String, (usize, usize)>,
    /// enum Name → [(file, enum index)]
    enums_by_name: BTreeMap<String, Vec<(usize, usize)>>,
    /// enum Type (`x:ST_CellType`) → (file, enum index)
    pub enums_by_type: BTreeMap<String, (usize, usize)>,
    /// .NET namespace → file, learned from unambiguous enum references.
    dotnet_ns_file: BTreeMap<String, usize>,
    /// Extra enum values from overrides, by enum Type.
    pub extra_enum_values: BTreeMap<String, Vec<String>>,
}

pub fn type_part(name: &str) -> &str {
    name.split('/').next().unwrap_or(name)
}

/// The element QName part of an SDK Name (`a:CT_Table/a:tbl` → `a:tbl`).
pub fn element_part(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

impl<'a> Model<'a> {
    pub fn new(sdk: &'a Sdk, xsd: &'a Xsd, overrides: &'a [Override], diags: &mut Diagnostics) -> Self {
        let mut index = BTreeMap::new();
        let mut enums_by_name: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
        let mut enums_by_type = BTreeMap::new();
        for (fi, (fname, f)) in sdk.files.iter().enumerate() {
            for (ti, t) in f.types.iter().enumerate() {
                // Abstract names (`x:CT_Color/`) repeat legitimately: bases are
                // resolved by class name within a file, never through this index.
                if index.insert(t.name.clone(), (fi, ti)).is_some() && !t.name.ends_with('/') {
                    diags.push(format!("DuplicateSdkName {} (in {fname}); the later entry wins", t.name));
                }
            }
            for (ei, e) in f.enums.iter().enumerate() {
                enums_by_name.entry(e.name.clone()).or_default().push((fi, ei));
                enums_by_type.insert(e.ty.clone(), (fi, ei));
            }
        }
        let mut extra_enum_values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for o in overrides {
            if let Override::AddEnumValue { enum_type, value, .. } = o {
                extra_enum_values.entry(enum_type.clone()).or_default().push(value.clone());
            }
        }
        let mut m = Model {
            sdk,
            xsd,
            overrides,
            index,
            enums_by_name,
            enums_by_type,
            dotnet_ns_file: BTreeMap::new(),
            extra_enum_values,
        };
        // Learn .NET namespace → file from enum references that are unambiguous.
        let mut learned: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for (_, f) in &sdk.files {
            for t in &f.types {
                for a in &t.attributes {
                    if let Some((ns, name)) = dotnet_enum(&a.ty)
                        && let Some(c) = m.enums_by_name.get(name)
                        && c.len() == 1
                    {
                        learned.entry(ns.to_owned()).or_default().insert(c[0].0);
                    }
                }
            }
        }
        m.dotnet_ns_file = learned.into_iter().filter(|(_, s)| s.len() == 1).map(|(k, s)| (k, s.into_iter().next().unwrap())).collect();
        m
    }

    pub fn ty(&self, at: (usize, usize)) -> &'a SdkType {
        &self.sdk.files[at.0].1.types[at.1]
    }

    pub fn enum_at(&self, at: (usize, usize)) -> &'a SdkEnum {
        &self.sdk.files[at.0].1.enums[at.1]
    }

    pub fn tns(&self, file: usize) -> &'a str {
        &self.sdk.files[file].1.target_namespace
    }

    pub fn ns_of_prefix(&self, p: &str) -> Option<&'a str> {
        if p == "xml" {
            return Some(XML_NS);
        }
        self.sdk.prefixes.get(p).map(String::as_str)
    }

    /// Resolves an SDK `EnumValue<…>` type to an enum, preferring the same file.
    fn resolve_enum(&self, ty: &str, file: usize, diags: &mut Diagnostics, ctx: &str) -> Option<(usize, usize)> {
        let (ns, name) = dotnet_enum(ty)?;
        let cands = self.enums_by_name.get(name)?;
        if let Some(c) = cands.iter().find(|c| c.0 == file) {
            return Some(*c);
        }
        if cands.len() == 1 {
            return Some(cands[0]);
        }
        if let Some(&f) = self.dotnet_ns_file.get(ns)
            && let Some(c) = cands.iter().find(|c| c.0 == f)
        {
            return Some(*c);
        }
        diags.push(format!("AmbiguousEnum {ctx}: {ty} has {} candidates; typed as String", cands.len()));
        None
    }

    pub fn map_type(&self, ty: &str, file: usize, diags: &mut Diagnostics, ctx: &str) -> Ty {
        let base = ty.split('<').next().unwrap_or(ty);
        Ty::Simple(match base {
            "StringValue" => "String",
            "BooleanValue" => "Boolean",
            "OnOffValue" => "OnOff",
            "TrueFalseValue" => "TrueFalse",
            "TrueFalseBlankValue" => "TrueFalseBlank",
            "ByteValue" => "UInt8",
            "UInt16Value" => "UInt16",
            "UInt32Value" => "UInt32",
            "UInt64Value" => "UInt64",
            "SByteValue" => "Int8",
            "Int16Value" => "Int16",
            "Int32Value" => "Int32",
            "Int64Value" => "Int64",
            "IntegerValue" => "Integer",
            "DoubleValue" => "Double",
            "SingleValue" => "Single",
            "DecimalValue" => "Decimal",
            "DateTimeValue" => "DateTime",
            "HexBinaryValue" => "HexBinary",
            "Base64BinaryValue" => "Base64Binary",
            "ListValue" => "List",
            // ADR-0008 §6: only through a `set_type` override.
            "DmlPercent" => "DmlPercent",
            "EnumValue" => {
                return match self.resolve_enum(ty, file, diags, ctx) {
                    Some((f, e)) => Ty::Enum(f, e),
                    None => Ty::Simple("String"),
                };
            }
            other => {
                diags.push(format!("UnknownSdkType {ctx}: {other}; typed as String"));
                "String"
            }
        })
    }

    /// Enum values including override additions.
    pub fn enum_values(&self, at: (usize, usize)) -> Vec<(String, Option<String>)> {
        let e = self.enum_at(at);
        let mut v: Vec<(String, Option<String>)> = e.facets.iter().map(|f| (f.value.clone(), f.name.clone())).collect();
        if let Some(extra) = self.extra_enum_values.get(&e.ty) {
            for x in extra {
                if !v.iter().any(|(val, _)| val == x) {
                    v.push((x.clone(), None));
                }
            }
        }
        v
    }

    /// Lexical check of a default against the SDK type (ADR-0002 §1.2).
    pub fn default_fits(&self, ty: &Ty, d: &str) -> bool {
        let int = |s: &str, neg: bool| {
            let s = s.strip_prefix(if neg { "-" } else { "+" }).unwrap_or(s);
            !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
        };
        match ty {
            Ty::Enum(f, e) => self.enum_values((*f, *e)).iter().any(|(v, _)| v == d),
            Ty::Simple(s) => match *s {
                "UInt8" => d.parse::<u8>().is_ok(),
                "UInt16" => d.parse::<u16>().is_ok(),
                "UInt32" => d.parse::<u32>().is_ok(),
                "UInt64" => d.parse::<u64>().is_ok(),
                "Int8" => d.parse::<i8>().is_ok(),
                "Int16" => d.parse::<i16>().is_ok(),
                "Int32" => d.parse::<i32>().is_ok(),
                "Int64" | "Integer" => d.parse::<i64>().is_ok() && (int(d, true) || int(d, false)),
                "Boolean" => matches!(d, "true" | "false" | "0" | "1"),
                "OnOff" => matches!(d, "true" | "false" | "0" | "1" | "on" | "off"),
                "TrueFalse" => matches!(d, "t" | "f" | "true" | "false"),
                "TrueFalseBlank" => matches!(d, "t" | "f" | "true" | "false" | ""),
                "DmlPercent" => {
                    d.parse::<i32>().is_ok()
                        || d.strip_suffix('%').is_some_and(|n| {
                            let n = n.strip_prefix('-').unwrap_or(n);
                            let (i, f) = n.split_once('.').unwrap_or((n, "0"));
                            !i.is_empty() && !f.is_empty() && i.bytes().chain(f.bytes()).all(|b| b.is_ascii_digit())
                        })
                }
                "Double" | "Single" | "Decimal" => {
                    matches!(d, "INF" | "-INF" | "NaN") || (d.parse::<f64>().is_ok() && !d.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E'))
                }
                _ => true,
            },
        }
    }

    /// The abstract base entry of `t` in the same schema file, if any.
    fn base_of(&self, file: usize, t: &SdkType) -> Result<Option<(usize, usize)>, ()> {
        if FRAMEWORK_BASES.contains(&t.base_class.as_str()) {
            return Ok(None);
        }
        let f = &self.sdk.files[file].1;
        match f.types.iter().position(|b| b.name.ends_with('/') && b.class_name == t.base_class) {
            Some(i) if f.types[i].name != t.name => Ok(Some((file, i))),
            _ => Err(()),
        }
    }

    /// Base chain, root-most first, ending with `at` itself. `Err` marks an
    /// unresolvable base somewhere in the chain.
    fn chain(&self, at: (usize, usize)) -> (Vec<(usize, usize)>, bool) {
        let mut chain = vec![at];
        let mut incomplete = false;
        let mut cur = at;
        while chain.len() < 16 {
            match self.base_of(cur.0, self.ty(cur)) {
                Ok(Some(b)) => {
                    chain.push(b);
                    cur = b;
                }
                Ok(None) => break,
                Err(()) => {
                    incomplete = true;
                    break;
                }
            }
        }
        chain.reverse();
        (chain, incomplete)
    }

    /// Overrides applying to `name`: inherited ones (keyed on the abstract
    /// `<type part>/` entry, across files) first, then the entry's own, so the
    /// entry's own override takes precedence (ADR-0002 §1.3).
    pub fn overrides_for(&self, name: &str) -> Vec<&'a Override> {
        let inherited = format!("{}/", type_part(name));
        let mut v: Vec<&Override> =
            self.overrides.iter().filter(|o| o.target() == Some(inherited.as_str()) && inherited != name).collect();
        v.extend(self.overrides.iter().filter(|o| o.target() == Some(name)));
        v
    }

    /// `p:l`, `:l` or `l` → (namespace URI, local, prefix).
    pub fn resolve_qname(&self, q: &str) -> Option<(String, String, String)> {
        match q.split_once(':') {
            None => (!q.is_empty()).then(|| (String::new(), q.to_owned(), String::new())),
            Some(("", l)) => (!l.is_empty()).then(|| (String::new(), l.to_owned(), String::new())),
            Some((p, l)) => self.ns_of_prefix(p).map(|u| (u.to_owned(), l.to_owned(), p.to_owned())),
        }
    }

    fn make_attr(&self, a: &SdkAttr, decl_file: usize, decl_name: &str, diags: &mut Diagnostics) -> Option<Attr> {
        let Some((ns, local, prefix)) = self.resolve_qname(&a.qname) else {
            diags.push(format!("UnresolvedAttributePrefix {decl_name} {}", a.qname));
            return None;
        };
        let ctx = format!("{decl_name}@{}", a.qname);
        let ty = self.map_type(&a.ty, decl_file, diags, &ctx);
        let default = self.xsd_default(decl_name, &ns, &local);
        // A `set_type` override on this attribute decides the type; the
        // default is judged against that type when it is applied.
        let retyped = self.overrides_for(decl_name).into_iter().any(|o| match o {
            Override::SetType { attribute, .. } => self.resolve_qname(attribute).is_some_and(|(n, l, _)| n == ns && l == local),
            _ => false,
        });
        let default = match default {
            Some(_) if retyped => None,
            Some(d) if !self.default_fits(&ty, &d) => {
                diags.push(format!("DefaultMismatch {ctx}: XSD default {d:?} does not fit SDK type {}; dropped", a.ty));
                None
            }
            d => d,
        };
        Some(Attr {
            ns,
            local,
            prefix,
            ty,
            default,
            property: a.property_name.clone(),
            comment: a.property_comments.clone(),
        })
    }

    /// The XSD default for attribute (ns, local) of the complex type named
    /// by an SDK Name's type part (join3).
    pub fn xsd_default(&self, sdk_name: &str, attr_ns: &str, attr_local: &str) -> Option<String> {
        let (p, ctn) = type_part(sdk_name).split_once(':')?;
        let ns = self.sdk.prefixes.get(p)?;
        self.xsd
            .defaults
            .get(&(ns.clone(), ctn.to_owned(), (attr_ns.to_owned(), attr_local.to_owned())))
            .map(|d| d.value.clone())
    }

    /// Does a wildcard with SDK `Namespace` `any_ns` (`None` = `##any`),
    /// declared in schema file `file`, admit an element in namespace `ns`?
    pub fn wildcard_admits(&self, any_ns: Option<&str>, file: usize, ns: &str) -> bool {
        let tns = self.tns(file);
        let toks: Vec<&str> = any_ns.map(|n| n.split_whitespace().collect()).unwrap_or_default();
        match toks.as_slice() {
            [] | ["##any"] => true,
            ["##other"] => !ns.is_empty() && ns != tns,
            list => list.iter().any(|t| match *t {
                "##local" => ns.is_empty(),
                "##targetNamespace" => ns == tns,
                u => u == ns,
            }),
        }
    }

    /// Fully resolves one (non-abstract or abstract) entry.
    pub fn resolve(&self, name: &str, diags: &mut Diagnostics) -> Option<Entry> {
        let &at = self.index.get(name)?;
        let t = self.ty(at);
        let (chain, incomplete) = self.chain(at);
        if incomplete {
            diags.push(format!("UnresolvedBase {name}: base class {} is not in its schema file; attrs_incomplete", t.base_class));
        }
        let mut attrs: Vec<Attr> = Vec::new();
        let mut particle: Option<Particle> = None;
        let mut leaf_text = false;
        let mut validators = Vec::new();
        let mut child_props = BTreeMap::new();
        let mut sdk_children: Vec<String> = Vec::new();
        for &c in &chain {
            let ct = self.ty(c);
            for a in &ct.attributes {
                if let Some(attr) = self.make_attr(a, c.0, &ct.name, diags) {
                    match attrs.iter_mut().find(|x| x.ns == attr.ns && x.local == attr.local) {
                        Some(x) => *x = attr,
                        None => attrs.push(attr),
                    }
                }
            }
            if ct.particle.is_some() {
                particle.clone_from(&ct.particle);
            }
            leaf_text |= ct.is_leaf_text;
            if !ct.validators.is_empty() {
                validators.clone_from(&ct.validators);
            }
            for ch in &ct.children {
                sdk_children.push(ch.name.clone());
                if let Some(p) = &ch.property_name {
                    child_props.insert(ch.name.clone(), p.clone());
                }
            }
        }
        let (prefix, local) = match name.rsplit('/').next().and_then(|e| e.split_once(':')) {
            Some((p, l)) => (p.to_owned(), l.to_owned()),
            None => (String::new(), String::new()),
        };
        // ---- overrides ----
        let mut set_content: Option<&Vec<SlotOverride>> = None;
        for o in self.overrides_for(name) {
            match o {
                Override::AddAttribute { name: q, ty, default, .. } => {
                    let Some((ns, l, p)) = self.resolve_qname(q) else {
                        diags.push(format!("OverrideError {name}: add_attribute {q}: unknown prefix"));
                        continue;
                    };
                    let ty = self.map_type(ty, at.0, diags, name);
                    if let Some(x) = attrs.iter_mut().find(|x| x.ns == ns && x.local == l) {
                        x.ty = ty;
                        x.default.clone_from(default);
                    } else {
                        attrs.push(Attr { ns, local: l, prefix: p, ty, default: default.clone(), property: None, comment: None });
                    }
                }
                Override::SetDefault { attribute, value, .. } => match self.find_attr(&mut attrs, attribute) {
                    Some(x) => x.default = Some(value.clone()),
                    None => diags.push(format!("OverrideError {name}: set_default on unknown attribute {attribute}")),
                },
                Override::SetType { attribute, ty, .. } => {
                    let new_ty = self.map_type(ty, at.0, diags, name);
                    let rejoin = |x: &Attr| self.xsd_default(name, &x.ns, &x.local);
                    match self.find_attr(&mut attrs, attribute) {
                        Some(x) => {
                            let d = rejoin(x);
                            x.default = match d {
                                Some(d) if self.default_fits(&new_ty, &d) => Some(d),
                                Some(d) => {
                                    diags.push(format!("DefaultMismatch {name}@{attribute}: XSD default {d:?} does not fit set_type {ty}; dropped"));
                                    None
                                }
                                None => None,
                            };
                            x.ty = new_ty;
                        }
                        None => diags.push(format!("OverrideError {name}: set_type on unknown attribute {attribute}")),
                    }
                }
                Override::AddChild { name: child, after, before, max, .. } => {
                    let Some(p) = particle.as_mut() else {
                        diags.push(format!("OverrideError {name}: add_child on an entry without a particle"));
                        continue;
                    };
                    let anchor = after.as_ref().or(before.as_ref()).unwrap();
                    match p.items.iter().position(|i| i.name.as_deref() == Some(anchor.as_str())) {
                        Some(pos) => {
                            let occurs = match max {
                                Some(0) => vec![crate::sdk::Occurs { min: Some(0), max: None }],
                                m => vec![crate::sdk::Occurs { min: Some(0), max: Some(m.unwrap_or(1)) }],
                            };
                            let el = Particle { kind: None, name: Some(child.clone()), occurs: Some(occurs), ..Particle::default() };
                            p.items.insert(if after.is_some() { pos + 1 } else { pos }, el);
                        }
                        None => diags.push(format!("OverrideError {name}: add_child anchor {anchor} not found")),
                    }
                }
                Override::MceWrapped { children, .. } => {
                    if let Some(p) = particle.as_mut() {
                        remove_elements(p, children);
                    }
                }
                Override::SetContent { slots, .. } => set_content = Some(slots),
                Override::AddEnumValue { .. } => {}
            }
        }
        // ---- content ----
        let mut child_names = BTreeSet::new();
        if let Some(p) = &particle {
            lower::all_sdk_names(p, &mut child_names);
        }
        let content = if let Some(slots) = set_content {
            child_names.extend(slots.iter().flat_map(|s| s.names.iter().cloned()));
            ContentModel::Slots(slots.iter().map(slot_from_override).collect())
        } else if let Some(p) = &particle {
            match lower::lower(p, false) {
                Ok(sl) => match lower::ecma_disagreement(name, &sl, self.xsd, &self.sdk.prefixes) {
                    Some((a, b)) => {
                        diags.push(format!("EcmaOrder {name}: SDK slots allow an order of {a}/{b} that the ECMA XSD forbids; unsupported"));
                        ContentModel::Unsupported { names: names_of(p), reason: format!("ECMA order disagreement ({a}, {b})") }
                    }
                    None => ContentModel::Slots(sl),
                },
                Err(e) => ContentModel::Unsupported { names: names_of(p), reason: e.text().into() },
            }
        } else if leaf_text {
            let ty = validators
                .iter()
                .find_map(|v| match (v.name.as_str(), v.ty.as_deref()) {
                    ("EnumValidator", Some(t)) => self.enums_by_type.get(t).map(|&(f, e)| Ty::Enum(f, e)),
                    ("NumberValidator", Some(t)) => xsd_simple(t).map(Ty::Simple),
                    _ => None,
                })
                .unwrap_or(Ty::Simple("String"));
            ContentModel::Text(ty)
        } else {
            ContentModel::Empty
        };
        // Children admitted by a wildcard slot (e.g. `a:graphicData`, whose
        // content is `xsd:any`): the SDK `Children` list names the elements
        // the SDK knows to occur there; those whose namespace the wildcard
        // admits are children too, so `ChildOf` is generated for the
        // allowlisted ones (ADR-0002 post-acceptance note "wildcard parents").
        if let ContentModel::Slots(sl) = &content {
            let wild: Vec<&LSlot> = sl.iter().filter(|s| s.names.iter().any(|(n, _)| n == lower::ANY)).collect();
            for c in &sdk_children {
                let Some((ns, _, _)) = self.resolve_qname(element_part(c)) else { continue };
                if wild.iter().any(|s| self.wildcard_admits(s.any_ns.as_deref(), at.0, &ns)) {
                    child_names.insert(c.clone());
                }
            }
        }
        Some(Entry {
            name: name.to_owned(),
            file: at.0,
            class_name: t.class_name.clone(),
            summary: t.summary.clone(),
            prefix,
            local,
            attrs,
            attrs_incomplete: incomplete,
            content,
            child_names,
            child_props,
        })
    }

    fn find_attr<'x>(&self, attrs: &'x mut [Attr], q: &str) -> Option<&'x mut Attr> {
        let (ns, l, _) = self.resolve_qname(q)?;
        attrs.iter_mut().find(|x| x.ns == ns && x.local == l)
    }
}

fn names_of(p: &Particle) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    lower::all_names(p, &mut s);
    s
}

fn remove_elements(p: &mut Particle, names: &[String]) {
    p.items.retain(|i| !(i.kind.as_deref().unwrap_or("Element") == "Element" && i.name.as_ref().is_some_and(|n| names.contains(n))));
    for i in &mut p.items {
        remove_elements(i, names);
    }
}

fn slot_from_override(s: &SlotOverride) -> LSlot {
    let lim = |v: Option<u32>| -> Lim { v.filter(|&n| n != 0) };
    LSlot {
        names: s.names.iter().map(|n| (n.rsplit('/').next().unwrap_or(n).to_owned(), lim(s.max_each))).collect(),
        total: lim(s.max_total),
        any_ns: None,
        sdk_names: s.names.iter().cloned().collect(),
    }
}

/// `EnumValue<A.B.Name>` → (`A.B`, `Name`).
fn dotnet_enum(ty: &str) -> Option<(&str, &str)> {
    let inner = ty.strip_prefix("EnumValue<")?.strip_suffix('>')?;
    inner.rsplit_once('.')
}

fn xsd_simple(t: &str) -> Option<&'static str> {
    Some(match t {
        "xsd:integer" => "Integer",
        "xsd:int" => "Int32",
        "xsd:boolean" => "Boolean",
        "xsd:unsignedInt" => "UInt32",
        "xsd:base64Binary" => "Base64Binary",
        "xsd:double" => "Double",
        "xsd:unsignedByte" => "UInt8",
        "xsd:nonNegativeInteger" => "UInt64",
        "xsd:dateTime" => "DateTime",
        "xsd:byte" => "Int8",
        "xsd:short" => "Int16",
        "xsd:long" => "Int64",
        "xsd:unsignedShort" => "UInt16",
        "xsd:unsignedLong" => "UInt64",
        "xsd:float" => "Single",
        "xsd:decimal" => "Decimal",
        _ => return None,
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_overrides;

    fn load() -> (Sdk, Xsd) {
        let root = crate::root();
        (Sdk::load(&root.join(crate::SDK_DIR)).unwrap(), Xsd::load(&root.join(crate::XSD_DIR)).unwrap())
    }

    fn default_of(e: &Entry, local: &str) -> Option<String> {
        e.attrs.iter().find(|a| a.local == local).and_then(|a| a.default.clone())
    }

    /// ADR-0002 §1.3: an override on an abstract entry is inherited by every
    /// entry with the same complex-type part; the entry's own override wins.
    #[test]
    fn override_inheritance_and_precedence() {
        let (sdk, xsd) = load();
        let ov = parse_overrides(
            r#"
            [[override]]
            op = "set_default"
            on = "w:CT_OnOff/"
            attribute = "w:val"
            value = "true"
            reason = "on/off semantics"
            evidence = "ECMA-376 Part 1 17.17.4"

            [[override]]
            op = "set_default"
            on = "w:CT_OnOff/w:i"
            attribute = "w:val"
            value = "false"
            reason = "precedence test"
            evidence = "test"

            [[override]]
            op = "add_attribute"
            on = "x:CT_Row/x:row"
            name = "xr:uid"
            type = "StringValue"
            reason = "Office writes it"
            evidence = "[MS-XLSX]"

            [[override]]
            op = "add_enum_value"
            enum = "x:ST_CellType"
            value = "future"
            reason = "test"
            evidence = "test"

            [[override]]
            op = "mce_wrapped"
            parent = "x:CT_Worksheet/x:worksheet"
            children = ["x:CT_LegacyDrawing/x:legacyDrawing"]
            reason = "only seen inside mc:AlternateContent"
            evidence = "test"
            "#,
        )
        .unwrap();
        let mut d = Diagnostics::default();
        let m = Model::new(&sdk, &xsd, &ov, &mut d);
        let n = m.index.keys().filter(|k| k.starts_with("w:CT_OnOff/") && !k.ends_with('/')).count();
        assert_eq!(n, 191, "ADR-0002 §1.3 counts 191 w:CT_OnOff entries");
        let b = m.resolve("w:CT_OnOff/w:b", &mut d).unwrap();
        assert_eq!(default_of(&b, "val").as_deref(), Some("true"));
        let i = m.resolve("w:CT_OnOff/w:i", &mut d).unwrap();
        assert_eq!(default_of(&i, "val").as_deref(), Some("false"));
        let row = m.resolve("x:CT_Row/x:row", &mut d).unwrap();
        assert!(row.attrs.iter().any(|a| a.local == "uid" && a.prefix == "xr"));
        let at = m.enums_by_type["x:ST_CellType"];
        assert!(m.enum_values(at).iter().any(|(v, _)| v == "future"));
        let ws = m.resolve("x:CT_Worksheet/x:worksheet", &mut d).unwrap();
        assert!(!ws.child_names.contains("x:CT_LegacyDrawing/x:legacyDrawing"));
        assert!(d.lines.iter().all(|l| !l.starts_with("OverrideError")), "{:?}", d.lines);
    }

    /// A wildcard parent (`a:graphicData`, content `xsd:any`) gets the
    /// SDK-listed children its wildcard admits; named content models are
    /// unchanged by the rule.
    #[test]
    fn wildcard_parent_children() {
        let (sdk, xsd) = load();
        let mut d = Diagnostics::default();
        let m = Model::new(&sdk, &xsd, &[], &mut d);
        let gd = m.resolve("a:CT_GraphicalObjectData/a:graphicData", &mut d).unwrap();
        assert!(gd.child_names.contains("a:CT_Table/a:tbl"));
        let tr = m.resolve("a:CT_TableRow/a:tr", &mut d).unwrap();
        assert!(!tr.child_names.contains("a:CT_Table/a:tbl"));
        assert!(m.wildcard_admits(None, 0, "urn:x"));
        let tns = m.tns(0).to_owned();
        assert!(!m.wildcard_admits(Some("##other"), 0, &tns));
        assert!(m.wildcard_admits(Some("##other"), 0, "urn:x"));
        assert!(m.wildcard_admits(Some("##local urn:y"), 0, ""));
        assert!(!m.wildcard_admits(Some("##local urn:y"), 0, "urn:x"));
    }

    #[test]
    fn phase0_entries_and_inheritance() {
        let (sdk, xsd) = load();
        let mut d = Diagnostics::default();
        let m = Model::new(&sdk, &xsd, &[], &mut d);
        let c = m.resolve("x:CT_Cell/x:c", &mut d).unwrap();
        let attrs: Vec<_> = c.attrs.iter().map(|a| (a.local.as_str(), a.default.as_deref())).collect();
        assert_eq!(
            attrs,
            [("r", None), ("s", Some("0")), ("t", Some("n")), ("cm", Some("0")), ("vm", Some("0")), ("ph", Some("false"))]
        );
        let t = m.resolve("x:CT_Xstring/x:t", &mut d).unwrap();
        assert!(t.attrs.iter().any(|a| a.ns == XML_NS && a.local == "space"), "xml:space from x:CT_Xstring/");
        let sdt = m.resolve("w:CT_SdtBlock/w:sdt", &mut d).unwrap();
        assert!(sdt.attrs_incomplete);
    }
}
