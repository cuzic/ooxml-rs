# ADR-0024 レビュー（Opus, round 9）

対象: `plan/xlsx-edit-journal-r8` の `docs/adr/0024-xlsx-edit-journal.md`（revision 8、round 8 対応後、コミット `9c70b74`）。
round 8 の Owner response を照合し、文書全体を独立に読み直した。コードは `main` の `f88d02c` 相当の
`ooxml-core` / `ooxml-xlsx` で照合した。今回はプローブを走らせていない（指摘はどれも ADR の規範文とコードの読みから導ける）。

## 結論（先に）

round 8 の 6 件の Major への修正は、どれも書かれた範囲では成り立っている。

- R1 は最適化に格下げされた。R1 を外しても、出力と拒否は変わらない。
- snapshot はセルの派生状態を持つ。
- 所属は持ち越しの印と今の差分の 2 つに分かれた。
- C6 はすべてのノードの span を付け替える。
- `truncate_fast` は複数 target とグループのセルで replay に退く。
- oracle 1 に構造の向きが入った。

ただし、Major が 2 件ある。どちらも、出力のキャッシュが `Ok` のまま古くなる誤りで、spike が通らない経路にある。

- **M1**: snapshot を閉じる規則は「その構造操作のシートのセル」と「その操作が text を書き換える数式セル」に限られている。
  別シートのセルの snapshot は、構造操作をまたいで開いたまま残る。そのため R2 は、構造操作の前の
  キャッシュと派生状態を、後の epoch で戻す。round 8 の M6 で入れた構造の向きの述語も、
  逐次側の記録（操作のときに数式だったセル）に結び付いているので、この場合を見ない。
- **M2**: R3 は strip 集合から content-changed のセルを引く。§4.4 の定義では、hand-off の新しい master も content-changed になる
  （`f` の `ref` と text が変わる）。だが hand-off は新しい master の `<v>` を残す。
  そのため、新しい master の入力が変わっても、そのキャッシュは strip されない。

どちらも、規則を 1 行直せば済む。

- M1: 構造操作は、すべての開いた snapshot を閉じる。
- M2: R3 は「キャッシュを持つ数式セル」を引かない。

Q7 の推奨 (A) を変える必要は無い。ただし、どちらも (A) だけが持つ種類の誤りである。
(B) は逐次の strip をそのまま使い、snapshot も持たない。そのため、Q7 の事実に足すのが妥当である（n2）。

**converged: no**

## 前ラウンドの解消状況

- **M1（R1）**: 解消。R1 は (a) target がすべて存在し、(b) どれもグループのセルでなく、(c) C1 が「§4.4 の状態も、以後の検証が読む状態も変えない」と報告する op だけを飛ばす。
  R1 を外した場合も確かめた。op は適用され、R2 が snapshot（派生状態を含む）へ戻す。
  `set_formula` で同じ text を書いた場合、census の `has_cache` は一度 false になるが、R2 の restore が戻す。
  `sst/@count` の delta は 0 である。`dimension` は C5 により書き込みでは動かない。
  したがって、「R1 を外しても出力と拒否は同じ」は成り立つ。
  §4.5、§7.1、Consequences、Q7 の文言もそろっている。
- **M2（派生状態の restore）**: 同じエポックの中では解消。ただし、snapshot が別シートの構造操作をまたぐと、
  戻す派生状態が古い（本ラウンド M1）。
- **M3（持ち越しの印）**: 同じシートでは解消（`F20` の例は正しく動く）。別シートのセルには印が付かない（本ラウンド M1）。
- **M4（C6）**: 解消。すべてのノードの付け替えと、代わりの案（再 parse と census の作り直し）、変異、コストがそろっている。
- **M5（`truncate_fast`）**: 解消。行・列の属性の op には穴がある（m2）。
- **M6（構造の向き）**: 構造操作のときに数式だったセルについては解消。後で書き戻されて restore されたセルは対象外（本ラウンド M1）。
- **m1〜m6、n1〜n4**: 解消。

## Findings

### M1（Major）: 別シートのセルの snapshot は構造操作で閉じない。R2 は、古いキャッシュと派生状態を後の epoch で戻す。どの oracle もこれを捕まえない

- 根拠:
  - §4.4 "Epochs close snapshots" で閉じるのは、次の 2 種類だけである。
    - その構造操作のシートのセル
    - その操作が text を書き換える数式セル
  - ここで判定に使うのは、セルの**今の**ノードである。snapshot の中身は見ない。
    そのため、別シートのセルが今は定数で、snapshot が「編集したシートに触れる数式（キャッシュあり）」の場合、
    その snapshot は開いたまま残る。
  - 例（`B` を編集するシート、`A!X` を別シートのセルとする）:
    1. Base で `A!X` は `=MATCH(7,B!A:A,0)` で、キャッシュがある。
    2. epoch 0 で `set_number(A!X, 9)` を実行する。snapshot は Base の数式ノードになる。
    3. `insert_rows(B, 3, 1)` を実行する。
       - `A!X` は `A` のセルなので、snapshot は閉じない。
       - 今は定数なので、text の書き換えでも閉じない。
       - ADR-0018 §3.7 の strip 集合は、編集の前の census から求める（ADR-0018 §3.7 "computed from the census … before the edit"）。
         `A!X` はこのとき定数なので、`calc.strips` に入らない。
    4. epoch 1 で `set_formula(A!X, "MATCH(7,B!A:A,0)")` を実行する。
       - snapshot と内容が同じなので、R2 は Base のノードを、**Base のキャッシュごと**戻す。
       - 列全体の参照は、行の挿入で中身がずれる。そのため、このキャッシュは古い。
    5. 保存時の R3 の strip 集合:
       - content-changed のセルは無い（`A!X` は restore された）。
       - 構造の記録に `A!X` は無い。
       - したがって、`<f>MATCH(7,B!A:A,0)</f><v>…</v>` が古いまま `Ok` で出る。
    - 逐次側では、`set_formula` がキャッシュを落とす。
  - text が同じまま意味が変わる場合も同じ形になる。たとえば `=B!A5*2` を定数で上書きし、
    `insert_rows(B, 3, 1)` の後に、呼び出し側が epoch 1 の座標で `B!A5*2` を書く場合である
    （逐次側では、この text は旧 `A4` を指す）。
    `add_sheet`（3-D の範囲に入る）、`move_sheet`（3-D の所属が変わる）、`rename_sheet`、`delete_sheet` も、別シートの数式の text を書き換えずに値を変える。
    これらの操作でも同じことが起きる。
  - 派生状態も古いまま戻る。たとえば snapshot の数式が名前 `MyRange`（= `B!A1:A10`）を使う場合を考える。
    構造操作が名前を `B!A1:A11` に書き換えても、restore は `A1:A10` の依存の辺を戻す。
    すると、以後の R3 の依存計算と、以後の拒否が、replay していない逐次と食い違う。
    ただし replay も同じ判断をするので、oracle 12 では一致してしまう。
  - snapshot の寿命の文言にも、同じ根から来るあいまいさがある。
    - §2 と §4.4 は「before the first call **in an epoch** whose plan targets a cell」とする。
    - 一方で、snapshot は「until … a structural op closes it」まで生きるともする。
    - 前者を文字どおりに実装すると、別シートのセルは epoch 1 の最初の書き込みで新しい snapshot を取る。
      持ち越しの印は付かない（閉じていないので）。
    - 例: Base 5 → epoch 0 で 9 → 別シートの構造操作 → epoch 1 で 10 → 9。
      このセルは epoch 1 の snapshot（9）に戻るので restore され、changed set から出る。
      Base（5）とは違うのに、`dimension` が広がらない。依存先のキャッシュも strip されない。
  - oracle が捕まえない理由:
    - oracle 1 の構造の向き（round 8 M6）は、逐次側の構造操作がそのとき strip したセルの記録である。
      `A!X` はそのとき定数なので、記録に無い。
    - 述語の前半「a kept `<v>` is byte-equal to the Base's for that cell id」は成り立ってしまう。
    - 後半「no cell it reads differs in P from the Base」は id で比べる。`B` の各セルは id では変わらないので、これも成り立つ。
    - oracle 11 の noise は 1 つの epoch の中だけである。oracle 12 は replay も同じ判断をするので一致する。
    - spike は構造操作を持たない。
- 修正案:
  - §4.4 を次のようにする。「構造操作（行・列の操作もシートの操作も）は、**ブック全体のすべての開いた snapshot** を閉じる。
    閉じたとき snapshot の状態に戻っていないセルには、持ち越しの印（内容が違えば content の印も）を付ける」。
    - 戻っていたセルは、R2 がすでに restore しているので、印は付かない。
    - 失うのは、別シートのセルで epoch をまたぐ R2 だけである。これは oracle 11 がすでに「P で比べる」としている範囲に入る。
  - §2 の Snapshot の定義を「セルが開いた snapshot を持たないとき、その最初の書き込みの前に取る」にする。
    「in an epoch」は外す。
  - 代わりの案もある。「構造操作は、snapshot の数式を §3.7 の規則で調べ、strip するなら snapshot のキャッシュを none にする」。
    ただし、名前の解決を含む派生状態の付け替えも要るので、閉じるほうが単純である。
  - oracle 1 の構造の向きに、もう 1 本足す。
    「Journal の出力で `f` と `<v>` を持ち、ログの op が target にしたセルについて、
    ログの構造操作のどれかが、出力の text に §3.7 の触れる判定（あるいは ADR-0009 §12）を当てて strip するなら、`<v>` を持たない」。
    逐次側の記録に頼らない形にする。
  - J2 に上の 2 つの例を固定ケースとして足す（`MATCH` の列全体の参照と、`B!A5*2` の text が同じ例）。
    oracle 9 に「別シートの snapshot を閉じない」変異を足す。

### M2（Major）: R3 は content-changed のセルを strip 集合から引く。hand-off の新しい master は content-changed だが、キャッシュは自分の write で決まっていない。そのため、入力が変わっても古い `<v>` が残る

- 根拠:
  - R3 は「minus the content-changed cells, whose own write decided their cache (a new formula has none …; a constant has no `f`)」とする。
  - §4.4 は、content を「the formula text, kind and every `f` attribute (`t`, `ref`, `si`, …)」と定義する。
    hand-off の新しい master もこれに含まれる（「Every target … joins the set if it differs, a hand-off's new master included」）。
  - hand-off の新しい master では、`f` に `ref` と text が入る。そのため content が snapshot（member）と違い、content-changed になる。
  - ところが、`main` の hand-off は `ref` と text だけを書き、`<v>` を残す（`edit.rs` L809-822 の (iv)、`set_reference` と `set_text` のみ）。
    つまり、そのキャッシュは「自分の write」で決まっていない。
  - 例（round 8 の `o1_shared.xlsx` の形。group `C2:E3`、master `D2`、member の text は `$A{r}+{c}$1`）:
    1. `set_number(D2, 1)` を実行する。hand-off により、`C2` が新しい master になる（`=$A2+C$1`、Base のキャッシュは残る）。
    2. `set_number(A2, 99)` を実行する。
    - 逐次側: `A2` の書き込みが依存先 `C2` を strip する。`plan_strip`（L740-747）は `has_cache` があり、書いていない `C2` を対象にする。
    - Journal 側: R3 の strip 集合は、`{D2, C2, A2}` の依存先から `{D2, C2, A2}` を引いたものになる。
      `C2` は残らない。そのため `<f t="shared" ref="…" si="…">$A2+C$1</f><v>旧値</v>` が `Ok` で出る。
    - 順序を逆にしても同じである。
    - 構造操作で閉じた後も、持ち越しの content の印があれば同じになる。
  - これは round 7 の B1 と同じ形の誤り（strip 集合から引きすぎる）である。
    oracle 1 の述語の後半（読むセル `A2` が P で違う）が捕まえるのは、生成器がこの並びを引いた場合だけである。
    固定ケースは無い。round 7 M2 の hand-off の固定ケースは、入力を変えない。
- 修正案:
  - R3 の引き算をやめ、「strip 集合のうち、保存時に `f` と `<v>` を持つセルを strip する」にする。
    - 書いた数式はキャッシュを持たず、定数は `f` を持たない。そのため、ほかのセルについては、今の引き算と同じ結果になる。
    - 引き算を残すなら、対象を「op が自分のセルとして content を書いたセル」に限る。hand-off だけで変わったセルは入れない。
    - §2 の用語表、§4.4 の content-changed、§6.1 の `would_strip` も、これに合わせる。
  - oracle 1 に上の例を固定ケースとして足す（両側で `C2` のキャッシュが無い）。oracle 9 に「hand-off の新しい master を strip 集合から引く」変異を足す。

### m1（Minor）: 書いて戻しただけの Journal でも、registry の part（`sharedStrings.xml`、`styles.xml`）が dirty のまま残る。GetPut と R2 の「Layer 1 can raw-copy」が成り立たない

- 根拠:
  - `main` の文字列の書き込みは、`si` を追加し、`has_count` なら毎回 `sst/@count` を `set_attr` する（`registry/strings.rs` L240-246）。
  - スタイルの patch も、`xf` と font などを追加する。
  - R2 は `c` ノードと派生状態を戻すが、registry の part には手を付けない。
  - R4 は使われない項目を落とす。だが、part を Base のノードへ戻す（clean にする）とは書いていない。
  - そのため、oracle 5 の「write then revert … save byte-identically」は、文字列やスタイルで落ちうる。
    oracle 11 も、noise に文字列が入ると、F との違いが registry の part の再描画に出る。
  - spike の text layer は「Base のテキストと違う part だけ `set_part`」したので、この点が隠れていた。
  - oracle 5 と 11 が J1 で捕まえるので、Minor とする。
- 修正案:
  - C4 に次を足す。「session の項目がすべて落ち、count と uniqueCount が Base の値に戻った part は、Base のノード
    （C6 の後は今の Base のノード）へ戻し、dirty を消す」。
  - 部分的に残る場合は、その part は許容差分（R4）に入る。R2 の「Layer 1 can raw-copy a part whose every write was reverted」は、
    この規則を引いて書く。

### m2（Minor）: 行・列の属性の op（`set_row_height` など）が、§9.1 の許容差分と `truncate_fast` から抜けている

- 根拠:
  - §4.1 は、行・列の属性を keyed op（`(SheetId, RowId | ColId, Prop)`）とする。
  - §9.1 の和は、changed set のセル、R3、R4、`dimension`、構造と barrier の op から成る。行・列の属性の op はどれにも入らない。
    そのため oracle 4 は、正しい実装でも落ちる。
  - §10.3 の `truncate_fast` は「every cut op is a keyed op」なら fast path に入る。だが、戻すのは「each cell a cut op targeted」である。
    行・列の属性の op はセルを target にしないので、切っても何も戻らない。退避の条件にも入っていない。
    oracle 14 の生成器がこの op を引けば捕まる。
- 修正案:
  - §9.1 に「各行・列の属性の op の行・列」を足す。
  - §10.3 の退避の条件に「切る op が行・列の属性の op」を足す。または、行・列の要素にも snapshot を取ると書く。

### m3（Minor）: C7 は「R2 が restore しなかった changed cell」だけを正規の形にする。持ち越しの印を持つセルが後の epoch の snapshot へ restore されると、epoch 0 の痕跡が残る

- 根拠:
  - epoch 0 で `set_number(9)`、`set_font(bold)`、`set_style_index(0)` を実行すると（`s` の無いセル）、`s="0"` が残る。
  - 構造操作で閉じると、epoch 1 の snapshot は `s="0"` のノードになる。epoch 1 で書いて戻すと、R2 はこのノードを restore する。
  - C7 は restore したセルを対象にしないので、`s="0"` が出力に残る。
    noise の無い F では `s` が無いので、epoch 0 の中の noise なのに、oracle 11 の「byte-identical」が J2 で落ちる。
  - `<c …></c>` も同じである。
- 修正案:
  - C7 の対象を「changed set のセルのうち、ノードが Base のもの（epoch 0 の snapshot）ではないもの」にする。
    持ち越しの印を持ち、restore されたセルも含める。

### m4（Minor）: Delivery の J0 の受け入れ条件は、J1 以降に入る API と Commit を前提にしている

- 根拠:
  - J0 は「oracle 1's harness on keyed ops」とし、受け入れ条件に「oracle 1's cache predicate in full」を入れている。
    fixture には shared group と array がある。
  - ところが、Commit（§7）は J1 にある。R3 に要る C3 と、hand-off の target を報告する C1 は、「before J1's Commit」とされている。
  - C1 が無いと、J0 の changed set には新しい master が入らない。そのため、oracle 1 の `dimension` の述語と oracle 4 が、正しい実装でも落ちる。
- 修正案:
  - 次のどちらかにする。
    - J0 の oracle 1 を「保存しない比較（shadow と逐次 workbook を P で比べる、拒否の一致）」に限り、キャッシュの述語は J1 に移す。
    - C1 の target の報告と C3 を J0 の前提に移す。

### m5（Minor）: 1 つの呼び出しに属する複数の op の途中で `truncate(seq)` した場合が決まっていない

- 根拠:
  - `write_cells` は「one op per cell」（§4.1）で、splice の書き込みも複数の keyed op である。
  - `truncate(seq)` の `seq` がその途中にあると、replay は「受け付けたことの無い部分的な呼び出し」を流す。
    §3.2 の「it re-applies only ops that were accepted on the same prefix」が成り立たない。
    hand-off の選び方や `SharedFormulaMaster` の判定は、呼び出しの単位で決まる。
  - R1 も op ごとに判定するので、`write_cells` の 1 セルだけを飛ばすと、逐次とは違う呼び出しになる。
- 修正案:
  - 次の 2 つを書く。
    - `truncate` は呼び出しの境界でだけ受け付ける（途中は切り上げる、または拒否する）。
    - 複数セルの呼び出しでは、R1 は呼び出し全体が no-op の場合だけ飛ばす。
  - adapter のトランザクションは呼び出しの境界の `seq` を持つので、実害は小さい。

## nit

- n1: C7 の「an `s` that resolves to `xf` 0 is written as its snapshot wrote it (absent, or `s="0"`)」は、snapshot が別の `s`（例: `s="3"`）を持っていた場合が決まっていない。
  「snapshot が `s` を持たなければ無し、それ以外は `s="0"`」のように、snapshot と最終状態だけで決まる形に書く。
- n2: Q7 の「Facts for the decision」に、本ラウンドの M1 と M2 を、(A) だけが持つ種類の誤りとして足す。
  状態から定義した規則（restore、引き算）が、逐次の履歴と食い違う誤りである。
  (B) は逐次の strip を使い、snapshot を持たないので、この種類の誤りを持たない。推奨は変えなくてよい。

## その他の確認結果（指摘なし）

- **R1 を外した場合の同値性**: 上の「前ラウンドの解消状況」M1 のとおり成り立つ。
  `set_font(bold)` が Base の `xf` と同じ内容の session `xf` を作る場合（dedupe が当たらない場合）は、R1 でも R2 でも消えない。
  ただし P では等しく、GetPut の文言（「skipped or restored」）の外なので、指摘にしない。
- **持ち越しの content の印と R3**: 同じシートで閉じたセルについては、反例が無い。
  epoch 1 の snapshot がキャッシュを持つ数式になるのは、内容を書いていない場合だけで、そのときは content の印が付かない。
  例外は M2 の hand-off である。
- **C6 の代わりの案**: 再描画した part を出力から parse し直し、そのシートの census を作り直す案は、保存で snapshot を捨てるので成り立つ。
  `styles.xml` と `sharedStrings.xml` も再描画した part なので、逆引き表も作り直すことになる。R4 の記述と矛盾しない。
- **`truncate_fast` の退避の条件**: 複数 target、グループのセル、構造と barrier の op で replay に退く規則は、round 8 M5 の例を正しく扱う。
  行・列の属性の op は m2 を参照。
- **§9.3 と stale の印**: 変更なし。replay の失敗が `JournalBroken` になることも、§12 と oracle 3 で一致する。
- **公開リポジトリの規則**: 本文とこのファイルに、ローカルパス、ユーザー名、非公開プロジェクト名は無い。

## 判定

converged: no（Blocker 0 / Major 2 / Minor 5 / nit 2）

## Owner response（revision 8 の修正, round 9 対応）

spike のコードは変えていない。どの指摘も、レビューが示した単純で保守的な修正を採った。spike が通らない経路は Appendix B（"Review round 9"）に足した。

- **M1**: 採用。構造操作（行・列の操作、splice、シートの操作）は、**ブック全体のすべての開いた snapshot** を閉じる。閉じたとき snapshot の状態に戻っていないセルには持ち越しの印（内容が違えば content の印も）を付ける（§4.4 "Epochs close snapshots"）。§2 と §4.4 の Snapshot の定義から「in an epoch」を外し、「開いた snapshot を持たないセルの最初の書き込みの前」にした。oracle 1 の構造の向きに、逐次側の記録に頼らない **probe 節**を足した（Base に構造操作だけを当てた probe が strip するセルで、ログの op が target にし、Journal の出力で `f` と `<v>` を持つものは失敗。probe が構造操作を拒否したケースは数えて飛ばす）。J2 の固定ケースに `MATCH(7,B!A:A,0)` と `B!A5*2`（text が同じ）の例、`add_sheet`/`move_sheet`/`rename_sheet`/名前経由の例、別シートの持ち越しの印の例を足し、oracle 9 に「自分のシートと書き換えた数式だけを閉じる」変異を足した。
- **M2**: 採用。R3 の引き算をやめ、「strip 集合のうち保存時に `f` と `<v>` を持つセルをすべて strip する」にした。逐次の `plan_strip`（`has_cache` があり同じ plan が書かないセル）と同じ規則で、同じ plan が書くセルは新しい内容（キャッシュの無い数式か `f` の無い定数）を持つので、Journal 側でも同じ判定で外れる。round 7 の規則より strip が増えるだけなので、B1 は戻らない。§2 の用語表、§4.4 の content-changed、§6.1 の `would_strip`、§7.3 の記述を合わせた。oracle 1 に `set_number(D2,1)`→`set_number(A2,99)`（と逆順）の固定ケース、oracle 9 に変異を足した。
- **m1**: 採用。C4 と R4 に「session の項目がすべて落ち、count が Base の値に戻った registry part は Base のノードへ戻し dirty を消す」を足し、R2 の raw-copy の文言がこれを引くようにした。oracle 5 に文字列とスタイルの書いて戻すケース、oracle 9 に変異。
- **m2**: 採用。§9.1 に行・列の属性の op の行・列を足し、§10.3 の退避の条件に行・列の属性の op を足した。
- **m3**: 採用。C7（§7.1 と §7.4）の対象を「ノードが Base のものでない changed cell」にし、後の epoch の snapshot へ restore されたセルを含めた。
- **m4**: 前者の案を採用。J0 の oracle 1 は保存しない比較（shadow と逐次 workbook を P で、拒否の一致）に限り、保存側の述語（キャッシュの述語の全文を含む）は J1 の受け入れ条件に移した。保存のコストの計測も J1 にした（§6.1、§7.3、Q7、Consequences、Delivery）。
- **m5**: 採用。`truncate`/`truncate_fast` は呼び出しの境界でだけ受け付け、途中は `TruncateInsideCall` で拒否する（§10.3、§12）。R1 は複数の op を持つ呼び出しを全体が no-op のときだけ飛ばす（§7.1）。oracle 9 に変異。
- **n1**: 採用。C7 の `xf` 0 の規則を「Base のノードに `s` が無いか Base のノードが無ければ無し、それ以外は `s="0"`」にし、snapshot の履歴に依らない形にした。
- **n2**: 採用。Q7 の Facts に、M1 と M2 が (A) だけが持つ種類の誤り（状態で定義した規則と逐次の履歴の食い違い）であることを足した。推奨は変えない。

まだ証明されていないもの: 全 snapshot を閉じる規則と probe 節、hand-off の新しい master の strip、registry part の Base ノードへの復帰、呼び出し境界での truncate は、どれも spike で走っていない。J1〜J3 の固定ケースと変異で確かめる。
