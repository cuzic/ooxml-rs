//! Writes a small, named set of edited presentations plus `CHECKLIST.md`
//! for the project owner's manual check in Microsoft 365 PowerPoint
//! (ADR-0005 §7 "Manual"; items 14–16: ADR-0008 §4 find/replace; the
//! sandbox has no PowerPoint).
//!
//! ```text
//! cargo run -p ooxml-pptx --example pptx_manual_check -- <outdir>
//! ```
//!
//! Inputs are the third-party fixtures in `reference/fixtures/pptx/`; the
//! outputs are for local inspection only and are not redistributed.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ooxml_core::Package;
use ooxml_pptx::{CellFill, Edge, Line, Presentation, ShapeKind, TableColor, TextItem};

#[path = "../tests/oracle/shape_properties_case.rs"]
mod shape_properties_case;

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

    // 14–16. Cross-run find/replace (ADR-0008 §4).
    // Each pattern is picked from the fixture so that the case applies.
    {
        // 14: a match across two runs with different formatting
        let mut p = Presentation::open(fixture("poi/with_japanese.pptx"))?;
        let mut pick = None;
        'outer: for i in 0..p.slide_count() {
            let s = p.slide(i)?;
            for sh in s.shapes() {
                let Some(b) = sh.text_body() else { continue };
                for para in b.paragraphs() {
                    let runs = para.runs();
                    for w in runs.windows(2) {
                        let (a, b) = (w[0].text(), w[1].text());
                        let differ = match (w[0].raw().elements().next(), w[1].raw().elements().next()) {
                            (Some(x), Some(y)) => !x.structurally_eq(y),
                            _ => true,
                        };
                        if a.chars().count() >= 2 && b.chars().count() >= 2 && differ {
                            let tail: String = a.chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
                            let head: String = b.chars().take(2).collect();
                            pick = Some((i, format!("{tail}{head}")));
                            break 'outer;
                        }
                    }
                }
            }
        }
        let (si, pat) = pick.ok_or("no cross-run pair")?;
        let o = ooxml_pptx::ReplaceOptions::default().find(ooxml_pptx::FindOptions::default().scope(ooxml_pptx::Scope::Slide(si)));
        let rep = p.replace_all(&pat, "【置換】", &o)?;
        items.push(Item {
            file: save(&out, "14-replace-across-runs", &p)?,
            source: "poi/with_japanese.pptx",
            edit: format!("slide {}: replace_all({pat:?} → \"【置換】\"), {} match(es) across two differently formatted runs", si + 1, rep.replaced.len()),
            observe: vec![
                "Opens without a repair prompt.",
                "The replacement shows in the format of the run where the match started; the text after it keeps its own format.",
            ],
        });
    }
    {
        // 15: a line break in the replacement (the start run's a:rPr has child elements: subtree copy)
        let mut chosen = None;
        for f in ["poi/sample.pptx", "poi/layouts.pptx", "python-pptx/ph-inherit-props.pptx", "libreoffice/slide-sections.pptx"] {
            let mut p = Presentation::open(fixture(f))?;
            for i in 0..p.slide_count() {
                let word = {
                    let s = p.slide(i)?;
                    s.shapes().into_iter().filter_map(|x| x.text_body()).flat_map(|b| b.paragraphs()).flat_map(|q| q.runs()).find_map(|r| {
                        let t = r.text();
                        let w = t.split_whitespace().nth(1)?.to_owned();
                        (w.chars().all(char::is_alphanumeric) && w.len() > 3).then_some(w)
                    })
                };
                let Some(w) = word else { continue };
                let o = ooxml_pptx::ReplaceOptions::default().find(ooxml_pptx::FindOptions::default().scope(ooxml_pptx::Scope::Slide(i)));
                let rep = p.replace_all(&w, &format!("{w}\u{b}(new line)"), &o)?;
                if !rep.replaced.is_empty() {
                    chosen = Some((f, i, w, rep.replaced.len(), p));
                    break;
                }
            }
            if chosen.is_some() {
                break;
            }
        }
        let (f, si, w, n, p) = chosen.ok_or("no plain run")?;
        items.push(Item {
            file: save(&out, "15-replace-with-line-break", &p)?,
            source: f,
            edit: format!("slide {}: replace_all({w:?} → \"{w}\\v(new line)\"), {n} match(es): a:br + a new run copying the start run's a:rPr (subtree, links skipped)", si + 1),
            observe: vec!["Opens without a repair prompt.", "\"(new line)\" starts a new line in the same paragraph (Shift+Enter), in the same format as the word before it."],
        });
    }
    {
        // 16: text inside a hyperlink run replaced (no line break): the link stays
        let mut p = Presentation::open(fixture("poi/with_japanese.pptx"))?;
        let mut pick = None;
        for i in 0..p.slide_count() {
            let s = p.slide(i)?;
            let found = s.shapes().into_iter().filter_map(|x| x.text_body()).flat_map(|b| b.paragraphs()).flat_map(|q| q.runs()).find_map(|r| {
                let linked = r.raw().elements().flat_map(|e| e.elements()).any(|c| c.name().local() == "hlinkClick");
                let t = r.text();
                let w = t.split_whitespace().last()?.to_owned();
                (linked && w.len() > 2).then_some(w)
            });
            if let Some(w) = found {
                pick = Some((i, w));
                break;
            }
        }
        let (si, w) = pick.ok_or("no hyperlink run")?;
        let o = ooxml_pptx::ReplaceOptions::default().find(ooxml_pptx::FindOptions::default().scope(ooxml_pptx::Scope::Slide(si)));
        let rep = p.replace_all(&w, &format!("[{w}]"), &o)?;
        items.push(Item {
            file: save(&out, "16-replace-in-hyperlink", &p)?,
            source: "poi/with_japanese.pptx",
            edit: format!("slide {}: replace_all({w:?} → \"[{w}]\"), {} match(es) inside a run with a:hlinkClick", si + 1, rep.replaced.len()),
            observe: vec!["Opens without a repair prompt.", "The edited text is still a working hyperlink to the same target (Ctrl+click / Slide Show)."],
        });
    }

    // SA1–SA4. Phase 1b SmartArt text (ADR-0013 §4, owner question Q1:
    // does PowerPoint re-lay out SmartArt from the data model?)
    {
        const NODE: &str = "{EC0D1448-4AB9-47C0-A4EF-4E8640283403}";
        let mut p = Presentation::open(fixture("poi/SmartArt.pptx"))?;
        p.smart_art_mut(0, 4)?.set_text(NODE, "Hello SmartArt\nSecond line")?;
        items.push(Item {
            file: save(&out, "SA1-smartart-data-and-cache", &p)?,
            source: "poi/SmartArt.pptx, slide 1",
            edit: "set_text on the first node: data model and drawing cache both get \"Hello SmartArt\" / \"Second line\"".into(),
            observe: vec![
                "Opens without a repair prompt; the first box shows the two lines.",
                "Text Pane (SmartArt Design > Text Pane) shows the same two lines; the other boxes still show [Text].",
            ],
        });
        // the fake cache: the data says DATA-TEXT, the cache CACHE-ONLY
        let mut p = Presentation::open(fixture("poi/SmartArt.pptx"))?;
        p.smart_art_mut(0, 4)?.set_text(NODE, "DATA-TEXT")?;
        let bytes = rewrite(&p.to_bytes()?, "ppt/diagrams/drawing1.xml", |d| d.replace(">DATA-TEXT<", ">CACHE-ONLY<"))?;
        let file = "SA2-smartart-fake-cache.pptx".to_owned();
        std::fs::write(out.join(&file), bytes)?;
        items.push(Item {
            file,
            source: "poi/SmartArt.pptx, slide 1",
            edit: "data model node = DATA-TEXT; the drawing cache's copy of it rewritten by hand to CACHE-ONLY (a deliberately inconsistent file)".into(),
            observe: vec![
                "Which text does the first box show right after opening: DATA-TEXT (PowerPoint re-lays out from the data) or CACHE-ONLY (it trusts the cache)?",
                "Close without saving, reopen, then save: what does the box show, and does the saved drawing1.xml still say CACHE-ONLY?",
                "If possible also on macOS, the web and iOS (ADR-0013 Q1).",
            ],
        });
        // Q2: does PowerPoint re-fit cached sizes after a longer text?
        let mut p = Presentation::open(fixture("poi/SmartArt.pptx"))?;
        p.smart_art_mut(0, 4)?.set_text(NODE, "A much longer text than the box was laid out for, to see whether the sizes are re-fitted")?;
        items.push(Item {
            file: save(&out, "SA4-smartart-long-text", &p)?,
            source: "poi/SmartArt.pptx, slide 1",
            edit: "set_text on the first node with a long text (data and cache)".into(),
            observe: vec![
                "Right after opening: does the first box shrink its font to fit (re-fit), or does the text overflow until you click into it?",
                "Do the sibling boxes change font size to match (ADR-0013 Q2)?",
            ],
        });
        let mut p = Presentation::open(fixture("poi/SmartArt.pptx"))?;
        {
            let mut sa = p.smart_art_mut(0, 4)?;
            sa.set_text(NODE, "No cache")?;
            sa.drop_drawing_cache()?;
        }
        items.push(Item {
            file: save(&out, "SA3-smartart-cache-dropped", &p)?,
            source: "poi/SmartArt.pptx, slide 1",
            edit: "set_text(\"No cache\") then drop_drawing_cache: drawing part, its relationship and the dataModelExt reference removed".into(),
            observe: vec![
                "Opens without a repair prompt; the SmartArt is laid out and the first box shows \"No cache\".",
                "After saving, PowerPoint has written a new drawing part (ppt/diagrams/drawing*.xml).",
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

    // 23–25. Phase 1b tables (ADR-0013 §7 "Manual")
    {
        let mut p = Presentation::open(fixture("poi/table_test2.pptx"))?;
        {
            let mut s = p.slide_mut(0)?;
            let mut sh = s.shape_mut_by_id(4)?;
            let mut t = sh.table_mut()?;
            t.insert_row(4)?; // inside the rowSpan=3 block (columns 4–6) and the rowSpan=2 cell (column 1)
            t.insert_column(2)?; // inside the gridSpan=2 merges of column 2–3
        }
        items.push(Item {
            file: save(&out, "23-table-insert-inside-merges", &p)?,
            source: "poi/table_test2.pptx, slide 1",
            edit: "insert_row(4) inside the vertical merges; insert_column(2) inside the horizontal merges".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "The merged cells grew by one row / one column (Q3); no cell is split or duplicated.",
                "The new cells look like their neighbours above / left (fill, margins, text direction).",
            ],
        });
    }
    {
        let mut p = Presentation::open(fixture("poi/table_test2.pptx"))?;
        {
            let mut s = p.slide_mut(0)?;
            let mut sh = s.shape_mut_by_id(4)?;
            for (r, c, txt) in [(3, 1, "one"), (3, 2, "two"), (4, 1, "three"), (4, 2, "")] {
                sh.table_cell_mut(r, c)?.set_text(txt)?;
            }
            let mut t = sh.table_mut()?;
            t.merge(3, 1, 4, 2)?;
            t.set_border(3, 1, Edge::Left, &Line::Solid { width: 38100, color: TableColor::Rgb([0xC0, 0, 0]) })?;
            t.set_border(3, 1, Edge::Bottom, &Line::Solid { width: 38100, color: TableColor::Scheme("accent2".into()) })?;
            t.set_fill(3, 1, &CellFill::Solid(TableColor::Rgb([0xFF, 0xF2, 0xCC])))?;
        }
        items.push(Item {
            file: save(&out, "24-table-merge-borders-fill", &p)?,
            source: "poi/table_test2.pptx, slide 1",
            edit: "merge(3,1)-(4,2) (texts one/two/three moved into the merged cell), thick red left border and accent-2 bottom border on the merged range, light yellow fill".into(),
            observe: vec![
                "Opens without a repair prompt; the 2×2 merged cell reads one / two / three on three lines.",
                "The red line runs along the whole left side of the merged cell, the accent line along its bottom; the neighbouring cells show the same lines (no double or missing line).",
                "Table Design > Merge/Split: Split Cells on it works as in a PowerPoint-made merge.",
            ],
        });
    }
    {
        let mut p = Presentation::open(fixture("poi/table_test2.pptx"))?;
        {
            let mut s = p.slide_mut(0)?;
            let mut sh = s.shape_mut_by_id(4)?;
            let mut t = sh.table_mut()?;
            t.delete_row(6)?;
            t.delete_column(5)?;
            t.set_style(Some("{073A0DAA-6AF3-43AB-8588-CEC1D06C72B9}"))?; // a built-in style id
        }
        items.push(Item {
            file: save(&out, "25-table-delete-and-built-in-style", &p)?,
            source: "poi/table_test2.pptx, slide 1",
            edit: "delete_row(6), delete_column(5), table style set to a built-in id ({073A0DAA-…}) that tableStyles.xml does not define".into(),
            observe: vec![
                "Opens without a repair prompt; the table frame fits the remaining rows and columns.",
                "The table shows a built-in style (Table Design shows it selected), not plain cells.",
            ],
        });
    }

    // 26–31. Phase 2 (ADR-0012 §10 "Manual"): slides as units
    {
        let mut p = Presentation::new_with(ooxml_pptx::NewOptions { created: std::time::UNIX_EPOCH })?;
        for l in p.layouts()? {
            p.add_slide(&l, None)?;
        }
        items.push(Item {
            file: save(&out, "26-new-all-layouts", &p)?,
            source: "Presentation::new() (built-in template)",
            edit: "one slide per layout, in layout order (11 slides, nothing typed)".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "Slide i uses layout i: Title Slide, Title and Content, Section Header, Two Content, Comparison, Title Only, Blank, Content with Caption, Picture with Caption, Title and Vertical Text, Vertical Title and Text.",
                "Empty placeholders show PowerPoint's own prompt text (\"Click to add title\", …); no footer, date or slide number on the slides (Q5).",
                "File > Info shows 11 slides; the notes page view shows a slide image and a notes box.",
            ],
        });
    }
    {
        let mut p = Presentation::open(fixture("poi/sample.pptx"))?;
        let l = p.layouts()?.into_iter().find(|l| l.name.as_deref() == Some("Title and Content")).ok_or("layout")?;
        p.add_slide(&l, Some(1))?;
        p.duplicate_slide(0, None)?;
        let n = p.slide_count();
        p.move_slide(n - 1, 0)?;
        items.push(Item {
            file: save(&out, "27-add-duplicate-move", &p)?,
            source: "poi/sample.pptx",
            edit: "add_slide(Title and Content, at 1); duplicate_slide(0) (copy after it); move_slide(last, 0)".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "Order: original slide 2, original slide 1, its copy (identical), the new empty Title and Content slide.",
                "The copy is independent: editing it does not change the original.",
            ],
        });
    }
    {
        let mut p = Presentation::open(fixture("libreoffice/slide-sections.pptx"))?;
        let l = p.layouts()?.into_iter().next().ok_or("layout")?;
        p.add_slide(&l, Some(1))?;
        p.add_slide(&l, Some(0))?;
        p.delete_slide(4)?;
        p.move_slide(0, 5)?;
        items.push(Item {
            file: save(&out, "28-sections", &p)?,
            source: "libreoffice/slide-sections.pptx",
            edit: "add_slide at 1 (joins slide 1's section), add_slide at 0 (joins the first slide's section), delete_slide(4), move_slide(0, 5) (joins the section of its new predecessor)".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "The section panel shows every slide in a section; the new slides sit in the sections described in the edit (owner decision Q1).",
                "Sections emptied by the delete (if any) are still listed.",
            ],
        });
    }
    {
        let mut p = Presentation::open(fixture("libreoffice/airplane-transition.pptx"))?;
        let l = p.layouts()?.into_iter().next().ok_or("layout")?;
        p.add_slide(&l, None)?;
        p.duplicate_slide(0, None)?;
        items.push(Item {
            file: save(&out, "29-only-slide2-part", &p)?,
            source: "libreoffice/airplane-transition.pptx (its only slide part is slide2.xml)",
            edit: "duplicate_slide(0), add_slide at the end: new parts are slide3.xml and slide4.xml (largest number + 1), nothing renamed".into(),
            observe: vec!["Opens without a repair prompt.", "The copy keeps the slide transition of the original."],
        });
    }
    {
        let mut p = Presentation::open(fixture("python-pptx/sld-notes.pptx"))?;
        p.duplicate_slide(0, None)?;
        p.delete_slide(2)?;
        items.push(Item {
            file: save(&out, "30-notes-duplicate-delete", &p)?,
            source: "python-pptx/sld-notes.pptx",
            edit: "duplicate_slide(0) (its notes slide is copied); delete_slide(2) (the original second slide and its notes)".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "Slides 1 and 2 have the same speaker notes; editing the notes of one does not change the other.",
                "File > Info: 2 slides (app.xml counts); the stale slide-title list in File > Properties, if visible, is accepted (owner decision Q2).",
            ],
        });
    }
    {
        let mut p = Presentation::open(fixture("poi/layouts.pptx"))?;
        let i = (0..p.slide_count())
            .find(|&i| {
                p.slide(i).ok().is_some_and(|s| s.shapes().iter().any(|x| *x.kind() == ShapeKind::Picture))
            })
            .ok_or("a slide with a picture")?;
        p.delete_slide(i)?;
        items.push(Item {
            file: save(&out, "31-delete-with-picture", &p)?,
            source: "poi/layouts.pptx",
            edit: format!("delete_slide({i}): the slide with a picture; its image part goes with it when nothing else uses it"),
            observe: vec!["Opens without a repair prompt.", "The other slides are unchanged."],
        });
    }

    // 32. ADR-0008 manual check M4: a notes edit with a formatted
    // multi-line body (§2), plus a find/replace in notes (§4 notes scope)
    {
        let mut pkg = Package::open(fixture("poi/sample.pptx"))?;
        const NOTES: &str = "ppt/notesSlides/notesSlide1.xml";
        let x = String::from_utf8(pkg.read_part(NOTES)?)?;
        let empty = "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>";
        if !x.contains(empty) {
            return Err("sample.pptx notes body changed".into());
        }
        let y = x.replacen(empty, "<a:p><a:r><a:rPr lang=\"en-US\" b=\"1\"><a:solidFill><a:srgbClr val=\"C00000\"/></a:solidFill></a:rPr><a:t>Bold red notes</a:t></a:r><a:r><a:rPr lang=\"en-US\"/><a:t> and plain text</a:t></a:r></a:p>", 1);
        pkg.set_part(NOTES, y.into_bytes())?;
        let mut p = Presentation::from_bytes(pkg.to_bytes()?)?;
        {
            let mut n = p.notes_mut(0)?;
            n.body_mut()?.set_text("Bold red notes, edited\nSecond paragraph\u{b}after a line break\nThird paragraph")?;
        }
        let o = ooxml_pptx::ReplaceOptions::default().find(ooxml_pptx::FindOptions::default().scope(ooxml_pptx::Scope::Notes(0)));
        let rep = p.replace_all("Third", "3rd", &o)?;
        let text = p.notes(0)?.map(|n| n.text()).unwrap_or_default();
        items.push(Item {
            file: save(&out, "32-M4-notes-edit", &p)?,
            source: "poi/sample.pptx (SYNTHETIC notes text)",
            edit: format!("slide 1 notes: body set_text over a bold red first run (3 paragraphs, one line break), then replace_all(\"Third\" → \"3rd\") in notes ({} match); notes text now {text:?}", rep.replaced.len()),
            observe: vec![
                "Opens without a repair prompt.",
                "Notes pane of slide 1 shows three paragraphs; the second has a line break (Shift+Enter) before \"after a line break\".",
                "All three paragraphs are bold red (new paragraphs copy the format of the first run of the last paragraph, Phase 0 set_text).",
                "The slide itself and slide 2's notes are unchanged.",
            ],
        });
    }

    // P3a: formatted title range, paragraph alignment and autofit.
    {
        use ooxml_pptx::{AutoFitMode, FontRef, FormatChange, RunFill, RunFormat, WriteColor};
        use ooxml_schema::dml::TextAlignmentTypeValues;

        let source = "poi/sample.pptx";
        let mut p = Presentation::open(fixture(source))?;
        let id = find_shape(&mut p, 0, |shape| shape.text_body().is_some())?;
        let mut slide = p.slide_mut(0)?;
        let mut shape = slide.shape_mut_by_id(id)?;
        let mut body = shape.text_body_mut()?;
        body.autofit(AutoFitMode::Shape)?;
        let mut para = body.paragraph_mut(0)?;
        para.align(TextAlignmentTypeValues::Center)?;
        para.format_range(0, 5, &RunFormat {
            bold: Some(FormatChange::Set(true)),
            size: Some(FormatChange::Set(2800)),
            latin: Some(FormatChange::Set(FontRef::Typeface("Arial".into()))),
            fill: Some(FormatChange::Set(RunFill::Solid(WriteColor::Rgb([0x12, 0x34, 0x56]).into()))),
            ..RunFormat::default()
        })?;
        items.push(Item {
            file: save(&out, "p3a-text-formatting", &p)?,
            source,
            edit: "Format the first five title characters, centre the paragraph, and set shape autofit.".into(),
            observe: vec![
                "Opens without a repair prompt.",
                "Lorem is bold, 28 pt Arial, and dark blue. The remaining title text keeps its original formatting.",
                "The title paragraph is centred and retains every character.",
            ],
        });
    }

    {
        let source = shape_properties_case::FIXTURE;
        let mut p = Presentation::open(fixture(source))?;
        shape_properties_case::edit(&mut p)?;
        items.push(Item {
            file: save(&out, shape_properties_case::NAME, &p)?,
            source,
            edit: "On slide 3, move and resize Rectangle 1. Change it to a blue up arrow rotated 30° and flipped horizontally, with an orange dashed outline and arrowheads. Give Oval 2 a vertical white-to-teal gradient. Fit Group 4 around its members.".into(),
            observe: vec![
                "The blue up arrow and its orange outline appear above and left of the other members.",
                "Oval 2 has a white-to-teal gradient.",
                "Selecting Group 4 shows a bounding box around all three shapes. Compare Oval 2 and Right Arrow 3 against the source to check that fitting preserved their positions and sizes.",
            ],
        });
    }

    let mut md = String::from("# ooxml-pptx Phase 0 / 0.1 (1–13) and cross-run find/replace (14–16, ADR-0008 §4), inheritance (17–22, ADR-0008 §7 M1–M7), tables (23–25, ADR-0013), slides as units (26–31, ADR-0012), notes (32, ADR-0008 M4), and SmartArt text (SA1–SA4, ADR-0013) — manual check in Microsoft 365 PowerPoint\n\n");
    md.push_str("Generated by `cargo run -p ooxml-pptx --example pptx_manual_check -- <outdir>` (ADR-0005 §7, Manual).\n");
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

/// A copy of a package with one part's text rewritten (manual-check
/// fixtures that the library deliberately cannot produce).
fn rewrite(bytes: &[u8], name: &str, f: impl Fn(String) -> String) -> R<Vec<u8>> {
    use std::io::{Read as _, Write as _};
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        let n = e.name()?.into_owned();
        let mut v = Vec::new();
        e.read_to_end(&mut v)?;
        if n == name {
            v = f(String::from_utf8(v)?).into_bytes();
        }
        w.start_file(n, zip::write::SimpleFileOptions::default())?;
        w.write_all(&v)?;
    }
    Ok(w.finish()?.into_inner())
}
