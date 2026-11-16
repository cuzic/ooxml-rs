# PR #40 レビュー（Opus, round 1）

対象: `fix/core-fuzz-findings` @a05b771（base 1cf8132）。所見と再現テストは PR #36 @16e6999。

## 結論（先に）

**Blocker / Major はない。F-1〜F-6 と Info 項目はすべて閉じている。実ファイルでの退行もない。** Minor が 4 件、Info が 3 件ある。

- #36 の `fuzz_findings.rs` を #40 と組み合わせ、`--ignored` 付きで実行した。結果は 7 passed / 1 failed。失敗したのは F-7 だけで、F-7 は本 PR の範囲外（#14 で修正）。
- 実ファイル 298 個（`reference/**` 75 個、untrusted corpus 223 個）で、ファイルごと・part ごとに次の結果を記録し、base と #40 で比べた。
  - open の結果
  - 全 part の `read_part` の結果
  - .xml / .rels / .vml の `xml_part` の結果
  - 無編集保存の Layer 1 一致

  **出力は完全に一致した**（新しい拒否は 0 件、L1DIFF も 0 件）。
- 申告サイズ 0 の bomb ではメモリを使い切れない。zip 9.6 は申告サイズを超えた時点で読み取りを Err にするためで、最大確保量は 64 KiB だった（下の検証 2）。
- cargo-fuzz（nightly、フィルタなし）を `package_roundtrip`、`xml_t1b`、`lens_ops` で各 90 秒実行した。失敗は 0 件。#36 の 1 回目では、同じ程度の時間で数百件の失敗が出ていた。stable のスモーク（フィルタなし、`OOXML_SMOKE_ITERS=10`、release）も通った。
- 判定: **converged: yes**（Minor は任意対応、または follow-up でよい）。

## 検証したこと

1. **所見ごとの修正**: F-1、F-2、F-3、F-4、F-5、F-6 の再現テストはすべて pass。#36 の `known.rs` のフィルタを外した状態（`OOXML_FUZZ_ALL=1`）でも、ファザーとスモークは green。
2. **申告サイズで嘘をつく bomb**: 64 MiB の空白を deflate したエントリ（約 64 KiB）で試した。中央ディレクトリとローカルヘッダの申告サイズは 0、1000、16 MiB の 3 通り。どれも zip 9.6 の `Crc32Reader` が `File is larger than its declared uncompressed size` で Err にした。最大確保量は 0 と 1000 で 47 KiB、16 MiB で 32 MiB だった。`[Content_Types].xml` で申告 0 にした場合、open は Err で、最大確保量は 64 KiB。
   - 結論として、宣言サイズの合計（`max_package_bytes`）は実際に展開される量の上限になる。ParseOptions のドキュメントの主張は zip 9.6 で正しい。
3. **データディスクリプタ（bit 3）**: `ZipWriter::new_stream` で作ったパッケージは open、read、parse、Layer 1 一致まで通る。中央ディレクトリのサイズを使うので、ローカルヘッダの 0 は影響しない。
4. **重なったエントリ（Fifield 型）**: 中央ディレクトリの 4 エントリが同じローカルヘッダを指す場合、申告サイズの合計で `PackageTooLarge` になる。
5. **release の位置検査のコスト**: `poi-bug59058.docx` の `word/document.xml`（1.35 MB）を 15 回 parse し、最良値を比べた。base は 14.3〜14.5 ms、#40 は 14.7〜15.2 ms で、差は +2〜5%。U+0000 の `find` を含めた値で、問題にならない。
6. **XML の境界ケース**（いずれも期待どおり）
   - 受け付ける: 宣言の後の `<?xml-stylesheet?>`、宣言の後の改行、`\u{feff}<a/>`、宣言の後や内容中・ルートの後の U+FEFF（T1 一致）、`</a\n>`。
   - 拒否する: `\n<?xml …?>`（XML 1.0 の規則どおり）、コメントや CDATA 中の U+0000、`<a p:="1" xmlns:p="u"/>`、`<a xmlns:="u"/>`。
   - 以前から変わらない: `&#0;` は quick-xml が `Xml` で拒否する。UTF-16 の part は base でも `UnsupportedEncoding("UTF-16 (byte order mark)")` だったので、本 PR の退行ではない。
7. **lens の xmlns 拒否と ADR の計画**: ADR-0011 §「What else core needs」1 は、宣言を別 API の `LensMut::declare_namespace` で行う計画で、created node だけが対象。`set_attr` の拒否とは衝突しない。core の内部で `node.attrs` を直接触る経路も残っている（`set_attr` 以外に属性を書く公開プリミティブはない）。
   - 全リモートブランチを grep したが、format crate で `set_attr` に xmlns を渡すコードはない。ADR-0013 には名前空間宣言の記述がない。
8. **エラー型**: `Error` も `WellFormedness` も `#[non_exhaustive]`。`ParseOptions` も `#[non_exhaustive]` なので、フィールドを足しても非互換にならない。`open_with`、`from_bytes_with`、`parse_options()` が新しい上限を一貫して運ぶ。OPC metadata の part は `max_depth` だけを既定値以上に引き上げ、バイト上限は引き上げない。どちらも妥当。
9. **手動ミューテーション（21 個）**: 13 個を検出し、8 個が生き残った。生き残ったものの内訳は m3 にまとめた。

## 所見

### m1 [Minor] 実際の確保量は `max_part_bytes` の最大 2 倍になる。既定値では 1 MB 程度のファイルから 2 GiB を 1 回で要求できる

`package.rs` の `read_original` について。

```rust
let mut buf = Vec::with_capacity(declared.min(INITIAL) as usize);
file.take(limit.saturating_add(1)).read_to_end(&mut buf)?;
```

`read_to_end` は容量を倍々で伸ばす。そのため、申告サイズどおりに上限ちょうどまで展開されるとき（正当な申告なので zip も止めない）、最後の 1 回の伸長で容量が 2×limit になる。

- **根拠**: `max_part_bytes = 16 MiB` で、ちょうど 16 MiB の part を読んだときの最大の単一確保は 33,554,432（2.00×）だった。PR 自身の `zip_bomb_is_bounded_by_the_limits` も `peak <= 40 << 20`（16 MiB の上限に対して）を許している。既定値（1 GiB）なら、deflate で約 1 MB の part から 2 GiB を 1 回で要求できる。`xml_part` はさらに `Box<str>` に複写する。
- **影響**: F-6 が問題にした「overcommit なし・メモリ上限のあるコンテナ」では、まだ大きい。`read_to_end` は `try_reserve` を使うので abort にはならず、Io Err になる。宣言サイズに引きずられる問題自体は直っているので Minor。
- **提案**（どれか）
  - zip 9.6 が申告サイズを強制することを前提に、`try_reserve_exact(min(declared, compressed_size × 1032 + 64 KiB))` を確保する。deflate の圧縮率は最大約 1:1032 なので、申告が過大でも圧縮後サイズで抑えられる。その後は伸長しないので、確保量はほぼ正確になる。
  - 既定値やドキュメントに「ピークは上限の約 2 倍 + parse の複写」と書く。
  - XML part 用の既定値を下げる（例: 256 MiB）。コーパスで最大の XML part は 1.35 MB。

### m2 [Minor] `max_package_bytes` が Stored のエントリと、読まれない part も数える

- open 時の合計には、全エントリの `file.size()` が入る。Stored のエントリは展開後サイズ = 圧縮後サイズ ≤ ファイル長なので、bomb にはなりえない。また、触らない part は圧縮したまま copy-through され、展開されない。
- そのため、動画を多く埋め込んだ 4 GiB 超のプレゼンテーションは、テキストを 1 か所直すだけでも open で拒否される。
- **提案**: 合計の対象を Stored 以外のエントリに限る（Stored はファイル長で上限がある）。あるいは README に「巨大なメディアを含むパッケージは `with_max_package_bytes` で上げる」と 1 行足す。コーパスに該当するファイルはなく、現時点の実害はない。

### m3 [Minor] 防御のための分岐の一部をテストが固定していない（ミューテーションが生き残る）

| ミューテーション | 結果 | コメント |
|---|---|---|
| 属性の `local.is_empty() \|\|` を削除 | 生存 | `<a p:="1" xmlns:p="u"/>`（拒否されることは確認済み）を `f5_empty_names_are_refused` に足せば検出できる |
| `take(limit + 1)` → `take(limit)`、および読み取り後の `buf.len() > limit` を削除 | 生存 | zip 9.6 が申告サイズを強制するので、事前の `declared > limit` の検査の後では到達しない。ただし `take(limit)` にすると、ちょうど上限サイズの part で内側の reader が EOF に達せず、**CRC 検査が飛ぶ**。`max_package_bytes` のドキュメントは「zip が申告超過を拒否する」に依存しているので、これを固定するテストを足すべき。例: 申告を 0 / 1000 に書き換えた deflate bomb が Err になり、確保量が小さいこと（検証 2 と同じもの） |
| `declared > max_package_bytes` → `>=` | 生存 | 境界のテスト（合計 = 上限なら Ok）がない |
| End / Start の `PositionMismatch` の検査（3 か所）を削除 | 生存 | 2 つ目の BOM を拒否した後は、これを起こす入力が見つかっていない。多層防御としては妥当で、テストできないことは許容する。コメントにそう書いてあれば十分 |
| `end > len` → `>=`、`data_start` が `None` のとき → 0 | 生存 | 等価ミューテーション（中央ディレクトリが後ろに続くので end == len にはならない。`by_index_raw` の後の `data_start` は常に `Some`） |

### m4 [Minor] PR 本文と README の F-6 の記述

README の「the declared size decides the refusal, never the allocation」は正しい。ただし、`max_package_bytes` が実際の上限になる理由（zip ≥ 9.x が申告超過を拒否すること）は、`ParseOptions` の doc comment にしか書かれていない。`Cargo.toml` は `zip = "9"`、lock は 9.6.0 なので、m3 のテストで固定すれば、この記述のままでよい。

### Info

- **I-a**: F-7（二乗時間）は範囲外で、#14 で修正される。#36 側で `#[ignore]` を外すのは #14 のマージ後になる。PR 本文の手順どおり。
- **I-b**: `&#xFFFE;`、`<?XML …?>`（PI として受け付ける）、`<a:b:c>` など、I-1 の残りは今回の対象外。T1 / T1b は成り立つ。
- **I-c**: `EmptyPrefixBinding` は XML 1.1 の prefix の undeclare も拒否するが、OOXML は XML 1.0 なので問題ない。コーパスにも該当はない。

## 判定

**converged: yes** — Blocker・Major はない。所見の修正、実ファイルでの退行がないこと、bomb の上限、xmlns 拒否の設計を確認した。m1〜m4 は任意対応。対応するなら、m3 のテスト 2 件（申告を偽った bomb、属性の空 local name）が最も安く、効果が高い。

## 再現（scratch）

`/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr40/` 以下。

- `ws/`: #40 に #36 の `ooxml-robustness` と `fuzz/` を合わせたもの。
- `base/`: 1cf8132。
- `probe-src/`: `probe_dump.rs`（コーパスの比較）、`probe_bench.rs`、`review_probes.rs`、`mutate.py`。
- `dump-{base,ws}.txt`: コーパスの結果（diff は空）。
