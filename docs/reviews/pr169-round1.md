# PR #169 レビュー（Opus, round 1）

対象: fix/xlsx-ej-b1-dangling-v @ 09c44f5fede1175ccd471bc89e5166819981d8ed

## 結論（先に）
修正は B1 を塞ぐのに十分で、テスト 2 件はどちらも、修正を外す変異で落ちる。state_of は `t="s"` の `v` を、共有文字列の表に実在するアイテムに解決できたときだけ `Content::Str { inline: false }` にする。dangling な index、表が無い場合、C8 で読めないアイテムは `Content::Value`（raw）になる。そのため、今回の追加条件は「綴りが一致し、かつ実在するアイテムである」ことを要求し、fresh reservation（= 現在のアイテム数）と同じ綴りは必ず apply 側に倒れる。#170 とは衝突しない。converged: yes。

## 前ラウンドの解消状況
初回レビューのため該当なし。

## Findings
### n1（nit）: C8 で読めないアイテム（sst が mc:AlternateContent の中にある場合など）は、同じ index への書き込みでも skip されず apply になる
- 根拠: journal/state.rs の state_of は、`string_state_of` か `shared_text` が失敗すると raw の `Value` を返す（コメント「a dangling index, or `sst` behind `mc:AlternateContent`」）。新しい条件ではこのセルを state-same にしない。同じ index を書き直すだけなので出力は変わらず、安全側の過剰 apply になる（ADR-0024 R1 の「skip は確実に同一のときだけ」と整合）。ただし、コメントは dangling の場合しか説明していない。
- 修正案: edit.rs のコメントに「読めないアイテムも apply（安全側）」と 1 行足す。follow-up でよい。

## 検証
- CI: `gh pr checks 169` で全 5 ジョブ pass。
- `cargo test -p ooxml-xlsx --lib -- report_shared fixed_`: 47 passed。
- 変異 1: `&& matches!(&state.content, Content::Str { inline: false, .. })` を外すと、edit::tests::report_shared_with_a_dangling_v_applies と journal::tests::fixed::fixed_45_b1_dangling_v_spelling_a_fresh_reservation_is_applied の 2 件が落ちる。
- 変異 2: `inline: false` → `inline: true` にすると、report_shared_compares_the_index と fixed_39_spec38_identity_writes_skip_and_push_keys の 2 件が落ちる（identity skip も守られている）。
- 変異はすべて元に戻し、tree は clean。
- 見逃しの検討:
  - 解決済みの Str で、かつ plan が fresh reservation になる組み合わせは起きない。fresh index はアイテム数に等しく、実在するアイテムの index は必ずそれより小さい。
  - 同じ batch の先行書き込みが append したアイテムを指す `v` は、確定済みの sst から解決されるので Value になり、apply になる（正しい）。
  - 前後の空白付きの `v`（例 ` 1`）は、綴りの literal 比較で不一致になり apply になる（C5 と同じ）。
- 干渉: #170（impl/xlsx-js-compat @ c7d93be）とは `git merge-tree` で衝突なし。合わせた tree で `cargo test -p ooxml-xlsx --lib` が 336 passed（#170 round 2 で確認済み）。#170 は content_style_same の Formula 腕と新しい op の腕を変えるが、Shared 腕には触れないので、マージ順序の制約は無い。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 1）
