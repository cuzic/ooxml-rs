# ooxml-rs — 作業ガイド

設計の出発点は `docs/adr/0001-lossless-round-trip-architecture.md`。実装前に読むこと。

## Rust バージョン方針

- MSRV は **1.99**、edition は **2024**（`Cargo.toml` の `[workspace.package]`）。
  古い Rust 向けの回避策（外部 crate による代替、手書きのポインタ演算など）は書かない。
- 新しい std API があるなら、それを第一候補にする。既存コードを触るときに古い書き方を見つけたら置き換える。
- 1.97 以降の機能はモデルの学習データにない可能性が高い。下の表を正とし、
  不明な API は `rustup doc --std` か <https://doc.rust-lang.org/stable/std/> で確認してから使う。

### 優先して使う API（ooxml-rs での用途つき）

| 用途 | 使うもの | 版 | 使わないもの |
| --- | --- | --- | --- |
| Layer 2 `ElementSpan` のオフセット計算（quick-xml が借用で返したスライス → 入力内の範囲） | `str::substr_range` / `<[T]>::subslice_range` | 1.98 | `as_ptr()` の差分計算 |
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
| テストでのパターン一致 | `assert_matches!` | 1.96 | `assert!(matches!(…))` |
| 型エラーの文言（`ChildOf` など生成 trait） | `#[diagnostic::on_unimplemented]`、`#[diagnostic::do_not_recommend]` | 1.78 / 1.85 | — |
| 生成表のコンパイル時検証 | インライン `const { … }` | 1.79 | 実行時の `assert!` |
| lint 抑制 | `#[expect(…)]` | 1.81 | `#[allow(…)]` |

### まだ使えないもの（1.99 時点で nightly 限定。回避策を作り込まない）

`macro_rules!` による derive / 属性マクロ（`macro_derive`）、const trait（`const_trait_impl`）、
`adt_const_params`、`generic_const_exprs`。

## コード生成

- スキーマ由来のコードは生成ツールで作り、生成物をリポジトリに置く（`build.rs` での生成や、大規模な proc-macro は使わない）。
- 生成物は手で直さず、生成ツールか入力データを直す。

## 検証

- `cargo clippy --workspace --all-targets -- -D warnings`。clippy は `rust-version` を読み、
  MSRV 内で使える新しい書き方を提案する lint が有効になるので、警告を残さない。
