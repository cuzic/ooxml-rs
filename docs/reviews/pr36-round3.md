# PR #36 レビュー（Opus, round 3）

対象: `test/fuzz-core` @edc9bb1（round 2 からの差分は 00470ab〈main マージ〉と edc9bb1）。
検証: 独立した `git clone` 3 つ（head、head＋現在の origin/main 2495c82 のマージ、oracle 試作用）。Rust 1.99.1、nightly 1.102.0（5f4effd57）、cargo-fuzz 0.13.2。

## 結論（先に）

**round 2 の M1'・m10・m11・Nit はすべて正しく直っている。** 子プロセスが 0 件しか実行しない変異と abort する変異の両方で、親テストは失敗した。
main マージ後は 9 件の回帰テストが ignore なしで pass し、`known.rs` を消してもどのターゲットも既知の所見で落ちない（nightly 各 4 分で失敗なし）。

ただし、xlsx Phase1 に合わせて緩めた「他の part は L1 一致」の oracle は**緩めすぎている**。
除外対象が「全シート、workbook.xml、`[Content_Types].xml`、全 `.rels`」なので、ooxml-xlsx に入れた 7 つの変異のうち **5 つが生き残った**。
その中には、まさに懸念どおりの「無関係なシートを壊す」変異（X7）と「workbook.xml を calcPr 以外も書き換える」変異（X3）が含まれる。
styles.xml と sharedStrings.xml は除外されていないので、今も検査されている。
公開 API（`Workbook::dependents`）だけで作れる、より厳しい oracle を試作した。この試作は 7 つの変異をすべて検出し、誤検出もなかった（スモーク 150 iters、nightly 4 並列×5 分で計 2.4 万回）。

- Major 1 件（M2）、Nit 3 件
- 判定: **converged: no**（M2 のみ。試作をそのまま取り込めば次ラウンドで収束見込み）

## 解消状況

| ID | 状態 | 確認内容 |
|---|---|---|
| M1' | 済 | 子に `--include-ignored` を渡し、親が `1 passed` を assert する。変異で確認した。(a) `--ignored` に戻すと、子は 0 件実行で終了コード 0 になるが、親は「child process did not run the test」で**失敗**する。(b) 子の本体の先頭で `abort()` すると、親は**失敗**する |
| m10 | 済 | origin/main（#14・#19・#20・#40）をマージし、`pptx.rs` の比較を直した。9 件とも `#[ignore]` なしで pass し、`known.rs` とその呼び出しはすべて削除済み（grep で残りなし）。nightly で `known.rs` なしに 5 ターゲットを各 4 分回した結果、xml_t1 57.6 万回、xml_t1b 64.5 万回、lens_ops 27.3 万回、package_roundtrip 3.3 万回、xlsx_edit 1.0 万回で、**失敗なし**。既知の所見で落ちるターゲットはない |
| m11 | 済 | ワークフローに `cargo metadata --locked` のステップがある。head でも、現在の main とのマージ後でも通る |
| Nit 1（nightly の日付） | 済 | `nightly-2026-11-14` にし、コメントに rustc のハッシュを書いた |
| Nit 2（F-4 フィルタ） | 済 | フィルタごと削除 |
| Nit 3（seed 数の表示） | 済 | 合成の変種も数える |

**現在の main（#37・#44・#45・#46 入り、2495c82）とのマージ**: 衝突はない。`cargo test -p ooxml-robustness`（release）は全件 pass し、
スモーク `OOXML_SMOKE_ITERS=60` も pass、`fuzz/Cargo.lock` の `--locked` も通る。

## 所見

### M2 [Major] Phase1 用に緩めた「他 part は L1 一致」が、Phase1 の契約外の書き込みを見逃す

**場所**: `crates/ooxml-robustness/src/xlsx.rs` 108–121 行（`except` と `removable` の組み立て）

現在の除外対象は、content type がシートの part すべて、workbook part、`[Content_Types].xml`、名前が `.rels` で終わる part すべてである。
`calcChain.xml` は「消えてもよい」扱いになっている。ADR-0003 の Phase1 が実際に書き換えるのは次の部分だけで、除外範囲はこれよりずっと広い。

- 依存セルを持つシートの、依存セル（とスピル範囲などの結果セル）の `v`・`is`・`t` だけ
- workbook.xml の `calcPr/@fullCalcOnLoad` だけ
- calcChain を消すときの `xl/_rels/workbook.xml.rels` の 1 関係と、`[Content_Types].xml` の 1 Override だけ

`_rels/.rels`、シートの `.rels`、drawing の `.rels` などは、どの編集でも変わってはいけない。
calcChain は、数式セルを値で上書きしたときに限って消えなければならず、それ以外のときは残らなければならない。

**変異で確認した結果**（ooxml-xlsx `edit.rs` に手で入れ、スモーク `OOXML_SMOKE_ITERS=30` で実行）:

| # | 変異 | 現行 oracle | 試作 oracle |
|---|---|---|---|
| X1 | `strip_positions` が依存セルのある行の全セル（定数も）を剥がす | 検出（読み戻し） | 検出（読み戻し） |
| X2 | 依存セルの剥がしを別のワークシートに適用する | 検出（位置が範囲外のとき poison 経由で偶然） | 検出（「sheet2.xml changed beyond what Phase1 may change」） |
| X3 | `calcPr` に `iterate="1"` も書く | **生存** | 検出 |
| X4 | 数式セルを上書きしても calcChain を消さない | **生存** | 検出 |
| X5 | 依存セルの `f`（数式そのもの）も消す | **生存** | 検出 |
| X6 | 数式セルの集合が変わらなくても calcChain を消す | **生存** | 検出 |
| X7 | 編集のたびに、依存のない別シートの先頭セルの `v` を消す | **生存** | 検出 |

X7 は「Phase1 が無関係なシートを壊したら気づくか」という問いへの直接の答えで、現行の oracle では気づかない。
X5 は、依存シートでの剥がしすぎも見ていないことを示す。
スモークの 30 iters では、編集対象以外のシートに依存セルがあるケースが 59 回、calcChain の削除が 27 回あるので、入力側の頻度は十分ある。足りないのは oracle だけである。

**修正案**（試作済み。差分は `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-36r3-47r2/tight-oracle-prototype.diff`、約 150 行）:

1. 書き込みが成功するたびに、その直前に `wb.dependents(sheet_name, CellIndex)` を取り、(シート名, セル参照) の集合 D に足す。
   同じく直前の `cell(row, col).formula().is_some()` で「数式セルを上書きした」（F）を記録する。
2. 最後の比較は part ごとに行う。raw entry が一致する part は OK とし、そうでない part は次の規則のどれかを満たさなければならない。
   - **依存セルを持つシート**（D から `worksheet_by_name(..).part_name()` で引く）: 両方を parse して木を比べる。D に含まれる `c` に限り、子の `v`・`is` と属性 `t` の差を許す。それ以外の差は失敗。
   - **workbook part**: 何かの編集で依存または F があった場合に限り、`calcPr` の `fullCalcOnLoad` の差（または `fullCalcOnLoad="1"` だけの `calcPr` の新規挿入）だけを許す。変更後の値が `"1"` であることも確認する。
   - **`xl/_rels/workbook.xml.rels` と `[Content_Types].xml`**: calcChain が消えた場合に限り、calcChain の `Relationship`・`Override` 要素がなくなる差だけを許す。
   - それ以外（ほかのシート、すべての `.rels`、styles・sst など）: 差があれば失敗。
3. calcChain（**ooxml-xlsx と同じく workbook の calcChain 関係から解決**したもの）は、F なら消えていなければならず、F でなければ残っていなければならない。
   content type で calcChain を探すと、payload で関係を外した入力（孤立した calcChain は Phase1 が消さない）で誤検出する。これは試作の途中で実際に踏んだ。
4. 編集対象のシート自体は、これまでどおり読み戻しで見る。挿入があるため木の位置合わせが要るので、今回は対象外とした（任意の追加検査）。

`dependents()` は実装の依存グラフをそのまま使うので、グラフ自体の誤り（依存の取りこぼし）は検出できない。
それでも、「剥がしを正しい場所に適用したか」と「契約外の part やセルに触れていないか」は、この oracle で独立に検査できる。
試作の結果は、スモーク 150 iters で pass、nightly `xlsx_edit` 4 並列×5 分（計 2.4 万回、cov 10,767–10,775）で失敗も timeout もなかった。

文書（`fuzz-findings-2026-11.md` の「round 2 の後の変更」）と `fuzz/README.md` の xlsx_edit の行も、この規則に合わせて書き直すこと。

### Nit

1. **`1 passed` の部分一致**: `"11 passed"` も `contains("1 passed")` に一致する。`--exact` なので実害はない。
   `"test result: ok. 1 passed"` と照合するか、`running 1 test` を確認すると厳密になる。
2. **assert の順序**: 子が abort したときに、先に「child process did not run the test」が出る（変異 (b) で確認）。
   `status.success()` を先に検査するか、`running 1 test` で「選ばれた」ことと終了コードで「通った」ことを分けて検査すると、失敗理由が正しく出る。
3. `is_sheet` の `!t.contains("sharedStrings")` は不要（`…sharedStrings+xml` は `sheet+xml` を含まない）。M2 の修正で関数ごと不要になるかもしれない。

## 判定

**converged: no** — M2（xlsx の「他 part 不変」oracle を、依存セル・calcPr・calcChain に限った差分だけを許す形に締める）が必要。
round 2 までの指摘はすべて解消済み。M2 を試作どおりに入れ、X3–X7 で再確認すれば収束。
