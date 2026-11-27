# ADR-0023 レビュー（Opus, round 3）

対象: `docs/adr/0023-xlsx-range-io-and-python-binding.md` revision 3。ブランチ plan/xlsx-rpa-foundation @ `b564eebe752112709a82f54c45eb36411fb43086`（revision 3 の改稿は `b564eeb`、round 2 の記録は `252a097`）。

照合したもの:

- `docs/adr/0024-xlsx-edit-journal.md`（Accepted）: §4.1、§4.5（F13）、§6.1、§7.1（R1 の行）、§7.4 の C1 の行、§9.2、§10.1、§10.3、Q4、Q5、§13.4、Delivery（J0–J3）。
- この head の `crates/ooxml-xlsx/src/edit.rs`（`CellWritePlan::report`、`report_write`、`content_style_same`）、`crates/ooxml-xlsx/src/journal/op.rs`（`CellCall`）、`crates/ooxml-xlsx/src/worksheet.rs`（`CellWrite`）。

## 結論（先に）

round 2 の M1（Edit Journal を基盤にした書き直し）は、構成の面では応えている:

- 依存先に ADR-0024 を加えた。
- §6.2/§6.3 を journal call にした。
- §5 を View の normalized 読みにした。
- §11.2 を `truncate(seq)` と Q5 (A) にした。
- Delivery を J 系列の後に並べ直した。

ADR-0024 Delivery の J1（View と Commit）と J3（truncation）との対応も正しい。m1、m2、m4 も直っている。

しかし、新しい「drop 述語は C1 の per-target report」は、C1 の report が**検証を通ったプランにしか存在しない**ため、§6.2 と oracle 3 が約束する「ガードの前に、変化しない guarded セルを落とす」を実現できない（M1）。

また、revision 2 から続く §11.4 の主張「数式を含む領域を read → write-back しても、変えたセルしか変わらない」は、どちらの読みモードでも成り立たず、数式を黙って定数や文字列に置き換える（M2。round 2 で見落とした）。

**converged: no**（Major 2、Minor 3）。

## 前ラウンドの解消状況

- **M1（Edit Journal を基盤に）:** 構成は対応済み。
  - §11.2 の入口は `current_seq`（呼び出しの境界）、失敗時は `truncate(entry)`、Q5 (A) で save を拒否する。ADR-0024 Q5 の決定 (A) と §10.3 に一致する。
  - Delivery の「U2–U5 は J1 の後、U8 は J2/J3 の後」は、ADR-0024 Delivery の J1（View と Commit）、J2（structural）、J3（`truncate_fast` を含む truncation）に一致する。
  - J1 exit requirement（公開の `Journal`、View、Commit、`current_seq`）は、ADR-0024 の J1 の範囲にほぼ収まる。`current_seq` は ADR-0024 本文に無い名前なので、ADR 側に cross-ADR の要求として書いたのは妥当である。
  - ただし drop 述語の設計に新しい問題がある（新 M1）。
- **m1:** 対応済み（clearing set を複数グループにし、`replaced_groups` を共有）。oracle 7 に 2 グループのケースも足された。
- **m2:** 対応済み。全文を grep すると、`ReadCell` が残るのは revision 履歴と応答表の 2 か所だけ。
- **m3:** Python 側は対応済み（`CLEAR` → `ClearValue`）。ただし §6.3 の `clear_range` は `CellWrite::Clear` のままで、journal の op 集合とずれる（新 m1）。
- **m4:** 対応済み。baseline `213e27a`、sequencing、U5 の所有者を更新した。ただし Sources に古い記述が残る（新 m3）。

## Findings

### M1（Major）: 「drop 述語 = C1 の per-target report」では、変化しない guarded セル（結合メンバー、配列の結果セル）をガードの前に落とせない。§6.2 と oracle 3 が約束する挙動に戻らない

- **該当箇所:** §6.2 の「Reads equal」と「The point of `write_changed_cells`」（l.495–504）、oracle 3（l.1000–1008）、§11.4（l.898–900）。
- **根拠:**
  - C1 の report は `CellWritePlan::report`（`edit.rs:215`）で、`plan_cell_write(s)` が `Ok` を返したプランにしか呼べない。テストの `report_of` も `plan_cell_write(..).unwrap()` の後で呼んでいる。
  - プランを作る段階で、ガードは先に走る。結合の非 anchor セルへの書き込みは `MergedCell`、配列の結果セルへの書き込みは `ArrayResultCell` で `Err` になる。つまり、値が同じでも report は得られない。
  - ADR-0024 §4.5（F13）と §7.4 の C1 の行は、この順序（plan で検証 → `Err` が拒否 → `Ok` なら no-op 判定）を規範として定め、検証より前の no-op 判定は「拒否を隠す」として退けている（spike の `r1_unvalidated`: `MergedCell`、`ArrayResultCell`）。
  - ところが §6.2 は、これを述語にしたまま「unchanged merge members, table headers and array members are dropped before any guard sees them」と書き、oracle 3 は「load-bearing」と書いている。どちらも C1 の report では実現できない。
  - 結果として、round 1 の M4 の典型ケース（配列の結果や、値を持つ結合メンバーを含む領域の read → 1 セル変更 → write-back）は、再び `RefusedError` になる。
- **修正案:** どちらかに決めて、§6.2、oracle 3、§11.4 を揃える:
  - (a) drop 判定に、ガードを通さない読み取り専用の比較（C1 の §4.4 の状態の等しさを、プランなしでセル単位に評価するもの）を R1 と共有する形で切り出す。そのうえで「ガードされたセルを、同値なら落とす」ことが ADR-0024 F13 と矛盾しない理由（write_changed_cells 自身の sequential reference でも同じ drop をかける）を書く。
  - (b) guarded セルは落とさない（拒否する）と仕様を弱め、M4 の約束を取り下げる。

### M2（Major）: 数式を含む領域の read → write-back は、どちらの読みモードでも数式を黙って置き換える。§11.4 と oracle 3 の「変えたセルだけが変わる」は誤り

- **該当箇所:** §11.4（l.883–900）、§12 の `read_range(formulas=…)`（l.931–935）、§6.2「A cell with a formula never reads equal to a constant write … formula writes are never dropped」、oracle 3。
- **根拠:**
  - `formulas="value"` では、数式セルはキャッシュ値（`Number` など）として読まれる。それを書き戻すと、`int`/`float` → `Number` の定数書き込みになる。§6.2 の規則で「数式セルは定数と equal にならない」ので drop されず、**数式がキャッシュ値の定数に置き換わる**。
  - `formulas="formula"` では `"=" + text` の `str` が返る。§11.4 の規則で `str` は「always text, also when it starts with `=`」（Q8）なので、**数式が文字列 "=A1*2" に置き換わる**。
  - `"both"` は `(value, formula)` の組を返し、そのまま書き戻せない。
  - つまり、oracle 3 が対象にする「formulas と fill-down shared column を含む領域」の write-back は、黙ってデータを失う（Excel を使わずに数式を保つという、この ADR の動機そのものに反する）。
  - この矛盾は revision 2 から続いている（round 2 での見落とし）。
- **修正案:** 例として、次のいずれかにする:
  - `formulas="formula"` が `ooxml_rs.Formula(text)` を返し、それが `Formula` 書き込みとして C1 で同一と判定されて drop されるようにする。
  - `formulas="value"` で読んだ数式セルの値を、数式セルの同値のキャッシュと比べて drop する規則を足す。

  いずれの場合も、oracle 3 に「数式セルを含む read → write-back で数式が残る」ことを明示的に入れる。

### m1（Minor）: journal の op 集合（`CellCall`）に、この ADR が使う書き込みが無い

- **根拠:** `journal/op.rs` の `CellCall` は `Number`、`String`、`StringInline`、`RichString`、`Bool`、`Error`、`DateTime(ExcelDateTime)`、`Formula`、`RemoveFormula`、`ClearValue`、`DynamicFormula`、`*WithFormat`、各種スタイル。次のものは無い:
  - `Clear`（§6.3 の `clear_range` は `CellWrite::Clear` を書くとしている）
  - `IsoDate`、`FormulaWithResult`
  - この ADR の `DateTime(v, DateKind)` と `Duration`

  ADR-0024 の Delivery は、新しい op が「its own sequential reference」付きで oracle 1 に加わるまで `Unsupported` のままと定めている。
- **修正案:** U2/U4b の範囲に「journal op（CellCall）を追加し、sequential reference を oracle 1 に加える」を明記する。§6.3 の `clear_range` は `ClearValue`（stored cell に対しては `Clear` と同じ）に揃える。

### m2（Minor）: 節の相互参照の誤り

- **根拠:** §11.2（l.822）の「`truncate_fast` only where §10.1 allows」は、この ADR の §10（packaging）を指してしまう。`truncate_fast` の条件は ADR-0024 **§10.3** にある（§10.1 は Commit 成功後の処理）。
- **修正案:** 「ADR-0024 §10.3」に直す。

### m3（Minor）: Sources が古い

- **根拠:** l.1253 付近の Sources に「The ADR-0021 implementation's working tree (uncommitted, 2026-11-25)」が残っている。ADR-0024 と、journal の実装ファイル（`journal/op.rs`、`edit.rs` の C1 report）が Sources に無い。
- **修正案:** Sources を `213e27a` の main と ADR-0024 に更新する。

## 判定

converged: no（Blocker 0 / Major 2 / Minor 3 / nit 0）
