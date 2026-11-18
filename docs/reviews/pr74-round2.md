# PR #74 レビュー（Opus, round 2）

対象: test/fuzz-docx-replace @ bbed6de9e6b0e8491324485c27e4a44f0060cd79

## 結論（先に）

round 1 の M1、m1、m2、m3 はすべて直っている。オラクル（`docx_replace/oracle.rs`）は生の XML を走査して §1.4 の拒否を予測する。
境界・マーカー・書式・空白の判定はライブラリを呼ばずに再実装しており、find の `status` と replace の skipped・refused の両方をこの予測と突き合わせる。
round 1 で生き残った変異（A、B、C、E）は、すべて既定のテスト（`--lib` か iters=1 の smoke）で落ちる。
残るのは、`w:rPrChange` による拒否が既定のテストで検証されないという Minor 1 件である。
converged: yes（Blocker 0 / Major 0）。

## 前ラウンドの解消状況

- M1: 解消。`ParagraphXml::refusal`（`oracle.rs:176-209`）が、run の拒否状態、境界、マーカー、`MixedFormatting`、`UnpreservedWhitespace` を、`replace.rs` と同じ優先順で独立に予測する。
  `assert_status`（`docx_replace.rs:352`）で find の status を確かめ、`expected_refusals`（`docx_replace.rs:367-406`）で skipped・refused の理由と範囲を確かめる。
  どちらも完全一致を求めるので、受け入れ過ぎも過剰拒否も検出できる。`Core`（挿入の dry run）だけは予測外として扱うが、未編集の一致では出ないこと、ADR の拒否を隠さないことは確かめている。
- m1: 解消。`synthetic_substrings`（`docx_replace.rs:547-614`）が、合成文書の全部分文字列を 3 つの書式モードで走査する。
  `split` と `proofing` のプレースホルダーが既定オプションで 2 件とも置換されることも固定している。
- m2: 解消。`check_locality`（`oracle.rs:211-260`）が、run の開始タグと `w:rPr` をバイト単位で比べる。触っていない run は run 全体を比べ、触った run でも、一致に掛からない content child はバイト列で残ることを確かめる。
- m3: 解消。fuzz は入力から `Scope::{All, Story, Paragraph}` と `RunFormatting` の 3 値を選ぶ（`docx_replace.rs:149-161`）。範囲の期待値は scope で絞り、書式の予測もモードごとに切り替える（`FirstRun` では省き、`Identical` では proofing を無視しない）。

## 検証したこと

- オラクルの独立性: run の拒否状態だけは Phase0 の `Run::check_settable` を再利用している（`oracle.rs:93-107`。`replace.rs:407` と同じ関数）。
  この関数自体は `docx_edit` オラクルが予測と照合しているので、ここで二重に持たなくてよい。テキスト位置、境界、マーカー、書式の正規化、空白の判定は別の実装である。
- 実行時間（debug、CI の `cargo test --workspace` 相当）: `--lib docx_replace` は 1.1 秒、smoke の iters=1 は 3.9 秒。release ではそれぞれ 0.09 秒と 0.6 秒。
- `cargo clippy -p ooxml-robustness --all-targets --all-features -- -D warnings` は clean。
- 手書きの変異を `crates/ooxml-docx/src/replace.rs` に入れた（コミットしていない）。

| 変異 | `--lib docx_replace` | smoke iters=1 |
| --- | --- | --- |
| A: 境界の判定を無効化 | 検出 | 検出 |
| B: マーカーの判定を無効化 | 検出 | 生存 |
| C: 複数 run の一致を常に `MixedFormatting`（過剰拒否） | 検出 | 検出 |
| D: `same_formatting` が常に true | 検出 | 検出 |
| E: `UnpreservedWhitespace` を無効化 | 検出 | 検出 |
| F: `Identical` で proofing の差を無視する | 検出 | 生存 |
| G: `lastRenderedPageBreak` を常に「影響あり」として消す（局所性） | 生存 | 検出（`untouched run child changed`） |
| H: `w:rPrChange` による `InsideRevision` を無効化 | **生存** | **生存**（iters=20 で検出） |

- 他の open PR（#66, #72, #75, #76, #77）は `ooxml-robustness`、`fuzz/`、`replace.rs` に触れていない。干渉なし。

## Findings

### m1（Minor）: `w:rPrChange` による拒否が既定のテストで検証されない

- 根拠:
  - 変異 H（`replace.rs:408` の `has_rpr_change` を無効化）は、`--lib` と iters=1 の smoke を通る。iters=20 で初めて落ちる。
  - `w:rPrChange` を含むのは実フィクスチャの `poi-delins.docx` だけで、合成文書（`docx_replace.rs:479-504`）にはない。
  - オラクル側の予測（`oracle.rs:104-106`）は正しく書かれているが、既定の CI では通らない経路になっている。
- 修正案: `targeted_docs` に、`w:rPrChange` を持つ run と持たない run を隣り合わせた段落を足す（必要なら `w:ins` で囲んだ run も足す）。`synthetic_substrings` の全部分文字列走査が、そのまま `InsideRevision` の予測を検証する。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 0）
