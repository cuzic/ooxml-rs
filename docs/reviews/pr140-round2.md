# PR #140 レビュー（Opus, round 2）

対象: impl/xlsx-phase7a-rows-cols @ b254801f3f5b5819da80593b0d1998fe7b6ee10f

## 結論（先に）
converged: **yes**。round 1 の M1 と m1〜m4、nit は、挙動と変異の両方で解消を確かめた。
m2 の修正で入った「dirty なシートの census を DOM から作る」経路は、新しい規模の大きい変更である。専用の単体テストは無い。ただし既存テストが変異を検出し、コーパスでの差分検査（15,620 組）でも挙動はバイト単位で一致した（差は直列化の表記 8 件だけ）。
CI は 5 ジョブすべて pass。現在の origin/main（4801f0a）と衝突しない。マージした状態でもコーパスの件数は変わらない。残りは nit 2 件だけである。

## 前ラウンドの解消状況

### M1 解消
テスト 3 件が追加された。round 1 と同じ変異を入れると、それぞれ対応するテストが落ちる。
- M-a（`reanchor` で translation をしない）: `deleted_anchor_hands_off_to_surviving_cf_and_x14_dv_cells` が落ちる。x14 の DV も含む。
- M-b（複数範囲の拒否を外す）: `deleted_anchor_of_multi_area_rule_refuses_atomically` が落ちる。出力の byte 一致も確かめている。
- M-c（merge の新アンカーの検査を外す）: `merge_with_a_valued_replacement_anchor_refuses_atomically` が落ちる。

### m1 解消
- `edit_grid` は `sheet_structure_guard` ではなく `structural_guard` を呼ぶ。`SharedWorkbook` と poisoned の拒否は残る。`phase3.rs` に G1 の `SharedWorkbook` テストが足された。
- テスト `workbook_structure_lock_allows_row_insertion` がある。
- ADR の実装注記も「`lockStructure` はシート操作とコピーだけ」に直った。
- コーパスの件数の変化:
  - success: 2,248 → 2,387（+139）
  - Protected: 168 → 28（−140、残りはシート保護の 7 シート × 4 操作）
  - StructureBlocked: 633 → 634（+1）
  - 解除された 140 件のうち 139 件が成功し、1 件は次の拒否理由で止まった。拒否は計 857 件で、round 1 の 140 件という見込みと一致する。
- **lockStructure のブック 4 つの新しい成功出力 138 件**を openpyxl 3.1.5 で読み戻した。
  - 原本の全セルを、期待する移動先で比べた。値は一致し、数式セル 12,257 個はすべて数式のまま。不一致は 0 件。
  - `workbookProtection lockStructure="1"` も残っていた。
  - 残りの 1 件は、正規表現で lockStructure を検出できなかったブックの出力。

### m2 解消
- 1M セルの 1 行挿入の再計測（release、各 3 回）: baseline 1.71〜1.76 s、insert 3.49〜3.61 s。比は約 2.05×（PR の 2.014× と同程度）。
- 修正の中身: `formula_census` は dirty なシートを `census::scan_parsed_sheet` で木から走査する。走査の状態機械（`SheetScan`）は、バイト列の走査と共有している。木から取る値は、デコードと正規化を済ませたもの（`RawAttr::value`、`RawText::value`）なので、ストリーム側と同じ入力になる。
- **差分検査**（プローブ、コミットしない）: コーパスの各成功操作のあと、同じシートと先頭シートに 4 種類の 2 回目の操作を行った。
  - (a) メモリ上で続けた場合（木の census）と、(b) 保存して開き直した場合（バイトの census）を比べた。
  - 15,620 組のうち 15,612 組で、結果の種類・レポート・`to_bytes()` がすべて一致した。
  - 残る 8 組はすべて `xlookup.xlsx` で、Sheet1 への 2 回目の列挿入のものだった。レポートは同じで、違いはスピルのセルの表記（`<c r="E2"></c>` と `<c r="E2"/>`）だけ。census の違いではない。
- 変異 D（木の走査でテキストを捨てる）は既存テスト 6 件で落ちる（`strip_and_rewrite_in_one_clone` など）。この経路は ADR-0009 のシート操作の rebuild でも使われる。

### m3 解消
- `remove_indices(…, true)` は `sheetData` の行だけで、間にある CDATA でない空白テキストを run に含める。
- 整形した 1M セルのシートで `delete-99999`（open + 操作 + save）: 2.54〜2.59 s。整形しないシート（2.57〜2.59 s）と同じで、二乗の増加は消えた。
- **局所性**: 整形したシートで `delete_rows(10, 1000)` を行った。
  - 1〜10 行目は（`dimension` を除いて）バイト一致。
  - 1011 行目以降は `r` 属性を除いてバイト一致。
- 変異 E（`true` → `false`）で `deleting_formatted_rows_consumes_only_intervening_plain_whitespace` が落ちる。

### m4 解消
- 挿入で丸ごと XFD を越える `col` は削除（clip）される。テストは `column_formatting_entirely_shifted_past_xfd_is_clipped`。変異 F で落ちる。
- `numeric_shift` の `ShiftOffSheet` は要素名を持つようになった（`row 1 spans 16384:16384`、`col …`、`brk …`、`autoFilter …`）。

### nit 解消
- `selection/@sqref` の 1 セルの範囲は `A2` と書かれる。変異 G で `rewrites_single_cell_selection_without_a_range_separator` が落ちる。
- 件数の違い（813 と 801、8 と 10）は、ADR の実装注記で説明された。

## コーパス
- PR の head で `OOXML_XLSX_CORPUS=/tmp/poi-xlsx` を `--ignored` 付きで実行した。件数は次のとおり。
  - success 2,387
  - StructureBlocked 634
  - MissingReference 60
  - UnrewritableReference 47
  - NotAWorksheet 40
  - Protected 28
  - IterativeCalculation / SharedWorkbook 各 12
  - FormulaScanFailed 8
  - ArrayFormulaSplit 6
  - ShiftOffSheet 5
  - PartNotEditable 4
  - ReferenceDeleted 1
  - open refused 32
- 拒否はすべて byte 一致で、テストがそれを assert している。
- **origin/main（4801f0a、core の AlternateContent 変更 #149 を含む）をマージした一時コミット**でも、`phase7_rows_cols` の全 21 テスト（コーパスを含む）が pass した。件数も同一だった。
- 数式の書き換え経路（`plan_formulas`、`rewrite`）は round 1 から変わっていない。round 1 では独立オラクルで 202,913 式を照合済みである。

## Findings

### nit
- 整形した XML で行の帯を削除すると、帯の両端の空白が残る。そのため空行が 1 つ増える（`</row>\n    \n    <row r="11"`）。実装注記に書かれた設計どおりで、無害。
- 木からの census（`scan_parsed_sheet`）とバイト列からの census が一致することを直接確かめる単体テストは無い。今は既存テストの変異の検出と、上のコーパスの差分検査で担保されている。小さな差分テストを 1 つ置くと、今後の変更に強くなる。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
