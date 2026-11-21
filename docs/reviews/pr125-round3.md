# PR #125 レビュー（Opus, round 3）

対象: impl/xlsx-phase7-pre @ bfb6db90beaebe9f6a5e6ac668675a7686f46f88

## 結論（先に）
converged: **no**。round 2 の M1 と m1〜m5 は、挙動と変異で確かめて解消していた。現在の origin/main（14ece88）とコーパスで比べると、新たな拒否は `Protected` だけだった。
ただし、今回の「書き込みごとの判定」には、別シートへの書き込みについて Major が 1 件ある。構造編集が別シートも書き換えてステージした場合、hand-off が**書き換え前**の数式から計算され、`Ok` のまま誤った数式になる。この経路を使う呼び出し側は今のところ無い（§2.1.1 の書き込みは編集したシートの見出しセルだけ）。そのため、`edit` があるときに別シートへの書き込みを拒否するのが最小の修正になる。それを直せば Ready にしてよい。

## 前ラウンドの解消状況
- **M1 解消**:
  - `plan_cell_writes` は `unshifted = shifted.is_none()` を書き込みごとに求め、hand-off・依存セル・calcChain の判定に使っている。
  - テスト `mixed_shifted_writes_preserve_shared_formulas_and_strip_dependents` が round 2 のプローブを再現している。内容は次のとおり。
    - "Other"（Sheet2）の B2 に書くと、B3 が `ref="B3:B5"` の master になり、B4・B5 が残る。
    - 依存先の `H9` は残る。
    - ずらしたシートの依存セル（A3）のキャッシュは、編集後の座標で外れる。
    - calcChain は 1 回だけ削除される。
  - 変異（コミットはしない）。次の 4 つは、どれもこのテストで落ちる。
    - 判定をプラン全体（`edit.is_none()`）に戻す。
    - hand-off の判定だけを戻す。
    - 依存セルの判定だけを戻す。
    - calcChain の判定だけを戻す。
  - 追加のプローブ（ずらしたシートで行を削除。依存セルは削除行・移動行・`#REF!` 化した式にある。書き込みは別シートとずらしたシートの両方）:
    - 結果は `Ok`。
    - 移動した A5→A3 のキャッシュだけが外れ、削除行の依存セルは無視される。
- **m1 解消**:
  - `predecessor` の `Axis::Row` を恒等写像にする変異は、`shifted_row_plan_reports_array_cells_at_post_edit_coordinates` で落ちる。
  - `successor` を恒等写像にする変異は、同じテストと mixed テストの 2 件で落ちる。
  - `plan_strip_with_index` の `successor` を恒等写像にする変異も落ちる。
- **m2 解消**:
  - テスト `one_based_column_range_ends_pin_and_clip` がある。
  - `IndexBase::One` では、`col max="16384"` が `Keep` になる。
  - 削除範囲の内側にある範囲の端は、`at-1` に切り詰める。例: `5`（1 始まり）を Delete{at:2,count:4} すると `"2"` になる。
  - 端でないものは `Remove` のまま。
- **m3 解消**:
  - ADR-0009 の note を「one `mc:AlternateContent` wrapper」に直し、入れ子は `UnrewritableReference` と明記した。
  - シート操作の走査コードはこのコミットで変わっていない。挙動は round 2 のプローブ（main と同じ拒否）のままである。
- **m4 解消**:
  - 要約表の L58・L59・L63 のシート操作の列が `–` になった。前文には、別のガードで拒否することを書いた。
  - `each_column_matches_the_location_summary` は、全 10 列を比べる。
    - G1・Copy・Table rename の 3 列はデータと比べる。
    - rename・delete の 2 列は、dispatch 規則（R/X/kept）と意味で比べる。
    - 残りの列は `locations_summary.txt` と比べる。
  - 変異では、どれもこのテストで落ちる。
    - L63 の rename を `X` にする。
    - L58 の Place を変える。
    - L14 の delete を `R` にする。
  - Place・Path・Syntax・観測値の列は、ADR 自身の写しと比べているだけである（データとは照合していない）。これは許容する。
- **m5 解消**:
  - PR 本文に、#129 への `x14::URI_SPARKLINE` の依頼を書いた。
  - `git merge-tree HEAD pr129`（#129 @ 08f3ee3）は、`edit.rs` でまだ衝突する。後からマージする側が解消する、という取り決めのとおりである。
- **CI**: `gh pr checks 125` はすべて pass（test/clippy/codegen、libreoffice、read-back 3 種）。
- **main との関係**: origin/main は 14ece88 に進んだ。`git merge-tree --write-tree origin/main HEAD` は衝突しない。

## コーパス（現在の origin/main 14ece88 との比較）
- 方法: `spikes/xlsx-phase7/README.md` の手順どおりに実行した。
  - `tests/phase7_probe.rs` を origin/main（14ece88）の worktree に写し、両方を `OOXML_XLSX_CORPUS=/tmp/poi-xlsx` で実行した。
  - 結果は `compare_sheet_operations.py` で比べた。
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
  - `Protected` 35（うち 2 件は `LastVisibleSheet`→`Protected`）
- 行ごとに突き合わせると、OK→ERR は 68 件で、すべて `Protected` だった。round 2 と同じ数値である。
- `phase7_pre` の ignored を含む 8 テストは通過した。

## Findings

### M1（Major）: 構造編集が別シートも書き換えてステージした場合、そのシートへの書き込みの hand-off は書き換え前の数式から計算され、`Ok` のまま誤った数式になる
- 根拠:
  - `plan_cell_writes` では、ずらしていないシートの書き込みは `self.pkg` の**元の**パートで計画される。対象は次のとおり。
    - `plan_target`
    - `plan_handoff`（`translate(&m.text, …)` の `m.text` は census、つまり編集前の値）
    - `plan_strip_with_index`
  - ところが、構造編集は「(any)」の場所（L4 など）を、**全シートで**書き換える。たとえば Shifted で行を挿入すると、`Other` の `Shifted!A2` は `Shifted!A3` になる。
  - そのため、呼び出し側は Other のパートもステージする。
  - `WriteIndexMap` が受け取るのは 1 枚のワークシートだけなので、planner はそのステージを知る手段がない。
  - 適用時には、registry がステージした Other に差し替えたあとで、hand-off の**古い**テキストを書き込む。
- プローブ（edit.rs の tests に一時追加。コミットはしない）:
  - 準備:
    - Other の B2:B5 は、master が `Shifted!A2*2` の shared group。
    - Shifted に `Insert{row, at:0}` の map を渡し、Other!B2 に Text を書く。
    - registry には、ずらした Shifted と、`Shifted!A3*2` に書き換えた Other をステージする。
  - 結果は `Ok`。B3 は `<f t="shared" si="0" ref="B3:B5">Shifted!A3*2</f>` になった。
  - 正しくは `Shifted!A4*2` である。新しい master の B3 は、書き換え後の B2（`Shifted!A3*2`）を 1 行ずらしたものになる。
  - B4・B5 も、この誤った master から計算される。
  - 依存セルの strip の位置も、元のパートで求めている。今回は構造が同じなので実害は出なかった。
- 範囲:
  - 公開 API からはまだ呼べない。
  - §2.1.1 の呼び出し側（テーブルの列挿入の見出し、列名の変更）は、編集したシートにしか書かない。
  - しかし、この PR の doc コメントと ADR の注記は「Writes on other sheets get the ordinary shared-formula hand-off …」と約束しており、P7b/P7c はそれを前提にできる。round 2 の M1 と同じく、Delivery 0 の成果物の `Ok` での誤りとして Major とする。
- 修正案（どちらか）:
  - (a) `edit` が `Some` のとき、`edit.sheet` 以外への書き込みを拒否する（round 2 の修正案の 2 つ目）。doc コメントと ADR §2.1.1 の注記をそれに合わせ、拒否のテストを付ける。§2.1.1 の用途はこれで足りる。
  - (b) 呼び出し側がステージしたパートの集合を受け取り、そのシートへの書き込みを拒否する。または、ステージ側の文書で計画する。
  - どちらでも、上のプローブをテストにする。

### m1（Minor）: `others` の `post_edit` の写像を外す変異が生き残る
- 次の変異は全テストを通る。
  - 変異の内容: dependents の guard（`others` の計算）で、`post_edit` の `edit.successor(old)` を `Some(old)` にする。
  - 理由: ずらしたシートで、書き込んだセルと依存セルが重なるケースのテストが無い。
- 影響:
  - `others` の用途は次の 3 つ。
    - `RichValueCell` の検査
    - `IterativeCalculation` の検査
    - `needs_calc`
  - `needs_calc` は、`edit` があれば常に真になる。
  - したがって、影響は「反復計算のブックで、依存セルが全部書き込みで上書きされる」ときの拒否の有無だけになる。
- 対処: follow-up でよい。そのケース（依存セルを、ずらした座標で上書きする）のテストを 1 つ足す。

### 「複数のずらしたシート」について
- `WriteIndexMap` は 1 枚のシートしか持たないので、1 つのプランに 2 枚のずらしたシートは作れない（ADR の「1 操作 1 プラン」どおり）。
- 2 枚目のシートへの書き込みは「ずらしていないシート」として扱われ、上の M1 に当たる。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 1 / nit 0）

M1 を (a) で直してテストを付ければ、Draft を外して Ready にしてよい。m1 は follow-up でよい。
