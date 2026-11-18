# PR #68 レビュー（Opus, round 2）

対象: impl/xlsx-phase5-pre @ d4fa67888bd1c0c98c4b6a4314e55ea3ccd0d9ff

## 結論（先に）
round 1 の M1・M2・m1・m2・n1・n2 は、挙動で確かめてすべて解消していた。
単一セルの setter は `plan_writes` の 1 セルの場合になったが、main の単一セル API と出力が一致する（`Ok` の 5,000 件以上で byte 一致）。batch も、main の setter を同じ順で 1 つずつ呼んだ結果と一致する。拒否はすべて byte-identical で、poison も残らない。
残るのは、style-only 書き込みと「style を読む formula」を同じ batch に入れたときの順序の注記漏れ（m1、安全側、内部 API だけ）と、nit 1 件。
converged: yes。

## 前ラウンドの解消状況
- **M1（main と衝突）→ 解消**
  - PR は MERGEABLE で、CI は green。
  - `plan_writes` の構成:
    - 先頭で `check_editable()` を呼ぶ。
    - merge 範囲の anchor 規則（`m.first != at`）は main と同じ。
    - `CellOp::{Clear, Shared, Bool, Error, StyleOnly, InlineRich}` を扱う。
    - style の plan を batch 全体で積み上げる（`plan.next` / `staged`）。
    - sst は `reserve` で同じ文字列を 1 回だけ append し、`refs_delta` / `refs_changed` を合算する。
  - `CellWrite::String` は `plan_string_write` 経由で sst を使う（sst が無ければ inline）。
- **M2（複数の hand-off にテストが無い）→ 解消**
  - `a_batch_hands_off_both_shared_formula_masters` を追加した。
  - round 1 の変異（`handoffs.iter().take(1)`）は、このテストで落ちる。
- **m1（batch と 1 つずつの順序差）→ 解消**
  - ADR-0003 §3.1a の Oracles と ADR-0017 の note を実装に合わせた。
  - `fullCalcOnLoad` と `IterativeCalculation` の 3 ケースをテストにした。
  - `others` の除外を外す変異はテストで落ちる。
  - ただし style-only の場合が漏れている（下の m1）。
- **m2（`_xlws.FILTER`）→ 解消**: `_xlws.` だけの prefix も剥がして拒否し、`add_future_prefixes` が補う。テストもある。
- **n1 → 解消**: §4.4 に「表にある関数だけを検査する」と書いた。
- **n2 → 解消**: ADR のシグネチャを `Vec<(usize, CellIndex, CellOp, Extra)>` にそろえた。

## 検証したこと（問題なし）
- **main との差分プローブ**（使い捨て。コミットしていない）
  - 同じ乱数列を、main（b1413cf）と PR の両方で公開 API から流した。
  - fixture: synthetic・poi・poi-phase2 の全 xlsx と `Workbook::new()`。
  - 1 ケースは 1〜5 セル（A1:F12）で、1 fixture あたり 60 ケース。
  - **PR の単一セル setter と main**
    - 操作: number / string(sst) / string_inline / formula / remove_formula / bool / error / rich / set_num_format / set_format / number_with_format / datetime。
    - 両方 `Ok` の 2,753 件は、すべて byte 一致した（`Workbook::new()` の `core.xml` の時刻だけ除いて比較）。
    - 4 セル種の別の run でも、両方 `Ok` の 2,390 件がすべて一致した。
    - PR だけ `Err` になったのは、未 prefix の future 関数と `A1#` の拒否だけ（§4.4 の意図した変更）。
  - **PR の `write_cells` と、main の setter を 1 つずつ**
    - 両方 `Ok` の 2,366 件は、すべて byte 一致した。
    - 差は 2 種類だけで、どちらも文書化済み。
      - 同じ shared group の 2 セルを `SharedFormulaMaster` で拒否する（24 件）。
      - m1 の `IterativeCalculation` の順序差（4 件、batch は `Ok`）。
- **内部 API の batch と、PR の setter を 1 つずつ**
  - 範囲: `plan_writes` + `apply_writes`、Clear を含む全 13 種の `CellOp`、単一シートと複数シート、各 80 ケース。
  - `Ok` の 3,684 件が byte 一致した。不一致の 2 件と、複数シートでの不一致 6 件は、すべて下の m1 の場合。
  - `Err` の 541 件は、すべて byte-identical で、`poisoned` は無い。後ろのセルでの拒否も含む。拒否の内訳: MergedCell / ArrayAnchor / ArrayResultCell / TableHeaderCell / UnknownCellMetadata / NotExpressible / InvalidFormula / sst の `Core` など。
- **registry**
  - batch 内で同じ文字列を複数回書くと（`"dup"` ×2 と rich）、sst の `si` は 1 つになる。`count` と `uniqueCount` は、1 つずつ書いた結果と一致する。
  - 2 セルで同じ新しい xf を書くと、1 つにまとまる。
- **gate**
  - F-8 / #67 の census gate は、batch でも `Err` になり、byte-identical（既存テスト）。
  - merge 範囲: anchor 以外は `MergedCell` で拒否、anchor は書ける。
  - シート保護は、main の setter にも gate が無いので対象外。
- **手書きの変異**: 7 個すべてがテストで落ちた。
  - `handoffs.take(1)`
  - `others` の除外を外す
  - `reserve` の再利用を無効にする
  - `refs_delta -= strip.shared_refs` を消す
  - `plan_writes` の `check_editable` を消す
  - anchor 規則を外す
  - style の積み上げを捨てる
- **マージの解消（`git show --remerge-diff adfb1da`）**
  - error.rs・lib.rs・workbook.rs は、両側を残している。
  - edit.rs では、main の `edit_cell_mode`・`edit_style_only`・`apply` の各段を `plan_writes`・`apply_steps` に移していた。移した段: guard、style-only の自セル除外、`style_sensitive` のリセット、sst の `count` / `uniqueCount` 更新、index の更新。
  - 上の差分プローブでも、失われた main の挙動は見つからなかった。
- **他の PR との干渉**
  - #72（`strip_positions` に 1 行）とは、きれいにマージできる。
  - #77 は `lib.rs` の export 行で衝突する。後からマージする側が解く。

## Findings

### m1（Minor）: batch 内の style-only 書き込みは、batch 前の `style_sensitive` で判定する。この順序差が §3.1a の Oracles に無い
- **根拠**
  - `plan_writes`（`edit.rs:321`）は、`self.style_sensitive()` を batch 前の census で判定する。
  - そのため、batch が style を読む formula（`CELL`、または allowlist に無い `INDIRECT` など）を書いても、同じ batch の style-only セルは value 書き込みとして扱われない。
  - 1 つずつ書くと、formula の後の style-only 書き込みは dependents を strip する。
- **再現 1**
  - o1_precise の Sheet2 に `[C3 =INDIRECT("A1"), B1 NumFormatOnly("0.000")]` を書く。
  - 1 つずつ書くと、C1 の `<v>11</v>` と Sheet3 の `<v>22</v>` が消える。batch では残る。
- **再現 2**
  - poi/shared_formulas.xlsx に `[F9 =CELL("format",A1), B11 NumFormatOnly]` を書く。
  - A11 の `t="str"><v>ID</v>` が、batch では残る。
- **再現 3**
  - o1_iterate・47889 では、1 つずつ書くと `IterativeCalculation` になる。batch は `Ok`。
- **影響は安全側**
  - cache が書式に依存するには、その formula か上流の formula が書式を読む必要がある。
  - そういう formula が既存なら、batch 前から `style_sensitive` は true。
  - batch で書く formula なら、cache が無く、その下流は seed で strip される。
- **範囲**: style-only は公開の `CellWrite` に無いので、内部 API（Phase5）だけの話。順序を変えて formula を後にすれば、1 つずつ書いた結果と一致する。
- **修正案**
  - §3.1a の Oracles に 1 文足す。たとえば「style-only の書き込みは batch 前の style-sensitivity で判定する。batch が書く formula で workbook が style-sensitive になる場合、結果は formula を後に置いた順序で 1 つずつ書いたものと一致する」。
  - 再現 1 を `edit.rs` の単体テストにする。

### n1（nit）: `plan_writes` は、Worksheet を開いていないシートでは panic する
- **根拠**
  - `edit.rs:253` の `self.contexts.get(&sheet).expect("context loaded with the worksheet")` は、context を読み込んでいないシートの index を渡すと panic する。
  - 内部プローブで、`worksheet_by_index` を呼ぶ前に `plan_writes` すると再現した。
  - 単一セルの経路は Worksheet 経由なので到達しない（main と同じ）。
  - 複数シートを 1 つの plan で書く Phase5 の呼び出し側には、この前提が文書に無い。
- **修正案**: `plan_writes` で context を読み込むか、doc コメントに前提を書く。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 1）
