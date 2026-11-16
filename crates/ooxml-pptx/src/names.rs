//! Namespace URIs and relationship types used by the domain layer.

/// PresentationML main namespace.
pub const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
/// DrawingML main namespace.
pub const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// `a:graphicData@uri` of a table.
pub const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";
/// Modern comments (PowerPoint 2018/8 main).
pub const P188_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2018/8/main";
/// Drawing command monikers (`ac:`).
pub const AC_NS: &str = "http://schemas.microsoft.com/office/drawing/2013/main/command";
/// PowerPoint command monikers (`pc:`).
pub const PC_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2013/main/command";
/// Relationship from a slide to its modern comment part.
pub const MODERN_COMMENTS_REL: &str = "http://schemas.microsoft.com/office/2018/10/relationships/comments";
/// Relationships namespace (`r:`).
pub const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
