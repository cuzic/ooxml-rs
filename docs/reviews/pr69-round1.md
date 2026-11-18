# PR #69 レビュー（Opus, round 1）

対象: `fix/docx-phase1-followups` @ `bca9911774be3fe547a83e8a8ceecd903fbf00ad`

## 結論（先に）
**converged: yes**（Blocker/Major なし）。#52 の m1/m2/n1–n4 と #54 の n1 は挙動で直っている。#54 の m1（`\` の付いた item）は、報告された 4 つの形では直った。ただし、file 引数（スキップする引数）と同じ文字列になる形では、まだ見落とす（m1、Minor、*(u)*）。CI（test/clippy/codegen、python-pptx）は 2 件とも pass。

## 前ラウンドの解消状況
- #52 m1（他の story の T2/T3/T5/T5b）: 解消。`tests/story_oracles.rs` は 37 run を編集する。計測用に一時的なコピーを入れて数えると、T5（docx-rs）で確かめるのは 28 run（Comment010 15、testComment 2、PageSpecificHeadFoot 10、Headers 1）。残りの 9 run（脚注・文末脚注など）は T5b だけで確かめる。docx-rs の読み込みに失敗したファイルは 0。
- #52 m2（範囲が空の `Match`）: 解消。`replace_matches` の鮮度チェックより前、まだ何も書かない段階で `EmptyNeedle` を返す。`replace_at` も同じ経路を通る。`3..2` のような逆向きの範囲も `is_empty()` で拒否される。`Err` のときの byte-identical はテストにある。
- #52 n1–n4: 解消（ADR note、doc、Q17/Q20/Q23 のテスト、`OOXML_T7_ONLY`）。T7 full mode の poi-bug59058 の結果は、まだ PR に投稿されていない（CI の範囲外なので指摘にはしない）。
- #54 m1: 一部だけ解消。下の m1 を参照。
- #54 n1: 解消（P2/P3 を殺す行がある）。
- 手で入れた変異: `unescaped` の `!skip.contains(source)` を外すと、`fields::tests::codes` が落ちる（殺せる）。

## Findings
### m1（Minor, *(u)*）: escape を外した候補が、スキップする引数との突き合わせで消える
- 根拠: `fields.rs` の `item_tokens` と `key`。次の 2 か所で候補が消える。
  1. `!out.contains(&t)` は、`out` にもうある変異を足さない。ところが、その `out` の token は後で file 引数としてスキップされることがある。
  2. `filter(|(source, _)| !skip.contains(source))` は、source がスキップする引数と同じ piece から出た変異を、**すべて**捨てる。token のほうは 1 回分しか除かない。

  プローブ（`field_key` を直接呼ぶ）の結果:
  - `INCLUDETEXT "OLE_LINK1" "\OLE_LINK1"` → `INCLUDETEXT (item \OLE_LINK1)`（`OLE_LINK1` が候補に無い）
  - `INCLUDETEXT OLE_LINK1 "\OLE_LINK1"` → 同上
  - `INCLUDETEXT "\OLE_LINK1" "\OLE_LINK1"` → 同上
  - `LINK Word.Document.12 "OLE_LINK1" "\OLE_LINK1"` → `LINK (item \OLE_LINK1)`

  この PR 自身が `INCLUDETEXT "OLE_LINK1" OLE_LINK1` を self reference としているので、上の形も同じ扱いになるべきである。どれも Word が書かない形なので、#54 m1 と同じく Minor とする。
- 修正案: 変異は、スキップを済ませた後の候補 token から作る。あるいは、source によるスキップを token と同じく 1 回だけにし、`out` との重複除去は、スキップの後に `cands` に対して行う。上の 4 行は adversarial の m13 表に足す。

### n1: `replace_matches` の doc の Errors に `EmptyNeedle` が無い
- 根拠: `replace.rs` の `replace_matches` の doc の Errors の列挙（`StaleMatch` … the gates）。`replace_at` の doc も確かめること。
- 修正案: `[`Error::EmptyNeedle`] for an empty range` を足す。

### n2: T5 は docx-rs が読めなかったファイルを黙って飛ばす
- 根拠: `story_oracles.rs` の `docx_rs::read_docx(&out).map(..).unwrap_or_default()` と、その後の `json.is_empty()` で `continue` する箇所。今の fixture では飛ばされたものは 0。ただ、docx-rs が読めなくなる回帰があっても T5 は通ってしまう。
- 修正案: 入力を docx-rs が読めるときは、出力も読めることを assert する。

### n3: `ITEM_ARGUMENT` の doc が escape を外した候補に触れていない
- 根拠: `fields.rs` L109–L116。「whichever token Word reads as the item」のままで、#54 m1 の変異（と、上の m1 の残り）が書かれていない。
- 修正案: 「`\` を含む piece は、`\` を外した形とそれぞれの `\` の後ろも候補にする」と 1 文足す。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 3）
