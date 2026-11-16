# ADR-0006 レビュー（Opus, round 1）

対象: PR #22 `plan/xlsx-phase2`（de4cf68）— `docs/adr/0006-xlsx-phase2-shared-strings-and-styles.md`（Proposed rev 1）、`spikes/xlsx-phase2/`、`docs/roadmap/xlsx.md` の差分。
参照: ADR-0001/0002/0003、`crates/ooxml-xlsx/`（main と impl/xlsx-phase1）、rust_xlsxwriter 0.102.0 / calamine 0.38.0（cargo registry のソース）、LibreOffice `sc/source/filter/oox/stylesbuffer.cxx`（master）。
プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr22/`（`scripts/probe.py`, `scripts/detail.py`, `*_summary.md`, `probe_*.txt`）。

## 結論（先に）

追記専用（append-only）と二つの seed mode という骨格は健全です。in-range の既存参照（`c/@s`、`row/@s`、`col/@style`、`t="s"`、`fontId` など、`cellStyle/@xfId`、`tableStyleElement/@dxfId`）は、末尾への追加ではずれません。スパイクの数値は、フィクスチャでも POI でも完全に再現しました。

ただし、次の 5 点は Major です。

- **M3** 新しい numFmt id の採番が、`numFmts` の外で使われている id と衝突する。
- **M4** `mc:AlternateContent` についての事実認識と計数規則が誤っており、オラクル 6 がそのままでは通らない。
- **M1** 編集モードで、`Format::default()` の基準が、ブックの標準スタイルではなく rust_xlsxwriter の Calibri 11 になっている。
- **M2** セッションをまたいでエントリが際限なく増え、Excel の上限に達する。
- **M5** 書式だけの書き込みによる無効化を、数式の本文だけで判定している。定義名の中の `CELL` を見逃し、Phase1 の「既定で健全」が崩れる。

いずれも修正は局所的で、骨格を作り直す必要はありません。

**判定: Accepted after Minor only — no**（Major 5 件）。

## 再現

- `python3 -I spikes/xlsx-phase2/registry_survey.py --summary reference/fixtures/xlsx reference/round_trip_corpus` を実行した。README の Fixtures 列と全行一致した（24 packages、46/338、7/24 と 13 groups など）。
- POI の 366 ファイルで `--summary` を実行した（著者がダウンロードした `scratchpad/p2/poi` を使用）。342 packages、859/5686、75/340 と 388 groups（263/115/10）、18/247、21 種類の tint と 8 個など、README の POI 列と全行一致した。
- 独立確認として、`47dcff76` の raw URL から 8 ファイル（52348, 61605, style-alternate-content, ConditionalFormattingSamples, ExcelPivotTableSample, LIBRE_OFFICE-128382-0, 47889, sample-beta）を新たに取得した。SHA-256 は著者のコピーと一致した。
- 数値は再現する。ただし、README と ADR の**解釈**のうち 2 点は誤っている（M4、m10）。

## Findings

### Blocker

なし。

### Major

**M3. 新しいカスタム numFmt id が、`numFmts` 以外で使われている id と衝突する（§5.3）**

§5.3 は採番を `max(163, largest numFmtId in the file, largest id handed out) + 1` としています。「file」が `numFmts` 表だけを指すなら、既存の参照と衝突します。

- 根拠 1: `ConditionalFormattingSamples.xlsx`（Excel 作成）。
  - `numFmts` の最大 id は 173。
  - `dxfs/dxf/numFmt` は id 174–177 を、別の formatCode で使っている。
  - 最初の新規カスタムは 174 になり、dxf の 174 と別の書式コードで衝突する。
  - Excel 自身は dxf の id を `numFmts` と重ならないように採番しています。つまり id 空間は共有です。
  - POI 全体では、dxf または ext の numFmt id が `numFmts` の最大を超えるファイルが 3 件ありました（ConditionalFormattingSamples, craftonhills…, dataValidationTableRange）。
- 根拠 2: `LIBRE_OFFICE-128382-0.xlsx`。
  - `numFmts` は 160 と 161 のみ。
  - `pivotTable1.xml` と `pivotCacheDefinition1.xml` は `numFmtId="165"` を参照しているが、`styles.xml` には定義がない。
  - 新規の採番は 164, 165, … となり、2 個目で、ピボットのぶら下がり参照が当方の書式に意味を変える。これは既存参照の意味変更です。
- 修正案:
  - 「largest」の範囲を明記する。対象は次のとおり。
    - `styles.xml` 内の全 `numFmtId` 属性（`numFmts`、`cellStyleXfs`、`cellXfs`、`dxfs`、`extLst` 配下の x14 `dxfs` を含む）。
    - `pivotTable*.xml` と `pivotCacheDefinition*.xml` の `numFmtId`。
    - ワークシート `extLst` 内の `x14:cfRule//x14:dxf/numFmt`。
  - ミュータント「`numFmts` の id だけで max を取る」と、上記 2 ファイルの回帰テストを追加する。

**M4. `mc:AlternateContent` の事実誤認と、「1 子 = 1 エントリ」規則の不健全さ（§2「Counting entries」、オラクル 6、spike README）**

- 根拠（`style-alternate-content.xlsx`）:
  - `fonts` に AC が 6 個、`cellXfs` に 7 個（xf 22 + AC 7 = count 29）、`dxfs` に 11 個ある。
  - ADR の「only its `fonts`」は誤りです。
  - スパイクのフィクスチャ表で `cellXfs count attr wrong 1` と出ているのは、この AC のためです。README も fonts だけを注記しています。
- 帰結 1（実装不能なオラクル）: オラクル 6 は「fill-only format は成功」としています。しかし fill だけの新しい Format でも `cellXfs` に `xf` を 1 個追加します。§2 の規則ではこれは `Err(UnresolvedSibling)` になります。期待値が矛盾しています。
- 帰結 2（不健全な一般規則）: 「MCE selects exactly one branch」は正しいのですが、選ばれた branch（`Choice` または `Fallback`）の子は 0 個でも複数個でもかまいません。また `Fallback` は省略できます。したがって「AC 子 1 個 = エントリ 1 個」は、このファイルの形（全 AC が `(Choice 1 子, Fallback 1 子)`）でしか成り立ちません。この計数は次の箇所で使われます。
  - `uniqueCount`（§4.5）
  - `set_style_index` の範囲チェック（§5.6）
  - §4.3 の exact-form 再利用で返す index。§2 が拒否するのは追記だけで、再利用は拒否しません。sst に AC があれば、AC より後ろの `si` を再利用したときに返す index が reader 次第で変わります。
- 修正案:
  - AC を 1 エントリと数えるのは、全 branch がちょうど 1 個のエントリ型の子を持ち、`Fallback` が存在する場合に限る。それ以外の表では、index に依存する操作（追記、再利用、範囲チェック、count の再計算）をすべて拒否する。
  - オラクル 6 の期待値を「このファイルでは font・fill・border を問わず新しい Format はすべて `Err(UnresolvedSibling)`。`set_style_index(n)` は `n < 29` で成功」に改める。fill だけが伸びる例には別のフィクスチャを使う。

**M1. 編集モードの「既定値」がブックの標準ではなく Calibri 11 になる（§3、§5.4–5.6）**

`Format` の既定値は rust_xlsxwriter と同じく Calibri 11, scheme minor, theme 1 です。編集モードでは既存の font を参照しない（§2）ので、`Format::new().set_num_format("0.00")` を新しいセルに書くだけで、Calibri 11 の font が追記され、そのセルはその font で表示されます。

- 根拠 1: POI 340 件のうち 135 件（40%）で `fonts[0]` が Calibri 11 ではない。
  - Arial 10 が 47 件、Verdana 10 が 19 件、Arial 11 が 13 件、Calibri 12 が 11 件、宋体 11 が 6 件、ＭＳ Ｐゴシック 11 が 5 件など。
  - 少なくともサイズの違い（10 pt と 11 pt）は確実に見えます。
- 根拠 2: Excel はセルの書式を cell xf だけで決めます。
  - MS-OE376 の注記 "In Office, only the cell xf record defines the formatting applied to a cell."（LibreOffice `stylesbuffer.cxx` の `Xf::importXf` のコメントに引用されている）。
  - LibreOffice も、`fontId` が 0 より大きいか、または親スタイルの font と異なれば cell xf の font を使う（同ファイル 2176 行、2361 行）。
  - したがって両 reader でセルの見た目が変わります。
- §5.4 は fill 0 だけを exact-form で特例扱いしています。font と border には同じ扱いがありません。既定 border も毎回追記されます。
- これは API の値モデルの選択です。出荷後に互換を保って直すのは難しいので Major とします。
- 修正案:
  - `Format` の各グループ（font, fill, border, numFmt, alignment, protection）に「未指定 = 継承」を持たせる（例: `Option`）。
  - 編集モードの未指定グループは、`cellStyleXfs[0]`（`Normal`、無ければ `cellXfs[0]`）の `fontId`/`fillId`/`borderId` を **index のまま参照**する。
  - これは既存エントリを解釈しない（照合ではなく継承）ので Layer 3 とも矛盾しません。新規作成モードでは、テンプレートの既定がそのまま同じ値になります。
  - これで Q4 の大半も解決します（推奨の Q4 を参照）。

**M2. セッションをまたいだ際限のない増加が Excel の上限に当たる（§2、Consequences）**

開くたびに既存エントリはすべて opaque になります。そのため、前のセッションで自分が追記したエントリとも重複をまとめられません。同じ書式を書く日次バッチなら、1 回ごとに font 1、fill（none 以外）、border 1、カスタム numFmt 1、xf 1 が増えます。Consequences の「Excel compacts on its own save」は、Excel を通らないサーバ側のパイプラインには当てはまりません。

- 根拠: Microsoft "Excel specifications and limits" の値。
  - Unique cell formats/cell styles: 65,490
  - Fill styles: 256
  - Line weight and styles: 256
  - Unique font types: 512 per workbook
  - Number formats: 200–250
  - fill と border はおよそ 250 回の実行で上限に達します（M1 の既定 border の追記があれば、どの書式書き込みでも増えます）。
- 修正案:
  - (a) exact-form 再利用をスタイルにも採用する（Q1 を yes に）。条件は、`xf` とその参照先の `font`/`fill`/`border`/`numFmt` がすべて当方の正規形とバイト一致し、`xfId` も一致すること。正規形は決定的なので、前のセッションで追記した連鎖は確実に一致します。逆に Excel が書いた `apply*` パターンとはほぼ一致しないので、「意味の違うものを黙ってまとめる」危険は §4.3 と同じく生じません。
  - (b) 上限ガードを置く。`Err(StyleTableFull { table, limit })` を返し、上記の値を定数で持つ（numFmt は 200 を保守値とする）。
  - (c) オラクルを追加する。「保存して開き直し、同じ Format を書いても何も増えない」。

**M5. 書式だけの書き込みによる無効化が、数式本文の字句だけで判定されている（§7.3）**

「a style-only write seeds invalidation of every formula whose text calls `CELL` or `INFO`」という規則は、次を見逃します。

- 定義名の定義が `CELL("format",…)` を含み、数式は名前だけを参照する場合。
- XLM の `GET.CELL` 系を定義名に置く場合。xlsm で色や書式を集計する定番の手法です。

この場合、キャッシュ値が古いまま残り、ADR-0003 §2 の「not proven safe ⇒ depends on everything」原則に反します（Phase1 の precedents は定義名の本体まで展開しています。`precedents.rs` の `name_def`）。

- 修正案（どちらでもよい）:
  - 定義名の本文も走査し、`CELL`/`INFO`/`GET.CELL`/`GET.FORMULA` 系を推移的に含む数式を種とする。
  - より単純に、ブック内のどこか（セル数式または `definedName`）にこれらがあれば、書式だけの書き込みを値の書き込みと同じに扱う。
- `INFO` は書式に依存しないので外してもよいですが、保守的に残しても害はありません。

### Minor

**m1. 表が欠けている、または名前空間が違う `styles.xml` が規定されていない**

- `47889.xlsx` は `<styleSheet …/>` が空で、表がひとつもない。
- `sample-beta.xlsx` は Excel 2007 beta の名前空間（`http://schemas.microsoft.com/office/excel/2006/2`）。
- `52348.xlsx` は `cellStyleXfs` がない。それでも §5.5 は新しい xf に `xfId="0"` を書き、ぶら下がり参照になる。しかもこのファイルはオラクル 6 の対象。`no_drawing_patriarch.xlsx` も既存の xfId がぶら下がっている。
- 修正案: 次のどれにするかを明記する。
  - 表がない、または名前空間が SML でない場合は `Err(UnsupportedStylesPart)`。
  - `cellStyleXfs` が空なら `xfId` を書かない。
  - 不足している表を ADR-0002 の挿入で作る。

**m2. ぶら下がり参照の一般規則がない**

範囲外の既存参照（`c/@s`、`fontId`、`t="s"` の値、未定義の `numFmtId`）は、追記によって新しいエントリを指すように変わります。セル側には実例がありません（プローブの検出は AC による見かけ上のもの）。それでも M3 のピボットのように実在するので、次を規定するとよいです。

- `max(参照 index) ≥ len` なら追記を拒否する（または単に記録する）。
- オラクル 3 に「入力でぶら下がっていた参照は、出力でもぶら下がったまま」を加える。

**m3. `set_named_style` の「as Excel does」は不正確**

Excel では、スタイル側の `apply*="0"`（例: Good/Bad/Neutral の `applyNumberFormat="0"`）は「このスタイルは表示形式を含まない」という意味です。スタイルを適用してもセルの表示形式は残ります。ADR の方式は、スタイルの `numFmtId` で上書きしてしまいます。

- 根拠: POI では Good/Bad/Neutral が `applyNumberFormat=0` のファイルが 6 件。
- また §5.6 はすでに既存エントリ（`cellStyleXfs`）から id と alignment をコピーして新しいエントリを導出しています。「Q4 が初めての導出になる」という記述と矛盾します。

**m4. 日付の範囲**

- 時刻だけの値（serial < 1）と、1899-12-31（serial 0。rust_xlsxwriter は許容）を明示的に許可する。
- 上限は serial ではなく日付（9999-12-31）で決める。1904 系の上限 serial は 2957003 なので、`≥ 2958466` では 1904 系の 10000 年以降を通してしまう。
- 1900 系の serial 60 の読み取り結果も規定する。

**m5. CR オラクルが表せる範囲**

calamine 0.38 の `Style` には次がありません。

- `vertAlign`、`scheme`、`charset`、`condense`/`extend`、`readingOrder`、`quotePrefix`
- theme と rgb の区別（ARGB に解決されてしまう）

また、`indexed` の解決は既定パレット（`style_parser.rs` 153 行）で、`indexedColors` の上書きを見ません（13 件）。「`Style` equals the `Format`」は calamine が持つ項目への射影であると明記し、パレットを上書きしたファイルは色比較から外すこと。

**m6. sst の count 維持の一貫性**

- §4.5 は「the census counts `t="s"`」と書いているが、impl/xlsx-phase1 の `census.rs` はまだ数えていない（Consequences 側の記述が正しい）。
- registry が有効になるのは `set_string` のときだけ。Phase0 の `set_number` で `t="s"` を上書きしたときに −1 するのか（registry を有効にするのか）が書かれていない。
- オラクル 1 の「伸びなかった registry 部品は L1」は、count を再計算する設計と矛盾する場合がある（再利用だけ、または上書きだけのとき）。
- `set_string` の既存の挙動（inlineStr）を変えることと、「`set_string_inline` remains」と書いているが現状その API がない点も明記すること。

**m7. 名前空間の接頭辞**

`style-alternate-content.xlsx` は `x:` 接頭辞を使っています。新しいノードは、親の表と同じ名前空間と接頭辞で作ることを明記する（ADR-0002 §4.3 の挿入が担うなら、そう参照する）。

**m8. 伸びた表に `count` 属性がない場合**

sst は「absent stays absent」と規定されていますが、`styles.xml` の表については規定がありません（1–2 件）。

**m9. `Format` の Eq/Hash**

- `Tint(f64)` の正規化（−0.0、NaN、範囲外）と、font size（f64）の等価性を規定する。
- `Code("0.00")` と `Builtin(2)` は Format としては別の値になるが、xf の段階で id によって重複がまとまることを明記しておくとよい。

**m10. spike README の解釈**

AC の注記（fonts だけ）と、フィクスチャの `cellXfs count wrong` の原因を、M4 に合わせて直す。数値そのものは正しい。

## 推奨

- **Q1（exact-form 再利用）:** sst は yes。**スタイルも yes** とし、正規形の連鎖全体がバイト一致する場合に限る（M2 の主対策）。AC を含む表では無効（M4）。補足: 日本語の Excel ファイルの sst は大半の項目が `phoneticPr` を持つので、sst 側の効果は小さい。
- **Q2（テンプレートの sst）:** yes（rust_xlsxwriter の既定と Excel 自身の挙動に合う）。ただし、空の `<sst count="0" uniqueCount="0"/>` を Excel が素直に開くかを XL 項目に加える。だめなら、テンプレートには最初の文字列を書いたときに作る形（Phase3 待ち）を検討する。
- **Q3（名前付きスタイル + 上書き）:** Phase4 に先送りする。ただし m3（`apply*="0"` のグループはセルの現在の値を保つ）は Phase2 で直す。
- **Q4（既存の見た目を保つ編集）:** Phase2 で入れる。M1 の「継承」モデルとあわせて、`set_format_preserving` を「既存 xf のバイトを複製し、変える属性と対応する `apply*` だけを置き換える」と定義すれば無損失で、§5.6 と同じ導出の扱いになる。
- **Q5（`knownFonts`）:** 現状維持（触らない）。XL 10(c) の結果が出るまで、行を挿入しても属性を除去しない。必要と分かったら、近くの行の `x14ac:dyDescent` を写す方を優先する（除去は他ツールの描画に影響しうる）。
- **Q6（ふりがな）:** Phase2 では受け入れる。docs に「新しい文字列には読みがない（ふりがなによる並べ替えで未設定扱い）」と明記する。
- その他:
  - Excel の上限（65,490 / fills 256 / borders 256 / fonts 512 / numFmts 約 200）をガードとして持つ（M2）。
  - sst の index は unsignedInt だが、実用上の上限はメモリが先に来るので規定は不要。

## 判定

**Accepted after Minor only: no.** M1–M5 を直した rev 2 で round 2 を行う。どの修正も追記専用の骨格は変えず、規則の追加・明確化とオラクルの修正で済む見込みです。

## 出典

- Microsoft, Excel specifications and limits: <https://support.microsoft.com/en-us/office/excel-specifications-and-limits-1672b34d-7043-467e-8e27-269d656771c3>
- LibreOffice `sc/source/filter/oox/stylesbuffer.cxx`（master。`Xf::importXf`、`Xf::createPattern`）: <https://github.com/LibreOffice/core/blob/master/sc/source/filter/oox/stylesbuffer.cxx>（MS-OE376 の注記を引用している）
- [MS-XLSB] 2.4.695 BrtKnownFonts: <https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-xlsb/346d5d60-0dd3-4fdb-bcdc-20fd7837ebd6>
- calamine 0.38.0 `src/style.rs`、`src/xlsx/style_parser.rs`。rust_xlsxwriter 0.102.0 `src/format.rs`（`Font::default` = Calibri / `FontScheme::Body`）、`src/datetime.rs`。
- Apache POI test-data @ `47dcff769bb395034dddfd59c4609659b6dbc6a4`（ConditionalFormattingSamples, LIBRE_OFFICE-128382-0, style-alternate-content, 52348, 47889, sample-beta, no_drawing_patriarch）。
