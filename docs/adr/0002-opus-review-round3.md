# ADR-0002 レビュー（Opus, round 3）

対象: `docs/adr/0002-schema-driven-code-generation.md` revision 3（branch `docs/adr-0002-schema-codegen`, commit `48956b8`）

方針: 収束確認のための読み取り専用の敵対的レビュー。round 2（`0002-opus-review-round2.md`）の R2-M1 / R2-M2 / R2-m1〜m8 が解消されたかを判定し、revision 3 で新しく書いた規則（スロット規則、ECMA 照合、prefix 規則、dirty 規則、node id、MissingReference、parse scope）に新しい穴が無いかを確かめた。根拠は次のとおり。
- ADR-0001（Accepted、post-acceptance note を含む）との突き合わせ。
- spike の `cargo test`、README の負例3件、`py/` の3スクリプトを rustc 1.99.1 / `python3 -I` で再実行した結果。
- scratchpad（`review3/`）で書いた検証用スクリプト。`xcheck.py` は SDK と ECMA の排他性・個数上限の食い違いを調べる。ほかに単発のデータ照会をいくつか行った。

重大度は round 1・2 と同じ3段階（**Blocker** / **Major** / **Minor**）。判断の分かれる指摘には「判断」と書いた。

## 結論（先に）

**R2-M1 と R2-M2 は解消された。R2-m1〜m8 もすべて解消されている。** Blocker と Major は新たに見つからなかった。

- spike の `cargo test` は 6 件すべて通った。README の負例3件も再現した。
  - E0277 が `on_unimplemented` の文言どおりに出て、既存の `ChildOf<Cell> for CellValue` も指し示す。
  - E0116。
  - E0080（`default_is_valid`）。
- ADR に書かれた数値は、すべてスクリプトの出力と一致した。
  - 1,511（`join3.py`）。
  - 39（`typecheck.py`。内訳は a 25 / c 9 / p 5 で、SML は 0）。
  - 1,574 / 483 / 65（55 / 8 / 2）/ ECMA との順序の食い違い 8 件。いずれも `slottable.py`。
  - 2,057 = 2,122 − 65。
  - 同じファイル内で解決できない `BaseClass` 10 件。
- revision 3 で書き直した dirty 規則、path による node id、MissingReference、parse scope の note は、いずれも ADR-0001 の L2 シリアライズ規則と T2 に矛盾しない（後述「確認して問題が無かった点」）。

新しい指摘は Minor の6件。いずれも規則の言い回しの不足か、spike の数え方のずれで、設計を変える必要は無い。Phase0 に効くのは **R3-m3（テキストを書く primitive が §4.2 に無い）** だけで、Phase0 のすべての書き込みがここを通る。

**判定: Yes。下記の Minor を直せば Accepted にしてよい。** round 4 のレビューは不要。R3-m3 だけは Accepted にする前に1文足すことを推奨する。残りは同じコミットで直しても、実装時の宿題として記録するだけでもよい。

---

## round-2 指摘の解消状況

| ID | 内容 | 判定 | 根拠 / 残り |
| --- | --- | --- | --- |
| R2-M1 | スロットへの落とし込み（Choice の排他性、個数上限、SDK と ECMA の食い違い） | **解消** | §4.1 L342-394 で、`max_each` / `max_total`、繰り返す選択肢を持つ排他的な Choice の Unsupported 化、`All` の扱い、ECMA との順序照合と `set_content` を定めた。`x:CT_Filters`、`x:CT_SortState`、`p188:CT_Comment` は、spike でも Unsupported と判定された。`xcheck.py` で、ECMA が禁じる同時出現（排他性）と ECMA を超える個数も調べた。SDK のデータ自体の食い違いは `w:sdtPr` だけで、これは既存の 8 件に含まれる。残りは「持ち上げ」の意味と数え方の細部 → R3-m1, R3-m2 |
| R2-M2 | 既定値の結合（名前空間、`ref`、字句の不一致） | **解消** | §1.2 L91-117。`join3.py` の出力は 1,511 件（内訳 `ct` 1,382 / `agroup` 86 / `agroup+ref-global` 32 / `agroup+ref-use` 6 / `ct+ref-global` 5）。型に合わない 39 件は `DefaultMismatch` の診断を出して採用しない。§3 の const 検査がビルドを落とす経路は無くなった。union 型を入れる手段だけが未定義 → R3-m6 |
| R2-m1 | 新しいノードの prefix | **解消** | L224-235。`xml` は常に束縛済み、属性は空でない prefix、要素は親の prefix を優先する。細部 → R3-m5 |
| R2-m2 | node id と dirty の条件 | **解消** | L420-441。node id は path とし、`own_dirty` は「元が自己終了タグの要素が最初の子を得たとき」だけにした。閉じタグがある要素は、子を失っても開始・終了タグをそのまま出す。spike の `self_closing_parent_gains_first_child_and_ancestors_marked` と `insert_within_slot_by_column_and_dirty_path`（`!row.own_dirty` を確認する）が通る |
| R2-m3 | `insert_by` が位置を受け取れない | **解消** | L405, L414-418。`before(index_in_slot, existing)` と Phase0 の `Err(MissingReference)`。細部 → R3-m4 |
| R2-m4 | `mergeCells`、parse scope | **解消** | L497-505 に allowlist と parse scope を書いた。ADR-0001 の post-acceptance note（L47-52）とも一致する。§6 の rels のパスは workbook の名前から導く |
| R2-m5 | Any の namespace | **解消** | L365-367 |
| R2-m6 | `mc`/`r`/`xml` と feature をまたぐ `ChildOf` | **解消** | L169-173 で core の定数にし、L332-338 で `cfg(all(..))` にした |
| R2-m7 | notices と出典 | **解消** | L179-184、L119-121（Ecma から取得したファイルで測り直す）、L538-539（5,366 / 5,477） |
| R2-m8 | データ上の例外 | **解消** | L211-216 の `attrs_incomplete` で 10 件を扱う（再集計で 10 件を確認した）。spike の `content_contains` は `Unsupported` の名前の一覧も探す。`style_index` は `Err` を返し、型名は `CellView` に揃った |

---

## 新しい指摘

### R3-m1 [Minor] 「繰り返しによる持ち上げ」と繰り返す Choice の上限は、積として定義する必要がある（spike は無制限にしている）

**根拠**
- §4.1 L361-363: 「A repeating one that lowers to a single slot lifts its cardinality onto that slot」。L351-354: 繰り返す Choice の「both limits come from the choice's `maxOccurs` and each alternative's」。どちらも計算方法が書かれていない。
- `slottable.py` は、繰り返す Group/Sequence/Choice の中にあるスロットを、`maxOccurs` が有限でも一律に無制限（`None`）にしている（`out = [{q: None …}]`、`s = {q: None for q in s}`）。
- データでは、有限の `maxOccurs > 1` を持つ Group が 8 件、Choice が 30 件ある。ECMA の上限が意味を持つ例は次の2つ。
  - `a:CT_DuotoneEffect/a:duotone` は `Group{2..2}[Choice[scrgbClr, srgbClr, …]]`。ECMA の `dml-main.xsd` L1302-1306 も `EG_ColorChoice minOccurs=2 maxOccurs=2` で、色はちょうど2つ。
  - `w:CT_SectPr/w:sectPr` は `Group{0..6}[Choice[headerReference, footerReference]]`。
  
  `xcheck.py` で調べると、spike のように無制限へ持ち上げた場合、ECMA の上限を超える個数を許すモデルはこの2つだけだった（もう1件の `w:sdtPr` は ECMA との順序の食い違い 8 件に既に入っている）。spike どおりに実装すると、3つ目の色の挿入が `Ok` を返し、不正な出力になる。
- 細部: Choice の3つの規則は「alternatives are all single elements」と書かれている。しかし、slottable と判定されたモデルのうち 85 件では、繰り返さない Choice の選択肢が `Any` か、1スロットに落ちる入れ子の Sequence/Group（例: `c:CT_LegendEntry` の `txPr`）になっていて、3つの規則のどれにも文字どおりには当てはまらない。spike はこれらをまとめて扱っており、それは健全だが、文言と食い違う。

**提案**
- 次のように書く。「繰り返しの上限 r を持つ particle が1スロットに落ちる場合、`max_each` と `max_total` はそれぞれ r 倍する（無制限 × 何でも = 無制限）。繰り返す Choice の `max_each` は、Choice の上限と選択肢の上限の積とする」。
- Choice の規則の主語を「single elements」から「それぞれが1スロットに落ちる選択肢」に変える。
- 判断: ECMA との照合は、順序だけでなく同時出現と個数上限も比べると安全になる。今のデータでは追加で見つかるものは無い（上記のとおり）ので、必須ではない。

### R3-m2 [Minor] `slottable.py` は、トップレベルで繰り返す多スロットの Sequence を見逃している（ADR の規則では 66 / 2,056 になる）

**根拠**
- `lower()` は、繰り返す多スロットの group を `if not top and …` のときだけ Unsupported にする。トップレベルは対象外になっている。
- このため `x:CT_RPrElt/x:rPr`（`Sequence`, `Occurs: [{"Min": 1}]` で Max が無く、spike の読み方では無制限。`b`, `i`, `strike`, … の多スロット）が slottable と判定される。各スロットは無制限に持ち上げられる。
- ADR L369-370 の「unsupported if a multi-slot sequence/group repeats」をそのまま当てはめると Unsupported になる。数値は 65 → 66、2,057 → 2,056 になる。

**影響**
- 小さい。ECMA の `CT_RPrElt` は `choice maxOccurs="unbounded"`（`sml.xsd` L1826-1827）で、順序は自由。spike の解釈は ECMA より厳しいだけなので、不正な出力は生まない。
- ただ、spike が ADR の規則を実装している、という主張とはずれる。
- `rPr` は、Phase0 で読み取り専用の `x:r` の子で、将来リッチテキストを書くときに当たる。

**提案**
- spike の `not top` を外して数え直し、数値を直す。
- または ADR に「トップレベルの繰り返しは無視する」と書く。ただしその場合は、上記の理由で健全ではない場合がある。どちらにするかは判断。
- `Occurs` の Max が無いときの読み方は、spike のとおり「heuristic」であることを、数値と一緒に書いておく。

### R3-m3 [Minor] §4.2 にテキストを書く primitive と、その dirty の義務が無い（Phase0 のすべての書き込みが通る）

**根拠**
- §4.2 の表（L403-409）の primitive は、`insert_by` / `insert` / `get_or_insert` / `remove` / `set_attr` / `remove_attr` だけ。§3 の「Leaf text」（L307-309）は読み取りの型しか定めていない。
- `set_number` は `<v>` のテキストを、`set_string` は `<t>` のテキストを置き換えるか新しく作る。しかし、それを行う primitive も dirty の規則（L428-441）も無い。
- 実際のファイルには `<is><t/></is>` のような自己終了タグのテキスト要素があり、テキストを入れるには `own_dirty` が要る。L434-436 の「gains its first child」に、テキストの子が含まれるのかどうかが書かれていない。spike の判定も、要素を挿入したときだけ行っている。
- ADR-0001 の L240-246 は、`<t>` に `xml:space="preserve"` を付けることを書く側の義務にしている。どの層がそれを行うのかも決まっていない。

**提案**
§4.2 の表に `set_text(value)`（`Content::Text` の要素だけ）を足し、dirty の義務に次の2つを書く。
- 「テキストの置き換えは、その要素と祖先に `subtree_dirty` を立てる。元が自己終了タグの要素が最初のテキストを得たときは `own_dirty` を立てる」。
- 「`xml:space` の付け外しは `ooxml-xlsx` の `set_string` が `set_attr` で行う」、あるいは「`set_text` が行う」のどちらか1つ。

### R3-m4 [Minor] MissingReference の条件があいまいで、`r` の無いシートの読み取り側が決まっていない（判断）

**根拠**
- L416-418 は「whose existing children lack `r`」としているが、1つでも欠けていれば該当するのか、すべて欠けているときだけなのかが読み取れない。
- `r` があるものと無いものが混ざった `row` は、position-implied のセルを含む（`<c r="B1"/><c/>` の2つ目は C1）。「すべて」と読めば、このような row への書き込みが通り、`before` の比較が `r` の無い兄弟で誤る。
- 読み取りの `cell(row, col)`、`<dimension>` の再計算、merged range の判定（ADR-0001）は、いずれも `r` を前提にしている。`r` の無いシートでは、`cell()` が値のあるセルに対して黙って `None` を返しうる。破損した出力にはならないが、黙って誤った値を読むことになる。

**提案**
- 「1つでも `r` が無ければ」と書く。
- 判断: Phase0 では、同じ条件で読み取りも `Err(MissingReference)` にするか、位置から列番号を数える。どちらにするかを1文で決める。

### R3-m5 [Minor] prefix の規則と、ADR-0001 の `RawNode` の定義の細部

- L230-232 の「innermost in-scope binding for the URI」は、内側で同じ prefix が別の URI に再宣言されて隠れている束縛を除外していない。「その要素の位置で実際に効いている束縛のうち、最も内側のもの」と書く。
- 属性の場合、URI を束縛しているのが既定の名前空間（`xmlns="…"`）だけなら `Err(NamespaceNotInScope)` になる。そのことを L234 で明示する。L227-228 からそう読むことはできるが、L234 の「no binding」と字面が合わない。
- ADR-0001 の `RawNode` は、`name: QName`（未定義）と `attrs: Vec<(String, String)>` のままになっている。ADR-0002 L219-221 の「解決した URI と元の prefix を両方記録する」は、実質的に ADR-0001 の型を変えている。parse scope と同じように、ADR-0001 に post-acceptance note を1行足すとよい（判断）。

### R3-m6 [Minor] union の simple type を入れる手段が overrides に無い（判断。Phase0 には影響しない）

- L112-114 は「fixed where it matters by giving `ooxml-core` the union simple types」としている。しかし、SDK が `Int32Value` としている属性（`a:xfrm/@sx` など）の型を union に差し替える手段が、生成器の規則にも §1.3 の override 操作にも無い。§1.3 の操作は「fixed now」と書かれているのに、型を変える操作（例: `set_type { on, attribute, type }`）が含まれていない。
- 差し替えないかぎり、Transitional の正当な文書（`sx="50%"`）を読むと `AttrError` が返る。L115-117 の「never lossy」によって値は失われないので、安全側ではある。
- 提案: `set_type` を §1.3 に足す。あるいは「XSD の属性型が `ST_Percentage` 系であれば、生成器が union 型を選ぶ」という規則を書く。dml/pml/chart を allowlist に入れるまでに決めればよい。

---

## 確認して問題が無かった点（task の観点別）

- **元が自己終了タグの要素から子を取り除く場合**: 元が自己終了タグなら子は無いので、取り除けるのは自分で足した子だけになる。足した時点で `own_dirty` が立っているので、L440-441 のとおり自己終了タグとして再描画され、well-formed になる。ADR-0001 L224-227 の「(or loses its last one)」とも矛盾しない。
- **閉じタグのある要素が最後の子を失う場合**: `!own_dirty` のまま、元の開始タグと閉じタグがそのまま出る（`<row …></row>`）。T2 の局所性も保たれる。その後に属性を書くと `own_dirty` になり、自己終了タグとして再描画される。差分はそのノードの span の中に収まるので、これも T2 の範囲内。
- **属性の書き込みと `own_dirty`**: ADR-0001 の `own_dirty` の定義（「own name/attrs changed」）と一致する。祖先には `subtree_dirty` だけが立ち、祖先の開始タグはそのまま出る。spike の `insert_within_slot_by_column_and_dirty_path` で確認した。
- **path による node id と、返される子の view**: spike で、子の `LensMut` は `&mut self` から再借用している。そのため、挿入で添字がずれても古い path を持つ view は借用検査で存在できない。L423-426 の主張どおり。
- **MissingReference と ADR-0001 の from-scratch のテンプレート**: テンプレートの `<sheetData/>` には子が無いので、条件に当たらない。ooxml-rs が作る `row`/`c` には常に `r` が付く。矛盾は無い。
- **ADR-0001 の parse scope の note**: L47-52 は ADR-0002 §5 の L499-505 と一致する。ADR-0001 本文の L443「Layer 2 only covering `sheet1.xml`」は、この note で上書きされると読める。
- **Phase0 のエントリ**: `worksheet`（38 スロット、すべて単一 QName）、`sheetData`、`row`、`c`、`is`、`mergeCells`、`sst`、`sheets`、`workbook` は、いずれも単一 QName のスロットだけに落ちる。ECMA との順序の食い違いも無い。

## 推奨

1. **R3-m3**（`set_text` と、その dirty / `xml:space` の義務）: Phase0 の書き込みがすべて通る経路なので、Accepted にする前に足す。
2. **R3-m1**、**R3-m2**: §4.1 の「持ち上げ」を積として定義し、spike の数え方を直して数値（65/2,057 または 66/2,056）を確定する。
3. **R3-m4**: Phase0 の `r` の無いシートに対する読み書きの扱いを1文で決める。
4. **R3-m5**、**R3-m6**: 文言を直すか、実装時の宿題として記録する。

## 判定

**Yes。上記の Minor だけを直せば Accepted にしてよい**（R3-m3 は Accepted にする前に直すことを推奨し、残りは同じコミットで直すか、宿題として記録するかのどちらでもよい）。Blocker と Major は無い。revision 3 で書いた規則は、データ（SDK 2,122 モデル、ECMA の XSD）と spike の両方で裏付けられている。round 4 は不要。

### 検証に使ったもの（scratchpad: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review3/`）
- `cargo test`（`ws/`、6 passed）と、`neg/` に置いた負例3件（E0277 / E0116 / E0080）。
- `join3.py` / `typecheck.py` / `slottable.py` を再実行した。
- `xcheck.py`: SDK のスロットと ECMA の particle を比べ、排他性と個数上限の食い違いを調べる。
- 単発の照会: 有限の `maxOccurs > 1` を持つ group/choice、トップレベルの `Occurs`、`BaseClass` の未解決、Phase0 エントリのスロットへの落とし込み、繰り返さない Choice の要素以外の選択肢（85 件）。
