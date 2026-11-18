//! ADR-0008 §1 resolver tests: source distributions against the spike's
//! independent Python model (`spikes/pptx-phase1/chain.py`), named fixture
//! values, and synthetic one-rule decks (§7 O2: the edit puts each value at
//! exactly one chain level, so the expected source is known by
//! construction).

#[path = "oracle/cases.rs"]
mod cases;

use std::collections::BTreeMap;

use ooxml_core::Package;
use ooxml_pptx::{
    Autofit, Bullet, Fill, Lookup, MatchKind, Presentation, Reason, RunItem, ShapeKind, Source, TextItem, TextStyleKind,
};

fn src_name(s: Option<&Source<'_>>, direct: &str) -> String {
    match s {
        None => "None".into(),
        Some(Source::Direct) => direct.into(),
        Some(Source::ParagraphDefault) => "paragraph.defRPr".into(),
        Some(Source::ShapeListStyle { .. }) => "shape.lstStyle".into(),
        Some(Source::ShapeStyle) => "shape.style.fontRef".into(),
        Some(Source::Placeholder { role: ooxml_pptx::Role::Layout, .. }) => "layout.ph.lstStyle".into(),
        Some(Source::Placeholder { role: ooxml_pptx::Role::Master, .. }) => "master.ph.lstStyle".into(),
        Some(Source::Placeholder { role: ooxml_pptx::Role::NotesMaster, .. }) => "notesMaster.ph.lstStyle".into(),
        Some(Source::MasterTextStyle { style: TextStyleKind::Notes, .. }) => "notesMaster.notesStyle".into(),
        Some(Source::MasterTextStyle { style, .. }) => format!(
            "master.txStyles.{}",
            match style {
                TextStyleKind::Title => "titleStyle",
                TextStyleKind::Body => "bodyStyle",
                TextStyleKind::Other => "otherStyle",
                _ => "other",
            }
        ),
        Some(Source::PresentationDefault { .. }) => "presentation.defaultTextStyle".into(),
        Some(_) => "other".into(),
    }
}

fn geo_name(s: Option<&Source<'_>>) -> &'static str {
    match s {
        None => "None",
        Some(Source::Direct) => "slide",
        Some(Source::Placeholder { role: ooxml_pptx::Role::Layout, .. }) => "layout",
        Some(_) => "master",
    }
}

/// Counters named as `chain.py` names them, for slide `p:sp` text and
/// slide placeholders (AC entries and table cells excluded, as there).
fn count(bytes: &[u8], c: &mut BTreeMap<String, usize>) {
    let Ok(mut pres) = Presentation::from_bytes(bytes.to_vec()) else { return };
    let Ok(deck) = pres.deck() else { return };
    for i in 0..deck.slide_count() {
        let Ok(s) = deck.slide(i) else { continue };
        count_notes(&s, c);
        if s.chain_parts().is_err() {
            continue;
        }
        for sh in s.slide().shapes() {
            if *sh.kind() == ShapeKind::AlternateContent {
                continue;
            }
            let m = s.placeholder_match(&sh).unwrap();
            let kind = if m.is_some() { "ph" } else { "nonph" };
            if let Some(m) = &m {
                *c.entry("ph.slide".into()).or_default() += 1;
                let k = match m.kind {
                    MatchKind::Idx => "idx",
                    MatchKind::IdxFamily => "idx(family)",
                    MatchKind::IdxTieType => "idx+type",
                    MatchKind::IdxTieFamily => "idx+family",
                    MatchKind::Type => "type",
                    MatchKind::Family => "family",
                    MatchKind::Orphan => "orphan",
                    _ => "other",
                };
                *c.entry(format!("match.layout.{k}")).or_default() += 1;
                let g = s.geometry(&sh).unwrap();
                *c.entry(format!("geometry.from_{}", geo_name(g.source()))).or_default() += 1;
            }
            let Some(body) = sh.text_body() else { continue };
            for para in body.paragraphs() {
                let pp = s.para_props(&sh, body, para).unwrap();
                *c.entry(format!("{kind}.para.algn.from_{}", src_name(pp.align.source(), "paragraph"))).or_default() += 1;
                *c.entry(format!("{kind}.para.marL.from_{}", src_name(pp.margin_left.source(), "paragraph"))).or_default() += 1;
                *c.entry(format!("{kind}.para.bullet.from_{}", src_name(pp.bullet.source(), "paragraph"))).or_default() += 1;
                for it in para.items() {
                    let item = match it {
                        TextItem::Run(r) => RunItem::Run(r),
                        TextItem::Field(f) => RunItem::Field(f),
                        _ => continue,
                    };
                    let rp = s.run_props(&sh, body, para, item).unwrap();
                    *c.entry(format!("{kind}.runs")).or_default() += 1;
                    *c.entry(format!("{kind}.run.sz.from_{}", src_name(rp.size.source(), "run"))).or_default() += 1;
                    *c.entry(format!("{kind}.run.b.from_{}", src_name(rp.bold.source(), "run"))).or_default() += 1;
                }
            }
        }
    }
}

/// The notes counters of `chain.py` (ADR-0008 §1.3 "Notes", §1.7): body
/// placeholders of the notes shape tree (group members included) and the
/// source of each run's `sz`.
fn count_notes(s: &ooxml_pptx::DeckSlide<'_, '_>, c: &mut BTreeMap<String, usize>) {
    let Some(n) = s.notes_sheet() else { return };
    *c.entry("notes.slide".into()).or_default() += 1;
    if n.master().is_err() {
        *c.entry("notes.no_notes_master".into()).or_default() += 1;
        return;
    }
    for sh in n.shapes() {
        if *sh.kind() != ShapeKind::Shape || sh.placeholder().is_none_or(|ph| ph.view().0.attr(ooxml_core::QName::new("", "type")) != Some("body")) {
            continue;
        }
        *c.entry("notes.body_ph".into()).or_default() += 1;
        let m = n.placeholder_match(&sh).unwrap().unwrap();
        if m.master.is_none() {
            *c.entry("notes.body_ph_without_master_body".into()).or_default() += 1;
            continue;
        }
        let Some(body) = sh.text_body() else { continue };
        for para in body.paragraphs() {
            for it in para.items() {
                let TextItem::Run(r) = it else { continue };
                let rp = n.run_props(&sh, body, para, RunItem::Run(r)).unwrap();
                *c.entry(format!("notes.run.sz.from_{}", src_name(rp.size.source(), "run"))).or_default() += 1;
            }
        }
    }
}

const COMPARED: &[&str] = &["notes.", "ph.slide", "ph.runs", "nonph.runs", "match.layout.", "geometry.from_", "ph.run.sz.", "nonph.run.sz.", "ph.run.b.", "nonph.run.b.", "ph.para.", "nonph.para."];

/// Opt-in: `OOXML_PPTX_CORPUS=dir:dir cargo test -p ooxml-pptx --test resolve -- --ignored`.
/// With the 633-file corpus of `spikes/pptx-phase1/README.md`, every
/// compared counter must equal `spikes/pptx-phase1/results/chain-633.txt`
/// (the Python model of the same rules); with another corpus it prints.
#[test]
#[ignore = "needs OOXML_PPTX_CORPUS"]
fn corpus_matches_the_spike_model() {
    let Ok(dirs) = std::env::var("OOXML_PPTX_CORPUS") else { return };
    let mut c = BTreeMap::new();
    let mut files = 0;
    for d in dirs.split(':') {
        let mut entries: Vec<_> = std::fs::read_dir(d).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries.into_iter().filter(|p| p.extension().is_some_and(|e| e == "pptx")) {
            files += 1;
            count(&std::fs::read(&p).unwrap(), &mut c);
        }
    }
    let spike = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../spikes/pptx-phase1/results/chain-633.txt")).unwrap();
    let mut model: BTreeMap<String, usize> = BTreeMap::new();
    for line in spike.lines().filter(|l| !l.starts_with(' ')) {
        let mut it = line.split_whitespace();
        if let (Some(k), Some(v)) = (it.next(), it.next())
            && COMPARED.iter().any(|p| k.starts_with(p))
            && !k.contains("idx_duplicate")
            && !k.contains("idx_type_differs")
            && !k.contains("implicit_idx")
            && !k.starts_with("ph.slide.on_")
            && let Ok(v) = v.parse()
        {
            model.insert(k.to_owned(), v);
        }
    }
    let ours: BTreeMap<String, usize> = c.into_iter().filter(|(k, _)| COMPARED.iter().any(|p| k.starts_with(p))).collect();
    eprintln!("{files} files");
    let mut diffs = Vec::new();
    for k in model.keys().chain(ours.keys()).collect::<std::collections::BTreeSet<_>>() {
        let (a, b) = (model.get(k).copied().unwrap_or(0), ours.get(k).copied().unwrap_or(0));
        if a != b {
            diffs.push(format!("{k}: spike {a}, ooxml-pptx {b}"));
        }
    }
    eprintln!("{} counters compared, {} differ", model.len(), diffs.len());
    for d in &diffs {
        eprintln!("  {d}");
    }
    if files == 633 {
        assert!(diffs.is_empty(), "the resolver and the spike model disagree");
    }
}

/// Every fixture resolves without an error on every run, paragraph, body
/// and placeholder (and reading never dirties the package).
#[test]
fn every_fixture_resolves() {
    let mut c = BTreeMap::new();
    for f in cases::FIXTURES {
        let bytes = cases::read_fixture(f);
        count(&bytes, &mut c);
        let mut pres = Presentation::from_bytes(bytes).unwrap();
        let before = pres.to_bytes().unwrap();
        {
            let deck = pres.deck().unwrap();
            for i in 0..deck.slide_count() {
                let Ok(s) = deck.slide(i) else { continue };
                for sh in s.slide().shapes() {
                    if s.chain_parts().is_ok() {
                        s.body_props(&sh).unwrap();
                        s.outline(&sh).unwrap();
                    }
                }
                let _ = (s.links(), s.comments(), s.transition(), s.timing());
            }
            let _ = deck.sections();
        }
        assert_eq!(pres.to_bytes().unwrap(), before, "{f}: reading changed the package");
    }
    assert!(c["ph.slide"] > 50 && c["ph.runs"] > 100, "{c:?}");
}

#[test]
fn sample_title_and_body() {
    let mut pres = Presentation::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let deck = pres.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let shapes = s.slide().shapes();
    let title = shapes.iter().find(|x| x.name() == Some("Rectangle 2")).unwrap();
    let m = s.placeholder_match(title).unwrap().unwrap();
    assert_eq!(m.kind, MatchKind::Idx);
    let g = s.geometry(title).unwrap();
    assert!(matches!(g.source(), Some(Source::Placeholder { role: ooxml_pptx::Role::Layout, .. })), "{g:?}");
    let body = title.text_body().unwrap();
    let para = body.paragraphs()[0];
    let TextItem::Run(r) = para.items()[0] else { panic!() };
    let rp = s.run_props(title, body, para, RunItem::Run(r)).unwrap();
    assert_eq!(rp.size.value(), Some(&4400));
    assert!(matches!(rp.size.source(), Some(Source::MasterTextStyle { style: TextStyleKind::Title, .. })));
    // tx1 through the master's clrMap to dk1 of the theme
    let Some(Fill::Solid(col)) = rp.fill.value() else { panic!("{:?}", rp.fill) };
    assert_eq!((col.val, col.theme_slot.map(|t| t.as_str())), (Some("tx1"), Some("dk1")));
    assert_eq!(col.theme_slot, Some(ooxml_core::EnumValue::Known(ooxml_schema::dml::ColorSchemeIndexValues::Dark1)));
}

// ---------------------------------------------------------------------------
// Synthetic one-rule decks (§7 O2). SYNTHETIC: poi/sample.pptx with one
// documented XML edit, in memory.

fn patched(edits: &[(&str, &str, &str)]) -> Presentation {
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    for (part, from, to) in edits {
        let x = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
        assert!(x.contains(from), "{part}: {from:?} not found");
        pkg.set_part(part, x.replacen(from, to, 1).into_bytes()).unwrap();
    }
    Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

const SLIDE: &str = "ppt/slides/slide1.xml";
const TITLE_RUN: &str = r#"<a:r><a:rPr lang="en-US"><a:sym typeface="Wingdings" pitchFamily="2" charset="2"/></a:rPr><a:t>Lorem ipsum dolor sit amet</a:t></a:r>"#;

/// Resolves the title run of slide 1.
fn title_run<T>(pres: &mut Presentation, f: impl FnOnce(ooxml_pptx::RunProps<'_>, ooxml_pptx::ParaProps<'_>) -> T) -> T {
    let deck = pres.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let shapes = s.slide().shapes();
    let title = shapes.iter().find(|x| x.name() == Some("Rectangle 2")).unwrap();
    let body = title.text_body().unwrap();
    let para = body.paragraphs()[0];
    let TextItem::Run(r) = para.items()[0] else { panic!() };
    let rp = s.run_props(title, body, para, RunItem::Run(r)).unwrap();
    let pp = s.para_props(title, body, para).unwrap();
    f(rp, pp)
}

/// S1: steps 1, 2, 3 and 4 each win when set.
#[test]
fn s1_chain_steps() {
    let mut p = patched(&[(SLIDE, TITLE_RUN, r#"<a:r><a:rPr lang="en-US" sz="1111"/><a:t>x</a:t></a:r>"#)]);
    title_run(&mut p, |rp, _| {
        assert_eq!(rp.size.value(), Some(&1111));
        assert_eq!(rp.size.source(), Some(&Source::Direct));
    });
    let mut p = patched(&[(SLIDE, r#"<a:pPr hangingPunct="1"/>"#, r#"<a:pPr hangingPunct="1"><a:defRPr sz="2222"/></a:pPr>"#)]);
    title_run(&mut p, |rp, _| assert_eq!((rp.size.value(), rp.size.source()), (Some(&2222), Some(&Source::ParagraphDefault))));
    let mut p = patched(&[(SLIDE, "<a:lstStyle/><a:p><a:pPr hangingPunct", r#"<a:lstStyle><a:lvl1pPr algn="r"><a:defRPr sz="3333"/></a:lvl1pPr></a:lstStyle><a:p><a:pPr hangingPunct"#)]);
    title_run(&mut p, |rp, pp| {
        assert_eq!((rp.size.value(), rp.size.source()), (Some(&3333), Some(&Source::ShapeListStyle { level: 0 })));
        assert_eq!(pp.align.value().map(|v| v.as_str()), Some("r"));
    });
    // step 4 on a placeholder (review M2): own p:style/a:fontRef wins over the master
    let mut p = patched(&[(SLIDE, "<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr hangingPunct", r#"<p:spPr/><p:style><a:lnRef idx="0"><a:schemeClr val="accent1"/></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></p:style><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr hangingPunct"#)]);
    title_run(&mut p, |rp, _| {
        assert_eq!(rp.fill.source(), Some(&Source::ShapeStyle));
        let Some(Fill::Solid(c)) = rp.fill.value() else { panic!() };
        assert_eq!(c.val, Some("lt1"));
        assert_eq!(rp.latin.source(), Some(&Source::ShapeStyle));
        assert_eq!(rp.latin.value().and_then(|f| f.theme_ref), Some("+mn-lt"));
    });
}

/// S9 / r2-m9: an invalid level stops at the first level-dependent step.
#[test]
fn s9_invalid_level() {
    let mut p = patched(&[(SLIDE, r#"<a:pPr hangingPunct="1"/>"#, r#"<a:pPr hangingPunct="1" lvl="12"/>"#)]);
    title_run(&mut p, |rp, pp| {
        assert!(pp.level.is_err());
        assert_eq!(rp.size, Lookup::Unresolved(Reason::InvalidLevel));
    });
}

/// S10 / M1: an XSD default never stops the chain (cap="all" on the master
/// title style reaches a run whose a:rPr has no cap).
#[test]
fn s10_presence_not_defaults() {
    let mut p = patched(&[("ppt/slideMasters/slideMaster1.xml", r#"<a:defRPr sz="4400"><a:solidFill>"#, r#"<a:defRPr sz="4400" cap="all"><a:solidFill>"#)]);
    title_run(&mut p, |rp, _| {
        assert_eq!(rp.caps.value().map(|v| v.as_str()), Some("all"), "{:?}", rp.caps);
    });
}

/// S7 / M3: an empty theme ea typeface is ScriptDependent, never "".
#[test]
fn s7_script_dependent_fonts() {
    let mut p = patched(&[(SLIDE, TITLE_RUN, r#"<a:r><a:rPr lang="ja-JP"><a:ea typeface="+mn-ea"/></a:rPr><a:t>x</a:t></a:r>"#)]);
    let theme = {
        let pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
        String::from_utf8(pkg.read_part("ppt/theme/theme1.xml").unwrap()).unwrap()
    };
    // the theme's minor ea is empty (sample.pptx; asserted, not branched on: #56 m6)
    let minor = &theme[theme.find("<a:minorFont>").unwrap()..];
    assert!(minor.starts_with(r#"<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/>"#), "{}", &minor[..120]);
    let jpan = minor.split(r#"<a:font script="Jpan" typeface=""#).nth(1).unwrap().split('"').next().unwrap().to_owned();
    title_run(&mut p, |rp, _| {
        let Lookup::Unresolved(Reason::ScriptDependent { reference, .. }) = &rp.east_asian else { panic!("{:?}", rp.east_asian) };
        assert_eq!(*reference, "+mn-ea");
        // Q4 helper: a candidate by lang, never a Set value
        assert_eq!(rp.script_font_candidate(&rp.east_asian), Some(("Jpan", jpan.as_str())));
        assert_eq!(rp.script_font_candidate(&rp.latin), None);
    });
}

/// S6: the slide's colour-map override is used for scheme colours.
#[test]
fn s6_slide_color_map_override() {
    let mut p = patched(&[(SLIDE, "<a:masterClrMapping/>", r#"<a:overrideClrMapping bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#)]);
    title_run(&mut p, |rp, _| {
        let Some(Fill::Solid(c)) = rp.fill.value() else { panic!("{:?}", rp.fill) };
        assert_eq!(c.theme_slot.map(|t| t.as_str()), Some("lt1"));
        assert_eq!(c.map_source, Some(ooxml_pptx::MapSource::SlideOverride));
    });
}

/// S12 / r2-m1: a slide <a:normAutofit/> means 100 %, defaulted, even under
/// a layout fontScale.
#[test]
fn s12_autofit_group_winner() {
    let mut p = patched(&[(SLIDE, "<p:spPr/><p:txBody><a:bodyPr/>", "<p:spPr/><p:txBody><a:bodyPr><a:normAutofit/></a:bodyPr>")]);
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let shapes = s.slide().shapes();
    let title = shapes.iter().find(|x| x.name() == Some("Rectangle 2")).unwrap();
    let b = s.body_props(title).unwrap();
    let Some(Autofit::Normal { font_scale, .. }) = b.autofit.value() else { panic!("{:?}", b.autofit) };
    let Lookup::Set(r) = font_scale else { panic!() };
    assert!(r.defaulted);
    assert_eq!(r.value.as_ref().unwrap().percent(), 100.0);
}

/// Bullet group winner: buAutoNum without startAt is 1, defaulted.
#[test]
fn bullet_group_winner() {
    let mut p = patched(&[(SLIDE, r#"<a:pPr hangingPunct="1"/>"#, r#"<a:pPr hangingPunct="1"><a:buAutoNum type="arabicPeriod"/></a:pPr>"#)]);
    title_run(&mut p, |_, pp| {
        let Lookup::Set(r) = &pp.bullet else { panic!() };
        let Bullet::AutoNum { scheme, start_at } = r.value.as_ref().unwrap() else { panic!() };
        assert_eq!((scheme.as_str(), start_at.get(), start_at.defaulted), ("arabicPeriod", Some(1), true));
        assert_eq!(*scheme, ooxml_core::EnumValue::Known(ooxml_schema::dml::TextAutoNumberSchemeValues::ArabicPeriod));
        assert!(!r.defaulted);
    });
}

/// S6b / M6: the slide keeps a:masterClrMapping, the layout overrides.
#[test]
fn s6b_layout_color_map_override() {
    let mut p = patched(&[("ppt/slideLayouts/slideLayout1.xml", "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>", r#"<p:clrMapOvr><a:overrideClrMapping bg1="lt1" tx1="lt1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr>"#)]);
    title_run(&mut p, |rp, _| {
        let Some(Fill::Solid(c)) = rp.fill.value() else { panic!("{:?}", rp.fill) };
        assert_eq!((c.theme_slot.map(|t| t.as_str()), c.map_source), (Some("lt1"), Some(ooxml_pptx::MapSource::LayoutOverride)));
    });
}

fn title_body_props<T>(p: &mut Presentation, f: impl FnOnce(ooxml_pptx::BodyProps<'_>, Lookup<'_, ooxml_pptx::Xfrm>, Option<ooxml_pptx::PlaceholderMatch<'_>>) -> T) -> T {
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let shapes = s.slide().shapes();
    let title = shapes.iter().find(|x| x.name() == Some("Rectangle 2")).unwrap();
    f(s.body_props(title).unwrap(), s.geometry(title).unwrap(), s.placeholder_match(title).unwrap())
}

/// S12b / §1.6: a normAutofit scale reached from the layout is Unresolved.
#[test]
fn s12b_inherited_autofit_scale() {
    let mut p = patched(&[("ppt/slideLayouts/slideLayout1.xml", "<a:bodyPr/>", r#"<a:bodyPr><a:normAutofit fontScale="90000"/></a:bodyPr>"#)]);
    title_body_props(&mut p, |b, _, _| {
        let Some(Autofit::Normal { font_scale, .. }) = b.autofit.value() else { panic!("{:?}", b.autofit) };
        assert!(matches!(font_scale, Lookup::Unresolved(Reason::InheritedAutofitScale { .. })), "{font_scale:?}");
    });
}

/// §1.2: with an explicit idx pointing at another family, the master
/// placeholder follows the layout placeholder's family; txStyles follows
/// the slide's own type.
#[test]
fn explicit_idx_of_another_family() {
    let mut p = patched(&[(SLIDE, r#"<p:ph type="subTitle" idx="1"/>"#, r#"<p:ph type="title" idx="1"/>"#)]);
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let shapes = s.slide().shapes();
    let sub = shapes.iter().find(|x| x.name() == Some("Rectangle 3")).unwrap();
    let m = s.placeholder_match(sub).unwrap().unwrap();
    assert_eq!(m.kind, MatchKind::IdxTypeMismatch);
    assert_eq!(m.family, ooxml_pptx::Family::Title);
    let master_ty = m.master.and_then(|n| {
        let nv = n.elements().find(|c| c.name().local().starts_with("nv"))?;
        let nvpr = nv.elements().find(|c| c.name().local() == "nvPr")?;
        nvpr.elements().find(|c| c.name().local() == "ph").map(|ph| ph.attr(ooxml_core::QName::new("", "type")).unwrap_or("obj").to_owned())
    });
    assert_eq!(master_ty.as_deref(), Some("body"), "master placeholder by the layout placeholder's family");
}

/// §1.5 / m5b: a placeholder in a group does not inherit geometry.
#[test]
fn group_member_placeholder_geometry() {
    let mut p = patched(&[(
        SLIDE,
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2049" name="Rectangle 2"/>"#,
        r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="99" name="G"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2049" name="Rectangle 2"/>"#,
    ), (SLIDE, "</p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"2050\"", "</p:txBody></p:sp></p:grpSp><p:sp><p:nvSpPr><p:cNvPr id=\"2050\"")]);
    title_body_props(&mut p, |_, g, m| {
        assert!(m.is_some());
        assert_eq!(g, Lookup::Unresolved(Reason::GroupCoordinates));
    });
}

/// §1.3 step 4: fontRef idx="none" supplies no typeface (only its colour).
#[test]
fn font_ref_none() {
    let mut p = patched(&[(SLIDE, "<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr hangingPunct", r#"<p:spPr/><p:style><a:lnRef idx="0"><a:schemeClr val="accent1"/></a:lnRef><a:fillRef idx="0"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="none"><a:schemeClr val="accent2"/></a:fontRef></p:style><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr hangingPunct"#)]);
    title_run(&mut p, |rp, _| {
        assert_ne!(rp.latin.source(), Some(&Source::ShapeStyle), "{:?}", rp.latin);
        assert_eq!(rp.fill.source(), Some(&Source::ShapeStyle));
    });
}

/// Table-cell text stops after the cell's own list style (TableStyle).
#[test]
fn table_cells_stop_at_table_style() {
    let mut p = Presentation::from_bytes(cases::read_fixture("poi/with_japanese.pptx")).unwrap();
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let (mut table_style, mut total) = (0, 0);
    for sh in s.slide().shapes() {
        let Some(t) = sh.table() else { continue };
        for cell in t.rows().into_iter().flatten() {
            let Some(b) = cell.text_body() else { continue };
            for para in b.paragraphs() {
                for it in para.items() {
                    let TextItem::Run(r) = it else { continue };
                    total += 1;
                    match s.run_props(&sh, b, para, RunItem::Run(r)).unwrap().bold {
                        Lookup::Unresolved(Reason::TableStyle) => table_style += 1,
                        Lookup::Set(r) => assert!(matches!(r.source, Source::Direct | Source::ParagraphDefault | Source::ShapeListStyle { .. }), "{:?}", r.source),
                        other => panic!("{other:?}"),
                    }
                }
            }
        }
    }
    assert!(total > 0 && table_style > 0, "{table_style}/{total}");
}


// ---------------------------------------------------------------------------
// Review #56 round 1: m1 (synthetic S2/S3/S4, survivors Y5/Y6/Y11/Y14/Y16/Y18),
// m2 (group geometry), m3 (WinnerAttr, AttrError), M1 (new APIs).

const SLIDE2: &str = "ppt/slides/slide2.xml";
const MASTER: &str = "ppt/slideMasters/slideMaster1.xml";
const LOREM: &str = r#"<a:p><a:pPr hangingPunct="1"/><a:r><a:rPr lang="da-DK"/><a:t>Lorem</a:t></a:r></a:p>"#;

/// Resolves run 0 of every paragraph of the slide-2 shape named `name`.
fn slide2_runs(p: &mut Presentation, name: &str, f: impl Fn(usize, ooxml_pptx::RunProps<'_>, ooxml_pptx::ParaProps<'_>)) {
    let deck = p.deck().unwrap();
    let s = deck.slide(1).unwrap();
    let shapes = s.slide().shapes();
    let sh = shapes.iter().find(|x| x.name() == Some(name)).unwrap_or_else(|| panic!("{name}"));
    let b = sh.text_body().unwrap();
    for (k, para) in b.paragraphs().into_iter().enumerate() {
        let Some(item) = para.items().into_iter().find_map(|i| match i {
            TextItem::Run(r) => Some(RunItem::Run(r)),
            TextItem::Field(f) => Some(RunItem::Field(f)),
            _ => None,
        }) else {
            continue;
        };
        f(k, s.run_props(sh, b, para, item).unwrap(), s.para_props(sh, b, para).unwrap());
    }
}

/// S2: levels 0–8 read exactly their own bodyStyle level; lvl="9" is
/// invalid; a missing level is not filled from a lower one (Q3).
#[test]
fn s2_levels() {
    let paras: String = (0..=9).map(|l| format!(r#"<a:p><a:pPr lvl="{l}"/><a:r><a:rPr lang="da-DK"/><a:t>L{l}</a:t></a:r></a:p>"#)).collect();
    let mut p = patched(&[(SLIDE2, LOREM, &paras)]);
    let want = [3200, 2800, 2400, 2000, 2000, 2000, 2000, 2000, 2000];
    slide2_runs(&mut p, "Rectangle 3", |k, rp, pp| {
        if k > 9 {
            return;
        }
        if k == 9 {
            assert!(pp.level.is_err());
            assert_eq!(rp.size, Lookup::Unresolved(Reason::InvalidLevel));
            return;
        }
        assert_eq!(rp.size.value(), Some(&want[k]), "level {k}");
        assert_eq!(rp.size.source(), Some(&Source::MasterTextStyle { part: MASTER, style: TextStyleKind::Body, level: k as u8 }), "level {k}");
    });
    // the bodyStyle's lvl2pPr removed: level 1 does not borrow level 0's size
    let master = String::from_utf8(Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap().read_part(MASTER).unwrap()).unwrap();
    let body = &master[master.find("<p:bodyStyle>").unwrap()..];
    let (s2, e2) = (body.find("<a:lvl2pPr").unwrap(), body.find("</a:lvl2pPr>").unwrap() + "</a:lvl2pPr>".len());
    let lvl2 = body[s2..e2].to_owned();
    let mut p = patched(&[(SLIDE2, LOREM, &paras), (MASTER, &lvl2, "")]);
    slide2_runs(&mut p, "Rectangle 3", |k, rp, _| {
        if k == 1 {
            assert!(!matches!(rp.size.source(), Some(Source::MasterTextStyle { style: TextStyleKind::Body, .. })), "{:?}", rp.size);
        }
    });
}

fn raw_id(n: &ooxml_core::RawNode) -> Option<u32> {
    let nv = n.elements().find(|c| c.name().local().starts_with("nv"))?;
    nv.elements().find(|c| c.name().local() == "cNvPr")?.attr(ooxml_core::QName::new("", "id"))?.parse().ok()
}

/// (kind, ambiguous, candidates, layout placeholder id, master placeholder id)
fn slide2_match(p: &mut Presentation, name: &str) -> (MatchKind, bool, usize, Option<u32>, Option<u32>) {
    let deck = p.deck().unwrap();
    let s = deck.slide(1).unwrap();
    let shapes = s.slide().shapes();
    let sh = shapes.iter().find(|x| x.name() == Some(name)).unwrap();
    let m = s.placeholder_match(sh).unwrap().unwrap();
    (m.kind, m.ambiguous, m.candidates, m.layout.and_then(raw_id), m.master.and_then(raw_id))
}

const BODY_PH: &str = r#"<p:ph type="body" idx="1"/>"#;
const TITLE_PH: &str = r#"<p:ph type="title"/>"#;
const LAYOUT6: &str = "ppt/slideLayouts/slideLayout6.xml";

/// S3: idx ties, absent idx, family, orphan, `4294967295`, and step 3's
/// "idx 0 preferred" (survivor Y11).
#[test]
fn s3_placeholder_matching() {
    // tie: layout title and body both without idx; the slide body too
    let mut p = patched(&[(LAYOUT6, BODY_PH, r#"<p:ph type="body"/>"#), (SLIDE2, BODY_PH, r#"<p:ph type="body"/>"#)]);
    assert_eq!(slide2_match(&mut p, "Rectangle 2").0, MatchKind::IdxTieType);
    assert_eq!(slide2_match(&mut p, "Rectangle 3").0, MatchKind::IdxTieType);
    // absent idx, the only idx-0 candidate of another family → step 3 by type
    let mut p = patched(&[(SLIDE2, BODY_PH, r#"<p:ph type="body"/>"#)]);
    assert_eq!(slide2_match(&mut p, "Rectangle 3").0, MatchKind::Type);
    // absent idx, no such type → same family
    let mut p = patched(&[(SLIDE2, BODY_PH, r#"<p:ph type="subTitle"/>"#)]);
    assert_eq!(slide2_match(&mut p, "Rectangle 3").0, MatchKind::Family);
    // orphan: the layout has no title-family placeholder → master by family
    let mut p = patched(&[(LAYOUT6, TITLE_PH, r#"<p:ph type="pic" idx="50"/>"#)]);
    let (kind, _, _, layout, master) = slide2_match(&mut p, "Rectangle 2");
    assert_eq!((kind, layout), (MatchKind::Orphan, None));
    assert!(master.is_some());
    // idx 4294967295 (no layout placeholder has it) → type, ambiguous between idx 12 and 15
    let num = r#"<p:sp><p:nvSpPr><p:cNvPr id="90" name="Num"/><p:cNvSpPr/><p:nvPr><p:ph type="sldNum" sz="quarter" idx="4294967295"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:fld id="{5C1B3D53-6C46-4F5C-9A71-0D5A5A9C4E11}" type="slidenum"><a:rPr lang="en-US"/><a:t>2</a:t></a:fld></a:p></p:txBody></p:sp></p:spTree>"#;
    let mut p = patched(&[(SLIDE2, "</p:spTree>", num)]);
    let (kind, ambiguous, n, _, _) = slide2_match(&mut p, "Num");
    assert_eq!((kind, ambiguous, n), (MatchKind::Type, true, 2));
    // step 3 prefers idx 0: layout 7 has body idx 1 and body without idx;
    // the slide's body has idx 99 (no idx candidate) → the idx-0 one (id 9)
    let mut p = patched(&[
        ("ppt/slides/_rels/slide2.xml.rels", "slideLayout6.xml", "slideLayout7.xml"),
        ("ppt/slideLayouts/slideLayout7.xml", r#"<p:ph type="body" sz="half" idx="2"/>"#, r#"<p:ph type="body" sz="half"/>"#),
        (SLIDE2, BODY_PH, r#"<p:ph type="body" idx="99"/>"#),
    ]);
    let (kind, ambiguous, n, layout, _) = slide2_match(&mut p, "Rectangle 3");
    assert_eq!((kind, ambiguous, n, layout), (MatchKind::Type, false, 1, Some(9)));
}

/// S4: dt, ftr and sldNum take `p:otherStyle` (step 7 by the slide's family).
#[test]
fn s4_footer_placeholders_use_other_style() {
    let other = r#"</p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr u="sng"/></a:lvl1pPr></p:otherStyle>"#;
    let shape = |id: u32, ty: &str, idx: u32| format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{ty}"/><p:cNvSpPr/><p:nvPr><p:ph type="{ty}" idx="{idx}"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>x</a:t></a:r></a:p></p:txBody></p:sp>"#);
    let shapes = format!("{}{}{}</p:spTree>", shape(90, "dt", 10), shape(91, "ftr", 11), shape(92, "sldNum", 12));
    let mut p = patched(&[(MASTER, "</p:bodyStyle>", other), (SLIDE2, "</p:spTree>", &shapes)]);
    for name in ["dt", "ftr", "sldNum"] {
        slide2_runs(&mut p, name, |_, rp, _| {
            assert_eq!(rp.underline.value().map(|u| u.as_str()), Some("sng"), "{name}");
            assert_eq!(rp.underline.source(), Some(&Source::MasterTextStyle { part: MASTER, style: TextStyleKind::Other, level: 0 }), "{name}");
        });
    }
    // the body placeholder does not see otherStyle
    slide2_runs(&mut p, "Rectangle 3", |_, rp, _| assert_eq!(rp.underline, Lookup::Unset));
}

/// Survivors Y5/Y6: a themeOverride relationship makes theme fonts and
/// scheme colours Unresolved(ThemeOverride).
#[test]
fn theme_override_is_unresolved() {
    let rel = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/themeOverride" Target="../theme/themeOverride1.xml"/></Relationships>"#;
    let run = r#"<a:r><a:rPr lang="en-US"><a:latin typeface="+mj-lt"/></a:rPr><a:t>x</a:t></a:r>"#;
    let mut p = patched(&[("ppt/slides/_rels/slide1.xml.rels", "</Relationships>", rel), (SLIDE, TITLE_RUN, run)]);
    title_run(&mut p, |rp, _| {
        assert_eq!(rp.latin, Lookup::Unresolved(Reason::ThemeOverride), "{:?}", rp.latin);
        assert_eq!(rp.fill, Lookup::Unresolved(Reason::ThemeOverride), "{:?}", rp.fill);
        assert!(rp.size.value().is_some());
    });
}

/// Survivor Y16: `fontRef idx="major"` supplies +mj-lt / +mj-ea / +mj-cs.
#[test]
fn font_ref_major() {
    let style = r#"<p:spPr/><p:style><a:lnRef idx="0"><a:schemeClr val="accent1"/></a:lnRef><a:fillRef idx="0"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="major"><a:schemeClr val="accent2"/></a:fontRef></p:style><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr hangingPunct"#;
    let run = r#"<a:r><a:rPr lang="en-US"/><a:t>x</a:t></a:r>"#;
    let mut p = patched(&[(SLIDE, "<p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr hangingPunct", style), (SLIDE, TITLE_RUN, run)]);
    title_run(&mut p, |rp, _| {
        assert_eq!(rp.latin.source(), Some(&Source::ShapeStyle));
        assert_eq!(rp.latin.value().and_then(|f| f.theme_ref), Some("+mj-lt"));
        fn r(l: &Lookup<'_, ooxml_pptx::Font<'_>>) -> Option<String> {
            match l {
                Lookup::Unresolved(Reason::ScriptDependent { reference, .. }) => Some((*reference).to_owned()),
                Lookup::Set(x) => x.value.as_ref().ok().and_then(|f| f.theme_ref).map(str::to_owned),
                _ => None,
            }
        }
        assert_eq!((r(&rp.east_asian).as_deref(), r(&rp.complex_script).as_deref()), (Some("+mj-ea"), Some("+mj-cs")));
    });
}

/// #56 m2: a group's own geometry is its p:grpSpPr/a:xfrm; slide_rect maps
/// a member through chOff/chExt; a rotated group refuses.
#[test]
fn group_geometry_and_slide_rect() {
    let group = |rot: &str| {
        format!(
            r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="80" name="G"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm{rot}><a:off x="1000" y="2000"/><a:ext cx="200" cy="400"/><a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="81" name="M"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="10" y="20"/><a:ext cx="30" cy="40"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody></p:sp></p:grpSp></p:spTree>"#
        )
    };
    let mut p = patched(&[(SLIDE, "</p:spTree>", &group(""))]);
    {
        let deck = p.deck().unwrap();
        let s = deck.slide(0).unwrap();
        let shapes = s.slide().shapes();
        let g = shapes.iter().find(|x| x.name() == Some("G")).unwrap();
        let m = shapes.iter().find(|x| x.name() == Some("M")).unwrap();
        let gx = s.geometry(g).unwrap();
        assert_eq!(gx.value().map(|x| (x.x, x.y, x.cx, x.cy)), Some((1000, 2000, 200, 400)));
        assert!(m.is_text_box() && !g.is_text_box());
        assert_eq!(s.slide_rect(m).unwrap(), Some(ooxml_pptx::Rect { x: 1020, y: 2080, cx: 60, cy: 160 }));
        let title = shapes.iter().find(|x| x.name() == Some("Rectangle 2")).unwrap();
        assert!(!title.is_text_box());
    }
    let mut p = patched(&[(SLIDE, "</p:spTree>", &group(r#" rot="5400000""#))]);
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let shapes = s.slide().shapes();
    let m = shapes.iter().find(|x| x.name() == Some("M")).unwrap();
    assert!(matches!(s.slide_rect(m), Err(ooxml_pptx::Error::TransformedGroup { group_id: Some(80) })));
}

/// #56 m3: group-winner attributes carry their own `defaulted`; invalid
/// values are AttrError with the raw text.
#[test]
fn winner_attributes_and_invalid_values() {
    let mut p = patched(&[(SLIDE, "<p:spPr/><p:txBody><a:bodyPr/>", r#"<p:spPr><a:xfrm rot="abc" flipV="1"><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/>"#)]);
    title_body_props(&mut p, |_, g, _| {
        let x = g.value().unwrap();
        assert_eq!(x.rot.value.as_ref().unwrap_err().raw, "abc");
        assert!(!x.rot.defaulted);
        assert_eq!((x.flip_v.get(), x.flip_v.defaulted), (Some(true), false));
        assert_eq!((x.flip_h.get(), x.flip_h.defaulted), (Some(false), true));
    });
    let mut p = patched(&[(SLIDE, TITLE_RUN, r#"<a:r><a:rPr lang="en-US"><a:latin typeface="Arial" pitchFamily="zz"/></a:rPr><a:t>x</a:t></a:r>"#)]);
    title_run(&mut p, |rp, _| {
        let f = rp.latin.value().unwrap();
        assert_eq!(f.pitch_family.value.as_ref().unwrap_err().raw, "zz");
        assert_eq!((f.charset.get(), f.charset.defaulted), (Some(1), true));
    });
    let mut p = patched(&[(SLIDE, r#"<a:pPr hangingPunct="1"/>"#, r#"<a:pPr hangingPunct="1"><a:lnSpc><a:spcPts val="x1"/></a:lnSpc><a:buAutoNum type="romanLcPeriod" startAt="q"/></a:pPr>"#)]);
    title_run(&mut p, |_, pp| {
        let Lookup::Set(r) = &pp.line_spacing else { panic!() };
        assert_eq!(r.value.as_ref().unwrap_err().raw, "x1");
        let Some(Bullet::AutoNum { start_at, .. }) = pp.bullet.value() else { panic!() };
        assert_eq!(start_at.value.as_ref().unwrap_err().raw, "q");
    });
}

/// M1: schema defaults for Unset properties.
#[test]
fn xsd_defaults() {
    assert_eq!(ooxml_pptx::RunProps::xsd_default("dirty"), Some("true"));
    assert_eq!(ooxml_pptx::RunProps::xsd_default("sz"), None);
    assert_eq!(ooxml_pptx::BodyProps::xsd_default("upright"), Some("false"));
    assert_eq!(ooxml_pptx::ParaProps::xsd_default("algn"), None);
    assert_eq!(ooxml_pptx::script_for_lang("ja-JP"), Some("Jpan"));
    assert_eq!(ooxml_pptx::script_for_lang("zh-TW"), Some("Hant"));
    assert_eq!(ooxml_pptx::script_for_lang("zh-CN"), Some("Hans"));
    assert_eq!(ooxml_pptx::script_for_lang("en-US"), None);
}

// ---------------------------------------------------------------------------
// Review #56 round 2: r2-m2 (survivors Z6, Z9, Z10, Z19, Z21, Z24) and
// r2-m3 (slide_rect refuses to guess).

fn rect_of(p: &mut Presentation, name: &str) -> Result<Option<ooxml_pptx::Rect>, ooxml_pptx::Error> {
    let deck = p.deck().unwrap();
    let s = deck.slide(0).unwrap();
    let shapes = s.slide().shapes();
    let m = shapes.iter().find(|x| x.name() == Some(name)).unwrap();
    s.slide_rect(m)
}

#[test]
fn slide_rect_nested_flipped_and_invalid() {
    let member = r#"<p:sp><p:nvSpPr><p:cNvPr id="81" name="M"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="10" y="10"/><a:ext cx="10" cy="10"/></a:xfrm></p:spPr></p:sp>"#;
    let group = |id: u32, attrs: &str, inner: &str, xfrm: &str| {
        format!(r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="{id}" name="G{id}"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm{attrs}>{xfrm}</a:xfrm></p:grpSpPr>{inner}</p:grpSp>"#)
    };
    let x2 = r#"<a:off x="1000" y="1000"/><a:ext cx="200" cy="200"/><a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/>"#;
    let half = r#"<a:off x="100" y="100"/><a:ext cx="50" cy="50"/><a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/>"#;
    // nested: inner ×0.5 → (105,105,5,5); outer ×2 → (1210,1210,10,10)
    let nested = group(82, "", &group(83, "", member, half), x2);
    let mut p = patched(&[(SLIDE, "</p:spTree>", &format!("{nested}</p:spTree>"))]);
    assert_eq!(rect_of(&mut p, "M").unwrap(), Some(ooxml_pptx::Rect { x: 1210, y: 1210, cx: 10, cy: 10 }));
    // a flipped outer group
    let flipped = group(84, r#" flipH="1""#, &group(83, "", member, half), x2);
    let mut p = patched(&[(SLIDE, "</p:spTree>", &format!("{flipped}</p:spTree>"))]);
    assert!(matches!(rect_of(&mut p, "M"), Err(ooxml_pptx::Error::TransformedGroup { group_id: Some(84) })));
    // undecidable transforms → None (r2-m3)
    for (attrs, xfrm) in [
        (r#" rot="abc""#, x2),
        (r#" flipV="2""#, x2),
        ("", r#"<a:ext cx="200" cy="200"/><a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/>"#),
        ("", r#"<a:off x="1000" y="1000"/><a:ext cx="200" cy="200"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="100"/>"#),
        ("", r#"<a:off x="q" y="1000"/><a:ext cx="200" cy="200"/>"#),
        ("", r#"<a:off x="NaN" y="1000"/><a:ext cx="200" cy="200"/>"#),
        ("", r#"<a:off x="1000" y="1000"/><a:ext cx="200.5" cy="200"/>"#),
        ("", r#"<a:off x="1000" y="1000"/><a:ext cx="200" cy="200"/><a:chOff x="bad" y="0"/>"#),
        ("", r#"<a:off x="1000" y="1000"/><a:ext cx="200" cy="200"/><a:chExt cx="100"/>"#),
    ] {
        let g = group(85, attrs, member, xfrm);
        let mut p = patched(&[(SLIDE, "</p:spTree>", &format!("{g}</p:spTree>"))]);
        assert_eq!(rect_of(&mut p, "M").unwrap(), None, "{attrs} {xfrm}");
    }
    // a full turn is no rotation
    let g = group(86, r#" rot="21600000""#, member, x2);
    let mut p = patched(&[(SLIDE, "</p:spTree>", &format!("{g}</p:spTree>"))]);
    assert!(rect_of(&mut p, "M").unwrap().is_some());
}

/// Z6: a layout placeholder inside mc:AlternateContent is matched.
#[test]
fn layout_placeholder_inside_alternate_content() {
    let layout = String::from_utf8(Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap().read_part(LAYOUT6).unwrap()).unwrap();
    let start = layout.find("<p:sp><p:nvSpPr><p:cNvPr id=").unwrap();
    let end = layout[start..].find("</p:sp>").unwrap() + start + "</p:sp>".len();
    let sp = &layout[start..end];
    assert!(sp.contains(TITLE_PH));
    let ac = format!(r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Fallback>{sp}</mc:Fallback></mc:AlternateContent>"#);
    let mut p = patched(&[(LAYOUT6, sp, &ac)]);
    let (kind, _, _, layout_id, _) = slide2_match(&mut p, "Rectangle 2");
    assert_eq!(kind, MatchKind::Idx);
    assert!(layout_id.is_some());
}

/// Z19: a value from the master placeholder reports the slide's match kind.
#[test]
fn master_values_report_the_slide_match_kind() {
    let mut p = patched(&[(LAYOUT6, TITLE_PH, r#"<p:ph type="pic" idx="50"/>"#)]);
    let deck = p.deck().unwrap();
    let s = deck.slide(1).unwrap();
    let shapes = s.slide().shapes();
    let t = shapes.iter().find(|x| x.name() == Some("Rectangle 2")).unwrap();
    let g = s.geometry(t).unwrap();
    assert!(matches!(g.source(), Some(Source::Placeholder { role: ooxml_pptx::Role::Master, matched: MatchKind::Orphan, .. })), "{g:?}");
}

/// Z21 / Z24: only the six theme references are references; an invalid
/// coordinate keeps its raw text.
#[test]
fn theme_reference_shape_and_coordinate_raw() {
    let mut p = patched(&[(SLIDE, TITLE_RUN, r#"<a:r><a:rPr lang="en-US"><a:latin typeface="+xx-yy"/></a:rPr><a:t>x</a:t></a:r>"#)]);
    title_run(&mut p, |rp, _| {
        let f = rp.latin.value().unwrap();
        assert_eq!((f.typeface, f.theme_ref), ("+xx-yy", None));
    });
    let mut p = patched(&[(SLIDE, "<p:spPr/><p:txBody><a:bodyPr/>", r#"<p:spPr><a:xfrm><a:off x="q1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/>"#)]);
    title_body_props(&mut p, |_, g, _| {
        let Lookup::Set(r) = g else { panic!() };
        assert_eq!(r.value.unwrap_err().raw, "q1");
    });
}
