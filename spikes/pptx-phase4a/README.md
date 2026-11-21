# PowerPoint Phase 4a spike (modern comment writes)

Evidence for [ADR-0019](../../docs/adr/0019-pptx-phase4a-modern-comments.md).
This is a throwaway script, not production code. It reads untrusted `.pptx` files
with `zipfile` + `xml.etree` only. Parts that carry a DTD or an entity
declaration are skipped, nothing is extracted or executed, and packages are
de-duplicated by SHA-256. Run it with `python3 -I` from a directory that does
not contain the files.

| Script | Usage | Result |
|---|---|---|
| `comments_survey.py` | `python3 -I comments_survey.py <dirs/files…>` | [`results/comments-748.txt`](results/comments-748.txt) |
| `comments_survey.py` (P4a-0 rerun) | `python3 -I comments_survey.py reference/fixtures/pptx/project` | [`results/comments-p4a0-project.txt`](results/comments-p4a0-project.txt): the 10 PowerPoint-made fixtures (ADR-0019 post-acceptance note) |

What it counts:

- legacy comments: parts, `p:cm` attributes, date forms, `p15:parentCm` replies,
  `p:cm/p:extLst` URIs;
- comment authors: `p:cmAuthor` attributes, `clrIdx` against `id`, and the
  `p15:presenceInfo` provider and user-id forms;
- modern comments: part names against `sldId`/`p14:creationId` in hex,
  content types, the root's and the monikers' namespace declarations,
  `p188:cm` attributes and child order, anchor kinds and their monikers,
  whether `pc:sldMk` agrees with the slide, and whether `ac:*Mk@id` and
  `@creationId` agree with the shape (`a16:creationId`), text offsets,
  `ac:context`, `p188:pos`, replies, statuses, `txBody` content and
  `p188:extLst`;
- `p188:author` attributes, providers and user-id forms;
- the GUID form of every id (braces, case, version nibble, variant);
- the slide `p:extLst` URIs and `p188:commentRel` against the relationship;
- `mc:AlternateContent` in comment and author parts and on slides with
  comments;
- the presentation `p:extLst` URIs and `viewPr@showComments`;
- slides and decks with both comment kinds, and author registries without
  comments;
- coverage of `p14:creationId` (slides) and `a16:creationId` (shapes), and
  shapes at the top level of `p:spTree` against nested in groups.

## Corpora (downloaded to scratch, not committed)

These are the ADR-0008 sources, widened to every `.pptx` at the same pins.
They were fetched on 2026-11-21 with `gh api …/git/trees/<sha>?recursive=1`
plus `raw.githubusercontent.com` (as in `spikes/xlsx-phase7/README.md`):

| Corpus | Path filter | Files | Pin |
|---|---|---:|---|
| Apache POI | `test-data/slideshow/*.pptx` | 95 | `47dcff769bb395034dddfd59c4609659b6dbc6a4` |
| LibreOffice | `sd/qa/unit/data/pptx/*.pptx` (tree `fd95fe174f2d`) | 420 | `2e885d0bda9bc4ee9f3bde51905c4a7820b01f06` |
| ShapeCrawler | every `*.pptx` | 100 | `2ab474918ac89e7bbb8d7c649aca708d298171e2` |
| Open XML SDK | every `*.pptx` (incl. `TestDataStorage`) | 182 | `431ab05cf160248cc3885a4a766026d4f8243792` |
| python-pptx | every `*.pptx` | 67 | `278b47b1dedd5b46ee84c286e77cdfb0bf4594be` |
| `reference/fixtures/pptx/` | committed | 24 | this repository |

```sh
# one corpus; repeat with each row's repository, pin and filter
SHA=47dcff769bb395034dddfd59c4609659b6dbc6a4; OUT=$(mktemp -d)/poi
mkdir -p "$OUT"
gh api "repos/apache/poi/git/trees/$SHA?recursive=1" \
  --jq '.tree[]|select(.path|startswith("test-data/slideshow/"))|select(.path|test("\\.pptx$"))|.path' |
while IFS= read -r p; do
  curl -sfL -o "$OUT/$(printf %s "$p" | tr '/ ' '__')" \
    "https://raw.githubusercontent.com/apache/poi/$SHA/$(python3 -I -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$p")"
done
# LibreOffice's tree is too large for one recursive listing: list the
# sd/qa/unit/data/pptx subtree by its own sha (fd95fe174f2d…).
python3 -I spikes/pptx-phase4a/comments_survey.py <corpus dirs> reference/fixtures/pptx
```

The 2026-11-21 run (Python 3.15) listed 888 files, found 44 duplicates by
SHA-256, and read **748 presentations (1,580 slides)**. It skipped 84 Strict
packages, 11 that are not zip files and 1 CFB (encrypted) file. It takes
about 4 s.

## Results (quoted in ADR-0019)

- **Comments are rare in public corpora.** 5 decks have legacy comments (7
  parts, 11 `p:cm`). 2 decks have modern comments (2 parts, 2 `p188:cm`, 2
  replies): `libreoffice/modern-comments.pptx` and the project's
  PowerPoint-made `project/powerpoint-shape-comment.pptx`. No deck and no
  slide has both kinds.
- **Author registries outlive their comments.** 32 decks have
  `commentAuthors.xml` and no legacy comment, and 1 deck has `authors.xml`
  and no modern comment. The LibreOffice file `tdf147586.pptx` has both
  registries and no comment at all.
- **Modern comment parts** (2/2): the name is
  `ppt/comments/modernComment_{sldId:X}_{creationId:X}.xml` in upper-case hex
  without padding, and both values match the slide. The content type is
  `application/vnd.ms-powerpoint.comments+xml`. The root declares `a`, `r` and
  `p188`, and `pc`/`ac` are declared locally on each moniker. The slide
  carries `p:extLst/p:ext uri="{6950BFC3-D8DA-4A85-94F7-54DA5524770B}"`
  holding one `p188:commentRel r:id` that matches the relationship, and its
  `p:extLst` is the slide's last child. Neither comment part, neither slide
  nor the presentation has any `mc:AlternateContent`. No presentation
  `p:extLst` URI relates to comments: the corpus has only `{EFAFB233-…}` 291,
  `{2D200454-…}` 56 and `{521415D9-…}` 21.
- **Anchors** (2/2): `ac:deMkLst[pc:docMk, pc:sldMk, ac:spMk]` on a top-level
  `p:sp`. `pc:sldMk@sldId` and `@cId` match the slide. `ac:spMk@creationId`
  equals the shape's `a16:creationId` in the PowerPoint file and is
  all-zero in the LibreOffice-touched one. There is no `p188:pos`, no text
  offset and no `ac:context`.
- **`p188:cm`**: child order `ANCHOR, [replyLst,] txBody`. `status` is
  absent on both, there is no `p188:extLst` and no other attribute. A
  `txBody` is `a:bodyPr, a:lstStyle, a:p+`, with `a:p` → `a:r` → `a:rPr
  lang` + `a:t` only (4/4 bodies).
- **Ids**: every `p188:cm`/`p188:reply` id (4) is a braced, upper-case
  RFC 4122 **version 4** GUID. The 3 `p188:author` ids are braced and
  upper-case but **not v4** (version nibbles `A`, `3`, `F`; 2 of 3 have a
  non-RFC variant). Author ids therefore look derived, not random *(u)*.
- **Timestamps**: every `created` (4) and 8 of 11 legacy `dt` are
  `YYYY-MM-DDTHH:MM:SS.mmm` with no zone (millisecond precision). The other
  3 legacy `dt` come from LibreOffice (9 fractional digits).
- **`p188:author`**: the PowerPoint-made files have all five attributes. In
  the PowerPoint fixture, `userId` = `name` and `providerId="None"` (the
  identity was edited, see the fixture README). `tdf147586` has
  `providerId="AD"` and `userId="S::<upn>::<guid>"`.
  `libreoffice/modern-comments.pptx` lacks `userId` and `providerId`, both
  required by the SDK schema, and it does not open in PowerPoint 365
  (issue #114). That the missing attributes are the cause is *(u)*.
- **Legacy authors** (63): all have `id`, `name`, `initials`, `lastIdx` and
  `clrIdx`. `clrIdx` equals `id` 26 times and differs 37 times.
  `p15:presenceInfo` appears 20 times (`{19B8F6BF-…}`): `providerId` is `AD`
  13, `None` 4 (with `userId` = name, PowerPoint files), `Windows Live` 3.
- **Creation ids**: 820 of 1,580 slides have `p14:creationId`. 1,217 of
  7,629 drawing elements have `a16:creationId`. 2,738 of 7,629 drawing
  elements are nested in groups.
  Shapes inside `mc:AlternateContent` are counted in **both** the Choice
  and the Fallback branch, so the shape totals are slightly high (ADR-0019
  review n2). The proportions quoted in the ADR are not affected materially.

## Reference writers read for the ADR (not vendored)

- LibreOffice `2e885d0bda9bc4ee9f3bde51905c4a7820b01f06`,
  `sd/source/filter/eppt/pptx-epptooxml.cxx`. `WriteModernComments` (L1988–2085) and
  `WriteModernAuthors` (L1828–1853) write a modern part **next to** the
  legacy one for every slide (L2342–2355). The modern part has no anchor
  (the SDK schema requires one), no `p188:commentRel`, and authors without
  `userId`/`providerId`. Comment, reply and author ids are random
  (`tools::Guid::Generate`). Authors are keyed by name only (L1855–1864).
- Apache POI `47dcff76`: legacy comments only (`XSLFComments`). python-pptx
  1.0.2 reads no comments at all.
