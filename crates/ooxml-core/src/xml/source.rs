use std::{fmt, ops::Deref, sync::Arc};

/// Decoded XML content. Unchanged bytes share the document source, while
/// normalized and edited values keep a buffer of their own.
#[derive(Clone)]
pub(crate) struct SStr {
    source: Arc<str>,
    start: u32,
    end: u32,
}

impl SStr {
    pub(super) fn from_decoded(source: &Arc<str>, decoded: &str) -> Self {
        match source.substr_range(decoded) {
            Some(span) if !decoded.is_empty() => Self::span(source, span.start, span.end),
            _ if decoded.is_empty() => Self::span(source, 0, 0),
            _ => decoded.into(),
        }
    }

    pub(super) fn span(source: &Arc<str>, start: usize, end: usize) -> Self {
        Self {
            source: source.clone(),
            start: start as u32,
            end: end as u32,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        self
    }
}

impl From<&str> for SStr {
    fn from(value: &str) -> Self {
        // A reversed range denotes the entire owned buffer, including edited
        // values whose length need not fit in a parsed source offset.
        Self {
            source: Arc::from(value),
            start: 1,
            end: 0,
        }
    }
}

impl Deref for SStr {
    type Target = str;

    fn deref(&self) -> &str {
        if self.end < self.start {
            &self.source
        } else {
            &self.source[self.start as usize..self.end as usize]
        }
    }
}

impl PartialEq for SStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for SStr {}

impl fmt::Debug for SStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}
