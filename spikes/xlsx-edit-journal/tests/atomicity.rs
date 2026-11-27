//! ADR-0024 Test oracle 3: `Err` ⇒ byte-identical, call atomicity, and
//! "accepted ⇒ compiles" under injected Commit faults. Oracle 8's
//! truncation cases for keyed ops.

mod common;

use common::*;
use xlsx_edit_journal_spike::fixture::oracle_workbook;
use xlsx_edit_journal_spike::{Api, Content, Fault, JError, Journal, Op, Patch};

fn shadow_bytes(j: &mut Journal) -> Vec<u8> {
    j.shadow().unwrap().to_bytes().unwrap()
}

fn seeded() -> Journal {
    let mut j = Journal::open(oracle_workbook("A1:C5")).unwrap();
    for op in [content(k("A1"), Content::Number(5.0)), patch(k("B5"), Patch::Fill(red())), content(k("C1"), Content::Formula("A2+1".into()))] {
        j.append(Api::Native, op, None).unwrap();
    }
    j
}

#[test]
fn a_refused_one_op_call_changes_nothing() {
    for bad in [
        content(k("C4"), Content::Number(1.0)),                  // MergedCell
        content(k("A1"), Content::Formula("SUM(".into())),       // InvalidFormula
        patch(k("A1"), Patch::Index(99)),                        // StyleIndexOutOfRange
        content(k("A1"), Content::Number(f64::NAN)),             // InvalidNumber
        content(k("A1"), Content::Error("#BAD".into())),         // InvalidValue
        Op::Clear(k("A1")),                                      // Unsupported (P1)
    ] {
        let mut j = seeded();
        let (state, shadow) = (j.__state(), shadow_bytes(&mut j));
        let replays = j.replays;
        assert!(j.append(Api::Native, bad.clone(), None).is_err(), "{bad:?}");
        assert_eq!(j.__state(), state, "{bad:?}");
        assert_eq!(j.replays, replays, "a one-op refusal needs no replay");
        assert_eq!(shadow_bytes(&mut j), shadow, "{bad:?}");
    }
}

#[test]
fn a_call_refused_part_way_is_undone_by_replay() {
    let mut j = seeded();
    let (state, shadow) = (j.__state(), shadow_bytes(&mut j));
    let call = vec![
        (content(k("A2"), Content::Number(7.0)), None),
        (patch(k("A2"), Patch::Font(italic())), None),
        (content(k("C4"), Content::Number(1.0)), None), // refused: merged
    ];
    let e = j.append_call(Api::Native, call).unwrap_err();
    assert!(matches!(e, JError::Xlsx(ooxml_xlsx::Error::MergedCell { .. })), "{e:?}");
    assert_eq!(j.__state(), state);
    assert!(j.shadow_is_stale() || cfg!(feature = "mutant_keep_shadow"));
    assert_eq!(shadow_bytes(&mut j), shadow, "shadow restored");
    // the journal still commits to the seeded state
    let (out, _) = j.commit().unwrap();
    let mut clean = seeded();
    assert_eq!(out, clean.commit().unwrap().0);
}

#[test]
fn a_poisoned_shadow_apply_is_journal_internal_and_rebuilt() {
    let mut j = seeded();
    let (state, shadow) = (j.__state(), shadow_bytes(&mut j));
    j.__shadow_if_live().unwrap().inject_apply_failure(ooxml_xlsx::ApplyStep::Target);
    let e = j.append(Api::Native, content(k("A2"), Content::Number(1.0)), None).unwrap_err();
    assert!(matches!(e, JError::JournalInternal { .. }), "{e:?}");
    assert_eq!(j.__state(), state);
    assert_eq!(shadow_bytes(&mut j), shadow);
    j.append(Api::Native, content(k("A2"), Content::Number(1.0)), None).unwrap();
}

#[test]
fn commit_faults_leave_base_and_log_and_a_retry_gives_the_same_output() {
    let expected = seeded().commit().unwrap().0;
    let faults = [Fault::Normalize, Fault::OpenWorkingCopy, Fault::Batch(0), Fault::PanicInBatch(0), Fault::Serialize, Fault::Swap];
    for f in faults {
        let mut j = seeded();
        let (state, base) = (j.__state(), j.base().clone());
        j.fault = Some(f);
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let r = j.commit();
        std::panic::set_hook(prev);
        assert!(matches!(r, Err(JError::JournalInternal { .. })), "{f:?}: {r:?}");
        assert_eq!(j.__state(), state, "{f:?}: log unchanged");
        assert_eq!(j.base(), &base, "{f:?}: Base unchanged");
        j.fault = None;
        assert_eq!(j.commit().unwrap().0, expected, "{f:?}: retry");
        // and the next save of an empty Journal is the new Base
        assert_eq!(j.commit().unwrap().0, expected);
    }
}

#[test]
fn truncation_between_repeated_writes_and_style_chains() {
    let base = oracle_workbook("A1:C5");
    let mut j = Journal::open(base.clone()).unwrap();
    let s1 = j.append(Api::Native, content(k("A2"), Content::Number(1.0)), None).unwrap();
    j.append(Api::Native, content(k("A2"), Content::Number(2.0)), None).unwrap();
    j.append(Api::Native, patch(k("A2"), Patch::Fill(red())), None).unwrap();
    j.append(Api::Native, patch(k("A2"), Patch::Format(Box::new(bold()))), None).unwrap();
    j.truncate(s1).unwrap();
    assert_eq!(j.normalized(k("A2")).unwrap().unwrap().value, ooxml_xlsx::CellValue::Number(1.0));
    let (out, _) = j.commit().unwrap();
    let never = journaled(&base, &[content(k("A2"), Content::Number(1.0))]);
    assert_eq!(out, never.bytes, "oracle 8: byte-equal to a Journal without the later ops");
    // below the commit mark: refused, nothing changes
    j.append(Api::Native, content(k("A2"), Content::Number(3.0)), None).unwrap();
    let state = j.__state();
    assert!(matches!(j.truncate(s1 - 1), Err(JError::TruncateBeforeCommit { .. })));
    assert_eq!(j.__state(), state);
}
