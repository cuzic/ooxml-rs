# PR #16 レビュー（Opus, round 1）

対象: `core/depth-limit`（2f840f4）。`ParseOptions`、`DEFAULT_MAX_DEPTH = 1024`、`Error::TooDeep`、`XmlDocument::parse_with`、`Package::{from_bytes_with, open_with, parse_options}`、README、ADR-0002 の post-acceptance note、`tests/depth_limit.rs`。
プローブ: scratchpad `review-core-14-16/core-depth-limit/crates/ooxml-core/examples/probe16.rs`。操作ごとに、スタックサイズを指定したスレッドで実行し、深さ 1024 で必要な最小スタックを測った。あわせてコーパスの走査と、lens で深い挿入をするケースを試した。rustc 1.99.1、debug と release の両方で測った。

## 結論（先に）

**未収束（Major 1 件）。** パーサでの上限の検査は正しい。検査は `open` の先頭（`parse.rs:213`）にあり、ノードを確保する前に `stack.len()` だけで判定する。1M 段の入力も即座に拒否される。ラップしているのは `XmlDocument::parse` と `Package::ensure_parsed` の 2 経路で、どちらも上限を通る。手元のコーパス（72 パッケージ・1,641 part と、manual-check の 33 パッケージ・829 part）はすべて parse でき、最大の深さは 33 だった。#14 と機械的にマージでき、衝突はない（`git merge-tree`）。

ただし、既定値 1024 の根拠である「再帰的な走査は 2 MiB のスタックから十分に遠い」は、`Clone` と `Debug` については **debug ビルドで成り立たない**。深さ 1024 の `XmlDocument::clone()` は約 1.7 MiB のスタックを使う。そして ooxml-xlsx Phase1 は、編集のたびに `workbook.xml` を clone する（`impl-xlsx-phase1` `edit.rs:169`）。2 MiB のスレッド（`cargo test` の既定）で数百 KiB を使った状態から呼ばれると、捕捉できない abort がまだ起こりうる（M1）。

## 指摘

### M1 [Major] 深さ 1024 でも、再帰的な `Clone`/`Debug`/再描画が 2 MiB スタックの大半を使う。「far from the 2 MiB stack」は実測と合わない

**根拠**
- 主張している箇所: `xml/mod.rs:285-292`（「keeping every recursive walk … far from the 2 MiB default thread stack」）、ADR-0002 note L99（「stay far from the default stack」）、PR 本文「keeps recursive walks well inside a 2 MiB thread stack」。
- 実測（`probe16 <op> 1024 <KiB>`。入力は深さ 1024 の `<a k="v">` の連鎖。表の値は、その操作が成功した最小のスレッドスタック）:

| 操作（深さ 1024） | debug | release |
|---|---|---|
| `XmlDocument::clone()`（derive の再帰） | **≈1.7 MiB**（1.6 MiB では abort） | ≈768 KiB |
| `format!("{:?}", doc)`（derive Debug） | ≈1.28 MiB | ≈768 KiB |
| `force_rerender` + `serialize`（dirty 経路の `write_node`） | ≈640 KiB | ≈512 KiB |
| lens で最深部に挿入 → `serialize` | ≈640 KiB | ≈512 KiB |
| `structurally_eq` | ≈640 KiB | ≈96 KiB |
| `Drop` | ≤640 KiB（256 KiB で可） | ≈128 KiB |
| 編集なしの `serialize`（span をそのままコピー） | ≤128 KiB | — |

  参考: debug ビルドの clone は、深さ 512 で ≈896 KiB、深さ 256 で ≈512 KiB だった。
- 実際に通る経路: ADR-0003 rev 3 §3.1 の staged clone は `self.pkg.xml_part(&self.workbook_part)?.clone()`（`impl-xlsx-phase1/crates/ooxml-xlsx/src/edit.rs:169`）である。敵対的な `workbook.xml` は、未知の要素（`extLst` 配下など）を深さ 1024 まで入れ子にするだけで上限内に収まる。このとき debug ビルドでは、2 MiB のうち余裕は約 300 KiB しかない。format crate のフレーム、テストハーネス、async ランタイムのワーカー（多くは 2 MiB）が使う分によっては、`thread has overflowed its stack` で abort する。これは N1 が取り除こうとした失敗モードそのものである。
- 既定値の議論: 実ファイルの最大は 33 である。512 でも 15 倍、256 でも 7 倍の余裕がある。N1 の提案も「512 か 1,024」だった。

**提案（どれか、または組み合わせ）**
- (a) 既定値を 256 か 512 に下げる。docx で入れ子の表を正当に使う場合も、1 段あたり約 3 要素なので、256 でも 80 段の表になる。
- (b) `RawNode` の `Clone`、`Drop`、`write_node` の dirty 経路を、明示的なスタックによる反復に書き換える。`Debug` は、子要素を省略する手書きの実装にする。
- どちらの場合も、「深さ == 上限の木で、すべての再帰走査（clone、Debug、force_rerender + serialize、structurally_eq、drop）を、debug ビルドで 1 MiB に制限したスレッドで実行する」テストを足し、既定値の根拠をテストとして固定する。今の `default_limit_is_inclusive` が試しているのは、編集なしの serialize だけである。この経路は再帰しない。

### m1 [Minor] 「OPC メタデータは常に既定値で parse する」が `xml_part` では成り立たない

- doc（`package.rs:101-103`）と README は「`[Content_Types].xml` と `.rels` は常に既定値」と書いている。成り立つのは、open 時のキャッシュ（`ContentTypes::parse`、`Relationships::parse`、`Package::relationships`）だけである。
- `ensure_parsed`（`package.rs:193`）は、どの part にもパッケージのオプションを使う。`probe16 relsopt` で `max_depth = 1` にすると、`xml_part("_rels/.rels")` と `xml_part("[Content_Types].xml")` は `TooDeep { limit: 1 }` になる。
- #14 とマージした後は、`remove_part`（`.rels` を `xml_part` で parse する）が、小さい `max_depth` のパッケージで `RemovePartRefused` ではなく `TooDeep` を返す。
- 提案: `ensure_parsed` で、レジストリ part（CT と `.rels`）には `max(既定値, 指定値)` を使う。または doc を実装に合わせる。

### m2 [Minor] lens による編集で上限を超える木を作れ、その出力を自分で開き直せない

- `probe16 deepinsert 1024`: 深さ 1024 の最深部に `insert::<A>()` すると成功する。`serialize` は 13,311 バイトを出力するが、それを `XmlDocument::parse` にかけると `TooDeep { limit: 1024, position: 9211 }` になる。
- 挿入を繰り返せば深さに上限はない。そのため、`write_node` の再帰（M1）も上限で守られていない。
- 提案: `LensMut::insert*` で `path.len() + 2 > DEFAULT_MAX_DEPTH`（または、木が parse されたときの上限）なら `Err(TooDeep)` にする。少なくとも、「上限は parse 時だけ」と README に書く。

### m3 [Minor] 上限を上げる方向の設定に警告がない

- `with_max_depth(usize::MAX)` で上限は事実上なくなる。テストも 4000 を受け入れる。
- doc に「上限を上げると、再帰走査のスタック使用量が比例して増える（debug ビルドの clone で 1 段あたり約 1.6 KiB）。大きなスタックのスレッドで使うこと」と書くとよい。
- format crate（`Workbook::open`、`Presentation::open`）には、オプションを渡す口がない。ADR-0004 が Phase0 で不要としているなら、それで構わない。

### m4 [Minor・範囲外の観察] pptx は `TooDeep` を握りつぶし、参照の保護が黙って外れる

- `ooxml-pptx/src/presentation.rs:121-127` は、modern comments の part を `XmlDocument::parse(&b).ok()` で読む。パッケージのオプションも無視している。
- 1024 段を超える comments part は `None` になり、`scan_comments(None)` で、参照されているテキストの編集拒否（ADR-0005 §6）が無効になる。parse 失敗を握りつぶすのは以前からのパターンだが、#16 で失敗の新しい原因が加わった。pptx 側で fail closed にするべきである（この PR で直す必要はない）。

### m5 [Minor] テストの穴

- M1 の、深さ == 上限での再帰走査を試験していない。
- `open_with` を通るテストがない（`from_bytes_with` だけを試験している）。
- PR 本文は 10k/40k の再現を挙げている。core の 10k、100k、1M は試験しているが、docx 形の 40k（`w:tbl/w:tr/w:tc` の入れ子）は ooxml-docx 側のテストに任されている。core では `<a>` の連鎖で十分なので問題はない。ただ、PR 本文の「10k/40k」との対応を一言書くとよい。

## 確認できた点（指摘なし）

- 上限は、要素を確保する前に検査している。`stack` の長さはたかだか `max_depth` なので、メモリは O(上限 × ノードの大きさ) に収まる。テキスト、コメント、PI は深さに数えない。root は深さ 1 で、境界は inclusive（既存テストと読解で確認）。
- `Error::TooDeep` は ooxml-xlsx と ooxml-pptx に `Core(#[from])` で型付きのまま伝わる。`ParseOptions` は `#[non_exhaustive]` で、将来フィールドを足しても互換性は壊れない。
- 既存の fixture に回帰はない。`reference/**`（POI、SDK、calamine、LibreOffice、python-pptx、round_trip_corpus）の .xml/.rels/.vml 1,641 part は、すべて既定値で parse できた。最大は 33（`poi-bug59058.docx:word/webSettings.xml`）。manual-check の 829 part も最大 24。
- 編集なしの serialize は、深さ 1024 でもスタックを 128 KiB 未満しか使わない（span をそのままコピーするだけ）。release の `Drop` は 128 KiB。
- `xlsx/scan.rs` と #14 の `refers_to_rel_id` は、イベントを反復で読むだけで木を作らないので、上限の対象外で問題ない。

## 判定

**converged: no**。Blocker はない。Major は M1 の 1 件である。既定値を下げるか、`Clone`/`Debug`/dirty serialize を反復にし、深さ == 上限での制限スタックのテストを足せば解消する。
