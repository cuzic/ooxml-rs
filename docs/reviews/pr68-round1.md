# PR #68 レビュー（Opus, round 1）

対象: impl/xlsx-phase5-pre @ 717369b95361a84bec2b1a72a7b21515738622b9

## 結論（先に）
CellWritePlan の原子性は確認できた。後ろのセルで拒否しても `Err` ⇒ byte-identical で、poison も残らない。registry 経路（fold 拒否・`StalePlan`・fault/panic）とテスト、future 関数の拒否と round-trip も ADR-0003 addendum / ADR-0017 Q7 のとおり動く。
ただし PR は main と衝突しており（CONFLICTING）、衝突の解消は機械的にできない。#50 の sst・style 計画と #58 の `check_editable`・merge 範囲の anchor 規則を plan に組み込む設計作業になる（M1）。
加えて、この PR で新しく入った「1 回の batch で複数の hand-off」の経路にテストが無く、壊れたファイルを書く変異がテストを通り抜ける（M2）。
converged: no。

## 前ラウンドの解消状況
round 1 のため対象なし（中断された前回の試行の残りは参照していない）。

## 検証したこと（問題なし）
- **原子性の差分プローブ**（使い捨て。コミットしていない）
  - 対象: o1_precise / shared / arrays / name_scope / everything_volatile_ca / no_calcpr / manual / duplicate_caches の 8 fixture。
  - 方法: 1〜4 セルのランダムな batch を 2400 件流した。セルは A1:F12、操作は Number / String / Formula / RemoveFormula。
  - `Err` の 180 件: すべて byte-identical で、`Poisoned` は 0 件。後ろのセルの拒否も含む。
  - `Ok` の 2220 件: 2219 件が、同じ書き込みを 1 つずつ行った結果と一致した（不一致の 1 件は m1）。
- **その他の境界**
  - 空の batch は `Ok` で、出力は byte-identical。
  - 2 つの shared group の master（o1_shared の D2 と B2）を 1 回の batch で書くと、1 つずつ書いた結果と一致する。
  - poi/xlookup.xlsx の既存の `_xlfn.XLOOKUP` は batch で編集した後も残る。
- **`#` と `@` の判定**
  - 拒否される: `Sheet1!A1#`、`'Sheet1'!A1#`、`[1]Sheet1!A1#`、`SUM(Sheet1!A1:A2#)`、`@INDIRECT(…)`。
  - 通る: `T1[#Headers]`、`T1[@Col]`、`_xlfn.ANCHORARRAY` / `_xlfn.SINGLE`、`_xlfn._xlws.PY`、`_xlfn.LET(_xlpm.x,…)`。
- **手書きの変異**
  - `formula_set_changes |=` を `=` に変える: 単体テスト 2 件で落ちる（殺せた）。
  - dependents の union で `results` を捨てる: Phase1 テスト 3 件で落ちる（殺せた）。
- **CI**: test / clippy / codegen --check と pptx オラクルは green。

## Findings

### M1（Major）: main と衝突し、解消には #50・#58 の計画を CellWritePlan に組み込む設計作業が要る
- **根拠**
  - `git merge origin/main`（8610086）で 4 ファイルが衝突する。
    - `crates/ooxml-xlsx/src/edit.rs`（7 hunk）
    - `crates/ooxml-xlsx/src/error.rs`
    - `crates/ooxml-xlsx/src/lib.rs`
    - `crates/ooxml-xlsx/src/workbook.rs`
  - main の `edit_cell_mode` は、この PR が置き換えた単一セル経路に次のものを足している。
    - `check_editable()`: #58 の workbook 単位の gate。#67 の `RowMismatch` は census の block 経由でここに入る。
    - merge 範囲の anchor を書ける規則（`m.first != at`）。
    - `CellOp::{Clear, Shared, Bool, Error, StyleOnly}`。
    - `Extra` と `plan_registries`（sst の append と `refs_delta`、style の plan）。
    - `check_edit_cell` の dry run。
    - `style_sensitive` のリセット。
  - main の `set_string` は sst に書く（`write_string`）。
    - この PR の `CellWrite::String` は「As `set_string`」と書きながら inline で書く。
    - `a_batch_equals_the_same_writes_in_sequence` の比較相手は `set_string` なので、マージ後は前提が崩れる。
  - ADR-0017 §2.1 は CellWritePlan の検証に sst のチェックを含めるよう求めている（slot、同じ形の再利用、同じ plan 内で同じ文字列を 2 回 append しない）。
    - この PR はそれを「#50 がマージされたら」と先送りした。#50 はもうマージ済みなので、PR 本文の「後からマージする側が解く」に従えば、解くのはこの PR。
  - #61（`InlineRich`）と #72 も `edit.rs` と `worksheet.rs` に触れる。
- **修正案**
  - main を取り込み、`plan_writes` で次のことをする。
    - 先頭で `check_editable()` を呼ぶ。
    - セルごとに `plan_registries` を行い、`refs_delta` と sst の append を plan 全体で積み上げる（同じ文字列は 1 回だけ append）。
    - merge 範囲の anchor 規則を main に合わせる。
  - `CellWrite::String` を sst にするか inline にするかを決め、ADR-0003 §3.1a item 6 に書く（inline のままなら `set_string_inline` を参照する）。
  - マージコミットには、何をどう解いたかを書く。

### M2（Major）: 1 回の batch で複数の hand-off をする経路にテストが無く、壊れたファイルを書く変異が生き残る
- **根拠**
  - `edit.rs:477` の `for (part, h) in &handoffs` を `handoffs.iter().take(1)` に変える変異は、`cargo test -p ooxml-xlsx --all-features` を全部通る。
  - この変異の下で o1_shared Sheet2 に `[(D2, Number 1), (B2, Number 2)]` を書くと、`Ok` で次のファイルになる。
    - B2 の master が消える。
    - B3:B5 は `<f t="shared" si="0"/>` のまま残り、master が無い（壊れた shared formula）。
  - 変異の無いコードは正しい（1 つずつ書いた結果と一致）。
  - 既存のテストは、batch に master が 1 つ（`o1_shared` の D2）のケースしか持たない。
  - 複数の hand-off は、この PR で新しく入った経路（`handoffs: Vec`、適用ループ 2 つ）。
  - P5b の表作成は、範囲内の複数の group に触れる。
- **修正案**
  - `phase1_batch.rs` に、2 つの group の master を 1 回の batch で書くケースを足す。o1_shared Sheet2 の D2 と B2 で作れる。
  - batch の結果が 1 つずつ書いた結果と一致すること、両方の group に master があることを確かめる。

### m1（Minor）: 「batch は 1 つずつ書いた結果と同じ」「cell k は 1..k−1 の後の census で計画する」が、書く順序によっては成り立たない
- **根拠**
  - 実装は、dependents を batch の前の graph から取り、batch で書くセルを `others` から外す（`edit.rs:246`）。
  - そのため、batch で新しく書いた Everything な formula は、後ろのセルの dependent として数えられない。
- **再現**
  - o1_shared Sheet2 に `[A1 RemoveFormula, D3 =INDIRECT("A1"), C5 "s", F9 "s"]` を書く。
    - batch では `fullCalcOnLoad` が付かない。
    - 同じ順で 1 つずつ書くと付く。逆順で 1 つずつ書くと batch と一致する。
  - o1_iterate に `[J20 =INDIRECT("A1"), K20 1]` を書く。
    - batch は `Ok`。
    - 同じ順で 1 つずつ書くと `Err(IterativeCalculation)`。逆順なら `Ok`。
- **影響**: 差は、batch で書いたセルそのもの（cache の無い formula）だけなので、安全側。ただし次の 2 か所と食い違う。
  - ADR-0003 §3.1a item 2 の見出し（Accumulated census）とオラクルの文言。
  - ADR-0017 §2.1 の「cell k is planned against the census as cells 1..k−1 leave it」。
  - 加えて、`others` から書いたセルを外さない変異（`deps.cells.iter().copied()`）もテストを生き残る。
- **修正案**
  - §3.1a の文言を実装に合わせる。たとえば「batch は、formula を書くセルを最後に置いた順序で 1 つずつ書いた結果と一致する。guard と `calcPr` の判定は、plan が書くセルを除いた dependents で行う」。
  - 上の 2 例をテストにする。

### m2（Minor）: `_xlfn.` の無い `_xlws.FILTER(…)` / `_xlws.SORT(…)` が拒否されない
- **根拠**
  - `future.rs:66` は `_XLFN._XLWS.` と `_XLFN.` だけを剥がす。
  - `_xlws.FILTER(A1:A3,B1:B3)` は bare 名が `_XLWS.FILTER` になり、表に無いので `set_formula` が `Ok` になる。Excel では `#NAME?` になる。
  - `add_future_prefixes` も `_xlws.FILTER` をそのまま返す。
  - addendum §4.4 は「`_xlfn.FILTER` without `_xlws.` is refused too」として、prefix が一部だけの場合を拒否する方針。
- **修正案**
  - `_XLWS.` だけが付いた名前も剥がして表を引き、`MissingFunctionPrefix` にする。`add_future_prefixes` は `_xlfn.` を補う。
  - テストを 1 行足す。

### n1（nit）: §4.4 に、表に無い新しい関数の扱いを書く
- **根拠**: `GROUPBY`、`PIVOTBY`、`REGEXTEST`、`TRIMRANGE`、`PERCENTOF` などは RX 0.102 の表に無いので、prefix 無しで受け付けられる。
- **修正案**: owner の決定（固定の表）どおりなので、§4.4 に「表に無い関数は検査しない」と 1 文足す。

### n2（nit）: ADR と実装のシグネチャがずれている
- **根拠**: ADR-0003 §3.1a item 1 は `Workbook::plan_writes(&[(sheet, cell, op)])` と書くが、実装は `Vec<(usize, CellIndex, CellOp)>` を取る。
- **修正案**: どちらかに合わせる。

## 判定
converged: no（Blocker 0 / Major 2 / Minor 2 / nit 2）
