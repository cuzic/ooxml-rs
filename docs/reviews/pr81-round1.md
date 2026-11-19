# PR #81 レビュー（Opus, round 1）

対象: fix/review-followups @ 88dfe1a671971e1007c3b37ae198f2fa69ad91f8

## 結論（先に）
Issue #78 の 7 項目はすべて指示どおりに直っており、新しい fail-open の穴は見つからなかった。
pr66-r3 m1 の絞り込みは、残るセルの本文を書き換えない経路（結合の先頭セル・列の拡縮・縦結合の修復）に限られ、
本文が残る継続セルは `cell_is_empty` と `w:pPr` の検査で変更履歴を持てないので、変更履歴のマークアップは壊れない。
x14 のリンク例外も、例外対象が 2007 側の優先度を継承し 2007 側は検査されるので健全。converged: yes。

## 前ラウンドの解消状況
（round 1 のため無し。Issue #78 の各項目の確認を代わりに記す）
- pr77-r2 m1: `priorities()` が欠落・解析不能・0 を `None`/0 として返し、`first` で拒否（condfmt.rs:301）。負値・`"x"`・`"1.0"`・属性なしを 2007/x14 両方で Err ⇒ byte-identical を確認するテストあり（`try_on` が `assert_unchanged`）。振り直し経路は `renumber` の 1 箇所のみ。
- x14 リンク例外（condfmt.rs:180-184）: 優先度を持たない x14 規則は、2007 規則の `x14:id` から参照されているときだけ除外。参照元の 2007 規則の優先度は検査対象、明示の不正優先度を持つ x14 規則は除外されない。POI の `NewStyleConditionalFormattings.xlsx`（優先度なしの x14 dataBar）で `add_conditional_format_first` が成功する既存テストが例外を守る。`priorities` と `renumber` の走査範囲（`x14:conditionalFormattings/x14:conditionalFormatting/x14:cfRule`）も一致。
- pr77-r2 m2: validation.rs:168 で List/Custom + operator を計画段階で拒否。Err ⇒ byte-identical テストあり。
- pr66-r3 m1: `check_cell_property_revisions` は `w:tcPr` 直下の `tcPrChange`/`cellIns`/`cellDel`/`cellMerge`（`tcPr` に入り得る変更履歴の全部）を見る。結合の継続セル（k>0）は空であることが必須で、加えて `w:pPr` 内の変更履歴を拒否するため、継続セルに変更履歴が残ることはない。許すようになったケースの成功と範囲外のバイト一致はテスト済み。
- pr66-r3 m2: `delete_rows` の修復先の行の `trPr` の ins/del/trPrChange を拒否（tables.rs:551）。3 種とも Err ⇒ byte-identical テストあり。
- pr74-r2 m1: 変異プローブで確認。replace.rs:329 の `has_rpr_change` を `false` にすると `docx_replace::tests::synthetic_substrings` が落ちる。
- pr75-r2 n1: parse.rs:410 に `MAX_SPAN_OFFSET` との一致を追加。
- pr77-r1 n2: `poisoning_layout` を削除し `poisoning` を `pub(crate)` に。公開名の変更なし、生成物の変更なし。
- CI（test/clippy/codegen --check、python-pptx オラクル）は pass。open PR #82 とはファイルが重ならない。

## Findings
### m1（Minor）: 新しい拒否が rustdoc に書かれていない
- 根拠: `Worksheet::add_validation` の Refused 一覧（validation.rs:381-389）に、List/Custom + operator の `InvalidSetting` が無い。`Validation::compare` の doc（validation.rs:103）も「not List/Custom」とだけ書き、渡したときに追加が拒否されることを書いていない。ADR-0014 の note には記録済み。
- 修正案: `add_validation` の Refused 一覧に「List/Custom に比較演算子（[`Error::InvalidSetting`]）」を足し、`compare` の doc に拒否先を書く。

### n1（nit）: `check_cell_property_revisions` の書式
- 根拠: tables.rs:201 の `let revision = tc.elements()` 以降のメソッドチェーンの折り返しが rustfmt の出力と違う（周囲は `row_revision` と同じ形）。tables.rs:440 の同じ 4 要素の列挙とも重複している。
- 修正案: `row_revision` と同じ形に揃え、4 要素の配列を定数にして 440 行目と共有する。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 1）
