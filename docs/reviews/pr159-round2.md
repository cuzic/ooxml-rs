# PR #159 レビュー（Opus, round 2）

対象: impl/xlsx-ej-j0a @ ef02bbd171d07e4b5d25105b99b46753de277804

## 結論（先に）
converged: **no**（Major 1）。round 1 の M1・m1(b)・m2・n1・n2 は挙動で解消を確認した（merge ef02bbd の
`--remerge-diff` は空、journal のテストは 55 件 green）。ただし実装者が残した不一致は、単なる拒否の差では
なかった。逐次側が `Ok` を返す no-op（`remove_formula`）で、Journal は `Internal` を返して **`broken` になり、
以後のすべての op を `Broken` で拒否する**。oracle 1 (i) 違反で、しかもセッションを失う。修正は小さいので、この PR で入れる。

## 前ラウンドの解消状況
- M1: 解消。`validate_strips` は `KeepStaleCaches` の分岐で `plan_strip(..)?` を呼び、結果を捨てる（`edit.rs:503-507`）。
  `plan_strip` の副作用は `ensure_index` の index cache だけで、これは逐次側でも同じ入力で作られる。
  公開の `KeepStaleCaches` は `validate_strips = false` で変わらない（fixed_22）。fixed_21 は両側 `OutOfOrder`、log は空。
  生成器は `Fx::OutOfOrder` を含む。
- m1 (b): 解消・健全。style-only の call（`Format`/`StyleIndex`/`NamedStyle`/`NumFormat`/`Font`/`Fill`/`Border`）は
  すべて `CellOp::StyleOnly` を通り（`setters.rs:178-221`）、`worksheet.rs:462` で内容に触れない。
  `*_with_format` と `DateTime` は `fields` に `Content` を含む（`op.rs:160-166`）ので、content restore は省かれない。
  数値書式は `<v>` を変えない（表示は読み手が計算する）。missing セルを `set_named_style` が作る場合は
  `present` が違うので、そもそもこの分岐に入らない。
- m2: 解消。`RestoreStats` の下限（value/formula ≥ 10、shared ≥ 3、inline ≥ 1、content ≥ 10）と、D2 の inline の revert。
- m3: J0b へのコメントで可（合意どおり）。
- n1: 解消（open で全シートの恒等ベクトル、`cell_key` は `&self`）。n2: 解消（`?` で伝播）。

## Findings

### M1（Major）: snapshot が読めないシートへの no-op を逐次側は受理し、Journal は `broken` になる
- 根拠: `journal/mod.rs:214-249`。`snapshot_cell` が `Err`（行順違いのシートでは `cell_spot` が index を読んで
  `OutOfOrder`）でも `apply_seq` は走る。no-op の `remove_formula` は index を読まないので `Ok` になり、
  `snapshot_error` の分岐が「the edit located the cell, so the snapshot should have」として `broken = true` にする。
  この前提は no-op では成り立たない。
  プローブ（コミットしていない）: `out_of_order_dependent_workbook`、`cell(1, "B1", RemoveFormula)` →
  逐次側 `Ok(())`、Journal `Err(Internal { seq: 1, source: OutOfOrder(..) })`、続く `num("B1", 1.0)` も `Err(Broken)`。
  同じセルへの `Font` は両側 `OutOfOrder` で一致する（拒否で終わる op は問題ない）。
  生成器は `Fx::OutOfOrder` で Sheet2 への op を Sheet1 に付け替えている（`oracle1.rs:208-213`）ので、この差は見えない。
  `edit_refusal()` が `None` のブックで起きるので、J0b/C1 に回せる「既知の差」ではない。
- 修正案: snapshot の失敗を `broken` の理由にしない。
  1. `snapshot_error` があり `apply_seq` が `Ok` のとき、shadow のそのシートの part が変わっていないこと
     （C2 の dirty mark、または part の generation）を確かめ、変わっていなければ op を受理して **追跡しない**
     （seq・log・key は通常どおり積み、R2 はそのセルを飛ばす）。index を読めないシートを変える op は、逐次側でも
     index を読む時点で拒否されるので、この分岐に入るのは no-op だけになる。
  2. part が変わっていたら、そのときだけ `broken`（本当の内部不整合）。
  3. fixed case 23（上のプローブ。両側 `Ok`、続く op も両側で一致、`finish` で P が等しい）を足し、
     生成器の Sheet2 の付け替えを外す。
  1 が重いなら、代わりに snapshot の失敗を `apply_seq` の**前**に `Unsupported { op: "cell on a sheet the journal cannot index" }`
  として返す（P1/P2 と同じ扱い、`broken` にしない）。この場合は ADR §4.5 の P 一覧に P3 として書く。
  いずれにしても、「受理された op で Journal が壊れる」経路をなくすことがこの PR の条件。

## 確認したこと（問題なし）
- `validate_strips` は shadow の `has_cache`/`results_stripped` を見るので、逐次側が既に strip した formula も
  shadow では再び検証する。`strip_positions` の拒否はシートか行の構造（`check_edit`、`rows`、`cells`）か census と
  tree の不一致で、ライブラリの書き込みでは生じない。最初の strip で逐次側が通っていれば shadow も通る。過剰拒否の経路は見つからなかった。
- 他の open PR（#144 pptx）とは重ならない。CI はレビュー時点で Rust のジョブが pending。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 0 / nit 0）
