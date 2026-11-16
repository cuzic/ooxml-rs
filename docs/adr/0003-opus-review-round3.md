# PR #20 レビュー（Opus, ADR-0003 round 3 / 実装 round 1）

対象: PR #20 `impl/xlsx-phase1` @ `f3056f1`（origin）。ADR-0003 revision 3（Status: Proposed）と `crates/ooxml-xlsx` の Phase1 実装。

- ローカルの worktree は、レビュー中に `c7f0682`（core PR #14 round-1 修正の merge）と、`remove_part(…, &[REL_CALC_CHAIN])` への追従という未コミットの変更に進んでいた。
- 実行と probe は、この状態（HEAD + 未コミット差分）を scratchpad にコピーして行った。差分は core API への追従だけなので、以下の指摘はどちらの状態にも当てはまる。

方針: 読み取り専用の敵対的レビュー。scratchpad（`review-pr20/`）では次のものを作って走らせた。

- probe 試験 `wt/crates/ooxml-xlsx/tests/zz_probe.rs`: 手組みの xlsx を作り、`dependents` と実際の出力を見る。
- ミューテーション2件: `edit.rs` の strip を改変し、全試験を走らせた。
- 最悪ケースのベンチ `examples/zz_bench.rs`。

LibreOffice のソースは master `2e885d0b` で確認した。重大度は依頼の基準に従う。

- **Blocker/Major**: stale 値が読める、`Err` なのに変更が残る、破損、保証が試験されていない。
- **Minor**: それ以外。
- 判断の分かれるものには「判断」、実機で確かめていないものには「要検証」と書いた。

## 結論（先に）

実装は ADR rev 3 に忠実で、出来が良い。

- round 2 の Major 3件は、構造としてはすべて解消された。
  - spill の潜在領域は `graph.rs` に入った。
  - `LOOKUP` は除外され、根拠つきの allowlist になった。
  - `SUMIF` はリテラルの矩形に限られた。
  - 名前とシートの解決規則が書かれた。
  - validate-then-apply と poisoning が入った。
- 主な主張（allowlist に無いものは `Everything`、`Err` なら何も変わらない、性能）は、確かめた範囲では成り立つ。
  - 2.2 ms/edit は再現した。ただしこれは最良ケースである。
- `cargo test -p ooxml-xlsx --all-features`: 全件 pass（ignored 2）。

ただし、Major が2件残る。

1. **R3-M1（健全性）**: 名前・共有式の解決で、「曖昧なら `Everything`」の原則が守られていない箇所が3つある。どれも probe で、編集後もキャッシュが残ることを確かめた。
   - ローカル名の中の、シート名の無い参照。
   - 同じ `si` を持つ master が2つある場合。LibreOffice は先勝ち、実装は後勝ちになる。
   - case folding で衝突するシート名や定義名。
2. **R3-M2（保証が試験されていない）**: 「許された差分だけを、すべて assert する」という主張に反して、`assert_allowed_diff` は依存先の `c` の他の属性も、`row` の開始タグも比べていない。依存先の `s` 属性を消すミュータントも、依存先の行の `row/@ht` を書き換えるミュータントも、全試験を通った。

どちらも局所的な修正で済む。R3-M1 は §2.4 に3行足すことと実装の数箇所、R3-M2 は oracle の強化である。

**判定: converged = no。ADR を Minor 修正だけで Accepted にできるか = no。** round 4 で収束する見込みは高い。

---

## round-2 指摘の解消状況

| ID | 内容 | 判定 | 根拠 / 残り |
|---|---|---|---|
| R2-M1 (1) | spill の拡大 | **解消** | ADR §3.2 L402-407。`graph.rs` `dependents` の `mark`（`c.dynamic \|\| c.spill_error` で、アンカーからシート末尾までを seed にする）。O1 にケースあり |
| R2-M1 (2) | `LOOKUP`、allowlist の基準 | **解消** | §2.2 L237-270。`formula/functions.rs` は根拠つきの表で、除外の単体試験もある。表の各エントリは目視で確認し、範囲外を読むものは見つからなかった |
| R2-M1 (3) | 計算された `sum_range` | **解消** | §2.3 (a)。`precedents.rs` `literal_rect` と `Rule::SumIf`。`SUMIF(…,INDEX(…))` は `Everything` になる（単体試験） |
| R2-M1 (4) | 名前とシートの解決 | **概ね解消（新しい穴あり）** | §2.4 L308-334 は実装どおりで、試験もある。ただし、ローカル名の中のシート名の無い参照、`si` の重複、folding の衝突が規則に無い → R3-M1 |
| R2-M2 | oracle の独立性 | **部分的に解消** | O2 の生成器は拡張された。O3 は合成 workbook にも適用される設計になり、script と workflow もある。しかし期待値のデータは未生成で、試験は `#[ignore]`。著者の意味論から独立した唯一の oracle が、まだ1回も動いていない（R3-M1 の (a) のような意味論の誤りは、O3 の入力に入って初めて検出できる）。運用の推奨は「Owner questions」を参照 |
| R2-M3 | staged clone のコスト | **解消** | `edit.rs` L93-194（plan）、L267-372（apply、順序は ADR と一致）。clone は `workbook.xml` だけ。poisoning は試験されている。細部 → R3-m3 |
| R2-m1 | census の失敗 | **解消** | `census.rs` の `EditBlock`。`strip_positions` で census と Layer 2 の不一致を `Malformed` にする（`edit.rs` L440） |
| R2-m2 | API | **解消** | §6。`worksheet_by_index`、`NotAWorksheet`、全シートの table 読み取り |
| R2-m3 | hand-off の細部 | **解消** | 「first remaining member」、合成の証明（§4.2 step 4）、`'It''s'`、`'a!b'`、`LOG10(` の試験（`translate.rs`）。ただしシート修飾の衝突 → R3-m1 |
| R2-m4 | iterate、`autoNoTable` | **解消** | `edit.rs` L156。`autoNoTable` は XL の項目 |
| R2-m5 | roadmap | **解消** | `docs/roadmap/xlsx.md` L319 以降 |
| R2-m6 | O3 の運用 | **概ね解消** | baseline 差分、バージョンの記録、ライセンス。ただし揮発性の除外が推移的になっていない。ADR の記述（`expected.json`、`tests/o3.rs`）も実装（`expected.tsv`、`phase1_o3.rs`）と食い違う → R3-m5 |
| R2-m7 | 主張の範囲 | **解消** | Consequences L811-817 |

---

## R3-M1 [Major] 名前と共有式の解決が、曖昧な入力で「精密だが誤った」集合を返す

原則（§2 L195-199「示せなければ `Everything`」）が、次の3箇所で守られていない。どれも、ある reader の解釈ではキャッシュが stale になる。fixture は信頼できない入力として扱う、という前提に立つ。

### (a) ローカル名の中の、シート名の無い参照（要検証、ただし安全側に倒すべき）

**根拠**:

- `precedents.rs` L318-322 の `name_own` は、ローカル名の中のシート名の無い参照を、名前のスコープのシートに束ねる。
- ADR §2.4 はこの場合を書いていない。グローバル名のときだけ `Everything` になる（`Bare` の試験）。

**再現**（`zz_probe.rs` P4）:

- シート `S1` に `A1=1`、`C1 = S2!Loc`（キャッシュ 7）。
- シート `S2` に `A1=7`。
- `<definedName name="Loc" localSheetId="1">$A$1</definedName>`。

`S1!A1` を書き換えると、`dependents` は `[]` を返し、`S1!C1` は `<v>7</v>` を保ったまま出力される（probe で確認した）。

**なぜ問題か**:

- Excel は、名前の式の中のシート名の無い参照（BIFF の ptgRef、UI では `=!$A$1`）を「名前を使っているセルのシート」で評価する。
- LibreOffice は、名前の式を `ScAddress(0, 0, mnCalcSheet)` を基準にコンパイルする（`sc/source/filter/oox/defnamesbuffer.cxx` L276、`DefinedName::getScTokens`）。シート名の無い参照は、そのシートからの相対シート参照になる、と読める。
- どちらの解釈でも、`S1` から `S2!Loc` を使うと `S1!A1` を読む。実装の `S2!A1` とは食い違う。

この shape は Excel 自身は書かない（Excel は常にシートで修飾する）が、openpyxl などの利用者が書いたファイルでは珍しくない。

**提案**: §2.4 に「名前の式の中の、シート名の無い参照は `Everything`」を足す（グローバル名と同じ扱い）。Excel が書くファイルでの精度の損失は無い。

### (b) 同じ `si` を持つ master が2つある

**根拠**:

- `graph.rs` L77-83 は `by_si` を `HashMap` に `collect` するので、後の master が勝つ。
- `census.master_of` は先の master を返す。同じ実装の中でも二通りの解釈がある。
- LibreOffice の `applySharedFormulas`（`formulabuffer.cxx` L196-213）は `SharedFormulaGroups::set` を使う。これは `std::map::try_emplace` なので、**先の master が勝つ**（[sharedformulagroups.cxx](https://docs.libreoffice.org/sc/html/sharedformulagroups_8cxx_source.html)）。
- ストリーム順に読む reader でも、`A2` の時点で知られている master は `A1` だけである。

**再現**（P1）: `A1: <f t="shared" ref="A1:A2" si="0">B1</f>`、`A2` は member、`A5: <f t="shared" ref="A5:A6" si="0">C5</f>`、`A6` は member。

- 実装は `A2` を `C2` を読む式と解釈する。
- `B2` を書き換えると、依存先は `[]` で、`A2` の `<v>2</v>` が残る。
- LibreOffice の解釈では `A2 = B2` なので、stale になる。

**提案**: census で、同じシートに同じ `si` の master が2つ以上あれば、そのグループの全セルを `Everything` にし、hand-off は `Err` にする。

### (c) case folding で衝突するシート名と定義名は、最初の一致に解決される

**根拠**:

- `precedents.rs` L84-86 `Env::sheet` は `position` で最初の一致を返す。
- `resolve` L97-115 も、同じ名前のグローバル名やローカル名が複数あると、最初の一致を返す。
- `workbook.rs` の `dependents` と `worksheet_by_name` も lowercase の最初の一致を返すので、2枚目のシートは名前では開けない。

**再現**:

- P2: シート `Data` と `DATA`。`S!A1 = DATA!A1`。`worksheet_by_index(1)` で `DATA!A1` を書き換えても、`S!A1` のキャッシュが残る。
- P13: シート `k` と、Kelvin 記号のシート `K`（U+212A。Rust の `to_lowercase` では `k` になる）。結果は同じ。
- P3: グローバル名 `Rate` と `RATE`（参照先が違う）。`B1` を書き換えても依存先は `[]`。

ADR §2.4 は「Excel は大文字小文字だけ違うシート名を禁止するので照合は曖昧でない」と書く。しかしそれは Excel が書いたファイルの性質で、入力の性質ではない。さらに、Excel の大文字小文字の同一視が Rust の `to_lowercase` と一致する保証は無い（Kelvin の例、要検証）。

**提案**: 開くときに、folding した後のシート名と、（スコープごとの）定義名の重複を検出する。衝突に関わる参照と名前は `Everything` にする。または、workbook 全体の編集を拒否する（`EditBlock`。判断）。

### 試験

O1 に (a)〜(c) の3ケースを足す。(a) は O3 の入力にも入れる（LibreOffice の解釈がそのまま期待値になる）。

---

## R3-M2 [Major] 「許された差分だけを assert する」が試験されていない

**主張**: ADR §7 L595-605 と PR 本文は「the tests assert exactly this allowed set, and nothing else」としている。ADR §3.3 L421 は、依存先について「leave `f` and every other attribute untouched」としている。

**実際の oracle**（`tests/common/p1.rs` L121-205 `assert_allowed_diff`）:

- **依存先の `c`**: `f` の文字列と、`v`/`t`/`is` が無いことだけを確かめる（L149-152）。`s`、`cm`、`vm`、`ph` などの他の属性や、子の順序は比べていない。
- **`sheetData` の中**: 比べるのは `c` の span の集合（`cell_spans` L47）と、`sheetData` の外（`frame` L63）だけである。`row` の開始タグ（`spans`、`ht`、`s`、`customFormat`）、`c` の間のテキスト、行の順序は比べていない。依存先しか変わらない「他のシート」では、Phase0 の `assert_t2` も使われない。
- **calcChain のレジストリ**: 要素の数と「`calcChain` という文字列が消えた」ことだけを確かめる（L188-191）。「1要素とその直前の空白だけ」というバイト単位の主張（§4.3）は、ooxml-xlsx 側では試験されていない。

**ミュータント**（scratchpad で実施。どちらも `cargo test -p ooxml-xlsx --all-features` の全件が pass した）:

1. `edit.rs` apply (iii) の strip に、`c.remove_style_index()?;` を足す（依存先のスタイルが消える）。
2. 同じ箇所で、`row.set_attr(sml::Row::ATTR_HT, "99")?;` を足す（依存先の行の高さが変わる）。

どちらも、ユーザーに見える破損（書式が消える、行の高さが変わる）である。

**提案**:

- `assert_allowed_diff` を、「出力 = 入力に、許された編集だけを当てたもの」の**バイト比較**にする。
  1. 依存先ごとに、入力の `c` の span から `v`、`t`、`is` を取り除いた期待テキストを作る。
  2. sheet part 全体について、許された span を置き換えた入力と出力が一致することを確かめる。
- calcChain のレジストリについても、期待するバイト列（入力から、その要素と直前の空白テキストを切り取ったもの）と比較する。
- O1 の合成 workbook の式セルに、`s` 属性と `row` の `spans`/`ht` を持たせる。上の2つのミュータントを試験コメントに記録する。

---

## Minor

### R3-m1 シート修飾の名前/参照の衝突が、平行移動で `Err` にならない

**根拠**: ADR §2.4 L328-331 は「Translating such a token fails」とする。しかし `translate.rs` L63 は `r.area_offset == 0` のときだけ衝突を調べる。前件の解析（`precedents.rs` `ref_rects`）は修飾つきでも `Everything` にするので、二つの解析が食い違っている。

**再現**（P5）: ローカル名 `TAX2024` がある。master `B1: Sheet1!TAX2024+A1`、member `B2`。`B1` を上書きすると成功し、新しい master は `B2: Sheet1!TAX2025+A2` になる。

Excel がこのトークンを名前と読む場合、式の意味が変わる（`B2` はキャッシュを失うので stale にはならない）。

**提案**: `area_offset` の条件を外し、修飾つきのトークンも衝突として `Err` にする。

### R3-m2 `set_formula("==A1")` が `<f>=A1</f>` を書く

**根拠**: `worksheet.rs` L551 と `parse.rs` L31 が、それぞれ `=` を1つずつ取り除く。そのため、2つ目の `=` は lexer に届かない（P6 で確認）。

先頭が `=` の `f` テキストは Excel の文法外で、修復ダイアログになると考えられる（要検証）。

**提案**: `parse` を ADR §4.1 どおり「先頭の `=` を1つだけ許す」にする。`set_formula` は自分で1つ取り除いたうえで、`=` を許さない内部 parse を呼ぶ。

### R3-m3 poisoning の抜け道

- `Workbook::package()`（`workbook.rs` L353）は `&Package` を返し、`Package::to_bytes`/`save` は `&self` で呼べる。poisoned な workbook でも `wb.package().save(path)` で途中状態を書き出せる。
- apply の中の `expect`（`write_cell` の `"located row"` など）が panic した場合は、poison されない。`catch_unwind` した呼び出し側は、途中状態を保存できる。

**提案**:

- poisoned のときは `package()` が `Err` または `Option` を返すようにするか、doc に明記する。
- apply を、panic でも poison するガード（drop guard）で包む。

### R3-m4 性能: 2.2 ms は最良ケースで、ADR の一般化は正しくない

**再現**: `phase1_bench` を再実行すると、ADR の値が再現した。

| 項目 | 実測 |
|---|---|
| open | 686 ms |
| first edit | 126 ms |
| 1,000 編集 | 2.22 ms/edit |
| 依存先がすでに剥がされた編集 | 1.55 ms |
| save | 186 ms |

ところが、`B1` を `t="e"` / `#SPILL!` のキャッシュにした1セルだけの変種（`zz_bench.rs`、`WORST=1`）では、次のようになった。

| 項目 | 実測 |
|---|---|
| first edit | 480 ms |
| 以降の編集 | **90 ms/edit**（すでに剥がされていても同じ） |

**原因**: `Graph::dependents` は、毎回 20 万の式すべてを推移的にたどる。依存先が `Everything` の動的配列アンカー（`FILTER`、`SEQUENCE`、`UNIQUE` など。今どきの workbook では珍しくない）が左上にあれば、同じことが起きる。

**ADR の記述との関係**: Consequences L814「Edits cost time proportional to the cells that still lose a cached value, not to the size of the sheet」は誤りである。正しくは「依存集合の大きさに比例し、すでに剥がされたセルも数える」。

**提案（判断）**:

- ADR の記述を直し、最悪ケースの数字を併記する。
- 改善するなら、「キャッシュも result も剥がし終えた式で、それ以降に入ってきた辺が無いもの」で探索を打ち切るメモ化が考えられる。`set_formula` で辺が増えるときにメモを無効化する必要があるので、O2 で試験すること。

### R3-m5 O3 の記述と実装の食い違い

- ADR §7.1 L675、L678、L685 は `expected.json` と `tests/o3.rs` と書いている。実装は `expected.tsv` と `tests/phase1_o3.rs` である。
- ADR は「揮発性の関数を含むセル**とその推移的な依存先**を除外」と書いている。`phase1_o3.rs` L61 と L89 は、テキストに揮発性の関数を含むセルしか除外しない。
- 共有式の member は `text` が空なので、master が揮発性でも除外されない。
- 実害は baseline の比較でほぼ吸収されるが、偽陽性の原因になりうる。依存グラフ（`Workbook::dependents`）は使わずに、テキストを解決したうえで、手書きの推移閉包で除外する。

### R3-m6 試験の穴（小）

- **`workbook.xml` の `calcPr` の挿入位置に `mc:AlternateContent` がある場合**（§3.1 L356-359）: 試験が無い。P7 で、`Err(UnresolvedSibling)` になり、出力が L1 であることは確かめた。そのまま回帰試験にできる。
- **fault-injection**（`phase1_formulas.rs` L215-229）: 1種類の編集（`Sheet2!B1` の値による上書き）でしか試していない。次の編集でも各注入点を試す。
  - 共有式の master の hand-off を伴う編集
  - calcChain が無い workbook
  - `KeepStaleCaches`

### 範囲の判断（依頼の項目への回答）

- **条件付き書式とデータ入力規則の式**: セルのキャッシュ値を持たないので、「stale な cell-formula cached value」の主張とは無関係である。対象外でよい。
- **chart と pivot のキャッシュ**: R2-m7 のとおり、範囲外と明記されていれば十分。pivot の出力セルは Excel 自身も `refreshOnLoad` が無ければ更新しないので、Phase1 が悪化させることはない。
- **その他、穴が見つからなかったもの**: `@`（`Unary` として扱われ、上位集合になる）、構造化参照・`A1#`・`LET`/`LAMBDA`・`_xlfn.` の関数（いずれも `Everything`）、3-D の逆順、`A:A`/`1:1`、名前を介した名前（P11）、R1C1 形のトークン（未定義の名前として `Everything`）、data table（`Everything` と result の seed）、CSE 配列の result、spill がシート端まで広がる場合、小文字で書かれた関数名（`lookup(` は表に無いので `Everything`、P16）。

---

## Owner questions への推奨

1. **`KeepStaleCaches` を残すか**: 残してよい。ただし、名前に危険を表す（例: `ExcelOnlyKeepStaleCaches`）か、`#[doc]` の冒頭で「LibreOffice、calamine、pandas が誤った値を読む」と明記する。既定は今のままにする。下位 API（`fullCalcOnLoad` だけ）に分ける利点は小さい。
2. **O3 のデータの生成場所**: **手動の CI workflow（`o3-recalc.yml`）を推奨する。** LibreOffice のバージョンが記録されて再現でき、owner の環境に依存しない。owner が artifact を確認してコミットする。
   - R3-M1 (a) のような「著者の意味論の誤り」を検出できるのは O3 だけである。そのため、ADR の Accepted の条件にはしなくてよいが、**Phase1 milestone を閉じる（PR を merge する）前に一度回す**ことを推奨する。
   - workflow は `ubuntu-latest` の apt 版の LibreOffice を使うので、版を固定する（コンテナの digest など）か、記録された版を TSV の照合条件にする。
3. **閾値**: `Workbook::dependents` で足りる。警告の仕組みは要らない。R3-m4 の最悪ケースを doc に書く。
4. **manual モード**: 「`fullCalcOnLoad` を立てず、依存先のキャッシュは剥がす」を維持する。値は誤って見えるのではなく空に見えるので安全側である。XL の確認（F9 の前に何が表示されるか）は記録のためで、方針は変えなくてよい。

## 推奨する修正の優先順位

1. **R3-M1**: §2.4 に (a)〜(c) の規則を足し、実装する。O1 に3ケースを足す。
2. **R3-M2**: `assert_allowed_diff` をバイト比較にし、O1 の合成 workbook に `s` と `row` の属性を持たせる。
3. **R3-m1、R3-m2、R3-m3**: 数行ずつの修正。
4. **R3-m4、R3-m5、R3-m6**: 記述の修正と、試験の追加。
5. O3 の CI workflow を一度回し、`expected.tsv` をコミットする。

## 判定

- **converged: no.** Major 2件（R3-M1 健全性、R3-M2 保証が試験されていない）。どちらもアーキテクチャの変更は要らない。
- **ADR を Minor 修正だけで Accepted にできるか: no.** R3-M1 には §2.4 の規則の追加が要る。数行なので、round 4 で収束する見込みが高い。

## 出典と再現物

- probe 試験: scratchpad `review-pr20/wt/crates/ooxml-xlsx/tests/zz_probe.rs`
  - `probes`: P1〜P18
  - `probes_idx`: P1、P2、P4、P13 の実際の出力の確認
- ミューテーション: `review-pr20/edit.rs.orig` に対して、上記2件（strip で `remove_style_index`、`row/@ht`）。
- ベンチ: `examples/phase1_bench.rs`（再現）、`review-pr20/wt/crates/ooxml-xlsx/examples/zz_bench.rs`（`WORST=1 N=50`）。release build、この sandbox で計測。
- LibreOffice/core master `2e885d0b`:
  - `sc/source/filter/oox/formulabuffer.cxx` L190-235（`applySharedFormulas`）
  - `sc/source/filter/oox/defnamesbuffer.cxx` L274-300（`DefinedName::getScTokens`、`aReferenceAddr`）
  - `sc/source/core/tool/sharedformulagroups.cxx`（`try_emplace`）: <https://docs.libreoffice.org/sc/html/sharedformulagroups_8cxx_source.html>
- ECMA-376 Part 1 §18.3.1.40（`f`）、§18.2.5（`definedName`）。
