# ADR-0016 レビュー（Opus, round 1）

対象: PR #37 `docs/adr/0016-pptx-phase3-formatting-and-links.md`（Proposed, revision 1, commit `b4c46c8`）、
`spikes/pptx-phase3/`。
照合先:

- ADR-0012 rev 2（`plan-pptx-phase2`、commit `4766425`）
- core ブランチ `core/add-part`（HEAD `31906a9`、`crates/ooxml-core/src/package/add.rs` と、同ブランチ ADR-0001 の
  note「registry writes」）
- ADR-0005 rev 4（Phase 0.1、`impl-pptx-phase0-1-copy`）
- vendored XSD（`pml.xsd`、`dml-main.xsd`、`shared-commonSimpleTypes.xsd`）
- Open XML SDK data（`schemas_microsoft_com_office_drawing_2013_main_command.json`）

独立プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-30r2-37/`
（`probe/p37.py`、`probe/gd.py`。`python3 -I` で zip/XML を読むだけ）。

- コーパス: POI `test-data/slideshow` と `reference/fixtures/pptx`、ADR-0008 の追加コーパス（LibreOffice
  `sd/qa`、ShapeCrawler、生成物）。SHA-256 で重複を除いて 606 ファイル。Open XML SDK の分が無いので、spike の 619
  より少ない。
- 外部ソース（`psd/`）: LibreOffice master の `oox/source/drawingml/customshapes/presetShapeDefinitions.xml`
  （ECMA の定義ファイルの写し）と `oox/source/drawingml/table/predefined-table-styles.cxx`。

## 結論（先に）

方針は正しい。主な判断は次のとおりで、どれも妥当であり、比較表の出典もおおむね正確だった。

- 直接値だけを書く。`clear_*` は直接の属性・子要素を外すだけにする（ADR-0008 の provenance と整合する）。
- 色の transform を計算せず、与えられた順に書く。
- グループの extents を暗黙に再計算しない（python-pptx の `recalculate_extents` は scaled group のメンバーを
  動かしてしまう）。
- 参照が残る間はリンクの関係を消さない。
- テーマとテーブルスタイルは追記に限る。

問いの中心だった「run split は文字を動かさないので、アニメーションやコメントのロック下でも許す」という主張は、
**参照の側から見れば正しい**。

- `p:txEl` の子は `p:charRg`/`p:pRg`（文字・段落の index）だけで、run の index を使わない。
- モダンコメントの `oac:txMk` は `cp`/`len`（文字位置）で範囲を表す。`oac:context@hash` は、範囲内の
  OfficeArt プレーンテキスト（空白・タブ・改行を含む）のハッシュで、書式を含まない（MS-ODRAWXML
  `CT_TextCharRangeContext`）。
- run の分割で変わるのは run の境界だけで、どの参照も run index を持たない。

ただし、主張の**実装手段**が成り立たない（M2）。「Phase 0.1 の copy primitive で `a:hlinkClick` をスキップせずに
写す」とあるが、Phase 0.1 の `detach_as` は、残す部分木に関係 Id（`r:*`）があれば必ず `FormatNotCopyable` に
する。このため、コーパスでリンクを持つ run（84 件）はすべて分割できない。`ahyp:hlinkClr` の `a:extLst` も、
同じく拒否される。

もう 1 つの Major は、ADR-0012 rev 2 と core ブランチで確定したトランザクションモデルを使っていないこと（M1）。
`PartPlan`/`commit`、「ADR-0012 C3/C4/C5」、Q1 の「最小の空き番号か最大 + 1 か」は、どれも既に置き換えられて
いる。

## 判定

**Accepted after Minor only: no.** M1 と M2 を反映した revision 2 で round 2 を行う。Minor は rev 2 で一緒に
直せばよい。

---

## Major

### M1. 依存先のトランザクションモデルが古い（ADR-0011 `PartPlan` → core `RegistryPlan`）

**根拠:**

- ADR-0016 は、header、§1 原則 6、§7、§8、§11 で `Package::plan()` → `PartPlan` → `commit`（ADR-0011 §1）を
  前提にしている。ノート作成（§8）でも「the tree edits of `presentation.xml` are checked with it before
  `commit`」と、レジストリの plan とツリー編集を別々に検証する書き方をしている。
- core ブランチの実際の API は `RegistryPlan` と `Package::apply` で、レジストリ以外のパートの編集は
  `stage_part` で同じ plan に積む。ADR-0012 round 1 の B1 は、まさに「ツリー編集とレジストリ編集を別々に検証
  すると、拒否されたときにバイト同一にならない」という指摘だった。ADR-0012 rev 2 と core note は、この解決策で
  そろっている。
- 次の参照は既に存在しない: §7 と §8 の「ADR-0012 C3/C4/C5」（rev 2 で C1–C6 は廃止）、「ADR-0012 §2 C5 the
  largest + 1」。
- Q1 の「ADR-0011 §1.2 は最小の空き番号」は、core note の表（Part names: **max N + 1** … 「ADR-0011's
  "smallest free n" … can resurrect such a name」）で既に決着している。

**推奨:**

- §1 原則 6、§7、§8、§9、§10 を ADR-0012 §2 の「Transaction shape」に合わせて書き直す。手順は (1) clone して
  lens で編集 → (2) 1 つの `RegistryPlan` に `stage_part`、`add_part(Numbered)`、`add_relationship`、
  `remove_relationship` を積む → (3) `apply`。
- 次の操作はすべて `stage_part` を通す: ノートマスターの追加に伴う `presentation.xml` の
  `p:notesMasterIdLst`、`tableStyles.xml` の作成と追記、テーマの編集、関係の追加・削除を伴うスライドの
  ハイパーリンク編集。
- 関係の削除は core の `RegistryPlan::remove_relationship`（staged 内容に対する any-attribute 判定）と書く。
- Q1 は削除する（core note で決着済み、max + 1）。
- ヘッダの依存から ADR-0011 §1 を外し、「core `core/add-part`（ADR-0001 note "registry writes"）」にする。

### M2. 部分書式の run split は Phase 0.1 の copy primitive では書けない

**根拠:**

- §3 は「the split-off parts copy the run's `a:rPr` through the Phase 0.1 copy primitive … the copy here does
  **not** skip `a:hlinkClick`」と書いている。
- ADR-0005 §3「Format sources」と §9 によれば、`Lens::detach_as` は残す部分木に次のものがあると必ず拒否する:
  関係 Id（`r:`、Strict `r:`、`o:relid`）、`a:extLst`、0 でない `smtId`、Markup Compatibility、名前空間宣言。
  Phase 0.1 がリンクを**スキップ**するのは、この拒否を避けるためでもある。
- プローブ（スライドとノート、14,907 個の `a:rPr`）の結果:
  - `detach_as` で拒否される `a:rPr` は 84 件で、理由はすべてリンク（`a:hlinkClick`/`a:hlinkMouseOver` の
    `r:id`）だった。
  - `a:extLst`、0 でない `smtId`、その他の `r:*` は 0 件だった。
  - spike の `ahyp:hlinkClr` 12 件は `a:hlinkClick/a:extLst` の中にあるので、二重に拒否される。
- つまり、ADR の主張どおりに実装すると、「リンクのある run は分割できる」がちょうど成り立たない。

**ロックについての主張の検証**（ここは正しい）:

- `p:bldP`/`p:txEl` の子は `p:pRg st end`（段落 index）と `p:charRg st end`（文字 index）だけで、run index は
  ない（ECMA-376 §19.5、ADR-0005 §6 の分類と同じ）。
- `oac:txMk` は `cp`/`len` で文字範囲を指す。`oac:context` は `len` とテキストの `hash` を持ち、Open XML SDK の
  `CT_TextCharRangeContext` も同じ形をしている。MS-ODRAWXML によれば、hash の対象はプレーンテキストで、書式は
  含まない。
- run を分割しても文字の並びは変わらない。`\v`（`a:br`）とフィールドの扱いも Phase 0 と同じ。よって
  `AnimatedCharacters` や `CommentAnchoredText` のロックを書式書き込みに適用しないのは妥当である。

**推奨:**

- core に「同じパート内のコピー」用の detach モードを要求する。例: `detach_as_within(skip, Keep::RelIds)`。
  コピー先がコピー元と同じパートの場合に限り、関係 Id と、`a:hlinkClick` 配下の `a:extLst` を保持してよい、
  とする。
- ADR-0002 note「detach/insert」に、このモードの条件を 1 行加える。条件は「同じパート」「Id は書き換えない」
  「`smtId` は引き続き拒否」。
- 分割で同じ `r:id` を 2 つの run が共有することは、§7 の削除規則（参照がある限り関係を残す）とすでに整合して
  いる。そのことを §3 に書く。
- `smtId`（コーパスでは 0 件）は、分割の後半の run から外すのではなく `FormatNotCopyable` で拒否する
  （ECMA-376 §21.1.2.3.9 の一意性）。
- `format_range` の `f` は、検証してから分割する。クロージャではなく、値の集合（`RunFormat`）を受け取るように
  する。そうしないと、2 つ目の run で `ValueOutOfRange` が出たときに分割だけが残る。
- 値が変わらない場合（原則 4）は分割しない。隣り合う run を結合することもしない（文字位置は変わらないが、
  locality を保つため）。

---

## Minor

- **m1 ノートマスターの「無い」の判定（python-pptx 出力）.**
  - §8 は「If the presentation has none」としか書いていない。python-pptx は `notesMaster` 関係とパートを作るが、
    `p:notesMasterIdLst` を書かない。
  - そのデッキで `p:notesMasterIdLst` の有無だけを見ると、2 つ目のノートマスターを足してしまう。スキーマ上は
    最大 1 つで、`CT_NotesMasterIdList` の `notesMasterId` は `maxOccurs="1"`（`pml.xsd` 909 行）。
  - 判定は presentation → `notesMaster` の**関係**で行う。関係があってリストが無い場合は、既存の関係を指す
    `p:notesMasterId` だけを stage する。
  - ノートスライドがあるのにノートマスターが無いデッキ（`sc027-031.pptx`、ShapeCrawler 生成、45 枚のノート）
    では、作成を拒否する。既存のノートは新しいマスターを参照しないので、作ると矛盾した状態になる。
- **m2 ノートマスターの `sldImg` とスライドの縦横比.**
  - テンプレート（ADR-0012 §8）は 16:9 である。一方、コーパスの `sldSz` の比率は 4:3 が 216 ファイル、16:9 が
    358 ファイル、その他が 31 ファイルだった。
  - 4:3 のデッキに 16:9 用の `sldImg` を足すと、スライド画像の枠が合わない。PowerPoint 自身は、マスターを作る
    ときにデッキの比率に合わせる。
  - `sldImg` の `a:xfrm` を、`p:notesSz` の枠の中で `sldSz` の比率に合わせて計算するか、ずれることを明記する。
- **m3 `app.xml` の `Notes`.** ノートを作ったら `Notes` を増やす（ADR-0012 §7）。§8 に書かれていないので足す。
- **m4 ノート作成 API の二重化.** ADR-0012 §4.6 の `ensure_notes_slide` と、§8 の `notes_mut_or_create` が
  重なっている。Phase 3 は「`ensure_notes_slide` がマスターの無いデッキでもマスターを作る」に拡張し、テキストは
  ADR-0008 の `notes_mut` で扱う。`notes_mut_or_create` は、必要なら 2 つを合成した便利関数として定義する。
- **m5 graphicFrame と connector.**
  - §6 は位置を「the shape's `a:xfrm`」に書くとしているが、`p:graphicFrame` は `p:spPr` を持たず、位置は
    `p:xfrm` にある（コーパスに 328 件）。`set_rect` の対象に `p:graphicFrame/p:xfrm` を含めるかどうかを書く。
    含める場合、fill・line・geometry は `UnsupportedContent` にする。
  - 接続されたコネクタ（`a:stCxn`/`a:endCxn`、50 件）は、接続先のシェイプを動かしても経路が再計算されない。
    `spAutoFit` と同じく、「キャッシュは更新しない」を Consequences に書く。
- **m6 group extents の「PowerPoint recomputes the box on the next interaction」は未検証.** 手動確認（Q）に
  入れる。`xfrm` の無いグループ（6 件）の中で `fit_to_children` を呼ぶ場合は、`off = chOff`、`ext = chExt` の
  恒等変換で `xfrm` を作る、と書く。
- **m7 preset geometry.**
  - 数値は確認できた。LibreOffice が持つ ECMA 定義ファイルの写しで 187 preset、`avLst` を持つものは 123 で、
    `dml-main.xsd` の `ST_ShapeType` 187 値と過不足なく一致した。
  - コーパスの `prstGeom/a:avLst` 467 個の `gd@name` は、すべて定義ファイルの名前に含まれていた（0 件の不一致）。
    名前で検証しても既存ファイルを拒否しない。
  - `hexagon`、`star5`、`pentagon` などの `hf`/`vf` は、縦横比の補正値（PowerPoint も書く）である。調整値として
    扱ってよいが、doc に書く。
  - `set_preset` を `custGeom` のシェイプに使う場合（置き換えか `UnsupportedGeometry` か）と、preset を変えた
    ときに旧 `avLst` を外すことを書く。
- **m8 定義ファイルの vendoring.**
  - 既存の XSD は ECMA-376 **Part 4** のもの（`COPYRIGHT-NOTICE.txt`、`PROVENANCE.md`）。
    `presetShapeDefinitions.xml` は **Part 1** の `OfficeOpenXML-DrawingMLGeometries.zip` にある。
  - Ecma の既定の著作権表示は、「標準に準拠する製品の実装」（derivative (iv)）を許しているので、利用自体は可能。
  - ただし、別ディレクトリ（`reference/ecma-376-drawingml-geometries/`）に、Part 1 の版、URL、SHA-256、
    著作権表示を置き、`codegen --check` で検証する。生成するのは名前と既定値の表だけで、XML は crate に埋め込まない。
  - 「ADR-0002 §1 と同じ条件」は、Part 4 の条件を指しているように読める。Part 1 の copyright page を確認した
    結果を書く。
- **m9 テーブルスタイルの `@def` と GUID 形式.**
  - `tableStyles.xml` を新しく作るとき（コーパスで 50/606 ファイルに無い）に `@def` を新しいスタイルにすると、
    PowerPoint で「表の挿入」をしたときの既定まで変わる。
  - コーパスの `@def` は `{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}`（Medium Style 2 – Accent 1）が 538 件で、
    ほぼすべてを占める。新規作成時の `@def` はこの値にする。
  - 生成する GUID は `ST_Guid` のパターン `\{[0-9A-F]{8}-…\}`（大文字、`shared-commonSimpleTypes.xsd` 59 行）に
    合わせる。UUID ライブラリの既定は小文字なので、明記する。
  - built-in の 74 id は、LibreOffice master の `predefined-table-styles.cxx` で数えても 74 だった。
  - 「append-only, ADR-0001 Layer 3」は誤り。`tableStyles.xml` は Layer 3 のレジストリではないので、この ADR
    自身の規則として書く。
- **m10 リンク編集とコメントのアンカー.**
  - モダンコメントには `oac:hlinkMkLst`（ハイパーリンクへのアンカー）がある。ADR-0005 は、これを「any other
    anchor」としてロックしている。
  - 原則 5 は「書式書き込み」をロックから外すが、§7 のリンクの削除・置換が書式に入るのかがはっきりしない。
    `hlinkMkLst` のアンカーがあるシェイプでのリンクの削除・置換は、ADR-0005 のロックに従う、と書く。
  - 新しく作ったスライドジャンプは、ADR-0012 §4.2 で対象スライドを削除できなくする（`SlideIsReferenced`）。
    レイアウトやマスター上のリンクでも同じになる。Consequences に書く。
- **m11 箇条書きの子要素の意味.**
  - `Bullet::Char { font: None, … }` が「`a:buFont` を外す（継承）」なのか「既存の値を残す」なのかを書く。
    `buClr`/`buSz*`/`buFont` の 3 グループそれぞれについて決める。
  - `a:buBlip` を `Bullet::Char` などに置き換えると、画像関係は参照されなくなる。§7 と同じ規則で外すか、残すかを
    書く（残しても無害。Phase 0.1 と同じ扱い）。
- **m12 下線・文字の線と色.** `clear_*` と `underline` の対象に、`a:uLn`/`a:uLnTx`/`a:uFill`/`a:uFillTx` を
  含めるかを書く。`uFillTx` はコーパスに 93 件あり、下線色を残すと見た目が混ざる。
- **m13 リンクのスキーム.** `UnsafeLinkScheme` に `vbscript:` と `data:` を加える。PowerPoint はクリック時に
  確認を出すが、ライブラリの出力としては作らせない側に倒す。
- **m14 `themeOverride`.** テーマの色を編集しても、`themeOverride` を持つスライドやグラフには反映されない。
  §10 に 1 文書く。

---

## Owner questions への推奨

- **Q1（パート名の番号付け）: 削除する。** core note「registry writes」の表が max N + 1 に決めており、
  ADR-0012 rev 2 もそれに従っている（M1）。
- **Q2（`p:notesMasterIdLst` は必要か）: 必ず書く。手動確認は受理の条件にしない。**
  - スキーマが定義する場所であり（`CT_Presentation` の 2 番目の子、要素は 1 つまで）、PowerPoint は常に書く。
    コーパスでは、ノートマスターのパートを持つファイルのうち、このリストが無いものは 0 件だった。
  - python-pptx の出力（リストなし）について、PowerPoint の修復が起きるという報告は見つからなかった。
    PowerPoint は関係からもマスターを辿れるらしいが、それに頼る理由はない。
  - m1 の判定（関係で見る）と組み合わせれば、python-pptx の出力を開いたときにリストを補うこともできる。
  - 手動確認は、ADR-0012 の全体確認（修復なしで開くこと）に含めれば足りる。
- **Q3（`ahyp:hlinkClr`）: 要求されたときだけ書く（提案どおり）。**
  - コーパスで 12 件あり、PowerPoint 2019 以降の UI 操作（リンクの文字色を変えない）に対応する。
  - 古いリーダーは `mc` なしの `a:ext` を無視するので、害はない。
  - 書く場合、M2 のコピーで `a:hlinkClick/a:extLst` が保持されることが前提になる。
- **Q4（定義なしの built-in GUID）: 許してよい。**
  - PowerPoint 自身がそう書き（コーパスに 8 表）、LibreOffice も 74 id を持っている。
  - POI が描画しないのは POI 側の制限である。
  - built-in 一覧（74 id）は provenance つきで固定する。それ以外の未定義 id は `TableStyleNotDefined` のまま
    にする。

## 出典

- MS-ODRAWXML `CT_TextCharRangeContext`（hash はテキスト範囲のプレーンテキスト）:
  <https://learn.microsoft.com/fr-fr/openspecs/office_standards/ms-odrawxml/0f229f17-843d-491c-9bd5-ade52285daeb>
- python-pptx notes slide analysis（ノートマスターの既定生成。`notesMasterIdLst` への言及なし）:
  <https://python-pptx.readthedocs.io/en/latest/dev/analysis/sld-notes-slide.html>
- LibreOffice `oox/source/drawingml/customshapes/presetShapeDefinitions.xml`、
  `oox/source/drawingml/table/predefined-table-styles.cxx`（GitHub mirror の master。取得日 2026-11-16）
- ADR-0005 rev 4 §3「Format sources」、§6、§9（`impl-pptx-phase0-1-copy`）
- core `add.rs`（`stage_part`、`remove_relationship`、`copy_relationships`）、ADR-0001 note「registry writes」
  （`core-add-part`）
