# ADR-0024 レビュー（Opus, round 5）

対象: `plan/xlsx-edit-journal` の未コミットの `docs/adr/0024-xlsx-edit-journal.md`（revision 5）。
PR はまだ無い。コードは `main` の `f88d02c` で照合した。プローブは `ooxml-xlsx` を path 依存にした使い捨てのバイナリで、コミットしていない。

## 結論（先に）

round 4 の M1、M2、m2、m3、n1、n2 は解消した。M2 の新規則（`dimension` は作業コピーの逐次規則に任せ、P から外して述語で比べる）は、プローブで固定ケースと N3 の移動の両方を確かめた。oracle 1・4・5 は同時に満たせる。

ただし、新しい Major が 1 つある。

- M1: §9.2 は、スタイルの書き込みを「resolved `xf` equals the cell's current `s`」のとき no-op として省く。作業コピーにセルが無いとき、この規則は定まらない。`s` 属性の既定値 0 と読むと、列の既定スタイルの上の `set_style_index(0)` が省かれ、`Ok` のまま、セルの書式が変わる。

round 4 の m1 の直し（metadata の述語を Journal 側だけにする）は、Journal 側でも偽になる（m1）。

**converged: no**

## 前ラウンドの解消状況

- M1: 解消。§7.1 の表で、conditional な内容 op に後の item は加わらない。スタイルのパッチの行も、conditional な内容 op への併合を除く。round 4 のプローブの両方の入力は、別々のバッチになる。結果は逐次と同じ `s="1"`。
- M2: 解消。プローブ（Base の `dimension` は `A1:C5`）で、次を確かめた。
  - `D10` への書き込み → `delete_rows(10, 1)`: 逐次は `A1:D9`。Journal は N4 で書き込みを捨て、`A1:C5` のまま。どちらも述語を満たす。
  - `D10` への書き込み → `insert_rows(7, 1)`: 逐次は `A1:D11`。N3 で書き込みを後ろへ動かした順（挿入 → `D11`）も `A1:D11`。
  - 構造操作による範囲の書き換えは単調である。範囲が全部消えると `A1` になる（`sheet_ops/grid/features.rs` L521-522）。そのため、生き残った書き込みセルを覆う、という述語は、どちらの順でも成り立つ。
  - round 4 のプローブ（`dimension` は `A1`、`C3` へ 7 を書き戻す）では、逐次は `A1:C3` になる。Journal は書き込みを省くので Base のまま。oracle 5、4、1 がそろって通る。
- m1: 規則は入った。ただし Journal 側でも偽になる（新しい m1）。
- m2: 解消（conditional な最初の op は、土台の判定で読み飛ばす）。
- m3: 解消（two-half op は、セルがまだバッチに無いときだけ加わる）。
- n1、n2: 解消。

## Findings

### M1（Major）: §9.2 のスタイルの no-op は、作業コピーにセルが無いと定まらない。`s` を既定値 0 と読むと、`Ok` のまま書式が変わる

- 根拠:
  - §9.2: 「a style batch entry whose resolved `xf` equals the cell's current `s`」は省かれる。セルが無ければ `s` も無い。`CT_Cell/@s` の既定値は 0 で、既存セルの読み方（`registry_cell_state`、`edit.rs` L187-194）も、属性が無ければ 0 とする。
  - 一方、逐次では、存在しないセルへのパッチがセルを作る。§7.1 も「if it carries a style half …, it creates the cell with the row/col default」と書く。no-op として省くと、セルは作られない。
  - プローブ: Base の列 B は `col/@style="1"`（太字）で、`B5` は存在しない。`set_style_index(0)` を逐次で呼ぶ。
    - 逐次: `<c r="B5" s="0"/>` ができる。書式は既定（太字なし）。
    - §9.2 を「current `s` = 0」と読んだ Journal: resolved `xf` 0 = 0 なので省く。`B5` は無いままで、列の既定（太字）で表示される。`Ok` のまま、P の resolved style が食い違う。
  - 別の読み方（current は `missing_cell_style`）でも、問題は残る。列の既定と同じ `xf` を当てるパッチが省かれ、逐次側にだけ空の `c` が残る。P が空のスタイル付き `c` をモデルに入れるかどうかは、どこにも書かれていない。
  - 経路は普通に起きる。ADR-0023 の read → modify → write-back で、書式を既定に戻す場合である。oracle 1 の生成器は、`col/@style` の下の存在しないセルと full パッチを含んでいる。
- 修正案:
  - §9.2 に「作業コピーに無いセルへの entry（内容でもスタイルでも）は no-op ではない」と足す。
  - P に、空の `c` の扱いを 1 行足す。
  - oracle 1 に固定ケース（列の既定の上で、存在しないセルに `set_style_index(0)`）を足す。
  - oracle 9 に「存在しないセルを `s=0` として no-op 判定する」変異を足す。

### m1（Minor）: metadata の Journal 側の述語「a Base without the part gains one only if some `c` references a block」は、規則どおりの Journal でも偽になる

- N4 が entry を捨てるのは、直後の構造操作 `k+1` が id を殺すときだけである。2 つ目以降の構造操作が消すセルの entry は、捨てられずに書かれる。
- プローブ: metadata の無い Base で、次を逐次に実行した。
  1. `A5` に `set_dynamic_formula("_xlfn.SEQUENCE(B30)")`
  2. `insert_rows(20, 1)`
  3. `delete_rows(5, 1)`

  結果、`xl/metadata.xml` が残り、`cm` を持つ `c` は無い。
- Journal 側でも、entry は epoch 0 に留まる。N3 (ii) により、挿入が書き換える参照 `B30` を持つからである。epoch 1 から見た N4 は挿入で、何も殺さない。そのため、Journal の出力も逐次と同じになり、Journal 側の述語が落ちる。
- 修正案: 述語を「`c` から参照されないブロックを持つ新しい part は、その block を作ったセルが後の構造操作で消えた場合だけ」とする。あるいは、part の有無は両側とも述語から外し、参照の範囲だけを確かめる。

### m2（Minor）: `dimension` の Journal 側の述語「a sheet with no written cell and no structural op has the Base's bytes」と、§7 step 4 の同じ文は、barrier op の書き込みを数えていない

- 次の 2 つの barrier op は、`plan_cell_writes` → `plan_target` を通る。そのため、どちらの側でも `dimension` が広がる。
  - `set_hyperlink` に `text` を付けたもの（`hyperlink.rs` L218-221）
  - `merge_clearing` の隠れたセル（`range.rs` L199-201）
- プローブ:
  - Base の `dimension` が `A1:C5` で、`D10` に `text` 付きのハイパーリンクを置いた: `A1:D10` になる。
  - Base の `dimension` が `A1` で、`merge_clearing("B3:C3")` を実行した: `A1:C3` になる。
- 「written cell」を keyed entry だけと読むと、正しい実装が落ちる。また「the Base's bytes」をシートの part と読むと、他のシートの書き込みによる cache の削除、barrier op、行・列の性質の op でも落ちる。
- 修正案: 「written cell」を「Commit が実行した呼び出し（barrier op を含む）が書いたセル」と定める。「bytes」は `dimension` 要素のものだと書く。

### m3（Minor）: `dimension` の固定ケースは単一セルの `A1` なので、SheetJS の `!ref` に届かない

- SheetJS CE 0.20.3 は、`dimension/@ref` をコロンを含む範囲のときだけ `!ref` に使う（`xlsx.js` の `dimregex = /"(\w*:\w*)"/`、L15802）。`A1` のときは、セルから `!ref` を計算する。
- そのため、§6.1 の `!ref` の例外（session 中の View は広く、reload 後は狭い）は、範囲の形をした古い `dimension` でしか観測できない。例えば、`A1:C5` の外に Base の `D10` があり、それを書き戻す場合である。このとき、reload 後の `sheet_to_json` は `D10` を返さない。
- §6.1 の「it covers every cell the session changed」も、文字どおりには偽である。古い `dimension` の下では、構造操作で動いた Base のセルや、cache を削られたセルは覆われない（逐次も同じ）。
- 修正案:
  - 固定ケースか生成器に、範囲の形をした古い `dimension` を足す。
  - 差分ハーネスに、範囲の形をした古い `dimension` の外にあるセルを書き戻すケースを足す。
  - §6.1 の文を「every cell the session wrote」に直す。

## nit

- n1: Revision 5 の M1 の項は「the conditional op's cell is alone in its batch」と書く。一方、§7.1 の表は「no later item for `c` joins it」で、他のセルは同じバッチに入れる。項の文言を表に合わせる。

## その他の確認結果（指摘なし）

- **conditional な op の単独化と依存**: `remove_formula` は `f`、`v`、`t`、`is` をすべて消す（`worksheet.rs` L447-449、L759-766）。そのため、同じバッチの他のセルの書き込みによる cache の削除の影響を受けない。N2 は保たれる。
- **two-half op**: two-half op が入ったバッチに、同じセルの内容やパッチが後から加わる経路は無い。内容の行は `StyleOnly` を、パッチの行は「no style request」を条件にしている。
- **受け入れ条件**:
  - proptest の等式: M1 と m1 の固定ケースが要る。
  - View/Reload 法則: `!ref` の例外と矛盾しない（m3 は観測の問題）。
  - `Err` ⇒ byte-identical: 矛盾は無い。
  - 「受理されたものは必ずコンパイルできる」: 作業コピーの `dimension` が shadow と違っても、拒否は増えない。範囲が狭い側で新たに拒否する経路は見つからなかった。`dimension` が範囲でないときの `Malformed` は、両側で同じに起きる。
- **Q1〜Q6**: revision 4 から変更なし。選択肢・既定案・理由がそろっている。
- **公開リポジトリの規則**: 本文に、ローカルパス、ユーザー名、非公開プロジェクト名は無い。数値はすべて ADR か出典に結びついている。引用した行番号（`edit.rs` L373-376、L419-421）は `f88d02c` と一致する。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 3 / nit 1）
