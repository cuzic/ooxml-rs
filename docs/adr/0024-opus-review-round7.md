# ADR-0024 レビュー（Opus, round 7）

対象: `plan/xlsx-edit-journal-r8` の `docs/adr/0024-xlsx-edit-journal.md`（revision 8、コミット `1e8f1ce`）。
差分は `c1bfb25..1e8f1ce`。根拠は spike `spikes/xlsx-edit-journal/`（`cb57de4`、`ad74d32`）と、
`main` の `f88d02c` 相当の `ooxml-core` / `ooxml-xlsx`。
プローブは spike をリポジトリの外へコピーし、テストを 1 本足して動かした（コミットしていない）。

## 結論（先に）

Commit を「shadow を残して直列化する」に変える方向そのものは、spike の数値（1.6〜1.8×）で支えられている。
ただし、R3 の strip 集合の定義が、`Ok` のまま古いキャッシュを残す。これは spike で再現した（B1）。
spike の oracle 1 の述語は ADR の述語より弱いため、20,000 ケースの通過はこの経路を覆っていない。

ほかに Major が 5 件ある。

- 保存をまたいだ shadow の dirty/span の扱い（M1）
- 1 回の呼び出しが別のセルを変える場合の changed set（M2）
- 構造操作の calc state を保存時へ遅らせる規則（M3）
- `truncate_fast` の再適用範囲（M4）
- C1〜C5 に入っていない正規化（M5）

どれも、spike がキー付き op だけ、1 epoch だけ、shared formula なしで動いたことによる穴である。

**converged: no**

## 前ラウンドの解消状況

round 6 の m1〜m3、n1 は revision 7 で解消済み（round 6 は収束）。revision 8 は Commit を差し替えた。
round 6 の各点がどこへ移ったかを確かめた。

- m1（P1 は存在しないセルに何も作らない）: §4.1 の P1 の行と §7.2 step 4 に残っている。
  N1 の表は A.2 へ移った。
- m2（written cell は plan の target）: oracle 1 の `dimension` 述語の sequential 側に残っている。
  Journal 側は changed set に置き換わった（後述 M2 を参照）。
- m3（metadata はセルから block への写像）: P に残っている。
- n1: A.3 step 4 に移った。

## Findings

### B1（Blocker）: R3 の strip 集合は「changed set を除く」。そのため、スタイルだけを変えた数式セルが、入力が変わっても古いキャッシュを保つ

- 根拠:
  - §7.1 R3 は strip 集合を「the ADR-0003 dependents (transitively) of the changed set's content changes … **minus the changed set**」と定める。
    changed set（§2、§4.4）は、snapshot の状態と異なるセルすべてである。スタイルだけを変えたセルも入る。
  - そのため、数式セルに `set_font` を当て、その数式が読むセルの値を変えると、数式セルは changed set に入る。
    strip 集合からは外れ、`<v>` は Base のまま残る。
  - spike の実装も同じ規則である（`src/shadow.rs` `calc()`、`if !eff.contains(&d)`）。
  - プローブ: oracle workbook（`C1 = A1*2`、キャッシュ 2）で、`set_number(A1, 5)`、`set_font(C1, italic)` を実行した。
    - sequential: `<c r="C1" s="4"><f>A1*2</f></c>`（strip される）
    - Journal: `<c r="C1" s="4"><f>A1*2</f><v>2</v></c>`（`A1` は 5 なのに 2 のまま）
    - `oracle1()` はこのケースを**通す**。
  - spike の述語（`src/project.rs` `predicates`）は「kept `v` is the Base's」だけを確かめる。
    ADR の oracle 1 にある後半の「and no cell it reads … differs in P from the Base」が実装されていない。
    そのため、§7.3 や Appendix B が挙げる 20,000 ケースの通過は、この誤りを検出できない。
  - 結果: `Ok` のまま、LibreOffice が古い値を表示するファイルができる。これは Q1 (A) が防ごうとした失敗そのものである（ADR-0003 §3.3）。
  - R3 の規範文と §6.1 の pending strip set は、どちらもこの定義に依存している。
- 修正案:
  - R3 の「minus the changed set」を、「minus the cells whose **content** differs from their snapshot」に直す。
    内容を書き換えた数式セルは、自分の write でキャッシュが決まる（新しい数式はキャッシュなし、ADR-0003 §3.1a）。
    スタイルだけ変えたセルや、R2 で戻ったセルは、ほかのセルと同じく strip 対象にする。
  - oracle 1 の固定ケースに、上のプローブ（入力の変更と、依存先へのスタイル patch）を足す。
  - oracle 9 に変異「changed set 全体を除く」を足す。
  - J0 の受け入れ条件に「oracle 1 のキャッシュ述語を全文で実装する（読み先セルの P 比較を含む）」と明記する。
  - Appendix B と §7.3 で、20,000 ケースの通過を引く箇所に、「述語は前半だけだった」と注記する。

### M1（Major）: 残した shadow の「元のバイト」と dirty は、最初の Base を基準にしている。そのため、C2 の「dirty を消す」は 2 回目以降のセッションで以前の保存を巻き戻しうる

- 根拠:
  - Layer 2 の直列化は、文書の parse 元（`XmlDocument::source()`）と span から元のバイトを写す（`ooxml-core` `xml/write.rs`）。
    Layer 1 は、dirty でない part を開いたときの zip から写す（`Package::read_original`、`package.rs` L343）。
    どちらも shadow を開いた時点の Base であり、§10.1 の「output becomes the new Base」ではない。
    `to_bytes` は `&self` で、dirty を消さない。
  - そのため、2 回目のセッションでは次のことが起きうる。
    - セッション 1 で `A1` を 5 から 6 にして保存する。
    - セッション 2 で `A1` を 6 → 7 → 6 と書き、R2 で戻す。
    - snapshot はセッション 1 で書き換えたノードで、dirty であり、元の span は古い Base を指す。
    - C2 の文言（「clear `own_dirty`/`subtree_dirty` up the chain when nothing else under them is dirty」）どおりに dirty を消すと、
      古い Base の `<c r="A1"><v>5</v></c>`、あるいは part 全体が写される。`Ok` のまま、セッション 1 の編集が消える。
  - R2 の「A restored cell serializes as its original bytes, so Layer 1 can raw-copy a part whose every write was reverted」と、
    §10.1 の「After steps 2–4 it holds what the output holds」は、どちらも最初のセッションでしか成り立たない。
  - spike はこの経路を通らない。text layer が保存のたびに**新しい** Base を lazily に開き、pins で差を運ぶからである（Appendix B "Carried state"）。
    C2〜C5 が入ると carried state は無くなる、という主張（§7.4 の表の下）は、この点を扱っていない。
  - oracle 12 と 13 は「保存 → 2 回目のセッション」を含むので、実装すれば見つかる。ただし、規範文（C2）が誤りへ誘導している。
- 修正案（どちらかを選び、§7.4 と §10.1 に書く）:
  - (a) 不変条件として書く。「dirty とは shadow の parse 元（そのセッション列の最初の Base）と異なること」。
    C2 は snapshot ノード自身の dirty 状態を戻す。祖先の dirty は、parse 元と異なる子孫が無いときだけ消す。
    GetPut（§6.1）と oracle 5 は、2 回目以降は「再描画と再圧縮が決定的である」ことに依存すると明記し、それを確かめる。
  - (b) C6「rebase after save」を足す。保存の直列化中に新しい span を記録し、parse 元を出力へ付け替えて dirty を消す。
    コストを oracle 10 で測る。
  - どちらの場合も、oracle 13 に上の固定ケースを足す（保存した編集を 2 回目のセッションで書いて戻す）。

### M2（Major）: changed set と snapshot は op の key のセルだけを見る。shared formula の hand-off のように、1 回の呼び出しが別のセルを変える場合を覆っていない。oracle 11 の noise も、R2 が戻せないセルを含む

- 根拠:
  - §4.4 は「The first write … on a cell」「Cells a barrier op writes … are cells like any other」と書く。
    shared group の master への書き込みは、ADR-0003 §4.2 の hand-off で**別のセル**（次の member）を master に書き換える
    （`edit.rs` L446-460 `plan_handoff`）。この cell は snapshot を持たず、changed set に入らない。
    その結果、次のものがこのセルを覆わない。
    - §9.1 の locality（oracle 4 の allowed-diff）
    - §7.2 step 4 の `dimension`
    - `truncate_fast` の巻き戻し
  - R2 は `f` 属性を比べる（§4.4）。そのため、shared member や array member に定数を書き、元の数式を書き戻しても、戻らない。
    `set_formula` は `t="shared"`/`si`/`ref` も `ca` などの属性も再現しない。
    - oracle 11 の noise は「a change of value (… or formula) and a write-back」を「Base cells」に置き、
      fixture は「oracle 1's fixtures」（shared group、array、POI fixture を含む）である。
      この定義のままだと、noise が戻らず、byte-identical は**正しい実装でも落ちる**。
    - spike の fixture には shared formula も array も無い（`src/fixture.rs`）。
      spike の no-op 判定は `formula_plain` のときだけ（`src/commit.rs` `noop_kind`）。
      `truncate_fast` は shared-formula セルで replay へ逃げる。どれもこの経路を試していない。
  - §7.1 の「R1 is invisible in the output once R2 exists」は、shared member では成り立たない。
    R1 は同じ text の書き込みを飛ばすが、R1 が無ければ書き込みが group を割り、R2 は戻せない。
    oracle 1 の除外（shared-group hand-off shape）の行も、この前提で書かれている。
- 修正案:
  - §4.4 の snapshot と changed set を「呼び出しの plan が target にした全セル（hand-off の新しい master を含む）」で定める。
    round 6 m2 の「written cell」の定義を再利用する。
  - oracle 11 の noise を「R2 が戻せる Base セル」に限る（shared/array member と、`set_formula` で再現できない `f` 属性を持つセルを除く）。
    除いたセルは P で比べる、と書く。
  - §7.1 の R1 についての主張を「shared member を除いて」に直す。
  - oracle 1 の固定ケースに次の 2 つを足す。
    - shared master に定数を書き、元の text を書き戻す（両側で hand-off が起き、locality が新しい master の行を許す）。
    - shared member に同じ text を書く（R1 が飛ばす）。

### M3（Major）: R3 は構造操作の calc state も保存時へ遅らせる。だが、その集合を何で記録するかと、calcChain の扱いが書かれていない

- 根拠:
  - ADR-0018 §3.7 では、strip 集合は「computed … before the edit, in step 1 … at pre-edit indices」である。
    また、「calcChain: removed … whenever a formula cell moves or is deleted」とする。
  - R3 は「plus the strips ADR-0018 §3.7 requires of the session's structural ops」とだけ書く。
    C3 は「no strip, flag or calcChain step at write」とする。
    その結果、次の点が決まっていない。
    1. 保存時には、構造操作の前の状態（census、名前、テーブル）はもう無い。集合は append の時点で id で記録するしかないが、その記録が §4.2 の `LoggedOp` に無い。
       replay でどう再生するかも書かれていない。
    2. R3 の calcChain の条件は「a changed cell gained or lost a formula」だけで、数式セルの移動が入っていない。
       移動した数式は changed set に入らない（§4.4: 構造操作は snapshot を閉じるだけ）。
       そのため、行を挿入して数式が動いても calcChain が残る。位置が古い calcChain は、Excel が修復を求める原因になる。
    3. 構造操作の後の snapshot は「the node as that op left it」で、Deferred の下では §3.7 が strip すべき古いキャッシュを持つ。
       B1 の「minus the changed set」と組み合わさると、その後スタイルだけを変えたセルはキャッシュを保つ。
  - spike は構造操作を持たない（Appendix B "Limits"）。§4.4 も「The spike has no structural ops; the rule is checked when J2 lands」と書く。
    規範規則（normative）なのに、証拠も擬似コードも無い。
- 修正案:
  - C3 を次のように定める。構造操作は Deferred の下で、自分の §3.7 strip 集合（id）と「数式セルを動かしたか、消したか」を返す。
    `LoggedOp` に記録し、replay で再生する。
    `finish_calc` は、記録した集合のうち生きている id、つまり内容が後で上書きされていないものを strip に加える。
    数式を動かした構造操作が 1 つでもあれば、calcChain を外す。
  - oracle 1 の J2 固定ケースに、数式のある行の前への `insert_rows` を足し、calcChain が残らないことを確かめる。
    さらに、その後に移動したセルへスタイル patch を当て、キャッシュが戻らないことも確かめる。

### M4（Major）: `truncate_fast` は「the cell's ops up to `seq` since the commit mark」を、後の epoch で取った snapshot の上に再適用する

- 根拠:
  - §10.3 は「`restore_cell` puts back its snapshot (C2), then the cell's ops up to `seq` since the commit mark are applied again」とし、
    構造操作が**切られる** op に含まれるときだけ replay へ逃げる。
  - snapshot は epoch ごとに取り直す（§4.4: 構造操作が snapshot を閉じ、次の書き込みで新しい snapshot を取る）。
  - 例: epoch 0 で `A1 = B5*2` を書く。`insert_rows(3, 1)` を実行する（残す側）。epoch 1 で `A1` に値を書く（切る側）。
    `truncate_fast` は epoch 1 の snapshot（`B6*2`）を戻した後、「commit mark 以降の `A1` の op」として epoch 0 の `set_formula("B5*2")` を
    birth-epoch の text のまま再適用する。結果は `B5*2` になり、replay の `B6*2` と食い違う。round 1 B1 と同じ種類の誤りである。
  - oracle 14 は J3 で見つけうるが、spike は 1 epoch しか試していない。
- 修正案: 再適用するのは「その cell の snapshot を取った後、`seq` までの op」に限ると書く。
  snapshot が現在の epoch に無いセルでは replay へ逃げる、という既存の条件はそのまま残す。
  oracle 14 の生成器に、構造操作をまたぐ切断を足す（J3）。

### M5（Major）: spike の text layer は、戻っていないセルも正規化している。この正規化が C1〜C5 のどれにも入っておらず、「Commit is literally … `to_bytes()`」と履歴独立性の主張を支えるものが無い

- 根拠:
  - Appendix B と spike の README は、text layer の役割に、戻したセルだけでなく「every other written cell takes the shadow's bytes in a
    canonical form (… `xf` 0 written as the Base cell wrote it, a childless element self-closing)」を含める。
  - 固定テスト `style_reset_to_zero_then_written_matches_the_direct_write` は、`set_fill`、`set_style_index(0)`、`set_number(9)` の出力が
    `set_number(9)` 単独と同じバイトになることを確かめる。これは layer の正規化による。
  - core に入る C2 は、**戻ったセル**だけを元のノードに戻す。戻らないセルでは、次の例のように `s="0"` が残る。
    - 例: `set_number(9)`、`set_font(bold)`、`set_style_index(0)` を、`s` の無いセルに当てる。最後の op は R1 で飛ばない（その時点の `s` は bold）。
    - 内容は snapshot と違うので R2 も戻さない。出力は `<c … s="0">` になり、`set_number(9)` だけの履歴とバイトが違う。
  - §3.2 の「R2 removes the first four」と §7.4 の「Commit is literally `finish_calc` + `compact_registries` + `dimension` + `to_bytes()`」は、
    この正規化を落としている。
  - oracle 11 の noise は「内容を変えた後のスタイルの書き戻し」を作らないので、この差を検出しない。
- 修正案（次のどちらかで、F11 の全項目について、core で何が消すかを表にする）:
  - (a) C2 に「`s` の書き方を snapshot のノードに合わせる（`xf` 0 で、snapshot に `s` が無ければ属性を消す）」を加える。
  - (b) §7.1 の履歴独立性を「戻ったセルについては byte、ほかは P とその書式の正規形」に弱める。
  - さらに、oracle 11 に「F の内容 op の後に、スタイルの往復を挟む」variant を足す。

### m1（Minor）: R1 の「reads equal」が、spike と R2 の等しさと揃っていない

- 根拠:
  - R1 は ADR-0023 §6.2 の「same value kind and value, same formula text」を使う。R2 は §4.4 の「value with its type and the rich flag; formula text, `f` attributes and kind」を使う。
  - spike の R1 はさらに次の条件を持つ（`noop_kind`）。
    - 文字列: rich でないこと
    - 数式: `formula_plain` であること
    - `set_datetime`: 日付書式であること（two-half op）
  - ADR の R1 は、two-half op（`set_datetime`、`*_with_format`）をどう判定するかを書いていない。
  - 例: rich 文字列 "abc" のセルに `set_string("abc")` を書く。ADR の文言では飛ばしてよく、rich のランが残る。
    sequential はランを消すので、P が食い違う（oracle 1 で見つかるので、黙った誤りにはならない）。
  - §4.4 は「`s` が無い」と「`s="0"`」を同じとみなすか書いていない（R1 は「0 when the attribute is absent」と書く）。
- 修正案: R1 を「C1 の plan がそのセルに与える効果が、§4.4 の状態の等しさで恒等であること（two-half op は両方の half）」と 1 つに定める。
  §4.4 に「`s` が無いことは `s="0"` と等しい」を足す。

### m2（Minor）: R4 の `sst/@count` は changed set で数える。そのため、構造操作で消えた `t="s"` セルを数えない

- 根拠:
  - R4 と oracle 1 の Journal 側の述語は、「the Base's count plus the change in `t="s"` cells over the changed set」とする。
  - `delete_rows` と `delete_sheet` で消えたセルは、changed set から出る（§4.4、§5.5）。そのため count は減らない。
  - 新しいシートのテンプレートは問題にならない。
  - 述語が「changed set で数える」のか「全セルで数える」のかが曖昧で、J2 で両側の判定がずれる。
- 修正案: count は「保存時の `t="s"` セルの数」から直接数える。少なくとも、構造操作で消えたセルを差し引くと書く。
  F4 のとおり sequential 側は対象外のままでよい。

### m3（Minor）: C5 の「(or let the save recompute it)」が曖昧。§7.2 step 4 の「the Base's (as structural ops rewrote it)」は、書き込みのたびに `dimension` を広げる shadow からは得られない

- 根拠:
  - 今の sequential の書き込みは `plan_target` で `dimension` を広げる。shadow がそれを続けると、R2 で戻したセルの拡大は残る。
    さらに構造操作がその広がった範囲を書き換えるので、保存時に「Base の範囲を構造操作が書き換えたもの」を取り出せない。
- 修正案: C5 を「Journal の shadow では書き込みが `dimension` に触れない。構造操作は ADR-0018 §3.2 どおりに書き換える。保存時に changed set で広げる」と 1 案に決める。
  oracle 9 の変異（「restored cell が `dimension` を広げる」）と対応させる。

### m4（Minor）: §9.3 の保存失敗に 2 つの穴がある。panic のときに stale の印が付かないことと、replay 自体が失敗したときの状態

- 根拠:
  - §7.2 step 6 は「after any of them [steps 2–4] it is marked stale」とする。だが、step 2〜4 の中で panic が起きると、印を付けるコードに届かない。
    oracle 3 は `catch_unwind` を使う。
  - 失敗が sequential 呼び出しの決定的なバグなら、replay で同じ失敗が起きる。
    そのときの Journal の状態（stale のままか、`JournalBroken` か）が書かれていない。
    oracle 3 の「a retried Commit without the fault succeeds」は、注入した失敗にしか当てはまらない。
- 修正案:
  - 「step 2 に入る前に stale の印を付け、swap の後で外す」と書く。
  - replay が `JournalInternal` を返したら、Journal は §11 と同じく `JournalBroken`（`truncate` と reload のみ可）にすると定める。
    §12 の表の `JournalBroken` の行に、この場合を足す。

### m5（Minor）: revision 8 の規則に「(decided)」が付いている。一方、Status は Proposed で、Q7 は未確認。Q7 の選択肢にも、owner が判断に要る事実が欠けている

- 根拠:
  - 次の見出しが「decided」を名乗る。どれも revision 8 で書き換えたか、新しく足した節である。
    - §4.3、§4.4、§4.5 の「(decided)」
    - §7.1 の「(decided, normative)」
    - §10 の「(decided)」
  - 一方、ヘッダは「Until Q7 is confirmed, revision 7's compiled Commit (now Appendix A) is the accepted decision」とする。
  - Q7 (A) の数値（1.6〜1.8×）は text layer で測ったもので、C1〜C5 は未実装である。
    R1 は C1 が入るまで無効になる（§4.5）。Q7 はこの 2 点を書いていない。
  - Alternative 11 は、compiled Commit を spike で動かせる「second reference for oracle 1」として残すとする。
    Q7 は、それを J0/J1 のテスト用の第 2 エンジンとして残すか（§9.3 の「no second engine」を和らげる）を、選択肢に入れていない。
- 修正案:
  - revision 8 で書き換えたか足した節の見出しを「(proposed, revision 8)」にし、Q7 の確認時に戻す。
  - Q7 (A) に、次の 2 点を書く。
    - 「数値は text layer で測った。C2〜C5 での値は J0 で測り直す」
    - 「C1 が入るまで R1 は無効」
  - Q7 に、「compiled Commit をテスト専用の参照として残すか」を (A) の付帯の問いとして加える。

### m6（Minor）: C1〜C5 の表が足りない（M1 の rebase と M5 の正規化を除く分）

- 根拠: 本文の規則が依存しているのに、表に無い API がある。
  - snapshot の取得: `c` ノードを span と dirty 状態ごと複製する読み取り API。C2 は戻す側しか無い。
  - §4.4 の状態の等しさに要る読み取り: `f` 属性と rich のランを読む API。F2 は「the public API does not expose」と書く。
  - `restore_cell` に snapshot `None`（存在しなかったセル）を渡したときの動作: `c` と、セッションが作った `row` を消すか。
    `truncate_fast`（§10.3）は「a cell has no snapshot」のときに replay へ逃げるが、`None` は snapshot の一種なので区別が曖昧である。
  - §6.1 の pending strip set: `finish_calc` を shadow を変えずに計算する版（dry run）。
  - C4 の後に、shadow の dedupe 用の逆引き（`sst` と `xf` の内容から索引への表）を作り直すこと。
- 修正案: 上の 5 点を C1〜C5 の行に足すか、C6 以降として表に加える。

### m7（Minor）: pending strip set は append のたびに捨てられる。そのため、setter と getter を交互に呼ぶと、読むたびに 2〜5.5 ms かかる

- 根拠:
  - §6.1 と §6.3 は「computed on demand and cached until the next append」とする。spike の値は 110k セルで 2.0 ms、1.1M セルで 5.5 ms である。
  - ExcelJS の典型的な使い方（setter の直後に getter を呼ぶ）では、1,500 組で 1.1M セルに約 8 s かかる。
    これは、Context の欠陥 (b) を別の形で戻すことになる。
- 修正案: normalized read では、読む数式セルが changed set の推移的な依存先かどうかだけを確かめる（セル単位で問い合わせる）と書く。
  または、交互に呼ぶ負荷を oracle 10 の測定項目に足す。

## nit

- n1: revision 8 の項目は「findings F1–F14」と書くが、本文は F10（新しい oracle の提案）を一度も引かない。
  oracle 11〜14 の出典として F10 を書く。
- n2: oracle 11 の「Base cells」と oracle 13 の「normalized reads over the grid」は、範囲が決まっていない。
  前者は構造操作の後では id で言う。後者は「fixture の使用範囲と、その周り 1 行 1 列」などと定める。
- n3: §9.3 の 1 行目の「or in a step that does not change the shadow」に当たるのは step 1 だけである
  （step 5 と 6 は step 2〜4 の後なので、2 行目になる）。「before step 2」だけでよい。

## その他の確認結果（指摘なし）

- **R1 を検証の後に置くこと（F13）**:
  - 順序（plan → `Err` → no-op の判定 → apply）と、C1 が無い間は R1 を無効にすることは、拒否の等価性（oracle 1 (i)）を保つ。
  - oracle 1 の固定ケースにマージの非 anchor セルが入り、oracle 9 に「検証の前に判定する」変異が入った。
- **R4 の正規の番号付け**:
  - Base の項目は番号を変えない。session の項目は（シート順、行、列）で最初に参照された順に並ぶ。
    この 2 つで、R4 は履歴から独立する。
  - session の `xf` の索引を View に出さないこと（F12）と、P2 の境界（Base の `cellXfs` の数）は整合している。
  - `dxf` を compaction の対象から外すことと、§6.1 の `dxf` 索引も整合している。
- **R2 の epoch 内への制限**:
  - 構造操作をまたぐ書き戻しを P で比べる、という限定は正しい。
  - 「a reference-rewriting structural op closes those of every formula cell whose text it rewrites」は、
    別シートの数式も閉じるので、round 2 M1 の観点（別シートの数式）を満たす。
- **§7.2 の手順とアトミック性**:
  - 出力は直列化が成功した後でしか返さない。step 2〜4 の失敗は replay で戻す。この 2 点で、`Err` ⇒ byte-identical は保たれる（m4 の panic の点を除く）。
- **再配置した章の相互参照**:
  - 次の参照は、新しい配置と一致した。
    - §4.4 → §7.2 step 4
    - §6.1 → §7.2 step 4
    - A.3 → A.4、A.5
    - A.7 の変異の一覧
    - oracle 9 の「compiled Commit's mutants are in A.7」
  - revision 8 の項目にある移動先の対応表（A.1〜A.8）も、付録の内容と一致した。
- **数値**: 次の値は spike の README と一致した。
  - §7.3 の表
  - §3.2 の「about 300 bytes」（+2.2 MiB と +1.6 MiB の差）
  - §10.3 の 14〜28×
  - A.6 の 8.4×
- **公開リポジトリの規則**: 本文に、ローカルパス、ユーザー名、非公開プロジェクト名は無い。

## 判定

converged: no（Blocker 1 / Major 5 / Minor 7 / nit 3）

## Owner response（revision 8 の修正）

spike のコードは変えていない。spike の証拠が弱かった点は ADR の Appendix B（"What the spike does not show"）、§7.3、Q7 と spike の README に明記した。

- **B1**: 採用。R3 の strip 集合から引くのを「内容が snapshot と異なるセル（content-changed）」だけにした（§4.4 の定義、§7.1 R3）。スタイルだけの変更、content restore、R2 で戻したセルは通常どおり strip する。oracle 1 の述語は前半・後半とも必須と明記し、J0 の受け入れ条件に全文実装と shared/array fixture を足した（Delivery）。oracle 1 に固定ケース、oracle 9 に変異を足した。spike にこの誤りがあること、20,000 ケースが覆っていないことを Appendix B、§7.3、Q7、README に書いた。
- **M1**: 修正案 (b) を採用。C6 `rebase_after_save` を追加（直列化中に新しい span を記録し、parse 元と Package の original を出力に付け替え、dirty を全部消す）。§7.2 step 7、§7.1 R2、§10.1 に反映。oracle 12/13 に「保存した編集を 2 回目のセッションで書いて戻す」固定ケース、oracle 9 に変異、oracle 10 に C6 のコストを足した。
- **M2**: 採用。snapshot と changed set を「呼び出しの plan の target 全部（hand-off の新しい master を含む）」で定義（§4.4、C1 が target を返す）。§7.1 の R1 の主張に shared/array member の例外を書いた。oracle 11 の noise を R2 が戻せるセルに限り、除いたセルは P で比べる第 4 variant にした。oracle 1 に固定ケース 2 つを足した。
- **M3**: 採用（保守的な規則 + J2 で検証）。構造操作は Deferred の下で `StructuralCalc`（§3.7 の strip 集合を id で、数式セルを動かしたか、消した `t="s"` セル数）を返し、`LoggedOp.calc` に記録（§4.2、C3）。保存時に、記録した id のうち生きていて `f` を持つものをすべて strip し、数式を動かした構造操作があれば calcChain を外す（§7.1 R3）。oracle 1 の calcChain 述語に条件を足し、J2 の固定ケースと変異を足した。
- **M4**: 採用。`truncate_fast` が再適用するのは「現在の snapshot を取った op から `seq` まで」に限った（§10.3）。oracle 14 の生成器に構造操作をまたぐ切断を足した（J3）。
- **M5**: 修正案 (a) を採用。C2 に content restore（内容が戻り `s` だけ違うときは snapshot ノードに現在の `s`）を加え、C7（保存時の正規形: `xf` 0 は snapshot の書き方、`None` snapshot は属性なし、子のない要素は self-closing）を追加。§7.4 に F11 の各痕跡を消す API の表を置いた。「Commit is literally …」を C1〜C8 に直した。oracle 11 に「F の内容 op の後のスタイル往復」variant を足した。round 5 M1 の固定ケース（`B5`）の Journal 側のバイトは `<c r="B5"/>` になる（P は同じ）。
- **m1**: R1 を「すべての target が存在し、C1 の plan が P のモデル化する内容を変えない（two-half op は両方）」に一本化した。§4.4 の状態の等しさ（R2 用）とは意図して分け、理由を §7.1 に書いた（shared member の同じ text は R1 で飛ばす）。§4.4 に「`s` が無いことは `s="0"` と等しい」を足した。
- **m2**: 採用。`sst/@count` は shadow の running delta（構造操作が消した `t="s"` を引く）で保ち、Journal 側の述語は出力と Base の `t="s"` セルを直接数える（§7.1 R4、oracle 1）。構造部分は J2 で検証。
- **m3**: 採用。C5 を 1 案にした（shadow の書き込みは `dimension` に触れず、構造操作が書き換え、保存時に changed set で広げる）。oracle 9 に「shadow の書き込みが広げる」変異を足した。
- **m4**: 採用。step 2 の最初で stale の印を付け、swap で外す。replay の失敗は `JournalBroken`（§7.2、§9.3、§12、oracle 3）。
- **m5**: 採用。revision 8 で書き換えた見出しを「(proposed, revision 8)」にし、Q7 の "Default" を "Recommended, not yet decided" にした。Q7 に text layer の数値であること、C1〜C8 未実装、R1 が C1 まで無効、spike の証拠の欠け、compiled Commit をテスト専用の参照として残すかの付帯の問いを足した。
- **m6**: 採用。§7.4 の表を C1〜C8 にした（snapshot の読み取り、`restore_cell(None)` の動作と「snapshot 記録が無い」との区別、dry run と per-cell の strip 問い合わせ、C4 後の逆引きの再構築、`f` 属性と rich ランの読み取り C8）。
- **m7**: 採用。normalized read はセル単位の `would_strip` で問い合わせる（§6.1、§6.3、C3）。oracle 10 に setter/getter 交互の負荷を足した。
- **n1**: revision 8 の項目で oracle 11〜14 の出典に F10 を書いた。
- **n2**: oracle 11 の noise の対象を id とブロック開始時の Base 内容で定め、oracle 13 の範囲を「fixture の使用範囲と周り 1 行 1 列」にした。
- **n3**: §9.3 の 1 行目を「before step 2」だけにした。
