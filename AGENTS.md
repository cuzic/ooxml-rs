# ooxml-rs — エージェント共通ガイド（Codex・Claude 共通）

Rust で xlsx / docx / pptx を **ロスレスに往復編集** するライブラリ。設計の出発点は
`docs/adr/0001-lossless-round-trip-architecture.md`。実装前に、担当フェーズの ADR
（`docs/adr/`、Accepted のもの）とその review ファイルを読むこと。

## 役割分担

| 担当 | 仕事 |
| --- | --- |
| Claude | ADR の設計と spike、owner への質問の整理、PR の敵対的レビュー（Blocker/Major 探し）、マージ判断 |
| Codex | Accepted ADR に沿った実装、レビュー指摘の修正、main の取り込みと衝突解消、テスト・fixture・fuzz target の追加 |
| CI / スクリプト | テスト、clippy、`codegen --check`、python-pptx オラクル、変異テストやコーパス走査などの機械的検証 |

仕事は「タスク仕様」（スコープ・受け入れ条件・触ってよいファイル）で渡す。新しい作業は `[codex] …` の Issue、
既存 PR への修正や main の取り込みはその PR の `## [codex] タスク仕様` コメントに書く。
レビュー指摘の根拠は PR ブランチにコミットされた `docs/reviews/prNN-roundK.md` を読む。
仕様に無い判断が要るときは、実装で決めずに PR 本文の「Open questions」に書いて止める。

## 完了の定義（どの PR も）

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask codegen --check
```

- 3 つとも通ること。
- `ooxml-pptx` か `ooxml-core` に触れたら、CI の python-pptx オラクル（`.github/workflows/ci.yml`）も通ること。
  手元で回す場合（pip にネットワークが要る。無ければ CI に任せる）:

  ```sh
  T=$(mktemp -d)   # リポジトリの外
  python3 -m venv "$T/venv" && "$T/venv/bin/pip" install -r crates/ooxml-pptx/tests/oracle/requirements.txt
  cargo run --release -p ooxml-pptx --example oracle_cases -- "$T/cases"
  "$T/venv/bin/python" -I crates/ooxml-pptx/tests/oracle/check.py "$T/cases" reference/fixtures/pptx
  ```
- **拒否は無変更**: 編集 API が `Err` を返したら、保存結果は入力とバイト単位で同一（`Err ⇒ byte-identical`）。
  新しい拒否には必ずこのテストを付ける。
- **局所性**: 編集は触ったノードの span の外を変えない（ADR-0001 の T1/T1b/T2）。
- 新しい公開 API・拒否・挙動変更は、該当 ADR の post-acceptance note か PR 本文に記録する。
- 新しいコードには、それを壊す変異（手書きでよい）を最低 1 つ用意し、テストで落ちることを確かめる。

## 安全規則

- **第三者ファイルは信頼しない入力**。コーパスや fixture を読むスクリプトは `python3 -I`、
  エンティティ解決なし、DTD を含む part は読み飛ばす。ダウンロードしたものを実行しない。
- プローブや一時ファイルはリポジトリの外（スクラッチ領域）に置く。リポジトリに `PWNED` のような
  検証用の残骸を残さない。
- 作業が終わり、PR を作るか更新して報告を書いたら、**その作業のために自分で作った**一時クローン、`/tmp` の作業ディレクトリ、
  build target（`CARGO_TARGET_DIR`）を消す。1 つの作業で数十 GB になることがある。消す前に、すべて push 済みであること
  （`git status` が空で、`git log --branches --not --remotes` も空）を確かめる。
  ほかのエージェントと共有している target や、タスク用の `git worktree` は消さない（worktree はマージ後に Claude が片付ける）。
  PR 本文で参照する成果物は残してよい。
- 生成物（`crates/ooxml-schema/src/generated/`、`crates/ooxml-core/src/generated/parts.rs`、
  `codegen/report.txt`）は手で直さず、
  `codegen/allowlist/*.toml` や `codegen/overrides.toml` を直して `cargo xtask codegen` で再生成する。
  生成型の公開名を変える変更は ADR-0002 の note に書く。
- コミットやコメントにマシン固有の情報（ユーザー名、uid、絶対パス）を入れない。

## Git の約束

- ブランチは `plan/… impl/… core/… fix/… test/… docs/… chore/… ci/… perf/… spike/…`。
  1 つの作業に 1 つの `git worktree`。共有 checkout でブランチを切り替えない。
- **merge で追従し、rebase / force-push はしない**。衝突した生成物は再生成する。
- PR ブランチには Claude のレビューファイルのコミットも入る。既存 PR で作業を始める前と、push が拒否されたときは
  `git pull --no-rebase origin <branch>` する（ブランチ名を必ず書く。上流が main のままだと main を取ってくる）。
  新しいブランチの最初の push は `git push -u origin <branch>`。
- main の取り込みで手書きの衝突解消をしたら、そのマージコミットのメッセージに、何をどう解いたかを書く
  （マージ前に `git show --remerge-diff` で確認される）。
- Codex は PR を作って更新するが、**マージはしない**。マージは Claude が `.claude/skills/merge-pr` の手順で
  行う（owner が行ってもよい）。レビューを担当した Claude は、レビューファイルのコミットだけを PR ブランチに push する。
- 積み上げ PR はタイトルに「stacked on #NN」と書く。
- コミット末尾に、使っているツールの `Co-Authored-By:` トレーラーを付ける
  （例: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`）。PR 本文の末尾はツールの既定の署名に従う。
- レビュー指摘を直すコミットのメッセージには指摘 ID（`M1` `m2` など）を書く。

## リポジトリの地図

| 場所 | 中身 |
| --- | --- |
| `crates/ooxml-core` | OPC パッケージ、Layer 2 の `XmlDocument`/`RawNode`/lens、`RegistryPlan`（part・関係の追加削除） |
| `crates/ooxml-schema` | `cargo xtask codegen` の生成物（`*View`/`*ViewMut`、`ChildOf`、slot） |
| `crates/ooxml-{xlsx,docx,pptx}` | 各形式の API。`examples/manual_check.rs` が owner 用の手動確認ファイルを書き、`cargo xtask manual-check <outdir>` が 3 形式分を zip と CHECKLIST.md にまとめる |
| `crates/ooxml-bench` | 性能計測 |
| `crates/ooxml-robustness`, `fuzz/` | fuzz target と oracle（**まだ main に無い**。PR #36 で入る予定） |
| `xtask/` | `cargo xtask codegen`（生成）と `cargo xtask manual-check` |
| `codegen/` | allowlist、overrides、生成レポート |
| `spikes/` | ADR ごとの計測用スクリプト（使い捨て。コーパスは信頼しない入力として読む） |
| `reference/` | 同梱した SDK データ・ECMA XSD・fixture（`reference/fixtures/<format>/`） |
| `docs/adr/`, `docs/reviews/` | 設計と、そのレビュー記録（`docs/reviews/prNN-roundK.md`） |
| `docs/process.md` | 開発プロセスの実態 |

## Rust バージョン方針

- MSRV は **1.99**、edition は **2024**（`Cargo.toml` の `[workspace.package]`）。
  古い Rust 向けの回避策（外部 crate による代替、手書きのポインタ演算など）は書かない。
- 新しい std API があるなら、それを第一候補にする。既存コードを触るときに古い書き方を見つけたら置き換える。
- 1.97 以降の機能はモデルの学習データにない可能性が高い。下の表を正とし、
  不明な API は `rustup doc --std` か <https://doc.rust-lang.org/stable/std/> で確認してから使う。

### 優先して使う API（ooxml-rs での用途つき）

| 用途 | 使うもの | 版 | 使わないもの |
| --- | --- | --- | --- |
| Layer 2 `ElementSpan` のオフセット計算（quick-xml が借用で返したスライス → 入力内の範囲） | `str::substr_range` / `<[T]>::subslice_range`（戻り値は `std::ops::Range` ではなく新しい `core::range::Range`） | 1.98 | `as_ptr()` の差分計算 |
| 数値属性（`r="B2"` の行番号、`s="3"`）の出力 | `<整数>::format_into` + `core::fmt::NumBuffer` | 1.98 | `itoa` crate、`to_string()` |
| 引用符・括弧で囲まれた値の除去 | `str::strip_circumfix` | 1.98 | `strip_prefix` と `strip_suffix` の連続呼び出し |
| `Option`/`Result` の既定値への変換 | `map_or_default` | 1.98 | `map_or(Default::default(), …)` |
| 子要素を挿入してすぐ編集（dirty フラグ設定） | `Vec::insert_mut` / `push_mut` | 1.95 | `insert` の後に `&mut v[i]` を取り直す |
| 名前空間ごとの feature による生成コードの切り替え | `cfg_select!` | 1.95 | `cfg-if` crate |
| QName による分岐 | match の腕での `if let` ガード、let chains | 1.95 / 1.88 | 入れ子の `if let` |
| エラー経路のヒント | `core::hint::cold_path` | 1.95 | — |
| タグ・属性を文字列化せずに出力 | `std::fmt::from_fn` | 1.93 | 中間の `String` を作ること |
| バイト位置を文字境界へ丸める（エラー表示での切り出し） | `str::floor_char_boundary` / `ceil_char_boundary` | 1.91 | 手書きのループ |
| 型の識別を const 文脈で行う（生成表の検証） | const な `TypeId::of` | 1.91 | — |
| 逆引き表などの遅延初期化 | `LazyLock`（`get` / `force_mut` は 1.94） | 1.80 | `once_cell`、`lazy_static` |
| テストでのパターン一致 | `std::assert_matches!`（prelude に無いのでパス指定か `use`） | 1.96 | `assert!(matches!(…))` |
| 型エラーの文言（`ChildOf` など生成 trait） | `#[diagnostic::on_unimplemented]`、`#[diagnostic::do_not_recommend]` | 1.78 / 1.85 | — |
| 生成表のコンパイル時検証 | インライン `const { … }` | 1.79 | 実行時の `assert!` |
| lint 抑制 | `#[expect(…)]` | 1.81 | `#[allow(…)]` |

### まだ使えないもの（1.99 時点で nightly 限定。回避策を作り込まない）

`macro_rules!` による derive / 属性マクロ（`macro_derive`）、const trait（`const_trait_impl`）、
`adt_const_params`、`generic_const_exprs`。
