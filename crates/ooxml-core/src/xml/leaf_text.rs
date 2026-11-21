use super::{RawItem, RawNode, RawText, XmlDocument, mark_path_dirty};
use crate::error::{Error, Result};
use crate::name::{QName, TYPED_ELEMENT_NAMESPACES};
use crate::value::{check_xml_chars, parse_bool};

const XSI_NIL: QName = QName::new("http://www.w3.org/2001/XMLSchema-instance", "nil");

/// One element in a schemaless leaf-text path (ADR-0015 §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafStep<'a> {
    /// Namespace URI, or an empty string for an element without a namespace.
    pub ns: &'a str,
    /// Local name, without a prefix.
    pub local: &'a str,
    /// Zero-based index among element siblings with the same URI and local name.
    /// The root's index is zero.
    pub index: usize,
}

impl LeafStep<'_> {
    fn matches(self, node: &RawNode) -> bool {
        self.ns == node.name().ns() && self.local == node.name().local()
    }
}

impl XmlDocument {
    /// Runs the same validation as [`Self::set_leaf_text`] without changing the tree.
    pub fn check_set_leaf_text(&self, path: &[LeafStep<'_>], value: &str) -> Result<()> {
        self.validate_leaf_text(path, value).map(|_| ())
    }

    /// Replaces the text of an existing leaf in a schemaless XML part.
    ///
    /// The path begins at the document root. An empty or unmatched path returns
    /// [`Error::NoSuchElement`]. Each matched element must be outside the
    /// namespaces reserved for typed editing ([`Error::TypedNamespace`]).
    /// Elements, comments or processing instructions inside the target return
    /// [`Error::NotLeaf`]. A true `xsi:nil` returns [`Error::NilElement`], and XML
    /// 1.0 character validation returns [`Error::InvalidXmlChar`].
    ///
    /// On error the document is unchanged. Attributes, including `xml:space`,
    /// retain their values, and the original tags are preserved except when a
    /// self-closing target needs a closing tag.
    pub fn set_leaf_text(&mut self, path: &[LeafStep<'_>], value: &str) -> Result<()> {
        let (target, indices) = self.validate_leaf_text(path, value)?;
        if target.text() == value {
            return Ok(());
        }

        let target = mark_path_dirty(&mut self.root, &indices);
        if value.is_empty() {
            target.set_children(Vec::new());
        } else {
            if target.span().is_some_and(|span| span.close.is_none()) {
                target.mark_own_dirty();
                target.preserve_attribute_spelling();
            }
            target.set_children(vec![RawItem::Text(RawText::written(value.into(), false))]);
        }
        Ok(())
    }

    fn validate_leaf_text(&self, path: &[LeafStep<'_>], value: &str) -> Result<(&RawNode, Vec<usize>)> {
        let Some((root_step, steps)) = path.split_first() else {
            return Err(Error::NoSuchElement);
        };
        let mut node = &self.root;
        if root_step.index != 0 || !root_step.matches(node) {
            return Err(Error::NoSuchElement);
        }

        let mut indices = Vec::with_capacity(steps.len().min(self.max_depth()));
        for step in steps {
            check_namespace(node)?;
            let (index, child) = node
                .indexed_elements()
                .filter(|(_, child)| step.matches(child))
                .nth(step.index)
                .ok_or(Error::NoSuchElement)?;
            indices.push(index);
            node = child;
        }
        check_namespace(node)?;

        if node.children().iter().any(|item| !matches!(item, RawItem::Text(_))) {
            return Err(Error::NotLeaf);
        }
        if node.attr(XSI_NIL).and_then(parse_bool) == Some(true) {
            return Err(Error::NilElement);
        }
        check_xml_chars(value)?;
        Ok((node, indices))
    }
}

fn check_namespace(node: &RawNode) -> Result<()> {
    if TYPED_ELEMENT_NAMESPACES.contains(&node.name().ns()) {
        return Err(Error::TypedNamespace);
    }
    Ok(())
}
