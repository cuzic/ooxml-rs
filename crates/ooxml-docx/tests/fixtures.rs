//! T4 (refusals on fixtures) and the per-fixture run counts of ADR-0004 §6
//! (the same numbers `spikes/docx/phase0_gate.py` reports).

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::Error as CoreError;
use ooxml_docx::{Document, Error, RunContext};

fn open(stem: &str) -> Result<Document, Error> {
    Document::from_bytes(fixture(stem))
}

#[test]
fn open_time_refusals() {
    assert_matches!(open("poi-bug53475-password-is-pass"), Err(Error::Core(CoreError::EncryptedPackage)));
    assert_matches!(open("poi-ExternalEntityInText"), Err(Error::Core(CoreError::DtdNotAllowed { .. })));
    assert_matches!(open("sdk-strict-CommentRangeStart"), Err(Error::Core(CoreError::StrictNotSupported(_))));
    for stem in openable() {
        open(stem).unwrap_or_else(|e| panic!("{stem}: {e}"));
    }
}

#[test]
fn document_gates() {
    let gate = |stem| open(stem).unwrap().write_gate().unwrap_err();
    assert_matches!(gate("poi-delins"), Error::TrackRevisionsOn);
    assert_matches!(gate("poi-documentProtection_readonly_no_password"), Error::DocumentProtected { edit } if edit == "readOnly");
    assert_matches!(gate("sdk-TOC2"), Error::StaleFieldRisk { codes } if codes == ["PAGEREF", "TOC"]);
    // The outer TC (instruction holding a nested SEQ) is only found with the field stack (review M1).
    assert_matches!(gate("sdk-complex2010"), Error::StaleFieldRisk { codes } if codes == ["=", "SEQ", "TC"]);
    for stem in writable() {
        assert!(open(stem).unwrap().write_gate().is_ok(), "{stem}");
    }
}

/// T4: after a gate, reads still work, every write returns the gate error,
/// and the saved package is entry-for-entry identical to the input.
#[test]
fn gated_documents_read_but_never_write() {
    for &stem in GATED {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let expected = doc.write_gate().unwrap_err().to_string();
        assert!(doc.paragraph_count() > 0, "{stem}: reads work");
        let text: String = doc.paragraphs().map(|p| p.text()).collect();
        assert!(!text.is_empty(), "{stem}");
        let (p, r) = doc
            .paragraphs()
            .find_map(|p| p.runs().find(|r| r.check_settable().is_ok()).map(|r| (p.index(), r.index())))
            .unwrap_or((0, 0));
        assert_eq!(doc.set_run_text(p, r, "x").unwrap_err().to_string(), expected, "{stem}");
        assert_eq!(doc.append_paragraph("x").unwrap_err().to_string(), expected, "{stem}");
        assert_eq!(entries(&doc.to_bytes().unwrap()), entries(&input), "{stem}");
    }
}

/// (fixture, runs with a w:t in body flow, settable among them, settable in
/// w:hyperlink). Same numbers as `spikes/docx/phase0_gate.py` (README rev 2).
const COUNTS: &[(&str, usize, usize, usize)] = &[
    ("poi-55733", 1, 1, 0),
    ("poi-61745", 0, 0, 0),
    ("poi-Bug60341", 0, 0, 0),
    ("poi-FieldCodes", 2, 0, 0),
    ("poi-FldSimple", 1, 0, 0),
    ("poi-Headers", 0, 0, 0),
    ("poi-PageSpecificHeadFoot", 15, 15, 0),
    ("poi-bug59058", 3281, 3271, 307),
    ("poi-checkboxes", 15, 15, 0),
    ("poi-shapes-with-text", 0, 0, 0),
    ("poi-smarttag-snippet", 14, 6, 1),
    ("poi-table-alignment", 66, 66, 0),
    ("poi-testComment", 3, 3, 0),
    ("sdk-Comment010", 16, 16, 0),
    ("sdk-Field-more-field", 13, 5, 0),
    ("sdk-HelloO14", 1, 1, 0),
    ("sdk-coverpage1", 8, 0, 0),
    ("sdk-equation", 12, 12, 0),
    ("sdk-ftEdNote", 7, 7, 0),
];

#[test]
fn run_counts_match_the_spike() {
    let (mut total, mut settable) = (0, 0);
    for &(stem, text_runs, ok, in_link) in COUNTS {
        let doc = open(stem).unwrap();
        let runs: Vec<_> = doc
            .paragraphs()
            .flat_map(|p| p.runs().collect::<Vec<_>>())
            .filter(|r| r.raw().elements().any(|c| c.name().local() == "t" && c.name().ns() == W_NS))
            .collect();
        let set: Vec<_> = runs.iter().filter(|r| r.check_settable().is_ok()).collect();
        let link = set.iter().filter(|r| r.context() == RunContext::Hyperlink).count();
        assert_eq!((runs.len(), set.len(), link), (text_runs, ok, in_link), "{stem}");
        total += runs.len();
        settable += set.len();
    }
    assert_eq!(writable().count(), COUNTS.len());
    assert_eq!((total, settable), (3455, 3418));
}

fn refusals(stem: &str) -> Vec<String> {
    let doc = open(stem).unwrap();
    doc.paragraphs().flat_map(|p| p.runs().filter_map(|r| r.check_settable().err()).map(|e| format!("{e:?}")).collect::<Vec<_>>()).collect()
}

#[test]
fn per_run_refusals_on_fixtures() {
    // R1: complex fields (instruction and result), fldSimple.
    let r = refusals("poi-FieldCodes");
    assert!(!r.is_empty() && r.iter().all(|e| e == "InsideField"), "{r:?}");
    let doc = open("sdk-Field-more-field").unwrap();
    let text_runs_in_fields = doc
        .paragraphs()
        .flat_map(|p| p.runs().collect::<Vec<_>>())
        .filter(|r| r.raw().elements().any(|c| c.name().local() == "t") && r.in_field())
        .inspect(|r| assert_matches!(r.check_settable(), Err(Error::InsideField)))
        .count();
    assert_eq!(text_runs_in_fields, 8);
    assert!(refusals("poi-FldSimple").contains(&"InsideField".to_string()));
    // R3: smart tags and content controls.
    let doc = open("poi-smarttag-snippet").unwrap();
    let smart: Vec<_> = doc.paragraphs().flat_map(|p| p.runs().collect::<Vec<_>>()).filter(|r| r.context() == RunContext::SmartTag).collect();
    assert_eq!(smart.len(), 8);
    for r in smart {
        assert_matches!(r.check_settable(), Err(Error::UnsupportedContext { ancestor: "w:smartTag" }));
    }
    let doc = open("sdk-coverpage1").unwrap();
    let runs: Vec<_> = doc.paragraphs().flat_map(|p| p.runs().collect::<Vec<_>>()).collect();
    assert!(!runs.is_empty());
    for r in runs {
        assert_eq!(r.context(), RunContext::ContentControl);
        assert_matches!(r.check_settable(), Err(Error::UnsupportedContext { ancestor: "w:sdt" }));
    }
    // R4: runs holding mc:AlternateContent (shapes with text boxes).
    let r = refusals("poi-shapes-with-text");
    assert!(r.iter().any(|e| e.contains("RunNotTextOnly") && e.contains("mc:AlternateContent")), "{r:?}");
}

#[test]
fn append_into_body_with_alternate_content_is_refused() {
    let input = fixture("sdk-HelloO14");
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(doc.append_paragraph("x"), Err(Error::Core(CoreError::UnresolvedSibling(_))));
    assert_eq!(entries(&doc.to_bytes().unwrap()), entries(&input));
}

/// R2 on a real file: `poi-delins` with track changes switched off.
#[test]
fn runs_inside_revisions_are_refused() {
    let input = fixture("poi-delins");
    let settings = String::from_utf8(part(&input, "word/settings.xml")).unwrap();
    let off = settings.replace("<w:trackRevisions/>", "");
    assert_ne!(off, settings);
    let doc = Document::from_bytes(with_part(&input, "word/settings.xml", off.as_bytes())).unwrap();
    // Its HYPERLINK fields have tracked edits in their instructions
    // (w:delInstrText), which W2 does not classify (fail closed). The
    // per-run checks below do not depend on the document gates.
    assert_matches!(doc.write_gate(), Err(Error::StaleFieldRisk { codes }) if codes == [ooxml_docx::fields::REVISED_INSTRUCTION]);
    let mut seen = std::collections::BTreeSet::new();
    for p in doc.paragraphs() {
        for r in p.runs() {
            if let ctx @ (RunContext::Inserted | RunContext::Deleted) = r.context() {
                seen.insert(format!("{ctx:?}"));
                if !r.in_field() {
                    assert_matches!(r.check_settable(), Err(Error::InsideRevision { context }) if context == ctx);
                }
            }
        }
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), ["Deleted", "Inserted"]);
    // Deleted text is not part of the paragraph text; inserted text is.
    for p in doc.paragraphs() {
        let visible: String = p
            .runs()
            .filter(|r| !matches!(r.context(), RunContext::Deleted | RunContext::MovedFrom))
            .map(|r| r.text())
            .collect();
        assert_eq!(p.text(), visible);
    }
}
