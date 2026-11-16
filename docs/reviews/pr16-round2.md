# PR #16 レビュー（Opus, round 2）

対象: `core/depth-limit` の 61006a3（round 1 への対応コミット）。round 1 の記録は `docs/reviews/pr16-round1.md`。
プローブ: scratchpad の `review-pr16-r2/`（HEAD を `git archive` で複製したもの）。

- `crates/ooxml-core/examples/probe16r2.rs`: 操作ごとに必要な最小スタックを二分探索で測る。コーパスの走査と、pretty Debug の出力サイズの測定もする。
- `crates/ooxml-pptx/examples/probe16pptx.rs`: pptx の再帰走査と、パスの増幅を測る。

環境は rustc 1.99.1。debug と release の両方で測った。

## 結論（先に）

**収束（converged: yes）。Blocker・Major はない。**

- **M1 は解消した。** 既定値は 256 になった。深さ 256 で最もスタックを使う操作は debug ビルドの `clone()` で、約 431 KiB だった（release では 143 KiB）。テストは 1 MiB スレッドで回しているので、約 2.4 倍の余裕がある。手元で既定値を 1024 に戻すと、`recursive_operations_at_the_limit_fit_a_small_stack` は `clone + drop` で stack overflow になった。PR 本文の主張どおりである。
- **m1・m2・m3・m5 は解消した。** m4（pptx が comments part の parse 失敗を握りつぶす件）は範囲外として持ち越しで、PR 本文に pptx 担当者向けの注記がある。
- **回帰はない。** ホスト上で見つかった OOXML パッケージを重複を除いて走査した。4,730 パッケージ・92,602 part で、そのうち `reference/fixtures/**` を含む 480 ファイルはすべて既定値で parse できた。実ファイルの最大の深さは 33 のままである。256 で拒否されたのは 5 ファイルで、すべて深さ攻撃の再現用ファイルだった（後述 n4）。
- 新しい指摘は Minor 2 件と nit・観察 2 件である。
  - n1: pretty Debug が O(N·d²) の時間を使う。
  - n2: pptx の `shapes_of` が、深さ 256 でパスを約 256 倍に増幅する。

## 解消状況

| 指摘 | 状況 | 根拠 |
|---|---|---|
| **M1** [Major] 深さ 1024 で再帰的な Clone/Debug/再描画が 2 MiB の大半を使う | **解消** | `DEFAULT_MAX_DEPTH = 256`（`xml/mod.rs`）。実測値は下の表のとおりで、debug でも最大 431 KiB（clone）。`tests/depth_limit.rs::recursive_operations_at_the_limit_fit_a_small_stack` は 10 操作を 1 MiB スレッドで実行し、debug と release の両方で pass した。既定値を 1024 に戻すと `thread 'clone + drop' has overflowed its stack` で abort した（再現済み）。`cargo test --workspace` はすべて green。 |
| m1 `xml_part` でメタデータが利用者の上限を使っていた | **解消** | `ensure_parsed` の `is_opc_metadata` で `max(options.max_depth, DEFAULT)` を使う（`package.rs:203-206`）。open 時の読み込みと `relationships()` は従来どおり既定値で parse する。`xml_part_mut` も同じ `ensure_parsed` を通る。テストは `metadata_parts_never_use_a_lowered_limit` で、`max_depth = 1` でも CT と `.rels` を parse できる。下限は既定値なので、上限をこの方向に上げてもスタックが危険になることはない。 |
| m2 lens の挿入で上限を超えられた | **解消** | `XmlDocument.max_depth` を `LensMut` に渡し、`insert_at_slot` の先頭で `path.len() + 2 > max_depth` なら `TooDeep` を返す（`lens.rs:384-389`）。この検査は `slots()`・`place()`・`mark_path` より前にあるので、**Err のときに木は変更されない**（テストは `assert!(!d.is_dirty())` で確認している）。要素を作る経路は `lens.rs:398` の `new_created` だけで、`insert`、`insert_by`、`insert_in_slot`、`get_or_insert` はすべてここを通る。部分木ごと差し込む API はない。`&mut RawNode` を公開する API もないので、`LensMut::root`（既定値 256）を経由した回避もできない。プローブの `rawlens` で、clone した `RawNode` に深さ 256 で挿入すると `TooDeep` になることを確認した。 |
| m3 上限を上げる設定に警告がない | **解消** | `ParseOptions::max_depth` の doc、`with_max_depth`、README に警告がある。「1 段あたり約 1.7 KiB」は実測の 431 KiB / 256 = 1.68 KiB と合う。format crate にオプションを渡す口がない点は、round 1 で許容した。 |
| m4 pptx が `TooDeep` を握りつぶす（範囲外） | **持ち越し（合意済み）** | `ooxml-pptx/src/presentation.rs:121-127` は変更されていない。PR 本文の表で pptx 担当者に引き継いでいる。pptx の PR で fail closed にすること。 |
| m5 テストの穴 | **解消** | `open_with_reads_from_a_path` を追加した。10k/40k の対応は PR 本文に書かれている。上限を上げるテストは 8 MiB スレッドで実行している。 |

### 深さ 256（既定値）で各操作が成功した最小スタック（`probe16r2 <op> 256 <KiB>`、8 KiB 刻み）

| 操作 | debug | release |
|---|---|---|
| `XmlDocument::clone()` / `RawNode::clone()` | **≈431 KiB** | ≈143 KiB |
| pretty Debug `{:#?}` | ≈359 KiB | ≈263 KiB |
| Debug `{:?}` | ≈279 KiB | ≈175 KiB |
| `force_rerender` + serialize | ≈175 KiB | ≈111 KiB |
| lens_mut の remove → dirty serialize | ≈167 KiB | ≈111 KiB |
| `structurally_eq` | ≈127 KiB | ≈15 KiB |
| `Drop` | ≈79 KiB | ≈31 KiB |
| parse（反復）/ `Lens` の Debug（path のみ） | ≈39 KiB | ≈15 KiB |
| pptx `slide()` + `shapes()` + `slide_mut()`（入れ子の `p:grpSp` 253 段。`read::walk` と `refs::descendants` の再帰） | ≤128 KiB（64 KiB では overflow） | — |

スレッドの種類ごとの評価:

- 2 MiB（Rust の spawn と `cargo test` の既定、多くの async ワーカー）では、debug でも 4.7 倍の余裕がある。
- 8 MiB（Linux のメインスレッド）では問題にならない。
- 1 MiB（Windows のメインスレッド）では、debug の clone で約 600 KiB が残る。

### 再帰する操作の洗い出し（テストで網羅されているか）

自分自身を呼ぶ関数を、`crates/**/src` の全体から機械的に抽出した。
- **ooxml-core**: `structurally_eq`、`force_rerender`、`write_node`、それに derive の `Clone`/`Debug` と `Drop`。これ以外は委譲の false positive だった。parse は明示的なスタックによる反復で、`node_at`・`declarations` は path を fold する反復である。
- **ooxml-schema の生成コード**（`generated/{dml,pml,sml}.rs`）: 再帰はない。`raw()`、`text()`、`set_text()` は lens への委譲だけである。
- **ooxml-pptx**: `read::walk`（グループ）と `refs::descendants`。上の表のとおり ≤128 KiB で、core のテストには含まれない。
- **ooxml-xlsx**（このブランチ上）: 再帰はない。Phase1 の `workbook.xml` の clone は、core の clone テストで押さえられている。

core の再帰はすべて restricted-stack テストに含まれている。`PartialEq` は `RawNode` に実装されていない。`RawMarkup` の `PartialEq` は再帰しない。

## 新しい指摘

### n1 [Minor] pretty Debug（`{:#?}`）の時間が O(N·d²) で、深さ 256 では入力の数千倍の出力になる

**根拠**（`probe16r2 pretty <depth> <leaves>`、release ビルド）

| 入力 | 入力サイズ | `{:?}` の出力 | `{:#?}` の出力 | 時間 |
|---|---|---|---|---|
| 深さ 64・葉 1 | 823 B | 19 KB | 883 KB | 0.10 s |
| 深さ 256・葉 1 | 3.3 KB | 78 KB | **13.9 MB** | **6.4 s** |
| 深さ 255・葉 1,000 | 7.3 KB | 287 KB | **75 MB** | **48.6 s** |
| 深さ 255・葉 10,000 | 約 43 KB | — | — | 120 s で打ち切り |

- 原因は derive の pretty Debug にある。1 段ごとに `PadAdapter` が入れ子になり、出力の 1 バイトが d 段のアダプタを通る。そのため、時間は「深さに比例する出力量 × 深さ」になる。非 pretty の Debug は線形である。
- スタックは上限で抑えられている（359 KiB）。ただし、README が「the limit is what bounds … `Debug`」と書いているのは、スタックについてだけ正しい。
- crate 内（`*/src`）に `{:#?}` や `dbg!` で木を出力するコードはない。発火するのは、利用者が信頼できない文書を pretty Debug したときだけである（ログ、`dbg!`、テスト失敗時の出力）。このため Minor とした。
- 提案:
  - `RawNode` と `XmlDocument` の `Debug` を手書きにし、子要素は件数だけ、または先頭の数段だけを出す。round 1 の M1(b) 案と同じである。
  - 少なくとも README で「Debug output is O(nodes × depth)」と注意を書く。

### n2 [Minor・format crate、範囲外の観察] 深さ 256 でも、pptx のパスによる番地付けは 1 ノードあたり最大 256 語に増幅する

- `read::walk` は、各 shape に `ShapePath(path.clone())` を持たせる（`ooxml-pptx/src/read.rs:421-431`）。
- 実測（`probe16pptx`、release）:
  - 入れ子の `p:grpSp` 252 段の中に `<p:sp/>` を 20 万個入れた slide（zip は 19 KB、XML は 1.4 MB）で、`shapes()` と `slide_mut()` のピーク RSS は **459 MB** だった。
  - 同じ数の shape を深さ 1 に置くと 67 MB だった。
  - 線形に外挿すると、XML 14 MB（zip で 200 KB 程度）で 4.5 GB 前後になる。
- この PR が N1 の「無制限の増幅」を「最大 256 倍の線形」に抑えたことは確かである。上限の効果としては正しい。
- ただし README の「Without a limit, a few kilobytes … cost gigabytes」は、上限があればギガバイト級にはならない、と読める。実際には、数十 KB の zip で数百 MB になる。
- 直すのは pptx 側である（パスの接頭辞を共有する、`shapes()` を遅延にする、など）。この PR では、README の言い回しを「bounded to O(nodes × limit)」程度にしておけば十分である。

### n3 [nit] restricted-stack テストは、既定値を 512 に上げる回帰を検出しない

- 既定値を 512 にして `recursive_operations_at_the_limit_fit_a_small_stack` を回すと pass した（debug、56 s）。debug の clone は 1 段あたり約 1.7 KiB なので、1 MiB スレッドで abort するのは既定値がおよそ 600 を超えてからである。
- 「既定値の根拠をテストで固定する」という目的には足りている。ただ、doc の「half the default thread stack」という余裕を厳密に守らせたいなら、`STACK_KIB` を 512 にする（現状の 431 KiB に対して約 1.2 倍の余裕）。または、`const _: () = assert!(DEFAULT_MAX_DEPTH <= 256);` を置いて意図を明示する。
- ASan やカバレッジ計測のビルドではフレームが数倍に膨らむので、1 MiB で落ちる可能性がある。CI でそれらを回す予定がなければ、現状のままでよい。

### n4 [観察] 256 で拒否される実在の公開ファイルは、POI の深さストレス fixture 1 件だけ

- 走査した範囲:
  - `reference/**`（480 ファイル）
  - `scratchpad/corpus/files/`（223 ファイル）
  - SDK、umya、calamine、docx-rust（pandoc）のテストファイル
  - 過去の各レビューで作ったパッケージ（ホスト上の .docx、.xlsx、.pptx、.xlsm、.dotx、.potx、.xltx など 15,893 ファイル）

  内容のハッシュで重複を除くと 4,730 パッケージ、xml/rels/vml は 92,602 part だった。
- 深さの分布: ≤8 が 80,707、≤16 が 11,723、≤24 が 161、≤32 が 3、33 が 3、34〜128 は 0、>128 は 5。最大の実ファイルは引き続き 33（POI `bug59058` などの `webSettings.xml`）。
- `TooDeep` になったのは次の 5 件だけで、`Package::xml_part` の経路でも同じ結果だった。
  - `d2500`、`d10000`、`deep5000`、`deep40000`（いずれも N1 の再現用に合成したもの）
  - **Apache POI の `test-data/document/deep-table-cell.docx`**（深さ 15,005、5,000 段の入れ子の表）

  POI のファイルは StackOverflow の回帰テスト用のストレスファイルで、実際の文書とは考えにくい。また 1024 でも拒否されるので、256 に下げたことによる回帰ではない。
- 実在しうる docx の入れ子の表は 1 段あたり 3 要素なので、256 で約 80 段まで入る。実用上の拒否はないと判断する。
- 提案（任意）: ooxml-docx（#13）の fixture/テストで、このファイルが `TooDeep` 由来の open-time refusal（O5）になることを固定するとよい。

## 判定

**converged: yes。**

M1 は、既定値 256 と restricted-stack テストで解消した。実測で debug 431 KiB と 1 MiB の比は 2.4 倍で、1024 に戻すと abort することも再現した。m1・m2・m3・m5 も解消し、m4 は範囲外として合意のうえ持ち越した。

上限以下でスタックオーバーフローに至る経路は見つからなかった。Err を返したのに木が変更されている経路もない。実ファイルの拒否もない。新しい指摘はいずれも Minor 以下で、マージを止めない。

- n1（pretty Debug が O(N·d²)）は、Debug を手書きにする後続 PR か、README への注記で対応すればよい。
- n2（pptx のパスの増幅）は pptx 側の課題である。
