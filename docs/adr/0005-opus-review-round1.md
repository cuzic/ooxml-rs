# ADR-0005 レビュー（Opus, round 1）

対象: `docs/adr/0005-pptx-phase0.md`（Status: Proposed, 2026-11-16, main `b04b008`）

方針: 読み取り専用の敵対的レビュー。ADR-0001、ADR-0002（Accepted rev 5）、調査メモ、`docs/roadmap/pptx.md`、`reference/fixtures/pptx/README.md`、`spikes/pptx/` を読んだうえで、ADR の主張を次の方法で確かめた。

- 105 ファイルのコーパス（POI 94 個のうち読める 83 個、SDK 8 個、python-pptx 11 個、LibreOffice 3 個）と、コミット済みの 23 fixture に `spikes/pptx/*.py` を再実行した。足りない集計は自作スクリプトで補った（`python3 -I`、zipfile + ElementTree、スクリプトは fixture の外の scratchpad に置いた）。
- SDK データ `431ab05c`、ECMA-376 5th Transitional XSD、`spikes/adr-0002/py/slottable.py` を使った。
- 生成器での実地確認として、foundation worktree（`feat-foundation-v0`。コピーした時点では HEAD `12ab3f1` に未コミットの変更が乗った状態）を scratchpad にコピーした。§5 の allowlist をそのコピーの `codegen/allowlist/{pml,dml}.toml` に入れ、`cargo xtask codegen` と `cargo check -p ooxml-schema` を `pml` 単独、`dml` 単独、両方の3通りで実行した。worktree 本体には触れていない。レビュー中に、この branch には `53588de`（allowlist の `[write]`/`[read]` セクション化、キーワードを避けたメソッド名、ADR-0002 の post-acceptance note）と `71ef21e` が積まれた。以下の m1 と m8 はその後の状態も踏まえて書いている。

重大度は **[Blocker]**（ADR の規則どおりに実装すると、壊れたファイルを黙って出力する。Accepted にする前に直す必要がある）、**[Major]**（規則やオラクルが未定義、または誤っていて、Phase 0 の安全性の主張や範囲の記述に響く）、**[Minor]** の3段階。判断の分かれる指摘には「判断」と書いた。

## 結論（先に）

範囲の切り方は妥当。新しいパートも ID も作らない、直接の値だけを読む、AC の中は読み取り専用、アニメーションやコメントが指しているテキストは編集を拒否する、という方針は筋が通っている。数値の大半は再現できた（文末「確認できたこと」）。§5 の allowlist は、foundation の生成器で実際に生成でき、pml 単独・dml 単独・両方のどの feature 構成でもコンパイルが通った。既定値が型に合わない属性も、allowlist 全体で `serverZoom` の1件だけだった。

ただし、次の2点は、ADR の規則を書かれたとおりに実装すると壊れた出力になる。

1. コメントのアンカーの検査条件「`ac:spMk@id` の下にある `ac:txMk`」は、実際のデータでは成り立たない。`ac:spMk` は子を持たない葉要素なので、この検査は一度も発火しない。テキストに付いた modern comment の `cp`/`len` が、黙ってずれる（B1）。
2. 改行・制御文字の規則が `Run::set_text` と `Paragraph::set_text` にしか書かれていない。`add_run`/`add_paragraph`/`TextBody::set_text` に `\v` を渡すと、XML 1.0 で許されない U+000B がそのまま `a:t` に書かれる。foundation の writer はこの文字を素通しするので、出力は整形式でない XML になる（B2）。

Major は5件。「先頭ランの書式を残す」規則は、ラン 0 個の段落や `a:fld` だけの段落について定義されていない。ところが、空のプレースホルダー（「テンプレートを埋める」用途の本体）は、`p:txBody` 全体の 51 % を占める（M1）。フィールドの扱いは操作ごとに食い違っている（M2）。T4 は「どの `Err` にも fixture がある」と言うが、実際には `AmbiguousShapeId` を含む5種類に本物の fixture が無い（M3）。T3 は「同じ出力を2つの読み手が読んで一致する」ことしか見ないので、別の図形に書き込むバグを検出できない（M4）。ADR-0002 との接続では、`a:tbl` を型付きで見る経路と、`p:pic`/`p:cxnSp` の id を読む経路が allowlist から抜けている（M5）。

判定: **no**。B1、B2、M1〜M5 を直してから round 2 にかけるのがよい。いずれも文言・表・オラクルの修正で済み、アーキテクチャを変える必要は無い。

---

## B1 [Blocker] modern comment のアンカー検査は、書かれた条件では一度も発火しない

### 根拠
- ADR L300: `Error::CommentAnchoredText` を出す条件は「a modern comment of this slide anchors with `ac:txMk` (under an `ac:spMk@id` = shape id)」。
- SDK データ（`schemas_microsoft_com_office_drawing_2013_main_command.json`）では、`oac:CT_ShapeMoniker/oac:spMk` も `oac:CT_GraphicFrameMoniker/oac:graphicFrameMk` も `BaseClass: OpenXmlLeafElement` で、particle を持たない。つまり子要素を持てない。
- `p188:CT_Comment/p188:cm`（`..._powerpoint_2018_8_main.json`）のアンカーは、次の選択肢から1つを選ぶ形になっている: `pc:sldMkLst`, `oac:deMkLst`, `oac:txBodyMkLst`, **`oac:txMkLst`**, **`oac:tcMkLst`**, `oac:trMkLst`, `oac:gridColMkLst`, `p188:unknownAnchor`。どのリストも中身は `Any`。
- fixture `libreoffice/modern-comments.pptx` の実例では、`<ac:deMkLst><pc:docMk/><pc:sldMk …/><ac:spMk id="2" …/></ac:deMkLst>` のように、moniker が**兄弟として平らに並ぶ**。テキストに付くアンカーも同じく `ac:txMkLst` の中で `…<ac:spMk id=…/><ac:txMk cp=… len=…>` と並ぶ形だと考えられる（`txMk` の要素定義は SDK データに無く、fixture も無い。ADR Q5。ここは推定）。
- 表のセルは図形ではなく graphic frame の中にある。そのため moniker は `ac:spMk` ではなく `ac:graphicFrameMk` になるはずである（SDK にこの moniker と `oac:tcMkLst` がある）。ADR の条件はこのケースを見ていない。

### なぜ問題か
- 「`ac:spMk` の子孫にある `ac:txMk`」を探す実装は、何も見つけられない。そのまま編集が通り、コメントの `cp`/`len`（それに `ac:context@hash`）が指す位置が黙ってずれる。§8 L373-375 の「プレゼンテーションの中で slide のテキストを指す参照は、すべてバイト単位で保たれるか、名前付きのエラーで拒否される」という主張が、まさにこのケースで崩れる。
- `ac:txMk` を含む fixture が無い（Q5）ので、T4 もこの誤りを検出できない。

### 提案
- 条件を構造に依存しない保守的な形に書き換える。「このスライドの modern comment part に `ac:txMkLst`（または `ac:txMk` を含む任意のアンカーリスト）があり、その中のいずれかの moniker の `@id` が、編集対象の図形の id、またはその図形を含むグループ・graphic frame の id に一致する場合は、そのテキスト本体への編集を `Err`」。
- アンカーリストの中に解釈できない moniker がある場合や、comment part をパースできない場合は、そのスライドのテキスト編集を全部 `Err` にする（既知の構造だけを許可し、未知のものは拒否する）。
- テスト用の fixture は、Q5 の回答を待たずに作れる。`modern-comments.pptx` のアンカーをテストコードの中で `ac:txMkLst` に書き換えればよい（M3 と同じ方法）。

---

## B2 [Blocker] `add_run`/`add_paragraph`/`TextBody::set_text` での `\v`・`\n`・制御文字の扱いが無く、整形式でない XML を出しうる

### 根拠
- ADR L301: `InvalidXmlChar` は「XML 1.0 で許されない文字（**`\n`, `\v` は下で扱うので除く**）」。L302: `LineBreakInRun` が対象にするのは「`Run::set_text` への `\n`/`\v`、`Paragraph::set_text` への `\n`」だけ。
- `\v` を `a:br` に変える規則があるのは `Paragraph::set_text` だけ（L57-58）。`add_run(text)`（L59）、`add_paragraph(text)`、`TextBody::set_text`（L61-62。`\n` で分割することだけが書かれている）については、`\v` も、`\n` が文字列の中に残った場合も、何も書かれていない。
- U+000B は XML 1.0 の `Char` に含まれない。foundation の `crates/ooxml-core/src/xml/write.rs` の `escape_text` がエスケープするのは `& < > \r` だけで、それ以外の C0 制御文字は素通しする。`Lens::set_text`（`lens.rs`）も値を検査しない。

### なぜ問題か
- 表を書かれたとおりに読むと、`add_run("a\vb")` の `\v` は `InvalidXmlChar` から除外されているのに、`LineBreakInRun` の対象にもなっていない。結果として `\v` がそのまま書かれ、スライドのパートが整形式でなくなる。PowerPoint では修復ダイアログが出るか、ファイルを開けない。ADR-0001 の「正直に失敗する」方針に反し、Phase 0 で起こりうる最悪の壊れ方になる。
- `add_paragraph("a\nb")` は、LF を含む `a:t` を書く。PowerPoint がこれをどう表示するかは未確認。

### 提案
- 文字の規則を、表の行ではなく**すべての書き込み API に共通の前処理**として1か所に書く。
  - 全 API 共通: XML 1.0 の `Char` に含まれない文字は `InvalidXmlChar`（`\v` と、下で扱う改行は除く）。
  - `Run::set_text` と `add_run`: `\n`、`\v`、`\r` はどれも `LineBreakInRun`。
  - `Paragraph::set_text` と `add_paragraph`: `\v` は `a:br` に変換し、`\n` と `\r` は `Err`。
  - `TextBody::set_text`: `\r\n` と `\r` を `\n` に正規化してから `\n` で分割し、各行に `Paragraph::set_text` の規則を当てる（`\r` については m3 も参照）。
- core 側にも保険を置くとよい（判断）。`escape_text` に `Char` 外の文字が来たら `debug_assert` で止めるか、`Err` を返す。
- T4 に「API ごとに `\v`、`\n`、`\r`、U+0001 を渡す」パラメータ化テストを加える。

---

## M1 [Major] 「先頭ランの書式を残す」規則が、ラン 0 個の段落・フィールドだけの段落・2 行目以降の段落について定義されていない

### 根拠
- ADR L57: `Paragraph::set_text` は「first run keeps its `a:rPr`, other runs and breaks are removed」。L61-62: `TextBody::set_text` は「first paragraph keeps its `a:pPr` and first run's `a:rPr`, other paragraphs removed」。2 行目以降の段落をどう作るかは書かれていない。
- コーパス（105 ファイル、スライドのみ。自作 `probe.py` で集計）:
  - `p:txBody` 3,418 個のうち **1,737 個（51 %）**は空の段落 1 つだけ（`a:r` も `a:fld` も無い）。表のセルの `a:txBody` でも 240 個。
  - 先頭段落に `a:r` が無い `p:txBody` は 1,799 個。そのうち **99 個**は先頭段落が `a:fld` だけ（ほぼすべて `slidenum`。fixture では `layouts.pptx` slide10）。
  - ラン 0 個の段落 2,510 個のうち **620 個**は、`a:endParaRPr` に見た目に効く書式を持つ（`lang`/`dirty`/`err` などの校正用属性以外の属性 537 個、子要素 469 個）。例えば `sample_pptx_grouping_issues.pptx` には `sz="2200"` の空段落が 67 個ある。
- ADR の Alternative 2（L388-391）が python-pptx 方式を退けた理由は、「`a:rPr` を持たない新しいランを作ると、見た目の書式が失われる」ことだった。

### なぜ問題か
- 空のプレースホルダーに文字を入れる操作は、roadmap §4 が Phase 0 の価値として最初に挙げる「fill this template」の本体である。そこで「先頭ラン」が存在しないときの動作が決まっていない。素直に実装すると `a:rPr` 無しの `a:r` が作られ、`endParaRPr` の `sz=2200` などが反映されない。PowerPoint の UI で同じ空の箱に入力した結果とも見た目が変わる。ADR が Alternative 2 で退けた損失が、もっとも多いケースで起こることになる。
- `TextBody::set_text` で 2 行目以降を `add_paragraph` で作ると、`a:pPr`（`lvl`、`algn`、箇条書き）も `a:rPr`（explicit な `sz` は placeholder 内のランの 43 %）も付かない。1 行目だけ元の書式で、2 行目以降は既定の書式に戻る。
- 先頭段落が `a:fld` だけ（スライド番号や日付）のときの `TextBody::set_text` は、どう動くかが決まっていない（M2）。

### 提案
- `TextBody::set_text(lines)` は「**既存の段落を順に再利用する**」と定義する。i 行目は既存の i 番目の段落に `Paragraph::set_text` で書く。余った既存段落は削除し、足りない分だけ新しい段落を作る。こうすると、元の段落数の範囲では `a:pPr`/`a:rPr` が残り、clone のプリミティブ（Q2）も要らない。新しく作る段落の書式が失われることは、Consequences に明記する。
- ラン 0 個の段落に `Paragraph::set_text` するときは、次のどれかを ADR で選ぶ（判断）。
  (a) `a:rPr` 無しの `a:r` を作り、`endParaRPr` の書式は反映されないと明記して、§8 の Negative に件数（620/2,510）を書く。
  (b) `Err(NoRunToKeepFormat)` を返し、書式なしで書くことを呼び出し側に明示的に選ばせる（`set_text_unformatted` を別に設ける）。
  (c) Q2 の clone を Phase 0 の前提にして、`endParaRPr` を `a:rPr` として複製する。PowerPoint の入力時の動作にもっとも近い。
  レビュアーとしては (c) を推す。無理なら (a) に件数を添える。
- 先頭段落がフィールドだけ、または先頭の `a:r` の前に `a:fld` がある場合の規則は M2 と一緒に決める。

---

## M2 [Major] フィールド（`a:fld`）を拒否するかどうかが操作によって食い違う

### 根拠
- ADR L296: `FieldText`/`FieldInParagraph` を出すのは「writing a field, or `Paragraph::set_text`/`remove_run` would remove an `a:fld`」の場合。
- `remove_run(i)` は「i-th `a:r`」（L160-163）を消すので、`a:fld` を消すことはありえない。表のこの記述は空振りしている。
- 一方、`TextBody::set_text`（他の段落を消す）と `remove_paragraph` は、`a:fld` を含む段落を削除できるのに、表に載っていない。`a:fld` を含む段落はコーパスに 106 個あり、そのうち 99 個は先頭段落が `a:fld` だけ（M1）。

### なぜ問題か
- 「フィールドは Office が計算するキャッシュだから壊さない」という理由で `Paragraph::set_text` を拒否しているのに、より大きな単位の操作ではフィールドが黙って消える。利用者には、どちらが安全な操作なのか分からない。
- スライド番号のプレースホルダーに `TextBody::set_text` したとき、`a:fld` が消えるのか、残ったうえで後ろにテキストが付くのかが決まっていない。

### 提案
- 方針を1つに決める（判断）。案: 「`a:fld` を消すことになる編集（`Paragraph::set_text`、`TextBody::set_text`、`remove_paragraph`）はすべて `FieldInParagraph`。フィールドを消したい呼び出し側は `remove_field(i)` を明示的に呼ぶ（これは Phase 0 に入れても安全）」。
- `remove_run` は表から外す。

---

## M3 [Major] T4 の「すべての `Err` に fixture がある」は、今の fixture では成り立たない（`bug62513.pptx` の重複 id は図形ではない）

### 根拠
- ADR L292、L360: `AmbiguousShapeId` の fixture として `bug62513.pptx`（重複 id）を挙げている。roadmap §3.6 にも「id 0 ×3 in bug62513.pptx slide 17」とある。
- 実際の位置（自作 `ids.py`）: slide17 の id 0 の 3 個は、すべて `graphicData/mc:AlternateContent/mc:Fallback/p:oleObj/p:pic/p:nvPicPr/p:cNvPr` にある。OLE オブジェクトの代替画像で、`p:spTree` の図形ではない。§1 の走査（L48-51）は、こうした graphic frame を不透明な図形として報告するので、id 検索の対象にならない。
- コーパス 105 ファイルで、AC の外にある `p:cNvPr` の id が重複するスライドは **0**（survey の `dup_shape_ids` 4 ファイルは、すべて AC の分岐か、graphicData の内側にある）。
- 本物の fixture が無いエラーはほかにもある。
  - `AnimatedCharacters`: `p:charRg` はコーパス全体で 0。
  - `CommentAnchoredText`: `ac:txMk` を含む fixture が無い（Q5、fixture README の Known gaps）。
  - `EncryptedPackage`/`StrictConformance`: pptx の fixture が無い（README で認めている）。
  - `NamespaceNotInScope`、`UnresolvedSibling`: 該当するファイルが無い。

### なぜ問題か
- L356「each `Err` in §6 has at least one fixture-based test」は、受け入れ条件として満たせない。とくに `AmbiguousShapeId` は、挙げられた fixture を使ってもテストが「重複なし」で通ってしまい、検査が実装されていなくても気付けない。

### 提案
- T4 の fixture を2種類に分ける。(1) 本物のファイルのまま使う。(2) テストコードの中で、本物のファイルのスライド XML をメモリ上で書き換えて作る（id を重複させる、`p:charRg`/`ac:txMkLst` を差し込む、`conformance="strict"` にする、`a` 接頭辞を外す）。書き換えたファイルを配布するわけではないので、ライセンス上の問題も出ない。
- §6 の表と roadmap §3.6 の `bug62513.pptx` の記述を、「OLE の代替画像の中の id 0 ×3。図形の id としては重複していない」に直す。この fixture は、図形の id を検索する処理が graphicData の内側まで潜らないことの確認に使うとよい。

---

## M4 [Major] T3 の read-back は「同じ出力を2つの読み手が読んで一致する」だけで、編集の正しさを検証していない。python-pptx の限界の記述も事実と違う

### 根拠
- ADR L335-337: T3 は「a reader that shares no code with ooxml-rs must read the same slide texts as `ooxml-pptx` does」。
- ADR L345-347: python-pptx は「reads `a:r` only (no field text)」と書かれている。しかし python-pptx 1.0.2 の `CT_TextParagraph.content_children` は `a:r`、`a:br`、`a:fld` を返し（`pptx/oxml/text.py` L442-455）、`_Paragraph.text` はそれを連結する。`a:br` は `"\v"`（L379-385）、`a:fld` はキャッシュされた `a:t` の文字列になる（L352-357）。`a:r` しか読まないのは、`spikes/pptx/readback.py` が `p.runs` を使っているからで（`paras()`）、python-pptx の限界ではない。
- `readback.py` は placeholder の `type`/`idx` も出力しない。docstring に書かれている `"partname"` も、実際の出力には無い。
- `Paragraph::set_text`/`TextBody::set_text` の局所性について、T2（L328-334）には規定が無い。T2 が規定しているのは run 単位の `set_text`、`add_*`、`remove_*` だけ。

### なぜ問題か
- 書き込み先の図形やセルを取り違えるバグ（`ShapePath` の解決ミスや、表の行・列の取り違え）があっても、`ooxml-pptx` の読みと python-pptx の読みは同じ出力を読むので一致し、T3 は通る。T2 の局所性検査は「どこかの `a:t` が変わった」ことしか見ない。
- 改行（`a:br`）とフィールドを独立した読み手で確かめていない。B2 と M2 の領域が、どのオラクルにも守られていない。

### 提案
- T3 を「**期待値との比較**」に変える。`python-pptx(output) == apply(edit, python-pptx(input))`。`apply` はテスト側で JSON に意図した編集を当てる小さな関数にする。比較の範囲は、AC の外のすべての図形とセルにする。
- `readback.py` は段落ごとに `p.text`（`\v` とフィールドの文字列を含む）と、`[r.text for r in p.runs]` の両方を出力する。placeholder の `type`/`idx` と slide の partname も出す。
- T2 に `Paragraph::set_text`/`TextBody::set_text` の局所性を加える。差分は「先頭の `a:t` の範囲と、削除した兄弟の範囲の和」に収まること。
- L340「separate CI job」で Python の JSON を Rust のテストにどう渡すかを書く（期待値の JSON を golden としてコミットし、Python のジョブはそれを再生成して差分を見る、など。判断）。

---

## M5 [Major] ADR-0002 の仕組みとの接続に穴が2つある（`a:tbl` の型付きビュー、opaque 図形の id）

### 根拠
- ADR L254-258: 「Only `ChildOf<GraphicData> for Table` is wanted」。しかし `a:graphicData` の content は `Any` だけである。ADR-0002 §3（L374-377、L394）で `ChildOf` が生成されるのは、子の QName が親の `CONTENT` にあり、かつ両方が allowlist にある場合で、wildcard slot の扱いは定められていない。foundation の生成器で実際に生成したところ、`ChildOf<GraphicData>` は **1 件も生成されなかった**（`Table` も `Slide`/`Presentation` も）。SDK の `Children` を見ると、graphicData には `a:tbl`、`p:sld` だけでなく、ほぼすべてのルート要素（約 300 件）が並んでいて、「noisily `p:sld`/`p:presentation`」という説明より範囲がずっと広い。
- foundation の `ooxml-core` には `Lens::cast::<F>()`（QName を照合する型の付け替え）があり、これを使えば `a:tbl` を型付きで見られる。ただしこれは ADR-0002 の本文には無い。
- §3 の `ShapeRef::id()`/`name()`（L126-127）と `shape_by_id`/`AmbiguousShapeId`（L117-118、L292）は、`p:pic`、`p:cxnSp`、`p:contentPart` も含めたすべての図形の id を読む必要がある。ところが allowlist（L207-224）にあるのは `p:sp`、`p:grpSp`、`p:graphicFrame` の非表示プロパティだけで、`p:CT_Picture/p:pic`、`p:CT_PictureNonVisual/p:nvPicPr`、`p:CT_Connector/p:cxnSp`、`p:CT_ConnectorNonVisual/p:nvCxnSpPr`、`p:CT_ContentPart/p:contentPart`（`p14:nvContentPartPr`）が無い。
- §5 L201-203 は「Checked … by `allowlist_check.py`: … none has an AC child in the corpus at the write targets」と書くが、このスクリプトは pptx を一切読まない。しかも検査している一覧には ADR に無い `p:CT_SlideTiming/p:timing` が入っている（ADR L245 では allowlist 外）。AC の主張そのものは、自作の集計で確認できた（`p:txBody`/`a:txBody`/`a:p`/`a:r` の直下の AC は 0）。

### なぜ問題か
- 表のセルの読み書き（Phase 0 の範囲 6）は、`graphicData` → `a:tbl` の型付きの経路が無いと、ADR-0002 の仕組みの上では書けない。
- id の検索が、allowlist にある図形の種類だけを対象にすると、`p:sp` と `p:pic` で id が重複していても検出されない。その id を指すアニメーションや接続線を見落とし、`AmbiguousShapeId` の約束が守られない。

### 提案
- §5 に「`a:tbl` は `Lens::cast` で見る（`graphicData@uri` の検査の後）。`ChildOf<GraphicData>` は生成されない」と書き、ADR-0002 には `cast` を post-acceptance note で追記する。または、wildcard slot に対する `ChildOf` の生成規則を ADR-0002 に足す。
- 上の5つの非表示プロパティ系の要素を read で allowlist に入れる。あるいは「opaque な図形の `cNvPr` は、生成されたビューを使わず untyped に読む」と明記する。
- `allowlist_check.py` の `ALLOW` を ADR の表と一致させ、AC の検査（コーパスを走査する処理）を加えるか、AC の主張の出典を別のスクリプトにする。

---

## m1 [Minor] allowlist ファイルの置き場所と overrides の書式が、ADR-0002 とも foundation とも食い違う

- main の ADR-0002 §5 L608-610 は `codegen/allowlist.toml`（1 ファイル）。ADR-0005 L199-200 は `codegen/allowlist/pml.toml` と `dml.toml`。foundation branch は `codegen/allowlist/{sml,wml,pml,dml}.toml` で、**feature の group はファイル名で決まる**（`xtask/src/config.rs`）。レビュー中の `53588de` で、各ファイルは `[write]`/`[read]` セクション（`names = [...]`）の形になり、ADR-0002 にもこの配置を記録する post-acceptance note が入った。ただし main にはまだ入っていない。ADR-0005 は、ファイルの形（セクションと `names`）をこの note に合わせて書き、note を引用する。その note には「`ChildOf` は allowlist にある任意の2要素の間に生成される」とあるが、M5 で見たとおり wildcard slot の親（`a:graphicData`）は例外になる。この点も note に追記するとよい。
- overrides について、ADR-0005 L262 は `[[set_type]]` と書くが、foundation は `[[override]]` に `op = "set_type"` を付ける形（`codegen/overrides.toml` の冒頭、`config.rs` の `#[serde(tag = "op")]`）。ADR-0005 の例をこちらに合わせる。
- `a:CT_NonVisualDrawingProps/p:cNvPr` と `a:CT_TextBody/p:txBody` を pml.toml に置くのは、「SDK のスキーマファイルで group を決める」ことと整合しているので問題ない。その規則を一文書いておくとよい。

## m2 [Minor・判断] `DmlPercent` の `set_type` は今の生成器では通らない。Phase 0 では入れない方がよい

- foundation の `set_type` は `type` を `map_type`（`model.rs` L182-216）で SDK の型名として解釈する。`"DmlPercent"` は `UnknownSdkType` の診断になり、型は `String` に落ちる。
- `DefaultMismatch` は診断にすぎず、ビルドは止まらない（foundation の report では既に 39 件出ている）。Phase 0 は `serverZoom` を読まない。allowlist に入る属性の既定値を全部照合したところ、合わないのは `serverZoom` だけだった（自作 `defs.py`、28 件）。
- Q1 は「Phase 0 は override 0 件で `DefaultMismatch` を許容する。`DmlPercent` は Phase 3 で書式を書き込むときに、`set_type` の型の語彙（SDK の型名か core の型名か）と一緒に決める」とするのがよい。

## m3 [Minor] `\r` の意味が決まっていない

- foundation の `escape_text` は `\r` を `&#13;` にエスケープするので、`\r` は往復で保たれる。python-pptx も `&#13;` を CR として読み、生の CRLF は LF に正規化する（実測）。ただし `a:t` の中の CR を PowerPoint がどう表示するかは確認されていない。Windows 由来の `"a\r\nb"` を `TextBody::set_text` に渡すと、1 行目の末尾に CR が残る。B2 の提案どおり、正規化するか `Err` にするかを決める。

## m4 [Minor] `docProps/app.xml` は「件数が変わらない」わけではない

- ADR L311 は「`docProps/app.xml` is untouched (no counts change in Phase 0)」と書く。しかし `poi/sample.pptx` の app.xml には `Words`、`Paragraphs`、`TitlesOfParts`（スライドのタイトル文字列「Lorem ipsum dolor sit amet」×2）がある。`add_paragraph` で `Paragraphs` が、タイトルの編集で `TitlesOfParts` が古くなる。害は見た目だけ（エクスプローラーや検索インデックスの表示）なので、「untouched; Words/Paragraphs/TitlesOfParts may become stale」と書き直す。

## m5 [Minor] autofit の件数と対象

- ADR L307-308 の「`fontScale`, `lnSpcReduction`, 43」は、`fontScale` を持つものだけの数（スライドで 43）。`lnSpcReduction` は 34 で、どちらかを持つ `normAutofit` は 49 になる。
- `a:spAutoFit`（文字に合わせて図形の大きさを変える設定）もスライドに 374 ある。この場合は図形の `a:xfrm` が文字量に依存するキャッシュになり、Phase 0 の編集で古くなる。§6 の「Not refused」に加える。

## m6 [Minor] コミット済みのスクリプトで再現できない数値

- ADR L182-183 の「4,848 `a:t` … 1,070 with edge spaces」: 再集計すると `a:t` は 4,980（`a:r` 内に 4,874、`a:fld` 内に 106）、前後に空白があるものは 1,076。どちらの数も、コミット済みのスクリプトでは出力されない。結論（`xml:space` は 0 件）は変わらない。空白だけの `a:t` も 211 個あり、PowerPoint 自身が `xml:space` なしで書いていることの証拠として、こちらの方が強い。
- roadmap §3.2 と spike README の「81 percentage-union attributes」: `percent.py` を 5th edition の XSD に実行すると `total 66` になる。scratchpad に残っている spike 自身の出力 `pptx/percent.txt` も 66 だった。「SDK types 60 of them as Int32Value」も、再集計では 61。
- 数値を引用するときは、それを出力するスクリプトをコミットする（または数値を直す）。

## m7 [Minor・判断] `a:p` の中の AC と `a14:m`、AC の中の図形の報告

- `TextItem::Other(QName) /* a14:m */`（L143）はあるが、`a:p` に `mc:AlternateContent` や `a14:m` があるときに `Paragraph::set_text`/`remove_run` がどう動くかは書かれていない。ADR-0002 の「AC がある親への挿入は `Err`」は挿入にしか効かないので、削除や先頭ランへの書き込みは通ってしまう。コーパスには 0 件だが、Office の数式はこの形で書かれることが多い（推定）。「`a:p` に `pPr`/`r`/`br`/`fld`/`endParaRPr` 以外の子があれば、その段落への書き込みはすべて `Err(UnsupportedParagraphContent)`」と一行書いておけば済む。
- AC の中の図形（L51）について、どちらの分岐の図形を報告するのかを書く（`Of16-01.pptx` では id 7 が両方の分岐にある）。

## m8 [Minor] 生成器: `a:br` で `non_snake_case` の警告が出ていた（branch では修正済み）

- コピーした時点の生成器では、`Break` の子アクセサが `break__iter`、`break__mut_by`、`insert_break__by` になり（キーワード回避の `_` の後にさらに `_` が付く）、警告が 3 件出た。CLAUDE.md の `cargo clippy … -D warnings` では CI が落ちる。レビュー中の `53588de` が「Keyword-safe generated method names (break_iter, type_)」で直している。ADR-0005 の allowlist で生成し直して、警告が 0 件になることを実装ラウンドで確かめればよい。

## m9 [Minor] fixture のライセンス: 扱いは妥当。配布の境界だけ明記する

- 確認したこと:
  - 23 ファイルの SHA-256 は README と全件一致した。
  - Apache-2.0（POI）は LICENSE と NOTICE を同梱している（NOTICE は upstream と同一バイト）。MIT（python-pptx、Open XML SDK）は著作権表示と許諾文がある。
  - MPL-2.0（LibreOffice）は、改変していないファイルを MPL の下で配布し、README でそれを受け取り手に知らせているので、§3.1（Source Code Form の配布）と §3.3（Larger Work）の条件を満たす。MPL のファイルが `MIT OR Apache-2.0` のリポジトリの中にあること自体は問題ない。
- 足りないこと:
  1. トップの README（または LICENSE の注記）に「`reference/fixtures/**` は第三者のファイルで、それぞれのライセンスに従う。`MIT OR Apache-2.0` は適用されない」と書かれていない。
  2. M4 の提案や L339 のとおり oracle を `crates/ooxml-pptx/tests/` に移すときに、fixture を crate の中にコピーすると、`MIT OR Apache-2.0` と宣言した `.crate` に MPL や Apache のファイルが入り、NOTICE が付かない状態になる。「fixture は `reference/` から相対パスで読み、公開する crate には含めない（`include`/`exclude` で担保する）」と ADR に書く。
  3. xlsx の fixture（`poi/LICENSE`、`poi/NOTICE`、`SHA256SUMS` ファイル）と pptx の fixture（`LICENSES/` ディレクトリ、README の中のコードブロック）で配置が違う。`sha256sum -c` で直接使えるように揃えるとよい（細かい点）。
- POI と LibreOffice のテストファイルについて、元の作者を個別に追跡していないことは README が明記している。xlsx の fixture と同じ扱いなので、これで十分と判断する。

## m10 [Minor] API の細部

- `PlaceholderRef::idx() -> u32`（L136）は、ADR-0002 の「型付きの読み取りは `Result<_, AttrError>` を返す」に合わない。`id()`（L126）は `Result` を返している。
- アニメーションやコメントの検査で、編集対象の図形の `cNvPr@id` が読めない場合（欠落、数値でない）は `Err` にする、と一文足す（コーパスには 0 件）。
- `Paragraph::set_text` で先頭ランの `a:rPr` を残すと、そのランの `a:hlinkClick` も残る。先頭ランがリンクになっている段落は 28 個あり、その場合は新しい文字列全体がリンクになる。2 番目以降のランにリンクがある複数ランの段落も 18 個あり、そこではリンクが消える（削除したランの rels は孤立するが無害）。§8 に書く（判断）。
- トップの README は「Word と PowerPoint は、`ooxml-core` の設計を Excel で確かめるまで範囲外」と書いている。ADR-0005 の Context で、着手の前提（Excel Phase 0 の完了など）に触れておく（判断）。

---

## 確認できたこと（ADR の主張のうち再現したもの）

- `survey.py`: slides 546、`p:sp`/`p:txBody` 3,450/3,418、`p:ph` 713、`a:fld` 106、`p:sld` 直下の AC 12（7 ファイル）、`p:transition` は直下に 205・AC の中に 12、`p14:section` 17。roadmap の表と一致した。
- `runs.py`: 597/3,611（16.5 %）、visible diff 345（58 %）、proofing only 252、`has_br` 52、run 境界をまたぐ 2 語・3 語の窓は 22 %/30 %。`placeholders.py`: 711、687/49/2/22、xfrm 239（34 %）、`sz` 801/1,883。いずれも一致した。
- `p:bldP build="p"` 12、`p:pRg` 97、`p:charRg` 0、AC の中の `p:timing` 0。
- `xml:space` を持つ `a:t` は 0。`p:txBody`/`a:txBody`/`a:p`/`a:r` の直下の AC は 0。`a:p` の直下の `a14:m` は 0。`a:r` に `rPr`/`t` 以外の子は 0。
- `table_test2.pptx` は結合で隠れたセル 16・結合範囲 13、`shapes.pptx` は 3/3。結合で隠れたセルに文字があるものは、コーパス全体で 0。
- `readback.py` は python-pptx 1.0.2 で 23 fixture をすべて開けた。python-pptx のパーサーは `remove_blank_text=True` だが、空白だけの `<a:t> </a:t>` も保持する（実測）。
- §5 の allowlist: 名前はすべて SDK にあり、unsupported なモデルは無い。slot の lowering は ADR の記述どおり（`a:p` = `[pPr] [a14:m,br,fld,r] [endParaRPr]`、`a:r` = `[rPr] [t]`）。cross-feature の `ChildOf` は `p:txBody → a:p` と `p:graphicFrame → a:graphic` の 2 件で、どちらも `#[cfg(all(feature = "pml", feature = "dml"))]` で生成された。`pml` 単独、`dml` 単独、両方のどれでも `cargo check` が通った。

## 推奨する修正の優先順位

1. **B2**（すべての書き込み API に共通する文字の規則と、core の保険）: 1 か所に書けば済み、破損がもっとも重い。
2. **B1**（コメントのアンカー検査を moniker リストの構造に合わせて書き直し、未知のものは拒否する）。それに合わせて、メモリ上で作る fixture を用意する。
3. **M1**（空段落、フィールドだけの段落、複数行の規則。既存段落を再利用する定義）と **M2**（フィールドの方針を1つに決める）: 主な用途の動作がここで決まる。
4. **M4**（T3 を期待値との比較にし、`p.text` を使う）と **M3**（T4 の fixture を本物と、メモリ上で書き換えたものに分ける。`bug62513` の記述を直す）。
5. **M5**（`Lens::cast` を ADR-0002 に明記する。opaque な図形の id の読み方を決める。`allowlist_check.py` を ADR の表と揃える）と **m1**（allowlist と overrides の書式。ADR-0002 の post-acceptance note）。
6. m2〜m10。

## 判定

**Ready for Accepted after Minor only: no.** Blocker 2 件（B1, B2）と Major 5 件（M1〜M5）を直してから round 2 にかける。
