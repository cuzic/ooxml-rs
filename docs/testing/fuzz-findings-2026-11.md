# ファジング所見（2026-11）

対象: main @c4116e9（PR #16 depth-limit マージ後）の、信頼できない入力を受ける面。

- ooxml-core: `Package::from_bytes` / `to_bytes`、`XmlDocument::parse` / `serialize` / `force_rerender`、`LensMut` の書き込みプリミティブ
- ooxml-xlsx: `Workbook::from_bytes`、セルの読み取り、`set_number` / `set_string`、保存と再オープン
- ooxml-pptx: `Presentation::from_bytes`、全スライドの読み取り、テキスト編集（本文・段落・ラン・表セル）、保存と再オープン

ハーネスは `crates/ooxml-robustness`（publish しない dev 専用 crate）、cargo-fuzz のターゲットは `fuzz/`（ワークスペース外、nightly）。
各ターゲットの不変条件は `fuzz/README.md` の表にある。crate のソースは変更していない。修正は各 crate の担当者に任せる。

## 結論（先に）

**クラッシュ（panic / abort）は crate 内では見つからなかったが、不変条件の違反が 7 件ある。** High 1・Medium 4・Low 2、ほかに Info 2。

| ID | 重大度 | 概要 | 再現テスト |
|---|---|---|---|
| F-1 | **High** | 先頭に UTF-8 BOM が 2 つある part は要素の span が 3 バイトずれる。編集すると壊れた XML を書く（xlsx: `set_number` が Ok で、保存したファイルが開けない） | `f1_double_bom_edit_writes_well_formed_xml`、`f1_double_bom_xlsx_write_saves_a_workbook_that_reopens` |
| F-2 | Medium | データがアーカイブ末尾を越える zip エントリを受け付け、無編集の保存でそのエントリを別のバイト列にして書く（Layer 1 違反、Err にならない） | `f2_entry_past_end_of_archive_is_refused_or_copied_unchanged` |
| F-3 | Medium | `LensMut::set_attr` / `remove_attr` が `xmlns:*` 宣言を拒否しない。使用中の宣言の削除で非 well-formed、再束縛で既存の名前の名前空間が黙って変わる | `f3_lens_writes_to_namespace_declarations_keep_output_meaning` |
| F-6 | Medium | part の読み取りが**申告された**非圧縮サイズで先にメモリを確保する（10 バイトのエントリで約 4 GB を要求。libFuzzer では OOM） | `f6_declared_size_does_not_drive_allocation` |
| F-7 | Medium | `Package::from_bytes` の重複名検査が O(n²)。空エントリ 65,535 個・6 MB の zip で open に 30 秒（release） | `f7_open_is_not_quadratic_in_the_entry_count` |
| F-4 | Low | ルート要素の中の XML 宣言（`<?xml …?>`）を受け付け、木から落とす。再描画で消え、前後のテキストが 2 項目に分かれる（T1b 違反） | `f4_xml_declaration_inside_content_is_refused_or_kept` |
| F-5 | Low | 名前が空の要素（`< />`、`<\r/>`）を受け付ける。再描画で `</>` を書き、どのパーサーも読めない | `f5_empty_element_name_is_refused` |
| I-1 | Info | パーサーがほかにも XML 1.0 / Namespaces の非 well-formed 入力を受け付ける（下記） | — |
| I-2 | Info | 展開後サイズや圧縮率の上限がない（zip bomb） | — |

再現テストは `crates/ooxml-robustness/tests/fuzz_findings.rs` にあり、すべて
`#[ignore = "fuzz finding F-n, see docs/testing/fuzz-findings-2026-11.md"]` 付き（CI は green のまま）。
`cargo test -p ooxml-robustness --test fuzz_findings -- --ignored` で 8 件すべてが失敗することを Rust 1.99.1 で確認した。
修正したら `#[ignore]` を外し、`crates/ooxml-robustness/src/known.rs` の対応するフィルタを消すこと。
フィルタは既知の形の入力だけを飛ばし、ファザーが同じ所見を何度も報告しないようにするためのもの（`OOXML_FUZZ_ALL=1` で無効）。

xlsx・pptx の format crate 固有の不変条件（拒否なら無変更、成功なら読み戻し一致、他 part の L1、再オープン）の違反は見つからなかった。
F-1 は xlsx 経由でも再現する（core の問題が format crate に波及する）。

## 実行内容

環境: 32 コア、rustc nightly 1.102.0（5f4effd57 2026-11-13）、cargo-fuzz 0.13.2、libFuzzer（ASan、debug assertion 付き）。
すべて `-fork=N -timeout=10 -rss_limit_mb=2048`。シードは `reference/fixtures/**` と `reference/round_trip_corpus` から
`examples/seed_corpus.rs` で実行時に作った（コミットしない）: パッケージ 74 個、XML part 1,202 個、xlsx 入力 340 個、pptx 入力 971 個、lens 48 個。

| ターゲット | 1 回目（フィルタなし） | 2 回目（`known.rs` のフィルタあり） | 2 回目の新規の失敗 |
|---|---|---|---|
| `xml_t1` | 約 10 分、3.3M 実行 | **40 分**（5 並列）、13.0M 実行、cov 2,103 | 0 |
| `xml_t1b` | 約 10 分、5.4M 実行 | **40 分**（5 並列）、6.5M 実行、cov 2,121 | 0 |
| `lens_ops` | 約 6 分、0.75M 実行 | **20 分**（4 並列）、2.8M 実行、cov 3,013 | 0 |
| `package_roundtrip` | 約 7 分、0.15M 実行 | **20 分**（5 並列）+ **15 分**（6 並列）、0.42M 実行、cov 5,872 | OOM 2 件（どちらも F-6） |
| `xlsx_edit` | 約 6 分、87k 実行 | **20 分**（5 並列）、231k 実行、cov 7,100 | 0 |
| `pptx_edit` | 約 6 分、18k 実行 | **20 分**（5 並列）+ **15 分**（10 並列）、133k 実行、cov 6,882 | 0 |

1 回目で見つかった失敗（lens 638 件、xml_t1b 316 件、package 30 件、xml_t1 6 件。`examples/replay.rs` で仕分け、`cargo fuzz tmin` で最小化）は
すべて F-1〜F-6 のどれかだった。1 回目の xlsx / pptx のログにある crash カウントは、実行中にバイナリを再ビルドしたため
fork したジョブが起動できなかった分で、成果物は 0 件（入力起因の失敗ではない）。
2 回目の開始直後にできた `crash-*` 2 件は 1 回目の残りプロセスが書いたもので、現在のハーネスでは再現しない（F-2 / F-4 の形）。
F-7 はファザーの入力サイズでは届かないので、F-2 の調査中に手で測った。

stable のスモーク（`tests/smoke.rs`、決定的な変異）: debug・既定値（`OOXML_SMOKE_ITERS=1`）で約 12 秒、
release・`OOXML_SMOKE_ITERS=30` で 27 秒、どちらも失敗なし。

## 所見の詳細

### F-1 [High] 先頭の BOM が 2 つあると span がずれ、編集で壊れた XML を書く

- **再現**: `XmlDocument::parse("\u{feff}\u{feff}<p xmlns=\"urn:t\"><item/>x</p>")` は Ok。`<item/>` の span は
  `start: 9`（正しい）だが `open_end`・`close` などは 3 小さい。`insert::<Ext>()` の後の `serialize()` は
  `"\u{feff}\u{feff}<p xmlns=\"urn:<itex<ext/>/>x</p>"` になる。
  xlsx: `poi/51850.xlsx` など 14 個の fixture（書き込みが拒否されなかったもの全部） で、worksheet の先頭に BOM を 2 つ足すと `set_number(7.0)` が Ok を返し、
  保存したファイルの再オープンが `XML error … attribute key must be directly followed by '=' or space` で失敗した。
- **影響**: 無編集なら T1 は成り立つ（prolog・ルートの span・epilog を連結すると入力全体になるため）ので、ゼロ編集の検査では見えない。
  dirty な要素があると、ずれた範囲を verbatim で書き出すので**黙って壊れたファイルを保存する**。ADR-0001 の「壊れたファイルを黙って作らない」に反する。
- **推定原因**: `xml/parse.rs:80` は最初の BOM だけを `base = 3` で読み飛ばし、`Reader::from_str(&src[base..])` に渡す。
  quick-xml は入力先頭の BOM を自分でも読み飛ばすが、その 3 バイトを `buffer_position()` に数えない。
  そのため `before` / `after`（= `base + buffer_position()`）が 3 小さい。`substr_range` から計算する `start` だけが正しく、
  両者の一致は `parse.rs:221` の `debug_assert_eq!(start, before)` でしか検査していない（release では無検査）。
  cargo-fuzz は debug assertion 付きでビルドするので、この assert で検出された。
- **修正の方向**: 2 つ目の BOM を拒否する（`UnsupportedEncoding` か `Xml`）か、quick-xml に BOM を見せないようにする。
  あわせて `debug_assert` を通常の検査（不一致なら `Err`）にするとよい。

### F-2 [Medium] アーカイブ末尾を越えるエントリを受け付け、無編集の保存で別のバイト列にする

- **再現**: stored エントリ 2 個の zip で、`a.bin` の中央ディレクトリの compressed size を `0x7FFF0000` に書き換える。
  `Package::from_bytes` も `to_bytes` も Ok で、出力の `a.bin` の生データは 148 バイトから 286 バイトに変わる（CRC はそのまま）。
  ファザーでは、ローカルヘッダの extra 長などでデータ開始位置が末尾を越える形も見つかった（入力側の生データ 0 バイト、出力側 715 バイト）。
  検出したのは 20 件で、すべてこの 2 つの形のどちらかだった。
- **影響**: README の「`Raw` は (name, method, CRC-32, compressed bytes) unchanged」が成り立たない。
  壊れた入力から、別の形で壊れた出力を Err なしに作る。`read_part` は Err（`Invalid checksum` など）になる場合と、
  Ok になる場合（compressed size だけが過大な stored / deflate）がある。
- **推定原因**: open 時に検査するのは名前だけ（`package.rs:121-125`）。`write_to` の `raw_copy_file`（`package.rs:301`）は
  zip crate が `take(compressed_size)` で読める分（アーカイブの残り全部）をコピーし、ヘッダは申告値のまま書く。
- **修正の方向**: open 時に各エントリの `data_start() + compressed_size() <= len` を検査し、満たさなければ `Error::Package`。

### F-3 [Medium] lens が名前空間宣言への書き込みを拒否しない

- **再現**: `<p xmlns="urn:t" xmlns:o="urn:o"><o:z/></p>` で
  `remove_attr(QName::new(XMLNS_NS, "o"))` は Ok で、出力は `unbound prefix "o"` で parse できない。
  `set_attr(QName::new(XMLNS_NS, "o"), "urn:other")` も Ok で、出力を parse すると `o:z` の名前空間が `urn:other` になる（木は `urn:o` のまま）。
- **影響**: どちらも呼び出し側が `XMLNS_NS` の `QName` を渡したときだけ起きるので、信頼できない入力から直接は届かない。
  ただし README の「no primitive can write a part that is not well-formed」（`InvalidXmlChar` の項）と、
  dirty フラグが表す「木 = 出力」の対応が崩れる。生成コードが `xmlns` を属性として持つことはないが、手書きの format crate コードは渡せる。
- **修正の方向**: `XMLNS_NS` の属性への `set_attr` / `remove_attr` を新しいエラー（例: `NamespaceDeclaration`）で拒否する。
  宣言の追加が必要になったら専用の API にする（README の「Phase0 never declares namespaces」と同じ考え方）。

### F-6 [Medium] 申告された非圧縮サイズでメモリを確保する

- **再現**: `a.bin`（10 バイト、stored）の中央ディレクトリの uncompressed size を `0xF0000000` にする。
  `read_part("a.bin")` は約 4 GB の確保を要求する（テストはグローバルアロケータで最大要求サイズを記録して確認）。
  ファザーでは deflate のエントリ（圧縮後 191 バイト、申告 3,087,008,061 バイト）で `-rss_limit_mb=2048` の OOM として検出し、2 回目でも `malloc(4278125026)` などの OOM が 2 件出た。スタックは `package.rs:181` ← `package.rs:138`（open 時の `[Content_Types].xml` 読み取り）だった。
- **影響**: Linux の overcommit 下では触らないページは確保されないので、実害が出ないことが多い。
  overcommit を切った環境、32 ビット環境、メモリ上限のあるコンテナでは abort（`handle_alloc_error`）になる。
  `[Content_Types].xml` と `_rels/.rels` は open 時に読むので、**open だけで**到達する。
- **推定原因**: `package.rs:181` の `Vec::with_capacity(file.size() as usize)`。
- **修正の方向**: 確保量を `min(file.size(), 上限)` にする（例えば compressed size の定数倍や数 MiB）。I-2 の上限と合わせて決めるとよい。

### F-7 [Medium] open が entry 数に対して二乗時間

- **再現**: 空の stored エントリ n 個の zip。release ビルド（ファザー実行中の負荷あり）で
  n = 10,000 → 0.99 秒、40,000 → 12.0 秒、65,535（6 MB）→ 30.2 秒。zip crate の `ZipArchive::new` だけなら 12〜36 ms。
  テストは 4,000 と 16,000 の時間比を見る（debug で 0.84 秒 → 12.2 秒、比 14.6。線形なら 4 前後）。
- **影響**: 数 MB のファイルで数十秒 CPU を占有できる（サービスで受け付けると DoS）。ZIP64 なら entry 数はさらに増やせる。
- **推定原因**: `package.rs:123` の `parts.iter().any(|p| p.name.eq_ignore_ascii_case(&name))`。
  `find()` も線形探索なので、format crate が part ごとに `contains` / `read_part` を呼ぶ処理も同様に二乗になりうる。
- **修正の方向**: ASCII 小文字化した名前の `HashSet` で重複検査し、`find` 用に `HashMap<String, usize>` を持つ。

### F-4 [Low] 内容中の XML 宣言を受け付けて木から落とす

- **再現**: `<a>x<?xml version="1.0"?>y</a>` は Ok。`force_rerender` 後の出力は `<a>xy</a>`。
  元の木は `Text("x")`, `Text("y")` の 2 項目、再 parse した木は `Text("xy")` の 1 項目なので `structurally_eq` が false（T1b 違反）。
  `<a><?xml encoding="latin1"?></a>` は `UnsupportedEncoding` になる。ファザーの最小再現は `<sst>r<?xml?>,</sst>`（20 バイト）。
- **影響**: XML 宣言は文書の先頭にしか置けないので、入力は非 well-formed。親要素が dirty になると宣言が出力から消える（黙った欠落）。
- **推定原因**: `parse.rs:99` の `Event::Decl` はエンコーディングを検査するだけで、ルートの中でも項目を作らない。
- **修正の方向**: 先頭（BOM の直後）以外の `Event::Decl` を `Error::Xml` で拒否する。

### F-5 [Low] 名前が空の要素を受け付ける

- **再現**: `< />`、`<\r/>`、`<a>< /></a>` が Ok で、ルートの `XName` の local name が空になる。
  `force_rerender` 後の出力 `</>` は parse できない。
- **影響**: 入力は非 well-formed。名前が空の要素はスキーマの要素と一致しないので、通常の編集で own_dirty になることはなく、
  実害は T1b 検査と `force_rerender` に限られる。
- **推定原因**: quick-xml は `<` の直後の空白を名前の区切りとして扱い、空の名前をエラーにしない。`parse.rs:258` も名前を検査していない。
- **修正の方向**: `open()` で local name・prefix が空なら `Error::Xml`。I-1 と合わせて Name 生成規則を検査してもよい。

### I-1 [Info] ほかの非 well-formed 入力

parse が Ok を返し、T1 / T1b は成り立つ（再描画しても同じ形で読める）ので不変条件の違反ではない。
「書き込みでは拒否する文字・構文を、読み込みでは受け付ける」非対称の記録として残す。

| 入力 | 備考 |
|---|---|
| `<a>\0</a>`（生の U+0000）、`<a>&#xFFFE;</a>` | `set_text` は同じ文字を `InvalidXmlChar` で拒否する |
| `<1a/>`、`<a:b:c xmlns:a="u"/>` | Name / QName の生成規則違反（local name が `b:c` になる） |
| `<a xmlns:p=""/>`、`<a xmlns:xml="urn:x"/>`、`<xmlns:a xmlns:xmlns="u"/>` | Namespaces in XML 1.0 で禁止 |
| `<a xmlns:p="u" xmlns:q="u" p:x="1" q:x="2"/>` | 展開名が重複する属性 |
| `<a/>trailing`、`&lt;<a/>` | ルートの外の文字データ・参照 |

### I-2 [Info] 展開後サイズの上限がない

`read_part` と open 時の `[Content_Types].xml` / `.rels` の読み取りは、申告サイズが正しければ展開後のサイズに上限を設けない。
deflate の圧縮率は最大で約 1:1000 なので、1 MB の part で約 1 GB を確保させられる。
ファザーの入力サイズでは到達しないため所見としては数えないが、F-6 の修正と合わせて上限（設定可能な値）を決めることを勧める。

## 再現と再実行

```sh
# 再現テスト（stable 1.99）
cargo test -p ooxml-robustness --test fuzz_findings -- --ignored
# stable のスモーク（既知の所見は known.rs で除外、CI の cargo test にも含まれる）
OOXML_SMOKE_ITERS=50 cargo test --release -p ooxml-robustness --test smoke
# ファザー（nightly）
cargo run -p ooxml-robustness --example seed_corpus -- fuzz/corpus
cd fuzz && cargo +nightly fuzz run xml_t1b corpus/xml_t1b -- -max_total_time=600
# 成果物の仕分け（stable、1 入力 1 行）
cargo run -p ooxml-robustness --example replay -- xml_t1b fuzz/artifacts/xml_t1b/*
```

`.github/workflows/fuzz-smoke.yml`（`workflow_dispatch` と毎週月曜の schedule）は、stable スモークを release で、
各ターゲットの cargo-fuzz を 60 秒ずつ実行する。`ci.yml` は変更していない。
