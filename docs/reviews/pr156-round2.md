# PR #156 レビュー（Opus, round 2）

対象: impl/xlsx-ej-c2 @ b045545d02640829e28eddbe14eb4fcb7e833022

## 結論（先に）
converged: **yes**。round 1 の B1・M1・m1・m2 はすべてプローブで挙動として解消を確認した
（B1 は round 1 で挙げた「削除の後に snapshot を取る」筋も含む）。`elem_delta` は子リストを変える
core の全プリミティブ（`insert_child`/`remove_child`/`splice_children`/`retain_children`/`set_children`、
それを使う lens・`move_subtree`・`package/add`・same-part insert、deferred の展開と `release_children`）で
整合しており、xlsx の編集 API から snapshot〜restore の間に「同数で別の clean 要素」や「テキストだけの変更」を
祖先に作る経路は見つからなかった。残りは core の公開 API の文書精度とテストの穴で、Minor 2 / nit 1。

## 前ラウンドの解消状況
- B1: 解消。`unprotect_sheet`・`unmerge` を挟む write→restore で削除が残ることを確認。round 1 の第 2 の筋
  （C1 を snapshot・書く → `unprotect_sheet` → A1 を snapshot・書く → C1 restore → A1 restore）でも
  `sheetProtection` は戻らない。root は `elem_delta = -1` で clean にならない。
- M1: 解消。F6→A6→C6 の 3 セルを 3 通りの順で restore しても出力は入力と byte 一致、part は clean。
- m1: 解消。`restore_cell` が `forget_everything_closure()` を呼び、INDIRECT を戻した後の `dependents(C5)` は空。
- m2: 解消。row 作成は `check_insert_at` を変更前に通し、残りの lens 呼び出しは `expect`。`set_attr` は
  属性名と XML 文字しか検査せず、値は parse 済みの `s` なので `expect` は落ちない。

## 試して壊れなかったもの（プローブ、コミットしていない）
- 同数の置き換え: `unmerge("C3:D3")` → `merge("E5:F5")`、`unprotect_sheet` → `protect_sheet` を cell の
  write と restore の間に挟む → 新しい要素は created（dirty）なので root の dirty 子が残り、restore 後も新しい方が出る。
- 自己終了の source row `<row r="3"/>` に C3・D3 を書いて両方 restore → `<row r="3"/>` に戻り clean。
- session row 判定（span 無し）: `set_row_height` が作った row に F8 を書いて restore → `ht` を持つので残る。
  write で作った row に後から `set_row_height` → 残る。source row の `ht` 変更 → 残る（属性比較で clean にならない）。
  span を持つ row を session が作る経路（source row の clone 挿入）は xlsx に無い。source row を消して作り直すのは
  構造 op（行削除）だけで、§4.4 により snapshot を閉じる。
- テキスト・markup の子: worksheet / `sheetData` / `row` のテキストを変える xlsx の経路は、構造 op の
  `remove_indices(.., remove_intervening_whitespace)`（grid.rs）だけ。cell op は `c` の内側しか触らず、
  バリア op は lens の `remove_at`/insert で要素だけを動かす。snapshot〜restore の間には到達しない。
  仮に到達しても clean になった祖先は source の bytes に戻るだけで、失うのは空白だけ。
- `set_hyperlink` を挟む restore → hyperlink は残る。`dirty_counts_consistent` は全プローブで成立。
- `cargo test -p ooxml-core -p ooxml-xlsx` は全通過。

## Findings

### m1（Minor）: `elem_delta == 0` が見るのは子要素の「数」だけ。同数の並べ替えは core の `replace_element_at` で黙って戻る
- 根拠: core のプローブ（`restore_tests` に一時追加）: `<r><a><b/><c/></a><d><e/></d></r>` で `a` を `d` の後ろへ
  移す（`move_subtree` と同じ `remove_child` + clean な node の `insert_child`）→ `d/e` を dirty にしてから
  clean な clone で `replace_element_at(.., |_, _| true)` → 出力は `<r><a>…</a><d>…</d></r>`、`is_dirty() == false`。
  移動が消える。xlsx では `move_in_by` は workbook.xml のシート移動（構造 op）にしか使われず、`restore_cell` は
  worksheet の part しか触らないので到達しない。ただし `XmlDocument::replace_element_at` / `remove_element_at` は
  `pub` で、`node.rs` の `clear_dirty` の doc は「child list are as in the source」、`elem_delta` の doc は
  「the element list has the source's length」と、前者は事実より強い。
- 修正案: `replace_element_at` の doc に「clean な要素を足す・動かす変更（`move_subtree`、same-part insert）の後では
  `may_clean` が子の順序まで保証すること」を書くか、`element_list_unchanged` を「source の子と同じ数で、全子が
  span 順に並ぶ」に強める。J0 の前に doc だけでもよい。

### m2（Minor）: B1 の xlsx テストが「削除の後に snapshot」の筋を含まない
- 根拠: `a_removed_sibling_of_an_ancestor_is_not_brought_back` の `snapshot_late` は B1 を書いてから snapshot を
  取るが、削除（`unprotect_sheet`/`unmerge`）は snapshot の後。round 1 が挙げた「C1 を書く → unprotect → A1 の
  snapshot → … → C1 restore → A1 restore」（root が別の dirty 子を持つので `allows_clean` が通る筋）は
  テストに無い。挙動は正しい（上のプローブで確認）。
- 修正案: その順のケースを 1 つ足す。

### n1（nit）: `restore_tests::a_removed_child_keeps_the_ancestor_dirty` の `&[1 - 1]`
- `&[0]` と書けばよい。

## 他の確認
- docx / pptx の既定出力: b045545 の core 変更は `elem_delta` の加減と `clean_up` の条件だけで、`clean_up` は
  `replace_element_at` / `remove_element_at`（呼び出し元は xlsx の `restore_cell` のみ）からしか呼ばれない。
  serialize とフラグの立ち方は変わらない。
- CI（本レビュー時点）: python-docx・python-pptx・openpyxl の read-back と libreoffice は pass、test/clippy は pending（ローカルの `cargo test -p ooxml-core -p ooxml-xlsx` は通過）。
- 他の open PR は #144（pptx comments）のみで、ooxml-core の xml/lens に触れず干渉なし。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 1）
