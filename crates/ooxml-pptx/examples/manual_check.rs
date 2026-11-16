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
