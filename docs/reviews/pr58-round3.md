# PR #58 レビュー（Opus, round 3）

対象: impl/xlsx-phase4 @ 11dc221

round 2（`879fe76`、`docs/reviews/pr58-round2.md`、converged）以降の PR 側の変更だけを見た。

- `7ccf758`、`9c3a4ae`: origin/main のマージ（#50 Phase2 registries、#67 F-8、#36/#47 fuzz）。`git show --remerge-diff` で手解決部分を確認した。
- `11dc221`: #67 round 1 M2 の対応（Phase4 の範囲操作に workbook 全体の編集拒否を適用）。
- `SheetProtection` への改名（マージ `9c3a4ae` の中）。

検証は scratchpad の clone で行った。`phase4`・`row_mismatch`・`phase2`・`allowed_diff` を実行し、すべて pass した。CI（11dc221）も green。手書きの probe を 1 本走らせたが、コミットはしていない。

## 結論（先に）
converged: yes。7 つの Phase4 公開 API は、どれも最初の処理として `check_editable()`（poisoned と `census.block`）を通る。拒否したときは、別シートの F-8 不一致を相手にしても出力が byte-identical のままになる。マージの手解決では両側の挙動が残っている。Phase2 の style-only 書き込みは merged range の中でも通り、Phase2 の値書き込み（`Shared`/`Bool`/`Error`）は non-anchor で `MergedCell` になる。`merge_clearing` の `Clear` は `sst/@count` を正しく減らす。残っているのは文書の Minor 1 件と nit 1 件だけである。

## 前ラウンドの解消状況
- round 2 は指摘なしで収束していた。その後のマージで退行は見つからなかった（下記）。

## 確認したこと
- **ゲート**: `merge`/`merge_clearing`（`plan_merge` の最初）、`unmerge`、`set_autofilter`、`remove_autofilter`、`protect_sheet`（`attributes()` より前）、`unprotect_sheet` の 7 つが、どれも先頭で `check_editable()` を呼ぶ。`edit_cell_mode` も同じ関数に統一されている。ほかに Phase4 の mutator は無い（`merged_ranges` は読み取りだけ）。`tests/row_mismatch.rs::f8_blocks_phase4_range_edits` が 7 つ全部について、`Err` を返すことと byte-identical のままであることを確かめている。
- **edit.rs の解消**: `edit_cell` → `edit_cell_ext` → `edit_cell_mode(.., extra, dry_run)` になった。`plan_registries` は `dry_run` の return より前にあり、`check_edit_cell` も registry の拒否まで見る。`StyleOnly` は merge guard より前に `edit_style_only` へ分岐するので、ADR-0014 rev 3 §3 と ADR-0006 の注記どおり style 書き込みは許される。
- **worksheet.rs**: 3 か所の `match` とも、`Clear` と Phase2 の各 op の和集合になっている。`apply` では `RemoveFormula | Clear` が同じ腕を通る。
- **refs_delta**: `Clear` は `-old_t_s` で数えられる。probe で確かめた（`62272.xlsx` の C3 に `set_string` → `merge_clearing("B2:C3")` → `count` が 2 から 1 になり、`uniqueCount` は append-only のまま。B3 の `s` は残る）。続けて non-anchor への `set_bool`/`set_error`/`set_string`/`set_number_with_format` を試すと、すべて `MergedCell` で byte-identical だった。`set_format` と anchor への `set_string` は Ok。
- **error.rs、README、manual_check、fixtures README、sml.toml**: 両側の項目がすべて残っている。sml.toml に重複は無い（`definedNames`/`mergeCells` は Phase4 の書き込み可の節へ移っている）。manual_check の `outs` は P1、P2、P4 の 3 つを連結している。
- **改名**: `lib.rs` は `registry::format::Protection`（セル書式）と `protect::SheetProtection` を両方 re-export しており、衝突しない。README、tests、manual_check、`Debug` 出力も `SheetProtection` に揃っている。

## Findings
### m1（Minor）: ADR-0014 に `SheetProtection` への改名が書かれていない
- 根拠: `docs/adr/0014-xlsx-phase4-range-features.md` L230、L594、L610、L627、L842 が `Protection` / `protect_sheet(Protection)` / `Protection::with_salt` のままで、実装注記も無い（879fe76..11dc221 で ADR-0014 は変更されていない）。README とコードは `SheetProtection` なので、ADR と食い違う。
- 修正案: ADR-0014 §11 の冒頭に「実装では `SheetProtection`（Phase2 のセル書式 `Protection` との衝突回避）」という 1 行の注記を足す。

### n1（nit）: 機械的な置換で doc コメントが不自然になっている
- 根拠: `crates/ooxml-xlsx/src/protect.rs` L111 `/// SheetProtection without a password, …`（元は英語の普通名詞 "Protection"）。
- 修正案: `/// Sheet protection without a password, …` にする。

## 他の open PR との干渉（参考）
- #68（P5-pre、CellWritePlan）と #61（Phase2 part 2）は `edit.rs`/`CellOp` に触る。`Clear` と `check_edit_cell` の dry-run 経路を両方とも残す必要がある。
- #72（wrapper の拒否）は `census.block` に理由を足すだけなら、`check_editable` 経由で Phase4 にも自動で効く。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 1）
