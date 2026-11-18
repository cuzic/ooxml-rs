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

macro_rules! od_rel {
    ($($name:ident = $local:literal;)*) => {
        $(#[doc = concat!("Relationship type `…/officeDocument/2006/relationships/", $local, "`.")]
        pub const $name: &str = concat!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/", $local);)*
    };
}

od_rel! {
    REL_SLIDE = "slide";
    REL_SLIDE_LAYOUT = "slideLayout";
    REL_NOTES_SLIDE = "notesSlide";
    REL_NOTES_MASTER = "notesMaster";
    REL_COMMENTS = "comments";
    REL_IMAGE = "image";
    REL_AUDIO = "audio";
    REL_VIDEO = "video";
    REL_HYPERLINK = "hyperlink";
    REL_TAGS = "tags";
    REL_CHART = "chart";
    REL_CHART_USER_SHAPES = "chartUserShapes";
    REL_PACKAGE = "package";
    REL_OLE_OBJECT = "oleObject";
    REL_VML_DRAWING = "vmlDrawing";
    REL_THEME_OVERRIDE = "themeOverride";
    REL_DIAGRAM_DATA = "diagramData";
    REL_DIAGRAM_LAYOUT = "diagramLayout";
    REL_DIAGRAM_STYLE = "diagramQuickStyle";
    REL_DIAGRAM_COLORS = "diagramColors";
    REL_PRES_PROPS = "presProps";
    REL_VIEW_PROPS = "viewProps";
    REL_EXTENDED_PROPERTIES = "extended-properties";
}

/// `…/office/2007/relationships/media`.
pub const REL_MEDIA: &str = "http://schemas.microsoft.com/office/2007/relationships/media";
/// `…/office/2007/relationships/hdphoto`.
pub const REL_HDPHOTO: &str = "http://schemas.microsoft.com/office/2007/relationships/hdphoto";
/// `…/office/2007/relationships/diagramDrawing`.
pub const REL_DIAGRAM_DRAWING: &str =
    "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing";
/// `…/office/2011/relationships/chartStyle`.
pub const REL_CHART_STYLE: &str =
    "http://schemas.microsoft.com/office/2011/relationships/chartStyle";
/// `…/office/2011/relationships/chartColorStyle`.
pub const REL_CHART_COLOR_STYLE: &str =
    "http://schemas.microsoft.com/office/2011/relationships/chartColorStyle";

/// Content type of a slide part.
pub const CT_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
/// Content type of a notes slide part.
pub const CT_NOTES_SLIDE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";

/// `p:ext@uri` of the section list (`p14:sectionLst`).
pub const SECTION_LIST_URI: &str = "{521415D9-36F7-43E2-AB2F-B90AF26B5E84}";
/// `p14` namespace.
pub const P14_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";
/// Relationships namespace (`r:`).
pub const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
