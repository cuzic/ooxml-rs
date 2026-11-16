//! `codegen/allowlist/*.toml` and `codegen/overrides.toml` (ADR-0002 §1.3, §5).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::Res;

/// Feature groups, in feature order. Each has `codegen/allowlist/<group>.toml`.
pub const GROUPS: &[&str] = &["sml", "wml", "pml", "dml"];

/// One `codegen/allowlist/<group>.toml`:
///
/// ```toml
/// [write]          # `*View` + `*ViewMut` (typed setters, insert/remove helpers)
/// names = ["x:CT_Row/x:row"]
/// [read]           # `*View` only
/// names = ["x:CT_Workbook/x:workbook"]
/// [detach]         # optional: also `ooxml_core::Detachable` (must be listed above)
/// names = ["a:CT_TextCharacterProperties/a:rPr"]
/// ```
///
/// `[detach]` opts an entry into `Lens::detach_as` (ADR-0002 note
/// "detach/insert"). List only types whose subtrees hold no document-unique
/// value (shape ids, `p:ph@idx`, `w14:paraId`, …) and no reference to one.
///
/// `ChildOf` impls are generated between any two allowlisted entries
/// (read or write), so the generic `LensMut` primitives stay available for
/// read entries; only the generated typed write surface depends on `[write]`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowlistFile {
    #[serde(default)]
    pub write: Section,
    #[serde(default)]
    pub read: Section,
    #[serde(default)]
    pub detach: Section,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

#[derive(Clone, Debug)]
pub struct Allowed {
    pub group: &'static str,
    pub access: Access,
    /// Listed in `[detach]`: generates `Detachable` (and `SameType` partners).
    pub detach: bool,
}

/// SDK Name → (group, access).
pub fn load_allowlists(dir: &Path) -> Res<BTreeMap<String, Allowed>> {
    let mut out = BTreeMap::new();
    for &g in GROUPS {
        let p = dir.join(format!("{g}.toml"));
        let f: AllowlistFile = toml::from_str(&std::fs::read_to_string(&p)?).map_err(|e| format!("{}: {e}", p.display()))?;
        for (list, access) in [(&f.write.names, Access::Write), (&f.read.names, Access::Read)] {
            for n in list {
                if n.ends_with('/') {
                    return Err(format!("{}: {n:?} is an abstract entry; allowlist concrete SDK Names", p.display()).into());
                }
                if out.insert(n.clone(), Allowed { group: g, access, detach: false }).is_some() {
                    return Err(format!("{n:?} is allowlisted twice").into());
                }
            }
        }
        for n in &f.detach.names {
            match out.get_mut(n) {
                Some(a) if a.group == g => a.detach = true,
                _ => return Err(format!("{}: [detach] entry {n:?} must also be in [write] or [read] of this file", p.display()).into()),
            }
        }
    }
    Ok(out)
}

/// One `[[override]]` entry. Every entry needs `reason` and `evidence`.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Override {
    AddAttribute {
        on: String,
        name: String,
        #[serde(rename = "type")]
        ty: String,
        #[serde(default)]
        default: Option<String>,
        reason: String,
        evidence: String,
    },
    AddChild {
        parent: String,
        name: String,
        #[serde(default)]
        after: Option<String>,
        #[serde(default)]
        before: Option<String>,
        /// Maximum occurrences (default 1; 0 = unbounded).
        #[serde(default)]
        max: Option<u32>,
        reason: String,
        evidence: String,
    },
    AddEnumValue {
        /// SDK enum `Type`, e.g. `x:ST_SheetState`.
        #[serde(rename = "enum")]
        enum_type: String,
        value: String,
        reason: String,
        evidence: String,
    },
    SetDefault {
        on: String,
        attribute: String,
        value: String,
        reason: String,
        evidence: String,
    },
    MceWrapped {
        parent: String,
        children: Vec<String>,
        reason: String,
        evidence: String,
    },
    SetContent {
        on: String,
        slots: Vec<SlotOverride>,
        reason: String,
        evidence: String,
    },
    SetType {
        on: String,
        attribute: String,
        #[serde(rename = "type")]
        ty: String,
        reason: String,
        evidence: String,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotOverride {
    /// SDK Names (`x:CT_Cell/x:c`) of the slot's elements.
    pub names: Vec<String>,
    /// 0 or absent = unbounded.
    #[serde(default)]
    pub max_each: Option<u32>,
    #[serde(default)]
    pub max_total: Option<u32>,
}

impl Override {
    /// The SDK Name the override is keyed on (`None` for enum overrides).
    pub fn target(&self) -> Option<&str> {
        match self {
            Override::AddAttribute { on, .. }
            | Override::SetDefault { on, .. }
            | Override::SetContent { on, .. }
            | Override::SetType { on, .. } => Some(on),
            Override::AddChild { parent, .. } | Override::MceWrapped { parent, .. } => Some(parent),
            Override::AddEnumValue { .. } => None,
        }
    }
    fn reason_evidence(&self) -> (&str, &str) {
        match self {
            Override::AddAttribute { reason, evidence, .. }
            | Override::AddChild { reason, evidence, .. }
            | Override::AddEnumValue { reason, evidence, .. }
            | Override::SetDefault { reason, evidence, .. }
            | Override::MceWrapped { reason, evidence, .. }
            | Override::SetContent { reason, evidence, .. }
            | Override::SetType { reason, evidence, .. } => (reason, evidence),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverridesFile {
    #[serde(default, rename = "override")]
    overrides: Vec<Override>,
}

pub fn load_overrides(path: &Path) -> Res<Vec<Override>> {
    parse_overrides(&std::fs::read_to_string(path)?)
}

pub fn parse_overrides(text: &str) -> Res<Vec<Override>> {
    let f: OverridesFile = toml::from_str(text).map_err(|e| format!("overrides.toml: {e}"))?;
    for o in &f.overrides {
        let (r, e) = o.reason_evidence();
        if r.trim().is_empty() || e.trim().is_empty() {
            return Err(format!("override {o:?} needs a non-empty reason and evidence").into());
        }
        if let Override::AddChild { after, before, .. } = o
            && after.is_some() == before.is_some()
        {
            return Err(format!("add_child {o:?} needs exactly one of after / before").into());
        }
    }
    Ok(f.overrides)
}
