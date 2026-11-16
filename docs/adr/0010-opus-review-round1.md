# ADR-0010 レビュー（Opus, round 1）

対象: `plan/core-performance`（PR #32）@ `c172ef5`。`docs/adr/0010-core-performance.md`（Proposed）と `spikes/core-performance/`。

照合した相手:

- `crates/ooxml-core`（main 相当）
- 進行中のブランチ: #14 / core/add-part（staged な `RegistryPlan`）、#18 `core/detach-insert`、#19、#26 `core/insert-at`、#29 `perf/xlsx-row-index`

プローブは `scratchpad/review-24r2-32/adr10/spikes/core-performance/src/bin/probe.rs` です（spike のコピーに追加しました）。

## 結論（先に）

- **方向は妥当で、spike の正しさの主張は再現しました。** compact tree（interned name、span-backed `SStr`、u32 span、ちょうどの大きさの slice）と、遅延サブツリーを検証付き skip で作る方針のことです。
  - spike を実行し直しました。1,589 個の fixture part で、T1 がバイト一致、core と同じ木、全 re-render も一致しました。
  - アロケーション数（5.29 → 2.05 / element）と heap（1.44 GiB → 286 MiB）は完全に一致しました。
  - ADR-0001 の Layer 1 と Layer 2 の直列化規則、T1、T1b、T2、ADR-0002 の xml:space、prefix 規則、depth 256、`DtdNotAllowed` は、設計として保てます（下の「保証の確認」）。
  - core の公開アクセサのシグネチャも保てます。`RawNode` などのフィールドは、すでに全部 `pub(crate)` です。
- **ただし Major が 4 件あります。**
  - M1: 遅延ノードを、展開しないまま dirty にできる経路がある。
  - M2: 「検証はスキャンのごく一部のコスト」は実測で誤り。その上に立つ目標値も根拠がない。
  - M3: #29 の行インデックスは全行を展開するので、「そのまま合成できる」は誤り。
  - M4: `release_part` が core/add-part の `RegistryPlan` と衝突し、release ビルドで編集が黙って消える。
  - どれも ADR の文面を直せば済み、方針の変更は不要です。
- **「失敗する入力は、今日と同じエラーで失敗し続ける」は達成できます。** しかも速度の利得は残ります。
  - core の parser と同じ検査（`with_checks` 付きの属性、`normalized_value`、GeneralRef、DocType、要素と属性の prefix 束縛、深さ）を、木を作らずに行う skip を実装して測りました。1M セルで約 315 ms です。
  - spike の索引スキャン（検証なし）は 165 ms、compact の全 parse は約 700 ms、core の parse は 800〜880 ms でした。
  - 検証のコストはスキャンと同じくらいかかります（約 1.9 倍）。それでも全 parse より約 2.2 倍速く、メモリの利得はそのまま残ります。
  - 前提は、同じ parser ループの「構築しないモード」として実装することです。別に実装すると、同じエラーになることを保証しにくくなります。

## 保証の確認（ADR-0001 / ADR-0002）

| 保証 | 判定 | 根拠・条件 |
|---|---|---|
| Layer 1 passthrough | 保たれる | §4 のレベルは書き直す part だけに効く。§5 の `release_part` は clean な木だけを Raw に戻す。 |
| Layer 2：触っていない span をバイトどおり出す | 保たれる | 展開した子の span は絶対オフセット（§2）。spike の T2 チェックも通過（再実行で確認）。**M1 の穴を塞ぐことが条件。** |
| T1 / T1b / T2 | 保たれる | T1b は `force_rerender` が全部を先に展開する。M1 が条件。 |
| dirty の義務（ADR-0002） | 保たれる | 値は lens 経由でだけ書き換わり、owned の `SStr` に差し替わる。フラグの扱いは今日と同じ。 |
| prefix 規則 | 保たれる | lens が祖先の宣言を見る時点で、祖先は展開済み。遅延ノードは自分のスコープを記録する。 |
| xml:space | 保たれる | `xml_space_preserve()` はノードごとの属性で判定し、lens の text 書き込みは展開済みのノードに対して行う。 |
| depth 256 と lens 挿入の上限 | 保たれる | skip は絶対深さで数える（プローブでも 300 段は拒否）。展開時は、そのノードの深さを起点にする必要がある（m3）。 |
| `DtdNotAllowed` など parse 時の拒否 | 保たれる | skip で DocType と GeneralRef も検査する必要がある。プローブでは、重複属性、未束縛の要素 prefix・属性 prefix、未宣言の実体（本文・属性値）、終了タグの不一致を、core と同じく拒否した。 |
| 公開アクセサのシグネチャ | 保たれる | `RawAttr`/`RawText`/`RawMarkup`/`RawNode` のフィールドはすべて `pub(crate)`。#18 の `RawAttr { … }` 構築は core の内部。`ParseOptions` だけは注意（m3）。 |

## Findings

### M1（Major）展開していない遅延ノードが dirty になり得る。直列化すると中身が消える

- §2 は「展開されていない遅延ノードは決して dirty にならない」とし、その根拠を「`LensMut` が遅延ノードの**中に降りる**と展開する」に置いています。
- しかし、行そのものを `LensMut` の対象にして属性を変える経路では、中に降りません。たとえば `row/@ht`、`@spans`、#29 の行挿入まわりです。この場合 `own_dirty` が立ちます。
- 今日の直列化（`xml/write.rs` の `write_node`）は、`own_dirty` のノードを `render_open_tag(out, node, node.children.is_empty())` で描きます。
  - 展開していない遅延ノードは子が空に見えるので、`<row …/>` と書かれ、**セルが全部消えます**。
  - span は `Some` なので、自己閉じの判定（`s.close.is_none() && …`）でも救われません。
- core の内部は、フィールドを直接読む箇所が多くあります。これらは `children()` の展開を経由しません。
  - `write.rs`、`structurally_eq`、#18 の `detach_node`（`n.children` を直接走査）、`Detached::height()`
  - core/add-part の `apply`（`root.children.get_mut`、`e.attrs.iter_mut`）、`lens.rs` の挿入
- **修正案（どちらかを ADR に明記）。**
  - (a) 遅延ノードへの `&mut` アクセスはすべて先に展開する。`children` フィールドを private なアクセサ（`children_mut()` が展開する）の裏に隠し、stage 2 の受け入れ条件に「core 内に `.children` の直接アクセスがない」ことを加えます。
  - (b) 直列化で、遅延ノードの中身 `src[open_end..close.0]` をそのままコピーする。
  - (a) を推奨します。stage 4 の arena も同じアクセサ化を前提にするので、二度手間になりません。
  - 受け入れ条件の property test にも、「展開せずに遅延ノードの属性だけを変える」ケースを加えてください。

### M2（Major）検証付き skip のコストが過小評価されている。stage の目標値に根拠がない

- §2 は「検証はスキャンのごく一部」と書いています。実測（1M セル、load 11〜23、best of 7 を 2 ラウンド）は次のとおりです。

  | 処理 | 時間 |
  |---|---|
  | quick-xml イベントのみ | 142〜146 ms |
  | spike の索引（検証なし） | 164〜168 ms |
  | **検証付き skip + 行索引**（core と同じ検査、木なし） | **312〜320 ms** |
  | compact 全 parse | 698〜706 ms |
  | core `XmlDocument::parse` | 799〜879 ms |

- 検証で時間はほぼ倍になります。主に、属性の `with_checks` と `normalized_value`、それに prefix の解決です。ただし全 parse よりは約 2.2 倍速いので、主張そのもの（同じエラーで、速度の利得もある）は成り立ちます。
- **目標値（Targets）はこの数字と合いません。**
  - Stage 1 だけで open ≤ 0.7 s：spike を再実行すると、1M の parse の速さは core 比 1.1〜1.3× でした（844 → 662 ms。プローブでも 1.13〜1.26×）。ADR の「1.9×」は、core が 2.84 s かかった負荷の高い run の値です。
    - 今日の open（drop 除く）は 1.46〜1.95 s です。parse が 1.3× 速くなっても 0.7 s には届きません。
    - census（0.15〜0.29 s）と、breakdown の内訳に現れない残り約 0.36〜0.39 s（sharedStrings の parse などと推定）もあります。
  - Stage 2 で open ≤ 0.3 s：検証付き skip だけで約 0.31 s かかります。これに decompress、SST、遅延行ノードの構築（名前の intern と属性の `SStr`）が加わります。
  - ≤ 40 MiB：保持する source（`Arc<str>`）だけで 33 MiB あります。
    - 遅延行ノードは 1 行あたり Node 72 B、属性 slice、`OnceLock`、スコープの `Arc`、source と深さ（m3）で、100k 行なら 15〜25 MiB と見込まれます。
    - spike の 1.5 MiB は `(r, start, end)` の 12 B/行だけを数えた値で、ADR の「about 100 bytes」とも違います。
    - SST の木も加わります。
  - また、source が sheet の大きさに比例して残ります。Phase6 の文言「memory bounded by rows touched」は文字どおりには満たせません。そのことを明記してください。
- **修正案。**
  - skip は「同じ parser ループの、構築しないモード」として実装すると明記してください。検査の順序とエラー位置が構造上一致します。
  - 目標値は「同じ session の main 比」の相対値にするか、上の内訳に基づいて引き直してください。たとえば stage 1 は heap ≤ 1/4 と drop ≤ 1/3、stage 2+2x は open ≤ 0.6 s、heap ≤ 80 MiB、edit+save 1,000 ≤ 0.5 s が目安です。

### M3（Major）#29 の行インデックスは全行を展開する。「そのまま合成できる」は誤り

- §2 の「Row index」は、「索引は row ノードの `r` 属性から作られ、行を展開しないので両立する」としています。
- #29 の `crates/ooxml-xlsx/src/index.rs` `SheetIndex::build` は、全行について `row_cells(n, r)` を**その場で**計算します。
  - その中で `has_ac(row)` と `row.indexed_elements()` を呼び、各行の `children()` を読みます。
  - 遅延 `sheetData` の上では、open 時に全行を展開することになり、§2 の利得が丸ごと消えます。
- #29 の bulk cell iterator も全行を展開します。展開した行は `OnceLock` に残るので、全部読むと heap は今日の compact tree と同じ量に戻ります。
- **修正案。**
  - `RowEntry::cells` を行ごとの遅延計算（初回の `locate` 時）にする、と stage 2x の条件に書いてください。
    - `RowErr` は今日も行ごとに保持され、その行を locate したときに報告されます。遅延にしてもエラーの意味は変わりません。
    - `SheetErr`（行の `r` の欠落、不正、順序）は、行の属性だけで判定できます。
  - 全行の読み取りは、展開しない `content_span()` を quick-xml で走査するか、clean な行を元に戻す（`&mut` で `OnceLock` を `take`）か、どちらかを §2 で選んでください。
  - Excel 担当者と合意したことを、interaction 列に書いてください。

### M4（Major）`release_part` が staged な `RegistryPlan` と衝突し、編集が黙って消える

- core/add-part（作業中）の `RegistryPlan` は、計画時の part の状態に依存します。`edit_for` は、Parsed なら `install: None`（パッケージの木を使う）、Raw なら `install: Some(parse)` を記録します。
- `apply`（`package/add.rs` L948-）の扱い:
  - `generation` が同じなら、そのまま適用します。
  - `install: None` なのに part が Raw になっていると、`debug_assert!(false, "planned as parsed"); continue;` を通ります。release ビルドでは、**その registry 編集が黙って捨てられます**。
- `generation` を進めるのは、今日は「変更」と `xml_part_mut` だけです。§5 の `release_part`（Parsed{clean} → Raw）は「変更ではない」ように読めるので、このままでは generation を進めない実装になりやすいです。
- **修正案。** §5 に「`release_part` は `generation` を進める（計画は Stale になる）」と書いてください。`parse_part(&self)` は進めません。また、§6 の「part の状態遷移」に core/add-part の `Replaced` と `added` を含めてください。

### m1（Minor）テキスト中心の文書では compact の parse が一貫して遅い。ノイズではない

- ADR §1 と spike の README は、`word/document.xml` の 31 ms 対 39 ms を「ホストのノイズの範囲」としています。
- 再測定では、core 14.6〜15.0 ms に対して compact 19.5〜20.0 ms でした（best of 15 を 3 ラウンド、spike 本体の再実行でも 14.9 対 20.0）。**一貫して 30〜34% 遅い**です。
- Consequences の「every format crate benefits」と、推奨順の「compact tree は pptx・docx に効く」は、メモリについては正しい（2.7×）ですが、時間については誤りです。
- **修正案。** 原因を調べてください（parse ごとの interner の検索、テキストの `SStr` の作り方、要素ごとの scratch vector などが候補）。stage 1 の受け入れ条件には、「docx/pptx の最大 fixture で parse が main 比で悪化しない（同じ session、best of N）」を加えてください。

### m2（Minor）spike の速度比は 1 回の高負荷 run の値

- 「parse 1.1–1.9× faster」「parse plus drop 4.1 s → 1.7 s（2.4×）」は、core の 1M parse が 2.84 s かかった run の値です（baseline の breakdown では 1.15 s）。
- 再実行すると 1.1〜1.3× でした。drop の 3.3〜6.9× と allocation 数は再現します。
- 複数回の範囲で書き、「速度の主な利得は drop とメモリにある」と言い直すのが正確です。

### m3（Minor）`ParseOptions::defer` の型と配管

- `ParseOptions` は `Copy`、`PartialEq`、`Eq`、`#[non_exhaustive]` です。`defer: &[QName]` を足すと lifetime 引数が増え、`Package` のフィールドの型が壊れます。
  - `QName` は `&'static str` の組なので、`&'static [QName]` にすれば `Copy` も lifetime なしも保てます。
- `Package` は parse options を 1 つしか持ちません（`from_bytes_with`）。worksheet だけを defer する手段（part ごとの `xml_part_with` か、content type ごとの指定）が未定義です。
  - パッケージ全体で `sml:row` を指定すると、externalLink の `sheetData/row` も遅延になります。害はありませんが、明記してください。
- `RawNode::children(&self)` は文書にアクセスできません。そのため遅延ノード自身が、source の `Arc`、parse options、**自分の深さ**（展開時の深さ上限の起点）を持つ必要があります。M2 のノードの大きさの見積もりにも入れてください。

### m4（Minor）`parse_part` の意味が、Parsed・dirty・Replaced の part について未定義

- 「新しい木を返し、part は Raw のまま」とあるのは、Raw の part の場合だけです。
- 推奨: `parse_part(name)` は `read_part(name)` の現在の内容を parse したものと定義してください。dirty なら serialize した結果、Replaced ならそのバイト列です。
- `Package::parse_raw` と同じ深さの下限（metadata floor、`package.rs` L203-205）を共有することも明記してください。core/add-part の `parse_raw` を公開版にすれば、実装は 1 つで済みます。

### m5（Minor）圧縮レベル：適用範囲と実データでの差

- 適用範囲: core/add-part では、新しく追加した part が `CompressionMethod::Deflated`（既定のレベル）で書かれます。「新しく書く part」に、追加した part と Replaced の part を含めてください。
- `Level(i64)` が範囲外のときの扱い（エラーの型）を決めてください。
- 実データ: fixture 全体（1,590 part、7.96 MB）を part ごとに圧縮しました。
  - level 3 は 1.440 MB で、level 6 の 1.368 MB より **5.3% 大きく**、速さは 1.2 倍にとどまります（63 対 76 ms）。
  - 33 MiB の合成 sheet の「within 3%、5× faster」は、大きく単調な part での値です（再実行では 5.8×）。
- 実データの数字を併記してください。

### m6（Minor）interaction の表と pptx の read path

- interaction の表に、次の項目が抜けています。
  - **core/add-part**：`apply` が `a.value.clone_from(ct)`、`e.attrs.iter_mut()`、`root.children.get_mut` を直接使うので、stage 1b で機械的な修正が必要です。
  - **#26**：`insert_at` は `children.insert` を使います。
  - **#18**：`detach_node` と `height()` は展開を経由する必要があります（M1）。
- pptx の `Slide<'a>` は `&'a XmlDocument` を持ちます（`read.rs` L416）。所有する view に変えると、`pres.slide(i)?.shapes()` のような一時値の借用が使えなくなります。ooxml-pptx の公開 API の使い方が変わるので、ADR-0005 の担当者への申し送りに書いてください。
- layout、master、theme は、placeholder の継承のために繰り返し読まれます。非キャッシュの対象は slide だけにする、と書いてください。

### nit

- `XName(Arc<XNameData>)` の derive の `Debug` は出力の形が変わります。`Hash` は内容で計算すると明記してください（ポインタで計算すると、`Eq` の「内容でも比較する」と矛盾します）。
- Migration の stage 0 の「frozen copy of today's parser」は、`ooxml-core/tests` に置く写しです。生成物ではないので、CLAUDE.md の「生成物は手で直さない」とは衝突しません。ただし、置き場所（`tests/support/`）を書いておくと迷いません。

## Owner questions への推奨

1. **既定の圧縮レベル:** `Balanced`（level 3）を推奨します。
   - 書き直しが効くのは大きな part で、そこでは約 5〜6 倍速くなります。
   - 実データでのサイズ増は約 5% です（m5）。
   - 最大の圧縮が必要な利用者には `Smallest` があります。比較するテストは展開後の内容を見ているので（`tests/edit.rs` の `entries`）、壊れません。
2. **Stage 4（arena）:** 今は除外しないでください。stage 2 の数字を見てから決めます。M1 の修正案 (a)（`children` をアクセサの裏に隠す）を stage 1b で入れておけば、arena の導入は lens の内部だけで済みます。
3. **pptx の読み取りキャッシュ:** LRU は推奨しません。借用の寿命が複雑になり、サイズの調整も難しいからです。
   - `slide()` は今の署名のまま（キャッシュする）にしておきます。
   - そのうえで、`parse_part` を使う所有型の読み取り API（例: `Presentation::read_slide(&self, i) -> SlideDoc`）か、`release_slide(i)` を足してください。
   - layout と master はキャッシュしたままにします。

## 判定

- converged: **no**
- Accepted after Minor only: **no**
  - M1〜M4 は、すべて ADR の文面（不変条件、受け入れ条件、目標値、interaction）を直せば解消します。spike のコードの作り直しは不要です。
  - 次の round では、M1 の不変条件と M2 の目標値の引き直しを中心に確認します。
