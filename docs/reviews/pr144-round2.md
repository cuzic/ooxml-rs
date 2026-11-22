# PR #144 レビュー（Opus, round 2）

対象: impl/pptx-phase4a1-add-comment @ 12d4f194eb1ce23468bc17d4edea18ae3444332b

## 結論（先に）
converged: yes。round 1 の M1、m1–m4、n1–n2 はすべて解消した。
M1、m2、m3（`delete_slide` の予約）、m4 は、round 1 の変異をもう一度入れて、追加したテストが落ちることを確かめた。
round 1 以降の差分はテスト、ADR の追記、lo-check の削除だけで、本体のコードは変わっていない。新しい指摘は無い。
#152 とは `oracle_cases.rs` と `oracle/check.py` が隣り合う追加行で衝突する。両方の行を残せば解消するので、指摘にはせず、マージの手順として下に書いた。

## 前ラウンドの解消状況
- **M1（解消）**: `local_author_reused_with_different_user_id` と `author_without_provider_is_never_reused` を追加した。後者は Local と Account の両方を確かめる。
  - 変異「Local の照合に `userId` を含める」は前者のテストを落とす。
  - 変異「`providerId` の無い作成者を Local として扱う」は後者のテストを落とす。
- **m1（解消）**:
  - ADR-0019 §9 の Reused 行と、post-acceptance の「P4a-1 implementation notes」に、fail-closed の拒否として記録した。
  - `cId="0"` を書くかどうかは、「Still open」にオーナーの判断待ちとして載った。
  - `invalid_slide_creation_ids_are_refused_atomically` が `powerpoint-mixed-deck-resaved.pptx` で、拒否の理由と、出力がバイト一致することを確かめている。
- **m2（解消）**: `new_part_name_uses_uppercase_hex_for_both_ids` を追加した。期待する名前は `modernComment_1AF_ABC.xml`。小文字の hex の変異はこのテストを落とす。
- **m3（解消）**: 拒否のテストを追加した。
  - 置き場所の外の `p14:creationId`、`CreationIdsExhausted`、関係の無い `p:ext{6950BFC3}`
  - 共有されたパート、モダンの関係 2 本、名前の衝突（`NameInUse`）、接頭辞の衝突
  - authors の AC、解析できない場合、関係 2 本
  - `InsideAlternateContent`、`AmbiguousShapeId`、`contentPart`
  - 10000 年、`SlideIndexOutOfRange`

  `delete_slide_skips_unreadable_comment_parts_and_reserves_removed_ids` は、壊れたパートがあっても `Ok` を返すことと、削除した GUID が予約されることを確かめる。`slides.rs` の予約を外す変異はこのテストを落とす。
- **m4（解消）**: 新しいスライドの XML に、バイト列で `<p14:creationId xmlns:p14="…" val="` があり、`xmlns:p14` がちょうど 1 回であることを確かめる。`sample.pptx` のルートは `p14` を宣言していないので、ルートで宣言する変異は検出される。
- **n1（解消）**: `tools/lo-check/check.py` から `odp_annotations` を削除した。ADR §11.5 と Delivery も、提案と fallback の記述に直した。
- **n2（解消）**: P4a-1 の注に `xmlns:r1` / `r1:id` を記録した。

## 確かめたこと
- `cargo test -p ooxml-pptx --lib` は 33 件すべて通る。`cargo clippy -p ooxml-pptx --all-targets` の警告は 0 件。
- CI は 4 ジョブが緑で、test/clippy/codegen のジョブは実行中だった。
- origin/main（`be68176`）とは衝突しない。
- **#152 との衝突**（`impl/pptx-ac-lifts-u4`、`git merge-tree` で確認）: どちらも、同じ位置に 1 行を足すだけの衝突。ADR-0019 と `slides.rs` は自動でマージされる。
  - `examples/oracle_cases.rs`: #144 の `comments_case` のループと、#152 の `alternate_table_style_case(&out)?;`
  - `tests/oracle/check.py`: `comments.main(...)` と `check_alternate_table_style(outdir)`

  解き方は、両方の呼び出しを残すこと。#152 が先にマージされたら、#144 は origin/main をマージして（rebase はしない）、python-pptx のジョブで両方のケースが走ることを確かめる。
- round 1 以降の差分は `comments.rs` のテストだけ（+369 行、`#[cfg(test)]` の中）、ADR-0019 の追記（+23/−3）、lo-check の削除（−27）。本体、生成物、公開 API は変わっていない。

## Findings
なし。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 0）
