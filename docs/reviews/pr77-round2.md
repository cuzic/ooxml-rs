# PR #77 レビュー（Opus, round 2）

対象: impl/xlsx-phase4-part2 @ 5ba70cfd72e33afed900a318a9279348516fc419

## 結論（先に）
converged: yes。round 1 の Major 4 件（M1–M4）と Minor 4 件（m1–m4）は、round 1 のプローブを再実行してすべて解消を確認した。拒否はどれも無変更（byte-identical）を保つ。
round 1 以降のコミットは 5ba70cf の 1 件だけで、生成物（`crates/ooxml-schema/src/generated/`）に触れず、Part 2 の編集 API の `check_editable` もそのまま。CI（Rust 1.99、python-pptx）は緑で、手元の `cargo test -p ooxml-xlsx --all-features` も通った。
新しく見つけたのは Minor 2 件。m3 と同じ種類の fail open（解析できない、または負の既存 `priority` を数えない）と、`Validation::compare` の doc に書いた制約が強制されていない点。

## 前ラウンドの解消状況
- M1: 解消。`rangefmla.rs` L36–49 は `Expr::Opaque` と `[QUALIFIED]` 関数を `InvalidFormula` で拒否し、明示的に解析できた形（`Ref`/`Name` の prefix）だけを通す。
  プローブ（DV custom と CF expression の両方、Err ⇒ byte-identical）: `Other!A1#`、`'Other'!A1#`、`Other!F(1)`、`SUM(Other!F(1))`、`Table1[Col]`、`Cfvo::Formula("Other!A1#")` → `InvalidFormula`。
  `[1]Other!A1`、`Other!X`、`Other:S!A1`、`Other!#REF!`、`Other!1:1`、`Other!A:A` → `CrossSheetReference`。`S!A1` は `Ok`。
  同じシートのスピル（`S!A1#`）も拒否されるが、「明示的に解析できた形だけを通す」方針どおりで、ADR の post-acceptance note に書かれている。
- M2: 解消。`validation.rs` L286–305。プローブ: 2 規則のうち 1 つを消す → `count="1"`、`x14:other` は残る。最後の規則を消して兄弟が残る → `x14:dataValidations` だけ消え、`ext` と `x14:other` は残る。兄弟が無い → `ext` を消す。
- M3: 解消。`validation.rs` L205–219、`hyperlink.rs` L184–189。`sqref="A:A"`、`ref="A:A"` に加え、`sqref`/`ref` 属性が無い場合も `ValidationOverlap`/`HyperlinkOverlap`（`existing: ""`）で拒否され、無変更。
- M4: 解消。`IconSetValues`（生成済みの 2007 版 ST_IconSetType、17 値。`3Stars`/`3Triangles`/`5Boxes`/`NoIcons` は parse できない）。しきい値の数を先頭の数字と照合し（`condfmt.rs` L230–239）、NaN/inf、0–100 の外の percent/percentile、`rank` 0、percent の `rank` > 100 を拒否する。ColorScale の NaN もプローブで拒否を確認した。
- m1: 解消。ADR-0014 §1 の後に post-acceptance note がある（L245–253）。
- m2: 解消。`rangefmla::check_second_formula` を DV と CF の両方から呼ぶ。
- m3: 優先度 0 の場合は解消（`condfmt.rs` L286）。負の値と解析できない値は抜ける（下の m1）。
- m4: 解消。新しい `hyperlink` に `xmlns:r` を宣言する。プローブ: ルートの `r` が別の URI に束縛されていると `xmlns:r1` を選ぶ（`r1:id`）。
- n1: 解消。n2: 未対応（nit なので follow-up でよい）。

## Findings

### m1（Minor）: 負の、または解析できない既存の `priority` を数えないので、`add_conditional_format_first` が先頭にならない
- 根拠: `crates/ooxml-xlsx/src/condfmt.rs` L161–176（`priorities` は `parse::<u32>().ok()` で読めない値を捨てる）と L441 以降（`renumber` も同じ）。L286 の拒否は `0` だけを見る。
  プローブ: `<cfRule priority="-1"/>`（xsd:int としては有効）に `add_conditional_format_first("A2", …)` → `Ok`。新しい規則は `priority="1"` になり、既存の `-1` のほうが優先されたまま残る。`priority="x"`、`"1.0"`、属性なしでも `Ok`。
  round 1 の M3/m3 と同じ種類の fail open。Excel が書く値は 1 以上なので入力はまれで、壊れたファイルにもならないため Minor とする。
- 修正案: `priorities` が、1 以上の u32 として読めない `priority`（属性なしを含む）を返せるようにする。`first` のときはそれを `InvalidSetting` で拒否する（`max + 1` の経路は今のままでよい）。`priority="-1"` のケースを `conditional_format_review_refusals_are_atomic` に足す。

### m2（Minor）: `Validation::compare` の doc にある「`List`/`Custom` 以外」が強制されていない
- 根拠: `crates/ooxml-xlsx/src/validation.rs` L103–110。プローブ: `Validation::compare(ValidationType::List, Between, "1", Some("2"))` → `Ok`。`<dataValidation type="list" …><formula1>1</formula1><formula2>2</formula2>` を書く。
- 修正案: `add_validation` で、`operator` が `Some` で種類が `List`/`Custom` なら `InvalidSetting` で拒否し、Err ⇒ byte-identical のテストを付ける。

## 他の open PR との干渉（参考）
- 今の open PR は #72、#75、#76、#23。round 1 から変わっていない。この修正コミットは `ooxml-xlsx` の Part 2 のファイルと ADR-0014 にしか触れないので、新しい干渉は無い。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 0）
