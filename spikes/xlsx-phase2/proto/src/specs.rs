//! Hand-written lens specs for the `styles.xml` elements ADR-0006 needs.
//!
//! The real implementation would get these from `xtask codegen` (allowlist
//! entries); writing them by hand here is how the prototype finds out which
//! entries that allowlist needs. Child order follows the Transitional
//! `sml.xsd` (`CT_Stylesheet`, `CT_Font` is an unbounded choice, `CT_Border`
//! a sequence, …).

use ooxml_core::{AttrSpec, ChildOf, Content, ElementSpec, QName, SimpleType, Slot};

pub const NS_X: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

const fn slot(names: &'static [QName], max: Option<u32>) -> Slot {
    Slot { names, wildcard: None, max_each: max, max_total: max }
}

macro_rules! spec {
    ($t:ident, $local:literal, [$($a:literal : $ty:ident),*], $content:expr) => {
        pub struct $t;
        impl ElementSpec for $t {
            const QNAME: QName = QName::new(NS_X, $local);
            const SDK_NAME: &'static str = concat!("proto:", $local);
            const ATTRS: &'static [AttrSpec] =
                &[$(AttrSpec { name: QName::new("", $a), ty: SimpleType::$ty, default: None }),*];
            const CONTENT: Content = $content;
        }
    };
}

macro_rules! q {
    ($($l:literal),*) => { &[$(QName::new(NS_X, $l)),*] };
}

macro_rules! children {
    ($p:ty: $($c:ty),*) => { $(impl ChildOf<$p> for $c {})* };
}

spec!(StyleSheet, "styleSheet", [], Content::Slots(&[
    slot(q!("numFmts"), Some(1)), slot(q!("fonts"), Some(1)), slot(q!("fills"), Some(1)),
    slot(q!("borders"), Some(1)), slot(q!("cellStyleXfs"), Some(1)), slot(q!("cellXfs"), Some(1)),
    slot(q!("cellStyles"), Some(1)), slot(q!("dxfs"), Some(1)), slot(q!("tableStyles"), Some(1)),
    slot(q!("colors"), Some(1)), slot(q!("extLst"), Some(1)),
]));
spec!(NumFmts, "numFmts", ["count": UInt32], Content::Slots(&[slot(q!("numFmt"), None)]));
spec!(NumFmt, "numFmt", ["numFmtId": UInt32, "formatCode": String], Content::Empty);
spec!(Fonts, "fonts", ["count": UInt32], Content::Slots(&[slot(q!("font"), None)]));
spec!(Font, "font", [], Content::Slots(&[slot(
    q!("b", "i", "strike", "condense", "extend", "outline", "shadow", "u", "vertAlign", "sz", "color", "name",
       "family", "charset", "scheme"),
    None,
)]));
spec!(Bold, "b", ["val": Boolean], Content::Empty);
spec!(FontSize, "sz", ["val": Double], Content::Empty);
spec!(FontName, "name", ["val": String], Content::Empty);
spec!(Color, "color", ["auto": Boolean, "indexed": UInt32, "rgb": HexBinary, "theme": UInt32, "tint": Double], Content::Empty);
spec!(FgColor, "fgColor", ["auto": Boolean, "indexed": UInt32, "rgb": HexBinary, "theme": UInt32, "tint": Double], Content::Empty);
spec!(Fills, "fills", ["count": UInt32], Content::Slots(&[slot(q!("fill"), None)]));
spec!(Fill, "fill", [], Content::Slots(&[slot(q!("patternFill", "gradientFill"), Some(1))]));
spec!(PatternFill, "patternFill", ["patternType": String], Content::Slots(&[
    slot(q!("fgColor"), Some(1)), slot(q!("bgColor"), Some(1)),
]));
spec!(Borders, "borders", ["count": UInt32], Content::Slots(&[slot(q!("border"), None)]));
spec!(Border, "border", ["diagonalUp": Boolean, "diagonalDown": Boolean, "outline": Boolean], Content::Slots(&[
    slot(q!("start"), Some(1)), slot(q!("end"), Some(1)), slot(q!("left"), Some(1)), slot(q!("right"), Some(1)),
    slot(q!("top"), Some(1)), slot(q!("bottom"), Some(1)), slot(q!("diagonal"), Some(1)),
    slot(q!("vertical"), Some(1)), slot(q!("horizontal"), Some(1)),
]));
spec!(Left, "left", ["style": String], Content::Slots(&[slot(q!("color"), Some(1))]));
spec!(Right, "right", ["style": String], Content::Slots(&[slot(q!("color"), Some(1))]));
spec!(Top, "top", ["style": String], Content::Slots(&[slot(q!("color"), Some(1))]));
spec!(Bottom, "bottom", ["style": String], Content::Slots(&[slot(q!("color"), Some(1))]));
spec!(Diagonal, "diagonal", ["style": String], Content::Slots(&[slot(q!("color"), Some(1))]));
spec!(CellXfs, "cellXfs", ["count": UInt32], Content::Slots(&[slot(q!("xf"), None)]));
spec!(Xf, "xf", [
    "numFmtId": UInt32, "fontId": UInt32, "fillId": UInt32, "borderId": UInt32, "xfId": UInt32,
    "quotePrefix": Boolean, "pivotButton": Boolean, "applyNumberFormat": Boolean, "applyFont": Boolean,
    "applyFill": Boolean, "applyBorder": Boolean, "applyAlignment": Boolean, "applyProtection": Boolean
], Content::Slots(&[slot(q!("alignment"), Some(1)), slot(q!("protection"), Some(1)), slot(q!("extLst"), Some(1))]));

children!(StyleSheet: NumFmts, Fonts, Fills, Borders, CellXfs);
children!(NumFmts: NumFmt);
children!(Fonts: Font);
children!(Font: Bold, FontSize, Color, FontName);
children!(Fills: Fill);
children!(Fill: PatternFill);
children!(PatternFill: FgColor);
children!(Borders: Border);
children!(Border: Left, Right, Top, Bottom, Diagonal);
children!(Left: Color);
children!(Right: Color);
children!(Top: Color);
children!(Bottom: Color);
children!(CellXfs: Xf);
