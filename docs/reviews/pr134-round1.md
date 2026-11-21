# PR #134 レビュー（Opus, round 1）

対象: fix/followups-pr121-pr123 @ 2baab631cac798ce841d255e79d8cab61ab5446c

## 結論（先に）
Issue #128 の各項目（121-m1〜m5、n1〜n4、123-m2〜m4）は実装・テストともに入っており、受け入れ条件の変異はすべて落ちた。
新しい不具合として Blocker/Major は見つからなかった。z-index の負値の扱い（Minor）と、範囲の注記漏れ・sweep の経路の nit だけ。
converged: yes。

## 前ラウンドの解消状況
（round 1 のため、Issue #128 の項目ごとに確認）
- 121-m1: 解消。`require_owner` 6 箇所（notes 174、thread 削除 201、VML 追加 244・削除 250、person 167、thread 追加 305）を 1 つずつ外すと、
  どれも `ambiguous_part_links_and_insertion_slots_are_atomic` が落ちる。新しい 5 ケースは `rejected`（全バイト不変）で確認している。
- 121-m2: 解消。SetText/RemoveNote の `is_fallback` 拒否を外した。スレッドが残るセルは前段の `is_thread` で今まで通り `ThreadedComment`。
  孤立 fallback の削除は comment と VML shape を消し、threaded 側に参照は残らない（`tc=` の author が残るだけで無害）。
  孤立 fallback への `add_note`/`add_comment` は `NoteExists`（プローブで確認）。拒否を戻す変異で `orphan_fallback_note_can_be_edited_and_removed` が落ちる。
- 121-m3: ADR-0017 に注記あり。
- 121-m4: 解消。新規 VML のルートだけが `v`/`o`/`x` を宣言し、子は宣言しない。既存 VML への挿入は従来通り子で宣言（`child_vo`/`child_vox`）、
  テストは挿入 shape の span 以外が元と一致することを確認（局所性）。ルート宣言を消す変異、子にも宣言を残す変異の両方で落ちる。
- 121-m5: 解消。メモ追加前のコピーでスレッドの add → reply → remove を通し、person 拒否と add_comment 拒否を別集計。
- 121-n1: 解消。既定名前空間の判定は挿入先の祖先の連鎖（author: authors→root、comment: commentList→root、text: comment→commentList→root、
  threadedComment/person: root）を見ており、接頭辞付きルートや `xmlns="urn:other"` の再束縛でも正しい。新規パートはラッパーが既定名前空間を宣言する。
  常に宣言する変異で 3 テストが落ちる。
- 121-n2: 解消。大文字小文字を区別する変異で `fallback_uid_matches_thread_guid_ignoring_case` が落ちる。
- 121-n3: ADR-0017 に注記あり。
- 121-n4: 解消（下の m1 を除く）。固定 `z-index:1` に戻す変異で落ちる。`i64::MAX` での拒否は編集前に返るので全バイト不変。
- 123-m2 / 123-m3 / 123-m4: 解消。README のアンカー `#project-made-file-project` は見出しと一致、ライセンスは Cargo.toml の `MIT OR Apache-2.0` と一致。CI 全 5 ジョブ通過。

## Findings
### m1（Minor）: 既存 shape の z-index がすべて負だと、新しいメモが負の z-index になる
- 根拠: `crates/ooxml-xlsx/src/comments/vml.rs` の `z_index` は `max().unwrap_or_default() + 1`。プローブで既存メモの `z-index:1` を `z-index:-5` に変えて
  `add_note(B2)` すると、新しい shape は `z-index:-4`。VML の負の z-index はセルの背面に置く指定なので、表示したメモが隠れうる（修正前は常に 1）。
  Excel 自身は正の値を書くので、実ファイルでは稀。
- 修正案: `max(0, 既存の最大値) + 1` にし、ADR の注記も「1 以上」と書く。

### n1（nit）: z-index が i32 を超えうる
- 根拠: 上限を `i64::MAX` で判定しているので、既存の最大が `2147483647` なら `2147483648` を書く。CSS/VML の実装が 32 bit 整数で読む場合の挙動は未確認。
- 修正案: 上限を `i32::MAX` にそろえるか、ADR 注記に「i64 で扱う」理由を一行書く。

### n2（nit）: sweep で「作ったばかりの VML にスレッドを足す」経路が通らなくなった
- 根拠: 121-m5 の要求どおりスレッドはメモ追加前のコピーに入れるため、同じブックでメモ（新規 VML）→スレッドの順に積む経路はコーパスでは通らない。
  単体テストでは通っているので挙動の問題ではない。
- 修正案: 不要。気になるならメモ追加後のブックにも add_comment を 1 回足す。

### 補足（指摘ではない）
- PR #129（xlsx P5d）も `docs/adr/0017-xlsx-phase5-objects.md` の同じ末尾（`@@ -1041,3`）に注記を追記しているので、後にマージする側で文書の衝突が出る。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 2）
