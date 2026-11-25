# ADR-0024 レビュー（Opus, round 10）

対象: `plan/xlsx-edit-journal-r8` の `docs/adr/0024-xlsx-edit-journal.md`（revision 8, spike 検証の取り込み後、コミット `0544979`）。
今回の目的は、これまでのラウンドと違う。文書が spike の証拠と食い違っていないか、そして「revision 7 Accepted / revision 8 Proposed（Q7 待ち）」としてマージしてよいかを確かめる。
照合には、spike（`spikes/xlsx-edit-journal/`、`6339047` 相当）のコードとテスト、README の "Revision 8 verification"、`main` の `ooxml-xlsx`（`f88d02c` 相当）を使った。

走らせたもの:

- `cargo test --release`（既定の 512 ケース）: 全スイートが通った。
- `./mutants.sh` を次の 6 つで走らせた。どれもテストを落とす。
  - `adr_r3_formula_only` と `adr_r3_seed_raw`（各 3 件）
  - `adr_dead_mark_drops_fx`（1 件）
  - `adr_missing_is_content`（2 件）
  - `mutant_c7_created_keeps_s0`（1 件）
  - `mutant_close_own_sheet`（5 件）
- README の件数と 1 件ずれる行がある。proptest の乱数による差で、固定ケースはどれも README のとおり落ちる。

## 結論（先に）

Blocker も Major も無い。この文書は、「revision 7 Accepted / revision 8 Proposed（Q7 待ち）」としてマージしてよい。

- **規範の規則は、spike が走らせているものと一致する。** 確かめた規則は次のとおり。
  - R3 の種（P と同じ比較。作られた空の `c` は style のみの変更）
  - strip の判定（`f`+`<v>`、および配列の結果セル）
  - 引き算をしないこと
  - calcChain（id で数え、死んだ id を含める）
  - 構造操作で、ブック全体の snapshot を閉じること
  - 持ち越しの印
  - R4 の `sst/@count` の差分
  - `truncate_fast` の退避の条件
  - C7 の `xf` 0
  - R1 を C1 まで止めること

  それぞれ、`src/shadow.rs` の `calc()`、`holds_cache_at_save()`、`apply_structural()` と、該当するテストで照合した。
- **verified と書かれた項目は、検証されている。** Appendix B.2 の "Verified" の各項目について、テスト名と変異を確かめた。
- **unproven の一覧には抜けがある（m4）。** spike が走らせていない part A の経路が、いくつか一覧に無い。
  どれも verified とは書かれていないので、Minor とする。
- **Q7 の数値は、表の値としては正しい。** ただし、2 点で読み手を誤らせうる（m1, m2）。
  - 「2× の目標を満たす（just）」は 2.01× の 1 回の計測による。
  - (A′) の 4.64× は、再オープンの後に依存グラフを作り直すコストを含まない。

  どちらも推奨 (A) を変えない。
- 公開リポジトリの規則: ローカルパス、ユーザー名、非公開の名前は無い（ADR、README、spike のソースを grep した）。

**converged: yes**

## 前ラウンドの解消状況

round 9 の Owner response（M1, M2, m1〜m5, n1, n2）は、どれも本文に入っている。spike 検証の取り込みで、さらに次が直った。

- **round 9 M1**（ブック全体の snapshot を閉じる）: 本文（§4.4 "Epochs close snapshots"）と spike（`apply_structural` の `closes`）が一致する。
  `mutant_close_own_sheet` は 5 件のテストを落とす。
- **round 9 M2**（引き算をしない）: 一致する。spike の検証で、種を raw で比べる文言が別の誤りを生むことが分かった。
  その文言は、P と同じ比較に直された（`adr_r3_seed_raw`）。
- **round 9 m1**（registry part を Base のノードへ戻す）: 本文にある。
  spike は text layer がテキストを比べるので、この規則そのものは走っていない（m4）。
- **round 9 m5**（呼び出しの境界での truncate）: 本文にある。spike には `TruncateInsideCall` のテストが無い（m4）。
- **round 9 m2〜m4, n1, n2**: 解消。

## Findings

### m1（Minor）: 「2× の目標を満たす」は、計測（2.01×、1 回）より強い。§7.3 の先頭の箇条書きは、古いままである

- 根拠:
  - §7.3 の第 2 表の値は、135 ms = **2.01×** である。README の "Measurements" に回数の記載が無い（第 1 表は「two runs per size」）。
  - それでも、次の箇所が「満たす」と書いている。
    - 改訂履歴 L382-383「so the 2× target holds only with the kept shadow」
    - §7.3 L1096「The 2× target is met only with the kept shadow, and only just」
    - Q7 L1968「The 2× target is met, just」
    - README の Reading
  - 2.01 は 2 を超えている。1 回の計測なので、雑音の範囲とも言える。だが「満たす」とは言えない。
  - §7.3 L1065「The save meets ADR-0018 §3.8's 2× target at both sizes」は、`ad74d32` の表についての文である。
    すぐ下の表が 2.01× を示すので、今の状態の主張として読まれる。
  - 推奨 (A) の根拠「(A) is the only option that can meet ... 2×」は、text layer の項が core で消えるという見込みに立つ。
    この見込みは §7.3 に書いてあり、それ自体は妥当である。
- 修正案:
  - 「about 2× (2.01×, one run): the target is reached only within noise, with the text layer; J1 must show it on C2–C8」のように書く。
  - 計測の回数を書く。§7.3 L1065 には「(at `ad74d32`)」を付ける。

### m2（Minor）: Q7 の (A′) の 4.64× は、再オープン後の依存グラフの作り直しを含まない。§7.3 の (A′) の 2 行は、測っている範囲が違う

- 根拠:
  - `src/bin/measure.rs` の `a_prime` が測る範囲は、行によって違う。
    - 1 行目「save 155 + reopen 158 = 313 ms = 4.64×」の reopen は、`j.normalized(A1)` 1 回の時間である。
    - 2 行目「second session, (A′) … save 142 ms (2.11×)」は、保存だけで、reopen を含まない。
  - 同じ表の 2 回目のセッションの append は、(A) で 25 µs/op、(A′) で 81 µs/op である。
    README によれば、その差は再オープンした shadow での依存グラフの構築である。
    2,000 op では約 112 ms、つまり `to_bytes` の約 1.7 倍が、(A′) で保存のたびに次のセッションへ持ち越される。
  - Q7 は、(A′) のコストを「**4.64×** a sequential `to_bytes` per save」とだけ書く。
    owner が (A′) を J1 の出荷形として受け入れるかどうかを判断するには、この持ち越しのコストも要る。
  - 推奨は (A) で、(A′) は途中の段階なので、判断の向きは変わらない。
- 修正案:
  - Q7 の (A′) と Facts に「plus the dependency-graph rebuild on the next write after each save (second session 81 vs 25 µs/op, about 1.7× a `to_bytes` at 2,000 ops)」を足す。
  - §7.3 の (A′) の 2 回目のセッションの行に「save only, reopen not included」と書く。

### m3（Minor）: R3 の `fullCalcOnLoad` の条件が、構造操作について、spike とも逐次とも合っていない

- 根拠:
  - §7.1 R3 の条件は「`fullCalcOnLoad` is set when the strip set has a cell or the set of formula cells changed」である。
  - spike は、`calc.chain_removed`（構造操作が数式セルを動かしたか消したか、または死んだ id の `fx`）も `formula_set_changes` に入れる。
    そのため、calcChain を消すときには必ず `fullCalcOnLoad` も立てる（`src/shadow.rs` L338-339、`needs_full_calc`）。
    本文には、このことが書かれていない。
  - 逐次の構造操作は、ADR-0018 §3.7 の依存先（`deps.formulas`）が 1 つでもあれば、`fullCalcOnLoad` を立てる。
    キャッシュを持たない数式でも立てる（`sheet_ops/grid.rs` L1033）。
  - 一方、C3 の記録（§7.4 C3 の行、および spike）は、「only cells holding a cache are recorded, as `plan_strip` does」である。
    §4.2 の `StructuralCalc.strips` の注釈も、この区別をしていない。
  - そのため、J2 の実装者が spike のとおりに記録すると、次の場合に結果が分かれる。
    構造操作が触れる数式が、どれもキャッシュを持たない場合（よくある例は、openpyxl などが `<v>` なしで書いた数式）である。
    逐次は `fullCalcOnLoad` を立て、Journal は立てない。
  - oracle 1 の述語は「set **if** …」の片方向なので、どちらの食い違いも捕まらない。
  - 影響を受けるのは、もともとキャッシュの無い数式だけである。そのため、Q1 (A) の意図（古い値を見せない）は崩れない。それで Minor とする。
- 修正案:
  - `StructuralCalc` に `needs_full_calc: bool`（その op の ADR が `fullCalcOnLoad` を立てる条件を満たした）を足す。
  - R3 の条件を「… or the formula-cell set changed, or a session structural op met its ADR's `fullCalcOnLoad` condition (`calc.needs_full_calc`)」とする。
  - spike の「calcChain を消すなら立てる」は、この条件で包むか、意図して強くしていることを Appendix B.2 に書く。
  - J2 の固定ケース: キャッシュの無い `=B!A1` と、`insert_rows(B, 0, 1)`（両側で `fullCalcOnLoad`）。

### m4（Minor）: 「Still unproven」の一覧に、spike が走らせていない part A の経路が抜けている

- 根拠:
  - 一覧（改訂履歴 L390-393、Appendix B.2 L2455-2459、Q7 L1986-1989）は、次を挙げる。
    - シートの操作
    - splice
    - barrier の op
    - P1
    - round 8 M5 の hand-off の切断
    - oracle 12 と 14 の white-box の比較
    - C6 の span walk
    - style に敏感なブック
    - 1.1M セルでの計測
  - spike の `Op`（`src/op.rs`）とテストを見ると、次も走っていない。
    - **`set_dynamic_formula`**: spill の結果セル（R3 の「array (or spill) anchor」の spill の部分）、metadata の述語、派生状態の `vm` を含む。
      fixture に動的配列が無い。
    - **リッチ文字列と共有のリッチ文字列**: C8 の行は「Partly: no runs」と書いている。だが一覧には無い。
    - **行・列の属性の op**: §10.3 は「not run」と書いている。だが一覧には無い。
    - **呼び出しの境界での truncate**（`TruncateInsideCall`、round 9 m5）: spike にテストが無い。
      round 9 の Owner response も、これを「まだ証明されていない」と書いていた。
    - **registry part を Base のノードへ戻すこと**（round 9 m1）: spike はテキストの比較で代用している。
    - **(A) で構造操作を含むセッションの後の 2 回目のセッション**: spike は、その保存の後で必ず再オープンする（C6 の行の Limit）。
    - **実際の `Deferred` の op の記録を white-box で比べること**: C3 の行の Limit にはある。だが一覧には無い。
  - どれも verified とは書かれていない。J0〜J3 の受け入れ条件と oracle で拾われる。そのため Minor とする。
    ただし、owner が Q7 で読む一覧として不完全である。
- 修正案: 3 か所の一覧に上の 7 項目を足す（改訂履歴、B.2、Q7）。Q7 では、まとめて 1 行にしてよい。

### m5（Minor）: C6 が J1 の前提条件かどうかが、§7.4 と Delivery と Q7 で食い違う

- 根拠:
  - §7.4 L1114: 「Each is a Delivery prerequisite of J1」（C6 を含む）
  - Delivery L1744: 「the rest of **C1–C8** (§7.4) before J1's Commit」と書いた直後に、「until C6 lands, the shadow is reopened from the output after each save (Q7 (A′))」と書いている。
  - Q7 (A′): 「lets J1 ship before C6. C6 then becomes a later cost optimization」
  - 実装者には、J1 の開始に C6 が要るのかどうかが読めない。
- 修正案: §7.4 と Delivery を「C1–C5, C7, C8 before J1's Commit; C6 before the end of (A′) (Q7), J1 runs on the reopen stand-in until then」にそろえる。

## nit

- n1: §6.1 の `would_strip` は「an array result of a **stripped** anchor」と書く。
  §7.1 R3 は「result cell of an array (or spill) anchor **in the strip set**」と書く。R3 にそろえる。
  anchor が strip 集合にあり、anchor 自身が `<v>` を持たない場合に、2 つの文言の結果が違う。
- n2: R3 の calcChain の「counted by id with dead ids included」と、oracle 1 の「a Base formula cell whose id died is a difference」は、読み方が逆に見える。
  結果は `removes_calc_chain` で一致する。R3 に「a dead id counts with the formula-ness its last write gave it; a Base formula cell deleted by a structural op is covered by `removes_calc_chain`」と書く。
- n3: §7.1 Consequences「its "skips on a group cell" mutant by oracle 1's fixed case」は、`mutant_r1_skip_group`（P で判定）のことである。
  R1 (b) を外しただけの変異は、C1 の (c) が「census kind and group」を見るため、J1 でも等価になりうる。
  oracle 9 の「J1 must test it」に、等価になりうることを添える。
- n4: §4.5 の Cost と Consequences の第 1 項目「plus R1's test … (2–3 µs per op)」は、R1 が動いていた `ad74d32` の値である。
  R1 を止めている間は、書き戻しの多いセッションで 25 µs/op（§7.3）になることを添える。
- n5: oracle 1 の固定ケースの一覧には、F13 の配列の結果セルの場合が無い。
  spike は `f13_same_value_on_merge_non_anchor_and_array_result` で走らせ、§4.5 も引いている（`set_number(G3, 4)`、`ArrayResultCell`）。一覧に足す。
- n6（spike）: `mutants.sh` の既定の一覧に、`mutant_c7_created_keeps_s0` が無い。README の表と oracle 9 には載っている。

## その他の確認結果（指摘なし）

- **規則と spike の照合**:
  - R3 の種は、`same_meaning`（P の比較）で決まる。`(None, Some(x))` は `!x.is_empty_content()` である。
  - 持ち越しの content の印も、同じ比較で付く（`apply_structural`）。
  - strip の判定は、`f`+`<v>` と、`in_array_result` で `<v>` を持つ結果セルである。
  - 引き算は、変異の下でしか起きない。
  - calcChain は、`chain_removed`（数式が動いたか消えた場合、`fx` の死んだ印）と、印と今の差分の `fx` で決まる。
  - `sst/@count` は、`closed_sst_delta` から、消えた `t="s"` を引く。

  本文の §4.4 と §7.1 は、これらと一致する。
- **verified の項目**: B.2 が挙げるテスト名は、どれも `tests/` にある。
  - `m3_round7_insert_before_a_formula_then_restyle_it`
  - `a_formula_overwritten_then_deleted_drops_calc_chain`
  - `c7_created_cell_restyled_in_a_later_epoch`
  - `m1_round9_*` の 3 件
  - `m2_round9_handoff_then_input_both_orders`
  - ほかも同様

  変異の表（README）と oracle 9 の一覧も対応する。等価な変異 2 つ（`mutant_r3_subtract_content`、`mutant_r1_group_cells`）は、理由と一緒に記録されている。
- **計測の転記**: §7.3 の第 2 表、改訂履歴、Consequences、Q7、oracle 10 の値は、README の表と一致する。
  - 49.8 µs
  - 135 ms / 2.01×
  - 155 + 158 = 313 ms / 4.64×
  - 25 vs 8 µs
  - 81 µs
  - 43 vs 307 ms
  - 799 vs 149 ms
- **改訂履歴と相互参照**:
  - "Revision 8, spike-verified" の各項目は、参照先の節（§4.4、§7.1、oracle 1、oracle 11、§7.3、B.2）に対応する。
  - R1〜R4、C1〜C8、oracle 1〜14 の番号に、古い参照は見つからなかった。
  - Status 行のラウンドごとの件数（7: B1+M5、8: M6、9: M2）は、各ラウンドのファイルと一致する。
- **(B) の扱い**: Q7 は、(B) の証拠が半分のキャッシュ述語に立っていたことを書いている。検証では、(B) を変異の一覧でしか再実行していないことも書いている。
  README の compiled-Commit の変異の行と一致する。
- **公開リポジトリの規則**: ADR、README、spike のソースと `mutants.sh` に、`/home/`、ユーザー名、非公開プロジェクト名は無い。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 5 / nit 6）

Minor と nit は、CI が通れば直してよい（再レビュー不要）。この文書は、「revision 7 Accepted / revision 8 Proposed（Q7 待ち）」として PR にしてよい。

## Owner response

Minor 5 件と nit 6 件を、すべて本文に入れた。Status 行と改訂履歴に、round 10 の項を足した。

- **m1**: 2.01× は 1 回の計測なので、「2× の目標には雑音の範囲でしか届いていない（spike では満たすと言えない）」と書き直した。J1 が C2–C8 の上で示す、とした（改訂履歴、§7.3、Q7、Consequences）。
  §7.3 の第 2 表に「one run」を足した。`ad74d32` の「meets 2× at both sizes」は「At `ad74d32` (historical; …) the save met …」とした。spike の README も同じに直した。
- **m2**: (A′) のコストに、次のセッションでの依存グラフの作り直し（81 対 25 µs/op、2,000 op で `to_bytes` の約 1.7 倍）を足した（Q7 の (A′) と Facts、§7.3 の箇条書き、Consequences、改訂履歴）。
  §7.3 の表の (A′) の 2 行に、何を含むかを書いた（1 行目: save と reopen、作り直しは次のセッション。2 行目: save only, reopen not included）。
- **m3**: `StructuralCalc` に `needs_full_calc` を足した（ADR-0018 §3.7: op の依存先に数式が 1 つでもあれば、キャッシュの有無に関係なく立つ）。
  R3 の `fullCalcOnLoad` の条件に、この句を足した。同時に、formula-cell 集合の変化を「セルの書き込みによる変化」に限った。構造操作は、その op 自身の条件で扱う。
  spike の「calcChain を消すなら立てる」は、`main` と同じ条件ではないと書いた。キャッシュの無い数式がずれるだけの場合を逃す。また、生き残る数式が読まない数式セルを消す op では、`main` より強い。
  C3 の行と B.2 の C3 の行に、spike が `needs_full_calc` を記録しないことを書いた。oracle 1 の `fullCalcOnLoad` は両方向（if and only if）にした。
  J2 の固定ケースとして、`=B!A1`（`<v>` なし）と `insert_rows(B, 0, 1)` を足した。未証明の一覧にも足した。
- **m4**: 指摘の 7 項目と m3 の項目を、3 か所の「Still unproven」に足した（改訂履歴、B.2、Q7。Q7 はまとめて 1 文）。
- **m5**: §7.4 と Delivery と Consequences をそろえた。C1–C5、C7、C8 は J1 の Commit の前提（C2 は J0 の前提、C3 の構造の部分は J2 の前提）。C6 は (A) の前提で、J1 は C6 なしに (A′) で出してよい。
- **n1**: §6.1 を R3 の文言（result cell of an array (or spill) anchor in the strip set）にそろえた。
- **n2**: R3 の calcChain に、「死んだ id は最後の書き込みが与えた formula-ness で数え、構造操作が消した Base の数式セルは `removes_calc_chain` が扱う」と足した。
- **n3**: §7.1 に `mutant_r1_skip_group`（P で判定）と書いた。oracle 9 には、R1 (b) を外しただけの変異は J1 でも等価になりうることを添えた。
- **n4**: §4.5 の Cost と Consequences の第 1 項目に、「`ad74d32` で R1 あり」と、R1 を止めている間は 25 µs/op であることを書いた。
- **n5**: oracle 1 の固定ケースに、`set_number(G3, 4)`（配列の結果セル、`ArrayResultCell`）を足した。
- **n6**: `mutants.sh` の既定の一覧に、`mutant_c7_created_keeps_s0` を足した。

Rust のコードは変えていない。
