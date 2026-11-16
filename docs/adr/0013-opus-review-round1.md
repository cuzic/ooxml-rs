# ADR-0013 レビュー（Opus, round 1）

対象: PR #33 `docs/adr/0013-pptx-phase1b-tables-and-smartart.md`（Proposed, rev 1）と `spikes/pptx-phase1b/`（`ac7793e`）。
プローブは `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr33/probe/`（`sa.py`, `sa2.py`, `tc.py`, `ns.py`, `runs.py`, `ph.py`, `edges.py`）に置いた。どれも spike の `tables_smartart.py` を import して同じ dedup で走らせている。入力は `pptx-p1/`, `pptx/`, `pptx-py/`, `reference/fixtures/pptx`。この入力では 553 files・tables 86・SmartArt frames 88 になり、ADR の 619 files を再現できなかった（m5）。

## 結論（先に）

方針はよい。表は「整形式でなければ拒否し、計画してから適用する」。SmartArt は「データとキャッシュの両方を書き換え、レイアウトエンジンは持たない」。どちらも妥当だが、規則にそのままでは通せない穴が 2 つある。

1. **2 次元結合の形式を取り違えている。** コーパスで PowerPoint が書いた 2 次元結合 3 つは、全部が「縁にも span を持つ」形式（edge form）である。ADR は edge form を python-pptx/LibreOffice の形式と書き、そのまま残すとしている。span の加減算は origin にしか掛けないので、2 次元結合を行・列の挿入や削除が貫くと、縁の span が古いまま残る。結果として表の中に矛盾した span が書き込まれる（B1）。
2. **コメントのアンカーが宙に浮く。** ADR-0013 は「ADR-0005 §6 が graphic frame ごとロックするので既に覆われている」と書く。しかし ADR-0005 §6 のロックが拒否するのは **text edit** だけである（`CommentAnchoredText`: "any text edit on a comment-locked shape"）。行の削除や結合は text edit ではない。このため `ac:tcMk`/`trMk`/`gridColMk` が指す行・列・セルを消すと、アンカーが宙に浮く（B2）。

ほかに Major が 4 件ある。

- M1: `a16:rowId` を新しく作れない。名前空間がローカル宣言になっているため。
- M2: Phase 0.1 の複製プリミティブは `a:tcPr`/`a:bodyPr`/`a:lstStyle` の複製と `a:p` の移動を扱えない。
- M3: SmartArt で集約されたシェイプの書式源と run の対応が決まっていない。
- M4: 隣り合うセルが共有する罫線の規則がない。

Q1 は公開された一次情報から見て「PowerPoint はデータから再レイアウトする」可能性が高い。確認は「キャッシュに偽のテキストを入れたデッキ」で行うことを勧める（m1）。

## Findings

### B1 2 次元結合で PowerPoint が使う形式は edge form。span の算術が縁のセルを更新しない

- ADR §2 は「Covered cells may additionally carry the edge spans written by python-pptx and LibreOffice … kept as found; cells created or re-flagged by Phase 1b get only the flags (POI's and, judging by the corpus, PowerPoint's form)」と書く。§2 の表では `insert_row` が「the origin's `rowSpan` grows by 1」、`delete_row` が「a covered cell's merge shrinks (`rowSpan − 1`)」で、span を変えるのは origin だけである。
- 根拠（`probe/sa.py`）: コーパスに 2 次元の origin（`gridSpan>1` かつ `rowSpan>1`）は 3 つある。3 つとも PowerPoint 製である（`sc066-001.pptx` は "Microsoft Office PowerPoint"、`tbl-cell.pptx` は "Microsoft Macintosh PowerPoint"。後者は python-pptx が実装の手本にしたテストファイル）。3 つとも edge form で、spike が数えた edge セル 7 個（3 + 4）はすべてこの 2 ファイルにある。
  ```
  tbl-cell.pptx slide2:
  0 [rowSpan="2" gridSpan="3" 'merged c'], [rowSpan="2" hMerge="1"], [rowSpan="2" hMerge="1"], …
  1 [gridSpan="3" vMerge="1"], [hMerge="1" vMerge="1"], [hMerge="1" vMerge="1"], …
  ```
  1 次元の結合だけでは、どちらの形式かを区別できない。したがって「flags-only が PowerPoint の形式」はコーパスと矛盾する。ECMA-376 §21.1.3.16 の定義（`hMerge` は「左隣と結合する」、`rowSpan` は「このセルから縦結合が始まる」）でも、上端行の covered セルが `hMerge`+`rowSpan` を持つ形は自然に読める。
- 何が壊れるか: PowerPoint が作った 2×2 結合の中に `insert_row` すると、origin は `rowSpan="3"` になる。一方、(0,1) の `hMerge` セルは `rowSpan="2"` のまま、新しい行の (2,0) には `gridSpan` が付かない。PowerPoint が二度と書かない矛盾した span が出力され、テストの well-formedness オラクルもこれを検出しない（spike の検査は origin から covered 方向の flag しか見ていない。m5）。`delete_row`/`delete_column`/`insert_column` でも同じことが起きる。
- 推奨:
  - edge form を **正規形** とする。origin の行にある covered セルは `rowSpan` = origin の値を持ち、origin の列にある covered セルは `gridSpan` = origin の値を持つ。`merge` はこの形で書く（python-pptx の `_Cell.merge` と同じ）。
  - span の算術は「origin と、その行・列にある edge セル」に一律に掛ける。新しく作る covered セルは、すぐ上（または左）の covered セルの結合属性を写す。`rowSpan`/`gridSpan` は再計算する。
  - well-formedness に「edge span は origin の span と等しいこと。flag-only は許容するが、混在は `TableNotWellFormed`」を加える。
  - T3 のオラクルで、PowerPoint 製の `tbl-cell.pptx` / `sc066-001.pptx` に対して 2 次元結合を貫く insert/delete を行い、結果が edge form のまま整合していることを確認する。手動チェックにも 2 次元結合への行挿入を入れる。

### B2 コメントのロックは text edit にしか掛からない。構造編集はアンカーを宙に浮かせる

- ADR §2 "Row and column ids" は「Deleting a row or column is refused when a modern comment anchors to it … already covered by ADR-0005 §6, which locks the whole graphic frame for such anchors (`CommentAnchoredText`)」と書く。
- 根拠: ADR-0005 §6 は「`shape_mut` computes … which locks apply」と述べ、エラー表は `CommentAnchoredText { shape_id }` = "any text edit on a comment-locked shape" と定めている。ロックの対象は text edit だけであり、`delete_row`/`delete_column`/`merge`（covered セルの段落を移す）/`set_*` は含まれない。また ADR-0013 §5 のエラー表にも、構造操作に対する `CommentAnchoredText` は載っていない。このまま実装すると、`ac:tcMkLst`/`ac:trMkLst`/`ac:gridColMkLst` が指す `a16:rowId`/`colId` の行・列を消せてしまい、アンカーが宙に浮く。結合で covered になったセルや、段落を移されたセルの `txMk@cp/len` も意味を失う。
- 推奨: §2 に明記する。ADR-0005 §6 でロックされた graphic frame には、構造操作（insert/delete/merge/unmerge）を一切許さず、`CommentAnchoredTable { shape_id }`（新設）を返す。セル属性の変更だけは許すかどうかを決めて書く。T4 に「`tcMkLst` 付きコメントがある表で `delete_row`/`merge` を呼ぶと拒否され、出力がバイト同一」を加える。
  - 将来、行 id ごとの細かい判定（その id にアンカーがなければ削除してよい）に緩めるなら、「`ac:tcMk@rowId,colId`/`trMk`/`gridColMk` をすべて解決できること、解決できなければ全体をロック」と規定する。挿入は rowId を壊さないので許せる。

### M1 新しい行・列の `a16:rowId`/`colId` は、今のコアでは作れない（全件 `NamespaceNotInScope`）

- ADR §2 は新しい行・列に `max+1` の id を付けるとする。
- 根拠（`probe/ns.py`）: `a16:rowId` を持つスライド 33 枚のうち、`xmlns:a16` をルート（`p:sld`）で宣言しているのは **0 枚** である。PowerPoint は `<a16:rowId xmlns:a16="…" val="…"/>` のように要素ごとに宣言する（`sc066-001.pptx` で確認）。一方コアの規則（ADR-0002 §3、PR #18 の注記）は「a newly created node gets the prefix already bound … if no binding is in scope the write returns `Err(NamespaceNotInScope)`」で、Phase 0.1 の `detach_as` は「a namespace declaration inside the source」と `a:extLst` の複製を拒否する。したがって隣の行の `a:extLst` を写すことも、新しく作ることもできない。id を持つ表 50/119（コーパスの PowerPoint 2016 以降の表のほぼ全部）で、`insert_row`/`insert_column` がすべて拒否される。かといって id を付けずに入れると、§2 自身の「部分的な id は `TableNotWellFormed`」に反する。
- 付随する問題:
  - `val` は `xsd:unsignedInt` で、27 表は max > 2³¹ である。`max+1` が `u32::MAX` を超える場合の規則がない。
  - 最大の行を消してから挿入すると、同じ id を再利用する。ADR-0012 §3 の append-only の趣旨に反する。
- 推奨: コアに「既知の名前空間 URI を、生成する要素上でローカル宣言して作る」プリミティブを足す（ADR-0002 の追補として。宣言を許す URI は allowlist で絞る）。または Phase 1b では「`a16` 付きの表への挿入は `RowIdNotSupported` で拒否」と正直に書く。id は「表内で未使用かつ削除履歴と衝突しない値。`max+1` が溢れたら未使用値を探す」とする。

### M2 Phase 0.1 の複製プリミティブで足りるという前提が成り立たない（`a:tcPr`/`a:bodyPr`/`a:lstStyle`、`a:p` の移動）

- ADR は §2 で「each new `a:tc` copies, through the Phase 0.1 copy primitive, the `a:tcPr` … a text body with that cell's `a:bodyPr`/`a:lstStyle` copies」と書き、§2 `merge` で「the paragraphs of the other cells move to the origin」と書く。
- 根拠: ADR-0005 rev 4 §9 は「`Detachable` is generated only for the allowlist's `[detach]` entries (`a:rPr`, `a:endParaRPr`, `a:pPr`)」と定めている。`a:tcPr`/`a:bodyPr`/`a:lstStyle` は `[detach]` の対象外である。段落の移動にはプリミティブがそもそもない。`detach_as` で代用すると、`a:hlinkClick r:id`、`a:extLst`、`smtId`、`mc:` を含む段落で `FormatNotCopyable` になる。一方、同じパート内の **移動** なら r:id も一意属性も重複しないので、本来は拒否する理由がない。
- コーパス（`probe/tc.py`）では `tcPr` に `extLst` も `r:*` も無い（0）。`[detach]` に加えれば実害は出ない。
- 推奨: §3 の allowlist 変更に `[detach]`（`a:tcPr`、`a:bodyPr`、`a:lstStyle`、各々の一意属性の指定）を明記する。`merge` 用に、同一パート内で `move_subtree`（`a:p` を丸ごと移す。id/r:id はそのまま残す。名前空間と深さは移動先で事前に検査）をコアの追補として定義する。または merge の段落移動を Phase 0 の `set_text` 的な「テキストと書式源による再構成」に落とし、失われるもの（リンク、フィールド）を拒否条件として列挙する。

### M3 SmartArt で集約されたシェイプの書式源と run の対応が決まっていない（描画を誤ったまま成功を返す）

- ADR §4 は「the node's slice of that shape's paragraphs is then replaced … new paragraphs/runs take the Phase 0.1 format sources **of the drawing shape**」と書く。
- 根拠:
  - Phase 0.1 の段落の書式源は「last paragraph's `a:pPr`」である。集約シェイプの段落レベルは `['0','1','1']`、`['0','1','1','1']`（`Bar_List.pptx` ほか。`probe/sa.py` の `agg levels`）。たとえば親ノード（スライス先頭、`lvl=0`）に 2 段落目を足すと、シェイプの最後の段落、つまり子の `lvl="1"`・`buChar` を写してしまい、PowerPoint の再レイアウトとは違う見た目になる。逆も同じである。
  - `dgm:t` を持たないノードが 12 ある（`probe/sa.py`）。データの段落は 0 なのに、描画には空段落が 1 つある（`sa2.py` の "both empty (shape paras 1 vs data 0)"）。§4 の「exactly the concatenation」をこのまま適用すると拒否になるのか通るのかが曖昧で、スライスが空のときの挿入位置と書式源も決まっていない。
  - `destOrd` が同値の `presOf` があると順序が決まらない。
  - run について:
    - データとキャッシュの run 数は 147/148 段落で一致する（`probe/runs.py`）。
    - データ側 `a:rPr` の属性（`lang`/`dirty` 系を除く）は 170/170 run でキャッシュに写っている。キャッシュの `sz` はレイアウトが計算した値である（116 run）。
    - ADR は「existing paragraphs keep … first run's `a:rPr`」としか書かない。run 単位の編集（`remove_run`、run の `set_text`、書式 setter）の対応規則がない。そのためデータ側の run 書式を変えてもキャッシュに反映されず、LibreOffice/POI では古い書式のまま成功を返す。
- 推奨:
  - 書式源はシェイプ単位ではなく **スライス単位** とする（スライスの最後の段落の `a:pPr`/`a:endParaRPr`）。スライスが空なら直前の兄弟スライスの末尾の段落を使い、それもなければ `SmartArtCacheInconsistent` とする。
  - 不変条件を「`dgm:t` の欠落は空段落 1 つと同値」と定義し直す。`destOrd` が同値なら拒否する。
  - run 単位の不変条件を足す。各段落の run 数が一致すること。データ run の属性は描画 run の属性の部分集合であること。そのうえで run 編集はインデックスで対応させ、データ側の書式属性の変更をキャッシュ側にも重ね書きする。この不変条件を spike で数えて ADR に載せる。

### M4 隣り合うセルが共有する罫線の規則がない

- ADR §3 は「borders `lnL/lnR/lnT/lnB` … an existing `a:ln*` is replaced as a whole」とし、セル単位で置き換える。
- 根拠（`probe/edges.py`）: PowerPoint 製の表では、左右に並ぶセル（左の `lnR` と右の `lnL`）が両側とも同じなのが 53、片側だけが 12、**食い違いは 0** である。上下（上の `lnB` と下の `lnT`）も同じなのが 59、片側だけが 6、食い違いは 0。セル単位の setter は PowerPoint が書かない食い違いを簡単に作る。そのときどちらの線が描かれるかは consumer によって変わる。結合範囲の外周（covered セル側の `lnR`/`lnB`）をどう扱うかも決まっていない。
- 推奨: API を「辺」単位にする（`set_border(Edge::Right, …)` は隣のセルの `lnL` も同じ値にする。表の外周なら片側だけ）。結合範囲なら外周上の全セル（covered を含む）に書く。食い違いのない出力を T3 の不変条件にする。

### m1 Q1 には公開された一次情報がある。手動チェックは「偽キャッシュ」で行う

- Microsoft Open Specifications（Q&A 912190, Mike Bowen）は「drawing#.xml is mostly meant to be a cache of the generated/evaluated layout … and that cache is overwritten on any recomputation (such as editing the SmartArt or re-saving the document)」と答えている。Aspose フォーラム 65095 には「data.xml だけを訳した → PowerPoint は訳文を表示し、drawing.xml を描く Aspose の画像出力は旧文のまま」という報告がある。PowerPoint は開いた時点でデータから再レイアウトしている可能性が高い。そうなら `drop_drawing_cache` は PowerPoint 利用者には安全で、キャッシュの編集は LibreOffice/POI/Aspose/サムネイル向けということになる。
- 推奨する手動チェック（Win/Mac デスクトップ、PowerPoint for the web、iOS）:
  - (a) データだけ編集し、キャッシュには `CACHE-ONLY` という別の文字列を入れたデッキ。どちらの文字列が表示されるかでソースが分かる。
  - (b) 両方を編集したデッキ。
  - (c) キャッシュを落としたデッキ。
  - 各デッキを開いたら、保存せずに閉じる操作と、保存して drawing を再生成させる操作の両方を行い、差分を取る。
  - Q2: 1 つのノードに長いテキストを入れ、編集操作をせずに兄弟ノードのフォントサイズが揃って縮むかを見る。
  - `docProps/thumbnail.jpeg` も古いまま残ることを Consequences に書く。

### m2 `drop_drawing_cache` の意味の詰め残し

- `remove_part`（PR #14）は、自分のリレーションシップパートを持つパートを `HasOwnRelationships` で拒否する。描画パートの 2 つは `image`/`hyperlink` の rels を持っている（`probe/sa.py`）ので、拒否される。このエラーが §5 に載っていない。
- 次の手順も明記したい。
  - `dsp:dataModelExt` の `a:ext` を外した後、`dgm:extLst` が空になったら `dgm:extLst` も消す。
  - 外すべきリレーションシップは **スライス** の rels にある。`relId` はデータパートに書かれているが、解決はスライドから行う。
  - `remove_part` には `…/2007/relationships/diagramDrawing` だけを許す。
  - Content Types の Override は `remove_part` が消す。
- 同じ描画を複数のフレームが参照する例はコーパスに 0 件だが、その場合は拒否すると書いておくとよい。

### m3 表スタイル: `@def` だけを許すのは理由と食い違う

- コーパスの大半は `<a:tblStyleLst def="{5C22544A…}"/>` が空のまま（定義 0 件で def だけがある）。ADR が却下理由に挙げた「built-in を知らない reader はスタイルなしにフォールバックする」は `@def` にもそのまま当てはまる。それなのに `@def` は許し、それ以外の built-in（`{2D5ABB26…}` No Style/No Grid、`{5940675A…}` No Style/Table Grid など）は拒否している。
- 推奨: PowerPoint の built-in GUID の閉じたリスト（LibreOffice の `predefined-table-styles.cxx` と同じ集合）を許す。またはどちらも許さない。`tableStyleId` を外す操作（`set_style(None)`）も入れる。

### m4 origin が空のときの `merge`

python-pptx の `append_ps_from` は「a single empty paragraph in target is overwritten」、つまり origin に空段落が 1 つだけあれば、それを捨てて移動してきた段落に置き換える。ADR の「appended」をそのまま実装すると、先頭に空行が残る。python-pptx に合わせて規定する。

### m5 spike の検査範囲と再現性

- 結合の整合性検査は origin から covered 方向しか見ていない（`tables_smartart.py` 233–255 行）。次の点は検査していない。
  - covered セルがちょうど 1 つの origin に属すること
  - flag の種類（origin と同じ列にあるセルが `hMerge` を持たないこと）
  - edge span の値
- このため「≤ 2 of 119」は §2 の well-formedness と同じ条件で測った数ではない。
- また、スクラッチの全コーパス（`pptx-p1`/`pptx`/`pptx-py`/fixtures）で再実行すると 553 files・tables 86 になり、619/119 を再現できない。README に入力ディレクトリと各ソースのコミットを列挙する。

### m6 アニメーションのロックの記述

表を対象にしたタイミングは 3 件あり、すべて `p:spTgt` に子が無い（全体アニメーション。`probe/tc.py`）。表の構造編集はアニメーションに影響しない。SmartArt のノード単位のアニメーションは `p:graphicEl/a:dgm@id`（データの `modelId`）を指し、テキスト編集では変わらない。§4 の「text animations … on the graphic frame」は graphic frame には `p:txEl` がないので空の規則になるか、`spTgt` 一般と読まれると過剰な拒否になる。「`a:dgm@id` は保たれるので拒否しない」と明記する。

### m7 枠サイズの細部

- `p:graphicFrame` が `p:grpSp` の中にあるときは、グループの `a:chExt`/`a:ext` を更新しないと決めて書く（描画への実害は小さい）。
- 行の高さはテキストで伸びる（38 表で `cy ≠ Σh`）。`delete_row` で `cy -= @h` とすると、`cy` が実寸より大きく残ることがある。これは PowerPoint が開くときに直すと書き添える。

### m8 プレースホルダーと `dgm:t` の欠落

- 空でないテキストを入れたときに `phldr` を外す規則はよい（34 個の `phldr` ノードの描画は、`sz` 付きの `endParaRPr` を持つ空段落で、書式源として足りる。`probe/ph.py`）。テキストを空に戻したときに `phldr="1"` に戻すかどうかを決める。
- `dgm:t` が欠けたノードに書く場合、`a:bodyPr`/`a:lstStyle` を新しく作る規則を書く。

## 推奨（Q1–Q4）

- **Q1:** 一次情報（m1）から、PowerPoint はデータから再レイアウトする見込みが高い。ただし、偽キャッシュを使った手動チェック（m1 の (a)–(c)）で確定させる。確定するまでは「データとキャッシュの両方を編集」を既定のまま維持する（LibreOffice/POI 向けに必要）。`drop_drawing_cache` は明示呼び出しとして残す。
- **Q2:** 手動チェックの対象とする。その結果に関わらず、自動フィットの再計算はスコープ外のままでよい（ADR-0005 §6 と同じ扱い）。
- **Q3:** 同意する。ただし B1 の edge form で行うこと。手動チェックのデッキには 1 次元と 2 次元の両方の結合を入れる。
- **Q4:** 塗りと罫線は Phase 1b に入れてよい。ただし罫線は M4 の「辺」単位 API にする。表スタイルは m3 のとおり built-in GUID の閉じたリストを許す。

## 判定

- B1, B2, M1–M4 があるので、**Minor の修正だけで Accepted: no**。
- 次の改訂で B1（edge form を正規形にする）、B2（構造操作のロック）、M1（`a16` 生成のプリミティブ、または拒否）、M2（`[detach]` の追加と同一パート内の移動）、M3（スライス単位の書式源と run の不変条件）、M4（辺単位の罫線）が解消すれば、round 2 で収束する見込み。

出典: ECMA-376 Part 1 §21.1.3.16 (`a:tc`)、ADR-0005 rev 4 §3/§6/§9、ADR-0002 の detach/insert 注記、core `remove_part`（`core-remove-part` worktree の `crates/ooxml-core/src/package.rs`）、python-pptx 1.0.2 `pptx/oxml/table.py`（`append_ps_from`）、[Microsoft Q&A 912190](https://learn.microsoft.com/en-us/answers/questions/912190/is-rot-attribute-in-txxfrm-autogenerated-in-a-smar)、[Aspose forum 65095](https://forum.aspose.com/t/aspose-slides-smartart-problem-in-exporing-to-any-format/65095)。
