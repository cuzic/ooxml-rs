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
pub(crate) struct SlideEntry {
    pub(crate) slide_id: u32,
    pub(crate) part: Option<String>,
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
/// if let Some(id) = pres.slide(0)?.placeholders().first().map(|p| p.id()).transpose()? {
///     pres.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.set_text("New title")?;
///     pres.save("deck.pptx")?;
/// }
/// # Ok::<(), ooxml_pptx::Error>(())
/// ```
///
/// A complete, runnable version is `examples/replace_text.rs`.
#[derive(Debug)]
pub struct Presentation {
    pub(crate) pkg: Package,
    pub(crate) slides: Vec<SlideEntry>,
    /// The presentation part (`ppt/presentation.xml`).
    pub(crate) main: String,
    /// Session high-water mark of `p:sldId@id`: the largest id seen,
    /// allocated or removed while this value lives (ADR-0012 §3, review
    /// m2/N6). Lost when the `Presentation` is dropped.
    pub(crate) slide_id_high_water: u32,
    /// Table edit state that outlives one `SlideMut` (deleted `a16` ids),
    /// by slide part (ADR-0013 §2: the session is the `Presentation`).
    tables: std::collections::BTreeMap<String, crate::write::TableSession>,
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

    pub(crate) fn from_package(mut pkg: Package) -> Result<Self> {
        let main = pkg.main_document_part().ok_or_else(|| Error::NotAPresentation("no officeDocument relationship".into()))?;
        if !pkg.contains(&main) {
            return Err(Error::NotAPresentation(format!("officeDocument target {main:?} does not exist")));
        }
        let slides = scan_slides(&mut pkg, &main)?;
        let slide_id_high_water = slides.iter().map(|e| e.slide_id).max().unwrap_or(0);
        Ok(Presentation { pkg, slides, main, slide_id_high_water, tables: Default::default() })
    }

    /// Re-reads the slide list after a structural edit.
    pub(crate) fn rescan(&mut self) -> Result<()> {
        self.slides = scan_slides(&mut self.pkg, &self.main)?;
        self.tables.retain(|part, _| self.pkg.contains(part));
        Ok(())
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
        let table_styles = self.table_style_ids();
        let tables = self.tables.entry(part.clone()).or_default();
        let doc = self.pkg.xml_part_mut(&part)?;
        if !doc.root().is(pml::Slide::QNAME) {
            return Err(Lens::<pml::Slide>::root(doc.root()).unwrap_err().into());
        }
        refs.scan_timing(doc.root());
        Ok(SlideMut { doc, info, refs, table_styles, tables })
    }

    /// `a:tblStyle@styleId`s defined in the presentation's `tableStyles`
    /// part (found by relationship); empty when absent or unreadable.
    fn table_style_ids(&self) -> Vec<String> {
        const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
        let Some(main) = self.pkg.main_document_part() else { return Vec::new() };
        let Ok(rels) = self.pkg.relationships(&main) else { return Vec::new() };
        let Some(part) = rels.by_type(REL).filter_map(|r| rels.resolve(r)).find(|p| self.pkg.contains(p)) else {
            return Vec::new();
        };
        let Some(doc) = self.pkg.read_part(&part).ok().and_then(|b| XmlDocument::parse(&b).ok()) else { return Vec::new() };
        doc.root()
            .elements()
            .filter(|e| e.is(QName::new(crate::names::A_NS, "tblStyle")))
            .filter_map(|e| e.attr(QName::new("", "styleId")).map(str::to_owned))
            .collect()
    }

    /// Parses the whole deck once — every slide's layout, master and theme,
    /// the comment parts — and returns a read view (ADR-0008 §1.1, §5).
    /// Never marks anything dirty.
    pub fn deck(&mut self) -> Result<crate::Deck<'_>> {
        let main = self.pkg.main_document_part().ok_or_else(|| Error::NotAPresentation("no officeDocument relationship".into()))?;
        let entries: Vec<(u32, Option<String>)> = self.slides.iter().map(|e| (e.slide_id, e.part.clone())).collect();
        let (slides, rels) = crate::deck::load(&mut self.pkg, &main, &entries)?;
        let pkg = &self.pkg;
        let pres_part = pkg.part_names().find(|n| *n == main).ok_or_else(|| Error::NotAPresentation(main.clone()))?;
        let pres = pkg.parsed_part(&main).ok_or_else(|| Error::NotAPresentation(main.clone()))?;
        Ok(crate::Deck { pkg, pres_part, pres, slides, rels })
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
        // Apply. Every check ran above, so a failure here is a bug; it is
        // still made harmless (review #48 m4): each slide's tree is cloned
        // before it is changed, and on any error every changed slide is
        // restored, so `Err` leaves the package byte-identical.
        let mut backups: Vec<(String, XmlDocument)> = Vec::new();
        let mut failure = None;
        'apply: for (i, ps) in plans {
            if !ps.iter().any(|(_, _, _, p)| p.changes()) {
                continue;
            }
            let part = match self.entry(i) {
                Ok((_, part)) => part,
                Err(e) => {
                    failure = Some(e);
                    break 'apply;
                }
            };
            match self.pkg.xml_part(&part) {
                Ok(doc) => backups.push((part, doc.clone())),
                Err(e) => {
                    failure = Some(e.into());
                    break 'apply;
                }
            }
            let mut slide = match self.slide_mut(i) {
                Ok(s) => s,
                Err(e) => {
                    failure = Some(e);
                    break 'apply;
                }
            };
            for (body, pc, loc, plan) in ps {
                if let Err(e) = replace::apply_plan(&mut slide, &body, pc, &plan) {
                    failure = Some(e);
                    break 'apply;
                }
                for range in plan.accepted {
                    report.replaced.push(Match { location: loc.clone(), range });
                }
            }
            #[cfg(test)]
            if fault::FAIL_AFTER_SLIDE.get() == Some(i) {
                failure = Some(Error::UnsupportedContent("injected fault (test)".into()));
                break 'apply;
            }
        }
        if let Some(e) = failure {
            // restore every slide even if one restore fails (review #48 r2
            // nit); the first error of all is returned
            for (part, doc) in backups {
                match self.pkg.xml_part_mut(&part) {
                    Ok(d) => *d = doc,
                    Err(_) => core::hint::cold_path(),
                }
            }
            return Err(e);
        }
        Ok(report)
    }
}

/// The slide list of the presentation part (`p:sldIdLst`, `r:id`s resolved).
pub(crate) fn scan_slides(pkg: &mut Package, main: &str) -> Result<Vec<SlideEntry>> {
    let rels = pkg.relationships(main)?;
    let doc = pkg.xml_part(main)?;
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
    Ok(slides)
}

#[cfg(test)]
pub(crate) mod fault {
    use std::cell::Cell;

    thread_local! {
        /// Fault injection for `replace_all`'s apply loop (review #48 m4).
        pub(crate) static FAIL_AFTER_SLIDE: Cell<Option<usize>> = const { Cell::new(None) };
    }

    #[test]
    fn a_failure_while_applying_restores_every_slide() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx/poi/sample.pptx");
        let input = std::fs::read(path).unwrap();
        let mut pres = super::Presentation::from_bytes(input).unwrap();
        let before = pres.to_bytes().unwrap(); // the package as written without any change
        // "e" occurs on both slides: slide 0 is changed, then the fault hits after slide 1
        FAIL_AFTER_SLIDE.set(Some(1));
        let r = pres.replace_all("e", "E", &crate::ReplaceOptions::default());
        FAIL_AFTER_SLIDE.set(None);
        assert!(r.is_err());
        assert_eq!(pres.to_bytes().unwrap(), before, "a failed replace_all must leave the package byte-identical");
        // and without the fault the same call changes both slides
        let rep = pres.replace_all("e", "E", &crate::ReplaceOptions::default()).unwrap();
        assert!(rep.replaced.iter().any(|m| m.location.slide == 0) && rep.replaced.iter().any(|m| m.location.slide == 1));
    }
}
