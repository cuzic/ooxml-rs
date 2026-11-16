# PR (flaky peak alloc) レビュー（Opus, round 1）

対象: PR #64 `fix/flaky-peak-alloc`（ded3e71）— `crates/ooxml-core/tests/fuzz_regressions.rs` の F-6 割り当てプローブを thread-local 化。

## 結論（先に）

**根本原因の分析は正しい。修正も妥当で、テストは空振り（vacuous）になっていない。Blocker / Major なし。マージ可。**

- CI 失敗（run 43853377167, head 69aaa68）は `fuzz_regressions.rs:259:5`（f6 の 2 つ目の assert、宣言サイズ 256 MiB のケース）で `requested 67108864 bytes`。
  - 67108864 = 64 MiB ちょうど。これは並行する `zip_bomb_is_bounded_by_the_limits` の `bomb.resize(64 << 20, b' ')`（cap 3 → amortized growth で必要量 64 MiB ちょうど）と一致する。この行は `MEASURE` ロックの**外**で走るので、旧実装のプロセス共通 `ARMED` では f6 の計測窓に混入しうる。
  - `read_part` の実回帰なら値は宣言サイズ 256 MiB（268435456）になるはず（下記注入実験で確認）。64 MiB にはならない。
  - `read_original`（package.rs:311–328）は a05b771（F-6 修正）以降変更なし。#49/#52/#54 は触っていない。
- `read_part` は呼び出しスレッドだけで割り当てる。zip 9.6 は `default-features = false, features = ["deflate"]` で rayon 無効、flate2 / zlib-rs にもスレッド生成なし。thread-local プローブで取りこぼす割り当ては現状ない。

## 検証（scratchpad: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-flaky/`、git clone コピー）

| 実験 | main (5e906ef) | PR (ded3e71) |
|---|---|---|
| テストバイナリを `--test-threads=16` で反復実行 | 全体 5 回 + fixture 以外 20 回: 失敗 0/25（自然再現せず、タイミング依存） | 全体 5 回: 失敗 0/5 |
| 決定的な混入プローブ: 別スレッドが 64 MiB を連続割り当てする間に `peak_alloc(read_part)` を 50 回 | **FAIL `requested 67108864 bytes`**（CI と同じ値） | PASS |
| `read_part` 先頭に同一スレッドで 64 MiB 割り当てを注入 | f6・zip_bomb とも FAIL | f6・zip_bomb とも **FAIL**（空振りではない） |
| 実際の F-6 回帰を注入（`Vec::with_capacity(declared as usize)`） | — | f6 **FAIL `requested 268435456 bytes`** |

自然実行での再現はできなかったが、混入プローブで main の機構と CI と同じ値を決定的に再現できた。PR では同じプローブが通り、本物の回帰はこれまでどおり検出される。

## GlobalAlloc 内の thread_local の安全性

- `const { Cell::new(false) }` 初期化で、`Cell<bool>` は Drop なし。Linux ではネイティブ `#[thread_local]` に下ろされ、初回アクセスでも遅延初期化・デストラクタ登録・割り当ては起きない。アロケータから再入しない。
- `try_with(...).unwrap_or(false)` なので、TLS 破棄中でもアロケータ内で panic しない（Drop なしの場合は実際には失敗しないが、防御として正しい）。
- `peak_alloc` 側の `ARMED.with` はアロケータ外なので問題ない。
- `LARGEST` はグローバルのままだが、書くのは armed なスレッドだけで、`MEASURE` で直列化されている。問題なし。

## Findings

- **Minor（任意）** `fuzz_regressions.rs:25-27` のドキュメントコメント: 「計測スレッド以外の割り当ては数えない」という前提は、`read_part` が単一スレッドであることに依存している。将来 zip の `rayon` feature や並列展開を入れるとプローブが取りこぼす。その旨を 1 行書いておくとよい。例:
  ```rust
  /// Assumes the measured code allocates on the calling thread (true while zip
  /// is built without `rayon`).
  ```
- **Nit** `f()` が panic すると、そのスレッドの `ARMED` が true のまま残る。ただしテストスレッドはそのまま終了するので実害はない（旧実装はグローバルに残るのでむしろ悪かった）。対応不要。
- **Nit** PR 本文の検証が「3/3 runs pass」だけで、flake の修正としては証拠が弱い。上の混入プローブの結果を本文に 1 行足すと、レビューする人が納得しやすい（任意）。

## 判定

**converged: yes**（Blocker / Major なし。Minor・Nit は任意）
