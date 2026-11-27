# ADR-0023 レビュー（Opus, round 4）

対象: `docs/adr/0023-xlsx-range-io-and-python-binding.md` revision 4。ブランチ plan/xlsx-rpa-foundation @ `7b5e2fa0eb2b305172c08712048153ad26766396`（revision 4 の改稿は `7b5e2fa`、round 3 の記録は `d874e81`）。

照合したもの:

- ADR-0024（Accepted）: §4.4、§4.5（F13）、§7.1（R1 の行。review round 8 の M1 で、shared member が自分の式を書き戻す場合は apply すると定めたもの）、§10.3。
- この head の `crates/ooxml-xlsx/src/edit.rs`: `content_style_same` の `CellOp::Formula` 腕（l.380–400。plain な `FormulaState`（`t="normal"`、属性なし）と比べる）と、`CellWritePlan::report`。

## 結論（先に）

round 3 の M1 には正しく応えている。drop 述語は、プランを作らずにセル単位で評価する §4.4 の状態の等しさになり、ガードの前に判定できる。F13 との関係も、「API の定義した意味として、sequential twin も同じ drop をかける」と説明され、筋が通っている。sequential twin（`Workbook::write_changed_cells`/`clear_range`）の追加は、ADR-0024 の part A（各 journal op は自分の sequential reference を持つ）と整合する。m1–m3 も直っている。

M2 への応答は、value モード（キャッシュが等しければ drop、0023 固有）には十分である。しかし formula モードの「同一の `Formula` 書き込みは、共有の report で drop される」は、**shared formula のメンバーと master、配列の anchor では成り立たない**。§4.4 の等しさは数式の kind と `f` の全属性を比べ、`Formula` 書き込みは plain（`t="normal"`、属性なし）として比べられるからである。そのため、oracle 3 が対象にする fill-down の shared 列や配列を含む領域を formula モードで往復すると、拒否されるか、グループが書き換わる（新 M1）。

**converged: no**（Major 1、Minor 1）。

## 前ラウンドの解消状況

- **M1（C1 の report ではガードの前に drop できない）:** 解消。§6.2 は、プランを作らない読み取り専用の §4.4 の状態の等しさを R1 と共有する形に改めた。F13 との関係（R1 の汎用 skip は、sequential が出す拒否を隠してはならない。一方 write_changed_cells の drop は API の定義で、twin も同じ drop をかけるので食い違わない）も妥当である。sequential twin を `Workbook` に置くことは、ADR-0024 part A の考え方（新しい op は sequential reference とともに oracle 1 に入る）と一致する。
- **M2（数式の黙った置き換え）:** 一部解消。
  - value モード: キャッシュが等しければ drop する 0023 固有の規則で、数式が保たれる。twin も同じ規則なので、journal とも食い違わない。R1 に無い規則を 0023 固有とするのは妥当である（R1 は no-op の skip を汎用に行うだけで、API の意味は定めない）。
  - formula モード: `ooxml_rs.Formula` を返すので往復でき、Q8（`str` は常にテキスト）とも整合する。ただし同一判定に穴がある（新 M1）。
- **m1:** 解消。`clear_range` は `ClearValue`。U2 で `IsoDate`、`FormulaWithResult`、kind 付きの `DateTime`、`Duration` を `CellCall` に足し、sequential reference を oracle 1 に入れる。
- **m2:** 解消（ADR-0024 §10.3）。
- **m3:** 解消（Sources を `213e27a` の main、ADR-0024、`journal/op.rs`、`edit.rs` に更新）。

## Findings

### M1（Major）: formula モードの往復で、shared formula のメンバーと master、配列の anchor は「同一の `Formula` 書き込み」として drop されない。fill-down 列を含む領域の書き戻しは、拒否されるかグループを書き換える

- **該当箇所:** §6.2「A formula write drops when its text, kind and attributes equal the cell's effective formula (the shared report covers it)」（l.524–526）、§11.4、§12 の `read_range(formulas="formula")`、oracle 3（l.1037 以降の「a fill-down shared column」と「a formula-mode round-trip keeps every formula」）。
- **根拠:**
  - 共有するという §4.4 の等しさは、数式について「text, kind and every `f` attribute」を比べる。`edit.rs` の `CellOp::Formula` 腕（l.380–400）は、書き込みを plain な `FormulaState`（`t: "normal"`、`reference`/`si` なし）として組み立てて比べている。コメントにも「A shared member's own text therefore differs (its `t`/`si` attributes), so it applies and splits」とある。
  - ADR-0024 §7.1 の R1 の行は、review round 8 の M1 で「a shared member's own text written back is applied」と**意図して** no-op にしないと定めている。
  - Python の `Formula` は `.text` だけを持ち、kind も `si` も `ref` も運ばない。そのため `CellWrite::Formula` は常に plain になり、shared のメンバーと master、配列の anchor のセルとは「kind と属性」が等しくならない。
  - 結果（oracle 3 の領域で formula モードを往復した場合）:
    - 領域が shared グループの一部だけを覆う: 同じグループの複数セルへの書き込みになり、`SharedFormulaMaster` で拒否される。
    - グループ全体を覆う: §6.3 によりグループ全体が plain な数式に置き換わり、`t="shared"` の保存形が失われる。XML の差分は変更したセル以外にも及ぶ。
    - 配列の anchor: `ArrayAnchor` で拒否される。
  - いずれも、oracle 3 の「formula-mode round-trip keeps every formula（identical `Formula` writes drop）」と「XML diff touches only that cell」に反する。
- **修正案:** 例として、次のいずれかにする:
  - (a) value モードのキャッシュ規則と同じく、0023 固有の「effective formula の等しさ」を定める。shared のメンバーは master の式を翻訳したテキスト、配列の anchor はその式と比べ、kind と属性は書き込み側に無いので比べない。等しければ drop する。R1 とは共有しないと明記する（R1 は意図して apply する）。
  - (b) `Formula` が読み取り元の kind（shared/array）と識別子を運び、同一ならそのまま drop する。

  どちらの場合も、oracle 3 に「shared 列の一部と全体」と「配列の anchor」の formula モード往復で、バイトが一致するケースを足す。

### m1（Minor）: journal セッションで drop 述語がどの状態を読むかが明記されていない（キャッシュ規則が twin と食い違いうる）

- **根拠:**
  - shadow は `CalcPolicy::Deferred` で、書き込み時にキャッシュを strip しない（ADR-0024 R3）。View の normalized モードは、保留中の strip をマスクする（§5）。sequential twin は、書き込みの時点で strip する。
  - §6.2 は「the pre-drop calls it on read state」としか書いていない。例として `A1` を変更した後、`B1 = A1*2` の古いキャッシュ値を書き戻すと:
    - 述語が shadow の生の状態を読めば、キャッシュが等しいので drop し、数式が残る。
    - twin ではキャッシュが strip 済みなので apply され、数式が定数になる。

    両者が食い違う。
- **修正案:** §6.2 に「セッションでは、drop 述語（キャッシュ規則を含む）は normalized View の状態を読む（保留中の strip はキャッシュ無しと読む）」と 1 文足し、oracle 1 か 3 に、このケースを足す。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 1 / nit 0）。Status は Proposed のまま（Accept 判定はしない）。
