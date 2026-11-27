# PR #164 レビュー（Opus, round 1）

対象: impl/xlsx-ej-j0b1 @ 759cf48
converged: no

## 結論（先に）
row/col の鍵導出（範囲は列ごとに 1 鍵、プロパティ別）、範囲外の行・列が `apply_seq` で拒否されてから
id を読む順序、P1・`set_dynamic_formula` の受理、`Internal` の全経路からの recover、C8 への置き換えは
いずれも正しい。Major が 1 件ある: **replay 自体が panic すると、Journal は途中まで再構築された状態のまま
印が残らず**、次の `append` がその壊れた状態の上で `Ok` を返す（プローブで再現）。修正は小さい。
converged: no（Blocker 0 / Major 1 / Minor 4 / nit 3）。

## 前ラウンドの解消状況
初回（PR #159 round 3 の m1/n1 はこの PR で対応済み: `part_unchanged` と 4 通りの単体テスト、
`debug_assert!` と `drop_fresh_once` による `should_panic` テスト。どちらも挙動で確認）。

## 実装者の逸脱の判定
- fixed 31 の `cm` ケースが到達不能（`main` の `remove_formula`/`set_formula` は動的アンカーで
  `ArrayAnchor`、共有マスターへの `set_dynamic_formula` は `SharedFormulaMaster`）: **受け入れ**。両側の拒否を
  fixed 31 が確かめ、`cm` は `state.rs` の `cm_is_part_of_a_formula_state` が白箱で押さえる
  （`mutant_state_ignores_cm` はこれで落ちる）。`mutant_dynamic_not_content` は fixed 32 の鍵数で落ちる。
- replay 比較で `sst/@count` を無視: **受け入れ（follow-up 必須）**。正規化は `xl/sharedStrings.xml` の最初の
  ` count="` だけを外し、`uniqueCount` と他パートは比較したまま。ただし原因は「`to_bytes` の出力が
  それ以前の `to_bytes` の有無に依存する」ことで、J1 の oracle 12（保存込みの replay 等価）では
  そのまま問題になる（m3）。
- fault の重み（dynamic 4、fault 約 1/3）: 受け入れ。全 step が発火する方が価値が高い。
- `keys_of(&Op)`（仕様は `&LoggedOp`）: 受け入れ。append と replay が同じ関数を使う点は満たす。
- stale 印を `append` のラッパーで立てる: 受け入れ。印を立ててから `apply_seq` までは読み取りだけ。
- P2 の延期: 条件付き受け入れ（m4）。

## Findings

### M1（Major）: replay 中の panic が Journal を途中状態のまま残し、次の呼び出しが黙って続行する
- 根拠: `replay.rs:113-124` の `recover` は replay の**前に** `self.stale = None` にし、`mod.rs:224-235` の
  `settle` は `self.stale.take()` で印を消してから `recover` を呼ぶ。replay は shadow を新しくし、
  `tracked`/`keys` を空にしてから op を流すので、途中の panic で
  「log は全件、shadow・`tracked`・`keys` は途中まで、`stale == None`、`broken == false`」が残る。
  R2 の中の panic では shadow は poisoned にならないので、次の `settle` も何もしない。
  プローブ（コミットしない）: `num(A1,9)`, `num(A1,1)`, `num(B1,5)` の後、`fail_restore_once = Some(true)` と
  `inject_apply_failure(Target)` を置いて `num(A2,5)` → recover の replay が 2 番目の op の restore で panic。
  結果: `stale=None broken=false poisoned=false log=3 keys 2 -> 1 tracked=1`、続く `append(num(C1,2))` は
  `Ok(4)`、shadow のバイトは双子（同じ 4 op）と**不一致**（`B1=5` が shadow に無いのに log にはある）。
  `rebuild()` も同じ（印なしで replay する）。ADR §4.5「a call is atomic」、§3.2、仕様 B6 の趣旨に反し、
  `Ok` での黙ったデータ欠落になる。replay は live と状態が違いうる（`sst/@count` が実例）ので、
  live で通った op が replay で panic する経路は「起きない」とは言えない。
- 修正案: replay が `Ok` で終わるまで印を残す。例えば `recover` で `self.stale = Some(mark)` のまま
  `replay()` を呼び、`Ok` のときだけ `None` にする（`rebuild` も replay の前に印を立てる）。あるいは replay の前に
  `broken = true` にして成功時に戻す（panic 後は `Broken`）。上のプローブを `replay.rs` のテストにする
  （`catch_unwind` の後、次の `append` が再 replay して `same(j, twin)`、または `Broken`）。

### m1（Minor）: replay の `tracked.clear()` を消す変異が journal の全テストを通る
- 根拠: `replay.rs:149` の `self.tracked.clear();` をコメントアウトしても
  `cargo test -p ooxml-xlsx --all-features --lib journal` は 96 件すべて通る。失敗する op が**新しいセルを
  `tracked` に入れた後で**失敗する回復テストが無いため（poison は `tracked.extend` の前、
  `restore_failure_recovers` と panic のテストの対象 `A2` は前の op ですでに追跡済み）。挙動は正しい。
- 修正案: 初めて触るセルへの no-op 書き戻し（oracle fixture の `num("C3", 7.0)`。定数 7 なので full restore）に
  `fail_restore_once = Some(false)` を当て、`Internal` の後に `same(j, twin(prefix))` を見るテストを足す。

### m2（Minor）: 受け入れの証拠が PR 本文に無い
- 根拠: 仕様の受け入れ 3（J0a の `append_cost` を 10k/100k 行で前後比較、10k で +5% 以内）、全幅
  `set_column_*` の 16,384 鍵のコスト、19 個の手変異の一覧（本文は「implementation notes below」と書くが
  その節が無い）がどれも載っていない。
- 修正案: 3 つを PR 本文に貼る（変異は名前と落ちたテスト）。

### m3（Minor）: `sst/@count` の差は `to_bytes` の履歴依存で、J1 の oracle 12 で表に出る
- 根拠: `tests/oracle1.rs:596-` のコメント自身が「最後の再走査（間に挟まった `to_bytes`）次第」と書く。
  `replay.rs` の `same()` がバイト一致で通るのは、どちらの側も途中で `to_bytes` を呼ばないから。
- 修正案: C2 の follow-up issue を立て（`restore_cell` の参照数か、`to_bytes` が registry を変えないこと）、
  コメントにその番号を書く。この PR では比較の緩和のままでよい。

### m4（Minor）: P2 の配線を延期した（T5b はマージ済み）
- 根拠: 仕様 §3.5 は「T5b がマージ済みなら配線する」。#163 はマージ済みで、この PR は `Unsupported` のまま。
  安全側の過剰拒否で、fixed 20 は拒否を確かめる。
- 修正案: follow-up PR（fixed 33、生成器のセッション index）を J1 の前提として追跡する。本文の記載で足りる。

### n1（nit）: `settle` の失敗時に報告する `seq` がずれうる
- 根拠: `mod.rs:235` は `self.next_seq` を渡す。`next_seq += 1` の後で panic した呼び出しでは、失敗した op ではなく
  その次の seq になる。
- 修正案: `mark.next_seq` を渡す。

### n2（nit）: `keys_of` は列範囲の id が軸に無いと黙って鍵を 0 個にする
- 根拠: `mod.rs:440`。`op_of` が先に範囲を検査するので今は起きないが、J2 の死んだ id で黙って鍵が消える。
- 修正案: `else` に `debug_assert!(false, ...)` を置くか、`Result` にする。

### n3（nit）: replay の「同じ op に戻る」確認が debug だけ
- 根拠: `replay.rs:172` の `debug_assert_eq!`（`Debug` 文字列比較）。release の replay では食い違いを検出しない。
- 修正案: 現状でよいが、J2（構造 op の id 再利用）で常時の検査にするか、oracle 12 で担保する旨を書く。

## 確認したこと（問題なし）
- 鍵: `Cols` は `first..=last` の各列に 1 鍵、`Row` はプロパティ別、`write_cells` は名指しセルごとに
  `Content` 1 鍵（重複セルは 1 回）、P1 は欠けたセルにも `Content`。fixed 28/32 の assert は変異
  （列の先頭だけ、プロパティ共有、鍵なし）を落とす形になっている（変異は実行していない。実行したのは m1 の 1 個だけ）。
- 拒否の等価: row/col は target を持たず、`op_of` は `apply_seq` の `Ok` の後でだけ index を読む。
  `MAX_ROWS`、`16_384`、`first > last`、NaN、幅 300、outline 8、`OutOfOrder` の Sheet2 はどれも両側で
  同じ種類で拒否され、log と鍵は空（fixed 29）。
- P1 の意味: ADR §4.4 の「条件付き op が飛ばすセルは target でない」に対し、欠けたセルを `None` の
  snapshot で追跡する実装は、セルが欠けたまま R2 で何もしないので観測上同じ（仕様 B7）。
  `Content` 鍵は §4.3 の whiteout どおり。fixed 24/25/27 で両側一致。
- recover の範囲: `Internal` を返す経路（`apply_seq` 後の poison、snapshot 失敗かつパート変化、`op_of`、
  R2 の `snapshot_cell`/`state_of`/`restore_cell`）はすべて `append` の `match` から `recover` に入る。
  log・`next_seq`・`keys`・`tracked`・shadow のバイト・index fingerprint・派生状態を双子と比べる
  `same()` は仕様どおり。
- C8 への置き換えは R2 の等価を緩めない方向にしか変えない: 共有/inline の別、run の `rPr`（正規形）、
  phonetic は比較に残る。緩むのは `f` の真偽値の綴り（`ca="1"` = `ca="true"`）と `t` の `xml:space` で、
  どちらも snapshot（同じ内容）への restore を許すだけ。
- 他の open PR（#144, pptx）とファイルの干渉なし。
- `cargo test -p ooxml-xlsx --all-features --lib journal`: 95 passed, 4 ignored（約 60 s, debug）。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 4 / nit 3）
