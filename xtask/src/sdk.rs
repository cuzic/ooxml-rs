//! Open XML SDK `data/` JSON model (only the fields the generator reads).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::Res;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SchemaFile {
    pub target_namespace: String,
    #[serde(default)]
    pub types: Vec<SdkType>,
    #[serde(default)]
    pub enums: Vec<SdkEnum>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SdkType {
    pub name: String,
    pub class_name: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub base_class: String,
    #[serde(default)]
    pub is_leaf_text: bool,
    #[serde(default)]
    pub attributes: Vec<SdkAttr>,
    #[serde(default)]
    pub children: Vec<SdkChild>,
    #[serde(default)]
    pub particle: Option<Particle>,
    #[serde(default)]
    pub validators: Vec<Validator>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SdkAttr {
    #[serde(rename = "QName")]
    pub qname: String,
    #[serde(rename = "Type")]
    pub ty: String,
    #[serde(default)]
    pub property_name: Option<String>,
    #[serde(default)]
    pub property_comments: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SdkChild {
    pub name: String,
    #[serde(default)]
    pub property_name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Particle {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub occurs: Option<Vec<Occurs>>,
    #[serde(default)]
    pub items: Vec<Particle>,
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Occurs {
    #[serde(default)]
    pub min: Option<u32>,
    #[serde(default)]
    pub max: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Validator {
    pub name: String,
    #[serde(rename = "Type", default)]
    pub ty: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SdkEnum {
    #[serde(rename = "Type")]
    pub ty: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub facets: Vec<Facet>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Facet {
    pub value: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NamespaceEntry {
    pub prefix: String,
    pub uri: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PartFile {
    pub name: String,
    pub base: String,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub relationship_type: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub root_element: Option<String>,
    #[serde(default)]
    pub extension: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub paths: BTreeMap<String, String>,
    #[serde(default)]
    pub children: Vec<PartChildEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PartChildEntry {
    pub name: String,
    pub api_name: String,
    #[serde(default)]
    pub max_occurs_great_than_one: bool,
    #[serde(default)]
    pub min_occurs_is_non_zero: bool,
}

/// The SDK data set: schema files in sorted file-name order.
pub struct Sdk {
    pub files: Vec<(String, SchemaFile)>,
    pub parts: Vec<PartFile>,
    /// prefix → URI
    pub prefixes: BTreeMap<String, String>,
}

fn sorted_json(dir: &Path) -> Res<Vec<std::path::PathBuf>> {
    let mut v: Vec<_> = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    v.retain(|p| p.extension().is_some_and(|e| e == "json"));
    v.sort();
    Ok(v)
}

impl Sdk {
    pub fn load(dir: &Path) -> Res<Self> {
        let mut files = Vec::new();
        for p in sorted_json(&dir.join("schemas"))? {
            let f: SchemaFile = serde_json::from_slice(&std::fs::read(&p)?)
                .map_err(|e| format!("{}: {e}", p.display()))?;
            files.push((p.file_name().unwrap().to_string_lossy().into_owned(), f));
        }
        let mut parts = Vec::new();
        for p in sorted_json(&dir.join("parts"))? {
            parts.push(serde_json::from_slice(&std::fs::read(&p)?).map_err(|e| format!("{}: {e}", p.display()))?);
        }
        let ns: Vec<NamespaceEntry> = serde_json::from_slice(&std::fs::read(dir.join("namespaces.json"))?)?;
        let prefixes = ns.into_iter().map(|e| (e.prefix, e.uri)).collect();
        Ok(Sdk { files, parts, prefixes })
    }
}

/// `Occurs` → maximum (None = unbounded). Missing Occurs = 1; a present
/// entry without `Max` = unbounded; several (per Office version) → the most
/// permissive (ADR-0002 §4.1).
pub fn max_occurs(p: &Particle) -> Option<u32> {
    match &p.occurs {
        None => Some(1),
        Some(o) => {
            if o.iter().any(|e| e.max.is_none()) {
                None
            } else {
                o.iter().filter_map(|e| e.max).max().or(Some(1))
            }
        }
    }
}

/// Can the particle match nothing? (missing Occurs = 1..1; Occurs without
/// `Min` = min 0.)
pub fn optional(p: &Particle) -> bool {
    if let Some(o) = &p.occurs
        && !o.is_empty()
        && o.iter().all(|e| e.min.unwrap_or(0) == 0)
    {
        return true;
    }
    match p.kind.as_deref().unwrap_or("Element") {
        "Choice" => p.items.iter().any(optional),
        "Sequence" | "Group" | "All" => p.items.iter().all(optional),
        _ => false,
    }
}
