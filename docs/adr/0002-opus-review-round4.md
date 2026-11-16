# ADR-0002 レビュー（Opus, round 4: revision 5 差分）

## 対象/方針

- 対象: `docs/adr/0002-schema-driven-code-generation.md` revision 5（branch `docs/adr-0002-schema-codegen`, commit `bd8301d`）。差分 `50e9f79..bd8301d` に絞った。あわせて `docs/research/2026-11-16-word-powerpoint-applicability.md`、`spikes/adr-0002/README.md`、`spikes/adr-0002/py/{slottable,bynamespace,corpus_check}.py` を読んだ。
- 方針: 読み取り専用の敵対的レビュー。数値はすべて `python3 -I` で再実行して確かめた。検証用スクリプトは scratchpad の `review4/` に置いた（`probe.py`: 外側の繰り返しの上限を追う計測版 `lower`、`ctx.py`/`pararpr.py`: `w:rPr` の文脈別の再検査、`rprcmp.py`、`st_nodup.py`: 重複名を除外しない ECMA 照合）。
- 使ったデータ: Open XML SDK の data、Ecma の公式 Part 4 zip（SHA-256 は ADR の値と一致）、Ecma text copyright policy v3 の PDF、POI のコーパス 197 ファイル。
- 重大度は前回までと同じ。Blocker/Major は「設計が誤っている、または実装できない」場合に限る。

## 結論（先に）

**Blocker と Major は無い。revision 5 は、下の Minor を直せば Accepted のままでよい。**

- 数値はすべて再現した。
  - `slottable.py`: 2,122 / 1,573 / 537 / 12（排他 4 + 多スロットの選択肢 8）、曖昧 1（`w:tbl`）、ECMA との食い違い 8。
  - 旧スクリプト（`50e9f79`）では 66（55 + 8 + 3）。
  - `bynamespace.py`: 研究メモの表と一致する（「その他」は DML ext/other 147 と Other 199 の合計）。
  - `corpus_check.py`: 197 ファイル、合計 171,537 個、違反 0。AC の親の表、`smartTag` 24 件、ラン数の統計も一致。
- 3つの規則はいずれも、**外側の繰り返しが unbounded である限り**正確である。
  - 実質的に繰り返す Choice: `((a*|b*))*` ≡ `(a|b)*`。
  - `(a? b? …)*` ≡ `(a|b|…)*`。
  - 理由: `lower` は内側から再帰する。外側の繰り返しの中では、多スロットの部分は「全部省略可能なら自由スロットに潰す、そうでなければ `NS`」のどちらかになる。そのため、潰す時点では各部分がすでに単一の自由スロットになっている（例: `((c,d)? a?)*` は内側の `(c,d)` が先に `NS` になる）。
  - 必須の兄弟がある `(c, (a*|b*))*` は `NS` になり、データには 0 件。
  - `optional()` の `Occurs` の読み方は SDK の符号化と合っている（`[{}]`=0..∞、`[{"Max":1}]`=0..1、`[{"Min":1}]`=1..∞、`Occurs` が無ければ 1..1）。
- 健全性の穴は、外側の上限が有限のときだけ（R4-m1）。データには該当が 0 件。
- 曖昧な QName については、既存の兄弟がどのスロットに属するかを決める規則が書かれていない。素直な実装では誤った位置に挿入する（R4-m2）。
- ライセンスの節は、引用は正確だが前提の一つが事実と合わない。ほかにも抜けがある（R4-m5）。

## Findings

### R4-m1（Minor）: 実質的な繰り返しと自由スロットの規則は、外側の上限が有限だと受理しすぎる

§4.1 は「`maxOccurs ≠ 1` の粒子の内側にある Choice は繰り返す Choice」としている。`(a? b? …)*` の規則も、繰り返しの上限を区別していない。しかし上限が有限だと、等価性が成り立たない。

- `Group{0..2}[Choice[a*, b*]]` は「a の連続」「b の連続」を高々2つまでしか許さないので、`a b a` は禁じられる。`slottable.py` はこれを `{a: None, b: None}` の1スロットにし、`max_total` も無制限なので受理してしまう。
- `Sequence{0..2}[a?, b?]` では、`b a b a` が禁じられる（2回の繰り返しでは作れない）。`slottable.py` の自由スロットは `{q: None}` を固定で入れるので、これも受理する。

データでの該当: 計測版の `lower`（`review4/probe.py`）で確かめた。

- 外側によって排他判定を免れた Choice は 254 箇所（51 モデル）、自由スロットへの潰しは 61 箇所（48 モデル）。
- 外側の上限が有限なものはどちらも **0 件**。

現在の数値は正しいが、規則の文言が SDK の更新や override で破られうる。

修正案:
- 両規則の条件を「外側の繰り返しが unbounded」に限定する。
- 有限の上限で同じ形になった場合は、Unsupported にして診断を出す。
- `slottable.py` にも同じ条件を入れる（`outer` を真偽値ではなく上限で持つ）。

### R4-m2（Minor）: 2つのスロットに現れる QName について、既存の兄弟のスロットの決め方が未定義

§4.1 は「その QName の挿入だけが曖昧で、他の子（`w:tr` を含む）は普通に挿入できる」としている。しかし §4.2 の `insert_by` は、既存の各兄弟のスロットを使う。

- 「`before` はそのスロットの既存の子に対して呼ばれる」
- 「次に、より上位のスロットの最初の子の前に入れる」

`w:tbl` の `bookmarkStart` などが既存の兄弟にあるとき、それがスロット 0 と 3 のどちらに属するかを決める規則が ADR に無い。

具体的な誤り: QName → 順位の表を後勝ち（spike の `corpus_check.py` の `rank` の内包表記と同じ）で作ったとする。`<w:tbl><w:bookmarkStart/><w:tblPr/>…` に `tblGrid` を挿入すると、先頭の `bookmarkStart` が順位 3 と判定される。その結果「より上位のスロットの最初の子」がそれになり、`tblGrid bookmarkStart tblPr` という不正な並びを出力する。先勝ちの表では、行の間にあるマーカーが順位 0 になり、`w:tr` の `before` コールバックに渡す「スロットの既存の子」から漏れる。

関連して、次の2点がある。

- `corpus_check.py` は後勝ちなので、先頭にマーカーがある正しい `w:tbl` を違反と数えてしまう。コーパスには `tblPr` より前に子を持つ `w:tbl` が **0 個**だった（`review4/ctx.py`）。つまりスロット 0 は実ファイルでは一度も検証されていない。
- `slottable.py` の ECMA 照合は、重複名をローカル名で除外している。このため `w:p` の `r`（`w:r` と `m:r`）や `w:rPr` の `shadow`（`w:` と `w14:`）のように、2スロットではないものまで比較から外れる（31 モデル）。除外をやめて再実行すると、食い違いは 9 件になる。増えた1件は `w:tbl` 自身なので、隠れた食い違いは無い。ただし除外の条件は QName 単位で、かつ実際に2スロットにあるものに限るべきである。

修正案: §4.1 か §4.2 に次の規則を書く。

- 既存の子のスロットは、文書順に左から決める。各子には、自分のスロットのうち直前の子のスロット以上で最小のものを割り当てる。受理される並びには必ず非減少の割り当てがあり、この貪欲法はそれを見つける。
- 一意に決まらない場合（そのような割り当てが無いなど）は `Err(UnresolvedSibling)` を返す。
- spike の `rank` も同じ方法に直す。

### R4-m3（Minor）: revision 5 の規則変更が、他の文と食い違ったまま残っている

- §3「Compile-time checks」は「no QName appears twice in one `CONTENT`」のままである。§4.1 の新しい文「applies within a slot only」と矛盾する。§3 を直す。今のままだと `w:tbl` がビルドを失敗させる。
- §4.1「Repetition multiplies」の最後の文「A repeating particle that lowers to more than one slot is unsupported, at any depth」が、すぐ上の自由スロットの規則と矛盾する。「必須の部分を含むもの」に限定するか、「潰した後で」と書く。「A nested Sequence/Group with maxOccurs ≤ 1 is spliced in place」も、実質的に繰り返す文脈では当てはまらない（潰すか Unsupported になる）。
- Consequences の「including every WordprocessingML and PresentationML base-namespace model」は言い過ぎである。ECMA との食い違い 8 件のうち 6 件は WML 基本名前空間のモデルで（`tcBorders`、`tcMar`、`tblBorders`、`tblCellMar`、`sdtPr`、`fieldMapData`）、override を足すまで挿入不可になる。研究メモ §1 はそう書いている。§4.1 の「WML/PML base namespaces have 0 unsupported」も、同じ文の中で「ECMA 照合の前」と明記する。
- §1.3 に挿入した2文のせいで、続く「If an override's content is taken from there」の「there」が `ooxmlsdk` の override ではなく `w:CT_OnOff/` の例を指すように読める。指示対象を名前で書く。
- Status 行は「was itself reviewed in `0002-opus-review-round4.md`」と、レビューの前に書かれている。結果（Minor の修正を反映したこと）を書き足す。

### R4-m4（Minor）: コーパス検査の主張は、検査した範囲より強い

`corpus_check.py` が確かめているのは、**スロットに載っている子の順位が単調であること**だけである。

- スロットに無い子と `mc:AlternateContent` は飛ばしている。
- `max_each`/`max_total` は見ていない。
- `w:rPr` は文脈にかかわらず `CT_RPr` に対応させている。実際には 41,714 個のうち 19,759 個が `w:pPr` の下（`CT_ParaRPr`）で、47 個が `rPrChange` の下にある。そのため、`CT_ParaRPr` の先頭のスロット `ins`/`del`/`moveFrom`/`moveTo` の位置は「スロットに無い子」として検査されていない。

主張が崩れないことは確かめた。

- 2つの型を下げたスロットでは、共通する名前の間に順序の逆転が無い。`CT_ParaRPr` だけにあるのは `ins`、`del`、`moveFrom`、`moveTo`、`w14:conflict*`、`oMath`（`review4/rprcmp.py`）。
- 文脈どおりの型で再検査しても、違反は 0 件だった（`review4/pararpr.py`）。

修正案:
- ADR の文言を「listed children appear in slot-rank order」にする。
- spike は親によって型を選ぶ（または README に限界を書く）。

### R4-m5（Minor）: ライセンスの節は、前提の一つが事実と合わず、条件の抜けもある

引用は正確である。

- 既定の表示は、表示を付けるかぎり copy/publish/distribute を許す。
- 派生物 (iv) は「standard conformant products by implementing (e.g. by copy and paste wholly or partly)」である。
- 「content of this document itself may not be modified in any way」も正しい。
- 第5版の PDF（1,553 ページ）には copyright/© /BSD の文字列が無く、2ページ目は空白である。XSD 26 本にもコメントやヘッダーが無い。
- `ooxmlsdk` の同梱物と、CR 以外がすべて一致することも確かめた。

問題は次の4点。

1. **前提が事実と合わない。** ADR は「Ecma の既定の表示は、実際にはそれ以前の標準にも適用される（applies in practice to standards predating it）」と書いて、それを根拠にしている。FAQ が述べているのは「2009 年以降の新しい標準にはすべて表示を入れてきた。それより古い標準には遡って入れていないが、同じ原則が適用される」である。2016 年版はこの「古い標準」に当たらない。正しい論理は次のとおり。
   - Purpose 節によれば、Ecma の仕様は既定の表示か代替の表示のどちらかで覆われなければならない。
   - 代替の表示は総会の承認が必要で、使うなら明示される。
   - 2009 年以降の標準には既定の表示が入るはずである（この版は現に欠いているが）。
   - したがって、適用されるのは既定の表示である。

   結論は変わらない。根拠の文を差し替える。研究メモ §6 も同じように直す。
2. **FAQ の republish 条件のうち「up to date」に触れていない。** FAQ には「published unchanged, and up to date」とある。`PROVENANCE.md` に「更新の際に最新版かどうかを確かめる」と書く。
3. **§2 と §1.2 で、`THIRD-PARTY-NOTICES` に載せる内容が食い違う。** §2 は「the Ecma provenance note」のままで、§1.2 は「Ecma's default notice in full」である。表示は「this Copyright License and Disclaimer are included on all such copies and derivative works」を求めているので、§2 も「全文」にそろえる。
4. **改行コードを無改変に保つ仕組みが無い。** 「無改変（改行コードも原本のまま）」を守るには、`.gitattributes` に `reference/ecma-376-transitional-xsd/** -text` を書く必要がある。現在リポジトリに `.gitattributes` は無い。`autocrlf=input` の環境でコミットすると LF に正規化される。`xtask codegen --check` で SHA-256 を照合すれば、これを検出できる。

細かい点: PDF の表紙は「October 2016」、Ecma の掲載と zip の名前は「December 2016」である。PROVENANCE には両方を書いておくとよい。

### R4-m6（Minor）: 基底エントリの override の継承について、件数とキーが不正確

- **件数。** `w:CT_OnOff/` から派生するエントリは **191** 件ある（`w:` 186 件と `w15:` 5 件）。186 は WML の main ファイルの中だけの数である。
- **基底エントリの重複。** `w:CT_OnOff/` 自体が2つのファイル（wordml 2006 main と 2012）に重複して存在する。抽象基底の名前が複数ファイルに現れる例は 23 件ある。override は同じ Name のすべての写しに適用する、と書く必要がある。
- **継承のキー。** `BaseClass` の文字列 `OnOffType` は `w:CT_OnOff/`、`m:CT_OnOff/`、`w14:CT_OnOff/` の3つの基底で共通である。このように曖昧な抽象クラス名は 13 種ある（`ColorType`、`EmptyType` など）。したがって、継承は `BaseClass` ではなく Name の型の部分（`w:CT_OnOff/…`）で決めると明記する。Name の型の部分で決めた場合、`BaseClass` と食い違う例は 0 件だった。
- **別の基底。** `m:CT_OnOff/`（30 件、`m:val`）と `w14:CT_OnOff/`（4 件、`w14:val`）は別の基底なので、この override の効果は及ばない。on/off の意味が必要なら、別々に override を置く。
- **優先順位。** 基底と派生の両方に override がある場合の優先順位（派生が勝つ、のが自然）も書いておく。

## 推奨

1. R4-m1: 2つの規則を unbounded の外側に限定し、有限の上限では Unsupported と診断を出す。spike にも同じ条件を入れる。
2. R4-m2: 既存の兄弟のスロットを貪欲法で左から決める規則を §4.1/§4.2 に書く。spike の `rank` と ECMA 照合の重複除外を、QName 単位に直す。
3. R4-m3: §3 の重複チェックの文言、§4.1 の「at any depth」と「spliced」、Consequences の「every WML/PML model」、§1.3 の「there」、Status 行を直す。
4. R4-m4: コーパス検査の主張を「順位の単調性」に弱める。spike では `w:rPr` の型を親によって選ぶ。
5. R4-m5: 「predating」の根拠を差し替える。「up to date」を PROVENANCE に書く。§2 の NOTICES を全文にそろえる。`.gitattributes` に `-text` を書く。
6. R4-m6: 191 件に直す。継承のキーは Name の型の部分にする。重複した基底エントリの扱いと、優先順位を明記する。

## 判定

**yes**: 上の Minor（R4-m1〜m6）を直せば、revision 5 は Accepted のままでよい。

いずれも文言を直すか条件を一つ足すことで解消でき、設計の変更は要らない。現在のデータで数値を変えるものは無い。R4-m6 の 186 → 191 だけは例示の数値の訂正である。
