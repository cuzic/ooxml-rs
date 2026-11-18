# PR #51 レビュー（Opus, round 1）

対象: `impl/pptx-phase2` @ `397cce0`。ADR-0012（Accepted rev 3、owner の Q1〜Q5 決定と実装ノートを含む）と照合した。
リポジトリは変えていない。作業は scratchpad の `review-51-57/` で行い、`git clone` したコピーを使った。

- `pr51/`: `397cce0` のまま。`tests/probe51.rs`（プローブ P1〜P4）と `examples/rsweep51.rs`（全出力を保存する sweep）を置いた。
- `mut51/`: レビュアーのテストを入れていないコピー。`scripts51/mut51.py` の mutant 25 個を流した。
- `scripts51/check51.py`: レビュアー独自の構造チェッカー（Python、入力と出力の比較）。カスケードの期待値は独立に計算する。
- `pr51/` の `mergeprobe` ブランチ: 現在の GitHub main（`5f50cf6`、#52 を含む）をマージしたもの。

## 結論（先に）

**未収束（Blocker 0、Major 1）。** 実装は ADR-0012 の規則をおおむね正確に写しており、corpus と独立 oracle でも新しい構造欠陥は出なかった。ただし、§5 の文言どおりに実装された「スライドが 0 枚でセクションがある場合」の扱いが、§5 自身の不変条件（セクションの連結 = `p:sldIdLst`）を破る。その後の構造操作はすべて `SectionsInconsistent` で拒否され、保存したファイルも同じ状態になる（M1）。

- 検証で問題がなかったもの:
  - corpus sweep（617 デッキ、出力 3,522 本をすべて保存して検査）: 新しいダングリング関係 0、content type のない新しいパート 0、触れていないパートの変更 0、`sldId` の重複 0、セクション不一致 0、`app.xml` の `Slides`/`Notes`/`HiddenSlides` の誤り 0。拒否はすべてバイト同一で、panic は 0。
  - delete のカスケード: 1,161 件すべてで、レビュアーが独立に計算した集合（§6 の規則、外部からの参照で不動点まで落とす）と一致した。
  - 作者が「入力の欠陥」とした 12 件の帰属: 正しい。全出力について入力との差分を取り、新しく生じた欠陥が 0 であることを確かめた。差分が出た 4 件もすべて入力由来か、チェッカー側の表記差だった（後述）。
  - python-pptx: 出力だけが読めないケースは 0（16 件は入力も読めない）。作者の structure oracle は 233 cases ok を再現した。テンプレートの 11 レイアウトすべてで、`add_slide` の結果は python-pptx の `add_slide` と同じだった（shape id、名前、`p:ph` の属性、`txBody` の有無）。
  - #38 の staging 規則: `stage_part` に渡すのは、`xml_part` の clone か新しい木だけである（`app.xml` は `read_part` を parse し直したもの。N1 の規則では detach される側なので安全）。`set_relationship_target` は使っていないので、retarget と remove が 1 つの plan に同居することはない。操作ごとに plan は 1 つで、refusal はすべて `apply` の前に起きる。
  - `cargo test --workspace`、`codegen --check`、`pptx-template --check`: 通った。現在の main（#52 を含む）へのマージも衝突なしで、55 binary すべてが通った。
- mutant: レビュアーの 25 個のうち 15 個は検出された。意味のある生存は 6 個ある（m1）。作者の「23/23」とは集合が違う。

## Major

### M1 [Major] セクションがあり、スライドが 0 枚のデッキに add / duplicate すると、スライドがどのセクションにも入らない。以後の構造操作はすべて拒否される

`crates/ooxml-pptx/src/slides.rs:963-972`（`insert_into_sections` の `else { return Ok(()); // no slides: nothing to join }`）。ADR-0012 §5（`docs/adr/0012-pptx-phase2-slides-and-shapes.md:460`）の「with no slides or no sections, nothing」をそのまま実装している。しかし同じ §5 の 451 行目は「Invariant checked before and kept after every operation」と書き、468 行目は「the rules above never create one」と書いている。ADR の中で矛盾している。

再現（`probe51.rs::p1_sections_zero_slides_then_add`）:

1. `libreoffice/slide-sections.pptx` のスライドを `delete_slide(0)` で全部消す。セクションは 2 つ残り、どちらも空になる。ここまでは正しい。
2. `add_slide(&layouts[0], None)` は `Ok` を返す。保存すると、`sldIdLst = [405]` で、セクションはどちらも空のままになる。
3. 同じセッションの 2 回目の `add_slide`、`move_slide(0, 0)`、保存したファイルを開き直しての `delete_slide(0)` は、すべて `Err(SectionsInconsistent)` になる。

作者のテスト `sections_follow_add_move_delete_duplicate` は `while p.slide_count() > 1` で止めているので、この経路を通らない。

提案:

- 0 枚のときは、**最初のセクション**の先頭に入れる（at は 0 しかありえない）。PowerPoint で、空のセクションだけのデッキに新しいスライドを足したときの振る舞いに近いと思われる。manual check の項目にも加える。
- ADR-0012 に post-acceptance note を足し、§5 の「with no slides …, nothing」を「with no sections, nothing; with sections but no slides, the first section」に直す。
- 代わりに拒否（`SectionsInconsistent` を先回りして返す）にしてもよい。ただし「全部消してから作り直す」という自然な使い方が止まるので、推奨しない。
- テスト: 全部消す → add → move → delete の後で、`check_structure` を通す。

## Minor

### m1 [Minor・テスト] 生存した mutant 6 個にテストを足す

`scripts51/mut51.py`（`--test structure --test new_deck --lib`、release）の結果。

| mutant | 場所 | 意味 |
|---|---|---|
| R01 カスケードの「外から参照される候補を落とす」ループを 1 回で止める | `slides.rs:696-710` | 共有チャートの埋め込みが候補に残る。core の `remove_parts` が拒否するので壊れはしないが、成功すべき delete が `Core(…)` になる。共有チャート（またはその他の共有された木）を持つスライドの delete テストがない |
| R07 `show="false"` を hidden と見なさない | `slides.rs:101-103` | `hidden_slides_are_counted` は `show="0"` だけを使っている |
| R10 `max_creation_id` がノートスライドを見ない | `slides.rs:337-358` | 複製したスライドの値が、既存のノートスライドの値と衝突しうる |
| R14 tags を「異なる target ごとに 1 回」ではなく関係ごとにコピーする | `slides.rs:749-763` | ADR §4.4 N3 の規則（2 つの `r:id` が同じ tags を指す場合）にテストがない |
| R17 位置 0 への追加で、次のスライドのセクションではなく常に最初のセクションに入れる | `slides.rs:963-968` | ADR §5 m11（「空の最初のセクションは暗黙に埋めない」）にテストがない。最初のセクションが空のデッキで、位置 0 に add するテストを足す |
| R23 コメントの `Id` を他の場所で参照していても拒否しない | `slides.rs:768-775` | `UnsupportedContent`（`p188:commentRel` 以外からのコメント `Id` 参照）の拒否経路にテストがない |

ほかに生存したもの: R21（パッケージ関係からの参照、source が `None`）は、実データで起こりにくいが、1 行のテストで検出できる。R09（空の `p:extLst` を残す）は、`CT_ExtensionList` の `ext` が `minOccurs=0` なのでほぼ等価。R15 と R24 は等価か、それに近い。

### m2 [Minor・文書] ADR-0012 §1 の 5〜6（`add_text_box`、`add_picture`、`ensure_notes_slide`）がこの PR にない。そのことが ADR に記録されていない

- PR 本文には「Follow-ups, not in this PR (coordinator's decision)」とある。しかし ADR の post-acceptance note（実装）は、「`next_shape_id` を `add_text_box`/`add_picture` のために公開した」としか書いていない。§1 の Scope は、今も Phase 2 に入っていると読める。
- そのため、レビュー依頼にある「media の content type: `Override` へのフォールバック（N7）」は、実装もテストも存在しない。この PR では検証できない。
- 提案: ADR-0012 の実装ノートに「§4.5、§4.6 は follow-up PR で実装する（coordinator の判断、日付）。N7 はそちらで検証する」と 1 項目足す。README の Phase 2 の表にも「not yet」を 1 行足す。

### m3 [Minor・テスト] `tests/oracle/structure.py` が、ADR §10 の oracle の一部を検査していない

- §10 は、構造 oracle の条件として「入力で到達不能だったもの以外に、到達不能なパートがない」を挙げている。しかし `check_structure` はこれを検査していない。delete のカスケードの中身（何が消えて何が残るか）、`app.xml` の数、ノートのコピーの中身も検査していない。
- 「触れていないパートは同じバイト」の判定は、Rust 側が書く固定の `CHANGED` リストを除外に使っている。
- レビュアーの `check51.py` は、カスケードの期待値（到達集合を外部参照で不動点まで削る）、到達不能パートの差分、`app.xml` の真値との比較を独立に行い、全出力 3,522 本で 0 件だった。同等の検査（少なくとも、カスケードの期待値と到達不能パートの差分）を oracle に入れることを推奨する。そうすれば R01 のような退行が CI で見える。

## Nit

- **n1**: `p14:creationId` にはセッションの high-water がない。`sld-notes.pptx` で duplicate → その複製を delete → もう一度 duplicate とすると、2 つ目の複製も同じ値 `2300479740` になる（`probe51.rs::p3`）。`sldId` は high-water で必ず違う値になり、`pc:sldMk` は両方を持つので、modern comments が誤って結び付くことはない。ADR §3 の文言（「全スライドの max + 1」）とも一致する。§3 に「`creationId` はセッション内で再利用されうるが、`sldId` が違うので害はない」と 1 文足せば十分である。
- **n2**: ADR §6 は、カスケードの型を「transitional と、定義があれば strict/Microsoft の URI」と書いている。しかし `names.rs` の定数は transitional だけである。strict URI の関係は `UnsupportedSlidePart` で拒否されるので fail closed だが、ADR と実装のどちらかを揃える。
- **n3**: `delete_slide` のカスタムショー検査（`slides.rs:565-576`）は、`p:sldIdLst` の `r:id` だけを見る。カスタムショーが同じスライドを**別の** `rId` で指す場合は、core の `remove_parts` が拒否する（staged の `presentation.xml` がまだその `Id` を名指ししているため）。fail closed だが、エラーは `SlideInCustomShow` ではなく `Core(…)` になる。presentation から slide part への全関係の `Id` で照合すれば揃う。
- **n4**: `docs/adr/0001-lossless-round-trip-architecture.md:187` に、main との差分として空行が 1 行だけ入っている（意図しない差分と思われる）。

## sweep の詳細（`rsweep51` と `check51.py`）

操作は次のとおり: 全スライドの delete と duplicate、`move(0 → 末尾)`、最初のレイアウトでの `add`、chain（全スライドを duplicate し、その複製を delete）。

| 結果 | 件数 |
|---|---:|
| delete ok / duplicate ok | 1,161 / 1,017（作者の 1,163 / 1,019 との差は、`pptx/out` の 2 デッキを入れていないため） |
| add ok / move ok / chain ok | 614 / 113 / 617 |
| 拒否（パッケージは不変） | 型 `customXml`、`control`、`chartEx`、`legacyDrawing`、カスタムショー 14、`SlideIsReferenced` 20、show range 14 ほか |
| `REFUSAL_CHANGED` / panic | 0 / 0 |

`check51.py` で差分が出た 4 件は、すべて実装の欠陥ではなかった。

- `tdf169781` delete 1 の「新しい到達不能パート」`image4.png`: 入力で孤立していた `legacyDrawing2.vml` がこの画像を参照している。§6 の「外からの関係があれば残す」に従った結果で、正しい。
- `tdf135843_export` と `sc055-075` の「複製の XML が違う」: 書き換えた `p14:creationId` 要素だけが再シリアライズされ、`" />"` が `"/>"` になった。チェッカー側の表記差である。
- `sc054-074_google_slides` の「ノートの逆向きの関係がない」: 入力のノートにもない（Google Slides の出力）。

## 判定

**converged: no**

- Blocker: なし。
- Major: M1（スライド 0 枚のセクション。実装と ADR §5 の文言を直す）。
- Minor: m1（mutant R01、R07、R10、R14、R17、R23 のテスト）、m2（§4.5、§4.6 の先送りを ADR に記録する）、m3（oracle にカスケードと到達不能パートの検査を入れる）。
- Nit: n1〜n4。

M1 を直し、m1 のうち R17 と M1 のテストを入れれば、次の round で収束する見込みである。
