# PR #158 レビュー（Opus, round 1）

対象: impl/xlsx-ej-c8 @ 6bded81f3c710320e22710db67898b067fdc0d5d

## 結論（先に）
converged: **no**。読み取り専用性（dirty なし・`to_bytes()` byte-identical）、session 中に追加された
`si` の live 読み（`set_rich_string` 直後に読めることをプローブで確認）、`f` 属性の真偽値解釈
（`parse_bool` の空白除去、未解釈値は `other` に残る）、prefix 非依存の `rPr` 正規形はいずれも正しい。
しかし (1) API が「workbook 内の生きているセル」しか読めず、§4.4 の比較のもう片方である
**snapshot の node**（`CellSnapshot::node()`）の状態を得る手段がない（M1）。(2) `rPh`/`phoneticPr`
を読まないため、ふりがな付きの `si` とふりがな無しの `si`（同じ text）が「同じ状態」になる
false equal がある（M2）。日本語の Excel ファイルでは `rPh` は普通に出るので、R1/R2 が sequential と
違う出力を作る。

## 前ラウンドの解消状況
round 1 のため無し。

## Findings

### M1（Major）: snapshot 側の状態を読めない — §4.4 の比較の片側しか提供していない
- 根拠: `crates/ooxml-xlsx/src/cell_state.rs:94`（`formula_state` は module private の `fn`）、
  `:168`（`runs_of` も private）、`:185-241`（`with_cell` / `cell_formula_state` / `cell_rich_runs` は
  `sheet, at` で live の `c` を探すだけ）。§4.4 の「back in its snapshot state」と R2 は
  **snapshot の node**（`snapshot.rs:74` `CellSnapshot::node() -> Option<&RawNode>`）と現在のセルを比べる。
  snapshot は workbook の木から外れた clone なので `cell_formula_state(sheet, at)` では読めず、J1 は
  この PR の関数で snapshot 側の `FormulaState` も `RichRun` も作れない（`t="s"` の snapshot は
  sst の index 解決も要る）。C8 の目的（state equality の入力）が半分しか満たされていない。
- 修正案: node 単位の入口を `pub(crate)` で出す。例: `FormulaState::of(&RawNode) -> Option<FormulaState>`
  （`c` から `f` を探す）と `Workbook::rich_runs_of(&mut self, c: &RawNode, at) -> Result<Option<Vec<RichRun>>>`
  （`cell_rich_runs` の本体をこちらに移し、live 版はそれを呼ぶ）。テストは `snapshot_cell` の node と
  live セルで同じ結果になることを 1 本。

### M2（Major）: `rPh` / `phoneticPr` を見ないので、ふりがなだけが違う文字列が false equal になる
- 根拠: `cell_state.rs:166-178`（`runs_of` は `r` 子しか見ず、`r` が無ければ `None`）、`:201-203` の doc。
  プローブ（tests に一時追加、コミットしていない）: sst
  `<si><t>漢字</t><rPh sb="0" eb="2"><t>カンジ</t></rPh><phoneticPr fontId="1"/></si><si><t>漢字</t></si>`
  で A1→si 0、B1→si 1 とすると `cell_rich_runs(A1) == cell_rich_runs(B1) == None`。value
  （`text_of` は `t` と `r` だけ）も同じなので §4.4 では同じ状態になる。
  経路: Base のセルが ふりがな付き `si` を指す → `set_string(同じ text)`。sequential は reuse が
  「rich と phonetic の item は一致しない」（`registry/strings.rs:7`）ので ふりがな無しの新 `si` を指す。
  Journal 側は R2 が snapshot に戻し（C1 後は R1 が skip し）、ふりがなが残る。sequential と出力が違い、
  P も `rPh` をモデルしていないので oracle 1 も気付かない。Japanese Excel は入力時に `rPh` を書くので
  稀な入力ではない。
- 修正案: string item の中身として `rPh` と `phoneticPr` も読む（例: `RichRun` 列とは別に
  `phonetic: Option<String>` = `rPh`/`phoneticPr` 子の `canonical` を連結したもの、または `is`/`si` が
  「`t` 1 個だけ」でないときは item 全体の `canonical` を返す）。plain + `rPh` を `None` にしない。
  ADR §4.4 と P の "rich runs" に phonetic を含めることを J1 か ADR の次の改訂で明記する。

### m1（Minor）: `<c t="s"/>`（`v` なし）が `Err`、value API は `Empty`
- 根拠: `cell_state.rs:214`（`v` 無しは `""`）→ `:223` で `InvalidValue { "shared-string index \"\"" }`。
  プローブで `CellRef::value()` は `Ok(Empty)`。J1 が状態読みの `Err` を op の失敗として扱うと、
  sequential が受け付ける書き込みを Journal だけが拒否する（oracle 1 (i)）。安全側だが過剰拒否。
- 修正案: `v` が無ければ `Done(None)`（value と同じく空）。あわせて doc に「`Err`（dangling index、
  `mc:AlternateContent`）を J1 は "unequal"（restore しない）として扱う」と書いておく。false unequal は
  restore 1 回分のコストで済むので、比較側で `Err` を不一致に落とすのが安全。

### m2（Minor）: テストの穴 — session 中の `si`、`mc:AlternateContent`、`canonical` のテキスト
- 根拠: 自作変異 2 個がどちらも 12 テスト全通過で生き残った:
  1. `cell_state.rs:232` の AC 検査を `if false` にする（AC を含む sst のテストが無い）。
  2. `:85` で `canonical` のテキスト子を全部捨てる（`rPr` の子にテキストが無い fixture だけ）。
     `canonical` は `pub(crate)` で、P の h1（「extension children … as canonical XML」）に流用されると
     text だけ違う拡張子が false equal になる。
  また PR 本文の "The sst is read from its live part, so edits made in the session are seen" を確かめる
  テストが無い（`set_rich_string` 後の読みはプローブで正しく動くことを確認した）。
- 修正案: (a) `set_rich_string` で session に追加した `si` を読むテスト、(b) sst 直下に
  `mc:AlternateContent` があるときの `Err`、(c) `canonical(<x>a</x>) != canonical(<x>b</x>)` を 1 本ずつ。

### n1（nit）: `RichRun::text` は生テキスト、value は trim・`_xHHHH_` decode 済み
- 根拠: `cell_state.rs:54-55`、`worksheet.rs:71-80`（`text_of`）。`<t>a </t>` と
  `<t xml:space="preserve">a </t>` は runs では等しく value では違う。value と一緒に比べる限り
  false unequal 側にしか倒れないので問題ではないが、runs 単独で P の "text" に使うとずれる。
- 修正案: doc に「value と組で比べる。runs の text は表示テキストではない」と一言。PR 本文の
  "a plain `t` next to `r` runs" も value（`text_of` が `t` と `r` を連結）で拾われる旨を書くと誤解が減る。

## 判定
converged: no（Blocker 0 / Major 2 / Minor 2 / nit 1）
