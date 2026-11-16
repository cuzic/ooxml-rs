# ADR-0017 レビュー（Opus, round 1）

対象: PR #46 `docs/adr/0017-xlsx-phase5-objects.md`（Proposed, revision 1）+ `spikes/xlsx-phase5/`
（worktree `plan-xlsx-phase5`, HEAD `c41c38e`）。
参照: ADR-0003 rev 4（`impl-xlsx-phase1`）、ADR-0006、ADR-0009 rev 2（`plan-xlsx-phase3`）、
ADR-0014 rev 2（`plan-xlsx-phase4`）、PR #38 `RegistryPlan`（`core-add-part` `31906a9`）、
PR #26 `insert_at`/`check_insert_at`（`core-insert-at` `631fc90`）。
コーパス: POI `test-data/spreadsheet` @ `47dcff76` をスクラッチへ新規取得（366 ファイル）+
`reference/fixtures/xlsx/calamine`。スパイクを再実行し、ADR の表の数値（342 パッケージ、各カウンタ）を再現した。
ライブラリ: rust_xlsxwriter 0.102.0（crates.io）、openpyxl 3.1.5（PyPI sdist）、POI @ `47dcff76`
（`XSSFVMLDrawing`/`XSSFDrawing`/`XSSFSheet`/`XSSFTable`/`CommentsTable`）を取得し、本設計が依拠する引用を確認した
（確認できた行番号は下記の根拠欄に記す。引用はおおむね正確。不正確な 1 件は m9）。

## 結論（先に）

方針の骨格（id はスキャンした最大値 + 1 でセッション high-water を持つ、VML はパースして追記、
表の列名は ST_Xstring で書く、チャートのキャッシュ更新は明示呼び出し）は妥当で、コーパスの根拠も
しっかりしている。ただし次の 2 点が健全ではない。

1. **「小さなパートを `RegistryPlan` に stage し、apply 後にシートを in-place 編集する」方式の原子性は、
   操作がシートへの挿入 1 つで終わる場合にしか証明されていない。** 表作成と動的配列は、apply の後に
   Phase1/Phase2 のセル書き込みを多数行う。ところが Phase1 にはバッチ計画がなく
   （ADR-0003 §3.1「batch API は不要」）、calcChain の削除（`remove_part`）と `workbook.xml` の差し替えは
   1 つのトランザクションの外で起きる。そのため、通常の拒否（`vm` セル、共有数式の hand-off 失敗など）が
   apply の後に起こりうる（B1）。
2. **VML の shape 番号の割り当てが、`o:spid` と描画側の id を見ていない。** 本 ADR の 2 つの割り当て器は
   互いに衝突する。コーパスの `mv-calculator-final-2-20-2013.xlsm` に今の規則を当てると、既存の
   `_x0000_s1133` と重複する番号が出る（B2）。

ほかに Major が 4 件ある: 表削除で自己参照の構造化参照が残る（M1）、構造化参照の列名をエスケープしていない（M2）、
フォールバックノートの `xr:uid` に必要な名前空間と `mc:Ignorable` の規定がない（M3）、
表作成が既存セルを上書きするときの意味論が未定義（M4）。

**判定: no**（Minor の修正だけでは Accepted にできない）。

---

## Blocker

### B1. apply 後の in-place 編集に Phase1/Phase2 の多数セル書き込みが含まれ、「apply 後の失敗は構成上ありえない」が成立しない

**問題。** §2.1 は step 3 で「`Package::apply` → シートの in-place 編集 → Phase1/Phase2 の効果」と書き、
「A failure after apply is impossible by construction」としている。この証明が成り立つのは、
in-place の編集がすべて step 1 で `check_insert_*` によって検査済みの場合だけである。
しかし実際には次の操作が apply の後に走る。

- **表作成**（§6.1）: 見出しセル n 個、集計行のセル、計算列のセル（列数 × 行数）。いずれも Phase1 の
  `set_formula` と Phase2 の文字列経路を通る。
- **動的配列**（§9）: セル、Phase1 の calcChain 方針、`fullCalcOnLoad`。
- **見出し同期**（§6.3）: Phase0/1 の値書き込み（依存先の無効化）。

これらは step 1 で検査されていない。

**根拠。**

- ADR-0003 rev 4 §3.1 の plan → validate → apply は **1 回の編集** 単位で定義されている。apply の中で
  (i) `remove_part(calcChain)` と (ii) clone した `workbook.xml` の差し替えを行い、§3.1 末尾で
  「A batch API is therefore not needed for Phase1」と明言している。
  - k 番目のセルの計画は、1..k−1 番目のセルを書いた後の census に依存する。たとえば計算列のセル同士や、
    集計行の `SUBTOTAL(109,T[Col])` が計算列を参照するケースである。
  - したがって、表の全セルを事前にまとめて検証する手段が今は存在しない。
- Phase1 の計画段階の拒否は「バグ」ではなく **通常の `Err`** である。例: `RichValueCell`（`vm` セル）、
  `UnknownCellMetadata`（`cm`）、`SharedFormulaMaster`（共有マスタの hand-off で翻訳に失敗）、
  `AmbiguousName`、`UnresolvedSibling`（`calcPr` の隣に `mc:AlternateContent`。ADR-0003 は 78/333 の
  `workbook.xml` に AC があると記録している）。
  - §6.1 の `TableRange` は、範囲が `vm`/`cm` のセルや共有数式マスタを含む場合を挙げていない。
  - そのため、これらの拒否は表パートとリレーションシップが apply された **後** に起こる。
  - 結果は「半分できた表」か、ADR-0003 の poisoning のどちらかになる。poisoning ではセッションの編集が
    すべて失われる。poisoning は ADR-0003 では「構成上ありえないバグ」のための網であり、通常の拒否の
    受け皿ではない。
- calcChain の削除は Phase1 の apply が `Package::remove_part` で単独に行う（ADR-0003 §4.3）。
  これは本 ADR の「1 操作 = 1 つの `RegistryPlan`」（§2.1 の 2）の外にある。step 1 で走らせた
  `check_remove_part` の結果が、先行する `RegistryPlan` の apply（workbook `.rels` への追加など。
  例: `metadata.xml`）の後もなお有効であることは、どこでも示されていない。
- 順序の制約もある。PR #38 の `xml_part_mut` は generation を上げる（`package.rs` L377）。
  Phase1/Phase2 の検証が `xml_part_mut` を使う場合（`check_insert_at` は `LensMut` 上にしかない。
  `lens.rs` L463）、それを `RegistryPlan::new` の後に呼ぶと apply は必ず `StalePlan` になる。
  §2.1 はこの制約をシートについてしか書いていない。

**推奨。**

1. Phase5 の前提として、**Phase1 の複数セル書き込み計画**（例: `CellWritePlan`）を ADR-0003 への追記として
   定義する。対象のセル集合全体について §3.1 の全ガードと位置を 1 回で検証し、census の更新を計画の中で
   累積する。表作成、動的配列、見出し同期はこれを step 1 で作る。
2. calcChain の削除（`remove_parts(&[calcChain], &[REL_CALC_CHAIN])`）と `workbook.xml` の `calcPr` 変更
   （`stage_part`）を、**同じ `RegistryPlan`** に入れる（ADR-0009 §2 と同じ形）。こうすると apply 後に残るのは、
   sheetData・`tableParts`・sst に対する、検査済みの in-place 挿入だけになる。
3. §2.1 に操作ごとの「step 1 で検証するもの / plan に入るもの / apply 後に in-place で行うもの」の表を加える。
   あわせて「`xml_part_mut` を使う検証（Phase1/2 のものも含む）はすべて `RegistryPlan::new` の前に行う」と明記する。
4. 試験: 表作成と動的配列について、apply 後の各 in-place 段の直前に fault injection を入れる（ADR-0003 と同じ方式）。
   さらに「範囲内に `vm` セル / 共有数式マスタ / `cm` セルがある場合は、apply 前に `Err` となり全バイトが不変」
   であることを回帰試験にする。

### B2. VML の shape 番号が `o:spid` と描画の `cNvPr` を見ていない。既存ファイルと自分自身の割り当ての両方で番号が重複する

**問題。** §2.2 の VML shape の規則は「シートの既存 shape 番号の max + 1」だが、番号の出どころが
`id="_x0000_sN"` しか想定されていない。一方、`cNvPr` は「描画内の max と、同じシートの VML 番号」の
両方を見る **非対称** な規則になっている。

**根拠（コーパスでの probe 結果）。**

- 名前付きの shape（`id="CheckBox28"` など）は、番号を **`o:spid` にだけ** 持っている。
  - `45540_form_Footer.xlsx` / `45540_form_Header.xlsx`: 数値の `id` は 2138 の 1 つだけで、
    40 個の `o:spid` が 2079–2140 にある。
  - `mv-calculator-final-2-20-2013.xlsm`: `id` から得られる番号の最大は 1132 で、`o:spid` の最大は 1331。
    id だけを見ると次の番号は **1133** になる。これは既存の `o:spid="_x0000_s1133"`（`snf_per_diem`）と
    重複する。描画 `drawing1.xml` の `cNvPr id="1133"` とも重複する。
- POI は `id` 属性しか走査しない（`XSSFVMLDrawing.java` L161-172）。本 ADR はこれを「ids = max existing + 1」
  として参照しているが、それはこのバグも引き継ぐことを意味する。
- 描画の id と VML の id は **シートで 1 本のカウンタ** を共有している。`ExcelWithAttachments.xlsm` では
  描画の `cNvPr` が 1025–1031 で、VML が 1032–1038 と連番で続いている。
  - 本 ADR の規則では、ノートのあるシート（VML 1025）に画像を入れると `cNvPr` = 1026 になる。
  - 続けてノートを足すと、VML = 1 + max(VML) = 1026 となり、**自分の割り当て同士が衝突する**。
  - §2.2 の理由欄「we do not take a number the VML side uses」が、逆方向では守られていない。
- スパイクの「218 of 218」は `<v:shape … id="…_s(\d+)$">` だけを数えている（`objects_survey.py` L310-318）。
  `o:spid` も含めると番号は 466 あり、ブロック外は `right-to-left.xlsx` の `_x0000_s-1`（負数）2 件だけだった。
  LibreOffice は `id="_x0000_s1026 "` のように末尾に空白を付けることもある。
- ブロックの走査（新しい VML パートに 1 + max を割り当てる）は、**ブック内の全 VML** を読む必要がある。
  §4.2 では XML として読めない VML（`BrNotClosed.xlsx`）は編集を拒否するだけで、**他のシート** の
  ブロック走査にそのパートをどう含めるかが決まっていない。

**推奨。**

- **シートごとに 1 つの shape 番号割り当て器** を置き、`cNvPr` と VML shape の両方に使う。
  - 値 = 1 + max(描画の全 `cNvPr`（group と AC の全分岐を含む）、VML の `id` と `o:spid` から取れる番号、
    シートの `control/@shapeId` と `oleObject/@shapeId`、セッション high-water)。
  - VML 番号は、そのシートのブロック範囲に入るように調整する。
  - 解析できない番号（負数や末尾空白）は無視する。ただし既存の文字列とは一致させない。
- ブロック走査では、XML として読めない VML について `o:idmap/@data` だけをバイト列から読む。
  それもできない場合は、新しいブロックの割り当てを `Err(PartNotEditable)` にする。
- id-space checker（試験オラクル 2）に「シート内の `cNvPr` ∪ VML 番号（`o:spid` を含む）は一意」を加える。
  コーパス一巡（オラクル 3）では `mv-calculator…xlsm` と `45540_form_*.xlsx` を明示的なケースにする。
- スパイクを修正する（`o:spid`、`v:shape` 以外の VML 要素、`data="1,2"` のブロック単位の重複判定）。
  そのうえで Context の数値を更新する。
- 配送順への影響: この割り当て器は P5a（画像）で必要になる。P5c ではなく P5a に入れる。

---

## Major

### M1. 表の削除で、表自身の構造化参照が宙に浮く

§6.4 は「他から使われていなければ削除し、セルは普通のセルとして残す」としている。しかし表の内側のセルは
自分自身を参照している。

- 集計行は `SUBTOTAL(109,Tabelle1[Quantity])`（`bug66215.xlsx`、`DataTableCities.xlsx`）。
- 計算列は `Tabelle1[[#This Row],[Quantity]]*…` や `Tabelle1[[#Totals],[Total]]`（`bug66215.xlsx`）。

ADR-0009 の `TableInUse` 検査は「どの数式位置にでも現れたら拒否」なので、そのまま使うと集計行や計算列を
持つ表は **常に削除できない**。逆に自己参照を除外すると、壊れた参照が残る。
Excel の「範囲に変換」は、これらを A1 参照に書き換えている。

さらに、`tableType="queryTable"` の表（13 個）は自分の `.rels`（queryTable パート）を持つ。
これを削除の閉集合に入れるのか、拒否するのかが書かれていない。

**推奨。** 次のどちらかを選び、仕様に書く。

- (a) 自分の範囲内に自己参照が 1 つでもあれば `Err(TableInUse)`。
- (b) Excel の変換形に書き換える（これは XL 項目）。

rev 2 では (a) を推奨する。あわせて「`tableType` が `worksheet`（既定）以外の表、または自分の `.rels` を
持つ表は削除を拒否する」と明記する。

### M2. 生成する構造化参照で列名をエスケープしていない

§6.1 は集計を `SUBTOTAL(10x,Table[Col])`、計算列を `Table[[#This Row],[Col]]` と書くだけで、列名の
エスケープ規則がない。

- RX は列名中の `'`・`#`・`[`・`]` の前に `'` を付けている（`table.rs` L2021-2026）。
- コーパスの LibreOffice 出力には、前後に空白を持つ列名を二重括弧で書いた例がある:
  `Таблица1[[Sum for Q1 ]]`、`Таблица1[[ Sum for Q2 ]]`（`LIBRE_OFFICE-128382-0.xlsx`）。
  末尾空白の例は `63934.xlsx` の `[Gender ]` にもある。
- エスケープしないと、`Q1 [est]` のような列名から不正な数式ができ、Excel の修復を招く。

見出し同期の `ColumnInUse` 検査（§6.3）の「any spelling」も、次の綴りを照合対象に含める必要がある。

- エスケープされた形と二重括弧の形。
- RX が出力する **表名なし** の `[[#This Row],Col]`（`formula.rs` L976-1011 は `@` を文字単位で置換するだけ）。

**推奨。**

- 構造化参照の列指定子を作る関数を 1 つ定義する（`'` エスケープと、必要なら `[[…]]`）。
  集計、計算列、`ColumnInUse` の照合がすべてこれを使う。
- Phase1 の lexer で往復できることを確認する。
- 試験に `LIBRE_OFFICE-128382-0.xlsx` と `63934.xlsx` を使う。

### M3. フォールバックノートの `xr:uid` に必要な名前空間宣言と `mc:Ignorable` の規定がない

§5 はフォールバックノートの `comment/@xr:uid` をスレッド id にするとしている。`64759.xlsx` ではルートに
`xmlns:mc`、`mc:Ignorable="xr"`、`xmlns:xr` がある。しかしコーパスの comments パートでは、
**31 中 26 のルートに `xr` の宣言も Ignorable もない**。

- その comments パートにフォールバックを足すと、`xr` を宣言しなければ XML として壊れる。
- 宣言しても `mc:Ignorable` がなければ、MCE に従う旧版の Excel（2007/2010。まさにフォールバックを読む側）が
  未知の名前空間の属性でエラーにする。
- ルートを変える場合は T2 の変更になるが、試験オラクル 1 の allowed-diff 表に入っていない。

**推奨。**

- 新しい `comment` 要素に `xmlns:xr`、`xmlns:mc`、`mc:Ignorable="xr"` を **局所的に** 宣言する
  （MCE は任意の要素に Ignorable を置ける）。ルートは変えない（スパークラインの §8 と同じ方針）。
- または、ルートに足すことを allowed-diff に明記する。
- ルートにすでに `xr` が別の URI で宣言されている場合は拒否する。
- どちらの方式でも、Excel 2010 相当で開けるかを XL 項目にする。

### M4. 表作成が既存セルを上書きするときの意味論が決まっていない

§6.1 は見出しセル、集計行、計算列のセルを「書く」としているが、範囲内に既存の内容がある場合の扱いがない。

- 呼び出し側が列名を与えたとき、既存の見出し文字列や数値を上書きするのか。
- 集計行や計算列の位置にある既存の値や数式（共有数式マスタを含む）、`cm`/`vm` セルをどう扱うのか。
- 上書きされるセルの Phase1 依存先（見出しセルを A1 で参照している数式）を無効化するのか。

§10 の Phase1 の行は集計と計算列の数式だけを挙げ、見出し文字列の書き込みによる依存先の無効化に触れていない。
`fullCalcOnLoad` は Excel 専用で、LibreOffice は古い値を表示する（ADR-0003 自身の注記）。そのため Phase1 の
保証に穴が開く。

**推奨。**

- 規定: 見出し・集計・計算列の位置が空でないセルは、「呼び出し側の列名と同じ文字列」の場合を除いて
  `Err(TableRange { reason: "cell not empty" })` とする。もしくは明示的な `overwrite(true)` を設け、
  そのときは Phase1 のクリアと書き込みを使う。
- `TableRange` の拒否理由に `vm`/`cm` セルと共有数式マスタを加える。
- これらの書き込みは B1 のバッチ計画で事前に検証する。

---

## Minor

- **m1（removal の stage 範囲）。** §2.1 は「Removals (picture, note, thread, table) stage the sheet part」としている。
  しかし画像、ノート、スレッドの削除はシートを変えない。シートの stage（ADR-0003 の計測で 289 ms の clone）が
  必要なのは表の削除だけである。
  - PR #38 の `step()` は 1 段ごとに plan 全体を clone する（`add.rs` の `step`。stage 済みの木も含む）。
    そのため、シートを stage した後の各段でシートの clone が繰り返される。
  - 表の削除ではシートを最後に近い段で stage するか、段数を最小にする。
- **m2（表 id の走査範囲）。** `table/@id` の max に `x15:tableSlicerCache/@tableId` も含める。
  Alternatives 6 自身が「スライサーキャッシュの参照を蘇らせない」ことを理由にしている。
  ADR-0009 の `sheetId` が calcChain の `i` を含めているのと同じ扱いになる。
- **m3（`metadata.xml` の挿入位置）。** calamine の `pictures_in_cell_and_over_cell.xlsx` の `metadata.xml` は
  XLRICHVALUE だけで、`valueMetadata` はあるが `cellMetadata` がない。
  - XLDAPR を足す場合、`futureMetadata name="XLDAPR"` は既存の `futureMetadata` の後、`cellMetadata` は
    `valueMetadata` の **前** に入れる必要がある。§9 の「append」は、スキーマのスロット挿入（lens）と書き直す。
  - `xmlns:xda` は新しい `ext` の上で宣言し、ルートは変えない。
  - この fixture を試験ケースにする。
- **m4（画像ヘッダの安全性と EMU）。** マジックバイトとヘッダの解析は、境界を検査し panic しないこと
  （fuzz 対象に加える）。
  - 次の場合は `ImageSize` にするか、96 dpi にフォールバックする: DPI = 0、PNG `pHYs` の単位 0、
    JFIF の density 単位 0、極端に小さい DPI。
  - `ext` の `cx`/`cy` は ST_PositiveCoordinate（≤ 27,273,042,316,900）を超えたら拒否する。
  - JPEG の SOF は全マーカー（SOF0–SOF15。ただし DHT/JPG/DAC は除く）を走査し、高さ 0（DNL）は `ImageSize` にする。
  - BMP の負の高さ、EMF の `rclFrame`、WMF の placeable ヘッダの単位を明記する。
  - EXIF の回転は扱わないと明記する。
- **m5（allowed-diff 表の欠落）。** 以下が抜けている。いずれもオラクルを生成する元になる表なので、全操作分そろえる。
  - 画像: 新しい描画の場合のシート `.rels`。
  - 表: 見出し同期の行、`workbook.xml` の `calcPr`、calcChain の削除。
  - 動的配列: calcChain と `workbook.xml`。
  - チャートシート: `workbook.xml`、workbook `.rels`、`app.xml`、CT。
  - スレッド: 2 つの CT `Override`。
  - 削除系（画像、ノート、スレッド、表）: 行そのものがない。
- **m6（見出し同期の適用範囲）。** `tableType="queryTable"`（13）と `"xml"`（5）の表は、列が `queryTableField` と
  `xmlColumnPr` に結び付いている。ピボットキャッシュとスライサーも列名で結び付く。
  - これらの表、またはピボットやスライサーのソースになっている表の見出し同期は拒否する。
  - 空文字列への変更（Excel なら `Column1` になる）も拒否する。
- **m7（`@` と未来関数の扱い）。**
  - `[@Col]` の変換は文字単位の置換（RX）ではなく、Phase1 の lexer のトークンに対して行う。
  - 入力テキストにスピル演算子 `A1#` や暗黙の交差 `@` があれば拒否する（ファイル形は `_xlfn.ANCHORARRAY`/`_xlfn.SINGLE`）。
  - Q7 は Phase1 の `set_formula` の契約変更になるので、ADR-0003 側に注記が必要。
- **m8（チャートキャッシュ更新の拒否条件）。** `ChartNotRefreshable` に次を加える。
  - キャッシュが `mc:AlternateContent` の中にある場合（コーパスで 16 チャートに AC がある）。
  - `c15:datalabelsRange` のキャッシュがある場合。
  - `c:multiLvlStrRef` の場合。
  - `c:f` が名前、表、列全体を参照している場合。
  - 新規チャートの分類軸は、数値なら `numRef`、それ以外は `strRef` と選択規則を明記する。
- **m9（引用の精度）。**
  - POI の `updateHeaders` は `\n`/`\r` しか置換しない（`XSSFTable.java` L837-838）。`_x005F_` を含む
    ST_Xstring の符号化は POI ではなく仕様に由来する。§6.3 の「as the corpus and POI show」を直す。
  - §4.2 のノート削除で、同じ行と列の `Note` shape が複数ある場合の扱い（全部消すか拒否するか）を書く。
- **m10（GUID）。** v4 の乱数でも、既存のスレッド id と person id との衝突検査は安価なので行う。
  `dT` が UTC かローカル時刻かを XL 項目に加える。
- **m11（スパークラインの ext 順序）。** 末尾への追記はスキーマ上は合法である。ただし Excel は ext を
  CF → DV → sparkline → slicer の順に書くので、既存の ext の並びに対する挿入位置を XL 項目にする。

---

## 推奨（Q1–Q7）

| Q | 推奨 |
|---|---|
| Q1 | **提案どおり `oneCellAnchor` + 自然サイズ**。幅のモデルを持たずに寸法が正確になる。ただし LibreOffice と calamine の読み戻しを独立読者オラクルに加える。 |
| Q2 | **再利用する。ただし条件付き。** 内部ターゲットで、画像のコンテンツタイプかつ拡張子が一致するパートに限る。ハッシュは初回の挿入時に遅延計算する。削除は計画のビュー上で参照を数える（`remove_parts` に `image` 型を許可）。 |
| Q3 | 提案の短い形でよい。ただし定数にして、Excel が保存時に書き換えるか（XL）で最終決定する。フォールバック本文は Excel の判定には使われないので、ここが原子性や破損に影響することはない。 |
| Q4 | **提案どおり v4 乱数 + 試験用の決定的なソース**。加えて、既存 id との衝突検査を行う（m10）。 |
| Q5 | **提案どおり拒否**。 |
| Q6 | **提案どおり明示呼び出し**。m8 の拒否条件を追加する。 |
| Q7 | **提案どおり拒否 + opt-in ヘルパ**。ただし `set_formula` への適用は ADR-0003 の改訂として扱い、`#`/`@` 演算子の拒否も含める（m7）。 |

## 試験計画・配送計画への追記

- オラクル 2 に次を加える。
  - シート内の shape 番号の一意性（`cNvPr` ∪ VML の `id`/`o:spid`）。
  - comments で `xr` を使う場合の宣言と `mc:Ignorable` の存在。
  - 生成した構造化参照が lexer で往復すること。
  - `metadata.xml` の子要素がスキーマ順であること。
- オラクル 3 の明示ケースに次を加える: `mv-calculator…xlsm`、`45540_form_*.xlsx`、`ExcelWithAttachments.xlsm`
  （B2）、`bug66215.xlsx`（M1）、`LIBRE_OFFICE-128382-0.xlsx`（M2）、calamine の rich value fixture（m3）。
- 配送: P5b（表）と P5d（動的配列）の前に、B1 の Phase1 バッチ計画（ADR-0003 の追記と実装 PR）を置く。
  B2 の割り当て器は P5a に入れる。

## 判定

Accepted after Minor only: **no**（Blocker 2 件、Major 4 件）。
B1・B2・M1–M4 を反映した rev 2 で、round 2 を行う。
