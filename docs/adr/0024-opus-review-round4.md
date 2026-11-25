# ADR-0024 レビュー（Opus, round 4）

対象: `plan/xlsx-edit-journal` の未コミットの `docs/adr/0024-xlsx-edit-journal.md`（revision 4）。
PR はまだ無い。コードは `main` の `f88d02c` で照合した。プローブは 2 種類で、どちらもコミットしていない。

- `ooxml-xlsx` を path 依存にした使い捨てのバイナリ
- `git archive` で取り出したコピーに足した使い捨ての単体テスト（crate 内部の `edit_cell_ext` を呼ぶため）

## 結論（先に）

round 3 の M1 と M2 は、規則としては解消した。round 3 のプローブ X/Y は、revision 4 の規則で逐次と同じ `xf` になる。動的配列と行削除の例も、P の比較では一致する。

ただし、新しい Major が 2 つある。

- M1: §7.1 の併合規則は、書式パッチを conditional な内容 op（`remove_formula`）の書き込みに併合してよいと読める。ところが、数式の無いセルでは、計画が書き込みを丸ごと飛ばす（`edit.rs` L419-421）。そのため、パッチが `Ok` のまま消える。
- M2: §7 step 4 は、`dimension` を shadow の値に合わせる。この規則は、§9.2 の no-op の省略と矛盾する。Base の `dimension` がすべてのセルを覆っていない場合、oracle 1 と oracle 5 を同時に満たせない。

**converged: no**

## 前ラウンドの解消状況

- M1: 解消。two-half op は畳まず、存在しないセルの土台は畳む前の最初の op で決める（§4.4、§7.1）。
  - X（`set_datetime` → `set_number`）: N1 は two-half op を越えないので、2 つの op が順に残る。最初の op がスタイルの半分を持つので、列の既定の上で `EnsureDate` を解決する。結果は `xf{14, fontId=1}` で、逐次と同じ。
  - Y（`set_number` → `set_datetime`）: 内容を単独で先に書き、`set_datetime` は別のバッチになる。`s=0` の上で解決し、結果は `xf{14, fontId=0}` で、逐次と同じ。
  - full パッチが最初のパッチを落とす場合も、`Format` は current を読まない（`registry/styles.rs` L498）。どちらの土台でも同じ `xf` になる。
- M2: P の比較としては解消した。ただし、述語の 1 つが、自分の固定ケースの逐次側で偽になる（m1）。
- m1: 解消（§4.5 と §11 がそろった）。§12 の表の文言だけが残る（n1）。
- m2: 解消。§4.5、§9.3、oracle 3、oracle 6 の間で、戻すものの列挙とカウンタの扱いが一致する。
- m3: 規則は入った。ただし新しい M2 を生む。数式セルの value と `t` の扱いは解消した。
- n1: 解消。

## Findings

### M1（Major）: 書式パッチを `remove_formula` の書き込みに併合すると、数式の無いセルではパッチが `Ok` のまま消える

- 根拠:
  - §7.1 の表は、次の 2 つを定めている。
    - 「style patch … joins when `c`'s write there has content and no style request and the key's start state is not `Missing`: they merge」
    - 「conditional content entry: never for a cell already in the batch」

    後者は、conditional な内容 op が後から加わる場合だけを禁じる。conditional な内容 op が先にバッチにあり、そこへパッチが加わる場合は、前者によって併合になる。
  - `plan_cell_writes` は、`RemoveFormula` の対象に数式が無いと `continue` する（`edit.rs` L419-421）。この `continue` は `extra.style` の処理より前にあるので、同じ書き込みのスタイル要求も一緒に捨てられる。
  - 規則に合う入力は 2 種類ある。
    - 開始状態が `Other`（定数）のセルに `remove_formula` → `set_fill`。
    - 数式セルに `set_number` → `remove_formula` → `set_fill`。N1 によって、`remove_formula` は後のバッチに置かれる。その時点で数式はもう無い。
  - 範囲に `write_cells` の `RemoveFormula` を当ててから書式を付ける形は、ADR-0023 の group-wise clear で普通に起きる。
- プローブ: Base の `C3` は定数 7、`xf 1` は太字。
  - 逐次（`edit_cell(RemoveFormula)` → `edit_cell_ext(StyleOnly, Index(1))`）: `<c r="C3" s="1"><v>7</v></c>`
  - §7.1 の併合（`edit_cell_ext(RemoveFormula, Extra { style: Index(1) })`）: `Ok(())` が返るが、結果は `<c r="C3"><v>7</v></c>`

  P の resolved style が食い違う。`Ok` のまま、黙って誤る。
- 修正案:
  - §7.1 の表の「style patch」の行に「`c`'s write is not a conditional content op」を足す。
    - 別案: conditional op を、常に単独のバッチにする。
  - oracle 1 に固定ケースを足す（定数セルに `remove_formula` → `set_fill`、数式セルに `set_number` → `remove_formula` → `set_fill`）。
  - oracle 9 に「conditional op にパッチを併合する」変異を足す。

### M2（Major）: `dimension` を shadow に合わせる規則（§7 step 4）は、no-op の省略（§9.2）と矛盾する。Base の `dimension` が古いと、oracle 1 と oracle 5 を同時に満たせない

- 根拠:
  - 逐次 API は、書き込むセルが `dimension` の外にあると、`dimension` を広げる。セルがすでに存在するか、値が同じかは見ない（`worksheet.rs` L306-317 の `plan_target`。`StyleOnly` も同じ）。
  - shadow は逐次の状態なので、値を変えない書き戻しでも `dimension` が広がる。
  - 一方、§9.2 はその entry を省き、oracle 5 は「write the Base value」の Journal がバイト単位で Base と同じになることを求める。oracle 4 も「skipped no-op entries contribute no row」とする。
  - §7 step 4 に従って `dimension` を shadow の値に合わせると、oracle 5 と oracle 4 が落ちる。合わせなければ、P の `dimension/@ref` で oracle 1 (iii) が落ちる。規則どおりに実装すると、どちらかが必ず落ちる。
  - 現実には、ADR-0021 の SheetJS の規則（「reverting a value removes that edit」）と、ADR-0023 の read → modify → write-back で起きる。この 2 つは、§9.2 が GetPut として約束した経路である。Base の `dimension` がすべてのセルを覆っていないだけで、未変更のセルの書き戻しが出力を変える。
- プローブ: Base の `dimension` は `A1`、`C3` は定数 7。`C3.set_number(7)` を逐次で 1 回呼ぶ。
  - 結果: 出力は Base と一致しない。差は `<dimension ref="A1:C3"/>` だけで、セルは `<c r="C3"><v>7</v></c>` のまま。
  - Journal の経路では、§9.2 が entry を省き、§7 step 4 が `A1:C3` を書く。そのため oracle 5 が落ちる。
- 修正案: 次のどちらかを決めて書く。
  - P の `dimension` を除外表に移し、述語を次のようにする。両側とも「`dimension` がすべての `c` を覆う（あるいは Base の値のまま）」。Journal 側は「書いた entry か構造操作が無いシートでは Base のまま」。あわせて、§6.1 の Reload 法則に `!ref` の例外を書く。
  - §7 step 4 を「書き込まれた entry または構造操作があるシートだけ」に限る。同時に、oracle 1 の P は、no-op だけのシートの `dimension` の差を許す。

  どちらの場合も、古い `dimension` を持つ fixture を oracle 1・4・5 の生成器に入れる。

### m1（Minor）: metadata の述語「a side without a referenced block adds no part」は、自分の固定ケースの逐次側で偽になる

- 固定ケース（`set_dynamic_formula("1+1")` at `A5` → `delete_rows(4, 1)`）では、逐次側に `xl/metadata.xml` が残る。`c/@cm` で参照されないブロックを持つ part である（round 3 のプローブ出力 `dyn-seq.xlsx` で確認）。
- 述語は**両側**で確かめると書いてあるので、逐次側が落ちる。一方、固定ケースには「both pass」と書いてある。
- 修正案: この述語を Journal 側だけに当てる。逐次側には「参照されるブロックは範囲内」だけを当てる。

### m2（Minor）: 存在しないセルの最初の op が conditional な内容 op のとき、§7.1 の「every patch resolves on 0」は偽である

- `remove_formula` は、存在しないセルを作らない（`edit.rs` L419-421 の `continue`）。そのため逐次では、後のパッチは列・行の既定の上で解決される。
- 実装が「内容を単独で先に書く」に従って作業コピーで実行するだけなら、結果は正しくなる。しかし、本文のとおりに土台を 0 に固定すると誤る。
- 修正案: 「content only」を「a content op that creates the cell」に直す。conditional な op は、土台の判定で読み飛ばす。

### m3（Minor）: two-half op が、`StyleOnly` の書き込みがあるバッチに加われるかが書かれていない

- §7.1 の表の「content entry … joins when `c`'s write there is `StyleOnly`」を two-half op に当てると、1 つの書き込みにスタイル要求が 2 つ入る。`Extra { style }` は 1 つしか持てない。
- 修正案: 「a two-half op joins only when `c` is not yet in the batch」と書く。

## nit

- n1: §12 の `JournalInternal` の行は「a bug」と書く。一方、§11 は、フラッシュの拒否に「classifier cannot see」状態依存の拒否（`StyleTableFull`）も含める。§12 の行に「or a refused flush」を足す。
- n2: §7 step 2 は、shadow から `dimension` を記録する。shadow が truncate のあとで stale のときは、先に replay することを書く。

## その他の確認結果（指摘なし）

- **§4.4 の副作用の一覧**: 漏れは無かった。
  - `sharedStrings.xml` も `styles.xml` も、内容 op では作られない。
    - 共有文字列の表が無いブックでは、文字列はインラインのまま書かれる（`registry/strings.rs` 冒頭）。
    - styles の part が無いブックでは、パッチは `NoStylesPart` で拒否される。
    - したがって、N1・N4 で捨てた entry が part の集合に差を残すのは、metadata だけである。
  - `vm` のセル（richData）への内容 op は `RichValueCell` で拒否される。動的配列のアンカーへの内容 op は `ArrayAnchor` で拒否される。
  - テーブルの見出しへの書き込みは `TableHeaderCell` で拒否される。
  - comments は barrier op なので、捨てられない。
  - `calcPr` は calc-state の除外に入る。
- **two-half op と N3 (iv)**: 存在しないセルで two-half op が最初の op のとき、その op は構造操作を越えて動ける。しかし、生き残る id の `missing_cell_style` は、行の挿入・削除でも列の挿入・削除でも変わらない（行の `s` は id とともに動き、`col` の範囲は生き残りを覆ったまま）。そのため誤りは無い。
- **id カウンタを戻さない規則**: replay は、記録した `New` id を再利用する。1 op の拒否では id を割り当てない。そのため、oracle 3 の「id counter equal」（1 op）と「may only have grown」（途中拒否）、oracle 6 は両立する。
- **受け入れ条件**: 次の 3 点を除けば、検証できる形で書かれている。
  - proptest の等式: M1・M2 のケースを生成器に入れる必要がある。
  - View/Reload 法則: M2 の `!ref` の例外が要る。
  - `Err` ⇒ byte-identical: 矛盾は無い。
  - 「受理されたものは必ずコンパイルできる」: 新しい規則によって Commit の拒否が増える経路は見つからなかった。M1 は拒否ではなく、黙った誤りである。
- **Q1〜Q6**: 変更なし。選択肢・既定案・理由がそろっている。
- **公開リポジトリの規則**: 本文に、ローカルパス、ユーザー名、非公開プロジェクト名は無い。数値はすべて ADR か出典に結びついている。
- **open PR**: #151（census の遅延化、ADR-0010 stage 2）は、§7 step 3 の open のコストに関係する。ただし、規則とは衝突しない。

## 判定

converged: no（Blocker 0 / Major 2 / Minor 3 / nit 2）
