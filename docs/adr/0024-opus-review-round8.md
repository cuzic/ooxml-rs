# ADR-0024 レビュー（Opus, round 8）

対象: `plan/xlsx-edit-journal-r8` の `docs/adr/0024-xlsx-edit-journal.md`（revision 8、round 7 対応後、コミット `972a69f`）。
round 7 の Owner response を照合し、文書全体を独立に読み直した。コードは `main` の `f88d02c` 相当の
`ooxml-core` / `ooxml-xlsx` で照合した。プローブは `ooxml-xlsx` を path 依存にした使い捨てのバイナリで、コミットしていない。

## 結論（先に）

round 7 の B1（strip 集合から引くのは content-changed だけ）は R3 の規範文、§6.1、oracle 1 の述語、Delivery のすべてに
反映され、矛盾は無い。m2〜m4、m6、n1〜n3 も文言として解消している。

ただし、round 7 の修正のうち 3 つが新しい穴を作った。

- m1 の修正（R1 を P で判定する）は、shared member の書き戻しを飛ばす。そのため、後の op の拒否が逐次と食い違う。
  §4.5 の「refusal equivalence holds by construction」が成り立たない。プローブで再現した（M1）。
- M1 の修正（C6）は「再描画したノードの新しい span」だけを記録する。
  Layer 2 は、dirty でないノードも `src[span]` から写す。そのため、同じ part の clean なノードの span もずれる（M4）。
- M4 の修正（`truncate_fast` の再適用範囲）は、1 つの op が複数のセルを target にする場合（hand-off）を扱っていない（M5）。

ほかに Major が 3 件ある。

- C2 の `restore_cell` が census（`has_cache`、kind、依存グラフ）を戻すと書いていない（M2）
- R2 の「restore したセルは changed set から出る」が、§4.4 の epoch をまたぐ所属と矛盾し、`dimension` が狭くなる（M3）
- oracle 1 のキャッシュ述語が、構造操作による古さを表せない（M6）

どれも spike が通らない経路（構造操作、shared group、2 回目のセッション、core 内の restore）にある。
文書はこれらを「to be verified」と正直に書いている。ただし、規範文そのものが誤りへ誘導している点は、検証以前に直す必要がある。

**converged: no**

## 前ラウンドの解消状況

- **B1**: 解消。R3 は「minus the content-changed cells」になった。§2 の用語表、§4.4 の content-changed の定義、
  §6.1 の `would_strip`、oracle 1 の述語（後半を必須と明記）、oracle 9 の変異、Delivery の J0 受け入れ条件、
  Appendix B の注記がそろっている。
  §7.3 の「直しても strip が増えるだけで、コストはほぼ同じ」も妥当である。
- **M1**: 方針 (b)（C6）は正しい。ただし、C6 の仕様が足りない（本ラウンド M4）。
- **M2**: target の定義と changed set は解消した。ただし、同じ対応で R1 を P にしたことが、本ラウンド M1 を生んだ。
- **M3**: `StructuralCalc` と R3 の構造部分は筋が通っている。ただし、ADR-0009 の操作が入っていない（m1）。
  oracle 側の述語も足りない（M6）。
- **M4**: 単一 target の場合は解消した。複数 target の場合は本ラウンド M5 を参照。
- **M5**: C2 の content restore、C7、F11 の表は解消した。C7 が `ooxml-core` の Layer 2 を変える点は m5 を参照。
- **m1**: R1 は一本化された。ただし、本ラウンド M1 を参照。
- **m2〜m4、m6、m7、n1〜n3**: 解消（m7 は `would_strip` の条件に抜けがある。m2 を参照）。
- **m5**: 見出しと Q7 の事実は解消した。Q7 の事実の偏りは本ラウンド m6 を参照。

## Findings

### M1（Major）: R1 が shared member の書き戻しを P で飛ばすため、後の op の拒否が逐次と食い違う。§4.5 の「refusal equivalence … holds by construction」が偽になる

- 根拠:
  - §7.1 R1 は「a shared member's own text written back is skipped (P expands it)」とする。
    逐次側では、同じ `set_formula` が member を group から外し、通常の数式にする（`edit.rs` apply の
    `census.remove(fid)` → `FKind::Normal` で `push`）。
    そのため、R1 の後、Journal の shadow と逐次の workbook では group の形が違う。P の外の状態で、
    oracle 1 は「shared-group hand-off shape」の除外でこれを許している。
  - ところが、group の形は**検証の入力**である。`write_cells` は同じ group の 2 セルを 1 回で書くと
    `SharedFormulaMaster` で拒否する（`edit.rs` L453-458）。
  - プローブ（`o1_shared.xlsx`、group `C2:E3`、master `D2`）:
    - Journal（R1 が `set_formula(E3, "$A3+E$1")` を飛ばした状態）で `write_cells([D2: 1, E3: 2])` →
      `Err(SharedFormulaMaster: another cell of shared group 1 is written in the same batch)`
    - 逐次（`set_formula(E3, "$A3+E$1")` を実行した後）で同じ `write_cells` → `Ok(())`
  - したがって、正しい実装でも oracle 1 (i) が落ちる。
    §4.5 の「holds by construction for part A」と Consequences の「Refusals equal the sequential API's by construction … R1 included」
    はどちらも誤りである。
  - 同じ形の食い違いは hand-off にもある。後で master を書くと、新しい master の選び方（`plan_handoff` は group の
    member の最小位置を選ぶ）が両側で変わる。`translate` の失敗による拒否も変わりうる。
  - round 7 の m1 への対応（「R1 uses P, not §4.4's stricter state equality, because a skipped op leaves the node untouched」）は、
    出力だけを見ている。shadow が以後の検証の土台であること（§3.2）を見落としている。
- 修正案:
  - R1 の等しさを「C1 の plan が**ノード**を変えない」（§4.4 の状態の等しさ）にする。
    少なくとも shared/array member と master では、P ではなく §4.4 の等しさを使う。
    こうすると、両側で group が割れ、形も同じになる。§7.1 の「R1 is invisible … except on shared and array members」の例外は、
    「R1 も R2 も shared member の書き戻しを消さない（両側で割れる）」に変わる。
  - 一般則として §4.5 に次を書く。「R1 が飛ばしてよいのは、shadow の状態のうち**検証が読むもの**（census の kind と group、
    merge、table、array、`vm`/`cm`）を変えない op だけ」。
  - oracle 1 の固定ケースの「a shared member's own text written back (R1 skips it …)」を、
    「…の後に、同じ group の master と member を 1 回の `write_cells` で書く（両側で同じ結果）」に直す。
  - oracle 1 の除外行「shared-group hand-off shape」の理由文から、R1 の部分を消す。

### M2（Major）: C2 `restore_cell` は XML ノードだけを戻し、census と索引を戻すと書いていない。census の `has_cache` が古いと、R3 は restore したキャッシュを strip しない

- 根拠:
  - 書き込みの apply は、ノードと一緒に census を更新する（`edit.rs` L860-910）。
    - 書いたセルの `FCell` を `remove` し、数式なら `has_cache: false` で `push` する
    - 依存グラフに `add` する
    - strip した数式の `has_cache = false`、`results_stripped = true`
    - hand-off の新しい master の kind、text、range
    - `style_sensitive` のキャッシュを捨てる
    - 行の索引（`indexes`）を `effect.update` する
  - strip の計画は、census の `has_cache` を見る（`plan_strip`、`edit.rs` L745-747: `if c.has_cache && …`）。
    `finish_calc` が今の strip 機構を使えば、同じ判定になる。
  - 例: 数式セル `F = A1*2`（キャッシュあり）に定数を書き、元の数式を `set_formula` で書き戻す。
    - census には、`has_cache: false` の新しい `FCell` が入る。
    - R2 が Base のノード（`<v>` あり）を戻しても、census が古いままだと、`has_cache` は false である。
    - 同じセッションで `A1` を変えると、R3 の strip 集合に `F` が入る。だが `plan_strip` は `has_cache` が false なので飛ばす。
    - 出力は `<f>A1*2</f><v>2</v>` で `Ok` になり、B1 と同じ「古いキャッシュ」になる。
  - 逆向きの例: 数式セルに定数を書いて restore した場合を考える。census に数式が戻らなければ、次のことが起きる。
    - 以後の検証（`ArrayAnchor`、`SharedFormulaMaster`、依存の拒否）が逐次と食い違う。
    - R3 の依存計算にもその数式が入らない。
  - spike は restore を保存時の text layer で行い、shadow の木と census には手を付けなかった。
    そのため、この経路は一度も通っていない（Appendix B の「What the spike does not show」にも無い）。
  - C2 の表と F11 の表は、`XmlDocument` の dirty の数え方まで書く。だが census については何も書かない。
- 修正案:
  - C2 に「`restore_cell` は shadow の派生状態をすべて snapshot の時点へ戻す」を足す。
    派生状態とは、census の `FCell`（kind、text、range、`has_cache`、`results_stripped`、`volatile`、`dynamic`）、
    依存グラフ、`vm_cells`、行の索引、`style_sensitive` のキャッシュである。
    実装として最も単純なのは、snapshot に「そのセルの `FCell`（あれば）」も含め、restore で census の項目を差し替えることである。
  - content restore（`s` だけ違う）でも同じことをする。
  - oracle 12（replay equality）に「census と索引の比較」を足す。replay した shadow とライブの shadow の census を比べる白箱テストで、
    出力だけでなく以後の拒否が揃うことも確かめる。
  - oracle 1 の固定ケースに次の 2 つを足す。
    - 上の例（数式を定数で上書きして書き戻し、入力を変える。両側で `F` がキャッシュ無し）
    - 数式セルを定数で上書きして元の数式へ戻し、その後 array や shared の拒否が逐次と一致すること
  - oracle 9 に「`restore_cell` が census を戻さない」変異を足す。

### M3（Major）: R2 は restore したセルを無条件に changed set から外す。これは §4.4 の「構造操作が閉じたセルは所属を保つ」と矛盾し、`Ok` のまま狭い `dimension` を生む

- 根拠:
  - §4.4 の changed set は「current state differs from snapshot」**と**、「earlier epochs … not back … when a structural op closed their snapshot」の和である。
    閉じたセルは所属と content-changed の印を保つ（§4.4 "Epochs close snapshots"）。
  - 一方 §7.1 R2 は「If a target is back in its snapshot state …, it is restored … **and the cell leaves the changed set**」と、無条件に書く。
    次の箇所も同じ前提で書かれている。
    - §7.2 step 4「A cell … R2 restored … grows nothing」
    - §6.1「A write-back that … R2 restores grows nothing」
    - §9.1「A cell … R2 restored contributes no row」
    - oracle 4「a restored cell keeps the Base's bytes byte-for-byte」
  - 例を示す。`dimension` が `A1:C5` のシートで、`F20` は Base では存在しない。
    - epoch 0 で `set_number(F20, 6)` を実行する。`F20` は changed set に入り、content-changed になる。
    - `insert_rows(100, 1)` を実行する。`F20` は動かないが、同じシートなので snapshot は閉じる。
    - epoch 1 で `set_number(F20, 7)`、`set_number(F20, 6)` を実行する。`F20` は epoch 1 の snapshot（6）に戻る。
      R2 の文言どおりなら、`F20` は changed set から出る。
    - 保存時の `dimension` は「Base の範囲を構造操作が書き換えたもの」を changed set で広げる（C5）。
      `F20` はもう changed set に無いので、範囲は `F20` を覆わない。
      SheetJS は `!ref` の外のセルを読まないので（§6.1、§14）、値が `Ok` のまま読み手から消える。
      locality（oracle 4）の許容行にも `F20` が入らない。
    - `dimension` の Journal 側の述語は「changed set のセルを覆う」なので、この誤りを通してしまう。
  - キャッシュの面は、今の規則ではたまたま守られる。content-changed の印が消えても、そのセルを読む数式は、
    同じシートを参照するので ADR-0018 §3.7 の strip 記録に入るからである。ただし、それは §3.7 の対象の取り方が広いことに頼っている。
    シート操作（m1）や、将来の P5/P6 では保証されない。
  - さらに、epoch 1 の snapshot のバイトは Base のバイトではない。そのため、oracle 4 の「keeps the Base's bytes」は、正しい実装でも成り立たない。
  - spike は 1 epoch だけなので、この矛盾は表に出ていない。
- 修正案:
  - R2 を次のように直す。「restore したセルは、**この epoch の**差分を失う。前の epoch から持ち越した所属と content-changed の印は残る」。
    実装上は、セルごとに「前の epoch から持ち越した印」と「今の epoch で snapshot と違う」の 2 つを持ち、和を changed set とする。
  - §7.2 step 4、§6.1、§9.1、oracle 4 の文言を「epoch 0 で restore したセル」（または「持ち越しの印の無いセル」）に限る。
  - J2 の oracle 1 固定ケースに上の例を足す（`F20` が両側の `dimension` に入る）。
  - `dimension` の Journal 側の述語に「Base と P で異なる、生き残ったセルをすべて覆う」を足す。changed set の定義に頼らない形にする。
  - oracle 9 に「restore が持ち越しの印も消す」変異を足す。

### M4（Major）: C6 は「再描画したノードの新しい span」だけを記録するとする。だが Layer 2 は clean なノードも `src[span]` から写すので、変更した part では**すべての**ノードの span を付け替える必要がある

- 根拠:
  - `ooxml-core` `xml/write.rs` の `write_node` は、`subtree_dirty` でないノードを `src[span.start..span.end()]` で写す。
    dirty な親の開始タグと終了タグも、span から写す。`src` は `XmlDocument::source()` である。
  - C6 は、出力を「the parse source of every `XmlDocument`」にする。そのとき、変更した part の中で、最初の置き換えより後にある
    clean なノードとテキスト（`RawText` の span）は、新しい part のテキストの中で位置がずれる。
    C6 の文言「record each re-rendered node's new span」をそのまま実装すると、2 回目のセッションの保存で、
    ずれた範囲のバイトを写す。結果は壊れた XML（または別のセルのバイト）で、しかも `Ok` になる。
  - ずれる対象は、ADR の例にあるセルだけではない。同じ part の `row`、`sheetData`、`mergeCells` なども、
    2 回目のセッションで子が 1 つでも dirty になれば影響を受ける。
  - oracle 5、12、13 の 2 回目のセッションのケースで見つかる見込みは高い。ただし規範文が誤った実装へ誘導している。
    また、C6 のコスト（oracle 10）を「再描画したノードだけ」で見積もると、低く出る。
- 修正案:
  - C6 を次のように直す。「出力の直列化の間に、変更した part の**すべての**ノードとテキストについて、新しい位置を記録する
    （clean なノードは、元の span に、その part でそれより前に生じた長さの差を足す）。
    raw-copy した part の span はそのままでよい」。
  - 別案として、変更した part だけを出力から parse し直す方法もある。その場合、snapshot（C2）は保存で捨てるので、ノードの同一性は要らない。
    ただし census と索引の付け替えが要る。どちらの案にするかを C6 に書き、コストを oracle 10 に入れる。
  - oracle 9 に「C6 が clean なノードの span を付け替えない」変異を足す。oracle 12 の 2 回目のセッションの固定ケースに、
    「session 1 で `A1` と `A3` を書き、session 2 で `A5`（同じ part の後ろ）を書き戻す」を足す。

### M5（Major）: `truncate_fast` は、1 つの op が複数のセルを target にする場合（shared の hand-off）に、replay と違う結果を出す

- 根拠:
  - §10.3 は「for each cell a cut op targeted, `restore_cell` puts back its snapshot, then the ops that targeted that cell **from the one
    that took its current snapshot** up to `seq` are applied again」とする。
    replay へ逃げる条件は「structural or barrier, or a cell has no snapshot record」だけである。
  - §4.4 により、keyed op でも複数のセルを target にしうる。master への書き込みは、hand-off の新しい master も target にする。
  - 例（`o1_shared.xlsx` の形）:
    - 残す op k1 は `set_number(D2, 1)` である。D2 は master で、hand-off により `C2` が新しい master になる。
      k1 は `C2` の snapshot（元の member のノード）を取る。
    - 切る op k2 は `set_number(C2, 2)` である。`C2` は master なので、次の member へ再び hand-off する。
    - `truncate_fast(k1)` は、k2 の target（`C2` と次の member）を snapshot へ戻す。
      その後、`C2` の snapshot を取った op k1 を再適用する。
    - だが、k1 の主な target `D2` は restore されておらず、すでに定数である。そのため、再適用しても hand-off は起きない。
      `C2` は `<f t="shared" si="1"/>` の member のまま、master の無い group に残る。
    - replay は `C2` を master にする。結果は食い違い、出力は修復が要るファイルになりうる。
  - spike は shared-formula セルで replay へ逃げた（§10.3 末尾）。この経路は試されていない。oracle 14 の生成器も、
    「構造操作をまたぐ切断」（J3）だけを明記していて、hand-off を明記していない。
- 修正案:
  - §10.3 の fast path の条件に「切る op と再適用する op がどれも target を 1 つだけ持ち、その target が shared/array の
    master または member でない」を足す。そうでなければ replay へ逃げる。
    （別案は、再適用する op の target 全体の閉包を restore することである。だが、同じ閉包を再帰的に追う必要があり、単純ではない。）
  - oracle 14 の生成器に、shared master を書く op を残し、新しい master を書く op を切るケースを足す。
    J0 から fixture に shared group があるので、J3 の生成器に明記する。

### M6（Major）: oracle 1 のキャッシュ述語は、構造操作による古さを表せない。そのため、M3（round 7）の構造 strip を落とす変異は、固定ケース 1 つでしか捕まらない

- 根拠:
  - 述語は「a kept `<v>` is byte-equal to the Base's for that cell id, and no cell it reads (ADR-0003 dependents, transitively) differs in P from the Base」である。
    後半は**値の変化**しか表さない。ADR-0018 §3.7 が strip するのは、値が変わらなくても結果が変わる数式である。
    - `ROW()`、`CELL`、`ADDRESS`、`OFFSET`（文字列引数）、`INDIRECT`
    - 中身がずれる列全体の参照（`MATCH(x, A:A, 0)`）
    - 縮んだ範囲と名前（`ROWS(MyRange)`）
    - `#REF!` になった参照
    これらはすべて「読むセルが P で同じ」まま古くなる。
  - 本文は、このセッションの判定を戻らない形で持つ（§4.2 `calc`、R3 の構造部分）。しかも §9.3 は「no second engine」とし、
    oracle 1 をその守りとする。だが、この述語では、構造 strip 記録を捨てる変異（oracle 9、「structural strip records dropped」）を、
    proptest の生成ケースがほとんど捕まえられない。
    捕まえるのは J2 の固定ケース 1 つ（`insert_rows` の後のスタイル patch）だけである。
  - B1（round 7）は「述語が半分しかなかった」ために 20,000 ケースが誤りを見逃した例である。同じ形の穴が、構造部分に残っている。
  - 述語の「Journal's strip set ⊆ the sequential strip set」は、strip しすぎを防ぐ向きである。安全側の向きは無い。
- 修正案:
  - oracle 1 の述語に、構造操作の向きを足す。「逐次側で構造操作の plan が strip したセル（ADR-0018 §3.7 / ADR-0009 §12 の集合を
    参照側で記録する）のうち、その後に内容を書かれず、Journal 側でまだ `f` を持つセルは、Journal 側でキャッシュを持たない」。
    逐次の参照実装の白箱記録で判定できる。
  - または、述語の後半を「…differs in P from the Base, **and no structural op of the side touched a sheet it references or it is
    position-dependent**（§3.7 の判定をそのまま使う）」に広げる。
  - oracle 1 の生成器の数式に、`ROW()`、列全体の参照、名前付き範囲を明記する（J2）。

### m1（Minor）: `StructuralCalc` と R3 は ADR-0018 §3.7 だけを挙げる。シート操作（ADR-0009）の strip 種と calcChain の規則が入っていない

- 根拠: §4.1 の構造操作には、`add_sheet`、`delete_sheet(_strict)`、`rename_sheet`、`move_sheet` が含まれる。
  ADR-0009 は操作ごとに別の strip 種を持つ。
  - `add_sheet`: 3-D の範囲に入るもの、`SHEET()`/`SHEETS()`、`Everything`、新しく解決する名前
  - `move_sheet`: 3-D の範囲の所属が変わるもの
  - `delete_sheet`: calcChain を外す
  C3 と §4.2 のコメント（「ADR-0018 §3.7's strip set」）のままでは、これらの操作の記録の中身が決まらない。
  また、`delete_sheet` は `formula_moved` の語に収まりにくい。
- 修正案: `StructuralCalc` を「その操作の ADR（ADR-0018 §3.7 または ADR-0009 §12 の各操作）が逐次で strip するセルの集合」と定める。
  `formula_moved` を「calcChain を外す条件を満たした」（`removes_calc_chain`）に改名する。
  J2 の固定ケースに、`move_sheet` で 3-D 範囲の所属を変えるケースを足す。

### m2（Minor）: §6.1 の `would_strip` の条件が R3 と揃っていない

- 根拠: §6.1 は「a recorded structural strip, or a transitive dependent of a content-changed cell, and not itself content-changed」とする。
  R3 の「style-only の変更の依存先（`style_sensitive()` のとき）」と、「保存時に `f` を持つ」の 2 つが抜けている。
  `CELL("format", …)` を使う workbook では、normalized read はキャッシュを見せるのに、保存は strip する。
  そのため Reload law と oracle 13 が落ちる（黙った誤りにはならない）。
- 修正案: `would_strip(cell)` を「`cell` が R3 の strip 集合（§7.1）に入る」と定義し、条件を R3 からだけ引く。

### m3（Minor）: Delivery の段階と oracle の対応がずれている

- 根拠:
  - oracle 5 は「after a save (C6)」を含み、oracle 12 は「a save and a second session」の固定ケースを持つ。どちらも J1 にある。
    だが、保存後の log の扱い、commit mark、echo record（§10.1）は J3（「compaction, echo records」）である。
  - Delivery の前文は「C1–C8 before J1's Commit」とする。一方 J0 は「redoes it on C1–C8」で、J0 で C2 を使う（「the shadow with R2 on C2」）。
  - oracle 10 の「save cost (§7.2 steps 2–5) … with C6 and C7 included」は、直列化（step 6）と C6（step 7）を範囲から外している。
- 修正案: §10.1 の最小部分（log を空にし、commit mark を置き、snapshot を捨てる）を J1 に移す。echo record と compaction は J3 に残す。
  前文を「C2 は J0 まで、残りは J1 の Commit まで」にする。oracle 10 は「steps 2–7」にする。

### m4（Minor）: §10.2 は、spike の 2 回目のセッションの結果を、注記なしで引いている

- 根拠: §10.2 は「In the spike a second session of the same ops reversed … saved the Base bytes at 1.1M cells」とする。
  Appendix B の M1 は、この種の通過が「text layer が新しい Base を開いて pin を運んだため」で、shadow だけでは成り立たないと書く。
  §10.2 の主張は、C6 の設計の証拠として読まれうる。
  §7.3 の「C6 … is part of the measured save」も、spike で測ったように読める。
- 修正案: §10.2 に「text layer の pin による。C6 の証拠ではない（Appendix B）」と書く。§7.3 は「J0 で測る save に含める」とする。

### m5（Minor）: C2 と C7 は `ooxml-core` の Layer 2 を変える。docx と pptx にも影響するのに、それが書かれていない

- 根拠:
  - C2 は「`XmlDocument` needs a count of dirty descendants」とする。C6 は parse 元と span の付け替えである。
    C7 は「Layer 2 renders a re-rendered element with no children self-closing」とする。
  - 今の `write_node` は、dirty でない開始タグと終了タグを元のバイトから写す。子をすべて失った要素は `<x …></x>` のままになる。
    これを変えると、docx/pptx の編集結果のバイトも変わる。
  - ADR-0001 の保存規則と、各形式の oracle（python-pptx など）に影響しうる。
- 修正案: C7 の self-closing は、`ooxml-xlsx` 側で `c` を `own_dirty` にして済ませるか、オプトインの API にする。
  §7.4 に「C2 と C6 は `ooxml-core` の変更で、全形式の ADR-0001 テストを通す」と書く。
  Q7 の事実にも、この 1 行を足す。

### m6（Minor）: Q7 の事実が (A) の側にだけ付いている。段階的な選択肢も無い

- 根拠:
  - 半分のキャッシュ述語（`src/project.rs`）は、`EJ_MODE=compile` の oracle 1 でも同じものを使う。
    そのため、(B) の 20,000 ケースも、依存先の古いキャッシュを確かめていない。
    (B) は逐次の API で strip するので、B1 の誤りは無い。だが、証拠の強さは (A) と同じく割り引いて読む必要がある。
  - C1〜C8 は、規模の見積もりが無い。とくに `ooxml-core` の C2、C6、C7 と、本ラウンド M2 の census の同期である。
  - 「(A) を C6 なしで出す。保存後は出力を開き直す（revision 7 の §10.1）」という中間案がある。
    これは M1（round 7）と本ラウンド M4 の危険を避け、コストは約 4×（開き直し ≈ 2.3×）になる。
    C6 が入るまでの段階として選べるのに、Q7 に無い。
- 修正案: Q7 の事実に、(B) の証拠も同じ述語であることと、C1〜C8 の規模（どの crate、どの不変条件）を足す。
  選択肢に「(A′) (A) で、C6 が入るまでは開き直す」を足す。

## nit

- n1: R1 の「value, type and rich runs; formula text and kind; resolved style」は、網羅の列挙に読める。
  P は h1 と `cm`/`vm` の写像も持つ。「P がセルについてモデル化するものすべて（例: …）」と書く。
- n2: R4 と oracle 1 の `sst/@count` は「the Base's count」を使う。だが、Base に `sst` part が無い場合と、`@count` が無い場合
  （省略可能な属性）の値が決まっていない。「無ければ Base の `t="s"` セル数」などと定める。
- n3: R4 は「session items that **a cell** references」とする。`row/@s` や `col/@style` から参照されうるなら、
  「shadow のどこかが参照する」と書く。参照されえないなら、そう書く。
- n4: §4.4 の「a carried strip (§7.1 R3) … is part of the snapshot」の「carried」は、spike の pin の語である。
  C3 と C6 の後では、保存で strip したノードが木にあるだけなので、「保存で strip されたノード（`v` なし）」と書けば足りる。

## その他の確認結果（指摘なし）

- **R3 の strip 集合の向き**: 「content-changed を引く」は、内容を書いたセルのキャッシュが自分の write で決まる、
  という ADR-0003 §3.1a と整合する。content restore と R2 restore のセルが strip 対象に戻ることも、§4.4、§6.1、oracle 1 の固定ケースと一致する。
- **最終グラフで依存を求めること**: R3 は保存時の依存グラフで依存先を求める。逐次は書き込み時のグラフで求める。
  両者の違いは、依存関係を変える操作（数式の書き換えは content-changed、構造操作とシート操作は自分の strip 種）で覆われる。
  反例は見つからなかった（ADR-0009 の reorder の種を含む。m1 の記録がある前提）。
- **§7.2 の stale の印と §9.3**: 印を step 2 の最初に付けるので、panic でも残る。replay の失敗が `JournalBroken` になることも、
  §12 と oracle 3 で一致する。
- **`sst/@count` の running delta**: 構造操作の削除を引く規則と、出力から直接数える Journal 側の述語は、同じ値を与える
  （Base の `@count` が実数と違っていても、差だけを足すので一致する）。
- **R4 の正規の番号付けと F12**: round 7 の確認どおりで、変更は無い。
- **相互参照**: §2 の図、§6.3 の表、§7.4 の F11 の表（6 項目が §3.2 の 6 痕跡と対応する）、oracle 9 の round 7 変異の一覧、
  revision 8 の項目の round 7 対応表は、本文と一致した（m3 と m4 の箇所を除く）。
- **公開リポジトリの規則**: 本文にローカルパス、ユーザー名、非公開プロジェクト名は無い。

## 判定

converged: no（Blocker 0 / Major 6 / Minor 6 / nit 4）

## Owner response（revision 8 の修正, round 8 対応）

spike のコードは変えていない。round 7 の修正が新しい穴を作った点は、より単純で保守的な規則か、replay への退避で直した。
spike が通らない経路は ADR の Appendix B（"Review round 8"）と spike の README に書いた。

- **M1**: 採用。R1 は §4.4 の状態の等しさで判定し、さらに「以後の検証が読む状態（census の kind と group、依存、merge、table、array、`cm`/`vm`）を変えない」ことを C1 が報告する場合に限る。shared/array のセル（master と member）への書き込みは R1 で飛ばさない（両側で group が割れる）。R1 は出力にも拒否にも影響しない最適化になった（§4.5、§7.1、Consequences）。oracle 1 の固定ケースを「member の書き戻しの後に master と member を 1 回の `write_cells`」に直し、除外行の理由から R1 を消し、oracle 9 に変異を足した。
- **M2**: 採用。snapshot がセルの派生状態（census の `FCell`、依存、`vm`、行の索引）を持ち、`restore_cell` がノードと一緒に戻す（§4.4、R2、C2）。oracle 12 に shadow の派生状態の白箱比較、oracle 1 に `has_cache` の固定ケース、oracle 14 に `truncate_fast` 後の `delete_sheet_strict` の固定ケース、oracle 9 に変異を足した。
- **M3**: 採用（修正案どおり）。所属を「前の epoch からの持ち越しの印（死ぬか保存まで消えない）」と「今の epoch の差分」に分け、restore は後者だけを消す（§4.4、R2、§6.1、§6.3、§7.2 step 4、§9.1、§9.2、oracle 4）。`dimension` の Journal 側の述語に、changed set に頼らない「target になり、Base のセルと値・型・解決スタイル・有無・数式かどうかが違うセルを覆う」を足した（数式の text は構造操作が書き換えるので外した）。J2 に `F20` の固定ケース、oracle 9 に変異。
- **M4**: 採用。C6 は再描画した part の**すべての**ノードとテキストについて出力上の span を記録する（raw-copy した部分木の中は差分を足す）。代替（その part を出力から parse し直し、census を作り直す）を明記し、J1 で選ぶ。C6 が入るまでは保存後に開き直す（Q7 (A′)）。oracle 12 に `A1`/`A3` → `A5` の固定ケース、oracle 9 に変異、oracle 10 にコストを足した。
- **M5**: 採用。`truncate_fast` は、切る op か再適用する op が複数の target を持つ呼び出しに属するか、shared/array のセルを target にするとき replay に退く（§10.3、§4.5）。oracle 14 の生成器に hand-off の切断（J3）、oracle 9 に変異。
- **M6**: 採用（前者の案）。oracle 1 のキャッシュ述語に構造の向きを足した: 逐次側の構造操作が strip したセル（参照側の白箱記録、id で）のうち、Journal の出力で `f` を持つものはキャッシュを持たない。生成器に `ROW()` などの位置依存の数式、列全体の参照、名前付き範囲、3-D 範囲を足し（J2）、固定ケースを足した。「構造 strip 記録を捨てる」変異は生成ケースで捕まる。
- **m1**: 採用。`StructuralCalc.strips` を「その操作の ADR（ADR-0018 §3.7 か ADR-0009 §12）が逐次で strip する集合」とし、`formula_moved` を `removes_calc_chain` に改名して `delete_sheet` を含めた（§4.2、R3、oracle 1）。J2 に `move_sheet` と 3-D 範囲、`delete_sheet` と calcChain の固定ケース。
- **m2**: 採用。`would_strip(cell)` は「R3 の strip 集合に入る」とだけ定義した（§6.1）。
- **m3**: 採用。前文を「C2 は J0 まで、残りは J1 の Commit まで」にし、§10.1 の最小部分を J1 に移し、oracle 13 の固定ケースを J1、生成ケースを J3 にした。oracle 10 は steps 2–7。
- **m4**: 採用。§10.2 に text layer の pin によること（C6 の証拠ではない）を書き、§7.3 は「J0 で測る save に含める」にした。README にも注記。
- **m5**: 採用。C7 は `ooxml-xlsx` で子の無い `c` を空要素のノードに置き換え、Layer 2 の既定の出力は変えない。C2 と C6 は `ooxml-core` の変更で、全形式の ADR-0001 テストと python-pptx オラクルを通すと §7.4 と Q7 に書いた。
- **m6**: 採用。Q7 に (B) の証拠も同じ半分の述語であること、C1〜C8 の規模（crate と不変条件。行数の見積もりはしない）、R1 が任意になったこと、選択肢 (A′)（C6 までは開き直す、推定約 4×）を足した。推奨は (A) を (A′) 経由で。
- **n1**: R1 は §4.4 の状態の等しさに加え、ノードの残り（h1、`cm`/`vm`）を変えないことを求める形にし、P の列挙をやめた。
- **n2**: `sst/@count` は、Base の `sst` に `@count` が無ければ出力にも無く、`sst` part が無ければ Base の count を 0 とする（R4、oracle 1）。
- **n3**: R4 に「part A では session の項目を参照できるのは `c` だけ（`row/@s`、`col/@style` を設定する op が無い）。C4 はそれでも両方を走査する」と書いた。
- **n4**: 「carried strip」を「保存で strip されたノード（`f` があり `v` が無い）」に直した（§4.4、R3、oracle 9）。

まだ証明されていないもの: 持ち越しの印、構造 strip の記録と構造の向きの述語、C6 の span の付け替え、派生状態の restore、hand-off をまたぐ `truncate_fast` の退避は、どれも spike で走っていない。J0〜J3 の固定ケースと変異で確かめる。
