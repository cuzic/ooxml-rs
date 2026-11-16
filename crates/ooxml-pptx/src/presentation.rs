//! `Presentation`: open/save, the slide list, lazy slide access (ADR-0005 §2).

use std::path::Path;

use ooxml_core::{ElementSpec, Lens, Package, QName, XmlDocument};
use ooxml_schema::pml::{self, SlideId, SlideIdList, SlideIdView};

use crate::error::{Error, Result};
use crate::names::MODERN_COMMENTS_REL;
use crate::read::{Slide, SlideInfo};
use crate::refs::SlideRefs;
use crate::replace::{self, BodyCtx, FindOptions, Match, OnRefusal, ParaPlan, ReplaceOptions, ReplaceReport, Scope, TextLocation};
use crate::write::{SlideMut, node_at};

/// (text body path, paragraph child index, location, plan) of one paragraph.
type PlannedParagraph = (Vec<usize>, usize, TextLocation, ParaPlan);

#[derive(Clone, Debug)]
struct SlideEntry {
    slide_id: u32,
    part: Option<String>,
}

/// An open PowerPoint package (`.pptx`, `.pptm`, `.ppsx`, `.potx`).
///
/// Every part that is not edited is written back with its original
/// compressed bytes (ADR-0001 Layer 1). Only slide parts are ever edited;
/// the presentation part is parsed read-only.
///
/// # Examples
///
/// ```no_run
/// use ooxml_pptx::Presentation;
///
/// let mut pres = Presentation::open("deck.pptx")?;
/// for i in 0..pres.slide_count() {
///     for shape in pres.slide(i)?.shapes() {
///         if let Some(body) = shape.text_body() {
///             println!("slide {i}, shape {:?}: {}", shape.name(), body.text());
///         }
///     }
/// }
/// let id = pres.slide(0)?.placeholders()[0].id()?;
/// pres.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.set_text("New title")?;
/// pres.save("deck.pptx")?;
/// # Ok::<(), ooxml_pptx::Error>(())
/// ```
///
/// A complete, runnable version is `examples/replace_text.rs`.
#[derive(Debug)]
pub struct Presentation {
    pkg: Package,
    slides: Vec<SlideEntry>,
}

impl Presentation {
    /// Opens a file (read fully into memory).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_package(Package::open(path)?)
    }

    /// Opens a package from bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::from_package(Package::from_bytes(bytes)?)
    }

    fn from_package(mut pkg: Package) -> Result<Self> {
        let main = pkg.main_document_part().ok_or_else(|| Error::NotAPresentation("no officeDocument relationship".into()))?;
        if !pkg.contains(&main) {
            return Err(Error::NotAPresentation(format!("officeDocument target {main:?} does not exist")));
        }
        let rels = pkg.relationships(&main)?;
        let doc = pkg.xml_part(&main)?;
        let pres = Lens::<pml::Presentation>::root(doc.root())
            .map_err(|_| Error::NotAPresentation(format!("{main:?} is not a p:presentation")))?;
        if pres.attr(QName::new("", "conformance")) == Some("strict") {
            return Err(Error::StrictConformance);
        }
        let mut ids = Vec::new();
        if let Some(list) = pres.child::<SlideIdList>() {
            for sld in list.children::<SlideId>() {
                let v = SlideIdView(sld);
                let slide_id = v.id()?.ok_or_else(|| Error::NotAPresentation("p:sldId without @id".into()))?;
                ids.push((slide_id, v.relationship_id().map(String::from)));
            }
        }
        let slides = ids
            .into_iter()
            .map(|(slide_id, rid)| SlideEntry {
                slide_id,
                part: rid
                    .and_then(|r| rels.by_id(&r).and_then(|r| rels.resolve(r)))
                    .filter(|p| pkg.contains(p)),
            })
            .collect();
        Ok(Presentation { pkg, slides })
    }

    /// Saves to `path` (temp file + rename; the same path is safe).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        Ok(self.pkg.save(path)?)
    }

    /// The package as bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.pkg.to_bytes()?)
    }

    /// The underlying package (read-only), e.g. to read other parts.
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// Number of entries in `p:sldIdLst`.
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    fn entry(&self, index: usize) -> Result<(SlideInfo, String)> {
        let e = self.slides.get(index).ok_or(Error::SlideIndexOutOfRange { index, count: self.slides.len() })?;
        let part = e.part.clone().ok_or(Error::DanglingSlide { slide_id: e.slide_id })?;
        Ok((SlideInfo { index, slide_id: e.slide_id, part_name: part.clone() }, part))
    }

    /// Slides in `p:sldIdLst` order; an entry whose `r:id` does not resolve
    /// to an existing part is `Err(DanglingSlide)` (never skipped).
    pub fn slides(&self) -> impl Iterator<Item = Result<SlideInfo>> + '_ {
        (0..self.slides.len()).map(|i| self.entry(i).map(|(info, _)| info))
    }

    /// The `index`-th slide, parsed on first access. Never marks anything dirty.
    pub fn slide(&mut self, index: usize) -> Result<Slide<'_>> {
        let (info, part) = self.entry(index)?;
        let doc = self.pkg.xml_part(&part)?;
        Lens::<pml::Slide>::root(doc.root())?;
        Ok(Slide { doc, info })
    }

    /// The `index`-th slide for editing. Scans the slide's `p:timing` and
    /// its modern comment parts (read-only; they stay Raw) so that edits
    /// of referenced text can be refused (ADR-0005 §6).
    pub fn slide_mut(&mut self, index: usize) -> Result<SlideMut<'_>> {
        let (info, part) = self.entry(index)?;
        let mut refs = SlideRefs::default();
        let rels = self.pkg.relationships(&part)?;
        for r in rels.by_type(MODERN_COMMENTS_REL) {
            // Fail closed: a comment part that cannot be located, read or
            // parsed — for whatever reason, including resource limits of the
            // XML layer — is `None`, and `scan_comments(None)` locks the text
            // of **every** shape of the slide (ADR-0005 §6). The error itself
            // is not propagated: the slide stays readable and editable
            // outside text (and its text edits are refused with
            // `CommentAnchoredText`).
            let doc = match rels.resolve(r).filter(|p| self.pkg.contains(p)) {
                None => None,
                Some(p) => match self.pkg.read_part(&p) {
                    Err(_) => None,
                    Ok(b) => XmlDocument::parse(&b).ok(),
                },
            };
            refs.scan_comments(doc.as_ref());
        }
        let doc = self.pkg.xml_part_mut(&part)?;
        if !doc.root().is(pml::Slide::QNAME) {
            return Err(Lens::<pml::Slide>::root(doc.root()).unwrap_err().into());
        }
        refs.scan_timing(doc.root());
        Ok(SlideMut { doc, info, refs })
    }

    fn scope_indices(&self, scope: Scope) -> Result<Vec<usize>> {
        match scope {
            Scope::Slides => Ok((0..self.slides.len()).collect()),
            Scope::Slide(i) => {
                self.entry(i)?;
                Ok(vec![i])
            }
        }
    }

    /// Every literal match of `pattern` in the paragraphs of the slides in
    /// scope (ADR-0008 §4): text bodies of `p:sp` (groups included), table
    /// cells (merged ones too) and the representative shape of an
    /// `mc:AlternateContent`. Matches never cross paragraphs; byte ranges
    /// refer to [`crate::ParagraphRef::text`]. Never marks anything dirty.
    ///
    /// Errors: [`Error::InvalidPattern`], [`Error::DanglingSlide`],
    /// [`Error::SlideIndexOutOfRange`], parse errors.
    pub fn find(&mut self, pattern: &str, opts: &FindOptions) -> Result<Vec<Match>> {
        replace::check_pattern(pattern)?;
        let mut out = Vec::new();
        for i in self.scope_indices(opts.scope)? {
            let slide = self.slide(i)?;
            let root = slide.doc.root();
            for t in replace::targets(slide.doc) {
                let body = node_at(root, &t.body).expect("target path");
                for (k, pc) in replace::paragraphs(body).into_iter().enumerate() {
                    let text = replace::paragraph_text(node_at(body, &[pc]).expect("paragraph"));
                    for range in replace::find_in(&text, pattern, opts.ascii_case_insensitive) {
                        out.push(Match {
                            location: TextLocation { slide: i, shape: t.shape.clone(), cell: t.cell, paragraph: k },
                            range,
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    /// Replaces every match of `pattern` (as [`Self::find`]) with
    /// `replacement`, by the rules of ADR-0008 §4 (see the `replace`
    /// module): the run where a match starts keeps its format, covered runs
    /// are removed or trimmed, fields and line breaks inside a match are
    /// refused, a `\v` in the replacement becomes `a:br`. Every match of
    /// the scope is checked before the first change; refused matches are
    /// reported in [`ReplaceReport::skipped`] ([`OnRefusal::Skip`]) or stop
    /// the whole call without any change ([`OnRefusal::Abort`]).
    ///
    /// Errors (nothing changed): [`Error::InvalidPattern`],
    /// [`Error::LineBreakInParagraph`] and [`Error::InvalidXmlChar`] for the
    /// replacement, [`Error::DanglingSlide`], [`Error::SlideIndexOutOfRange`],
    /// parse errors.
    pub fn replace_all(&mut self, pattern: &str, replacement: &str, opts: &ReplaceOptions) -> Result<ReplaceReport> {
        replace::check_pattern(pattern)?;
        let segs: Vec<String> = replace::segments(replacement)?.into_iter().map(String::from).collect();
        let segs: Vec<&str> = segs.iter().map(String::as_str).collect();
        let indices = self.scope_indices(opts.find.scope)?;
        let mut report = ReplaceReport::default();
        // (slide, [(body path, paragraph child, location, plan)])
        let mut plans: Vec<(usize, Vec<PlannedParagraph>)> = Vec::new();
        for &i in &indices {
            let mut slide = self.slide_mut(i)?;
            let mut slide_plans = Vec::new();
            for t in replace::targets(slide.doc) {
                let body_ctx: std::result::Result<_, Error> = slide.shape_mut(&t.shape).and_then(|mut sh| match t.cell {
                    None => sh.text_body_mut().map(|b| b.guard().clone()),
                    Some((r, c)) => sh.table_cell_mut(r, c).map(|b| b.guard().clone()),
                });
                let root = slide.doc.root();
                let body = node_at(root, &t.body).expect("target path");
                let blocked_err = body_ctx.as_ref().err();
                let blocked = || blocked_err.expect("blocked").duplicate();
                for (k, pc) in replace::paragraphs(body).into_iter().enumerate() {
                    let ctx = BodyCtx {
                        blocked: blocked_err.map(|_| &blocked as &dyn Fn() -> Error),
                        guard: body_ctx.as_ref().ok(),
                    };
                    let para_path = [t.body.as_slice(), &[pc]].concat();
                    let plan = replace::plan_paragraph(slide.doc, &para_path, pattern, &segs, opts, &ctx);
                    if plan.accepted.is_empty() && plan.refused.is_empty() {
                        continue;
                    }
                    let loc = TextLocation { slide: i, shape: t.shape.clone(), cell: t.cell, paragraph: k };
                    slide_plans.push((t.body.clone(), pc, loc, plan));
                }
            }
            plans.push((i, slide_plans));
        }
        for (_, ps) in &mut plans {
            for (_, _, loc, plan) in ps.iter_mut() {
                for (range, e) in plan.refused.drain(..) {
                    report.skipped.push((Match { location: loc.clone(), range }, e));
                }
            }
        }
        if opts.on_refusal == OnRefusal::Abort && !report.skipped.is_empty() {
            report.aborted = true;
            return Ok(report);
        }
        for (i, ps) in plans {
            if !ps.iter().any(|(_, _, _, p)| p.changes()) {
                continue;
            }
            let mut slide = self.slide_mut(i)?;
            for (body, pc, loc, plan) in ps {
                replace::apply_plan(&mut slide, &body, pc, &plan)?;
                for range in plan.accepted {
                    report.replaced.push(Match { location: loc.clone(), range });
                }
            }
        }
        Ok(report)
    }
}
