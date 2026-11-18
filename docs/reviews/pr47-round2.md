# PR #47 レビュー（Opus, round 2）

対象: `test/fuzz-docx` @7d8b5e4（#36 @edc9bb1 に積む。round 1 からの差分は 968bd3d〈#36 のマージ〉と 7d8b5e4）。
検証: 独立した `git clone` 2 つ（head、head＋現在の origin/main 2495c82 のマージ）。Rust 1.99.1、nightly 1.102.0（5f4effd57）、cargo-fuzz 0.13.2。

## 結論（先に）

**M1 は解消した。** 編集結果を公開 API から予測し、実際の結果と突き合わせる oracle が入った。
round 1 の手入れ変異 11 個は、**すべて**スモーク（`OOXML_SMOKE_ITERS=30`）で検出される。m1–m3 も正しく入っている。

残りの所見は Minor 以下である。

- 予測は分類の誤りを検出できない。予測が `write_gate` と `check_settable` をそのまま使うので、分類そのものの誤りは予測側も同じように間違える。
  新しく入れた変異 3 個（M12–M14）は生き残った。そのうちフィールド判定（M12）は、append の予測と同じ数え直しで安く検出できる（試作で確認済み）。
- **timeout は直りきっていない。** `poi-bug59058.docx` では、編集 0 回でも ASan で約 6 秒かかる。nightly では 6 worker 中 5 つが初期コーパスの段階で timeout した。
  計測したマシンは高負荷（load avg 約 55 / 32 コア）で、数値は割り引いて読む必要がある。
- 文書に古い記述が残っている（m4 の残り）。

この PR 固有の Blocker・Major はない。ただし、#36（土台）が M2 で未収束であることに注意。

- Minor 3 件（m5–m7）、Nit 2 件
- 判定: **converged: yes**（この PR の差分について。m5 の timeout は CI の docx_edit ジョブが落ちる原因になりうるので、マージ前の対応を強く推奨）

## 解消状況

| ID | 状態 | 確認内容 |
|---|---|---|
| M1 | 済 | `predict_set`（ゲート → 段落・ラン → `check_settable` → 文字）と `predict_append`（ゲート → 文字 → `w:body` → `w:fldChar` の数え直し → A2）の予測を、実際の結果と完全一致で比較する。過剰な拒否も検出できる。`targeted_inputs()` の 7 本で、A2・開いたフィールド・`xml:space` の経路も毎回通る |
| m1 | 済 | ランの属性と `w:rPr` の構造一致、端・連続空白の `w:t` の `preserve` を確認している（変異 5・7・11 で検出） |
| m2 | 済 | 挿入点が `w:body` の中にあり、本文直下の `w:sectPr` より前にあることを確認している（共通の接頭辞・接尾辞から取れる区間との交差で判定。正しい） |
| m3 | 済 | テキストの半分を `Input::printable`（端・連続空白、タブ、改行、`&<>"'`、非 BMP 文字を含む）から作る |
| m4 | ほぼ済 | #36（main 入り）をマージして数値を更新した。古い記述は m7 に回す |
| timeout | 一部 | parse を 1 回にまとめ、512 KiB 超では 2 ステップに減らした。まだ足りない（m5） |

### 変異の再実行（スモーク `OOXML_SMOKE_ITERS=30`、release）

| # | 変異 | 結果 |
|---|---|---|
| 1 | `set_run_text` の `write_gate` を削除 | 検出（予測不一致） |
| 2 | `set_run_text` の `check_settable` を削除 | 検出（予測不一致） |
| 3 | `append_paragraph` の `write_gate` を削除 | 検出（予測不一致） |
| 4 | A2（`ends_with_section_break`）を無効化 | 検出（予測不一致） |
| 5 | `set_t` が `preserve` を付けない | 検出（`lacks xml:space="preserve"`） |
| 6 | Tab で部分変更のまま `Err` | 検出 |
| 7 | `w:rPr` も消す | 検出（`w:rPr` 不一致） |
| 8 | append が誤った index を返す | 検出 |
| 9 | `Br` を `w:tab` として書く | 検出（読み戻し） |
| 10 | append のフィールド内検査を無効化 | 検出（予測不一致） |
| 11 | no-op 判定が `lacks_needed_preserve` を見ない | 検出（`preserve`） |
| 12（新） | `body.rs` の `in_field` を `has_fld_char` だけにする（begin と end の間のランが設定可能になる） | **生存** |
| 13（新） | `check_settable` が変更履歴（revision）の wrapper を見ない | **生存** |
| 14（新） | `write_gate` が常に `Ok`（ゲートの判定漏れ） | **生存** |

### 実行結果

| 項目 | 結果 |
|---|---|
| head＋origin/main 2495c82 | 衝突なし。`cargo test -p ooxml-robustness`（release）全件 pass、スモーク docx 30 iters pass、clippy `-D warnings` 警告なし、`fuzz/Cargo.lock --locked` 通過 |
| nightly `docx_edit`（seed 394 件、6 並列×360 秒、ASan、`-timeout=10`） | アサーション失敗なし。**6 worker 中 5 つが timeout で停止**（m5）。止まらなかった 1 つは 888 回実行 |
| timeout の入力 4 件 | すべて fixture 8（`poi-bug59058.docx`、document.xml 1.35 MB）で、置き換えたのは main 以外の part。ASan 単独で 12.4–17.6 秒、release の `replay` では 0.36–0.39 秒 |

## 所見

### m5 [Minor] `poi-bug59058.docx` は編集 0 回でも ASan の `-timeout=10` に近く、2 ステップの制限では足りない

**場所**: `crates/ooxml-robustness/src/docx.rs` 177–200 行（`max_steps`）、`.github/workflows/fuzz-smoke.yml` の `-timeout=10`

計測値（ASan、同じマシン、load avg 約 55 / 32 コア）:

- ops 0 バイトの入力（編集なし）: 5.9 秒
- seed `f008-all`: 12.5 秒

固定費の内訳は、open、`to_bytes`、L1 比較、2 回目の `to_bytes` と L1 比較、`read_all` である。2 回目の保存と比較は、読み取りが何も変えていないことの確認に使っている。
その上に、1 ステップごとに保存・再オープン・`read_all` が加わる。

負荷を割り引いても、CI の runner（4 vCPU）でこの fixture の seed が 10 秒を超える可能性は高い。thin した 64 件の seed にも、fixture 8 の part 置換の変種は入りうる。
libFuzzer は最初の timeout で止まるので、週次の docx_edit ジョブが赤になる。

**修正案**（どれか 1 つ）:

- (a) main part が 512 KiB を超えるときは編集を 0〜1 回にする。あわせて「読み取り後の 2 回目の保存と L1 比較」を飛ばす。
- (b) docx_edit の fuzz 用 fixture から 1 MB 超の文書を外し、スモーク（release）でだけ使う。
- (c) ワークフローで docx_edit だけ `-timeout=30` にする。

文書の「2 回目の実行では timeout は出ていない」は、負荷の低い環境でのたまたまの結果と見るべきである。

### m6 [Minor] 予測が分類の公開 API をそのまま使うので、分類の誤りは見えない（フィールドは安く独立に検査できる）

**場所**: `docx.rs` 87–94 行（`predict_set`）、52–68 行（`read_all`）

M1 の oracle で守れるようになったのは、「分類どおりに拒否・許可しているか」である。分類そのもの（`in_field`、wrapper、ゲートの検出）の誤りは、予測も同じように間違えるので検出できない。変異 12–14 が生き残ったのはこのためである。
このうちフィールドは、`predict_append` がすでに `w:fldChar` を数え直している。同じ数え直しを `read_all` で各ランについて行えば、変異 12 は検出できる。
具体的には、各ランで `in_field() == (直前の深さ > 0 || 直後の深さ > 0 || w:fldChar を持つ)` を assert する（試作は `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-36r3-47r2/docx-infield-recount.diff`、約 15 行）。
試作の結果、変異 12 を検出し、スモーク 30 iters で誤検出もなかった。

変更履歴（13）とゲート（14）を独立に判定するには、祖先の走査や settings.xml の解釈が要る。これは任意とし、少なくとも文書に「分類そのものの正しさは単体テストの範囲で、ファザーは分類と結果の一貫性を見る」と書いておくこと。

### m7 [Minor] 文書の古い記述（m4 の残り）

**場所**: `docs/testing/fuzz-findings-2026-11.md`

- 冒頭の「xlsx Phase1（数式の経路、`impl/xlsx-phase1`）はまだ main にないので対象外」: Phase1（#20）は main に入っていて、#36 のハーネスも Phase1 を前提にしている。
- ooxml-docx の節の「対象: main @8c54366」: マージ後の main に更新する。
- 末尾の段落の「main part が 512 KiB を超える場合は編集回数を 4 回にした」と「12 回で ASan 10 秒超」は、すぐ上の「2 回まで」と矛盾する。どちらか一方にまとめる（m5 の対応後の値に）。

### Nit

1. lens の拒否の例外（`predicted.is_ok() && lens_refusal && kind == 3`）は、本文に `mc:AlternateContent` の子があることを条件にしていない。
   parse 済みの `parsed` で、body 直下に `mc:AlternateContent` がある場合に限ると、`UnresolvedSibling` の誤った拒否も検出できる。
2. `Input::printable` に含めた `'\u{AD}'`・`'\u{2011}'` は意図どおり（書ける文字）。ただ、コメントの「every writer must accept」は xlsx・pptx でも使う前提の書き方なので、docx 専用でないなら「Phase0 の docx/xlsx/pptx の writer」と範囲を書くとよい。

## 判定

**converged: yes** — この PR 固有の Blocker・Major はない。
m5（timeout）は、CI の docx_edit ジョブが週次で落ちる原因になりうるので、マージ前の対応を推奨する。
m6 のフィールドの数え直しは 15 行なので、同時に入れることを推奨する。
#36 が M2 で未収束なので、マージは #36 の収束を待つこと。
