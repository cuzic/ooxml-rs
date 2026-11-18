# PR #66 レビュー（Opus, round 3）

対象: impl/docx-phase2-part2 @ b0463a3e8913b3f9171ec24f0ee80298f58c9a4e

## 結論（先に）
converged: yes。round 2 の M1、m1、m2 は、挙動で見てすべて直っている。新しい拒否には Err ⇒ byte-identical のテストが付いていて、手書きの変異 3 つはどれもテストで落ちる。
round 2 以降のコミットは b0463a3 の 1 つだけで、触っているのは `tables.rs`、そのテスト、ADR の note だけ。生成物と公開名は変わらない。
残りは Minor 2 件で、どちらも follow-up でよい（1 件は安全側の過剰拒否、もう 1 件は `delete_rows` の修復先の行の検査漏れで、実害は無い）。

## 前ラウンドの解消状況
- M1: 解消。`locate_table(t, true)`（`tables.rs:330-337`）が、`insert_column`、`delete_column`、`merge_cells` の前に全行の `trPr/ins|del|trPrChange` を見て `RevisionInRange` を返す。テスト `column_edits_and_merges_refuse_row_revisions` は 3 種類 × 3 操作を byte-identical 付きで確かめる。ループを無効にする変異はこのテストで落ちる。中身の無いコメントも消えた。
- m1: 解消。`delete_rows_treats_a_different_span_as_a_vertical_merge_boundary` を追加。`&& cell.span == c.span` を消す変異はこのテストで落ちる。
- m2: 解消。Widen（`:600`）、Narrow（`:758`）、merge の左端の列（`:867`）、`delete_rows` の修復先のセル（`:540`）で `check_revisions` を呼ぶ。ADR の note（`0011-docx-phase2-structure.md:952`）にも書かれた。修復先の検査を消す変異は `delete_rows_refuses_revisions_in_repaired_cell` で落ちる。
- 他の open PR で `tables.rs`、ADR-0011、`wml.rs`、`dml.rs` を触るものは無い。CI（test/clippy/codegen --check と python-pptx オラクル）は緑。

## Findings

### m1（Minor）: 残るセルの検査が本文の変更履歴まで見るので、変更履歴付きの文字があるだけで拒否される
- 根拠: `check_revisions`（`tables.rs:192`）はセルの子孫をすべて探す。そのため、左上のセルの本文に `<w:ins><w:r>…</w:r></w:ins>` があるだけで、`merge_cells(t,0..1,0..2)` は `RevisionInRange("w:ins")` になる。gridSpan 2 のセルに同じ本文があると、`insert_column(t,1,500)` も同じく拒否される（プローブで確認）。
  書き換えるのは `w:tcPr` だけで、本文は変わらない。安全側の過剰拒否だが、レビュー中の文書ではよくある形なので、実用上の制約が大きい。
- 修正案: 残るセルについては `w:tcPr` の直下（`tcPrChange`、`cellIns`、`cellDel`、`cellMerge`）だけを見る。merge の continue になるセル（k>0）は、本文が空であることを `cell_is_empty` で確かめているので、そこには段落記号の `rPr` の検査を足せば足りる。

### m2（Minor）: `delete_rows` の修復先の行の `trPr` の変更履歴を見ていない
- 根拠: restart の行と、`<w:trPr><w:del/></w:trPr>` を持つ continue の行がある表で `delete_rows(t,0..1)` を呼ぶ。結果は `Ok` で、削除が履歴に残っている行のセルに `vMerge="restart"` が書かれる。
  `:540` はセルの中しか見ていない。削除を却下しても承認しても、修復しない場合より悪くはならないので、実害は無い。ただ、§7 の「any target row inside a revision」や、M1 で列の操作に入れた行単位の拒否と揃っていない。
- 修正案: `:537` の分岐で `row_revision(tr)` も確かめて拒否し、テストを 1 つ足す。拒否しないのであれば、そのことを ADR の note に書く。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 0）
