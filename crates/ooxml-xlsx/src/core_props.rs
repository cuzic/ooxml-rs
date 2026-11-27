//! `docProps/core.xml` for workbooks created from the template (ADR-0001
//! R2-m2 / R3-m1: regenerated wholesale and installed with `set_part`).

use std::time::{SystemTime, UNIX_EPOCH};

/// W3CDTF UTC timestamp, e.g. `2026-11-16T09:30:00Z`.
pub(crate) fn w3cdtf(t: SystemTime) -> String {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::from(duration.as_secs()),
        Err(error) => {
            let duration = error.duration();
            -i128::from(duration.as_secs()) - i128::from(duration.subsec_nanos() != 0)
        }
    };
    w3cdtf_seconds(secs)
}

pub(crate) fn w3cdtf_seconds(secs: i128) -> String {
    let (days, rem) = (secs.div_euclid(86_400) as i64, secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

/// A fresh `core.xml`: created and modified set to `t`, no creator.
pub(crate) fn generate(t: SystemTime) -> Vec<u8> {
    let now = w3cdtf(t);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
         <dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created>\
         <dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified>\
         </cp:coreProperties>"
    )
    .into_bytes()
}

const CORE_NS: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";
const TERMS_NS: &str = "http://purl.org/dc/terms/";
const CORE_PART: &str = "docProps/core.xml";
const CORE_TYPE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const EMPTY_CORE: &[u8] = b"<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\"/>";
const CORE_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";

use ooxml_core::{ChildOf, Content, ElementSpec, NsDecl, PartNameRequest, QName, RegistryPlan, SimpleType, Target};
use crate::{Error, Result, Workbook};

enum CoreProperties {}
enum ExtendedProperties {}

const CUSTOM_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const CUSTOM_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
const CUSTOM_TYPE: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const CUSTOM_PART: &str = "docProps/custom.xml";
const VARIANT_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";

const APP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const APP_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const APP_TYPE: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const APP_PART: &str = "docProps/app.xml";
const EMPTY_APP: &[u8] = br#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"/>"#;

macro_rules! text_properties {
    ($($field:ident: $ns:ident, $prefix:literal, $name:literal;)+) => {
        impl ElementSpec for CoreProperties {
            const QNAME: QName = QName::new(CORE_NS, "coreProperties");
            const SDK_NAME: &'static str = "CoreProperties";
            const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
            const CONTENT: Content = Content::Slots(&[ooxml_core::Slot {
                names: &[$($field::QNAME,)+],
                wildcard: None,
                max_each: Some(1),
                max_total: None,
            }]);
        }

        $(
            enum $field {}
            impl ElementSpec for $field {
                const QNAME: QName = QName::new($ns, $name);
                const SDK_NAME: &'static str = $name;
                const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
                const CONTENT: Content = Content::Text(SimpleType::String);
            }
            impl ChildOf<CoreProperties> for $field {}
        )+

        fn set_core_field(doc: &mut ooxml_core::XmlDocument, key: &str, value: &str) -> Result<()> {
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(value) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
            let mut root = doc.lens_mut::<CoreProperties>()?;
            match key {
                $($name => {
                    let mut field = if root.node().elements().any(|node| node.is($field::QNAME)) {
                        root.child_mut::<$field>().expect("property found")
                    } else {
                        root.insert_by_with_ns::<$field>(|_, _| false, &[NsDecl::new($prefix, $ns)])?
                    };
                    if $ns == TERMS_NS {
                        let terms = field.ensure_namespace_prefix(NsDecl::new("dcterms", TERMS_NS))?;
                        field.ensure_namespace_prefix(NsDecl::new("xsi", XSI_NS))?;
                        field.set_attr(QName::new(XSI_NS, "type"), &format!("{terms}:W3CDTF"))?;
                    }
                    field.set_text(value)?;
                })+
                _ => return Err(unsupported_property(key)),
            }
            Ok(())
        }

        impl Workbook {
            /// Reads a supported text property from the OPC core metadata.
            pub fn core_text_property(&mut self, key: &str) -> Result<Option<String>> {
                let tag = match key {
                    $($name => $field::QNAME,)+
                    _ => return Err(unsupported_property(key)),
                };
                let Some(part) = self.core_properties_part() else { return Ok(None) };
                let doc = ooxml_core::XmlDocument::parse(&self.pkg.read_part(&part)?)?;
                let root = doc.lens::<CoreProperties>()?;
                Ok(root.raw().elements().find(|node| node.is(tag)).map(|node| node.text().into_owned()))
            }

            /// Updates a supported text property without rewriting the other metadata.
            pub fn set_core_text_property(&mut self, key: &str, value: &str) -> Result<()> {
                self.check_editable()?;
                let part = self.core_properties_part();
                let source = part.as_deref().map(|part| self.pkg.read_part(part)).transpose()?;
                let mut doc = ooxml_core::XmlDocument::parse(source.as_deref().unwrap_or(EMPTY_CORE))?;
                set_core_field(&mut doc, key, value)?;
                if let Some(part) = part {
                    self.pkg.set_part(&part, doc.serialize().into_bytes())?;
                    Ok(())
                } else {
                    self.add_core_properties(doc)
                }
            }
        }
    };
}

text_properties! {
    Creator: DC_NS, "dc", "creator";
    Title: DC_NS, "dc", "title";
    Subject: DC_NS, "dc", "subject";
    Description: DC_NS, "dc", "description";
    Identifier: DC_NS, "dc", "identifier";
    Language: DC_NS, "dc", "language";
    Keywords: CORE_NS, "cp", "keywords";
    Category: CORE_NS, "cp", "category";
    LastModifiedBy: CORE_NS, "cp", "lastModifiedBy";
    Revision: CORE_NS, "cp", "revision";
    Version: CORE_NS, "cp", "version";
    ContentStatus: CORE_NS, "cp", "contentStatus";
    ContentType: CORE_NS, "cp", "contentType";
    Created: TERMS_NS, "dcterms", "created";
    Modified: TERMS_NS, "dcterms", "modified";
    LastPrinted: CORE_NS, "cp", "lastPrinted";
}

impl ElementSpec for ExtendedProperties {
    const QNAME: QName = QName::new(APP_NS, "Properties");
    const SDK_NAME: &'static str = "ExtendedProperties";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: Content = Content::Slots(&[ooxml_core::Slot {
        names: &[Company::QNAME, Manager::QNAME],
        wildcard: Some(ooxml_core::Wildcard::Any),
        max_each: Some(1),
        max_total: None,
    }]);
}

macro_rules! app_property {
    ($name:ident, $tag:literal) => {
        enum $name {}
        impl ElementSpec for $name {
            const QNAME: QName = QName::new(APP_NS, $tag);
            const SDK_NAME: &'static str = $tag;
            const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
            const CONTENT: Content = Content::Text(SimpleType::String);
        }
        impl ChildOf<ExtendedProperties> for $name {}
    };
}

app_property!(Company, "Company");
app_property!(Manager, "Manager");

impl Workbook {
    /// Reads a company or manager field from the application properties part.
    pub fn app_text_property(&self, key: &str) -> Result<Option<String>> {
        let tag = match key {
            "company" => Company::QNAME, "manager" => Manager::QNAME,
            _ => return Err(unsupported_property(key)),
        };
        let Some(part) = self.property_part(APP_REL) else { return Ok(None) };
        let doc = ooxml_core::XmlDocument::parse(&self.pkg.read_part(&part)?)?;
        let root = doc.lens::<ExtendedProperties>()?;
        Ok(root.raw().elements().find(|node| node.is(tag)).map(|node| node.text().into_owned()))
    }

    /// Updates an application property and preserves the other elements.
    pub fn set_app_text_property(&mut self, key: &str, value: &str) -> Result<()> {
        self.check_editable()?;
        let part = self.property_part(APP_REL);
        let source = part.as_deref().map(|part| self.pkg.read_part(part)).transpose()?;
        let mut doc = ooxml_core::XmlDocument::parse(source.as_deref().unwrap_or(EMPTY_APP))?;
        set_app_field(&mut doc, key, value)?;
        if let Some(part) = part {
            let mut plan = RegistryPlan::new(&self.pkg);
            plan.stage_part(&self.pkg, &part, doc)?;
            self.apply_registry_edit(plan, |_| Ok(()))
        } else {
            self.add_metadata_part(doc, APP_PART, "docProps/app{}.xml", APP_TYPE, APP_REL)
        }
    }
}

fn set_app_field(doc: &mut ooxml_core::XmlDocument, key: &str, value: &str) -> Result<()> {
    if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(value) {
        return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
    }
    let mut root = doc.lens_mut::<ExtendedProperties>()?;
    match key {
        "company" => root.get_or_insert::<Company>()?.set_text(value)?,
        "manager" => root.get_or_insert::<Manager>()?.set_text(value)?,
        _ => return Err(unsupported_property(key)),
    }
    Ok(())
}

fn application_properties_xml(sheet_names: &[String]) -> Result<ooxml_core::XmlDocument> {
    use std::fmt::Write;

    let count = sheet_names.len();
    let mut xml = format!(concat!(
        r#"<Properties xmlns="{}" xmlns:vt="{}">"#,
        "<Application>ooxml-rs</Application>",
        r#"<HeadingPairs><vt:vector size="2" baseType="variant">"#,
        "<vt:variant><vt:lpstr>Worksheets</vt:lpstr></vt:variant>",
        "<vt:variant><vt:i4>{}</vt:i4></vt:variant></vt:vector></HeadingPairs>",
        r#"<TitlesOfParts><vt:vector size="{}" baseType="lpstr">"#,
    ), APP_NS, VARIANT_NS, count, count);
    for name in sheet_names {
        if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(name) {
            return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
        }
        let name = quick_xml::escape::escape(name).replace('\r', "&#13;");
        write!(xml, "<vt:lpstr>{name}</vt:lpstr>").expect("string write");
    }
    xml.push_str("</vt:vector></TitlesOfParts></Properties>");
    Ok(ooxml_core::XmlDocument::parse(xml.as_bytes())?)
}

/// Encodes core, application and custom metadata for an OOXML package.
pub fn encode_document_properties(
    standard: &[(String, String)],
    custom: &[(String, String, String)],
    sheet_names: &[String],
) -> Result<[Option<String>; 3]> {
    let mut core = ooxml_core::XmlDocument::parse(EMPTY_CORE)?;
    let mut app = application_properties_xml(sheet_names)?;
    for (key, value) in standard {
        if matches!(key.as_str(), "company" | "manager") {
            set_app_field(&mut app, key, value)?;
        } else {
            set_core_field(&mut core, key, value)?;
        }
    }
    let custom = if custom.is_empty() {
        None
    } else {
        Some(custom_properties_xml(custom)?.serialize())
    };
    Ok([
        Some(core.serialize()),
        Some(app.serialize()),
        custom,
    ])
}

fn unsupported_property(key: &str) -> Error {
    Error::InvalidSetting { what: "core property".into(), reason: format!("unsupported property {key}") }
}

impl Workbook {
    fn add_core_properties(&mut self, doc: ooxml_core::XmlDocument) -> Result<()> {
        self.add_metadata_part(doc, CORE_PART, "docProps/core{}.xml", CORE_TYPE, CORE_REL)
    }

    fn add_metadata_part(&mut self, doc: ooxml_core::XmlDocument, preferred: &str, alternative: &str, content_type: &str, relationship: &str) -> Result<()> {
        let mut plan = RegistryPlan::new(&self.pkg);
        let name = if self.pkg.contains(preferred) {
            PartNameRequest::Numbered(alternative.into())
        } else {
            PartNameRequest::Exact(preferred.into())
        };
        let part = plan.add_part(&self.pkg, name, content_type, doc)?;
        plan.add_relationship(&self.pkg, None, relationship, Target::Part(part))?;
        self.apply_registry_edit(plan, |_| Ok(()))
    }

    /// Creates a custom property part with string, number, boolean and date values.
    /// Each entry contains a name, its VT type and the value. Filetime values
    /// use signed Unix seconds. Existing custom property parts are protected.
    pub fn add_custom_properties(&mut self, entries: &[(String, String, String)]) -> Result<()> {
        self.check_editable()?;
        if entries.is_empty() {
            return Ok(());
        }
        if self.pkg.package_relationships().by_type(CUSTOM_REL).next().is_some() {
            return Err(Error::invalid_argument(None, "custom properties already exist".into()));
        }
        let doc = custom_properties_xml(entries)?;
        self.add_metadata_part(doc, CUSTOM_PART, "docProps/custom{}.xml", CUSTOM_TYPE, CUSTOM_REL)
    }

    /// Reads scalar document properties with their XML value types.
    pub fn document_properties(&mut self) -> Vec<(String, String, String, String)> {
        package_document_properties(&mut self.pkg)
    }

    fn core_properties_part(&self) -> Option<String> {
        self.property_part(CORE_REL)
    }

    fn property_part(&self, relationship: &str) -> Option<String> {
        let rels = self.pkg.package_relationships();
        rels.resolve(rels.by_type(relationship).next()?)
    }
}

fn custom_properties_xml(entries: &[(String, String, String)]) -> Result<ooxml_core::XmlDocument> {
    use std::fmt::Write;

    let mut xml = format!(r#"<Properties xmlns="{CUSTOM_NS}" xmlns:vt="{VARIANT_NS}">"#);
    let mut names = std::collections::HashSet::new();
    for (index, (name, kind, value)) in entries.iter().enumerate() {
        let bad = || Error::invalid_argument(None, format!("invalid custom property {name:?}"));
        if name.is_empty() || !names.insert(name) {
            return Err(bad());
        }
        let text = match kind.as_str() {
            "lpwstr" => crate::escape::encode(value).into_owned(),
            "i4" if value.parse::<i32>().is_ok() => value.clone(),
            "r8" if value.parse::<f64>().is_ok_and(f64::is_finite) => value.clone(),
            "bool" if matches!(value.as_str(), "true" | "false") => value.clone(),
            "filetime" => {
                let seconds = value.parse::<i64>().map_err(|_| bad())?;
                w3cdtf_seconds(i128::from(seconds))
            }
            _ => return Err(bad()),
        };
        let name = crate::escape::encode(name);
        let name = quick_xml::escape::escape(name)
            .replace('\t', "&#9;").replace('\n', "&#10;").replace('\r', "&#13;");
        let text = quick_xml::escape::escape(text).replace('\r', "&#13;");
        let pid = u32::try_from(index + 2).map_err(|_| Error::Malformed("too many custom properties".into()))?;
        write!(xml,
            r#"<property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="{pid}" name="{name}"><vt:{kind}>{text}</vt:{kind}></property>"#
        ).expect("string write");
    }
    xml.push_str("</Properties>");
    Ok(ooxml_core::XmlDocument::parse(xml.as_bytes())?)
}

pub(crate) fn heading_property_keys(label: &str) -> Option<(&'static str, &'static str)> {
    match label {
        "Worksheets" | "工作表" | "Листы" | "أوراق العمل" | "ワークシート" |
        "גליונות עבודה" | "Arbeitsblätter" | "Çalışma Sayfaları" | "Feuilles de calcul" |
        "Fogli di lavoro" | "Folhas de cálculo" | "Planilhas" | "Regneark" |
        "Hojas de cálculo" | "Werkbladen" => Some(("Worksheets", "SheetNames")),
        "Named Ranges" | "Rangos con nombre" | "名前付き一覧" |
        "Benannte Bereiche" | "Navngivne områder" => Some(("NamedRanges", "DefinedNames")),
        "Charts" | "Diagramme" => Some(("Chartsheets", "ChartNames")),
        _ => None,
    }
}

/// Reads OOXML package document properties independently of the workbook encoding.
pub fn package_document_properties(package: &mut ooxml_core::Package) -> Vec<(String, String, String, String)> {
    const FIELDS: &[(&str, &str, &str, &str)] = &[
        (CORE_NS, "category", "Category", "string"),
        (CORE_NS, "contentStatus", "ContentStatus", "string"),
        (CORE_NS, "keywords", "Keywords", "string"),
        (CORE_NS, "lastModifiedBy", "LastAuthor", "string"),
        (CORE_NS, "lastPrinted", "LastPrinted", "string"),
        (CORE_NS, "revision", "RevNumber", "string"),
        (CORE_NS, "version", "Version", "string"),
        (DC_NS, "creator", "Author", "string"),
        (DC_NS, "description", "Comments", "string"),
        (DC_NS, "identifier", "Identifier", "string"),
        (DC_NS, "language", "Language", "string"),
        (DC_NS, "subject", "Subject", "string"),
        (DC_NS, "title", "Title", "string"),
        ("http://purl.org/dc/terms/", "created", "CreatedDate", "date"),
        ("http://purl.org/dc/terms/", "modified", "ModifiedDate", "date"),
        (APP_NS, "Application", "Application", "string"),
        (APP_NS, "AppVersion", "AppVersion", "string"),
        (APP_NS, "Company", "Company", "string"),
        (APP_NS, "DocSecurity", "DocSecurity", "string"),
        (APP_NS, "Manager", "Manager", "string"),
    ];

    fn variant_vector(root: &ooxml_core::RawNode, name: &'static str) -> Vec<String> {
        root.elements().find(|node| node.is(QName::new(APP_NS, name)))
            .and_then(|node| node.elements().find(|n| n.is(QName::new(VARIANT_NS, "vector"))))
            .into_iter().flat_map(ooxml_core::RawNode::elements)
            .filter_map(|node| {
                if node.is(QName::new(VARIANT_NS, "variant")) {
                    Some(node.elements().next()?.text().into_owned())
                } else {
                    Some(node.text().into_owned())
                }
            }).collect()
    }

    let mut values = Vec::new();
    for (relationship, custom) in [(CORE_REL, false), (APP_REL, false), (CUSTOM_REL, true)] {
        let Some(part) = package.package_relationships().by_type(relationship).next()
            .and_then(|rel| package.package_relationships().resolve(rel)) else { continue };
        let Ok(doc) = package.xml_part(&part) else { continue };
        if relationship == APP_REL {
            for key in ["HyperlinksChanged", "SharedDoc", "LinksUpToDate", "ScaleCrop"] {
                let enabled = doc.root().elements().find(|node| node.is(QName::new(APP_NS, key)))
                    .is_some_and(|node| node.text() == "true");
                values.push(("standard".into(), key.into(), "bool".into(), enabled.to_string()));
            }
            let headings = variant_vector(doc.root(), "HeadingPairs");
            let titles = variant_vector(doc.root(), "TitlesOfParts");
            let mut offset = 0_usize;
            for pair in headings.as_chunks::<2>().0 {
                let Ok(count) = pair[1].parse::<usize>() else { break };
                let end = offset.saturating_add(count);
                let fields = heading_property_keys(&pair[0]);
                if let Some((count_key, list_key)) = fields && !titles.is_empty() {
                    values.push(("standard".into(), count_key.into(), "int".into(), count.to_string()));
                    values.push(("standard".into(), list_key.into(), "array".into(), String::new()));
                    for title in &titles[offset.min(titles.len())..end.min(titles.len())] {
                        values.push(("standard".into(), list_key.into(), "item".into(), title.clone()));
                    }
                }
                offset = end;
            }
        }
        for node in doc.root().elements() {
            if custom {
                let Some(name) = node.attr(QName::new("", "name")) else { continue };
                if !node.is(QName::new(CUSTOM_NS, "property")) { continue; }
                let Some(value) = node.elements().find(|n| n.name().ns() == VARIANT_NS) else { continue };
                let text = value.text();
                values.push(("custom".into(), crate::escape::decode(name).into_owned(),
                    value.name().local().into(), crate::escape::decode(&text).into_owned()));
            } else if let Some((_, _, key, kind)) = FIELDS.iter().find(|(ns, name, ..)| node.is(QName::new(ns, name))) {
                values.push(("standard".into(), (*key).into(), (*kind).into(), node.text().into_owned()));
            }
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_custom_properties_are_atomic() {
        let mut book = Workbook::new_at(UNIX_EPOCH).unwrap();
        let original = book.to_bytes().unwrap();
        let invalid = [("Bad".into(), "i4".into(), "not an integer".into())];
        let e = book.add_custom_properties(&invalid).unwrap_err();
        assert!(matches!(e, Error::InvalidArgument { .. }), "{e:?}");
        assert_eq!(e.cell(), None);
        assert_eq!(book.to_bytes().unwrap(), original);
        for bad in [
            vec![("".into(), "lpwstr".into(), "x".into())],
            vec![("A".into(), "lpwstr".into(), "x".into()), ("A".into(), "lpwstr".into(), "y".into())],
            vec![("A".into(), "nope".into(), "x".into())],
        ] {
            let e = book.add_custom_properties(&bad).unwrap_err();
            assert!(matches!(e, Error::InvalidArgument { .. }), "{e:?}");
            assert_eq!(book.to_bytes().unwrap(), original);
        }
        let input = [("A & B".into(), "lpwstr".into(), "a_x0041_ & b".into())];
        book.add_custom_properties(&input).unwrap();
        let saved = book.to_bytes().unwrap();
        let e = book.add_custom_properties(&input).unwrap_err();
        assert!(matches!(e, Error::InvalidArgument { .. }), "{e:?}");
        assert_eq!(e.cell(), None);
        assert_eq!(book.to_bytes().unwrap(), saved);
        let mut reopened = Workbook::from_bytes(saved).unwrap();
        assert!(reopened.document_properties().contains(&(
            "custom".into(), "A & B".into(), "lpwstr".into(), "a_x0041_ & b".into(),
        )));
    }

    #[test]
    fn dates() {
        assert_eq!(w3cdtf(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(w3cdtf(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(w3cdtf(UNIX_EPOCH + Duration::from_secs(1_794_822_645)), "2026-11-16T09:50:45Z");
        let x = generate(UNIX_EPOCH);
        ooxml_core::XmlDocument::parse(&x).unwrap();
    }
}
