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

## P4a-0 evidence (`p4a0/`)

Inputs and COM probes behind ADR-0019's P4a-0 post-acceptance note (items 5,
9 and 10). They were made on 2026-11-21 in the same PowerPoint
16.0.20326.20318 as the fixtures. They are evidence, not test fixtures, so
no test reads them.

| File | What it is |
|---|---|
| `probe-p1.pptx` … `probe-p5.pptx`, `probe-author-one.pptx`, `probe-ada-lovelace.pptx` | A new blank deck each, with one slide comment added through COM `Comments.Add2(10, 10, name, initials, text, providerId, userId)` with the dummy identities of the note's table, then `SaveAs`. They show the author-id derivation (item 5) and supply the replacement ids used in `reference/fixtures/pptx/project/` |
| `rederive-in.pptx` | `project/powerpoint-slide-comment.pptx` with the author id replaced by the random v4 `{0F1E2D3C-4B5A-4978-8695-A4B3C2D1E0F9}` (made on Linux) |
| `rederive-out.pptx` | `rederive-in.pptx` opened in PowerPoint and saved with `SaveAs`: the id is kept |
| `mixed-deck-input.pptx` | The hand-made mixed deck (item 10), made by `python3 -I p4a0/make_mixed_input.py ../../reference/fixtures/pptx/poi/45545_Comment.pptx p4a0/mixed-deck-input.pptx`. PowerPoint's save of it is `project/powerpoint-mixed-deck-resaved.pptx`. Derived from an Apache-2.0 POI file (`reference/fixtures/pptx/LICENSES/apache-poi-*`) |
| `make_mixed_input.py` | The script above (standard library only) |

**Anonymised:** `docProps/core.xml` `dc:creator` and `cp:lastModifiedBy`
named the signed-in account in the PowerPoint-saved decks and became
`Author One`. Every author in these files is a dummy identity. The files
were grepped for the account's name, email and author id.

```
a3df7a180c4a1e29ad977cdc82377fc2d41b1841452597ee8efb98d71ac1cb33  mixed-deck-input.pptx
1ea28d847ce54edbea4aeafaaf5f862b9c13cefedca80488664589af2eefd42f  probe-ada-lovelace.pptx
a56378a7ccab913989a6d9c5cb72497784dd27a45870148dcf08c9fd9fb334e1  probe-author-one.pptx
51e83bbe1bc045cf0c44a82120d5cbbf1f6ba9d3103bfe0c964260dbf1a3018b  probe-p1.pptx
0e551ccbf8ed2ebbc8df546ded0550a3c2125ae0a6f0d00664b7aad0dd2df239  probe-p2.pptx
b92e1f00b8c1c1a0d32702cf154862bf591657f496084e1de0f6fad346f0c673  probe-p3.pptx
47d9ef3fe20e99d00bfbc4538c01763c1fc8a5c178c97ff3f47d3cd28a906531  probe-p4.pptx
840e73d574f5bc6cc611c8c1a37ab2b24f17a1132ef876ffa6c29b620121b146  probe-p5.pptx
a0fafface82c507e4242a15a21d15e0b1152ac2c2d22df7e24adf82fb8441fe1  rederive-in.pptx
e02fb843b59dc09c26d13c3066c85e86b56b29a3cb68f113c9d8bbfbb3a34b19  rederive-out.pptx
1aab44da5f384a4c9d4c4a59cb508c8b4720ee678f0e98fdc3c12820fe4cfda5  make_mixed_input.py
```

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
