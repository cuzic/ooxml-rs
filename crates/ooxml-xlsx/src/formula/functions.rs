//! The value-function table (ADR-0003 §2.2): functions shown by evidence to
//! read no cell outside the rectangles of the references passed to them.
//! Anything not listed here gives "depends on everything". Growing the table
//! is a reviewed change: one function, its evidence, and an O1/O2 test.

/// How a listed function's precedents are computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    /// The union of the arguments' precedents.
    Union,
    /// `SUMIF`/`AVERAGEIF`: literal `range`/`sum_range`, `sum_range` resized
    /// to `range`'s shape (§2.3 a).
    SumIf,
}

/// Evidence classes, cited per entry.
const DOC_SCALAR: &str = "Microsoft docs: arguments are values; a reference is only dereferenced";
const DOC_AGG: &str = "Microsoft docs; LibreOffice ScInterpreter iterates exactly the passed ranges";
const DOC_IFS: &str = "Microsoft docs; LibreOffice IterateParametersIfs requires every range to have the first range's size";
const DOC_LOOKUP: &str = "LibreOffice CalculateLookup checks the index against the passed range before reading";
const DOC_SELECT: &str = "result is one of the arguments (or a cell inside the first one)";
const DOC_SUMIF: &str = "Microsoft 'SUMIF function': sum_range resized from its top-left to range's shape";
const DOC_VOLATILE: &str = "reads no cell; stale by nature, no edit makes it more stale";
const DOC_REFINFO: &str = "reads no cell value; argument precedents kept (harmless over-approximation)";

/// (upper-case name as stored in files, rule, evidence).
pub(crate) const TABLE: &[(&str, Rule, &str)] = &[
    // Aggregates.
    ("SUM", Rule::Union, DOC_AGG),
    ("SUMPRODUCT", Rule::Union, DOC_AGG),
    ("SUMSQ", Rule::Union, DOC_AGG),
    ("PRODUCT", Rule::Union, DOC_AGG),
    ("MIN", Rule::Union, DOC_AGG),
    ("MAX", Rule::Union, DOC_AGG),
    ("MINA", Rule::Union, DOC_AGG),
    ("MAXA", Rule::Union, DOC_AGG),
    ("AVERAGE", Rule::Union, DOC_AGG),
    ("AVERAGEA", Rule::Union, DOC_AGG),
    ("COUNT", Rule::Union, DOC_AGG),
    ("COUNTA", Rule::Union, DOC_AGG),
    ("COUNTBLANK", Rule::Union, DOC_AGG),
    ("COUNTIF", Rule::Union, DOC_AGG),
    ("MEDIAN", Rule::Union, DOC_AGG),
    ("MODE", Rule::Union, DOC_AGG),
    ("LARGE", Rule::Union, DOC_AGG),
    ("SMALL", Rule::Union, DOC_AGG),
    ("STDEV", Rule::Union, DOC_AGG),
    ("STDEVP", Rule::Union, DOC_AGG),
    ("VAR", Rule::Union, DOC_AGG),
    ("VARP", Rule::Union, DOC_AGG),
    ("RANK", Rule::Union, DOC_AGG),
    ("AND", Rule::Union, DOC_AGG),
    ("OR", Rule::Union, DOC_AGG),
    ("_XLFN.XOR", Rule::Union, DOC_AGG),
    // Same-size multi-range conditionals.
    ("SUMIFS", Rule::Union, DOC_IFS),
    ("AVERAGEIFS", Rule::Union, DOC_IFS),
    ("COUNTIFS", Rule::Union, DOC_IFS),
    ("_XLFN.MINIFS", Rule::Union, DOC_IFS),
    ("_XLFN.MAXIFS", Rule::Union, DOC_IFS),
    // Resizing conditionals.
    ("SUMIF", Rule::SumIf, DOC_SUMIF),
    ("AVERAGEIF", Rule::SumIf, DOC_SUMIF),
    // Lookups that stay inside their table.
    ("VLOOKUP", Rule::Union, DOC_LOOKUP),
    ("HLOOKUP", Rule::Union, DOC_LOOKUP),
    ("MATCH", Rule::Union, DOC_LOOKUP),
    // Selection (value position; §2.3 a covers position-dependent uses).
    ("INDEX", Rule::Union, DOC_SELECT),
    ("CHOOSE", Rule::Union, DOC_SELECT),
    ("IF", Rule::Union, DOC_SELECT),
    ("IFERROR", Rule::Union, DOC_SELECT),
    ("_XLFN.IFNA", Rule::Union, DOC_SELECT),
    ("_XLFN.IFS", Rule::Union, DOC_SELECT),
    ("_XLFN.SWITCH", Rule::Union, DOC_SELECT),
    // Reference information.
    ("ROW", Rule::Union, DOC_REFINFO),
    ("COLUMN", Rule::Union, DOC_REFINFO),
    ("ROWS", Rule::Union, DOC_REFINFO),
    ("COLUMNS", Rule::Union, DOC_REFINFO),
    // Volatile without reads.
    ("NOW", Rule::Union, DOC_VOLATILE),
    ("TODAY", Rule::Union, DOC_VOLATILE),
    ("RAND", Rule::Union, DOC_VOLATILE),
    ("RANDBETWEEN", Rule::Union, DOC_VOLATILE),
    // Scalar math.
    ("ABS", Rule::Union, DOC_SCALAR),
    ("ACOS", Rule::Union, DOC_SCALAR),
    ("ACOSH", Rule::Union, DOC_SCALAR),
    ("ASIN", Rule::Union, DOC_SCALAR),
    ("ASINH", Rule::Union, DOC_SCALAR),
    ("ATAN", Rule::Union, DOC_SCALAR),
    ("ATAN2", Rule::Union, DOC_SCALAR),
    ("ATANH", Rule::Union, DOC_SCALAR),
    ("CEILING", Rule::Union, DOC_SCALAR),
    ("COMBIN", Rule::Union, DOC_SCALAR),
    ("COS", Rule::Union, DOC_SCALAR),
    ("COSH", Rule::Union, DOC_SCALAR),
    ("DEGREES", Rule::Union, DOC_SCALAR),
    ("EVEN", Rule::Union, DOC_SCALAR),
    ("EXP", Rule::Union, DOC_SCALAR),
    ("FACT", Rule::Union, DOC_SCALAR),
    ("FLOOR", Rule::Union, DOC_SCALAR),
    ("INT", Rule::Union, DOC_SCALAR),
    ("LN", Rule::Union, DOC_SCALAR),
    ("LOG", Rule::Union, DOC_SCALAR),
    ("LOG10", Rule::Union, DOC_SCALAR),
    ("MOD", Rule::Union, DOC_SCALAR),
    ("ODD", Rule::Union, DOC_SCALAR),
    ("PI", Rule::Union, DOC_SCALAR),
    ("POWER", Rule::Union, DOC_SCALAR),
    ("RADIANS", Rule::Union, DOC_SCALAR),
    ("ROUND", Rule::Union, DOC_SCALAR),
    ("ROUNDDOWN", Rule::Union, DOC_SCALAR),
    ("ROUNDUP", Rule::Union, DOC_SCALAR),
    ("SIGN", Rule::Union, DOC_SCALAR),
    ("SIN", Rule::Union, DOC_SCALAR),
    ("SINH", Rule::Union, DOC_SCALAR),
    ("SQRT", Rule::Union, DOC_SCALAR),
    ("TAN", Rule::Union, DOC_SCALAR),
    ("TANH", Rule::Union, DOC_SCALAR),
    ("TRUNC", Rule::Union, DOC_SCALAR),
    // Scalar text.
    ("CHAR", Rule::Union, DOC_SCALAR),
    ("CLEAN", Rule::Union, DOC_SCALAR),
    ("CODE", Rule::Union, DOC_SCALAR),
    ("CONCATENATE", Rule::Union, DOC_SCALAR),
    ("DOLLAR", Rule::Union, DOC_SCALAR),
    ("EXACT", Rule::Union, DOC_SCALAR),
    ("FIND", Rule::Union, DOC_SCALAR),
    ("FIXED", Rule::Union, DOC_SCALAR),
    ("LEFT", Rule::Union, DOC_SCALAR),
    ("LEN", Rule::Union, DOC_SCALAR),
    ("LOWER", Rule::Union, DOC_SCALAR),
    ("MID", Rule::Union, DOC_SCALAR),
    ("PROPER", Rule::Union, DOC_SCALAR),
    ("REPLACE", Rule::Union, DOC_SCALAR),
    ("REPT", Rule::Union, DOC_SCALAR),
    ("RIGHT", Rule::Union, DOC_SCALAR),
    ("SEARCH", Rule::Union, DOC_SCALAR),
    ("SUBSTITUTE", Rule::Union, DOC_SCALAR),
    ("T", Rule::Union, DOC_SCALAR),
    ("TEXT", Rule::Union, DOC_SCALAR),
    ("TRIM", Rule::Union, DOC_SCALAR),
    ("UPPER", Rule::Union, DOC_SCALAR),
    ("VALUE", Rule::Union, DOC_SCALAR),
    // Scalar logical / information.
    ("NOT", Rule::Union, DOC_SCALAR),
    ("TRUE", Rule::Union, DOC_SCALAR),
    ("FALSE", Rule::Union, DOC_SCALAR),
    ("ISBLANK", Rule::Union, DOC_SCALAR),
    ("ISERR", Rule::Union, DOC_SCALAR),
    ("ISERROR", Rule::Union, DOC_SCALAR),
    ("ISLOGICAL", Rule::Union, DOC_SCALAR),
    ("ISNA", Rule::Union, DOC_SCALAR),
    ("ISNONTEXT", Rule::Union, DOC_SCALAR),
    ("ISNUMBER", Rule::Union, DOC_SCALAR),
    ("ISTEXT", Rule::Union, DOC_SCALAR),
    ("N", Rule::Union, DOC_SCALAR),
    ("NA", Rule::Union, DOC_SCALAR),
    ("ERROR.TYPE", Rule::Union, DOC_SCALAR),
    // Scalar date and time.
    ("DATE", Rule::Union, DOC_SCALAR),
    ("DATEVALUE", Rule::Union, DOC_SCALAR),
    ("DAY", Rule::Union, DOC_SCALAR),
    ("DAYS360", Rule::Union, DOC_SCALAR),
    ("HOUR", Rule::Union, DOC_SCALAR),
    ("MINUTE", Rule::Union, DOC_SCALAR),
    ("MONTH", Rule::Union, DOC_SCALAR),
    ("SECOND", Rule::Union, DOC_SCALAR),
    ("TIME", Rule::Union, DOC_SCALAR),
    ("TIMEVALUE", Rule::Union, DOC_SCALAR),
    ("WEEKDAY", Rule::Union, DOC_SCALAR),
    ("YEAR", Rule::Union, DOC_SCALAR),
];

/// The rule for a function, `None` if not on the table.
pub(crate) fn rule(name: &str) -> Option<Rule> {
    TABLE.iter().find(|(n, _, _)| *n == name).map(|(_, r, _)| *r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_out_and_excludes_reference_returning_and_resizing_functions() {
        let mut names: Vec<_> = TABLE.iter().map(|e| e.0).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TABLE.len(), "no duplicates");
        for excluded in [
            "LOOKUP", "OFFSET", "INDIRECT", "CELL", "INFO", "GETPIVOTDATA", "SUBTOTAL", "AGGREGATE", "DSUM",
            "_XLFN.XLOOKUP", "_XLFN.ANCHORARRAY", "_XLFN.SINGLE", "_XLFN._XLWS.FILTER", "_XLFN.LET", "_XLFN.LAMBDA",
        ] {
            assert_eq!(rule(excluded), None, "{excluded}");
        }
        assert!(TABLE.iter().all(|(n, _, e)| n.chars().all(|c| !c.is_ascii_lowercase()) && !e.is_empty()));
    }
}
