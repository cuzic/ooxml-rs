# ADR-0012 レビュー（Opus, round 1）

対象: PR #30 `docs/adr/0012-pptx-phase2-slides-and-shapes.md`（Proposed, rev 1）、`spikes/pptx-phase2/`。
照合先: ADR-0001（main と PR #14 の interim note、`core-remove-part`）、ADR-0002、ADR-0005（main / rev 4
`impl-pptx-phase0-1-copy`）、ADR-0008 draft（`plan-pptx-phase1`）、ADR-0009 と round-1 review
（`plan-xlsx-phase3`）、core 追加系 primitive の作業ブランチ `core/add-part`（`core-add-part` worktree。commit
`a153161` と、レビュー時点の未コミット `crates/ooxml-core/src/package/add.rs`：`RegistryPlan`、
`PartNameRequest::Numbered`、`remove_parts`、`incoming_relationships`）。
独立プローブ: POI `test-data/slideshow`（HEAD を sparse clone、95 ファイル）+ `reference/fixtures/pptx`。
スクリプトは `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr30/probe/`
（`probe.py`, `notes.py`, `spid.py`。`python3 -I` で実行し、zip/XML の読み取りだけ）。python-pptx 1.0.2 と POI
の該当ソース（`max_shape_id`、`_next_id`、`new_placeholder_sp`、`XMLSlideShow.removeSlide`）はローカルで確認した。

## 結論（先に）

方向性は正しい。id とパート名を振り直さない、`max + 1` で割り当てる、POI のプロンプト文コピーではなく python-pptx
型のプレースホルダーを使う、リンク先の削除や custom show を拒否する、`TitlesOfParts` を推測で書き換えない、という
判断はどれも妥当で、比較表も出典どおりだった（python-pptx `max_shape_id` = `//@id`、`_next_id` の gap 探索、
POI `removeSlide` が sections・custom shows・入ってくるリンクを触らないこと）。

ただし、このままでは実装できないか、黙って壊れる箇所が 1 つ（B1）、Major が 4 つある。

1. **B1**: 「全ステップを最初の変更の前に検証し、拒否時はバイト同一」という主張が、§2 の primitive の組み合わせでは
   成り立たない。`presentation.xml` の `p:sldId` を消す前は、スライドの削除がコアの `StillReferenced` で必ず
   拒否される。消した後に検証するなら、拒否されたときには既に変更が入っている。ADR-0009 B2 と同じ構造の問題で、
   core ブランチの `RegistryPlan` でもレジストリ以外のパート編集はまだ扱えない。
2. **M1**: duplicate の「`.rels` を同じ `Id` のまま複製する」は、Id をコアが割り当てる C3 では表現できない。
3. **M2**: 削除の前提チェック「別スライドのジャンプリンク先ではない」が狭い。レイアウト・マスター・ノート・
   SmartArt からの `slide` 関係、`viewProps` のアウトライン `p:sld r:id`、Slide Zoom の `sldId` 参照が漏れる。
4. **M3**: ADR-0001 note の文案が、interim note の安全条件（関係タイプの allowlist）を落としている。後継関係も
   ADR-0003 / ADR-0009 / core ブランチの note と衝突する。
5. **M4**: §2 の C1–C6 が core ブランチの実 API と食い違っている（C5 と C6 の意味、C2 と C4 の有無、check 双子と
   plan の違い）。

## 判定

**Accepted after Minor only: no.** B1 と M1–M4 を反映した rev 2 で round 2 を行う。Minor は rev 2 で一緒に
直せばよい。

---

## Blocker

### B1. plan → validate → apply とバイト同一の主張が §2 の primitive では成り立たない

**根拠:**

- ADR §4 は「every refusal of §9 and every core `check_*` of §2 for every step happens before the first
  change; the package is byte-identical after a refusal」と書いている。
- PR #14 の `remove_part` と、core ブランチの `Package::remove_parts` / `RegistryPlan::remove_parts` は、残る
  ソースパートの本文を `pkg.read_part(src)` で読み、`refers_to_rel_id` で関係 Id を探す（`add.rs` の removal
  内）。`presentation.xml` には `<p:sldId r:id="rIdN">` があるので、削除前の状態で検証すると
  presentation→slide の関係は必ず `StillReferenced` になる。
- 通すには、先に Layer 2 で `p:sldId` を外す（必要ならセクションの `p14:sldId` も外す）しかない。ところが、
  その後で `remove_parts` が拒否されると（`UnresolvedMention`、無関係な `.rels` の `RegistryNotEditable`、
  M2 の参照など）、`presentation.xml` は既に変わっている。lens の tree を元に戻しても、パートの状態は
  `Raw` から `Parsed` に変わり、再圧縮されるので、ADR-0005 T2 型の「compressed bytes 同一」は保証されない。
- 追加側も同じ構造になる。新しいスライドの XML、`p:sldId r:id`、セクション、`app.xml` はレジストリ外の編集
  なので、`RegistryPlan` の `apply`（generation が進む）と順序づけが要る。`apply` の後に lens 編集が失敗すると、
  中途半端な状態が残る。
- core ブランチの `RegistryPlan` は、追加と削除を 1 つの plan に積み、`apply` を失敗させない形まで進んでいる。
  欠けているのは「plan に、レジストリ以外のパートを編集後の内容として渡す」手段だけ。

**推奨:**

- core に `RegistryPlan::stage_part(pkg, name, XmlDocument | bytes)`（ADR-0009 review B2 の `stage_replace`）
  を置く。`remove_parts` / `remove_relationship` の `StillReferenced` 判定は、ステージ後の内容で行う。
  `Package::apply` はレジストリ編集とステージ済みパートを同時に installs し、失敗しない。
- ADR §4 では、各操作を「(1) presentation.xml・app.xml・新規パートの tree を複製上で作る → (2) 1 つの
  `RegistryPlan` にステージと追加・削除を積む（ここまで `&Package`）→ (3) `apply`」と書く。§9 の拒否と core
  の拒否はすべて (1)(2) で出るようにする。
- これが core に入るまでは、「byte-identical after a refusal」を「拒否時の状態は未定義」に弱めてはいけない。
  Phase 2 の開始条件にする。
- テスト T4 に「core 側で拒否されるケース」（例: 削除対象スライドを指す、Id 参照つきの `slide` 関係を
  レイアウトに持つデッキ）を加え、`presentation.xml` を含む全パートのバイト同一を確かめる。

---

## Major

### M1. duplicate の「同じ Id で `.rels` を複製」は C3 では書けない

**根拠:**

- §4.4: 「its `.rels` a copy of the original's with the same `Id`s」。
- C3 も core の `RegistryPlan::add_relationship` も、Id を `rId{max+1}` で割り当てる。呼び出し側は Id を
  指定できない。さらに、`.rels` パートを `add_part` で直接足すことはできない（`RegistryPart` で拒否される）。
- そのため実装は、(a) 複製したスライド XML の `r:id`/`r:embed`/`r:link`/`r:pict` などを新しい Id に
  書き換える（どの属性を書き換えるかの規則が要る。any-attribute で一致した値を置き換える方式は、偶然同じ値を
  持つ非関係属性まで書き換える）か、(b) 新しい core primitive を使うか、のどちらかになる。ADR はどちらも
  定めていない。
- コーパスに実例がある: `with_japanese.pptx` の slide1 は内部 `hyperlink` 関係で `Target="#_ftn1"`
  （TargetMode なし）を 4 本持つ。`Target::Part` で張り直すと、解決先パートが無いので `PartNotFound` 相当で
  拒否される。
- 複製元が `ppt/slides/` 以外にある場合（スキーマ上は合法）、相対 `Target` を byte copy すると壊れる。新しい
  パートは `/ppt/slides/slide{N}.xml` に置かれるので、ターゲットの解決先が変わる。

**推奨:**

- core に `RegistryPlan::copy_relationships(pkg, from_source, to_new_source, filter)` を置く。新しいパート
  （同じ plan で追加したもの）に限り、元の `Id`・`Type`・`TargetMode` を保ったまま、Target を新しいソース
  からの相対パスとして再計算して書く。
- フィルタで `notesSlide` を除き、複製したノートへの関係は通常の `add_relationship` で張る。複製 XML 側は
  `notesSlide` の Id を参照しないので、Id がずれても問題ない。
- 解決できない内部ターゲット（`#_ftn1` 型）は、Target 文字列をそのまま写す。
- ノートスライドの複製にも同じ primitive を使う。ノートも `image`/`hyperlink` を持ちうる。

### M2. 削除の参照チェックが「別スライドからのジャンプリンク」だけ

**根拠:** スライドを指す参照は、ADR §4.2 の `SlideIsLinkTarget { from_slide }` の外にもある。

- **`slide` 関係の他のソース**: スライドレイアウトやマスターのアクションボタン（`ppaction://hlinksldjump`。
  キオスク型テンプレートの「先頭へ」ボタン）、他のスライドのノートにあるハイパーリンク、SmartArt の
  `diagramData` パートのハイパーリンク。
- **`viewProps.xml` の `p:outlineViewPr/p:sldLst/p:sld r:id`**（`CT_OutlineViewSlideEntry`）: viewProps 自身の
  `.rels` に `slide` 関係が入る。アウトライン表示でスライドを折りたたんだデッキに出る。今回のプローブでは
  0 件だった。
- **Slide Zoom**（PowerPoint 2019 以降）: `pslz:sldZmObj` が対象スライドを `sldId`（と `cId` =
  `p14:creationId`）で名指しする。関係ではない。Fallback の `p:pic` にある `hlinksldjump` は関係を使うが、
  Choice 側は sldId だけで指す。

関係を使う参照は、core の `StillReferenced` が最後の安全網になる。ただし B1 の順序問題で、それが適用途中に
出る。拒否も汎用の `Core(…)` になり、ADR の §9 表と一致しない。sldId だけで指す参照は、どこも検出しない。

**推奨:**

- 前提チェックを「スライドに入ってくる内部関係のうち、ソースが `presentation.xml` と自分のノートスライド
  （と、同じカスケードで消えるパート）以外のものがあれば拒否」に一般化する。core の
  `incoming_relationships` がそのまま使える。
- エラーは `SlideIsReferenced { from_part, rel_type }` にする。
- `viewProps` のアウトライン項目だけは、表示状態にすぎないので、エントリと関係を一緒に外してよい（拒否でも
  可）。どちらにするかを明記する。
- `sldId` を値として持つ既知の場所（`p14:sectionLst`、`pslz:sldZmObj@sldId`）を列挙して検査する。未知の
  `extLst` と `mc:AlternateContent` の中で数値 `sldId` 属性が対象と一致した場合も、拒否側に倒す。

### M3. ADR-0001 note の文案が安全条件を落とし、後継関係が衝突している

**根拠:**

- interim note（PR #14）の条件は 3 つある: (i) パートと一緒にしか消さない、(ii) 呼び出し側が関係タイプを
  明示的に許可する（型で暗黙に参照されるパートを守る）、(iii) any-attribute 規則。
- ADR-0012 §2 の文案は、(ii) を「by relationship Id, any attribute, or a format-level id」に置き換えて
  いる。暗黙参照は Id でも属性でも見えないので、関係タイプの allowlist を落とすと安全性が下がる。§6 の
  カスケード表は実質的に allowlist だが、note の文言には現れない。
- 後継関係: interim note は「ADR-0003 が受理され、§4.3 がこの条件を含むなら置き換える」と自分で書いている。
  ADR-0009 review（Q1）は「別の core PR・別の ADR-0001 note として定義する」を推奨した。core ブランチの
  モジュール doc は note 名「registry writes」を参照している。ADR-0012 が単独で「replaces the interim one」
  と書くと、3 本が同じ note を取り合う。

**推奨:**

- note は core PR（`core/add-part`）側で 1 本だけ書く（「registry writes」）。ADR-0012 はそれを参照し、
  承認条件として文言の要件を列挙するだけにする。
- 要件には、(ii) の関係タイプ allowlist、`remove_parts` の集合単位での検証（集合外から集合内を指す関係の扱い）、
  B1 のステージ後判定、残るエントリを振り直さないこと、を必ず含める。
- format レベルの id（`sldId`）の扱いは、pptx 側の規則として ADR-0012 本文に置く。core の note には入れない。

### M4. §2 の C1–C6 が core ブランチの実 API と食い違う

**根拠**（`core-add-part` の `add.rs`、レビュー時点）:

| ADR | core ブランチ | 差 |
|---|---|---|
| C1 `add_part` | `RegistryPlan::add_part(pkg, PartNameRequest, ct, content)` | 一致。core は、残っている同型の `Override` も受け付ける。孤児 `.rels` がある名前は拒否する（ADR-0009 M6） |
| C2 `add_default_content_type` | あり（`DefaultExists`, `WouldRetypePart`） | 一致 |
| C3 `add_relationship` | あり。同じ plan 内のパートをターゲットにできる | 一致。ただし M1 |
| C4 `remove_relationship` | あり（`StillReferenced`） | 一致。ただし B1 |
| C5 `unused_part_name(template)` | `PartNameRequest::Numbered` = max+1。既存パート・`Override`・**解決できない関係ターゲット**・**セッション中に消した名前** を数える | ADR の説明より強い。ADR の「a zip entry may still be pending」という根拠は誤り（core は削除後に同名を書ける）。理由は「消したパートを指す古い参照・履歴との混同を避ける」に直す |
| C6 `remove_part_with_relationships(name, …)`（単一） | `remove_parts(names, allowed)`（集合、plan 内） | 単一パートを順に消す方式では、B1 の順序問題が増える。集合方式に合わせる |
| 各 `check_*` 双子 | `RegistryPlan` を組み立てて捨てると検証になる | §2 冒頭と §4 の説明を plan モデルに書き換える |

§Consequences の「four new core primitives (C1–C4, C6)」は、列挙が 5 つ、表は 6 つで、数も合っていない。

**推奨:** §2 を core ブランチの API 名で書き直す。そのうえで、ADR-0012 が追加で要求するもの（B1 の
`stage_part`、M1 の `copy_relationships`）を core PR への要求として明記する。

---

## Minor

- **m1 shape id の上限計算が主張より弱い.**
  - 「every numeric unprefixed `@id`」には、`p:spTgt`/`p:bldP`/`p:bldGraphic` の `@spid`、`p:control@spid`、
    `p:oleObj@spid`（`_x0000_s1026`）が入らない。
  - プローブ: `keyframes.pptx` の slide15/26 は `p:control spid="3074"`/`"2053"` を持つが、`@id` の最大は 1。
  - 別パートのモダンコメントにある `ac:spMk id` も入らない（`modern-comments.pptx`）。
  - 「freed id may still be named by an animation, connector or comment」を防ぐという §3 の理由づけに合わせ、
    数値 `@spid`、`_x0000_s(\d+)` の数値部、関連コメントパートの `ac:*Mk@id` も max に含める。
  - 上限は `u32::MAX` より `i32::MAX` が安全。POI は `int` で扱う。
- **m2 sldId のセッション内再利用.** 「`max + 1`」だけでは、同じセッションで最大 id のスライドを消してから足すと、
  同じ id が再び割り当たる。セッション内の high-water（削除した id も数える）で割り当て、セッションをまたぐ
  再利用は防げないことを明記する（ADR-0009 M5 と同じ限界）。`creationId` が変わるので実害は小さい。
- **m3 カスケードは内部ターゲットだけ.** External の `image`/`video`/`oleObject`（プローブ: `56812.pptx`
  に外部 image 1 件、外部 oleObject 7 件）と、解決できない内部 `hyperlink`（`#_ftn1`）は「関係だけ」と明記する。
  §6 の表の `hyperlink (external)` は、モードに関係なく関係だけ、に直す。
- **m4 duplicate の文言の矛盾.** §4.4 は「comments are not copied」と書くが、許可タイプに `comments` は無いので、
  コメント付きスライドは `UnsupportedSlidePart` になる。どちらにするかを決める。コメントを落として複製を許すなら、
  コメント関係を除外するフィルタ（M1）で書ける。ノートスライドの `p14:creationId` も新しい値にするかを書く。
- **m5 tags の共有.** PowerPoint の複製は tags パートを別に作る。共有すると、tags を slide 単位のキーとして使う
  アドインが取り違えうる。tags は葉パートなので複製するのが安い（`custDataLst` の `r:id` は M1 で保たれる）。
- **m6 show range と add.** `p:sldRg` があるとき、拒否されるのは delete と move だけ。add と duplicate でも
  位置がずれるので、同じく拒否するか、範囲の意味が変わることを明記する。
- **m7 プレースホルダーの規則を明文化.**
  - txBody を付けるのは python-pptx と同じ `title`/`ctrTitle`/`subTitle`/`body`/`obj`（暗黙の obj を含む）。
  - 縦書きは名前が「Vertical …」になる。
  - グループ内や `mc:AlternateContent` 内のレイアウトプレースホルダーは複製しない。
  - レイアウトは master の `p:sldLayoutIdLst` から辿る。
  - §4.1 は `sldNum` を除き、§4.6（ノート）は含める。違いは意図どおりだと書く。
  - PowerPoint は「すべてに適用」済みのデッキでは新しいスライドにもフッター類を置く。この差を Q として残す。
  - ADR-0008 の `IdxTie*` 型（idx 0 が複数あるレイアウト）からも、型で区別できることを確認する。
- **m8 ADR-0008 との API 重複.**
  - ADR-0008 には `notes_mut`/`NotesMut`（`NoNotesSlide`/`NoNotesBody`）、`Deck::layouts() -> LayoutRef`、
    `sections()` がある。ADR-0012 の `notes_text`/`set_notes_text`/`layouts() -> LayoutInfo` は名前も責務も重なる。
  - §4.1 の `LayoutRef` と §11 の `&LayoutInfo` も不一致。
  - Phase 2 はノートスライドを作る `ensure_notes_slide()` だけを持ち、テキスト編集は ADR-0008 の `NotesMut` に
    寄せる。ノート本文が無い既存ノート（ADR-0008 のコーパスで 11 件）の扱いも書く。
- **m9 app.xml.**
  - プローブで `Notes` と `HiddenSlides` もコーパス全件で一致した（87/87）。§7 の「86/86」は `Slides` だけの数字
    なので、ほかの数も実測値として載せる。
  - `show="false"` も非表示に数える。
  - `set_notes_text` でノートを作ったときは `Notes` を増やす。
  - `app.xml` が壊れていて読めないときは、拒否ではなく触らない、と明記する。
- **m10 テンプレートの決定性とライセンス.**
  - `--check` が zip のバイトを比べると、deflate 実装や依存クレートの更新で CI が落ちる。パートの内容
    （名前・順序・展開後のバイト）で比べるか、Stored にする。
  - `new()` は `core.xml` に現在時刻を入れるので、出力は非決定的になる。テスト用に時刻を注入できるようにする。
  - 「Office-like」テーマは、Microsoft や python-pptx のテーマ XML を写さずに書いたことを明記する。色とフォント名は
    事実の選択にすぎない。
  - 必須の `p:notesSz` と `notesMasterIdLst` を内容の一覧に入れる。
- **m11 セクション規則の境界.** 位置 0 の追加・移動は「先頭のセクション」ではなく、「いま位置 0 にあるスライドの
  セクション」（後続のセクション）に入れるほうが、先頭の空セクションを勝手に埋めない。Q1 を参照。
- **m12 コーパスの固定.** POI HEAD に対して `units.py` を再実行すると、97 ファイル、ノートマスター 38、
  `slide-titles` 83 になり、数字が ADR と違う。コーパスの commit（POI `47dcff76` の test-data）を README に固定する。
- **m13 §3 の part 名テンプレートに `comment` がある.** Phase 2 はコメントを作らない。PowerPoint のモダンコメントは
  `modernComment_{sldId}_{hex}.xml` という命名で、連番ではない。削除する。

---

## 推奨（Q1–Q4）

- **Q1（セクション）: 先行スライドの規則に条件付きで賛成。**
  - PowerPoint の「新しいスライド」は、選択中のスライドの後ろ、同じセクションに入る。中間位置はこの規則と一致する。
  - 位置 0 は、後続（現在の先頭スライド）のセクションに入れる（m11）。空セクションは常に空のまま残る。
  - 位置だけでは「セクション k の先頭」を表せない。将来 `move_slide_to_section` を足す余地を書いておく。
  - 手動確認は ADR-0005 Q4b と一緒に行う。
- **Q2（`TitlesOfParts` の陳腐化）: 許容でよい。**
  - python-pptx は `app.xml` を一切更新しないが、その出力は広く PowerPoint で開かれている。
  - PowerPoint は保存時に `app.xml` を作り直す。
  - `HeadingPairs` と `TitlesOfParts` を両方とも触らなければ、ベクタの件数同士は整合したまま残る。
  - 件数（`Slides`/`Notes`/`HiddenSlides`）だけ更新する案に賛成。手動確認は Q4c と統合する。
- **Q3（リンク先の削除）: 拒否でよい。ただし M2 の一般化が前提。**
  - POI のように関係を残すと宙づりの関係になり、リンクを黙って消すと作者の意図を失う。
  - リンクを外して削除する明示 API（`delete_slide_unlinking`）は後のフェーズに回す。
- **Q4（テンプレート）: 16:9 と、PowerPoint 標準の 11 レイアウトを標準の順序で。**
  - 標準の順序: Title Slide, Title and Content, Section Header, Two Content, Comparison, Title Only, Blank,
    Content with Caption, Picture with Caption, Title and Vertical Text, Vertical Title and Text。
  - python-pptx の既定テンプレートも同じ 11 枚・同じ順序なので、`slide_layouts[6]`（= Blank）のような
    index 前提のコードや利用者の期待とずれない。
  - 7 枚では index がずれ、`twoTxTwoObj`（Comparison）、`picTx`、縦書きの 2 種が欠ける。
  - 4:3 は現行 PowerPoint の既定ではない。
