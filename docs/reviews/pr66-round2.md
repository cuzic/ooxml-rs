# PR #66 レビュー（Opus, round 2）

対象: impl/docx-phase2-part2 @ 0ab51894c5daae9ec5ae47718d545e5e0dc4f676（base main。#62 と origin/main をマージ済み）

## 結論（先に）
converged: no。round 1 の M1〜M3 と m1〜m3 は、挙動で見てすべて直っている。
マージの解決（`dml.toml`、`wml.toml`、生成物、`structure.rs`）にも問題はない。
ただし、新しく Major を 1 件見つけた。行単位の変更履歴（`w:trPr/w:ins|w:del`）を持つ表で、
`insert_column`、`delete_column`、`merge_cells` が `Ok` になる。ADR-0011 §7 はこれを拒否すると定めている（round 1 の見落とし）。

## 前ラウンドの解消状況
- M1: 解消。4 行（A restart / continue / B restart / continue）の表で `1..3` を消すと `restart, restart` になる。3 つの結合のうち 2 つの restart を消す場合と、gridBefore がある行の場合もプローブで正しかった。回帰テストは `delete_rows_preserves_vertical_merge_boundaries`。
- M2: 解消。消えるセルに `cellIns`、`tcPrChange`、`pPr/rPr/ins`、`pPrChange` があると、`merge_cells` と `delete_column` は `RevisionInRange` を返し、出力は byte-identical のまま。注記、フィールド、コメントも同じく拒否する。入れ子の表の中の脚注参照も `WouldRemoveNote` になる。`j > 0` のガードを外す変異はテストで落ちる。
- M3: 解消。19 px/m の PNG は 96 DPI として扱われる。extent が `MAX`（27273042316900）なら通り、`MAX+1` と 0 は `InvalidValue` で拒否される。Auto でも `MAX/914400` px を境に、通る側と拒否される側の両方をテストしている。
- m1: 解消。`horizontal_merge_refuses_vertical_merge_crossing_bottom` を追加。
- m2: 解消。dxa の tcW は、span の gridCol がすべて数値のときだけ加減する。auto/pct の tcW、幅の無い gridCol、引き算の下回り（300−1000）では tcW を変えない。ADR の note にも書かれた。tcW を直す処理を外す変異はテストで落ちる。
- m3: 解消。#62 と #63 はマージ済みで、open な PR のうち `dml.toml`/`wml.toml`/`dml.rs`/`wml.rs` を触るものは無い（#77 が触るのは `sml` だけ）。
  `git show --remerge-diff 4e76ca4` を見ると、本 PR 側の `a:CT_Point2D/a:off` と `a:CT_PositiveSize2D/a:ext` は main の write エントリーと重なるので落とされている。read に残っていた `a:xfrm`、`a:prstGeom`、`a:graphic`、`a:graphicData` は write へ移された。
  0ab5189 で `[detach]` の 3 項目（`a:tcPr`、`a:bodyPr`、`a:lstStyle`）が戻り、`[detach]` は origin/main と同じ内容になった。
  生成物 `dml.rs` と `wml.rs` について、origin/main の pub な型名、enum の variant、フィールド名、定数を抜き出して比べた。消えた名前も改名された名前も 0 件で、追加だけ（dml +125、wml +327）。`codegen --check` は CI で緑。
  `report.txt` の 42c087f の衝突解決（pic と vt の行の並び）も正しい。
  `structure.rs` の main との差分は、`para_location`、`check_structure_neighbour`、`para_lens` を `pub(crate)` にしたことと、`table_lens` を足したことだけで、#62 のコードとはぶつからない。

## Findings

### M1（Major）: 行単位の変更履歴がある表でも、列の操作と結合が Ok になる
- 根拠: ADR-0011 §7（`docs/adr/0011-docx-phase2-structure.md:519-520`）は「row-level revisions (`trPr/ins|del`) … are refused」と定めている。
  ところが `locate_table`（`tables.rs:318`）には「a table inside a revision is refused」というコメントがあるだけで、それを確かめるコードが無い。
  `trPr` の ins/del/trPrChange を見ているのは `insert_rows` のテンプレート行（`:383`）と、`delete_rows` の削除対象行（`check_removable`）だけだった。
  プローブ: 2×2 の表で 1 行目に `<w:trPr><w:del w:id="1" w:author="a"/></w:trPr>` を置く。
  `insert_column(t,1,500)`、`delete_column(t,1)`、`merge_cells(t,0..2,0..1)` はどれも `Ok` を返し、出力が変わる。
  merge では、削除が履歴に残っている行に `vMerge="restart"` が付き、その下の通常の行が continue になる。
  削除を承認すると restart の行が消え、continue だけが残る。`trPr/ins` の行でも同じく通る。
  変更履歴のある行の構造を、履歴に残さずに変えることになるので、§7 の M6 と同じ理由で「黙って承認または却下」にあたる。
- 修正案: 列の操作と merge では、対象の全行（merge では rectangle 内の行）の `trPr` に `ins`、`del`、`trPrChange` があれば、変更前に拒否する（§7 に合わせて `UnsupportedContext` か `RevisionInRange`）。
  `insert_rows` と `delete_rows` でも、挿入位置の隣の行や結合を直す行について同じ条件を見るか確認する。中身の無いコメント（`:318`）は消すか実装する。
  3 つの操作それぞれに、Err ⇒ byte-identical のテストを付ける。

### m1（Minor）: `delete_rows` の境界判定で、span の一致を見る部分にテストが無い
- 根拠: `tables.rs:518` の `&& cell.span == c.span` を消す変異が、tables/images の全テストを通ってしまう。
  今の実装では、span が違うセルで鎖が切れると修復しない。例: A restart（span 1）、span 2 の restart、span 1 の continue の 3 行で、`1..2` を消す。
  結果は `Ok` で、残った continue が A にくっつく。元から不整合な入力なので、挙動の良し悪しは判断できない。
- 修正案: 上の例の期待値を決めて（修復して restart にするのが安全側）、テストを足す。

### m2（Minor）: 消えないセルの変更履歴は残ったまま、構造だけが変わる
- 根拠: `tcPrChange` を持つ span 2 のセルに `delete_column` を適用すると `Ok` になり、gridSpan だけが黙って消える。
  `merge_cells(t,0..2,0..1)` では、下のセルの `cellIns` や `pPr/rPr/ins` がそのまま continue セルに残る。
  消える範囲ではないので、M6 の文言には反していない。ただ、`tcPr` を変更履歴に残さずに書き換えることになる。
- 修正案: Narrow/Widen の対象セルや、merge の左端の列のセルに `tcPrChange`、`cellIns`、`cellDel`、`cellMerge` があれば拒否する。拒否しないなら、その旨を ADR の note に明記する。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 2 / nit 0）
