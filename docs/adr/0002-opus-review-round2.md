# ADR-0002 レビュー（Opus, round 2）

対象: `docs/adr/0002-schema-driven-code-generation.md` revision 2（branch `docs/adr-0002-schema-codegen`）
方針: 読み取り専用の敵対的レビュー。round 1（`0002-opus-review-round1.md`）の B1-B3 / M1-M5 / m1-m5 が実際に解消されたかを判定し、revision 2 の修正が新たに持ち込んだ問題を探した。根拠は次の4つ。
- ADR-0001（Accepted）との突き合わせ。
- Open XML SDK データ（`dotnet/Open-XML-SDK@431ab05c`）と、`ooxmlsdk` 同梱の ECMA-376 Transitional XSD を `python3 -I` で集計した結果。
- spike（`spikes/adr-0002/`）を rustc 1.99.1 で実行した結果。
- scratchpad で書いた検証用の Rust/Python。

重大度は round 1 と同じ3段階で、**[Blocker]**、**[Major]**（設計の規則が誤っているか欠けていて、誤った出力やビルド不能につながる）、**[Minor]**。判断の分かれる指摘には「判断」と明記した。

## 結論（先に）

round 1 の Blocker 3件と Major 5件は、いずれも実質的に解消されている。
- XSD を既定値だけの第2入力にした。
- view を newtype にした（E0116 を回避）。
- 挿入をスロット + 呼び出し側の順序にし、`get_or_insert` / `remove` / dirty の義務を加えた。
- AC がある親への挿入を安全側の `Err` にした。
- 生成単位を (type, element) にした。

spike の `cargo test`（5件）と README の負例3件（E0277 + `on_unimplemented` の文言、E0116、E0080）は、すべてそのまま再現した。ADR の数値もほぼすべて再現した（1,449 / 2,122 / 1,368 / 699 / 55 / 187 of 344 / 8,510 / 1,236 / 775 / 128 / 112k / 粒子種別の件数）。

ただし、revision 2 で新しく書いた一般規則のうち2つに、データで確かめられる穴がある。
1. **スロットへの落とし込み（§4.1）が健全でない（R2-M1）。** 繰り返さない Choice の選択肢が繰り返し要素である場合、「1つのスロット、順不同」に落とすと排他性が失われる。Choice スロットの個数上限も定義されていない。さらに SDK データ自体が、ECMA の sequence を Choice に置き換えている箇所がある（`x:CT_Filters`）。どちらも、挿入が受理されたうえで Excel の修復ダイアログが出る出力になる。
2. **既定値の結合規則（§1.2）は、SML 以外では成り立たない（R2-M2）。** 結合できた既定値のうち 39 件は、SDK の型では妥当な値にならない（DrawingML などのパーセント `"100%"` が `Int32Value` に対応している、など）。§3 の「既定値が型に合うかをコンパイル時に検査する」をそのまま実装すると、`dml`/`pml`/`chart` のその要素を allowlist に入れた時点でビルドが通らなくなる。加えて、spike の結合方法は ADR 自身の QName 規則と食い違っていて、62 件を取りこぼしている。

どちらも Phase0（SML の 15 エントリ）には影響しない。また、どちらも規則の文言を直せば済み、アーキテクチャを変える必要はない。Minor は8件。そのうち **R2-m1（新しい `<t>` に `xml:space` を付けられない恐れ）** と **R2-m4（merged range の検出に必要な `mergeCells` が allowlist に無い）** は、Phase0 の実装で最初に当たる箇所なので先に直す価値がある。

**判定: Minor だけを直して Accepted にできるか → No。** R2-M1 と R2-M2 の規則を直す必要がある。どちらも数段落の修正で済む規模なので、round 3 で収束する見込みが高い。

---

## round-1 指摘の解消状況

| ID | 内容 | 判定 | 根拠 / 残り |
| --- | --- | --- | --- |
| B1 | SDK データに既定値が無い | **解消** | §1.2（L72-90）で XSD を第2入力にした。`xsd_defaults.py` を実行すると `matched to a default 1449` が出て、CT_Cell の5件（`s=0, t=n, cm=0, vm=0, ph=false`）も含まれる。ただし結合規則を一般化すると問題が出る → R2-M2 |
| B2 | 他クレートの型への inherent impl | **解消** | §3 L215-227。`ooxml-schema` に local newtype を置く形にした。負例で E0116 を再現した。newtype から子の view を貸し出せることを scratchpad で確認した（後述 R2-m2） |
| B3 | 挿入がスロット内の順序・重複・削除を表せない | **解消（細部は残る）** | §4.2 の表と dirty の義務、空白の規則。spike の `insert_within_band_by_column` と `max_occurs_one` が通る。dirty の伝播の仕組み、および `cmp` の形 → R2-m2, R2-m3 |
| M1 | AC の兄弟に対する反例 | **解消** | §4.3 で Phase0 は `Err(UnresolvedSibling)` にし、その後の規則と `mce_wrapped` も記録した。spike の `ac_sibling_is_refused` が通る |
| M2 | 生成単位と継承 | **解消（データ上の例外あり）** | L149-164。同じファイル内で解決できない `BaseClass` が wml に 10 件ある → R2-m8 |
| M3 | `ooxmlsdk` の評価の誤り | **解消** | L390-408 は事実と合う。「5,477 files で round-trip」は少し強い → R2-m7 |
| M4 | sheet1 を特定できない、`r` の読み取りが無い | **解消（別の漏れあり）** | L357-369、§6。ただし `mergeCells` が無い。また ADR-0001 の「Layer 2 は sheet1.xml だけ」との整合が書かれていない → R2-m4 |
| M5 | overrides の形式 | **概ね解消** | L91-106。ただし particle の形を直す操作が無く、R2-M1 の `CT_Filters` を overrides で直せない |
| m1 | const 評価 | **解消** | L241-252。モジュールレベルの `const _` にし、負例で E0080 を再現した。`Content::Unsupported` の親に対する検査の書き方が未定 → R2-m8 |
| m2 | QName の同一性 | **解消（新しい node の prefix に穴）** | L166-172 → R2-m1 |
| m3 | ライセンス表示 | **概ね解消** | L133-137。`ooxmlsdk` 由来の overrides が notices の一覧から漏れている → R2-m7 |
| m4 | `--check` の決定性 | **解消** | L128-132（`BTreeMap`、`prettyplease` を pin） |
| m5 | 数値の細部 | **解消** | 35 名前空間、187/344、版ごとの Occurs は最も緩いものを採る、を確認した |

---

## R2-M1 [Major] スロットへの落とし込み規則が健全でない（Choice の排他性と個数上限、SDK の Choice が ECMA の sequence と違う）

### 根拠
- §4.1 L271-273: 「a `Choice` (any occurrence) is one slot containing all its alternatives; children of that slot may appear in any order relative to each other」。L279-283 で Unsupported になるのは、多スロットの繰り返し、多スロットの選択肢、QName の重複の3つだけ。
- §4.2 L300: 「If the slot's `maxOccurs` is reached → `Err(AlreadyPresent)`」。スロット単位の上限しか無い。
- Consequences L447-448: 「slot lowering covers 2,067 of 2,122 content models for insertion」。
- データ（`slottable.py` が slottable と判定したモデルに限定して、祖先が繰り返さない Choice を数えた）:
  - **排他性が失われるもの: 4 モデル。** `x:CT_Filters/x:filters`、`x:CT_SortState/x:sortState`、`p188:CT_Comment/p188:cm`、`mso14:CT_BackstageGroups/mso14:firstColumn`。たとえば `p188:cm` は `Group > Choice[sldMkLst, …, oac:deMkLst*, oac:txBodyMkLst*, …]` で、意味は「どれか1種類の列」。1スロットにすると `deMkLst` と `txBodyMkLst` を混ぜて挿入できてしまう。
  - **SDK が ECMA と違う構造を持つもの:** `x:CT_Filters/x:filters` は、SDK では `Choice[x14:filter*, x:filter*, x:dateGroupItem*]`。ECMA の `sml.xsd` L38-43 では `sequence(filter*, dateGroupItem*)`。規則どおり1スロットにすると `dateGroupItem` を `filter` の前に挿入でき、ECMA の順序に反する。x14 の選択肢を合成した結果、SDK 側で sequence が choice に変わっている。
  - **個数上限が定義されていないもの:** `x:CT_SortState` は `Choice[x14:sortCondition{0..64}, x:sortCondition{0..64}]`。スロットの上限が 64 なのか 128 なのか、QName ごとなのかが ADR に無い。`All` も同様で、§4.1 L277 は「one slot」としているが、6 件ある `All` はすべて「各要素 0..1」。スロット単位の上限では、同じ要素を2度入れることを防ぐか、異なる要素を2つ入れることを防ぐか、のどちらか一方しかできない。

### なぜ問題か
- ADR の目的は「スキーマ順序違反による修復ダイアログ」を出さないことだ（Context L39-41）。上の例では挿入が `Ok` を返したうえで不正な出力になる。誤りが検出されずに表に出ないので、`Err` を返すより悪い。
- 件数は少ない（4 + α）。しかし「2,067 / 2,122 を挿入可能」という数字は、この規則が健全であることを前提にしている。
- `CT_Filters` は SDK データの側の問題なので、規則を直しても救えない。§1.3 の override 操作には particle の形を直すもの（例: `set_particle`）が無く、overrides でも直せない。

### 提案
- §4.1 に次の2点を足す。
  - 繰り返さない Choice で、2つ以上の選択肢のどれかが `maxOccurs > 1` の場合は Unsupported にする（あるいは、スロットに「1種類だけ」という制約を持たせる）。
  - スロットは QName ごとの上限とスロット全体の上限の両方を持ち、`insert_by` は両方を検査する。`All` は「QName ごとの上限 1」とする。
- SDK の Choice が ECMA の sequence と食い違う箇所の扱いを決める。候補は次の2つ。
  - (a) 生成器の検査として、標準名前空間の型について SDK の particle と XSD の particle の「要素の順序関係」を比べ、食い違いを診断として出す。`ooxmlsdk-build` の `oracle.rs` が同じ発想。
  - (b) overrides に `set_content { on, slots }` を足す。
  
  どちらも Phase0 の範囲外なので、「この場合は Unsupported にする」と書くだけでもよい。
- Consequences の 2,067 を修正後の数字に更新する。

---

## R2-M2 [Major] 既定値の「単純な参照」による結合は SML 以外で破綻する（字句の不一致、qualified な属性、グローバル属性への ref）

### 根拠
- §1.2 L77-81: 「Defaults are extracted by a plain lookup — (target namespace, `complexType/@name`, attribute name), following `attributeGroup` references」「*(spike: … joins 1,449 SDK attribute entries …)*」。
- §3 L245-246: 「every default is valid for its type」をモジュールレベルの const で検査し、生成器も同じ検査を先に走らせる。

検証（`review2/join3.py` と `typecheck.py`。名前空間を正しく扱う結合と、SDK の `Type` による妥当性の検査）:
1. **字句の不一致: 結合できた既定値のうち 39 件が、SDK の型では妥当な値にならない。** 例:
   - `a:CT_TransformEffect/a:xfrm @sx`: XSD は `default="100%"`（`dml-main.xsd` L1405）、SDK は `Int32Value`。
   - `c:CT_HoleSize @val`: XSD は `"10%"`、SDK は `ByteValue`。
   - `p:CT_Presentation @serverZoom`: XSD は `"50%"`、SDK は `Int32Value`。
   - `c:CT_SplitType @val`: XSD は `default="auto"`（`dml-chart.xsd` L386）。SDK の `SplitValues` には `cust, percent, pos, val` しか無く、`auto` が無い。
   
   原因は、Transitional の `ST_Percentage` が `union(ST_PercentageDecimal, s:ST_Percentage)`（L236-238）なのに、SDK はそれを整数（1/1000 %）の型として扱っていることにある。`ooxmlsdk` の round-trip ハーネスに `normalize_drawingml_percentage_attr_lexeme` があるのも、同じ不一致の表れ。SML の結合はすべて妥当だった。
2. **qualified な属性を取りこぼしている。** `wml.xsd` は `attributeFormDefault="qualified"` で、`default=` が 25 件ある。spike は SDK の QName `w:color` を文字列のまま XSD の `color` と比べているので、wml の既定値を1件も結合していない（例: `w:CT_Border/` の `color=auto`, `space=0`、`w:CT_Spacing` の8件）。
3. **グローバル属性への `ref` を取りこぼしている。** 既定値がグローバル属性の宣言の側にあるもの（`o:insetmode="custom"`, `o:connectortype`, `o:hralign`, `r:dm/lo/qs/cs/blip=""`）は、`ref=` を辿らないと取れない。37 件。
4. 2 と 3 を直すと、結合できる件数は 1,449 → **1,511** になる。また spike は prefix の文字列（`"r:id"`, `"xml:space"`）で照合している。これは §3 L166-167 の「prefixes are … never compared」に反する方法で、1,449 という数字はその方法で数えた値。

### なぜ問題か
- §3 の const 検査は、上の 39 件のどれかを含む要素を allowlist に入れた時点で失敗する（E0080、または生成器のエラー）。失敗が大きく表に出る点は安全側だが、「既定値を XSD から機械的に取る」という §1.2 の決定が、DrawingML/Chart/PresentationML では機械的に回らない。その場合の方針（既定値を捨てる、SDK の字句に変換する、`set_default` の override で書く、型を union にする）が ADR に無い。
- 同じ不一致は既定値の外にも及ぶ。Transitional の文書は `sx="50%"` と書いてよいが、SDK の型どおりに `Int32` として読むと、妥当な文書が `AttrError` になる。enum については §3 L229-235 が「never lossy」としているが、数値の型には同じ配慮が無い。
- spike の数字は、ADR の結合規則（名前空間、`attributeGroup`）の説明と方法が違う。「spike で確認した」の範囲を正確に書く必要がある。

### 提案
- §1.2 の結合規則に次の2点を足す。
  - 属性名は (namespace, local) で比べる。namespace は `attributeFormDefault`/`form` で決める。
  - `ref=` はグローバル属性の宣言まで辿る。
  
  いずれも単純な参照の範囲に収まる。
- 「XSD の既定値が、SDK の型の字句空間に入らない場合」の方針を1つ決めて書く。推奨（判断）: 生成器はその既定値を採用せず、`DefaultMismatch` の診断を出す。override（`set_default`、出典つき）がある場合だけ採用する。さらに simple type は XSD の union を正とし、`Percentage` のような union 型を core に持たせる。Phase0 には影響しない、と明記する。
- spike の `xsd_defaults.py` を上の規則で直し、数字を更新する（1,511 前後）。

---

## R2-m1 [Minor] 新しいノードの prefix 規則では `xml:space` を付けられない恐れがあり、属性と既定の名前空間の扱いも書かれていない

### 根拠
§3 L169-172: 「A newly created node gets the prefix already bound to its URI at the insertion point; if no binding is in scope the write returns `Err(NamespaceNotInScope)`」。ADR-0001 L238-242 は、先頭や末尾に空白がある値を書くときに `<t>` に `xml:space="preserve"` を付けることを必須にしている。

### なぜ問題か
- `xml` という prefix は宣言されないまま常に束縛されている（Namespaces in XML 1.0 §3）。スコープ内の `xmlns:*` 宣言だけを探す素直な実装では、`set_string("  x")` が `NamespaceNotInScope` を返す。Phase0 の必須要件で最初に当たる。
- 既定の名前空間は属性には適用されない。名前空間つきの属性の URI が `xmlns="…"` でしか束縛されていない場合、規則どおりに空の prefix を選ぶと、名前空間の無い別の属性になってしまう（WML のファイルが `w` を既定の名前空間にしている場合など）。逆に、名前空間の無い属性（SML の `r`, `s`, `t`）は、要素の名前空間とは関係なく常に prefix 無しで書く。
- 同じ URI に既定の名前空間と `x:` の両方が束縛されているファイルでは、どちらを使うかで出力が変わる。

### 提案
L169-172 に次の3点を書く。
- `xml` は常に束縛されている。
- 名前空間つきの属性には、空でない prefix しか使わない。名前空間の無い属性には prefix を付けない。
- 要素の prefix の候補が複数あるときは、親要素の prefix を優先する（決定的な規則を1つ決める）。

---

## R2-m2 [Minor] 祖先への dirty の伝播の仕組みが ADR-0001 の `RawNode` に無く、spike も確認していない。また dirty を立てる条件が不正確

### 根拠
- §3 L191-192: `Lens { &'a tree, node id }`, `LensMut { &'a mut tree, node id }`。§4.2 L311-315: primitive が「`subtree_dirty` on it and every ancestor up to the part root」を立てる。
- ADR-0001 L160-171 の `RawNode` は `children: Vec<RawXmlItem>` を持つだけの入れ子構造で、node id も親へのリンクも無い。
- spike `core/src/lib.rs` L108-109: 「Dirty propagation to ancestors is the caller's (path) job … here we just mark self」。`LensMut` は `&'a mut RawNode` を直接持っているので、祖先には届かない。L133: `own_dirty |= children.is_empty()`。

### なぜ問題か
- ADR-0001 の規則では、`!subtree_dirty` の祖先は自分の span をそのまま出力し、子へは再帰しない。祖先への伝播が1つ欠けるだけで、その下の編集が**黙って失われる**。ADR は義務を書いているが、それを実現できる表現（node id とは何か）を決めていない。
- 実現は可能だ。scratchpad（`review2/lens`）で、ルートからの添字の path を持つ `LensMut { tree: &mut Tree, path: Vec<usize> }` を書いて確かめた。`CellViewMut::value_mut(&mut self) -> VViewMut<'_>` で子の view を再借用でき、`get_or_insert` で祖先にまとめて `subtree_dirty` を立てられる。テストも通った。ただし、子の view は同時に1つしか持てない（逐次的な再借用になる）。
- `children.is_empty()` は条件として正しくない。`<row r="1"></row>`（空だが閉じタグがある）に子を足すと `own_dirty` になり、開始タグが属性から再描画される。そのため引用符の種類などが変わり、T2 が許す「挿入点」の外に差分が出る。正しい条件は ADR-0001 L210-214 のとおり「元が自己終了タグ（`span.close == None`）」。また、閉じタグのある要素が最後の子を失っても、元の開始タグと閉じタグをそのまま出力すれば well-formed なので、`own_dirty` にする必要は無い。L313-314 の「or loses its last one」は、自己終了タグに戻すことを強制し、不要な差分を生む。

### 提案
- §3 に「node id = パートのルートからの子の添字の列（ADR-0001 の入れ子の `RawNode` のまま）。primitive は path を辿って祖先に印を付ける」と書く。arena と親リンクを採るなら、ADR-0001 の `RawNode` の定義を修正すると明記する。
- dirty を立てる条件を「元の span が自己終了タグの要素が最初の子を得たとき」に限定する。

## R2-m3 [Minor] `insert_by` の `cmp(existing)` では `r` の無い行やセルを並べられない（判断）

L300 の `cmp` は既存の兄弟を1つだけ受け取る。CT_Row の `r` と CT_Cell の `r` はスキーマ上 optional で、`r` を省いて位置で表すファイルも読み手側（calamine など）は受け付ける。このとき順序は「同じスロットの何番目か」で決まり、1つのノードだけを見る比較関数では表せない。提案: `cmp` を「同じスロットの兄弟を文書順に渡す `FnMut(index_in_slot, &node)`」にするか、`insert_at(index_in_slot)` を足す。Phase0 で `r` の無いファイルを `Err` にするなら、その旨を書く。

## R2-m4 [Minor] Phase0 の allowlist に `mergeCells` が無い。また ADR-0001 の「Layer 2 は sheet1.xml だけ」との関係が書かれていない

- ADR-0001 L306 は「writing into a cell inside a merged range」を `Err` にすると決めている。これには `x:CT_MergeCells/x:mergeCells` と `x:CT_MergeCell/x:mergeCell` の読み取りが要るが、L360-369 に無い（round 1 M4 と同じ種類の漏れ）。
- 読み取り専用で加えた `workbook`/`sheets`/`sst`/`si` を lens で読むには、`workbook.xml` と `sharedStrings.xml` を `RawNode` ツリーに parse する必要がある。ADR-0001 L486 は「`RawNode` tree … for `sheet1.xml` only」、L443 は「Layer 2 only covering `sheet1.xml` in Phase0」と書いている。Layer 1 の「dirty の無い `Parsed` はそのまま出力」で安全ではあるが、範囲が広がったことを ADR-0002 で明記する（「編集するのは sheet1 だけ。読み取りのための parse は ADR-0001 の M2 規則に従う」）。
- §6 の「first `<sheet>` in `<sheets>`」と、ADR-0001 の「`sheet1.xml`」は、シートを並べ替えたブックでは別のパートを指す。どちらを指すかを一方に揃える（判断）。`xl/_rels/workbook.xml.rels` は固定のパスではなく、workbook パートの名前から導く、とも書くとよい。

## R2-m5 [Minor] Any のワイルドカードの「namespace set」はデータに無い

L275-276 は「matches unlisted names in that namespace set」とする。しかし SDK の `Any` 142 件のうち 136 件は `Namespace` を持たない（`{"Kind":"Any","Occurs":…}`）。持つのは `##other` 4 件、`##local` 1 件、`##any` 1 件だけ。実際にはほぼすべてが `##any` として働き、`mc:AlternateContent` やベンダー拡張の要素にも順位が付く。影響は小さい（78 件は `ext` の Choice の中、63 件は唯一の子で、他のスロットと並ぶのは `pc:CT_SlideMonikerList` の1件だけ）。提案: 「Namespace が無い場合は `##any`。AC の扱いは §4.3 が優先する」と書く。

## R2-m6 [Minor] feature の依存関係: `mc`/`r` と、グループをまたぐ `ChildOf`

- §4.3 の AC の判定は core の primitive の中で行う。したがって MC 名前空間の URI は core に手書きで持つ必要がある。L125-127 の「`mc:` は `ooxml-schema` の feature の後ろ」と並べると、ここが曖昧になる。spike は `MC_NS` を core に置いている。
- `x:CT_Sheet` の `r:id`（Phase0 の読み取り）は `r` 名前空間の属性だ。L372 の「`ooxml-xlsx` enables `sml`」だけで足りるのかを書く。
- 名前空間 URI に spreadsheetml を含むスキーマファイル（`xdr14` など）にも、DrawingML 系（`xdr:` 12 件、`a:` 6 件、`a14:` 2 件）の子を参照するものが計 20 件ある（`xdr14:cNvPr → a:hlinkClick` など）。`impl ChildOf<P> for C` は、P と C の feature が両方とも有効なときだけ生成する（`cfg(all(...))`）、という規則が要る。

## R2-m7 [Minor] notices と出典の細部

- §1.3 L105-106 は「`ooxmlsdk` 由来の override は notices で表示する（§2）」としている。しかし §2 L133-135 の `THIRD-PARTY-NOTICES` の一覧には .NET Foundation と Ecma しか無い。
- 1,236 / 775 / 1,449 は `ooxmlsdk` の同梱物で測った値で、その版が Part 4 5th edition と一致するかは確認されていない（`ooxmlsdk` 側にも由来の記録が無い）。vendoring の時点で、Ecma から取得したファイルで測り直すと書く。
- L392-393 の「passes round-trip on 5,477 files」について。README の表では 5,477 件が pass しているが、そのうち round-trip の対象は 5,366 件で、16 件は open-only、95 件は invalid-package の期待どおりの結果。「5,366 files round-trip, 5,477 pass」が正確。

## R2-m8 [Minor] 生成器が当たるデータ上の例外

- `BaseClass` が同じファイルの中で解決できないものが wml に 10 件ある（`CustomXmlElement` ×5、`SdtElement` ×5。SDK の手書き C# のクラス）。L162-164 の「cross-file lookup is an error」のままでは、wml を有効にした時点で生成器が止まる。「データに無い base は、overrides で属性を補うまで Unsupported にする」と書く。
- `const _: () = assert!(content_contains(P::CONTENT, C::QNAME))` について。spike の `content_contains` は `Sequence` 以外では `false` を返す。そのため `Content::Unsupported(&ParticleTree)` の親に対する `ChildOf` の impl を検査すると、ビルドが落ちる。particle の木も探索する、と書く。
- 細部: §3 の例は `style_index() -> Result<u32, AttrError>` だが、spike の `CellRef::style_index` はパースに失敗すると黙って 0 を返す。spike の型名 `CellRef` は ADR では `CellView`。spike は ADR を裏付けるためのものなので揃えておく。

---

## 推奨する修正の優先順位

1. **R2-M1**（Choice の排他性、QName ごとの上限、`All`、SDK の Choice と ECMA の sequence の食い違い）: 一般規則の健全性の問題。Consequences の数字もこれに依存する。
2. **R2-M2**（既定値の結合規則を名前空間で正しく行い、字句が合わない場合の方針を決める）: 入力仕様の一部。spike の数字も更新する。
3. **R2-m1**、**R2-m4**: Phase0 の実装で最初に当たる（`xml:space`、merged range の検出）。
4. **R2-m2**（node id の表現と dirty の条件）: core の実装を始める前に1文で決める。
5. 残りの R2-m3、R2-m5〜m8。

## 判定

**No（Minor だけを直して Accepted にできる状態ではない）。** Blocker は無い。Phase0 の SML の範囲に限れば設計は成立していて、spike でも確かめられた。しかし revision 2 で新しく決めた2つの一般規則（§4.1 のスロットへの落とし込み、§1.2 の既定値の結合）は、データ上の反例があるまま「決定」として書かれている。R2-M1 と R2-M2 を規則として直し（必要なら「Phase0 範囲外では Unsupported や診断にする」と限定してよい）、R2-m1 と R2-m4 を合わせて直せば、round 3 で Accepted にできる見込み。

### 検証に使ったもの（scratchpad: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review2/`）
- `join2.py` / `join3.py`: 名前空間を正しく扱う既定値の結合。`attributeFormDefault`、グローバル属性への `ref`、`extension` の base に対応。
- `typecheck.py`: 結合した既定値が SDK の `Type` の字句空間に入るかを検査する。
- `choice.py`: slottable なモデルのうち、排他的な Choice の選択肢が繰り返すものを数える。
- `lens/`: path ベースの `LensMut` と view の newtype。子の view の再借用と、祖先への伝播を確認した。
- `neg/`: spike の負例3件を再現した（E0277 / E0116 / E0080）。
