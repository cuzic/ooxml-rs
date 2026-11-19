# PR #72 レビュー（Opus, round 3）

対象: fix/xlsx-ignorable-wrappers @ 1cc5da9d3f6e4d3efb86d6a6fbeb1abfbed00da5

## 結論（先に）
converged: yes。round 2 の M1・m1 はどちらも解消した。`contains_wrapped_grid` が `mc:AlternateContent` を飛ばすのは親が `sheetData`・`row` のときだけになり、round 2 のプローブ 3 件と重複 `sheetData` はすべて `UnresolvedSibling(sheetData)` かつバイト一致で拒否される。過剰拒否・回帰・公開名の変更は見つからなかった。

## 前ラウンドの解消状況
- M1: 解消。`crates/ooxml-xlsx/src/index.rs:96` の `skip_ac`（親が `sheetData` か `row`）。外部ラッパー内の AC にある `row`・`c`・`sheetData`、`worksheet` 直下の AC にある `sheetData` の 4 入力が `tests/ignorable_wrappers.rs` の `grid_nodes_in_alternate_content_outside_indexed_parents_refuse_edits` で拒否・バイト一致。`worksheet` 直下の controls 用 AC は編集可能のまま（`alternate_content_with_controls_stays_editable`）。
- m1: 解消。`index.rs:146-148` で `worksheet` 直下の 2 つ目の `sheetData` を `wrapped_grid` にする。`duplicate_sheet_data_refuses_cell_edits` で拒否・バイト一致。

## 検証の範囲
- `cargo test -p ooxml-xlsx --all-features`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo xtask codegen --check` を手元で通過。
- 手書き変異 4 件はすべてテストで落ちた: `skip_ac = true`（M1 テストが失敗）、重複 `sheetData` 検査の削除（m1 テストが失敗）、`skip_ac` から `Row` を外す（`row_index.rs` の `stripping_ignores_alternate_content_the_target_refuses_it` が失敗）、`skip_ac = false`（同）。
- 追加プローブ: `row` 直下の AC に `row r="2"` を入れて A2 を書く → `Err(InvalidReference)`、AC 内の外部ラッパーに `c r="B1"` を入れて B1 を書く → `Err(UnresolvedSibling(row))`。どちらもバイト一致。
- 既存の AC 拒否のエラー値は変わらない（`target_rows` は `alternate` を先に見る。`row_index.rs`・`row_mismatch.rs`・`refusals.rs` の AC ケースは通過）。
- 公開名: 生成物・`ooxml-core`・`codegen/` の差分なし、新 variant なし。変更は `index.rs` の private 関数と `Worksheet` の doc、テストのみ。
- 他の open PR は #23（spike）のみで干渉なし。

## Findings
なし。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 0）
