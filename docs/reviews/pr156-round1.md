# PR #156 レビュー（Opus, round 1）

対象: impl/xlsx-ej-c2 @ 49cc875e8d9bf16aca279b3c8fd9e284d8ba9b92

## 結論（先に）
converged: **no**。core の dirty-child count 自体は全変更経路（`insert/remove/splice/retain/set_children`、
`mark_path_dirty`、`move_subtree`、`package/add`、deferred 展開・`release_children`）で整合しており、
docx/pptx の既定出力に影響する変更（フラグの立ち方）は見つからなかった。しかし `restore_cell` の
祖先クリーニングは「dirty な子がいない」ことしか見ておらず、**子要素の削除**を検出できないため、
cell の write → `unmerge`/`unprotect_sheet` → restore で削除が黙って元に戻る（B1）。
また session が作った row が restore 順序によって残る（M1、§4.4 の history independence 違反）。

## 前ラウンドの解消状況
round 1 のため無し。

## Findings

### B1（Blocker）: 祖先クリーニングが子要素の削除を見ず、バリア op の削除を黙って取り消す
- 根拠: `crates/ooxml-core/src/xml/mod.rs:202-213`（`clean_up` は `dirty_child_count() == 0` と
  `may_clean` だけで `clear_dirty`）、`crates/ooxml-xlsx/src/snapshot.rs:51`（`allows_clean` は
  snapshot 時の own/subtree フラグと属性しか比べず、子リストを比べない）。clean になった祖先は
  `src[span]` から raw copy されるので、削除された子が出力に戻る。プローブ（snapshot.rs の tests に一時追加）:
  `<sheetData>…</sheetData><sheetProtection sheet="1"/>` で `snapshot_cell(A1)` → `A1 = 7` →
  `unprotect_sheet(0)`（中間出力に `sheetProtection` 無し）→ `restore_cell(A1, Snapshot)` →
  出力に `<sheetProtection sheet="1"/>` が復活、part は clean。`<mergeCells count="1"><mergeCell ref="C3:D3"/>`
  で `unmerge(0, "C3:D3")` を挟んでも同じく `mergeCells` が復活する。どちらも §4.2 のバリア op で
  snapshot を閉じない（§4.4 は構造 op だけが閉じる）ので、J0 で R2 を載せると「書いて戻す」の間の
  unmerge/unprotect/remove_autofilter/remove_hyperlink 等が黙って失われる。
  snapshot を削除の**後**に取った場合も、root が別の dirty 子（`sheetData`）を持っていれば
  `allows_clean` は true になるため、snapshot 時の子要素数を記録するだけでは塞がらない
  （C1 を先に書く → unprotect → A1 の snapshot → A1 を書く → C1 restore → A1 restore で再現する筋）。
- 修正案: 「子リストが source と同じ」を core で判定できるようにする（例: parse 時の子要素数を node に持ち、
  `clean_up` は「dirty 子 0 かつ 現在の子要素数 == source の子要素数」のときだけ clean にする。clean な子は
  全て source span を持ち、clean な要素を足せるのは source node を元の位置へ戻す restore だけなので、
  これで削除を検出できる）。unmerge/unprotect を挟む xlsx テストと、子要素削除後の `replace_element_at` の core テストを足す。

### M1（Major）: session が作った row が、後から snapshot を取ったセルの restore では消えない
- 根拠: `crates/ooxml-xlsx/src/snapshot.rs:203` は「この snapshot の時点で row が無かった」
  （`marks[ROW].is_none()`）ときだけ row を消す。プローブ: `snapshot(F6)` → `F6 = 1`（row 6 作成）→
  `snapshot(A6)`（row 6 は既にある）→ `A6 = 2` → `restore(F6)` → `restore(A6)` → 出力に
  `<row r="6"/>` が残り、part は dirty（raw copy されない）。逆順に restore すれば消えるので出力が履歴に依存する。
  ADR §7.4 C2 は「the session created … if that row then has no `c` and no attribute the session set」で、
  どのセルの snapshot が先かを問わない。oracle 11（同一 epoch 内の書いて戻すは byte 一致）を破る。
- 修正案: 「session が作った row」を row node 自体で判定する（source span を持たない新規 node か）。
  上の 2 セル・後 snapshot の順序を両方試すテストを足す。

### m1（Minor）: restore で消した `Everything` 式の閉包キャッシュが残り、`dependents` が never-written と異なる
- 根拠: `crates/ooxml-xlsx/src/snapshot.rs:215` の `census.remove` は graph の `everything_closure`
  （`graph.rs` の `OnceCell`）を無効化しない。プローブ: `A1 = 1, B1 = A1+1` で一度 `dependents(C5)`、
  snapshot(A1) → `A1 = INDIRECT("Z9")` → `dependents(C5)` = [A1, B1] → restore(A1) →
  `dependents(C5)` = **[B1]**（never-written は []）。同じことは `main` の逐次書き戻しでも起きる
  （既存、安全側の過剰 strip）ので Minor とするが、C2 の「依存辺を戻す」には入る。
- 修正案: restore（と `edit.rs` の census.remove）で `everything_closure` をリセットする `Graph` メソッドを呼ぶ。

### m2（Minor）: `restore_cell` が tree を変えた後に `Err` を返しうる
- 根拠: `snapshot.rs:171`（`replace_element_at` の後の `set_style_attr(...)?`）、`:187`（row 挿入後の
  `set_row_index(...)?`）、`:198`（`empty_cell` の途中）。いずれも実際には到達しにくいが、`Err` 時に
  derived state は未更新のまま tree だけ変わる（doc の「`Err` leaves the workbook as it was」に反する）。
- 修正案: 失敗しうる lens 取得を変更前に済ませるか、ここは `expect` にして不変条件として扱う。

## 作者の open questions / deviations の判定
1. `Result<CellSnapshot>`: 可。locate が拒否されうるので妥当。
2. snapshot 時の祖先状態を条件にする: 方針は可（ADR の「他の子孫が dirty なら消さない」より安全側）。ただし B1 の通り子リスト削除が抜けている。
3. `None` + `Current` で空 `c` + 現在の `s`: 可。§4.4「created `c` still differs from `None` in presence」と整合。
4. session が作った `sheetData` を消さない: 可（実ファイルで `sheetData` 欠落はまれ、出力は valid）。Minor 未満。
5. restore で census id が変わる: 可。ADR の id は §5 の Journal の行・列 id で、R3 の calcChain 判定もそれで数える。J0 が census fid をキーにしないことだけ注意。
6. `dimension` と他セルの strip を戻さない: 可（C5・C3 の範囲）。

## その他の確認
- core のテスト（`cargo test -p ooxml-core`、`serialize` の debug_assert 込み）は通過。CI は libreoffice・python-docx・python-pptx・openpyxl が pass、test/clippy は本レビュー時点で pending。
- `package/add.rs` と `move_subtree.rs` の変更はフラグの立ち方が同じで count だけ増える。docx/pptx の既定出力は不変と判断。
- 他の open PR（#144）は ooxml-core の xml/lens/add に触れておらず干渉なし。

## 判定
converged: no（Blocker 1 / Major 1 / Minor 2 / nit 0）
