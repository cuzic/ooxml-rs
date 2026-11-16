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
        Some(Source::MasterTextStyle { style, .. }) => format!(
            "master.txStyles.{}",
            match style {
                TextStyleKind::Title => "titleStyle",
                TextStyleKind::Body => "bodyStyle",
                TextStyleKind::Other => "otherStyle",
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

const COMPARED: &[&str] = &["ph.slide", "ph.runs", "nonph.runs", "match.layout.", "geometry.from_", "ph.run.sz.", "nonph.run.sz.", "ph.run.b.", "nonph.run.b.", "ph.para.", "nonph.para."];

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
    assert_eq!((col.val, col.theme_slot), (Some("tx1"), Some("dk1")));
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
        assert_eq!(pp.align.value(), Some(&"r"));
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
        assert_eq!(rp.caps.value(), Some(&"all"), "{:?}", rp.caps);
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
    title_run(&mut p, |rp, _| {
        if theme.contains(r#"<a:ea typeface=""/>"#) {
            assert!(matches!(rp.east_asian, Lookup::Unresolved(Reason::ScriptDependent { .. })), "{:?}", rp.east_asian);
        } else {
            assert!(rp.east_asian.typeface().is_some_and(|t| !t.is_empty()));
        }
    });
}

/// S6: the slide's colour-map override is used for scheme colours.
#[test]
fn s6_slide_color_map_override() {
    let mut p = patched(&[(SLIDE, "<a:masterClrMapping/>", r#"<a:overrideClrMapping bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>"#)]);
    title_run(&mut p, |rp, _| {
        let Some(Fill::Solid(c)) = rp.fill.value() else { panic!("{:?}", rp.fill) };
        assert_eq!(c.theme_slot, Some("lt1"));
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
        assert_eq!(r.value.as_ref().unwrap(), &Bullet::AutoNum { scheme: "arabicPeriod", start_at: 1 });
        assert!(r.defaulted);
    });
}

/// S6b / M6: the slide keeps a:masterClrMapping, the layout overrides.
#[test]
fn s6b_layout_color_map_override() {
    let mut p = patched(&[("ppt/slideLayouts/slideLayout1.xml", "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>", r#"<p:clrMapOvr><a:overrideClrMapping bg1="lt1" tx1="lt1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr>"#)]);
    title_run(&mut p, |rp, _| {
        let Some(Fill::Solid(c)) = rp.fill.value() else { panic!("{:?}", rp.fill) };
        assert_eq!((c.theme_slot, c.map_source), (Some("lt1"), Some(ooxml_pptx::MapSource::LayoutOverride)));
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
