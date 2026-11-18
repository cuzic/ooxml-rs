# PR #36 レビュー（Opus, round 2）

対象: `test/fuzz-core` @66e1e07（round 1 からの差分 973fdf1・66e1e07）。
検証: 独立した `git clone` 2 つ（PR head と、origin/main 48aa8f0 をマージしたもの）で実行。Rust 1.99.1、nightly 1.102.0（5f4effd57 2026-11-13）、cargo-fuzz 0.13.2。

## 結論（先に）

**round 1 の M1・m1–m9 はすべて対応済みで、内容も正しい。** ただし新しく足した ZIP64 の子プロセス テストに、
**`#[ignore]` を外した瞬間に何も検査せず pass する**欠陥がある（M1'）。PR #40 と #14 が main に入ったので、
この PR では今まさに ignore を外す段階であり、外すとそのまま空振りの回帰テストになる。これを Major とする。

あわせて、**main をマージすると `ooxml-robustness` がコンパイルできない**（#19 の API 変更、1 行の修正）。
main マージ後は再現テスト 9 件すべてが pass し、`OOXML_FUZZ_ALL=1`（既知フィルタ無効）でもスモークが通るので、
この PR で main をマージして 9 件の `#[ignore]` を外し、`known.rs` のフィルタを消すべき。

- Major 1 件（M1'）、Minor 2 件（m10, m11）、Nit 3 件
- 判定: **converged: no**（M1' と main マージ＋un-ignore で次ラウンドは収束見込み）

## round 1 指摘の確認

| ID | 状態 | 確認内容 |
|---|---|---|
| M1（F-6 High） | 済 | 文書の結論・表・影響を訂正、修正方針も #40 の実装と一致。`f6_zip64_declared_size_does_not_abort` は PR head で子プロセスが SIGABRT → 親が失敗、main マージ後は pass（ただし下の M1'） |
| m1 timeout / max_total_time | 済 | `timeout-minutes` 20/30、seed を 64 件に間引き、README と入力 description に「初期コーパス一巡の後から数える」と明記。`thin` は名前順で n 件おきなので合成変種も満遍なく残る |
| m2 nightly / cargo-fuzz / lock | 済（Nit 1） | `nightly-2026-11-13`、`cargo-fuzz 0.13.2 --locked`、`fuzz/Cargo.lock` コミット。`cargo metadata --locked` は head でも main マージ後でも通る。zip 9.6.0 / quick-xml 0.42.0 がルートの lock と一致 |
| m3 permissions / 入力 | 済 | `permissions: contents: read`、dispatch 入力は `env:` 経由で数値検査、`matrix.target` は静的 |
| m4 F-7 と #14 | 済 | 文書とテストの ignore 理由に記載 |
| m5 F-7 の揺れ | 済 | 各 n で 3 回の最小値 |
| m6 対照 | 済 | F-1 xlsx・F-2・F-6（32 ビット / ZIP64）に「パッチ前は成功」の対照。ZIP64 は申告サイズを正しい値にした同じ zip を対照にしていてよい |
| m7 フィルタ | 済（Nit 2） | F-4 はルート開始タグ以降に限定、F-1 は `[Content_Types].xml` / `_rels/.rels` 以外は L1 検査を続ける |
| m8 seed | 済 | ZIP64・data descriptor・前置データ・壊れた CFB・part だけ Strict の 5 変種。生成物を確認: ZIP64 変種は CD の size が `0xFFFFFFFF` で zip64 extra あり、316 件生成 |
| m9 文書・oracle | 済 | F-2 の説明、`assert_l1_except` は raw で読めない入力を失敗に、pptx は全 text body の保存前後一致を検査 |

### ワークフローの安全性

トリガーは `workflow_dispatch` と `schedule` のみで、`pull_request_target` / `pull_request` はない（fork の PR コードがトークンや
キャッシュに触れる経路がない）。secrets 未使用、トークンは read のみ、`${{ }}` の `run:` 直接展開は `matrix.target` も含めて消えた。
アクションのタグ固定（`dtolnay/rust-toolchain@master` はブランチ）は `ci.yml` と同じ流儀なので、この PR の範囲外でよい。問題なし。

## 実行結果

| 項目 | 結果 |
|---|---|
| PR head: `cargo test -p ooxml-robustness --test fuzz_findings -- --ignored` | 9/9 失敗（主張どおり。ZIP64 は子プロセスの abort を親が報告） |
| main（#40・#14 入り）をマージ | **`src/pptx.rs:163` で E0308**（`x.path() == &path`、#19 で `path()` が値を返す型に変わった）。`&` を外すと build 可 |
| マージ後: 同じコマンド | **9/9 pass**（0.38 s） |
| マージ後: `OOXML_FUZZ_ALL=1 OOXML_SMOKE_ITERS=20` スモーク（release） | 5/5 pass、6 s。既知フィルタはもう不要 |

## 所見

### M1' [Major] ZIP64 テストは ignore を外すと空振りで pass する

**場所**: `crates/ooxml-robustness/tests/fuzz_findings.rs` 288–291 行

```rust
.args(["--exact", NAME, "--ignored", "--nocapture", "--test-threads=1"])
```

子プロセスに `--ignored` を渡しているので、テストに `#[ignore]` が付いている間だけ子が本体を実行する。
文書の手順どおり `#[ignore]` を外すと、子は「ignored のテストだけ」を選ぶので **0 件実行・終了コード 0** になり、親は常に pass する。
scratch で PR head（F-6 未修正）のまま ignore だけ外して実行した結果:

```text
running 1 test
.
test result: ok. 1 passed; ... finished in 0.00s     # abort するはずの未修正コードで pass
```

F-6 は High の所見で、このテストはその回帰ガードそのものなので Major とする。

**修正案**: `--ignored` を `--include-ignored` にし、さらに子の stdout に `1 passed` があることを assert する
（フィルタやテスト名の変更で 0 件実行になっても気づけるように）。

### m10 [Minor] main をマージするとコンパイルできない／9 件の un-ignore をこの PR で

**場所**: `crates/ooxml-robustness/src/pptx.rs` 163 行、`tests/fuzz_findings.rs` の全 `#[ignore]`、`src/known.rs`

テキスト上の衝突はないが、main（#19）の `ShapePath` API 変更で build が壊れる（`x.path() == path` に直す）。
#40 と #14 が main に入り、マージ後は 9 件すべて pass、既知フィルタ無効のスモークも通る。文書自身が「修正したら `#[ignore]` を外し、
フィルタを消す」と書いているので、この PR で origin/main をマージ（rebase ではなく merge）し、
9 件の ignore を外し（M1' の修正と同時に）、`known.rs` のフィルタを削除（または全関数を `false` に）し、文書の表に「修正済み（#40 / #14）」を加える。
stacked の #47 も同じ修正が要る。

### m11 [Minor] `fuzz/Cargo.lock` をコミットしても、ワークフローはそれを強制しない

**場所**: `.github/workflows/fuzz-smoke.yml` 83–89 行

`cargo fuzz run` は `--locked` 相当を付けないので、ルートの依存が変わって `fuzz/Cargo.lock` が古くなると、CI は黙って lock を更新して進む
（m2 の意図「週次の失敗をツールチェーン / 依存の更新と切り分ける」が崩れる）。
「fuzz」ステップの前に `cargo +"$FUZZ_NIGHTLY" metadata --locked --format-version 1 --manifest-path fuzz/Cargo.toml > /dev/null` を 1 行足せば、
lock が古いときに明示的に失敗する。現時点では head でも main マージ後でも lock は最新（確認済み）。

### Nit

1. **nightly の日付**: チャンネル `nightly-2026-11-13` の rustc は `8eb778d04 2026-11-12`。所見の実行に使った `5f4effd57 2026-11-13` は
   チャンネル `nightly-2026-11-14`（rustup のチャンネル日付は rustc のコミット日の翌日）。コメントの「Same toolchain as the runs in the
   findings document」を厳密にするなら `nightly-2026-11-14`。実害はほぼない。
2. **F-4 フィルタ**: ルート開始タグを「`<` の次が `?` `!` `/` 以外」で探すので、プロローグのコメント中の `<a` をルートと誤認しうる
   （`<!-- <a --><?xml?>…` のプロローグ重複宣言が外れる）。#40 で F-4 は直っておりフィルタごと消えるので対応不要。
3. `seed_corpus` の出力 `package_roundtrip: 74 packages` は合成変種（316 件）を数えていない。

## 判定

**converged: no** — M1'（`--include-ignored` と実行件数の assert）を直し、main をマージして m10（build 修正・9 件 un-ignore・フィルタ削除）を行えば収束。
m11 は任意だが 1 行なので推奨。
