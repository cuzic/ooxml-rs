# PR #77 レビュー（Opus, round 1）

対象: impl/xlsx-phase4-part2 @ 003c5734f92aa4e11ed32a47e4420b2a8b959456

## 結論（先に）
converged: no。範囲内の編集 API にはすべて `check_editable` が掛かっており、生成型の公開名も消えていない（main の `pub` 名はすべて残っている）。拒否の大半は無変更を保っている。
ただし次の 4 点が Major。他シート参照の拒否（ADR §5/§6、owner が維持を決めた fail closed）は、スピル参照 `Other!A1#` と修飾関数 `Other!F(1)` で `Ok` のまま抜ける。
x14 DV の削除は、同じ `ext` にある他の子要素ごと `ext` を消す。重複判定は、既存の `sqref`/`ref` を解析できないと重複なしとして通す（fail open）。
アイコンセットなどの CF パラメータは検証されず、Excel が修復するファイルを `Ok` で書く。

## 前ラウンドの解消状況
- （PR #77 の前ラウンドは無い）
- #58 round 3 m1: 解消。ADR-0014 §11 冒頭（L593）に `SheetProtection` の注記がある。
- #58 round 3 n1: 解消。`crates/ooxml-xlsx/src/protect.rs` L112 は `/// Sheet protection without a password, …`。

## Findings

### M1（Major）: 他シート参照の拒否がスピル参照と修飾関数で抜ける
- 根拠: `crates/ooxml-xlsx/src/rangefmla.rs` L35–51 と L55–68。`walk` は `Expr::Ref` と `Expr::Name` の prefix だけを見て、`_ => {}` で `Expr::Opaque` を素通しする。
  lexer の `spill_or`（`formula/lexer.rs` L437–444）は `Other!A1#` を prefix の無い `TokKind::Opaque` にし、`after_bang`（L462–465）は `Other!F(` を prefix の無い `Func("[QUALIFIED]F")` にする。
  プローブ（2 シートのブック、すべて `Ok` で書き込まれた）:
  - `Validation::custom("Other!A1#>0")` → `<formula1>Other!A1#&gt;0</formula1>`
  - `Validation::list_from("Other!$A$1#")` → `<formula1>Other!$A$1#</formula1>`
  - `RuleKind::Expression("Other!A1#>0")`、`RuleKind::Expression("Other!F(1)>0")` → そのまま `<formula>`
  - 同じ穴で、構造化参照 `Table1[Col]>0`（別シートのテーブルもありうる。Excel は DV/CF で構造化参照を受け付けない）も `Ok` になる。
  ADR §5/§6 の規則（他シートへの直接参照は `CrossSheetReference`）に反し、owner が決めた「Q2 までは fail closed」も破る。
- 修正案: `check_range_formula` で、`Expr::Opaque` と名前が `[QUALIFIED]` で始まる `Expr::Func` を拒否する（fail closed。前者は `CrossSheetReference` か `InvalidFormula`）。
  上の 4 式を `phase4_part2.rs` の拒否ケースに加え、Err ⇒ byte-identical を確かめる。

### M2（Major）: x14 DV の削除が、同じ `ext` にある他の子要素ごと `ext` を消す
- 根拠: `crates/ooxml-xlsx/src/validation.rs` L256–263 と L284–288。`total` は 1 つの `x14:dataValidations` に入っている規則の数で、`items.len() == total` のときに `list.remove_at(*ei)` で `ext` 全体を消す。
  同じ `ext` にある他の子要素（2 つ目の `x14:dataValidations` や未知の要素）も一緒に消える。
  プローブ: `<ext uri="{CCE6A557-…}"><x14:dataValidations count="1">…D1:D9…</x14:dataValidations><x14:other/></ext>` に `remove_validation("D1:D9")` → `Ok`。出力は `<extLst></extLst>` で、`x14:other` が黙って失われる。
  ADR §2 の行は「`ext` は空になったときだけ消す」。未知の内容は保持するという不変条件にも反する。
- 修正案: CF 側の `remove_x14_rule`（`condfmt.rs` L379–407）と同じ順にする。規則を消し、空になった `x14:dataValidations` を消し、`ext` は要素の子が残っていないときだけ消す。
  `count` は残った `x14:dataValidation` の数にする。このケースのテストを足す。

### M3（Major）: 重複判定が、解析できない既存の `sqref`/`ref` を重複なしとして通す（fail open）
- 根拠: `crates/ooxml-xlsx/src/validation.rs` L214（`ranges(&e).unwrap_or_default()`）、`crates/ooxml-xlsx/src/hyperlink.rs` L181–182（`CellRange::parse(existing).is_ok_and(..)`）。
  プローブ: 既存の `<dataValidation sqref="A:A">` に `add_validation("A5", …)` → `Ok`（同じセルに検証が 2 つ）。既存の `<hyperlink ref="A:A">` に `set_hyperlink("A5", …)` → `Ok`。
  ADR §5（`ValidationOverlap`: 1 つのセルに検証は 1 つ）と §4（`HyperlinkOverlap`）の規則が、入力によっては黙って破られる。
- 修正案: 既存の値を解析できなければ、重複とみなして拒否する（`ValidationOverlap`/`HyperlinkOverlap`。`existing` に元の文字列を入れる）。
  `remove_conditional_format` などの完全一致による検索は今のままでよい。テストを 2 件足す。

### M4（Major）: CF 規則のパラメータを検証せず、Excel が修復するファイルを `Ok` で書く
- 根拠: `crates/ooxml-xlsx/src/condfmt.rs` L228–231 と L540–545（`IconSet` の名前と `cfvo` の数を確かめない）、L456–458（`Cfvo::Num/Percent/Percentile` が NaN/inf のとき `num()` が `"NaN"`/`"inf"` を書く）、L504–512（`Top10` の `rank` 0）。
  プローブ（いずれも `Ok`）:
  - `IconSet("Bogus", [Min, Max])` → `iconSet="Bogus"`（ST_IconSetType に無い値）
  - `IconSet("3Arrows", [Min])` → 3 つのアイコンに対して `cfvo` が 1 つ
  - `IconSet("3Stars", …)` → x14 にしか無いセット（ADR §6 は x14 専用の形式の書き込みを範囲外にしている）を 2007 形式で書く
  - `DataBar(Num(NaN), …)` → `val="NaN"`
  - `Top10 { rank: 0, .. }` → `rank="0"`

  どれも Excel が修復するか、拒否するファイルになる（重大度表の「特定の入力で壊れたファイル」）。
- 修正案: アイコンセットを 2007 版 ST_IconSetType の 17 値の `enum` にする（公開 API なのでこの PR で行う）。または名前を許可リストで検査し、`cfvo` の数を先頭の数字と一致させる。
  `Cfvo` の数値は有限値に限り、`Percent`/`Percentile` は 0–100 にする。`rank` は 1 以上（percent なら 1–100）にする。どれも `InvalidSetting` で、Err ⇒ byte-identical のテストを付ける。

### m1（Minor）: 読み取り API を後続タスクとする ADR の注記が無い
- 根拠: owner の回答 1（PR コメント）は「ADR-0014 にはその旨を後続タスクとして記載する」。この PR の ADR の差分は §11 の 1 行だけで、§1 の表（L224–225）は `validations()`/`conditional_formats()` を挙げたまま。
- 修正案: §1 か §13 に、post-acceptance note として「`validations()`/`conditional_formats()` は Part 2 の範囲外。後続タスクとする（PR #77）」を 1 行足す。

### m2（Minor）: between / notBetween で 2 つ目の式が無くても受け付ける
- 根拠: `validation.rs` L105–111 と L186、`condfmt.rs` L196–201。プローブでは `Validation::compare(Whole, Between, "1", None)` と `CellIs(Between, "1", None)` が、式 1 つのまま `Ok` で書かれる。
- 修正案: `Between`/`NotBetween` は `formula2` を必須にし、それ以外の演算子では渡されたら拒否する（`InvalidSetting`）。

### m3（Minor）: `add_conditional_format_first` が優先度 0 の既存規則と重複する値を作る
- 根拠: `condfmt.rs` L268 と L410–447。プローブでは既存の `priority="0"` が 1 になり、新しい規則も 1 になる。ADR §6 は「新しい規則には一意の値」としている。
- 修正案: 既存の最小値が 1 未満なら拒否する。または `1 - min` だけずらす。

### m4（Minor）: ワークシートに `xmlns:r` が無いと外部リンクを拒否する（安全側に倒した過剰な拒否）
- 根拠: `hyperlink.rs` L187–189 は `NamespaceNotInScope` を返す（プローブで確認。無変更は保たれている）。サードパーティ製のブックでは、ルートに `r` の宣言が無いことがある。
- 修正案: 宣言を足す（core にその手段があれば）。無ければ、この拒否を doc コメントと ADR §13 の素通しされる拒否の一覧に書く。

### n1（nit）: `condfmt.rs` のモジュール doc が古い
- 根拠: `condfmt.rs` L3–6 は「Phase2 の `dxfs` registry が main に入るまで」と書いているが、registry はすでに使われている（`add_differential_format`）。
- 修正案: `Workbook::add_differential_format` の id を `Rule::with_dxf` に渡す、という説明に直す。

### n2（nit）: `poisoning_layout` が `range.rs` の `poisoning` の写しになっている
- 根拠: `layout.rs` L232–246 と `range.rs` L60–74 が同一。
- 修正案: `poisoning` を `pub(crate)` にして、それを使う。

## 他の open PR との干渉（参考）
- #72（wrapper の拒否）は `census.block` に理由を足す。Part 2 の全 API は `check_editable` を通るので、自動で効く。#72 は `edit.rs` にも触れるが、この PR の `edit.rs` の変更はテスト 1 件だけなので、衝突は小さい。
- #66, #74, #75, #76 は `ooxml-xlsx` にも `sml.rs`/`sml.toml` にも触れていない。

## 判定
converged: no（Blocker 0 / Major 4 / Minor 4 / nit 2）
