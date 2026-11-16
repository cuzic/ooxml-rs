//! Writes a small, named set of edited presentations plus `CHECKLIST.md`
//! for the project owner's manual check in Microsoft 365 PowerPoint
//! (ADR-0005 §7 "Manual"; the sandbox has no PowerPoint).
//!
//! ```text
//! cargo run -p ooxml-pptx --example manual_check -- <outdir>
//! ```
//!
//! Inputs are the third-party fixtures in `reference/fixtures/pptx/`; the
//! outputs are for local inspection only and are not redistributed.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ooxml_core::Package;
use ooxml_pptx::{Presentation, ShapeKind, TextItem};

type R<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/pptx").join(rel)
}

struct Item {
    file: String,
    source: &'static str,
    edit: String,
    observe: Vec<&'static str>,
}

fn save(out: &Path, name: &str, pres: &Presentation) -> R<String> {
    let file = format!("{name}.pptx");
    pres.save(out.join(&file))?;
    Ok(file)
}

/// The first `p:sp` text body on `slide` that satisfies `pred`, as an id.
fn find_shape(pres: &mut Presentation, slide: usize, pred: impl Fn(&ooxml_pptx::ShapeRef<'_>) -> bool) -> R<u32> {
    let s = pres.slide(slide)?;
    let id = s.shapes().into_iter().find(|x| pred(x)).ok_or("no matching shape")?.id()?;
    Ok(id)
}

fn main() -> R {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: manual_check <outdir>")?);
    std::fs::create_dir_all(&out)?;
    let mut items = Vec::new();

    // 1. Title + multi-line body (paragraph reuse, \v -> a:br)
    {
        let mut p = Presentation::open(fixture("python-pptx/ph-inherit-props.pptx"))?;
        let ph: Vec<u32> = p.slide(0)?.placeholders().iter().filter(|s| s.text_body().is_some()).map(|s| s.id()).collect::<Result<_, _>>()?;
        let mut s = p.slide_mut(0)?;
        s.shape_mut_by_id(ph[0])?.text_body_mut()?.set_text("Edited title — 編集済み")?;
        if let Some(&b) = ph.get(1) {
            s.shape_mut_by_id(b)?.text_body_mut()?.set_text("First line\nSecond line\u{b}(same paragraph, after a line break)\nThird line")?;
        }
        items.push(Item {
            file: save(&out, "01-title-and-body", &p)?,
            source: "python-pptx/ph-inherit-props.pptx, slide 1",
            edit: "TextBody::set_text on the first two placeholders (body: 3 paragraphs, one a:br)".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "Title and body show the new text; body has 3 bullet paragraphs, the 2nd with a soft line break.",
                "Fonts/sizes look like the template's; added paragraphs look like the existing ones (a:pPr / a:rPr are copied as subtrees, Phase 0.1).",
            ],
        });
    }

    // 2. Autofit cache after a longer text
    {
        let mut p = Presentation::open(fixture("poi/bug62513.pptx"))?;
        let mut target = None;
        for i in 0..p.slide_count() {
            let s = p.slide(i)?;
            if let Some(sh) = s.shapes().into_iter().find(|x| {
                x.text_body().is_some_and(|b| {
                    b.raw().elements().any(|c| c.name().local() == "bodyPr" && c.elements().any(|a| a.name().local() == "normAutofit" && !a.attrs().is_empty()))
                })
            }) {
                target = Some((i, sh.id()?, sh.text_body().map(|b| b.paragraphs().len()).unwrap_or(1)));
                break;
            }
        }
        let (si, id, n) = target.ok_or("no normAutofit shape")?;
        let text: Vec<String> = (0..n.max(1)).map(|k| format!("Considerably longer replacement text, line {k}, to overflow the box")).collect();
        let r = p.slide_mut(si)?.shape_mut_by_id(id)?.text_body_mut()?.set_text(&text.join("\n"));
        let edit = match r {
            Ok(()) => format!("slide {}: TextBody::set_text with {} long lines on shape id {id}, whose a:normAutofit has fontScale/lnSpcReduction (left as is)", si + 1, text.len()),
            Err(e) => {
                // fall back to run-level edits (refusal is documented)
                let mut s = p.slide_mut(si)?;
                let mut sh = s.shape_mut_by_id(id)?;
                let mut b = sh.text_body_mut()?;
                b.paragraph_mut(0)?.add_run(" — appended long text to overflow the autofit box, appended long text to overflow the autofit box")?;
                format!("slide {}: set_text refused ({e}); appended a long unformatted run to paragraph 1 of shape id {id} instead", si + 1)
            }
        };
        items.push(Item {
            file: save(&out, "02-autofit-stale", &p)?,
            source: "poi/bug62513.pptx",
            edit,
            observe: vec![
                "OWNER QUESTION (autofit, ADR-0005 Q4a): does the text overflow first and shrink only after you click into / edit the box, or does PowerPoint refit on open? Note what triggers the refit.",
                "Opens without a repair prompt.",
            ],
        });
    }

    // 3. Empty placeholder fill (a:endParaRPr copied to a:rPr)
    {
        let mut p = Presentation::open(fixture("poi/with_japanese.pptx"))?;
        let id = find_shape(&mut p, 0, |x| x.text_body().is_some_and(|b| b.paragraphs().iter().all(|q| q.items().is_empty())))?;
        p.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.set_text("空のプレースホルダーに入力 (filled)")?;
        items.push(Item {
            file: save(&out, "03-empty-placeholder-fill", &p)?,
            source: "poi/with_japanese.pptx, slide 1",
            edit: format!("Paragraph::set_text into the empty placeholder id {id}; the new run's a:rPr is a copy of a:endParaRPr"),
            observe: vec![
                "Opens without a repair prompt.",
                "The text looks the same as if typed into the empty placeholder in PowerPoint (size, language/proofing).",
            ],
        });
    }

    // 4. Table cells, CJK
    {
        let mut p = Presentation::open(fixture("poi/with_japanese.pptx"))?;
        let (si, id) = (0..p.slide_count())
            .find_map(|i| p.slide(i).ok()?.shapes().into_iter().find(|x| *x.kind() == ShapeKind::Table).and_then(|x| Some((i, x.id().ok()?))))
            .ok_or("no table")?;
        let mut s = p.slide_mut(si)?;
        let mut sh = s.shape_mut_by_id(id)?;
        sh.table_cell_mut(0, 0)?.set_text("見出し（編集）")?;
        sh.table_cell_mut(1, 1)?.paragraph_mut(0)?.set_text("セル B2 編集")?;
        items.push(Item {
            file: save(&out, "04-table-cells", &p)?,
            source: "poi/with_japanese.pptx",
            edit: format!("slide {}: table id {id}, cell (0,0) TextBody::set_text, cell (1,1) Paragraph::set_text", si + 1),
            observe: vec!["Opens without a repair prompt.", "Only those two cells changed; cell formatting kept."],
        });
    }

    // 5. Group members
    {
        let mut p = Presentation::open(fixture("libreoffice/slide-sections.pptx"))?;
        let mut done = None;
        'o: for i in 0..p.slide_count() {
            let s = p.slide(i)?;
            for x in s.shapes() {
                if x.depth() > 0
                    && let Some(b) = x.text_body()
                    && let Some(pi) = b.paragraphs().iter().position(|q| q.items().iter().any(|t| matches!(t, TextItem::Run(_))))
                {
                    done = Some((i, x.path().clone(), pi, x.id()?));
                    break 'o;
                }
            }
        }
        let (si, path, pi, id) = done.ok_or("no group member with text")?;
        p.slide_mut(si)?.shape_mut(&path)?.text_body_mut()?.paragraph_mut(pi)?.run_mut(0)?.set_text("Edited inside a group")?;
        items.push(Item {
            file: save(&out, "05-group-member", &p)?,
            source: "libreoffice/slide-sections.pptx",
            edit: format!("slide {}: run 1 of paragraph {} of group member id {id}", si + 1, pi + 1),
            observe: vec!["Opens without a repair prompt.", "The grouped shape shows the new text; the group is intact."],
        });
    }

    // 6. Slide outside every section (raw, documented edit of presentation.xml)
    {
        let mut pkg = Package::open(fixture("libreoffice/slide-sections.pptx"))?;
        let xml = String::from_utf8(pkg.read_part("ppt/presentation.xml")?)?;
        let patched = xml.replacen("<p14:sldId id=\"309\"/>", "", 1);
        assert_ne!(xml, patched, "fixture changed?");
        pkg.set_part("ppt/presentation.xml", patched.into_bytes())?;
        let mut p = Presentation::from_bytes(pkg.to_bytes()?)?;
        let last = p.slide_count() - 1;
        if let Ok(id) = find_shape(&mut p, last, |x| x.text_body().is_some_and(|b| !b.text().is_empty())) {
            let _ = p.slide_mut(last)?.shape_mut_by_id(id)?.text_body_mut()?.paragraph_mut(0)?.add_run(" (edited)");
        }
        items.push(Item {
            file: save(&out, "06-slide-outside-sections", &p)?,
            source: "libreoffice/slide-sections.pptx",
            edit: "SYNTHETIC: removed <p14:sldId id=\"309\"/> (the last slide) from p14:sectionLst in ppt/presentation.xml, so that slide belongs to no section; plus a small text edit on it".into(),
            observe: vec![
                "OWNER QUESTION (ADR-0005 Q4b, Phase 2 input): does PowerPoint open this without a repair prompt? Where does the last slide appear in the section view (own untitled section, appended to the last section, or a repair)?",
            ],
        });
    }

    // 7. Stale docProps/app.xml counts and titles
    {
        let mut p = Presentation::open(fixture("poi/sample.pptx"))?;
        let ph: Vec<u32> = p.slide(0)?.placeholders().iter().map(|s| s.id()).collect::<Result<_, _>>()?;
        let mut s = p.slide_mut(0)?;
        let mut t = s.shape_mut_by_id(ph[0])?;
        t.text_body_mut()?.paragraph_mut(0)?.run_mut(0)?.set_text("A different title than app.xml says")?;
        let mut b = s.shape_mut_by_id(ph[1])?;
        let mut body = b.text_body_mut()?;
        for k in 0..3 {
            body.add_paragraph(&format!("Added paragraph {k}"))?;
        }
        items.push(Item {
            file: save(&out, "07-stale-app-xml", &p)?,
            source: "poi/sample.pptx (docProps/app.xml: Words 29, Paragraphs 10, TitlesOfParts with the old title)",
            edit: "slide 1: title run text changed; 3 paragraphs added to the subtitle (add_paragraph, unformatted); docProps/app.xml untouched".into(),
            observe: vec![
                "OWNER QUESTION (ADR-0005 Q4c): any repair prompt or warning? After File > Info (or after saving from PowerPoint), are Words/Paragraphs/slide titles recomputed? Is the stale value visible anywhere before saving?",
                "The three added paragraphs use the placeholder's list style (no own formatting).",
            ],
        });
    }

    // 8. Modern comment anchored to the edited shape (whole-shape anchor)
    {
        let mut p = Presentation::open(fixture("libreoffice/modern-comments.pptx"))?;
        p.slide_mut(0)?.shape_mut_by_id(2)?.text_body_mut()?.set_text("Hello, edited")?;
        items.push(Item {
            file: save(&out, "08-modern-comment-shape-anchor", &p)?,
            source: "libreoffice/modern-comments.pptx",
            edit: "slide 1: title (shape id 2, anchored by a modern comment as a whole shape: ac:deMkLst/ac:spMk) set to new text".into(),
            observe: vec!["Opens without a repair prompt.", "The comment thread is still attached to the title shape."],
        });
    }

    // 9. Run edit in a shape with paragraph-level animation
    {
        let mut p = Presentation::open(fixture("poi/45545_Comment.pptx"))?;
        let id = 584_707;
        let pi = p
            .slide(0)?
            .shape_by_id(id)?
            .and_then(|s| s.text_body())
            .and_then(|b| b.paragraphs().iter().position(|q| !q.runs().is_empty()))
            .ok_or("no run")?;
        p.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.paragraph_mut(pi)?.run_mut(0)?.set_text("Edited animated paragraph")?;
        items.push(Item {
            file: save(&out, "09-animated-paragraph-run-edit", &p)?,
            source: "poi/45545_Comment.pptx",
            edit: format!("slide 1: run 1 of paragraph {} of shape {id} (p:bldP build=\"p\" + p:pRg animation; adding/removing paragraphs is refused, run text is allowed)", pi + 1),
            observe: vec!["Opens without a repair prompt.", "Slide Show: the by-paragraph animation still plays, in the same order."],
        });
    }

    // 10. Field removed explicitly, then text set
    {
        let mut p = Presentation::open(fixture("poi/layouts.pptx"))?;
        let (si, id) = (0..p.slide_count())
            .find_map(|i| {
                let s = p.slide(i).ok()?;
                s.shapes().into_iter().find(|x| x.text_body().is_some_and(|b| b.paragraphs().iter().any(|q| q.items().iter().any(|t| matches!(t, TextItem::Field(_)))))).and_then(|x| Some((i, x.id().ok()?)))
            })
            .ok_or("no field")?;
        let mut s = p.slide_mut(si)?;
        let mut sh = s.shape_mut_by_id(id)?;
        let mut b = sh.text_body_mut()?;
        b.paragraph_mut(0)?.remove_field(0)?;
        b.set_text("Static text instead of the slide-number field")?;
        items.push(Item {
            file: save(&out, "10-field-removed", &p)?,
            source: "poi/layouts.pptx",
            edit: format!("slide {}: remove_field(0) then TextBody::set_text on shape id {id}", si + 1),
            observe: vec!["Opens without a repair prompt.", "The box shows the static text (no slide number)."],
        });
    }

    // 11. Phase 0.1: new runs/paragraphs copy formatting with child elements
    {
        let mut p = Presentation::open(fixture("poi/sample.pptx"))?;
        let mut s = p.slide_mut(0)?;
        s.shape_mut_by_id(2049)?.text_body_mut()?.set_text("Title line one\u{b}after a line break\nSecond title paragraph")?;
        s.shape_mut_by_id(2050)?.text_body_mut()?.set_text("First subtitle paragraph\nSecond (copies a:pPr with a:spcAft)\nThird")?;
        items.push(Item {
            file: save(&out, "11-formatted-new-lines", &p)?,
            source: "poi/sample.pptx, slide 1",
            edit: "TextBody::set_text with \\v and new lines on the title (a:rPr has an a:sym child) and the subtitle (a:pPr has an a:spcAft child); Phase 0.1 copies both subtrees".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "New lines and the text after the line break look exactly like the first line (font, symbol font, size, paragraph spacing).",
                "Empty-line heights (end of paragraph) match the text size.",
            ],
        });
    }

    // 12. Phase 0.1: empty paragraph whose a:endParaRPr has child elements
    {
        let mut p = Presentation::open(fixture("poi/sample_pptx_grouping_issues.pptx"))?;
        let id = find_shape(&mut p, 0, |x| {
            x.text_body().is_some_and(|b| {
                let q = b.paragraphs()[0];
                q.runs().is_empty() && q.raw().elements().any(|c| c.name().local() == "endParaRPr" && c.elements().next().is_some())
            })
        })?;
        p.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.paragraph_mut(0)?.set_text("Typed into the empty paragraph")?;
        items.push(Item {
            file: save(&out, "12-formatted-empty-fill", &p)?,
            source: "poi/sample_pptx_grouping_issues.pptx, slide 1",
            edit: format!("Paragraph::set_text into the empty first paragraph of shape id {id}, whose a:endParaRPr has child elements (copied as the new a:rPr)"),
            observe: vec!["Opens without a repair prompt.", "The text looks as if typed there in PowerPoint (colour/font from the end-of-paragraph format)."],
        });
    }

    // 13. Phase 0.1: hyperlinks stay on their run
    {
        let mut p = Presentation::open(fixture("poi/shapes.pptx"))?;
        let id = find_shape(&mut p, 4, |x| *x.kind() == ShapeKind::Table)?;
        let cell = {
            let s = p.slide(4)?;
            let t = s.shape_by_id(id)?.and_then(|x| x.table()).ok_or("table")?;
            let mut found = None;
            'rows: for (r, row) in t.rows().iter().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    if cell.text_body().is_some_and(|b| b.raw().elements().any(|q| q.elements().any(|r| r.elements().any(|rp| rp.elements().any(|l| l.name().local() == "hlinkClick"))))) {
                        found = Some((r, c));
                        break 'rows;
                    }
                }
            }
            found.ok_or("no linked cell")?
        };
        p.slide_mut(4)?.shape_mut_by_id(id)?.table_cell_mut(cell.0, cell.1)?.paragraph_mut(0)?.set_text("Linked text\u{b}plain text after a break")?;
        items.push(Item {
            file: save(&out, "13-hyperlink-run", &p)?,
            source: "poi/shapes.pptx, slide 5",
            edit: format!("table id {id}, cell {cell:?}: Paragraph::set_text(\"Linked text\\vplain text after a break\"); the new run copies the a:rPr without a:hlinkClick"),
            observe: vec![
                "Opens without a repair prompt.",
                "Only \"Linked text\" is a hyperlink (same target as before); the text after the break looks the same but is not linked.",
            ],
        });
    }

    // 17–22: ADR-0008 manual checks M1–M3, M5–M7 (inheritance rules the
    // corpus cannot settle). Each deck is poi/sample.pptx with documented
    // XML edits; the checklist states what ooxml-pptx resolves so the owner
    // can compare. (M4, a notes edit, belongs to ADR-0008 §2, not built yet.)
    {
        fn sample_with(edits: &[(&str, &str, &str)]) -> R<Presentation> {
            let mut pkg = Package::open(fixture("poi/sample.pptx"))?;
            for (part, from, to) in edits {
                let x = String::from_utf8(pkg.read_part(part)?)?;
                if !x.contains(from) {
                    return Err(format!("{part}: {from:?} not found").into());
                }
                pkg.set_part(part, x.replacen(from, to, 1).into_bytes())?;
            }
            Ok(Presentation::from_bytes(pkg.to_bytes()?)?)
        }
        /// "size / typeface / fill (source)" of the first run of every text shape on slide 1.
        fn predict(p: &mut Presentation) -> R<String> {
            let deck = p.deck()?;
            let s = deck.slide(0)?;
            let mut out = Vec::new();
            for sh in s.slide().shapes() {
                let Some(b) = sh.text_body() else { continue };
                let Some(para) = b.paragraphs().into_iter().next() else { continue };
                let Some(TextItem::Run(r)) = para.items().into_iter().find(|i| matches!(i, TextItem::Run(_))) else { continue };
                let rp = s.run_props(&sh, b, para, ooxml_pptx::RunItem::Run(r))?;
                let bp = s.body_props(&sh)?;
                out.push(format!(
                    "{}: size {:?} from {:?}; latin {:?}; fill {:?}; autofit {:?}",
                    sh.name().unwrap_or("?"),
                    rp.size.value(),
                    rp.size.source(),
                    rp.latin.typeface(),
                    rp.fill.value().map(|f| format!("{f:?}")),
                    bp.autofit.value(),
                ));
            }
            Ok(out.join("; "))
        }
        const SLIDE: &str = "ppt/slides/slide1.xml";
        const MASTER: &str = "ppt/slideMasters/slideMaster1.xml";
        const LAYOUT: &str = "ppt/slideLayouts/slideLayout1.xml";
        let other_style = r#"</p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr sz="4000"/></a:lvl1pPr></p:otherStyle>"#;
        let shapes = r#"<p:sp><p:nvSpPr><p:cNvPr id="90" name="M1 text box"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="5300000"/><a:ext cx="3800000" cy="700000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="square"/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Text box</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="91" name="M1 shape"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="4600000" y="5300000"/><a:ext cx="3800000" cy="700000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:style><a:lnRef idx="2"><a:schemeClr val="accent1"/></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></p:style><p:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Shape</a:t></a:r></a:p></p:txBody></p:sp></p:spTree>"#;
        type Edit<'e> = (&'e str, &'e str, &'e str);
        type Case<'e> = (&'e str, &'e str, Vec<Edit<'e>>, &'e str, Vec<&'static str>);
        let cases: Vec<Case<'_>> = vec![
            (
                "17-M1-text-box-shape-styles",
                "M1: a text box (txBox=\"1\") and an autoshape with p:style/a:fontRef, no size of their own; defaultTextStyle has no size, otherStyle says 40 pt, bodyStyle 32 pt",
                vec![(MASTER, "</p:bodyStyle>", other_style), (SLIDE, "</p:spTree>", shapes)],
                "Q1/Q2: POI order (defaultTextStyle, then otherStyle) predicts 40 pt for both; LibreOffice predicts the application default (18 pt); the ECMA note predicts bodyStyle (32 pt) for the text box",
                vec!["Opens without a repair prompt.", "Text box size: ____ pt; shape size: ____ pt; shape text colour (white = fontRef lt1?): ____"],
            ),
            (
                "18-M2-explicit-idx-missing",
                "M2: the subtitle's p:ph gets idx=\"4294967295\" (no layout placeholder has it)",
                vec![(SLIDE, r#"<p:ph type="subTitle" idx="1"/>"#, r#"<p:ph type="subTitle" idx="4294967295"/>"#)],
                "§1.2: matched by type (subTitle) on the layout",
                vec!["Opens without a repair prompt.", "Home → Reset: the subtitle moves to the layout's subtitle position (yes / no / elsewhere)."],
            ),
            (
                "19-M3-layout-transition",
                "M3: the title layout gets <p:transition><p:fade/></p:transition>; the slide has none",
                vec![(LAYOUT, "</p:sldLayout>", r#"<p:transition spd="slow"><p:fade/></p:transition></p:sldLayout>"#)],
                "§3.4 / Q5: Slide::transition is None; the layout's is reported separately",
                vec!["Opens without a repair prompt.", "Slide Show: slide 1 fades in (layout transition applies) — yes / no."],
            ),
            (
                "20-M5-paragraph-defRPr",
                "M5: the title paragraph's own a:pPr gets <a:defRPr sz=\"6000\"/>; the run has no size",
                vec![(SLIDE, r#"<a:pPr hangingPunct="1"/><a:r><a:rPr lang="en-US"><a:sym"#, r#"<a:pPr hangingPunct="1"><a:defRPr sz="6000"/></a:pPr><a:r><a:rPr lang="en-US"><a:sym"#)],
                "§1.3 step 2: 60 pt from ParagraphDefault (POI); 44 pt if PowerPoint ignores a paragraph's defRPr",
                vec!["Opens without a repair prompt.", "Title size: ____ pt."],
            ),
            (
                "21-M6-layout-color-map",
                "M6: the slide keeps a:masterClrMapping; its layout overrides the map with tx1=\"lt1\" (white)",
                vec![(LAYOUT, "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>", r#"<p:clrMapOvr><a:overrideClrMapping bg1="lt1" tx1="lt1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:clrMapOvr>"#)],
                "§1.4: the layout override applies (POI, LibreOffice test file): title text white; ECMA's text would give the master's map (black)",
                vec!["Opens without a repair prompt.", "Title text colour: white / black / other: ____"],
            ),
            (
                "22-M7-autofit-and-link-colour",
                "M7: the title layout placeholder gets <a:normAutofit fontScale=\"50000\"/>, the slide body none; the title run gets a next-slide action link with ahyp:hlinkClr val=\"tx\"",
                vec![
                    (LAYOUT, "<a:bodyPr/>", r#"<a:bodyPr><a:normAutofit fontScale="50000"/></a:bodyPr>"#),
                    (SLIDE, r#"<a:rPr lang="en-US"><a:sym"#, r#"<a:rPr lang="en-US"><a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"><a:extLst><a:ext uri="{A12FA001-AC4F-418D-AE19-62706E023703}"><ahyp:hlinkClr xmlns:ahyp="http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor" val="tx"/></a:ext></a:extLst></a:hlinkClick><a:sym"#),
                ],
                "§1.6: autofit kind inherited (normAutofit) but its scale Unresolved(InheritedAutofitScale); the link's colour rule is RunColor (hlinkClr tx)",
                vec!["Opens without a repair prompt.", "Does the title text shrink? ____", "Is the linked title in the run colour (not the hyperlink colour)? ____", "Does clicking it in Slide Show go to the next slide? ____"],
            ),
        ];
        for (name, edit, edits, predicts, observe) in cases {
            let mut p = sample_with(&edits)?;
            let prediction = predict(&mut p)?;
            items.push(Item {
                file: save(&out, name, &p)?,
                source: "poi/sample.pptx (SYNTHETIC edits, ADR-0008 §7 Manual)",
                edit: format!("{edit}. Expected by the rule: {predicts}. ooxml-pptx resolves: {prediction}"),
                observe,
            });
        }
    }

    let mut md = String::from("# ooxml-pptx Phase 0 / 0.1 — manual check in Microsoft 365 PowerPoint\n\n");
    md.push_str("Generated by `cargo run -p ooxml-pptx --example manual_check -- <outdir>` (ADR-0005 §7, Manual).\n");
    md.push_str("For each file: open it in current Microsoft 365 PowerPoint (desktop), tick what holds, and write notes for anything else.\n");
    md.push_str("Inputs are third-party fixtures from `reference/fixtures/pptx/` (see its README for licences); do not redistribute the outputs.\n\n");
    md.push_str("PowerPoint version / build: ______________________\n\n");
    for it in &items {
        let _ = writeln!(md, "## `{}`\n\n- Source: {}\n- Edit: {}\n", it.file, it.source, it.edit);
        for o in &it.observe {
            let _ = writeln!(md, "- [ ] {o}");
        }
        md.push_str("- Notes:\n\n");
    }
    md.push_str("## Owner questions (summary)\n\n");
    md.push_str("- **Q4a autofit**: `02-autofit-stale.pptx` — when does PowerPoint recompute `a:normAutofit@fontScale`/`lnSpcReduction` after a text change?\n");
    md.push_str("- **Q4b sections**: `06-slide-outside-sections.pptx` — is a slide outside every `p14:section` accepted, repaired, or placed somewhere?\n");
    md.push_str("- **Q4c app.xml**: `07-stale-app-xml.pptx` — do stale `Words`/`Paragraphs`/`TitlesOfParts` matter (prompt, display, recomputed on save)?\n");
    std::fs::write(out.join("CHECKLIST.md"), md)?;
    println!("{} files and CHECKLIST.md written to {}", items.len(), out.display());
    Ok(())
}
