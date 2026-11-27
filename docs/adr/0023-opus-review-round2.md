# ADR-0023 レビュー（Opus, round 2）

対象: `docs/adr/0023-xlsx-range-io-and-python-binding.md` revision 2。ブランチ plan/xlsx-rpa-foundation @ `e849a1821acf28cf9e45fb8ca0f3b70454a8721a`（main `213e27a` = PR #170 マージ後を含む）。

照合したもの:

- この head の `crates/ooxml-xlsx/src/{worksheet.rs, workbook.rs, setters.rs, edit.rs, sheet_ops/mod.rs, streaming.rs, streaming_rows.rs, registry/format.rs}`、`crates/ooxml-core/src/package.rs`、`crates/ooxml-xlsx-wasm/src/lib.rs`、`Cargo.toml`。
- `docs/adr/0024-xlsx-edit-journal.md`（Accepted、revision 7。§10.3、§13.4、Q4、A.5、Context の (c)）。
- round 1 の記録（`0023-opus-review-round1.md`）。

## 結論（先に）

revision 2 は、round 1 の M1–M9 と m1–m13 に、それぞれの節で答えている。Context 表の Rust API 名は、#170 の着地で current main にすべて実在する。`Send`/`Sync` の主張もコンパイルで再確認した。

しかし、**ADR-0024（Accepted、main にある Edit Journal）を参照していない**。ADR-0024 は owner 決定 Q4 で「ADR-0023 は ADR-0024 が Accepted になった後、round 2 の前に Edit Journal を基盤として書き直す」と定め、§13.4 で次の 3 点を置き換え対象に指定している:

- §6.2/§6.3 のバッチ → journal op
- `write_changed_cells` → R1/R2
- §11.2 の `fork()` によるトランザクション → `truncate(seq)`

revision 2 はこの書き直しをしていない。round 1 の M1（トランザクション）、M3（グループ単位の clear）、M4（`write_changed_cells`）への修正は、まさにその置き換え対象の上に作られている。

**converged: no**（Major 1、Minor 4）。

## 前ラウンドの解消状況

- **M1（トランザクションの分離）:** 本文上は対応済み（owner スレッド、condvar、GIL を外して待つ）。`Workbook::fork` は main にあり、新しいフラグもコピーしている。ただし ADR-0024 §10.3 と §13.4 は、この仕組みを `truncate(seq)` に置き換えると定めている（新 M1 を参照）。
- **M2（`(crate, kind)` キー）:** 対応済み（§8、§11.3）。
- **M3（グループ全体の clear）:** 本文上は対応済み。ただし後で述べるとおり、main の現状との整合が要る（m1）。
- **M4（`None` = 変更なし、`CLEAR`、`write_changed_cells`）:** 本文上は対応済み。ただし「reads equal」は ADR-0024 の R1 と A.5 と重なる（新 M1）。
- **M5:** 対応済み（§5.1、§9）。§9 が名指しする Wasm 側の関数は、`crates/ooxml-xlsx-wasm/src/lib.rs` にすべて実在する: `classify_sheet` L5129、`classify_cell` L5136、`classify_scalar_value` L5418、`classified_value` L5407、`check_excel_edit` L5388、`scalar_cell_write` L5709、`scalar_read_ranges` L5102。
- **M6、M7、M8:** 対応済み（§5.1、§7、§5.2/§6.1）。`is_date_format` は `pub(crate)` で、`DateKind` の土台として使える。
- **M9:** 対応済み。`bindings/python/` は root `members = ["crates/*", "xtask"]` の外にあり、`exclude` の追加でも root の 3 コマンドは変わらない。
- **m1–m13:** 本文上はすべて対応済み。m7 の `Workbook::worksheet_ref` は未実装の予定 API で、名前の衝突は無い。

### 検証したこと

- **`Send`/`Sync`（この head でコンパイル）:** 次の assert がすべて通った: `Workbook: Send`、`Mutex<Workbook>: Send + Sync`、`Document`/`Presentation`/`ooxml_xlsx::Error`/`ooxml_core::Error: Send + Sync`。`Workbook: Sync` は ``OnceCell<ooxml_xlsx::graph::Dependents>` cannot be shared between threads safely`` で失敗した。Context 表のとおりである。
- **Context 表の API（この head に実在）:**
  - `pub` で実在: `write_cells`、`write_cells_with_styles`、`set_datetime`、`set_iso_date`、`declared_range`、`declared_bound`、`has_complex_value`、`has_non_scalar_content`、`ordinary_formula`、`shared_string_index`、`non_scalar_ranges`、`formula_result_ranges`、`array_formulas`、`shared_formula_at`、`is_date_style`、`row_numbers`、`cells_before_row`、`defined_name_nodes`、`global_defined_names`、`global_defined_name_formulas`、`set_(global_)defined_name(s)`、`worksheet_by_name`、`worksheet_by_index`、`sheet_names`、`worksheet_names`、`worksheet_identities`、`Workbook::fork`、`edit_refusal`、`Package::fork`、`Package::generation`。
  - `clear_shared_formula_row` は Worksheet 上で `pub`。
  - `stored_extent` は非公開の関数（U3 の内部で使う前提と矛盾しない）。
- **予定 API 名の衝突:** `SheetRef`、`worksheet_ref`、`UsedRange`、`used_range`、`first_free_row`、`ReadValue`、`CellRole`、`NameScope`、`DateKind`、`write_changed_cells`、`clear_range`、`RangeRead`、`ScalarOptions`、`NonScalarCell`、`AmbiguousName` などはどれも未使用。例外は `ReadCell` で、m2 に書く。

## Findings

### M1（Major）: Accepted の ADR-0024 と owner 決定 Q4 に反して、Edit Journal を基盤に書き直されていない。トランザクションと skip-unchanged が二重に作られる

- **該当箇所:** §6.2、§6.3、§11.2、§5（読む対象）、Prerequisite and sequencing、Delivery（U4b、U8）、Depends on。
- **根拠:**
  - ADR-0024（Status: Accepted, revision 7）の Context の表 (c) は、「`Workbook::fork()` per transaction (ADR-0023 §11.2)」を、各消費者がばらばらに持つ snapshot（bolted-on atomicity）の例として挙げている。
  - §10.3 は「This is how ADR-0023-style transactions (`with wb.transaction():`) roll back, without a snapshot of the parsed parts」と書く。
  - §13.4 は、ADR-0023 の §6.2/§6.3 を journal op のバッチに、`write_changed_cells` を §9.2（R1, R2）に、§11.2 を `truncate(seq)` に置き換え、`read_range` は View を normalized モードで読むと定めている。
  - Q4 は「Decided: rewrite after this ADR is accepted … so its transactions and batches are not built twice」。
  - ADR-0023 revision 2 には「0024」「Journal」「truncate」が一度も出てこない（grep）。Depends on にも無い。
  - 結果として、R1 と A.5 の「reads equal」（存在するセルだけ、group/array セルは skip しない、§4.4 の状態の等しさ）と、§6.2 の「reads equal」（型付きの読み取り値の比較、-0→0、String と Text の rich_text: false）という、定義の違う 2 つの no-op 判定が同じ crate にできる。
  - Python の save は ADR-0024 Q5 (A)（トランザクション中の save を拒否）にも従っていない。
- **なぜ重要か:** owner の決定と、Accepted な設計への違反である。U4b、U7、U8 をこのまま実装すると、ADR-0024 の J 系列と同じ `edit.rs` の計画経路（`plan_cell_writes`、clearing group）を、別の意味論で二重に作ることになる。CLAUDE.md の取り決めでは ADR-0024 の実装は Claude Sonnet が担当するので、担当の分担も衝突する。
- **修正案:** ADR-0024 §13.4 に沿って次を改稿する:
  - §6.2/§6.3 は journal op のバッチにする。
  - `write_changed_cells` は R1/R2 に寄せるか、同じ等価関数を共有するか、どちらかを決める。
  - §11.2 は `truncate(seq)` にし、Q5 (A) を採用する。
  - §5 は View を normalized モードで読む。

  Depends on に ADR-0024 を加え、Delivery を J 系列（J3/J4）の後に並べ直す。§9 と §10 は §13.4 のとおり影響を受けないので、そのまま残してよい。

### m1（Minor）: §6.3 が、#170 で main に入った shared グループの全置換経路（`replaced_groups`）と、clearing group が単数であることを考慮していない

- **根拠:** main の `edit.rs::plan_cell_writes` には 2 つの仕組みがある:
  - `CellOp::SharedFormula` の書き込みがグループの全セルを覆うと、hand-off を省く `replaced_groups`（#170）。
  - `Option<ClearingFormulaGroup>`（1 プランにつき 1 グループ）。

  §6.3 は後者だけを「一般化する」と書いている。計算列が複数並ぶ範囲を `clear_range` すると、1 プランに複数のグループが入る。
- **修正案:** M1 の改稿で journal op に移るとしても、「1 プランに複数のグループを許す」ことと、`replaced_groups` との統合を明記する。

### m2（Minor）: 予定している公開型 `ReadCell` が、既存の公開型 `ooxml_xlsx::streaming::ReadCell`（#170）と同じ名前で、意味が違う

- **根拠:** `streaming_rows.rs` の `pub struct ReadCell { column, kind, style, value, rich_text, formula, shared_formula, formula_range }` が、`streaming` モジュールから公開されている。§5.1 の `ReadCell` は crate root の別の型である。ADR は streaming 読み取り（`WorksheetRows`）にも触れておらず、「並行 API を足さない」という原則との関係が書かれていない。
- **修正案:** `RangeCell` などに改名するか、streaming 読み取りとの役割分担を Context に書く。

### m3（Minor）: `CellWrite::ClearValue`（ADR-0024 P1、main にある）が、`CLEAR`、`clear()`、`clear_range` の割り当てに出てこない

- **根拠:** main の `CellWrite` は `Number`、`String`、`IsoDate`、`Error`、`Bool`、`Clear`、`Formula`、`FormulaWithResult`、`RemoveFormula`、`ClearValue` の 10 個である（`worksheet.rs:834-856`）。`ClearValue` は存在しないセルを作らない。§11.4 の `CLEAR` → `Clear` は、空いた座標に `<c>` を作りうる（§6.2 の drop で隠れるだけである）。
- **修正案:** `CLEAR` と `clear()` を `ClearValue` に割り当てるか、`Clear` を選ぶ理由を書く。

### m4（Minor）: Context と Prerequisite が古い

- **根拠:**
  - 「Repository baseline: `f88d02c`」が残っている。
  - Context の表の列名「Pending ADR-0021/0022 work」と「not yet on `main`」が残っている。
  - Sequencing の手順 1 は、#170（`213e27a`）のマージですでに完了している。
- **修正案:** baseline を current main にし、表を「on main (PR #170)」に直し、Sequencing の 1 を完了と記録する。U1–U9 の分割そのものは有効である（M1 の改稿で U4b と U8 の中身が変わる）。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 4 / nit 0）
