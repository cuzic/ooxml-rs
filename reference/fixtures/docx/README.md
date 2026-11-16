# Word (.docx) fixtures

Twenty-six third-party `.docx` files (879 KB in total, largest 236 KB), chosen to exercise
specific WordprocessingML shapes for the Word roadmap (`docs/roadmap/docx.md`) and
ADR-0004 (`docs/adr/0004-docx-phase0.md`). Every file is unmodified upstream data. None
was produced by ooxml-rs.

**Treat these files as untrusted input.** Inspect them only with `python3 -I` and
`zipfile`/`ElementTree` (see `spikes/docx/`), or with ooxml-rs itself. Never open them in an
application that runs macros, and never execute anything inside them. Two of them are
deliberately hostile or unreadable (`poi-ExternalEntityInText.docx`,
`poi-bug53475-password-is-pass.docx`).

## Provenance and licences

| Prefix | Source | Revision | Licence | Notices kept here |
|---|---|---|---|---|
| `poi-` | Apache POI, `test-data/document/` (https://github.com/apache/poi/tree/trunk/test-data/document) | trunk `47dcff769bb395034dddfd59c4609659b6dbc6a4` (2026-11-13), downloaded 2026-11-16 | Apache License 2.0 | `licenses/apache-poi-LICENSE.txt`, `licenses/apache-poi-NOTICE.txt` (Apache-2.0 §4(d) requires the NOTICE to travel with redistributed parts of the Work) |
| `sdk-` | .NET Open XML SDK, `test/DocumentFormat.OpenXml.Tests.Assets/assets/` (https://github.com/dotnet/Open-XML-SDK) | `431ab05cf160248cc3885a4a766026d4f8243792` (the same commit ADR-0002 vendors its schema data from) | MIT, © .NET Foundation and Contributors | `licenses/open-xml-sdk-LICENSE.txt` |

The only change is the file name: the source prefix is added, and the SDK's directory path is
shortened. Each file's Git blob SHA-1 was checked against the upstream tree listing, and all 25
match. The SHA-256 values below are for local verification.

`poi-documentProtection_readonly_no_password.docx` was added on 2026-11-16 for ADR-0004 rev 2's
`DocumentProtected` gate (review M3). It comes from the same POI revision; its blob SHA-1
`1ec7063ab526ecc69f2f8cf10df669be2b3970d1` matches the upstream tree listing.

The upstream POI test files come from bug reports, so their content may have been authored by
third parties. They are redistributed here under the same terms as the POI test suite. The SDK
test files are Microsoft test assets. A few of them (`sdk-HelloO14.docx`) were edited by hand
upstream to create edge cases.

## What each file exercises

"Producer" is `docProps/app.xml`'s `Application`/`AppVersion`, where present. "Phase0" is
the outcome of the gates in ADR-0004 revision 2, §6, as implemented by `crates/ooxml-docx` (and
mirrored by `spikes/docx/phase0_gate.py`). Run counts are runs with a `w:t` in the body flow.

| File | Upstream name | Producer | Exercises | Phase0 |
|---|---|---|---|---|
| `poi-delins.docx` | `delins.docx` | Word 12 | Tracked changes: 32 `w:ins`, 4 `w:del` (8 `w:delText`), 46 `w:rPrChange`, paragraph-mark revisions; `<w:trackRevisions/>` written as an **empty element** (CT_OnOff: means *on*); 14 complex `HYPERLINK` fields; numbering; `xml:space="preserve"` | Err `TrackRevisionsOn` |
| `poi-FieldCodes.docx` | `FieldCodes.docx` | Word 14 | Complex fields `AUTHOR`, `CREATEDATE` with cached results (`begin`/`separate`/`end`); `w14:paraId` | Editable; both text runs refused (inside a field) |
| `poi-FldSimple.docx` | `FldSimple.docx` | Word 14 | `w:fldSimple` (`FILENAME`) with a cached-result run | Editable; no settable run |
| `poi-Bug60341.docx` | `Bug60341.docx` | none (SDK-style generator) | UTF-8 BOM before the XML declaration; body containing only `w:sectPr`; footer part named `word/footer.xml` (not `footerN.xml`), found only through the relationship; `PAGE`/`NUMPAGES` inside an SDT in the footer; `w:t` with leading/trailing spaces and **no** `xml:space` | Editable (body has no text) |
| `poi-testComment.docx` | `testComment.docx` | Word 16 | Comment with all of `comments.xml`, `commentsExtended.xml` (w15), `commentsIds.xml` (w16cid), `commentsExtensible.xml` (w16cex); an image inside the comment (`comments.xml.rels`) | Editable |
| `poi-shapes-with-text.docx` | `shapes-with-text.docx` | Word 16 | `mc:AlternateContent` as a child of `w:r` (7×; `wps`/`wpg` in `mc:Choice`, VML in `mc:Fallback`); text boxes (`w:txbxContent`) duplicated across both branches, so **5 `w14:paraId` values occur twice** | Editable; text-box runs are not body flow (not returned by `runs()`); the runs holding the shapes are `RunNotTextOnly` |
| `poi-table-alignment.docx` | `table-alignment.docx` | Word 16 (hand-edited) | Pretty-printed `document.xml`; **10 duplicated `w14:paraId`** (copy-pasted table rows); 6 tables; 12 words split across runs; zip **directory entries** (`word/`, `_rels/`, …) and 5 stored (uncompressed) entries | Editable (66/66 runs) |
| `poi-PageSpecificHeadFoot.docx` | `PageSpecificHeadFoot.docx` | Word 12 | Default + even headers/footers, `w:evenAndOddHeaders`; `fldSimple PAGE`; SDTs with `w:dataBinding` (date, text) inside `w:p` and `w:tr`; `vMerge`; glossary document part | Editable (15/15 runs) |
| `poi-Headers.docx` | `Headers.docx` | Word 12 | Three sections: `w:sectPr` inside `w:pPr` (section-break paragraphs) plus the body-level `w:sectPr`, each with its own header | Editable |
| `poi-55733.docx` | `55733.docx` | none (minimal generator) | Body **without** `w:sectPr`; no `styles.xml`; UTF-8 BOM; `<w:b w:val="false"/>` | Editable (1/1); append goes at the end of the body |
| `poi-smarttag-snippet.docx` | `smarttag-snippet.docx` | Outlook 12 | `w:smartTag` (8×) wrapping runs: an ECMA Transitional element **absent from the SDK's `CT_P`**, so an unlisted child for ADR-0002 | Editable (6/14); runs inside `smartTag` refused (`UnsupportedContext`) |
| `poi-ExternalEntityInText.docx` | `ExternalEntityInText.docx` | Word 14 (edited) | `<!DOCTYPE … <!ENTITY testent SYSTEM "http://poi.apache.org/">` and `&testent;` in a `w:t`: an XXE probe | Err `DtdNotAllowed` |
| `poi-bug53475-password-is-pass.docx` | `bug53475-password-is-pass.docx` | — | Password-protected document: a CFB/OLE container, not a zip (password `pass`) | Err `EncryptedPackage` |
| `poi-documentProtection_readonly_no_password.docx` | `documentProtection_readonly_no_password.docx` | Word 12 | `<w:documentProtection w:edit="readOnly" w:enforcement="1"/>` without a password hash | Err `DocumentProtected` |
| `poi-checkboxes.docx` | `checkboxes.docx` | Word 12 | Legacy form fields (`FORMCHECKBOX` with `w:ffData`), one field without `separate`; first/even/default headers and footers; `framePr` | Editable (15/15) |
| `poi-bug59058.docx` | `bug59058.docx` | Word 14 | Large real-world document (865 text paragraphs, 196 with several runs, 38 words split across runs); 296 `w:hyperlink`, 10 complex `HYPERLINK` fields; 79 numbered paragraphs; 18 inline pictures; 13 stored zip entries | Editable (3,271/3,281 runs, 307 of them in `w:hyperlink`) |
| `poi-61745.docx` | `61745.docx` | Word 15 | Two inline charts (`c:chart` in `a:graphicData`) with chart style/colour parts and embedded `.xlsx` workbooks: Layer 1 passthrough of a dependent part chain | Editable (no text) |
| `sdk-TOC2.docx` | `v2FxTestFiles/wordprocessing/Toc/TOC2.docx` | Word 12 | `TOC` field whose result spans several paragraphs, with nested `PAGEREF` fields, `_Toc` bookmarks and `w:hyperlink w:anchor` | Err `StaleFieldRisk` |
| `sdk-Field-more-field.docx` | `v2FxTestFiles/wordprocessing/field/Field-more field.docx` | Word 12 | `DATE` field whose **instruction is split over 9 `w:instrText` runs** (`DATE \@ "h时m分s秒"`) and whose cached result is split over 6 runs; `AUTHOR`, `PAGE` | Editable; 8 of 13 runs refused (inside fields) |
| `sdk-equation.docx` | `v2FxTestFiles/wordprocessing/equation/equation.docx` | Word 12 | OMML: 8 `m:oMath`, 1 `m:oMathPara` inside paragraphs | Editable (12/12 direct runs) |
| `sdk-coverpage1.docx` | `v2FxTestFiles/wordprocessing/cover page/coverpage1.docx` | Word 12 | Cover page: block SDT with `w:docPartObj`, data-bound SDTs (5 `w:dataBinding`) in `w:p`/`w:tr`/`w:tc`, date picker | Editable; all 8 runs refused (inside SDT, `UnsupportedContext`) |
| `sdk-ftEdNote.docx` | `v2FxTestFiles/wordprocessing/footnote & endnote/ftEdNote.docx` | Word 12 | Two footnotes and two endnotes plus the separator entries (`w:type="separator"`, `continuationSeparator`) | Editable (7/7) |
| `sdk-Comment010.docx` | `O15Conformance/WD/CommentExTest/Comments-Sample-15-12-01/Comment010.docx` | Word 15 | 12 comments with replies: `commentsExtended.xml` with `w15:paraIdParent` and `w15:done="1"`; `people.xml`; each commentEx `w15:paraId` equals the `w14:paraId` of the comment's last paragraph (12/12); `w15` is bound to the **pre-release URI** `http://schemas.microsoft.com/office/word/2010/11/wordml` (Office 2013 beta), which is not in the SDK's `namespaces.json`, so ooxml-rs must treat it as unknown and preserve it | Editable (16/16) |
| `sdk-HelloO14.docx` | `TestFiles/HelloO14.docx` | Word 14 (hand-edited) | `mc:AlternateContent` as a **direct child of `w:body`**; pretty-printed with BOM | Editable; append refused (`UnresolvedSibling`) |
| `sdk-strict-CommentRangeStart.docx` | `O14ISOStrict/Word/CommentRangeStart.docx` | Word 14 | ISO/IEC 29500 **Strict** namespaces (`http://purl.oclc.org/ooxml/wordprocessingml/main`) | Err `StrictNotSupported` |
| `sdk-complex2010.docx` | `TestFiles/complex2010.docx` | Word 14 | "Kitchen sink": 209 bookmarks, 693 `w14:paraId`, 2 comments, first-page header, `EQ` (11), `SEQ`, `=` formula, `DATE`/`TIME` fields, `w:sym`, OLE object, `mc:AlternateContent` in runs, anchored `wps` shapes, `framePr`; a `TC` field whose instruction contains a nested `SEQ` | Err `StaleFieldRisk` (`=`, `SEQ`, `TC`; the outer `TC` is found only with the field stack of rev 2) |

### Coverage the brief asked for

| Shape | Files |
|---|---|
| Text split across runs | `poi-bug59058`, `poi-table-alignment`, `sdk-Field-more-field`, `sdk-TOC2` |
| Fields (simple, complex, TOC, PAGE) and cached results | `poi-FldSimple`, `poi-FieldCodes`, `sdk-Field-more-field`, `sdk-TOC2`, `poi-Bug60341`, `poi-PageSpecificHeadFoot`, `poi-checkboxes` |
| Tracked changes | `poi-delins` |
| Document protection | `poi-documentProtection_readonly_no_password` |
| Content controls (SDT) | `sdk-coverpage1`, `poi-PageSpecificHeadFoot`, `poi-Bug60341` |
| Headers/footers | `poi-PageSpecificHeadFoot`, `poi-Headers`, `poi-checkboxes`, `poi-Bug60341` |
| Footnotes/endnotes | `sdk-ftEdNote` |
| Comments incl. extended/ids/people | `poi-testComment`, `sdk-Comment010`, `sdk-complex2010` |
| `mc:AlternateContent` in runs / in body | `poi-shapes-with-text`, `sdk-complex2010` / `sdk-HelloO14` |
| `xml:space` (with and without) | `poi-delins`, `poi-bug59058`, `poi-Bug60341` |
| Numbering | `poi-delins`, `poi-bug59058`, `poi-checkboxes` |
| ID anomalies (duplicate `w14:paraId`) | `poi-table-alignment`, `poi-shapes-with-text` |
| Sections (`sectPr` in `pPr`, missing `sectPr`) | `poi-Headers`, `poi-55733` |

### Known gaps (searched, not found)

- **Range markers before `w:tblPr`** (the ambiguous-slot case of ADR-0002 §4.1): no occurrence
  in 129 POI + 502 SDK files (`spikes/docx/survey.py`, key `tbl_marker_before_tblPr`). Word
  writes a bookmark that covers a whole table as a sibling of `w:tbl`. The case stays covered by
  the generator's unit test, not by a real file.
- **`w:altChunk`**: no occurrence in either corpus, or in the docx4j and LibreOffice sample
  names searched. A fixture needs a real file from a producer that writes it (e.g. an HTML
  import).
- **`w14:checkbox` content control**: only in `TestFiles/Complex01.docx` and `Document.docx`
  (≈ 0.8 MB each). Left out to keep the set small; add one if Phase 3 (SDT) starts.
- **Glossary document**: present in `poi-PageSpecificHeadFoot` (passthrough only); no
  fixture is chosen *for* it.
- **Word 2019+/Microsoft 365 output** (`w16du`, `w16sdtdh`, …): the newest producers here
  are Word 16 (2016). The project owner may want to add a file saved by a current Word.
- **Macro-enabled/template packages** (`.docm`, `.dotx`): not included. The main part's
  content type differs, which Phase0 accepts, but nothing tests it.

## Checksums (SHA-256)

```
b5c30080ffe667ef27414302ffd3ccd01ef1c85d4a5a459761b9c82c3421a4e0  poi-55733.docx
ec54badce04b9fc3c4c327e31fbbd06c1bb556056df9cf04760db75c8913f8a2  poi-61745.docx
5d4c919f2e06b84fbe35cfaaa4012e8f469b811e1f643deb3e660b798bfe4544  poi-documentProtection_readonly_no_password.docx
c182f7825ae588fe1c28497588d1bded635a5bef7eb7370fd1c589233a6471d5  poi-bug53475-password-is-pass.docx
89d0abb8eed1a58f262c49c386f74d8a82798d6344bed690d66b389514f0751c  poi-bug59058.docx
56147fba379742c9b73e98787ff171c7ed37b038e623ed26749dab6b90a009ab  poi-Bug60341.docx
06f1dc1370c772a020c6d5183c8b7c4fe30ad7a9fff108cb2f027128ddf5e209  poi-checkboxes.docx
7ecad102602586be7f1371f117fc567ea47f3ed0ca703b353e003002f1ff8d3f  poi-delins.docx
5cf7340ffd6516c2fd168f445ff78cf64bd86409fe69db68997fc9f94820c223  poi-ExternalEntityInText.docx
9247f85ab90b15f11d0cf14d0070fdcb5cdf47c65d6bc77cc32cb7ee682aad24  poi-FieldCodes.docx
6cf7364676d465e702cbbac44ca92331c525476e36a29c26f780cb9d7643b631  poi-FldSimple.docx
bdf91038f9a5454355e6393b7b74fd819a4adc480b4a7011a86e878403952b5b  poi-Headers.docx
90bb8251b18c9f94e0b8fb91192413922a09dbe418a4a210116221054378eaad  poi-PageSpecificHeadFoot.docx
1ef4d8cbfd5e2333897ab3193b58ce445cee7e45476d39e5626a4c7097f72d95  poi-shapes-with-text.docx
693ad667f07b6ca90c4375e024a122adb970e9f5cc7e29a1cd53e0a9cbfb6de9  poi-smarttag-snippet.docx
ee36729e0d050e529fea0b444006a194bea72b35cf8958ae58c6624c26156b41  poi-table-alignment.docx
06b63b13f0b78a7179151af52c2282557c1ed204899feee9fcef4ca575344f97  poi-testComment.docx
5989f09cabe485526b6dbfe59a83b1ac517dd26a0a3667b37f571f3984705649  sdk-Comment010.docx
752e6a16a622527017691f3269fc50b665eeef2e5e68daa1226fed8232ef31c5  sdk-complex2010.docx
728b7885cd5920a67a412c74b5fca6c7930e028f91dfc09b14f1559728e69517  sdk-coverpage1.docx
a0d7a34186e37e303ef10f6469834221d92ae74c886078d7f32f2dc1a7fd96a5  sdk-equation.docx
d7b71329f0d18a627975cb6eb0dd15c03dc6237a6b5779e8fe0239ef7a805f8b  sdk-Field-more-field.docx
43148c7be8ae5790131c5404f80c16603ce87cf7178e841a29928aeabe924ffa  sdk-ftEdNote.docx
4c2bb833050ee40f56522b74beb2ca81176d305996adbc26829e5117aa025e92  sdk-HelloO14.docx
a6aa3203a42e2095294e107687bb2c392b3a8cb6bc6cc1b20946960a8e8026d3  sdk-strict-CommentRangeStart.docx
4bb4acb8ade97234ce85ace0b1dc60b1a29d5efdcdb48c8b037bc01a123a7631  sdk-TOC2.docx
```

Verify with `cd reference/fixtures/docx && sha256sum -c` on the block above.
