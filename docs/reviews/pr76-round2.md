# PR #76 レビュー（Opus, round 2）

対象: impl/pptx-phase1-notes @ 96f8cf3913a39b423269b7ba3cb464890c506218

## 結論（先に）

round 1 の M1・M2・m1・m2 は、どれも挙動で解消を確かめた。修正を戻す変異を手で入れると、追加された試験がそれぞれ落ちる。
round 1 以降のコミット（`96f8cf3` のみ）に、拒否時のバイト同一性・局所性・fail closed・生成型の公開名についての後退は見つからなかった。
**converged: yes**。

## 前ラウンドの解消状況

- **M1: 解消.** `presentation.rs:267–275` の `scope_units` が、ノートの単位をパート名（`BTreeSet`）で重複除去する。最初に参照したスライドの単位だけが残る。
  - find と replace は同じ `scope_units` を通るので、両方に効く。
  - `tests/notes.rs` の `shared_notes_are_replaced_once` は、round 1 のプローブ 2 つ（run をまたぐ `foo`→`X`、`-`→`\v`）を試す。`replaced` が 1 件で、`location.slide == 0`、保存後の本文が `X!tail` / `a\vb` であることを確かめている。
  - `presentation.rs:561` の期待値は 1 件・slide 0 に直った。fault 試験は単位 0 の後に失敗させ、バイト同一性を確かめている。
  - 変異: 重複除去を無効にすると、lib の試験が落ちる。
- **M2: 解消.** `deck.rs:198` で、ノートとして採るのはルートが `p:notes` のパートだけになった。違えば `notes()` / `notes_sheet()` は `None`。
  - `deck_ignores_notes_relationship_to_slide` が round 1 のプローブ入力でこれを確かめ、`to_bytes` が入力と同一であることも確かめている。
  - 変異: 判定を `true` に戻すと、この試験が落ちる。
- **m1: 解消.** ADR-0008 の post-acceptance note に、4 つの enum の `#[non_exhaustive]` と追加 variant が記録された。生成物（`ooxml-schema`）には触れていない。
- **m2: 解消.** `notes.rs:73–78` で、最初のトップレベルの body プレースホルダーに `p:txBody` が無ければ `NoNotesBody` を返す。python-pptx の `notes_placeholder` と同じ扱い。
  - `mc:AlternateContent` 内の body が先にあっても、トップレベルの body があればそちらで決まる。ADR の note と一致する。
  - `NotesRef::body()`、`DeckSlide::notes().body()`、`body_mut`、`Scope::Notes` の find/replace が、同じ `body_loc` で一貫して「本文なし」になる。拒否はバイト同一（`first_body_placeholder_without_text_body_wins`）。
  - 変異: `continue` に戻すと、この試験が落ちる。
- **nit 3 件（`Deck::link` の Theme 固定、`TextStyleKind::Notes` の到達しない腕、`FAIL_AFTER_SLIDE` の doc）: 未対応。** 挙動には影響しないので follow-up でよい。

## Findings

新しい指摘はない。

- 手元で `cargo test -p ooxml-pptx --all-features` が全て通った。CI（Rust 1.99 の test・clippy・codegen --check、python-pptx T3 read-back）も緑。
- 他の open PR（#77, #75, #74, #72, #66, #23）は `ooxml-pptx` と ADR-0008 に触れていない。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 0）
