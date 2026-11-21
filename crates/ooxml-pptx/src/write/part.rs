//! Keeps the package available for paragraph edits that change relationships.

use std::ops::{Deref, DerefMut};

use ooxml_core::{Package, XmlDocument};

#[derive(Debug)]
pub(crate) enum EditPart<'a> {
    Package {
        package: &'a mut Package,
        name: String,
        slides: &'a [crate::presentation::SlideEntry],
        link_identity: &'a std::sync::Arc<()>,
    },
    Staged(&'a mut XmlDocument),
}

impl EditPart<'_> {
    pub(crate) fn reborrow(&mut self) -> EditPart<'_> {
        match self {
            Self::Package { package, name, slides, link_identity } => EditPart::Package {
                package,
                name: name.clone(),
                slides,
                link_identity,
            },
            Self::Staged(doc) => EditPart::Staged(doc),
        }
    }
}

impl Deref for EditPart<'_> {
    type Target = XmlDocument;

    fn deref(&self) -> &XmlDocument {
        match self {
            Self::Package { package, name, .. } => package
                .parsed_part(name)
                .expect("part parsed before editing"),
            Self::Staged(doc) => doc,
        }
    }
}

impl DerefMut for EditPart<'_> {
    fn deref_mut(&mut self) -> &mut XmlDocument {
        match self {
            Self::Package { package, name, .. } => package
                .xml_part_mut(name)
                .expect("part parsed before editing"),
            Self::Staged(doc) => doc,
        }
    }
}
