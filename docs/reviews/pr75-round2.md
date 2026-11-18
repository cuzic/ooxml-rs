# PR #75 レビュー（Opus, round 2）

対象: impl/core-perf-stage1 @ 350f51c16c7fdfc4940202e9b865f7485ac34672

## 結論（先に）
round 1 の M1・m1〜m3・n1 はすべて直っている。span は u32 4 個（16 B）で、dirty フラグは `open_end`/`close_end` の bit 31、
`RawNode` は 64 B（assert ≤ 64）。入力上限 2 GiB − 1 では全オフセットが ≤ 0x7FFF_FFFF なので bit 31 と衝突せず、
`expand` が両方の語をマスクするのでフラグはオフセットに漏れない。round 1 以降の変更（350f51c のみ）に回帰は無い。
converged: yes。

## 前ラウンドの解消状況
- M1: 解消。`node.rs:6-19` が u32 4 個、`node.rs:34` が `size_of::<RawNode>() <= 64`（プローブで 64）、`parse.rs:105` の上限が
  `MAX_SPAN_OFFSET`（2^31 − 1）、ADR-0010 の post-acceptance note（L813〜）に上限と配置を記録、`parse.rs:408` で境界（2^31 − 1 は Ok、
  2^31 は `XmlPartTooLarge`）を検査。
- m1: 解消。`check_span_limit` は UTF-16 BOM → UTF-8 → 2 個目の BOM → NUL の後（`parse.rs:93`）。`source.len() == bytes.len()` なので
  判定値も同じ。
- m2: 解消。PR 本文「性能」に `b64559f` の一時 worktree へ `breakdown.rs` だけをコピーして同一 CPU で測った手順がある。
- m3: 解消。`fuzz_regressions.rs:85-93` に `<!DOCTYPE r><r/>` と `<r/><!DOCTYPE r>` を追加し、frozen と照合している。
- n1: 解消（`mod.rs:33`、`parse.rs:5-13`）。

## 確認したこと
- ビット詰めの境界: 一時プローブで `start = 2^31−11, open_end = 2^31−6, close = (2^31−5, 2^31−1)` の要素に own/subtree dirty を立て、
  `span()` が立てる前と同一、フラグは両方 true を確認。自己終了要素に `mark_subtree_dirty` しても `close_end` は 0 にマスクされ `close: None`
  のまま、`own_dirty` は false のまま。作成ノード（`open_end = close_end = DIRTY_BIT`）は `span() == None`。
- `close()`（`node.rs:62`）は `close_end` を丸ごと上書きするが、呼び出しは解析中の `parse.rs:201` だけで、その時点ではフラグは立っていない。
- 変異: `expand` の `open_end` のマスクを外すと ooxml-core のテスト 1 件、`close_end` のマスクを外すと 2 件が落ちる。
  `check_span_limit` の `>` を `>=` にすると `parse.rs:408` が落ちる（limit が Ok であることを検査しているため）。
- `RawText` の WRITTEN 番兵 `[1, 0]` と `SStr` の u32 オフセットは上限 2^31 − 1 の下で変わらず正しい。
- 手元で `cargo test --workspace --all-features`（887 件通過）・clippy `-D warnings`・`codegen --check` が通過。CI（Rust 1.99、python-pptx T3）も緑。
- 不変条件: 編集 API・拒否の追加は無く Err ⇒ byte-identical に影響なし。T1 往復は既存テストと frozen 照合で通る。生成物と公開名に変更なし
  （`codegen --check` 通過）。公開 API の差は round 1 時点と同じ（`XmlPartTooLarge` の文言のみ変更）。
- 他の open PR（#23, #72, #74, #76, #77）は `ooxml-core/src` と ADR-0010 に触れていない。

## Findings

### n1（nit）: 境界テストが上限を数値で直書きしている
- 根拠: `crates/ooxml-core/src/xml/parse.rs:409` の `let limit = 2_147_483_647_usize;`。`MAX_SPAN_OFFSET` と別に値を書いているので、
  定数を変えたときにテストが追随しない（今は一致しており挙動は正しい。意図的に独立させたならそのままでよい）。
- 修正案: 意図を残すなら `assert_eq!(limit, MAX_SPAN_OFFSET as usize)` を 1 行足す。上の「上限近くのオフセットで dirty を立てても
  `span()` が変わらない」プローブを `node.rs` の単体テストとして入れるとビット詰めの退行をより直接に検出できる。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 1）
