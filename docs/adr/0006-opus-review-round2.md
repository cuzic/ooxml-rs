# ADR-0006 レビュー（Opus, round 2）

対象: PR #22 `plan/xlsx-phase2`（cf0d08c）の `docs/adr/0006-xlsx-phase2-shared-strings-and-styles.md`（Proposed rev 2）、`spikes/xlsx-phase2/README.md`、`docs/roadmap/xlsx.md` の差分。
証拠: draft PR #23 のプロトタイプ（`spike/xlsx-phase2-proto` 3ff565c、`spikes/xlsx-phase2/proto/`）。
参照: ADR-0003、round 1 レビュー、rust_xlsxwriter 0.102.0（cargo registry のソース）、Microsoft "Excel specifications and limits"。
プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr22-r2/`（`scripts/probe.py`、`probe.txt`、`fx.txt`、`poi.txt`、`out64420/`）。

## 結論（先に）

rev 2 で round 1 の Major 5 件はすべて設計として解消されました。プロトタイプの結果も再現しました（下記）。骨格（追記専用、二つの seed mode、ボトムアップの exact-form 再利用、既定スタイルからの index 継承、パッケージ全体での numFmtId 採番）は健全です。

新たに Blocker と Major はありません。Minor が 11 件あり、そのうち次の 4 件は実装前に本文を直すべきものです。

- **n1** `sst/@count` の再計算が worksheet だけを数えている。マクロシートの `t="s"` を落とし、Excel が書いた正しい `count` を壊す。プロトタイプで再現した（`64420.xlsm`）。
- **n2** §5.3 の「既存のカスタム numFmt は再利用しない」が §2 の numFmt exact-form 再利用と矛盾する。§5.3 の文言どおりに実装すると、M2（セッションをまたいだ増加）が numFmt と xf について再発する。
- **n3** `set_named_style` の xf を「スタイル名ごとに」重複排除すると、`apply*="0"` の群でセルから写した id が無視される。
- **n4** 予約位置（`fills` の 0/1）の既存エントリが exact-form 再利用の対象になる。POI の 10 ファイルでは `fills[1]` が `lightGray`/`darkGray` で、正規形とバイト一致しうる。

**判定: Accepted after Minor only — yes**（下記の Minor を直すことが条件）。

## 再現

- プロトタイプを `CARGO_TARGET_DIR=…/review-pr22-r2/target cargo build --release --offline` でビルドして実行した。
  - fixtures: OK 21 / REFUSED 3 / SKIP 2。
  - POI 366: OK 330 / REFUSED 5 / SKIP 31。
  - failures 0、`numFmts` 挿入 228、M1 の注記（既定フォントが Calibri 11 以外）121、M2 の 329/329。いずれも ADR の表と一致した。
- 独立プローブ `scripts/probe.py` を、POI と fixtures に対して実行した。結果は `probe.txt`。
- 注意: プロトタイプの検査 (d) の `count` は、本体と同じ worksheet だけの数え方で計算している。このため検査がトートロジーになっており、n1 を検出できない。

## 解消状況

| ID | 状況 | 根拠 |
|---|---|---|
| M1 既定が Calibri 11 | **解消**（残課題 n5） | §3 Inherit、§5.5。プロトタイプでは、継承した xf を calamine で読むと既定フォントになった（329/329、うち 121 は Calibri 11 以外）。ただし `set_bold()` などの builder は、編集モードでも Calibri 11 を起点にする（n5）。 |
| M2 セッションをまたいだ増加 | **設計上は解消**（n2、n4、n10） | §2 のボトムアップの exact-form 再利用、オラクル 6 の reopen、§5.10。プロトタイプでは、2 回目のセッションの font/fill/border がバイト一致し、numFmt/xf は id だけが違った（329/329）。再利用そのものはプロトタイプに実装されていない。§5.3 の矛盾（n2）を直さないと numFmt が毎回増える。 |
| M3 numFmtId の衝突 | **解消**（残課題 n6） | §5.3 は「全 XML パートの全 `numFmtId`」を対象にし、オラクル 8a がある。プローブでは、`numFmtId` を持つパートは `styles.xml`（344 件。うち 1 件は `xl\styles.xml` という名前）、pivotCacheDefinition（10）、pivotTable（9）だった。worksheet の x14 CF にはコーパス上の実例がない。走査の漏れは n6。 |
| M4 AC の計数 | **解消**（精度の nit、n9） | §2 の index-stable の定義、追記の全面拒否、オラクル 6 の修正。`style-alternate-content.xlsx` の AC は fonts 6、cellXfs 7、dxfs 11 で、cellStyleXfs には 0（プローブで確認）。 |
| M5 書式だけの書き込みによる無効化 | **解消**（残課題 n7） | §7.3 は、セルの数式または `definedName` に `CELL`/`INFO`/`GET.CELL`/`GET.FORMULA` があれば、値の書き込みと同じに扱う。ADR-0003 では `CELL`/`INFO`/UDF/`_xll.` はもともと `Everything` なので、その点は整合する。UDF は対象外のまま（n7）。 |
| m1 表の欠落、名前空間 | **解消**（n8 と衝突） | `UnsupportedStylesPart`、`cellStyleXfs` がなければ `xfId` を省く、表を挿入する、予約エントリを先に追加する。`47889` の 4 表と `fixture_minimal` の gray125 で再現した。 |
| m2 ぶら下がり参照 | **解消**（n8 と衝突） | §2 Dangling references、オラクル 3。 |
| m3 named style の `apply*="0"` | **解消**（新しいバグ n3） | §5.6。`apply*="0"` の群はセルの現在の id を保つ。 |
| m4 日付の範囲 | **解消** | 1904 系の上限 serial 2957003（= 2958465 − 1462）は正しい。時刻だけの値と serial 0 を許可し、serial 60 は `NonexistentDate`。 |
| m5 CR の射影 | **解消** | オラクル 4 に、calamine に見えない項目と indexedColors の除外を明記した。 |
| m6 sst の count | **一部解消**（n1、n11） | `set_string_inline` の新設、`set_number` では registry を有効にしないこと、オラクル 1 の T2 は明記された。ただし count の数え方が worksheet だけになっている（n1）。「再計算」と「差分」の記述が混ざっている（n11）。 |
| m7 接頭辞 | **解消** | §5.11。 |
| m8 `count` 属性がない場合 | **解消** | §5.2。 |
| m9 Eq/Hash | **解消** | `Tint`/`FontSize` の正規化と bit 比較。Code と Builtin は id で一つにまとまる。 |
| m10 spike README | **解消** | README の 49 行目と 73–74 行目。 |

## 新しい指摘

### Blocker / Major

なし。

### Minor

**n1. `sst/@count` の再計算でマクロシートとダイアログシートが漏れる（§4.5）**

§4.5 は「`t="s"` cells in all worksheets」としています。しかし ADR-0003 の census は「every worksheet, macro-sheet and dialog-sheet part」を走査します（ADR-0003 111 行）。プロトタイプは worksheet のリレーションだけを数えています（`main.rs` 508 行、`REL_WS`）。

- 再現（`64420.xlsm`、Microsoft Excel 作成）:
  - 入力は `count="3"`。worksheet 2 枚に 1 件ずつ、`xl/macrosheets/sheet1.xml` に 1 件あり、入力の count は正しい。
  - プロトタイプはこれを「input sst/@count wrong」と報告した。
  - 1 件追記した出力は `count="3"` になった。実際の参照は 4 件（`out64420/`）。
- つまり「すでに間違っている 7% を直す」はずの再計算が、正しい値を壊します。§4.5 の「exact after every append, in all 241」も、同じ数え方での自己検査です。
- 同じ漏れは次の箇所にも及びます。
  - §2 のぶら下がり検査（マクロシートのセルの `s` は `cellXfs` を参照する）。
  - オラクル 3（「every sheet」）。
  - §7.3 の書式関数の走査（XLM の `GET.CELL` は、まさにマクロシートに置かれる）。
- 修正案:
  - 「worksheets」を「Phase1 census が走査する全シートパート（worksheet、macrosheet、dialogsheet）」に改める。
  - プロトタイプの検査 (d) を本体と独立の数え方にする。
  - `64420.xlsm` を回帰に加える。
  - §7.3 の結果として、マクロシートの数式が `Everything` なら、書式だけの書き込みは ADR-0003 の `Err(MacroSheetDependent)` になる。これも Consequences に書いておく。

**n2. numFmt の再利用で §2 と §5.3 が矛盾する**

- §2: `numFmt` は「id ≥ 164 で `formatCode` が正規形と等しく、ほかの属性や子を持たない既存エントリ」を再利用する。
- §5.3: 「An existing custom `numFmt` with the same code is not reused … This is the edit-mode rule again.」
- §5.3 の文言どおりに実装すると、前のセッションで追記した numFmt も再利用されません。すると、numFmt が毎セッション 1 件増え、それを参照する xf も新しくなります。§5.10 の 200 件のガードにちょうど約 200 回で当たり、M2 が再発します。
- §2 が意図した読み方であることは、オラクル 6（reopen で増えない）からも明らかです。
- 修正案:
  - §5.3 の箇条を「既存エントリの再利用は §2 の exact-form 条件を満たすものに限る」に置き換える。
  - Context fact 3 の「16 files define built-in codes as customs」は、built-in の id へ写さない理由として残す。
- 補足: §2 の numFmt の条件は、実質的には code による値の一致です。安全である理由（同じ code なら表示は同じ、id ≥ 164 なら built-in の再定義と干渉しない、重複 code のどれを選んでも同じ）を一文書いておくとよいです。ミュータント「dedup against existing entries by value」の境界も明確になります。

**n3. `set_named_style` の重複排除キーが誤っている（§5.6）**

「These `xf` entries are deduplicated per style name among new entries」となっています。しかし rev 2 では、`apply*="0"` の群はセルの現在の id を写します（m3 の修正）。そのため、同じスタイル名でも結果の xf はセルごとに違います。

- 例: 表示形式の違う 2 セルに `set_named_style("Good")` を適用する。2 個目のセルが 1 個目の xf を受け取り、表示形式が変わってしまう。
- 修正案: 重複排除は、組み立てた xf のバイト（§2 の exact-form と同じキー）で行う。

**n4. 予約位置のエントリが exact-form 再利用の対象になる（§2、§5.4）**

§5.4 は「index 0 … and `fills` index 1 have a fixed meaning」と書いていますが、§2 の再利用は位置を区別しません。

- プローブ: `fills[1]` が `lightGray` のファイルが 3 件（64450, 64750, 66365）、`darkGray` が 7 件、`solid` が 1 件（52348）あった。
- rust_xlsxwriter の正規形では、色なしのパターン塗りは `<fill><patternFill patternType="lightGray"/></fill>`（`styles.rs` の `write_default_fill`）になります。`64450.xlsx` の `fills[1]` とバイト一致します。
- この場合、`lightGray` の塗りを求めた新しい Format は `fillId="1"` を受け取ります。Excel が fill 1 を常に gray125 として扱うかどうかは、ここでは未検証です（広く報告されてはいる）。ただ、ADR 自身が「固定の意味を持つ位置」と言っている以上、そこを再利用先にしないのが一貫します。
- 修正案: `fills` の 0/1 は、それぞれ正規形の `none`/`gray125` と一致する場合だけ再利用先にする。§5.4 最後の箇条（fill 0 の特例）はこの規則に含める。
- `fonts[0]`、`borders[0]`、`cellXfs[0]` の再利用は、意味が同じなので問題ありません。

**n5. 編集モードの builder が Calibri 11 から始まる（§3、M1 の残課題）**

§3: 「in edit mode … `set_bold()` therefore starts from `Font::default()` (rust_xlsxwriter's)」。

- 一番よくある書式（見出しを太字にする）が、Arial 10 のブック（POI では 44 件）で Calibri 11 の太字になります。M1 が問題にした見た目の差は、font 群を触る書式についてはそのまま残ります。
- alignment 群も同じです。プローブでは `cellStyleXfs[0]` に `alignment` を持つファイルが 41 件あり、日本語の Excel が書く `vertical="center"` が典型です。`set_align(Center)` を呼ぶと、vertical は bottom に戻ります。
- `with_default_font_of` という逃げ道はあり、仕様としても明記されています。そのため Minor とします。ただし既定の挙動として罠になるので、少なくとも次のどちらかを求めます。
  - (a) API ドキュメントとオラクル 10(f) に「編集モードで font 群を設定すると、ブックの既定フォントではなく Calibri 11 が起点になる」と明記し、例を `with_default_font_of` で示す。
  - (b) Q4 の preserving setter と同じ発想で、font と alignment に「部分指定」を持たせる。適用時に既定スタイルの font エントリのバイトを写し、`b` などの子だけを差し替える。バイトの複製なので、既存エントリの解釈には当たらない。
  - Phase2 の範囲では (a) で十分です。(b) は Phase4 の候補です。

**n6. numFmtId の走査の漏れ（§5.3）**

「A byte scan for the attribute is enough for a maximum, because an over-estimate is harmless」は正しいです。ただし、過小評価になる経路が次の 4 つあります。

- 引用符と空白: XML では `numFmtId = '165'` も正しい表記です。プロトタイプは `numFmtId="` だけを探している（`main.rs` 429 行）。コーパスに実例はない。
- 符号化: OPC の XML パートは UTF-16 でもよいです。プロトタイプは UTF-8 として読めないパートを黙って飛ばす。
- パートの選び方: プロトタイプは名前が `.xml` で終わるパートだけを見る。ADR の「any XML part」は content type（`…+xml`、`application/xml`）で判定すると明記する。コーパスには `xl\styles.xml` のような名前もあった。
- 上限: `numFmtId` は `xsd:unsignedInt` で、最大値 + 1 は桁あふれする。Excel が受け付ける上限（XLSB の `BrtFmt` の ifmt は 2 バイト）は未検証。
- 修正案:
  - 走査は属性の字句規則どおりに行う（`numFmtId`、空白、`=`、空白、`"` または `'`）。
  - UTF-8 として読めない XML パートは、UTF-16 として解読する。それもできなければ `Err` にする（飛ばさない）。
  - 採番の結果が 65535 を超えるなら `Err` にする。
- チャートの `c:numFmt` は `formatCode` と `sourceLinked` だけで id を持たず、slicer や timeline のキャッシュにも numFmtId はないので、範囲として困るものはありません。ただし、`sourceLinked="1"` のグラフはセルの表示形式を表示に使います。書式だけの書き込みがグラフの見た目を変えうることは、ノートとして書いておくとよいです。

**n7. UDF が書式だけの書き込みの対象から漏れる（§7.3）**

ADR-0003 の原則（「not proven safe ⇒ depends on everything」）では、VBA の UDF（`testNames.xlsm` の `myFunc`）と `_xll.` は `Everything` です。一方 §7.3 の起動条件は 4 つの関数名に限られています。色を集計する UDF（`=SumByColor(…)`）は、書式だけの書き込みでは古い値のまま残ります。

- 補足: Excel 自身も、書式を変えただけでは UDF や `CELL` を再計算しません。したがって実害は Excel と同等です。ただ、ADR が自分で掲げる原則とは食い違います。
- 修正案（どちらか）:
  - 起動条件を「ADR-0003 の許可リストにない関数（UDF、`_xll.`、未知の関数）または上記 4 つ」に広げる。
  - 「Excel と同じく UDF は対象外」と例外を明文化する。
- nit: `CELL` の info type の列挙に `"protect"`（保護は書式の一部）がない。規則は `CELL` 全体に掛かるので実害はないが、列挙を直す。

**n8. 予約エントリの追加とぶら下がり検査が衝突する（§2、§5.4）**

§2 は「参照 index ≥ 表の長さなら追記を拒否」とします。§5.4 は「表が空なら予約エントリを先に追記」とします。

- 例: `cellXfs` がなく `s="0"` のセルがある場合。または `fonts` がなく `fontId="0"` の xf がある場合。
- このとき前者の規則では追記できず、後者の規則では予約エントリがその参照に意味を与えます。`47889` のセルには `s` がないので、コーパスには実例がない。
- 修正案:
  - 「予約エントリ（index 0、fills の 1）に向いた参照は、予約エントリの追加で解消されるものとみなす。読み手がすでに想定している既定の意味を与えるだけだから」と明記する。
  - ぶら下がり検査は、予約エントリを追加した後の長さで行う。

**n9. index-stable の定義の精度（§2）**

次を明記するとよいです。

- 「nothing else」は「ほかの要素を持たない」の意味である。空白、コメント、PI は構わない。
- 「entry type」は QName で一致させる（`x:font` と `font` は同じ。`x14:font` は別）。
- 表の子に、エントリでも index-stable な AC でもない要素（無視可能な拡張要素など）があれば、index-stable ではないとする。
- AC が `styleSheet` の直下にあって表そのものを包んでいる場合は、「表がない」とはみなさない。挿入（m1）と既定スタイルの参照を拒否する。コーパスに実例はない。
- 既定スタイル（`cellStyleXfs[0]`）の読み取り（§3）と、`xfId="0"` の書き込みも index に依存する操作である。`cellStyleXfs` が index-stable でなければ拒否する一覧に加える。

**n10. Excel の上限の対応づけが推測に頼っている（§5.10）**

Microsoft の表（"Worksheet and workbook specifications and limits"）の行は次のとおりです。

- 「Unique cell formats/cell styles: 65,490」
- 「Fill styles: 256」
- 「Line weight and styles: 256」
- 「Unique font types: 1,024 global fonts available for use; 512 per workbook」
- 「Number formats in a workbook: Between 200 and 250, depending on the language version」

ADR はこれを表の長さに一対一で写していますが、次の点は根拠がありません。

- 65,490 は「cell formats/cell styles」なので、`cellXfs` だけでなく `cellStyleXfs` を合わせた数、または重複を除いた数とも読めます（`53568.xlsx` には `cellStyleXfs` が 3089 件ある）。
- 「font types」は font エントリではなく、フォント名の種類を指すとも読めます。
- 「Line weight and styles」が `borders` の件数を指すかどうかは不明です。

コーパスの最大は、fonts 41、fills 36、borders 74、cellXfs 482、numFmts 12 で、どの上限にも遠く、検証材料になりません。

- 修正案:
  - 65,490 は `cellXfs + cellStyleXfs` に対するガードにする（保守側）。
  - fonts、fills、borders、numFmts の値は「推定」と明記し、XL 項目（例: 300 fills、600 fonts、260 numFmts のファイルを Excel で開く）を追加する。
  - 結果が出るまでは、定数を差し替えやすい形（`const` の表）で持つ。

**n11. count の「再計算」と「差分」の記述が混ざっている（§4.1、§4.5、ミュータント一覧）**

§4.5 は「recomputed」としつつ、「The registry then adjusts the number by each write's delta」とも書いています。§4.1 は、registry を有効にしない `set_number` では count が 1 ずれると言います。この 3 つを合わせると、次が決まっていません。

- registry が有効になる前の Phase0/1 の書き込み（`set_number` で `t="s"` を上書きする、Phase1 の `set_formula` でセルを数式にする）の差分を、有効化の後に反映するのか。
- 修正案:
  - 「census は、セッション中のすべてのセル書き込み（Phase0/1/2）で `t="s"` の数を保つ。registry が有効なら、保存時にその値を書く」と一本化する。
  - ミュータント「count delta instead of recompute」は、「保存されていた count に差分を足す」と明確に言い換える。

### Nit

- §5.4 最後の箇条「A `Format` whose fill is `None`」は rev 1 の名残です。rev 2 では `None` は継承を意味します。`Some(Fill::NoFill)` に直す（n4 で消えるなら不要）。
- §3「child (`alignment`, `protection`) **by index**」は不正確です。子要素に index はなく、バイトの複製です。
- 既定スタイルの選び方: `Format::new()` は `cellStyleXfs[0]`（Normal）と同じ見た目になり、`s` のないセル（`cellXfs[0]`）と同じとは限りません。プローブでは fontId が違うファイルが 3 件（`bug66675` では `cellXfs[0]` が空の `<font/>` を指す）、alignment が違うファイルが 10 件ありました。どちらを意図しているかを一文書く（Normal で妥当）。オラクル 4 の「Inherited groups must read like the default style's」も同じ語を使う。
- `dxfs` の registry（§5.8）には「継承」が当てはまりません。dxf では省略が「上書きしない」を意味し、id もないためです。Phase4 で誤って既定スタイルの id を写さないよう、一文書いておく。
- exact-form の `HashMap<bytes, index>` は表ごとに作る（`dxf/font` と `fonts/font` は省略の意味が違う）こと、重複したバイトがあれば最小の index を選ぶことを明記する。
- 2 回目のセッションの比較（ADR の Prototype results）は「ids aside」です。再利用の実装そのものは検証していない、と一言添える。

## 指示された観点への回答（まとめ）

- **既定スタイルの index の複製と `cellStyleXfs`/`xfId`/`apply*`:**
  - 健全。複製した id は Normal と同じなので、`apply*` がなくても Excel（cell xf だけを見る）と LibreOffice（cell xf では used フラグが常に真）の解釈が一致する。
  - `xfId` を省いた場合（`cellStyleXfs` なし）の参照元は `cellXfs[0]` で、これも一貫している。
  - 問題は builder の起点（n5）と、Normal と `cellXfs[0]` の違いの明記（nit）。
- **バイト一致で意味が違う場合:**
  - 同じ表、同じ接頭辞の文脈なら、テーマ色や indexed もブック内では同じ意味になる。テーマが変わっても両方が同じように追従する。
  - dxf と cellXfs は表が別なので、表ごとの map なら衝突しない（nit）。
  - 唯一の実例は予約位置（n4）。
- **numFmtId の走査範囲:** 実在するのは styles（xf、dxf、x14 dxf）、pivotCacheDefinition（`cacheField`）、pivotTable（`pivotField`/`dataField`）、worksheet の x14 CF（コーパスに実例なし）。チャートは formatCode だけ。走査の漏れは n6。
- **index-stable:** n9。
- **StyleTableFull:** n10。
- **CELL/INFO/GET.CELL/GET.FORMULA:** ADR-0003 とは `Everything` 経由で整合する。UDF が漏れる（n7）。マクロシートは census 漏れ（n1）と `MacroSheetDependent`。
- **予約エントリ:**
  - fonts 0、borders 0、cellXfs 0、fills 0/1 は扱われている。
  - `cellStyleXfs` 0 と `cellStyles` の Normal は、Phase2 では作らず `xfId` を省くので不要。ただしプローブでは、`cellStyleXfs` はあるが `cellStyles` がないファイルが 8 件、Normal のない `cellStyles` が 3 件あった。その場合 `xfId="0"` は「Normal でないスタイル」につながる。§5.5 はすでにこれを許容している。
  - ぶら下がり検査との衝突は n8。
- **count の再計算と Phase0/1:** n1、n11。

## Q1–Q6（proposed default の妥当性）

- **Q1（exact-form 再利用）:** 妥当。n2（§5.3 の矛盾）、n3、n4、表ごとの map を直すことが条件。
- **Q2（テンプレートの sst）:** 方針は可だが、根拠の記述が誤り。
  - rust_xlsxwriter 0.102.0 は、文字列が 1 件以上あるときだけ sst を書く（`workbook.rs` 3381 行 `has_sst_table = …count > 0`、`packager.rs` 185 行と 392 行）。「matches rust_xlsxwriter's default」は誤りで、round 1 の私の記述も誤っていた。
  - 文字列のない書式マトリクスでは、RX オラクル 5 の部品一覧が食い違うので、許容差として列挙する。
  - XL 10(e) の結果しだいで、Phase3 の部品削除（保存時に空の sst を落とす）も選択肢になる。
- **Q3（named style + 上書き）:** Phase4 に先送りで妥当。
- **Q4（preserving setter）:** Phase2 で妥当。
  - 複製したバイトは `pivotButton`、`quotePrefix` などを持ち越すが、同じセルへの導出なので意味は保たれる。
  - `set_font(Font)` は font 群を丸ごと置き換えるので、n5 と同じく Calibri 11 が起点になる。`set_bold` のような部分的な setter は Phase4 の候補として記録しておく。
- **Q5（`knownFonts`）:** 触らない方針で妥当。
- **Q6（ふりがな）:** 受け入れて文書化する方針で妥当。

## 判定

**Accepted after Minor only: yes.** n1–n11 はすべて本文の修正（規則の追加・明確化、オラクルと回帰ファイルの追加）で済み、骨格は変わりません。n1 と n2 は、実装者が本文どおりに書くと誤った出力になる箇所なので、マージ前に直すことを強く勧めます。

## 出典

- Microsoft, Excel specifications and limits（"Worksheet and workbook specifications and limits" の表）: <https://support.microsoft.com/en-us/office/excel-specifications-and-limits-1672b34d-7043-467e-8e27-269d656771c3>
- ADR-0003 111 行（census の対象パート）、399 行（`MacroSheetDependent`）、180–203 行（UDF/`CELL`/`INFO` → `Everything`）
- rust_xlsxwriter 0.102.0: `src/workbook.rs` 3381 行、`src/packager.rs` 185・392 行、`src/styles.rs` `write_fill`/`write_default_fill`
- プロトタイプ `spikes/xlsx-phase2/proto/src/main.rs`: 136 行（`count_s_refs`）、421–439 行（`max_numfmt_id_in_package`）、508 行（`all_sheets` = `REL_WS` のみ）
- Apache POI test-data @ `47dcff76`: 64420.xlsm, 64450.xlsx, 52348.xlsx, bug66675.xlsx, FormulaEvalTestData_Copy.xlsx, no_drawing_patriarch.xlsx, 53568.xlsx, 47889.xlsx
