# ADR-0024 レビュー（Opus, round 6）

対象: `plan/xlsx-edit-journal` の未コミットの `docs/adr/0024-xlsx-edit-journal.md`（revision 6）。
PR はまだ無い。コードは `main` の `f88d02c` で照合した。プローブは `ooxml-xlsx` を path 依存にした使い捨てのバイナリと、
ADR-0021 の検証で使った ExcelJS 4.4.0 で、どちらもコミットしていない。

## 結論（先に）

round 5 の M1、m1〜m3、n1 はすべて解消した。revision 6 の変更（存在しないセルは no-op にしない、空の `c` をセルとして扱う、
metadata の述語を弱める、barrier op の書き込みを `dimension` に数える）を壊しにいったが、`Ok` での黙った誤り、
oracle 同士の矛盾、検証できない重要な経路は見つからなかった。

残るのは Minor 3 件と nit 1 件で、どれも文言か仕様の 1 行で直せる。

**converged: yes**

## 前ラウンドの解消状況

- M1: 解消。§9.2 は「`c` が作業コピーにあるとき」だけ no-op とし、存在しないセルの entry は必ず書く。
  プローブ: 列 B が `col/@style="1"`（太字）で `B5` が無い Base に `set_style_index(0)` を逐次で実行すると、
  `<c r="B5" s="0"/>` ができ、`dimension` は `A1:B5` になる。ADR の固定ケースの期待値と一致する。
  oracle 9 の変異（s=0 または row/col 既定と等しければ省く）も入った。
- m1: 解消。round 5 のプローブ（`SEQUENCE(B30)`、`insert_rows(20, 1)`、`delete_rows(5, 1)`）で、
  `xl/metadata.xml` は残り、`cm` を持つ `c` は無い。新しい述語（参照される `cm`/`vm` はすべて範囲内、
  part が新しくできるのはその側の log に `set_dynamic_formula` があるときだけ、参照されない part や block は残ってよい）は、
  両側で成り立つ。
- m2: 解消。「written cell」に barrier op の書き込みが入り、「Base bytes」は `dimension` 要素のバイトになった。
  プローブ: `set_hyperlink` に `text` を付けて `D10` に置くと `A1:D10`、`merge_clearing("B3:C3")` で `A1:C3` になる。
  どちらも述語を満たす。
- m3: 解消（固定ケースが範囲の `A1:A2` を使う。§14 に差分ハーネスのケースが入った。§6.1 は「every cell Commit wrote」）。
- n1: 解消。

## Findings

### m1（Minor）: P1（clear）は、存在しないセルの上で何をするかが決まっていない。`CellOp::Clear` をそのまま使うと、列や行の既定の書式が落ちる

- 根拠:
  - §4.1 の P1 は「clear a cell's value and formula, keeping its style」で、中身は「a public write over the existing `CellOp::Clear`」。
  - `plan_cell_writes` は、存在しないセルのときに `RemoveFormula` だけを飛ばす（`edit.rs` L419-421）。`Clear` は飛ばさないので、
    `write_cell` が `<c r="…"/>` を `s` なしで作る。§4.4 も「a content op … creating a cell, which then has no `s`」と書き、
    revision 6 の §9.2 は「an entry on a missing cell is never a no-op」とする。
  - その結果、列が太字のとき、存在しない `B5` に P1 を当てると、`Ok` のまま `B5` は太字でなくなる。
    これは「keeping its style」と矛盾する。Q3 の表（clear (P1): h2 keep）とも合わない。
  - ExcelJS 4.4.0 のプローブ: 同じ Base で `getCell('B5').value = null` → `writeFile` は `<c r="B5" s="1"/>` を書き、太字を保つ。
  - 逆に、P1 を「存在しないセルでは何もしない」と定めると、N1 の表で P1 を「unconditional full」とする分類が誤りになる。
    たとえば `set_number` → P1 の折りたたみで、逐次では空の `c` が残り、Journal ではセルが無いままになる。
    revision 6 から、P は空の `c` と存在しないセルを区別するので、oracle 1 が落ちる。
  - P1 は Part B で、J1 の前提条件である。どちらに決めても oracle 1 が食い違いを見つけるので、黙った誤りにはならない。
    そのため Minor とする。
- 修正案:
  - P1 の行に、存在しないセルのときの動作を書く。案は「何も作らない」で、ExcelJS と Q3 に合う。
  - その場合は、N1 の表で P1 を conditional の行へ移す。
  - oracle 1 に固定ケース（太字の列で、存在しないセルに P1、および `set_number` → P1）を足す。

### m2（Minor）: `dimension` の「covers every surviving cell the side wrote」は、何も変えない `remove_formula` を書き込みに数えると、両側とも落ちる

- 根拠:
  - `remove_formula` は、数式の無いセルでは `plan_target` の前で `continue` する（`edit.rs` L419-421）。
    そのため、セルがあっても無くても、`dimension` は広がらない。
  - プローブ: `dimension` が `A1` の Base で、`dimension` の外にある定数セル `C3` と、存在しない `D10` に `remove_formula` を実行した。
    どちらも `Ok` で、シートの part は入力とバイト単位で同一だった。
  - 述語は「a written cell being one written by any call the side ran」と書く。「呼んだ op の key」と読むと、逐次の参照も正しい Journal も落ちる。
    生成器は、`dimension` が全セルを覆わない fixture と、Base の定数への `remove_formula` を含むので、この経路に当たる。
- 修正案: 「written cell」を「その呼び出しの plan が target にしたセル（`plan_target` を通ったもの）」と定める。
  conditional な op が飛ばしたセルは含めない、と 1 行足す。

### m3（Minor）: metadata の P は、参照される block を集合として比べる。そのため、セルが別の block を指す誤りを見逃しうる

- 根拠:
  - P は「the metadata blocks referenced by some `c/@cm` or `c/@vm`, by content」を持つ。
    revision 6 の述語は、範囲内への解決だけを確かめる。
  - Base に `fCollapsed="1"` と `fCollapsed="0"` の XLDAPR block があり、両方が既存のセルから参照されているとする。
    このとき、Commit が新しい動的配列セルの `cm` を誤って collapsed の block に向けても、参照される block の集合は変わらない。
    さらに、P の「kind」が `fDynamic` だけを見る作りなら、この誤りは P を通る。
  - 今のコードでは、`ensure_block`（`dynamic.rs`）が最小の uncollapsed block を決定的に選ぶ。
    また、動的配列の anchor への書き込みは `ArrayAnchor` で拒否される（`edit.rs` L393-397）。
    そのため、`main` の上でこの誤りへ至る経路は見つからなかった。
- 修正案: P の metadata を「セルごとに、解決した block の内容」として持つ。集合ではなく、セル → block の写像にする。

## nit

- n1: §7 step 4 は、batch を「each one `CellWritePlan` per sheet」と書く。
  一方、`set_dynamic_formula` は metadata part の登録（`apply_writes_with(plan, registry)`）を伴う。
  `plan_writes` で作る batch には入らないので、単独の呼び出しにするか、内部で合成するかを一言書く。

## その他の確認結果（指摘なし）

- **空の `c` と oracle 4・5**:
  - 逐次だけが空の `c` を作る経路を探したが、見つからなかった。
    - `remove_formula` は、存在しないセルに何も作らない。
    - `merge_clearing` が `Clear` するのは、値のある隠れたセルだけで、両側に同じ `<c r="C3"></c>` が残る。
    - N1 と N1s の折りたたみは、セルを作る op を残す。
  - no-op の省略は、セルがあるときだけである。そのため、oracle 5（Base のバイトを保つ）と、P の「空の `c` はセル」は両立する。
  - oracle 4 では、新しい `c` と `row` の作成は allowed-diff に入る。
- **書き込みも構造操作も無いシートの `dimension`**: プローブ（`dimension` は `A1:C5`）で、Base のバイトが保たれることを確かめた。
  - 範囲外の `set_row_height(19, …)`、`set_column_width`、`set_autofilter("A1:F20")` は、どれも `<dimension ref="A1:C5"/>` を変えない。
  - 行・列の性質の op と範囲の feature は、`dimension` に触れない。
- **「新しい part は `set_dynamic_formula` があるときだけ」**:
  - metadata part を作るのは `dynamic.rs` の `set_dynamic_formula` だけである（`REL_METADATA` を追加するのはここだけ）。
  - 拒否された呼び出しは `Err` ⇒ byte-identical なので、part を作らない。
  - 動的配列の anchor への上書きは `ArrayAnchor` で拒否される。そのため、N1 が `set_dynamic_formula` を畳む経路は無い。
  - 述語は逐次側でも成り立つ。
- **`vm` を持つセル**: 内容の書き込みは `RichValueCell` で拒否される（`worksheet.rs` の `plan_target`）。
  そのため、no-op の判定が `vm` を持つセルに及ぶことは無い。
- **受け入れ条件**:
  - proptest の等式: round 5 の M1 と m1 の固定ケースが入った。m1 と m2 の直しは、それぞれ固定ケース 1 つで済む。
  - View/Reload 法則: `!ref` の例外と矛盾しない。
  - `Err` ⇒ byte-identical: 変更なし。
  - 「受理されたものは必ずコンパイルできる」: 存在しないセルの entry を必ず書くことにしても、
    書くのは shadow で受理された呼び出しと同じものなので、拒否は増えない。
- **Q1〜Q6**: revision 5 から変更なし。選択肢・既定案・理由がそろっている。
- **公開リポジトリの規則**:
  - 本文に、ローカルパス、ユーザー名、非公開プロジェクト名は無い。
  - 数値はすべて ADR か出典に結びついている（289 ms、0.29 s、68.8 ms ほか）。
  - 引用した行番号（`edit.rs` L373-376、L419-421）は `f88d02c` と一致する。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 3 / nit 1）
