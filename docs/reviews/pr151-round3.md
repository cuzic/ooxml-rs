# PR #151 レビュー（Opus, round 3）

対象: impl/core-perf-stage2 @ a120cac7da8370a566555d990496454c561dc749

## 結論（先に）
round 2 の M1 は挙動で解消を確認した。`parse.rs` の built 要素の宣言による snapshot 無効化を外すと、新しい回帰テストと、oracle の名前 1 つずつの defer（seed 再生）の両方が落ちる。
9b54712 以降のコミットは、round 2 のレビューファイルと、テストだけの修正 a120cac だけである。
**converged: yes**（Blocker 0 / Major 0）。

## 前ラウンドの解消状況
- M1: 解消。
  - `parse.rs` 517–519 行（`if self.scope.bindings.len() != marks { self.snapshot = None; }`）をコメントアウトし、`cargo test -p ooxml-core --release --no-fail-fast` を実行した。次の 2 件が落ち、他は通った。
    - `built_element_redeclaring_a_prefix_invalidates_the_shared_snapshot`（`tests/deferred.rs`）
    - `deferred_parser_matches_on_xml_fuzz_seeds`。失敗メッセージは `built-redeclare-between-deferred.xml: only [row] deferred: element name` で、`u2` が `u1` になっている。落ちたのは名前 1 つずつの経路である。
  - 変異を戻すと、全件 pass する。

## 確認できたこと（プローブ。コミットしない）
- コミット範囲: `9b54712..a120cac` は `0301949`（round 2 のレビューファイル）と `a120cac` だけである。a120cac が触るのは `xml/deferred_oracle.rs`（`#[cfg(test)]` の oracle）、`tests/deferred.rs`、seed 1 件だけで、本体のコードは変わっていない。
- 名前 1 つずつの経路が実際に走るかを確かめた。`compare_single_names` の呼び出しに一時的な `eprintln!` を入れて数えた。
  - fixture: oracle に入った 2435 part のうち 1272 part で走った。docx 154、pptx 723、xlsx 388、その他 7 である。
  - rels と `[Content_Types].xml` が多い。ただし `xl/worksheets/sheet*.xml`、`xl/workbook.xml`、`docProps/core.xml`、`ppt/presProps.xml` など、名前が 4〜6 種類の part も 100 件ほど含まれる。「6 種類以下」の絞り込みは空ではない。
  - seed: 整形式の 3 件すべてで走った（`alternate-content`、`namespaces`、`built-redeclare-between-deferred`）。残りの 2 件は壊れた入力で、エラーの一致を確かめる経路を通る。
  - 各名前について、次を比べている。T1、census、`compare_tree`（名前空間 URI を含む）、全 release 後の T1、全 release 後の `compare_tree`。黙って pass する形にはなっていない。
- CI: 実行中（docx、pptx、xlsx の read-back は pass）。ローカルの `cargo test -p ooxml-core --release` は全件 pass した。
- 他の open PR（#144）は core の xml に触れていない。

## Findings
なし。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 0）
