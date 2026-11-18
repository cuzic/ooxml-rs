# PR #47 レビュー（Opus, round 3）

対象: `test/fuzz-docx` @b82eacf（#36 @bf5c2ab に積む。round 2 からの差分は 726a873〈#36 のマージ〉と b82eacf）。
検証: 独立した `git clone`（head）。Rust 1.99.1、nightly 1.102.0（5f4effd57）、cargo-fuzz 0.13.2。

## 結論（先に）

**m5・m6・m7 と Nit 2 件は、すべて正しく直っている。**

- m5（timeout）: round 2 で timeout した入力 7 件（すべて `poi-bug59058.docx`）は、ASan で 1 件あたり 3.0〜4.9 秒になった（load avg 約 21〜32 / 32 コア）。
  nightly で seed 全量の 394 件を 6 並列×300 秒回し、**6 worker とも timeout なしで完走**した（計 1.8 万回、cov 6,187）。
- m6: `read_all` での `w:fldChar` の数え直しが入った。round 2 で生き残った変異 12（`in_field` が開いた深さを見ない）は検出されるようになった。
  新しい変異 15（`separate` を `end` として数える）も検出された。
- m7: 文書の古い記述（Phase1 が対象外、main @8c54366、4 回と 2 回の矛盾）はすべて解消した。

変異 1〜11 は引き続きすべて検出される。13（変更履歴の wrapper）と 14（ゲート）は、合意どおり単体テストの範囲として文書化された。

- 新しい所見は Nit 1 件だけ
- 判定: **converged: yes**

## 解消状況

| ID | 状態 | 確認内容 |
|---|---|---|
| m5 | 済 | main part が 512 KiB を超える文書は、編集 1 回で、読み取り後の 2 回目の保存と L1 比較も飛ばす（`large`）。計測は下表 |
| m6 | 済 | 試作どおりに入っている。各ランで `in_field() == (直前の深さ > 0 \|\| 直後の深さ > 0 \|\| w:fldChar を持つ)` を assert する。文書に「分類そのもの（変更履歴の wrapper、ゲート）は単体テストの範囲、ファザーは分類と結果の一貫性を見る」と書いてある |
| m7 | 済 | 冒頭は「Phase1（#20）は main に入っており、`phase1_oracle` で対応」に更新された。docx の節は「main @8c54366 で実行し、main @4f8e294 にマージして再確認」とあり、4f8e294 は origin/main 上の #49 である。大きな文書の制限の記述は「対策」の項 1 か所にまとまっている |
| Nit 1（lens の拒否の例外） | 済 | `body_has_ac`（`w:body` 直下に `mc:AlternateContent` がある）を条件に加えた |
| Nit 2（`printable` の doc） | 済 | 「Phase0 の docx/xlsx/pptx の text writer」に範囲を限定した |

### m5 の計測（ASan ビルドの `docx_edit`、`-runs=1 -timeout=10`、単独実行、load avg 約 21〜32 / 32 コア）

| 入力 | round 2（7d8b5e4） | round 3（b82eacf） |
|---|---|---|
| round 2 の nightly で出た timeout・slow-unit 7 件（fixture 8） | 12.4〜17.6 秒 | **3.0〜4.9 秒** |
| fixture 8、ops 0 バイト（`zero8`） | 5.9 秒 | 2.1 秒 |
| fixture 9・21 の append / set、`big-append` | — | 0.2〜0.5 秒 |

nightly `docx_edit`: seed 394 件（全量。CI の thin 後の 57 件より厳しい）、6 並列×300 秒、`-timeout=10`、`-rss_limit_mb=4096`。
同時に `xlsx_edit` を 4 並列で回していた。6 worker とも `DONE` まで到達し（各 2,604〜3,715 回）、timeout・crash・OOM はなかった。
CI の runner（4 vCPU）でも、10 秒に対して 2 倍以上の余裕がある。

### 変異の再実行（スモーク `OOXML_SMOKE_ITERS=30`、release）

| # | 変異 | round 2 | round 3 |
|---|---|---|---|
| 1–11 | （round 1・2 と同じ） | すべて検出 | すべて検出 |
| 12 | `body.rs` の `in_field` を `has_fld_char` だけにする | 生存 | **検出**（`run (0, 1): in_field disagrees with the fldChar recount`） |
| 13 | `check_settable` が変更履歴の wrapper を見ない | 生存 | 生存（単体テストの範囲として文書化済み） |
| 14 | `write_gate` が常に `Ok` | 生存 | 生存（同上） |
| 15（新） | `body.rs` の深さ計算で `separate` も `end` として数える | — | **検出**（`run (0, 3): in_field disagrees …`） |

## 所見

### Nit

1. **数え直しが独立しているのは「深さの規則」だけで、「どのランを数えるか」はライブラリの walk に従う**。
   数え直しは `doc.paragraphs()` / `p.runs()` が返すランを回る。このため、walk が `w:fldChar` を持つランを丸ごと飛ばす誤りは、両方から同じように消えて見えない。
   例としては、`w:ruby` や `w:sdtContent` の中に降りない誤りがある。
   実害は小さいので、`fuzz-findings` の m6 の段落に「ランの列挙はライブラリに従う」と一言足す程度でよい。
   なお `fields.rs` は `descend` を共有して walk との一致を保証しているので、ここを独立にする必要は薄い。

## 判定

**converged: yes** — round 2 の Minor 3 件と Nit 2 件はすべて解消した。m5 は ASan の計測と nightly の完走で確認した。
この PR 固有の Blocker・Major・Minor はない。
マージは土台の #36 の収束（round 4 で converged、m12 の対応を推奨）の後に行うこと。
