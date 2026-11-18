# PR #36 レビュー（Opus, round 4）

対象: `test/fuzz-core` @bf5c2ab（round 3 からの差分は 52ed49f〈origin/main マージ〉と bf5c2ab）。
検証: 独立した `git clone`（head）と、#65 の clone（#36 の oracle をそのまま含む）。Rust 1.99.1、nightly 1.102.0（5f4effd57）、cargo-fuzz 0.13.2。

## 結論（先に）

**M2 は解消した。** `src/phase1_oracle.rs` は round 3 の試作を、`Phase1Edits`（`before_write` / `wrote`）と `assert_phase1_diff` に整理したものである。
数式の集合が変わるケース（`set_formula`）にも一般化してある。
round 3 の変異 7 つ（X1–X7）を同じ形で作り直したところ、**7 つすべて**がスモーク（`OOXML_SMOKE_ITERS=30`）で検出された。
X7（無関係なシートの先頭セルの `v` を消す）も検出された。誤検出もない（スモーク 150 iters、nightly `xlsx_edit` 4 並列×5 分で計 6.4 万回）。
Nit 3 件も直っている。

新しく、誤検出の経路を 1 つ見つけた（m12、Minor）。
`dependents()` が返すシート名を、名前でシートに引き戻している。そのため、**完全に同じ名前のシートが 2 つ**あるブックで誤検出する。
workbook.xml の 1 バイトの変化で作れる形である。週次の CI が、ライブラリの誤りではない理由で赤になりうる。

- Minor 1 件（m12）、Nit 3 件
- 判定: **converged: yes**（Blocker・Major なし。m12 はマージ前の対応を推奨）

## 解消状況

| ID | 状態 | 確認内容 |
|---|---|---|
| M2 | 済 | 書き込みの直前に `dependents` と数式の有無を記録する。変わった part ごとに、依存セルの `v`・`is`・`@t`、`calcPr/@fullCalcOnLoad="1"`、calcChain の `Relationship`・`Override` の除去だけを許し、それ以外は L1 一致を要求する。calcChain は workbook の関係から解決し、「集合が変わった ⇔ 消えた」を両方向で assert している。`Rule::Deps` は a 側の `v`・`is` だけを落として b 側と比べるので、依存セルを持つシートが変わった場合は、剥がしが実際に行われたことも要求される |
| Nit 1・2（`1 passed`、assert の順序） | 済 | `running 1 test` → `status.success()` → `test result: ok. 1 passed` の順に assert している |
| Nit 3（`is_sharedStrings`） | 済 | `is_sheet` ごと削除 |
| 文書 | 済 | `fuzz-findings-2026-11.md` の「round 2 の後の変更」と `fuzz/README.md` の xlsx_edit の行が、新しい規則と一致している |

### 変異の再実行（ooxml-xlsx `edit.rs` に手で入れ、スモーク `OOXML_SMOKE_ITERS=30`、release）

round 3 と同じ sed と同じスクリプトを使った（`edit.rs` は round 3 から変わっていない）。

| # | 変異 | round 3 の oracle | round 4（bf5c2ab） |
|---|---|---|---|
| X1 | 依存セルのある行の全セル（定数も）を剥がす | 検出 | 検出（`(2,8) after reopen`） |
| X2 | 剥がしを別のワークシートに適用する | 検出（偶然） | 検出（`sheet2.xml changed beyond what Phase1 may change`） |
| X3 | `calcPr` に `iterate="1"` も書く | 生存 | **検出**（`workbook.xml changed beyond …`） |
| X4 | 集合が変わっても calcChain を消さない | 生存 | **検出**（`the formula-cell set changed but calcChain.xml stayed`） |
| X5 | 依存セルの `f` も消す | 生存 | **検出**（`sheet2.xml changed beyond …`） |
| X6 | 集合が変わらなくても calcChain を消す | 生存 | **検出**（`calcChain.xml removed although …`） |
| X7 | 無関係なシートの先頭セルの `v` を消す | 生存 | **検出**（`sheet2.xml changed beyond …`） |

参考（#65 の clone で、`xlsx_edit` に対して 150 iters で実行）:

- 依存セルをまったく剥がさない変異と、配列の結果を剥がさない変異は、`xlsx_edit` では**生存**した。
  剥がしがないと part が変わらず、raw が一致した part は比較を飛ばすためである。
- 編集したシート自体の先頭セルの `v` を消す変異は、`(0,0) after reopen` で検出された。
- 剥がしが足りない変異は、積み上げの #65（`xlsx_formula_edit`）がモデルの依存先で検出する（そちらでは 2 つとも検出された）。
  #36 単独の範囲としては許容できる。ただし、文書に「剥がし不足は #65 の範囲」と一言あるとよい（Nit 2）。

### 誤検出の確認

| 実行 | 結果 |
|---|---|
| スモーク `xlsx_edit`、`OOXML_SMOKE_ITERS=150`（release） | pass（8 秒） |
| nightly `xlsx_edit`、seed 563 件（全量）、4 並列×300 秒、`-timeout=10` | 計 6.4 万回、cov 11,156–11,160、失敗も timeout もなし |

## 所見

### m12 [Minor] 同名のシートが 2 つあると、`phase1_oracle` が依存セルを別のシートに割り当てて誤検出する

**場所**: `crates/ooxml-robustness/src/phase1_oracle.rs` 82–87 行（`dep_parts` の組み立て）、`before_write` の `wb.dependents(sheet, …)`

`Workbook::dependents` は `(シート名, セル)` を返す。oracle はこれを `worksheet_by_name(s)` で part に引き戻す。
`sheet_index` は完全一致する最初のシートを返すので、`<sheet name="Sheet1">` が 2 つあると、2 枚目の依存セルも 1 枚目の part に割り当てられる。
一方ライブラリは、`Sheet1!` 参照を曖昧（fold が一致）と判断して `Everything` にし、2 枚目の依存セルも正しく剥がす。
結果として、2 枚目の part の変化が「契約外」と判定される。

**再現**（#65 の `formula_books()[0]` の workbook.xml で `name="Sheet2"` を `name="Sheet1"` に置き換え、ランダムな ops 3,000 本で実行）:

- `check_xlsx`: 2,985/3,000 が `xl/worksheets/sheet2.xml changed beyond what Phase1 may change` で panic
- `check_xlsx_formula`（#65）: 2,750/3,000 が同じ理由で panic
- 大文字小文字だけが違う名前（`SHEET1`）は 0 件。完全一致が別のシートを指すため

ライブラリの出力は正しい。1 枚目の B2 を編集すると、2 枚目の `A1`（`Sheet1!B2+1`）と `B1` の `v` が消える。曖昧な参照なので `Everything` に倒すのが正しい。
fuzz 入力では、workbook.xml を payload にして `Sheet2` の 1 バイトを `1` に変えるだけでこの形になる。今回の nightly では出なかったが、週次ジョブが偽の失敗で赤になる経路である。

**修正案**（どちらか）:

- (a) `assert_phase1_diff`（と #65 の `check_xlsx_formula`）の入口で、`sheet_names()` に完全一致の重複があれば Phase1 の差分検査を飛ばす。読み戻しなど、他の検査は残す。
  名前でシートを引けないブックでは、`dependents()` の結果を part に対応づけられないためである。
- (b) ライブラリ側で、重複したシート名（Excel は開かない）を `edit_refusal` にする。これは ooxml-xlsx の担当者への提案とし、ハーネスは (a) で先に対応する。

### Nit

1. `fuzz-findings-2026-11.md` の「round 2 の後の変更」に、検出を確認した変異として X7（無関係なシートを壊す）も挙げるとよい。M2 の直接の動機だった変異である。
2. 同じ節に、「依存セルの剥がし**不足**は、part が変わらない限りこの oracle では見えない。#65 のモデルで検査する」と書いておくとよい。

3. **`f7_open_is_not_quadratic_in_the_entry_count` が高負荷で 1 回落ちた**（#65 の clone で `cargo test -p ooxml-robustness` を実行したとき）。
   条件は、load avg 44 / 32 コア、nightly 6 並列、同じ crate のスモークとの並行実行。単独で 3 回再実行すると、すべて pass した（0.12 秒）。
   doc コメントは「4 倍のエントリで 16 倍よりずっと小さい」と書いているが、assert は 8 倍である。
   線形なら 4 倍、二次なら 16 倍なので、閾値を 10〜12 倍にすると偽の失敗が減る。検出力は落ちない。
   あわせて、コメントと閾値を揃えること。

## 判定

**converged: yes** — Blocker・Major なし。M2 は変異 7 つすべての検出で確認した。
m12（同名シートでの誤検出）は、週次 CI の偽の失敗を避けるため、マージ前に (a) の数行で対応することを推奨する。
