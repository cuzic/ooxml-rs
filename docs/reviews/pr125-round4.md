# PR #125 レビュー（Opus, round 4）

対象: impl/xlsx-phase7-pre @ 67bbae9640188f362fe2a496c52ca3737494a717

## 結論（先に）
converged: **yes**。round 3 の M1 と m1 は、挙動と変異で確かめて解消していた。#129 を取り込んだマージの衝突解消は、両側の論理を残している。現在の origin/main と比べたコーパスで、新たな拒否は `Protected` だけだった。CI はすべて pass。
残りは Minor 2 件で、テストの穴と、到達しなくなったコードである。follow-up でよい。**Draft を外して Ready にしてよい。**

## 前ラウンドの解消状況
- **M1 解消**:
  - `plan_writes_after` は、index map があり、`edit.sheet` 以外への書き込みが 1 つでもあれば、計画の前に `Err` を返す。
    - 現在の型は `Error::Malformed("… must target the edited sheet")`。
  - round 3 のプローブを、テスト `shifted_write_plan_rejects_other_sheets_before_shared_formula_handoff` が再現している。
    - 準備: Sheet2 の master を `Shifted!A2*2` にし、`Shifted!A3*2` に書き換えた Sheet2 を staged registry に入れる。
    - 結果: Shifted!A1 と Sheet2!B2 を書くプランは拒否され、`to_bytes()` は変わらない（byte-identical）。
    - Shifted だけを書くプランは成功し、Sheet2 は書き換え後の文書のまま残る。
  - 変異（コミットはしない）: 拒否の条件を `false` にすると、このテストが落ちる。
  - ADR-0018 の §2.1.1 と P7-pre 実装注記、doc コメントは「編集したシートだけ」に直っている。
- **m1 解消**:
  - 依存セルのガードは `check_write_dependents` に切り出され、`shifted_dependent_guard_excludes_cells_written_at_their_new_coordinates` がこれを単体で確かめる。
    - 確かめる内容: 反復計算のブック、古い座標の vm セル。
  - `post_edit` の `successor` を `Some(old)` にする変異は、このテストで落ちる。
- **main の取り込み（#129, P5d）**: `git show --remerge-diff 67bbae9` で、衝突は `edit.rs` の依存セルの `let d = …` の 1 箇所だけだった。解消は次の形である。
  - `!unshifted` → `DepSet::default()`（この PR の書き込みごとの判定）
  - `DynamicFormula` → 右下全域の矩形で `dependents_of_seeds`（#129）
  - `!style_only || style_sensitive` → `compute_dependents`（両側に共通）
  - その他 → 空
  - 両側の論理は残っている。衝突しなかった #129 の変更も、そのまま入っている。
    - `formula_set_changes` の `DynamicFormula => true`
    - shared master への動的配列の拒否
    - census への `dynamic` の登録
  - `git merge-tree --write-tree origin/main HEAD`（origin/main は 3cbf68a）は衝突しない。
- **テスト**: `cargo test -p ooxml-xlsx --lib` は 83 pass。

## コーパス（origin/main 3cbf68a との比較）
- 方法: `phase7_probe` を origin/main の worktree に写し、両方を `OOXML_XLSX_CORPUS=/tmp/poi-xlsx` で実行し、`compare_sheet_operations.py` で比べた。
- 対象: 334 ブック、811 シート。
- rename:
  - 一致 760
  - 同じエラー 10
  - 新たに成功 6
  - `Protected` 35
  - 関係の付け替え 18
- delete:
  - 一致 582
  - 同じエラー 191
  - 新たに成功 3
  - `Protected` 35
- 行ごとに突き合わせると、OK→ERR は 68 件で、すべて `Protected` だった。round 2・3 と同じ数値である。
- CI: `gh pr checks 125` はすべて pass（test/clippy/codegen、libreoffice、read-back 3 種）。

## Findings

### m1（Minor）: ずらしたシートへの書き込みで hand-off・依存セル・calcChain を飛ばすことを、どのテストも固定していない
- 変異 `let unshifted = true;` は全テストを通る。
  - この変異では、ずらしたシートへの書き込みにも、元のパートでの hand-off と、編集後の座標での `compute_dependents` が走る。
  - その結果、どちらも誤った位置を使う。
- 今の挙動は正しい。M1 の拒否によって、index map があれば `unshifted` は常に偽になる。
- 対処:
  - ずらしたシートで、編集前の shared master に当たる座標へ書くテストを 1 つ足す。
    - 期待: `handoffs` が空、`strip` が空、`chain` が `None`。
  - あるいは、判定を `edit.is_none()` に戻して意図を明示する。

### m2（Minor）: round 2 で足した「ずらしたシートの strip」の経路は、M1 の拒否で到達しなくなった
- 対象は次の 3 つ。
  - `plan_strip_with_index` の shifted 分岐（`successor` と `shift_area` による矩形の移動）
  - `apply_writes_with_insertion` の `PartNotFound` の許容
  - `refs_delta += strip.shared_refs - stripped_refs`
- index map があると依存セルは常に空で、`chain` も常に `None` になる。
- 変異: 前の 2 つを `unreachable!()` にしても、全テストが通る。
- 対処: P7b/P7c でこれらを使う予定がなければ削除する。使うなら、そのときにテストを付ける。

### 補足（指摘ではない）
- #129 の「shared master を動的配列で置き換えない」ガードも `unshifted &&` の内側にあり、ずらしたシートへの書き込みでは働かない。
- §2.1.1 の契約（shared group の materialization は呼び出し側）どおりなので、P7b で動的配列を書く経路を足すなら、呼び出し側で同じ拒否を持つこと。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 0）

Draft を外して Ready にしてよい。m1・m2 は follow-up でよい。
