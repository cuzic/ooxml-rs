# PR #159 レビュー（Opus, round 1）

対象: impl/xlsx-ej-j0a @ f701d7f8ac7c20109fc964be98faf6e35aa077d9

## 結論（先に）
converged: **no**（Major 1）。R2 on C2 の本体（first-write snapshot、full / content restore の分岐、
hand-off target、拒否時に snapshot・key・seq を残さないこと）は ADR-0024 §4.4/§4.5 とタスクの決定 A1–A17 どおりで、
§4.4 の raw equality で「偽の equal」になる経路は見つからなかった。自作の変異 4 個（state が `t` を無視、
s 違いでも full restore、P がセルの style を無視、style-only 後の content restore 省略 = OQ1 (b)）は
最初の 3 個がすべて落ち、4 個目は全テスト green（(b) が健全であることの裏付け）。
Major は 1 件で、C3 の代わりに使っている `KeepStaleCaches` が、依存先シートの strip 計画で出る拒否を隠す。
`edit_refusal() == None` のブックで、逐次側は `Err`、Journal は `Ok` になる（oracle 1 (i) 違反。J1 では
「受理した Journal は必ず保存できる」にも触れる）。OQ1（style op が約 10 倍遅い）は J0 の Major ではなく、
J1 の前提条件とする。推奨は (b) をこの PR か J0b で入れ、(a) を C2 の follow-up にすること。

## 前ラウンドの解消状況
（round 1 のため、なし）

## Findings

### M1（Major）: `KeepStaleCaches` が依存先シートでの strip の拒否を隠す。逐次側が `Err` のとき Journal は `Ok` を返す
- 根拠: `crates/ooxml-xlsx/src/journal/mod.rs:96-97` は shadow を `KeepStaleCaches` にする（C3 の代わり、決定 A2）。
  `edit.rs:500-504` はこの policy のとき `plan_strip` を**呼ばない**。`plan_strip` は依存先シートごとに
  `ensure_index`、`xml_part`、`strip_positions`（`index.check_edit()`、`index.rows()`、`index.cells()`）を通るので、
  依存先シートの構造エラー（行の順序違い、`wrapped_grid` など）は逐次側でだけ拒否になる。
  `check_write_dependents` は policy に依らない。
  プローブ（コミットしていない）: Sheet1 `A1=1`、Sheet2 の `sheetData` は `<row r="2">…</row><row r="1"><c r="A1"><f>Sheet1!A1+1</f><v>2</v></c></row>`。
  `Workbook::edit_refusal()` は `None`。`set_number(Sheet1!A1, 5)` を実行すると、
  逐次側（`StripDependentCaches`）は `Err(OutOfOrder("rows in xl/worksheets/sheet2.xml"))`、`Journal::append` は `Ok(1)`。
  ADR §4.5 の「refusal equivalence holds by construction」が、この代わりの部品のせいで成り立たない。
  oracle 1 の fixture は依存先シートがすべて整っているので、生成テストではこの差に届かない。
  J1 で C3 が strip を save まで遅らせると、同じ入力で R3 の strip が commit 時に失敗する。これは
  「受理した Journal は必ず保存できる」（§4.5 の最後、§9.3）に違反する。
- 修正案: append の時点で strip を**検証だけ**する。たとえば crate-private のフラグ
  （`Workbook` の `pub(crate) validate_strips: bool`）を立て、`KeepStaleCaches` の分岐でも `plan_strip(..)?` を呼んで
  結果を捨てる。`CalcPolicy` の公開 variant は増やさない。そのうえで上のプローブを fixed case 21 として加える
  （両側で `OutOfOrder`、Journal は不変）。この PR でコアに手を入れない方針なら、最低限、このケースを「既知の不一致」として
  `#[ignore]` 付きの fixed case で固定し、C3 の受け入れ条件に「遅らせた strip も書き込み時に検証する」と明記する。
  どちらにするかは owner が決める。

### m1（Minor）: OQ1 について。style op の 10 倍は J0 の Major ではなく J1 の前提条件。(b) を先に、(a) は C2 の follow-up
- 根拠: `mod.rs:309-312` は、内容が snapshot と等しく `s` だけが違うセルに毎回 `restore_cell(.., Current)` を呼ぶ。
  style-only の op はほぼ必ずこの分岐に入る（プローブ: 256 ケースで content restore が 409 回、full restore が 177 回）。
  そのたびに C2 が `style_sensitive` を捨て、次の style 書き込みで全 formula を再走査する。
  正しさには影響しない。oracle 10 には CI の閾値がなく、J0 の役目は測ることなので、J0 の Major にはしない。
  ただし §4.5 の「2–3 µs/op (5%)」とは大きく食い違う（mixed で 357 対 72 µs/op、5 倍）。J1 は C2–C8 の上で
  この目標を示す必要がある（§7.3）ので、J1 の前に直す。
  - (b) は健全: style-only の op（`fields` に `Content` がない）はセルの内容に触れない。直前の R2 が内容を
    snapshot のものに戻しているか、セルが初めて触られたかのどちらかなので、内容が等しいときの content restore は、
    内容部分については必ず no-op になる。実際に `write_back` に「op が `Content` を書かないなら
    `Current` の restore を省く」を入れると、journal の全テスト（oracle 1 の 256 ケースを含む）は green だった。
    判定は O(1) で、Journal の中だけで済む。
  - (a) はより一般的: 定数の write-back で起きる full restore も cache を捨てる。こちらは C2（`snapshot.rs`）の変更なので、
    別タスクにする。
- 修正案: (b) をこの PR か J0b で入れ、oracle 10 を測り直す。(a) は C2 の follow-up として J1 の前に入れる。

### m2（Minor）: 生成器の coverage テストは、R2 の restore が実際に起きていることを確かめていない
- 根拠: `tests/oracle1.rs:379-402` が確かめるのは、op の族が 5 つ出ることと、ok/refused の件数だけ。
  R2 を通らなくなっても（revert が効かない、hot cell の値が変わった、など）このテストは green のままなので、
  oracle 1 は R2 について空振りになる。いまは十分に届いている（プローブ: full restore 177 回。内訳は Value 114、
  Formula 59、Shared 4、Inline 0）。ただし文字列の full restore は 256 ケースで 4 回しかなく、`Inline` と
  present かつ `Empty` は 0 回だった。
- 修正案: `Journal` に `#[cfg(test)]` のカウンタ（full / content restore を Content の種類ごとに数える）を置き、
  coverage テストで「full restore > N、content restore > N、Shared ≥ 1、Formula ≥ 1」を下限として確かめる。
  inline の revert を生成器に足す（たとえば Base の `t="s"` セルへの `StringInline` → `String`）。

### m3（Minor）: P がセルの `cm`/`vm` と `f` のグループ以外の属性（`ca`, `aca`, `dt2D`, `r1`, …）を落とす
- 根拠: `project.rs:430-435` の `FormulaKind` が持つのは `t="array"`/`"dataTable"` と `ref` だけ。
  `project.rs:466-469` は `cm`/`vm` を h1 から外す。§4.4 の raw equality（`state.rs:112-122`）はこれらを比べるので、
  R2 は正しい。ただし `state.rs` が `ca` や `cm` を無視する変異は、oracle 1 では見えない
  （fixture にこれらの属性がないため、どのみち到達しない）。決定 A4 の範囲内で、`cm`/`vm` は J0b に回っている。
- 修正案: J0b で `cm`/`vm` を解決するときに、`f` の残りの属性（`t`/`ref`/`si` 以外）を `FormulaKind` に入れ、
  `ca="1"` を持つ formula を fixture に 1 つ足す。

### n1（nit）: 拒否の経路で `cell_key` が軸ベクトルを遅延生成する
- `mod.rs:195-196` は `apply_seq` の前に `cell_key`（`rows`/`cols` への `entry().or_insert_with`）を呼ぶので、
  拒否された op でも `rows`/`cols` が増える。恒等ベクトルなので意味は変わらず、`JournalState` も比べていない。
  ただし J2 で「拒否ではベクトルも元のまま」（§4.5）を検証するときに紛らわしい。`open` で全シートのベクトルを作るか、
  拒否の後に読むだけの参照に変える。

### n2（nit）: `Journal::open` が `cell_format_count()` の `Err` を 0 として飲み込む
- `mod.rs:98` と `:174`。styles が壊れていると P2 の判定が黙って無効になる（逐次側も拒否するはずなので実害はない）。
  OQ2（open で styles registry を起こすと 25 ms）と合わせて、`cellXfs` の子の数を XML から直接読み、
  `Err` は `open` の `Err` にするとよい。

## 確認したこと（問題なし）
- 拒否 ⇒ 状態不変: `fresh` はローカルで、`apply_seq` の `Err` で捨てられる。seq、log、key はすべて `Ok` の後に積む
  （`mod.rs:224-269`）。P1/P2 は shadow に触れる前に `Unsupported` を返す。snapshot の失敗と適用の成功が重なると `broken`。
- hand-off target: `plan_handoff`（`edit.rs:696-735`）と同じ絞り込みと並び（`ambiguous_group` を除き、同じ `si` の
  生きた member の最小 `CellIndex`）。style-only の op では `edit.rs:383` が `target = None` にするので、hand-off は起きない。
  これは `writes_content` の絞り込みと一致する。
- §4.4 equality: sst は追記だけ（`registry/strings.rs:222-252`）なので、index が違っても Tree が同じなら restore して安全。
  `vm` を持つセルへの内容の書き込みは `plan_target` が拒否する（`worksheet.rs:292`）ので、`vm` を比べなくても偽の equal は起きない。
  member と master は `f` 属性の違いのせいで、snapshot の状態に戻ることがない（round 8 M1）。
- key index: 名前の付いた target にだけ、`fields` の group ごとに積む。hand-off のセルには積まない。
  `write_cells` は重複を除いてセルごとに `Content` を積む。
- ids/AxisVec: insert は末尾の n 個を押し出して殺し、delete は殺した数だけ末尾に `New` を足す。どちらも §5.2 どおり。
  counter は Journal に 1 つで、行と列で共有している。`New` が衝突することはない。
- 変異（手で入れて、テストを実行してから戻した）: state が `Value.t` を無視 → oracle 1 で落ちる。`cur == state` を
  `present && content` に弱める → fixed 07/12/20、keys、r2、oracle 1 で落ちる。P のセルの style を xf 0 に固定 →
  P の self-test (a) で落ちる。
- 他の open PR: #158（C8）は `lib.rs` に 1 行足すだけで衝突し、それ以外は重ならない。C8 が入ったら、`state.rs` を C8 に置き換える。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 3 / nit 2）
