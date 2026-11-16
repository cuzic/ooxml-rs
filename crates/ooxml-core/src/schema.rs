//! Schema description types that generated code (`ooxml-schema`) fills in
//! (ADR-0002 §3, §4.1).
//!
//! Everything here is constant data plus `const fn` checks. Behaviour lives
//! in [`crate::lens`].

use std::fmt;

use crate::name::QName;

/// Simple (attribute / leaf text) types, after the SDK's .NET value types.
///
/// Typed reads never coerce: a value outside the lexical space is returned
/// as an [`AttrError`] carrying the raw string (ADR-0002 §1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SimpleType {
    /// Any string (`StringValue`).
    String,
    /// `xsd:boolean` (`true`, `false`, `1`, `0`).
    Boolean,
    /// WordprocessingML on/off (`xsd:boolean` plus `on`, `off`).
    OnOff,
    /// VML `t`/`f`/`true`/`false`.
    TrueFalse,
    /// VML `t`/`f`/`true`/`false` or the empty string.
    TrueFalseBlank,
    /// `xsd:unsignedByte`.
    UInt8,
    /// `xsd:unsignedShort`.
    UInt16,
    /// `xsd:unsignedInt`.
    UInt32,
    /// `xsd:unsignedLong`.
    UInt64,
    /// `xsd:byte`.
    Int8,
    /// `xsd:short`.
    Int16,
    /// `xsd:int`.
    Int32,
    /// `xsd:long`.
    Int64,
    /// `xsd:integer` (read as `i64`).
    Integer,
    /// `xsd:double`.
    Double,
    /// `xsd:float`.
    Single,
    /// `xsd:decimal` (kept as text).
    Decimal,
    /// `xsd:dateTime` (kept as text).
    DateTime,
    /// `xsd:hexBinary` (kept as text).
    HexBinary,
    /// `xsd:base64Binary` (kept as text).
    Base64Binary,
    /// Whitespace-separated list (kept as text).
    List,
    /// Enumeration: the known lexical values, in SDK order.
    Enum(&'static [&'static str]),
}

/// One attribute of an element.
#[derive(Clone, Copy, Debug)]
pub struct AttrSpec {
    /// Attribute name (`ns = ""` for unqualified attributes).
    pub name: QName,
    /// Simple type.
    pub ty: SimpleType,
    /// Default from the ECMA-376 Transitional XSDs or an override
    /// (ADR-0002 §1.2); `None` if there is none.
    pub default: Option<&'static str>,
}

/// An `xsd:any` wildcard's namespace constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wildcard {
    /// `##any` (also used when the SDK data gives no namespace).
    Any,
    /// `##other`: any namespace except the given target namespace (and except
    /// no-namespace).
    Other(&'static str),
    /// An explicit list of namespace URIs (`""` stands for `##local`).
    List(&'static [&'static str]),
}

impl Wildcard {
    /// Does the wildcard admit an element in namespace `ns`?
    pub fn matches(&self, ns: &str) -> bool {
        match self {
            Wildcard::Any => true,
            Wildcard::Other(tns) => !ns.is_empty() && ns != *tns,
            Wildcard::List(l) => l.contains(&ns),
        }
    }
}

/// One schema rank in a lowered content model (ADR-0002 §4.1).
///
/// Children whose names are in the same slot may appear in any order
/// relative to each other; slots themselves are ordered.
#[derive(Clone, Copy, Debug)]
pub struct Slot {
    /// Element names in this slot.
    pub names: &'static [QName],
    /// Wildcard (`xsd:any`) admitted in this slot, if any.
    pub wildcard: Option<Wildcard>,
    /// Limit per QName (`None` = unbounded). For a slot whose names have
    /// different limits, this is the largest of them.
    pub max_each: Option<u32>,
    /// Limit for the whole slot (`None` = unbounded).
    pub max_total: Option<u32>,
}

impl Slot {
    /// Is `q` listed by name in this slot?
    pub fn contains(&self, q: QName) -> bool {
        self.names.contains(&q)
    }
}

/// A lowered content model.
#[derive(Clone, Copy, Debug)]
pub enum Content {
    /// No children, no text.
    Empty,
    /// Leaf text of the given simple type.
    Text(SimpleType),
    /// Ordered slots.
    Slots(&'static [Slot]),
    /// The particle could not be lowered to slots (ADR-0002 §4.1): typed
    /// reads work, insertion returns
    /// [`Error::UnsupportedContentModel`](crate::Error::UnsupportedContentModel).
    /// Carries every element name the particle allows.
    Unsupported(&'static [QName]),
}

impl Content {
    /// The slot list, if the model was lowered.
    pub fn slots(&self) -> Option<&'static [Slot]> {
        match self {
            Content::Slots(s) => Some(s),
            _ => None,
        }
    }
}

/// Schema knowledge for one generation unit: an SDK `Name`, i.e. a
/// (complex type, element) pair such as `x:CT_Cell/x:c` (ADR-0002 §3).
///
/// Implemented by uninhabited marker types in `ooxml-schema`.
pub trait ElementSpec: 'static {
    /// The element's name.
    const QNAME: QName;
    /// The SDK `Name` this marker was generated from (e.g. `x:CT_Cell/x:c`).
    const SDK_NAME: &'static str;
    /// Attributes, merged from abstract base entries.
    const ATTRS: &'static [AttrSpec];
    /// Content model.
    const CONTENT: Content;
    /// The SDK base class was not resolvable, so `ATTRS` may be incomplete;
    /// attribute writes return [`Error::IncompleteSpec`](crate::Error::IncompleteSpec).
    const ATTRS_INCOMPLETE: bool = false;

    /// Looks up an attribute spec by name.
    fn attr_spec(q: QName) -> Option<&'static AttrSpec> {
        Self::ATTRS.iter().find(|a| a.name == q)
    }
}

/// `Self` may be a child of `P` (generated only when both are allowlisted).
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an allowed child of `{P}`",
    note = "allowed children are listed in `{P}::CONTENT`"
)]
pub trait ChildOf<P: ElementSpec>: ElementSpec {}

/// A generated enumeration of an attribute's known values.
pub trait EnumType: Copy + Eq + fmt::Debug + 'static {
    /// Lexical values, indexed by [`Self::index`].
    const VALUES: &'static [&'static str];
    /// Variant for index `i` into [`Self::VALUES`]; `None` if out of range.
    fn from_index(i: usize) -> Option<Self>;
    /// Index into [`Self::VALUES`].
    fn index(self) -> usize;
    /// Lexical form.
    fn as_str(self) -> &'static str {
        Self::VALUES[self.index()]
    }
    /// Parses a lexical form (surrounding XML whitespace ignored).
    fn parse(s: &str) -> Option<Self> {
        let s = crate::value::trim_xml_ws(s);
        Self::VALUES.iter().position(|v| *v == s).and_then(Self::from_index)
    }
}

/// A typed enum read: never lossy (ADR-0002 Resolved question 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumValue<'a, E: EnumType> {
    /// A value the pinned schema data knows.
    Known(E),
    /// Any other value, verbatim.
    Unknown(&'a str),
}

impl<'a, E: EnumType> EnumValue<'a, E> {
    /// Classifies a raw value.
    pub fn from_raw(raw: &'a str) -> Self {
        match E::parse(raw) {
            Some(e) => EnumValue::Known(e),
            None => EnumValue::Unknown(raw),
        }
    }
    /// The lexical form.
    pub fn as_str(&self) -> &'a str {
        match self {
            EnumValue::Known(e) => e.as_str(),
            EnumValue::Unknown(s) => s,
        }
    }
    /// The known variant, if any.
    pub fn known(self) -> Option<E> {
        match self {
            EnumValue::Known(e) => Some(e),
            EnumValue::Unknown(_) => None,
        }
    }
}

/// A typed attribute read found a value outside the type's lexical space.
/// The raw value is kept, never coerced or dropped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("attribute {attr} has value {raw:?}, which is not valid for its type")]
pub struct AttrError {
    /// The attribute.
    pub attr: QName,
    /// The raw (unescaped) value.
    pub raw: String,
}

// ---- const helpers used by generated module-level `const _` checks ----

/// `const` string equality (byte loop; `&str == &str` is not `const`).
pub const fn const_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn name_in(list: &[QName], q: QName) -> bool {
    let mut i = 0;
    while i < list.len() {
        if list[i].const_eq(q) {
            return true;
        }
        i += 1;
    }
    false
}

/// Does `content` list `q` (in any slot, or in the `Unsupported` name list)?
pub const fn content_contains(content: Content, q: QName) -> bool {
    match content {
        Content::Slots(slots) => {
            let mut i = 0;
            while i < slots.len() {
                if name_in(slots[i].names, q) {
                    return true;
                }
                i += 1;
            }
            false
        }
        Content::Unsupported(names) => name_in(names, q),
        Content::Empty | Content::Text(_) => false,
    }
}

/// Like [`content_contains`], but also true when a wildcard slot admits
/// `q`'s namespace (children of a wildcard parent such as `a:graphicData`,
/// ADR-0002 post-acceptance note "wildcard parents").
pub const fn content_admits(content: Content, q: QName) -> bool {
    if content_contains(content, q) {
        return true;
    }
    let Content::Slots(slots) = content else {
        return false;
    };
    let mut i = 0;
    while i < slots.len() {
        if let Some(w) = slots[i].wildcard
            && wildcard_admits(w, q.ns)
        {
            return true;
        }
        i += 1;
    }
    false
}

/// `const` form of [`Wildcard::matches`].
pub const fn wildcard_admits(w: Wildcard, ns: &str) -> bool {
    match w {
        Wildcard::Any => true,
        Wildcard::Other(tns) => !ns.is_empty() && !const_str_eq(ns, tns),
        Wildcard::List(l) => {
            let mut i = 0;
            while i < l.len() {
                if const_str_eq(l[i], ns) {
                    return true;
                }
                i += 1;
            }
            false
        }
    }
}

/// No QName appears twice within one slot. (A QName in two *different*
/// slots is allowed, ADR-0002 §4.1.)
pub const fn no_duplicate_within_slot(content: Content) -> bool {
    let Content::Slots(slots) = content else {
        return true;
    };
    let mut s = 0;
    while s < slots.len() {
        let names = slots[s].names;
        let mut i = 0;
        while i < names.len() {
            let mut j = i + 1;
            while j < names.len() {
                if names[i].const_eq(names[j]) {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        s += 1;
    }
    true
}

const fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// `const` lexical check of an `xsd:double` (digits, optional sign,
/// fraction, exponent; or `INF`, `-INF`, `NaN`). Only the lexical form is
/// checked, since `f64::from_str` is not `const` (ADR-0002 §3).
pub const fn const_is_double(s: &str) -> bool {
    if const_str_eq(s, "INF") || const_str_eq(s, "-INF") || const_str_eq(s, "+INF") || const_str_eq(s, "NaN") {
        return true;
    }
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut exp = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            exp += 1;
        }
        if exp == 0 {
            return false;
        }
    }
    i == b.len()
}

const fn one_of(s: &str, vals: &[&str]) -> bool {
    let mut i = 0;
    while i < vals.len() {
        if const_str_eq(s, vals[i]) {
            return true;
        }
        i += 1;
    }
    false
}

/// Is `value` in the lexical space of `ty`? Used for defaults at compile
/// time. Text-kept types (`Decimal`, `DateTime`, …) only reject leading or
/// trailing whitespace.
pub const fn value_is_valid(ty: SimpleType, value: &str) -> bool {
    let b = value.as_bytes();
    if !b.is_empty() && (is_ws(b[0]) || is_ws(b[b.len() - 1])) && !matches!(ty, SimpleType::String) {
        return false;
    }
    match ty {
        SimpleType::String | SimpleType::List => true,
        SimpleType::Boolean => one_of(value, &["true", "false", "1", "0"]),
        SimpleType::OnOff => one_of(value, &["true", "false", "1", "0", "on", "off"]),
        SimpleType::TrueFalse => one_of(value, &["t", "f", "true", "false"]),
        SimpleType::TrueFalseBlank => one_of(value, &["t", "f", "true", "false", ""]),
        SimpleType::UInt8 => u8::from_str_radix(value, 10).is_ok(),
        SimpleType::UInt16 => u16::from_str_radix(value, 10).is_ok(),
        SimpleType::UInt32 => u32::from_str_radix(value, 10).is_ok(),
        SimpleType::UInt64 => u64::from_str_radix(value, 10).is_ok(),
        SimpleType::Int8 => i8::from_str_radix(value, 10).is_ok(),
        SimpleType::Int16 => i16::from_str_radix(value, 10).is_ok(),
        SimpleType::Int32 => i32::from_str_radix(value, 10).is_ok(),
        SimpleType::Int64 | SimpleType::Integer => i64::from_str_radix(value, 10).is_ok(),
        SimpleType::Double | SimpleType::Single | SimpleType::Decimal => const_is_double(value),
        SimpleType::DateTime | SimpleType::HexBinary | SimpleType::Base64Binary => true,
        SimpleType::Enum(vals) => one_of(value, vals),
    }
}

/// Every default in `attrs` is valid for its type.
pub const fn defaults_are_valid(attrs: &[AttrSpec]) -> bool {
    let mut i = 0;
    while i < attrs.len() {
        if let Some(d) = attrs[i].default
            && !value_is_valid(attrs[i].ty, d)
        {
            return false;
        }
        i += 1;
    }
    true
}

/// The check every generated spec gets as a module-level
/// `const _: () = assert_spec::<E>();` item (ADR-0002 §3): no QName twice
/// within a slot, every default valid for its type. Fails the build (E0080)
/// otherwise.
pub const fn assert_spec<E: ElementSpec>() {
    assert!(no_duplicate_within_slot(E::CONTENT), "a QName appears twice within one slot");
    assert!(defaults_are_valid(E::ATTRS), "an attribute default is not valid for its type");
}

/// Checked for every generated `impl ChildOf<P> for C`: `C::QNAME` is listed
/// in `P::CONTENT`, or admitted by one of its wildcard slots. Fails the
/// build (E0080) otherwise.
pub const fn assert_child<P: ElementSpec, C: ElementSpec>() {
    assert!(content_admits(P::CONTENT, C::QNAME), "ChildOf impl for a name the parent's content does not admit");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_admission() {
        const OTHER: Content = Content::Slots(&[Slot {
            names: &[QName::new("urn:t", "a")],
            wildcard: Some(Wildcard::Other("urn:t")),
            max_each: None,
            max_total: None,
        }]);
        const { assert!(content_admits(OTHER, QName::new("urn:t", "a"))) };
        const { assert!(content_admits(OTHER, QName::new("urn:x", "b"))) };
        const { assert!(!content_admits(OTHER, QName::new("urn:t", "b"))) };
        const { assert!(!content_admits(OTHER, QName::new("", "b"))) };
        const LIST: Wildcard = Wildcard::List(&["", "urn:y"]);
        const { assert!(wildcard_admits(LIST, "") && wildcard_admits(LIST, "urn:y") && !wildcard_admits(LIST, "urn:z")) };
        for ns in ["", "urn:y", "urn:z"] {
            assert_eq!(wildcard_admits(LIST, ns), LIST.matches(ns));
        }
    }

    #[test]
    fn double_lexical() {
        for ok in ["0", "-1.5", "1e10", ".5", "5.", "INF", "NaN", "+3E-2"] {
            assert!(const_is_double(ok), "{ok}");
        }
        for bad in ["", "e5", "1e", "abc", "1.2.3", "inf"] {
            assert!(!const_is_double(bad), "{bad}");
        }
    }

    #[test]
    fn defaults() {
        const A: &[AttrSpec] = &[AttrSpec {
            name: QName::new("", "t"),
            ty: SimpleType::Enum(&["n", "s"]),
            default: Some("n"),
        }];
        const { assert!(defaults_are_valid(A)) };
        assert!(!value_is_valid(SimpleType::UInt32, "-1"));
        assert!(!value_is_valid(SimpleType::Int32, "100%"));
    }
}
