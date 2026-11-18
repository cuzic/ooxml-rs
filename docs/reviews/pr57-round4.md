# PR #57 レビュー（Opus, round 4）

対象: impl/pptx-phase1b-tables @ 2ceb45a

範囲: round 3（0a6f72f）以降の修正 2ceb45a だけ（`git diff 0a6f72f 2ceb45a`）。CI は 2ceb45a で green。テストの再実行はしていない。

## 結論（先に）
M1 は解消した。`ooxml_pptx::Color` は再び `resolve::Color<'a>`（#56 の resolver 型）を指し、`pub use resolve::{…}` ブロックは main（origin/main）と完全に一致する。
PR 側の書き込み用 enum は `TableColor` に一貫して改名され、write API の文書・doc コメント・テスト・example に古い `Color` は残っていない。
m1 も PR 本文で直っている。converged: yes。

## 前ラウンドの解消状況
- M1（`Color` の付け替え）: 解消。`lib.rs:50` の resolve 再公開は main と diff なし、`ResolvedColor` はコード・文書のどこにも残らない（レビュー記録を除く）。`lib.rs:61` は `write::TableColor` を公開。
- m1（manual-check 番号）: 解消。PR 本文 84 行が「Manual-check items 23–25」。

## 確認したこと（問題なし）
- 改名の範囲: `write/table.rs` の enum・`Line::Solid.color`・`CellFill::Solid`・`check_color`・`write_color`、`write.rs` の再公開、`tests/tables.rs`、`examples/manual_check.rs` / `table_cases.rs`。
  `grep -w Color` で `src/write*`・ADR-0013 に `TableColor`/`LinkColor`/生成型以外のヒットなし。
- 文書: README に `TableMut::set_fill` / `set_border` と `TableColor` の行を追加、ADR-0013 に post-acceptance note（`TableColor` は resolver の `Color` との衝突回避）と §3 の fill/border 記述を `TableColor` で書き直し。決定の変更なし。
- 範囲外の変更なし: diff は上記 8 ファイル + round 3 のレビュー記録のみ。挙動は改名だけで不変（`check_color` / `write_color` の分岐は同一）。

## Findings
なし。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 0）
