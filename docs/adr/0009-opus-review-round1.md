# ADR-0009 レビュー（Opus, round 1）

対象: PR #27 `docs/adr/0009-xlsx-phase3-sheet-operations.md`（Proposed, rev 1）、`spikes/xlsx-phase3/`、
`docs/roadmap/xlsx.md`。照合先: ADR-0001（PR #14 interim note）、ADR-0003 rev 4（§2.1, §2.4, §3.1, §4.3）、
ADR-0006 rev 2、PR #14 `Package::remove_part` 実装（`core-remove-part` worktree, `crates/ooxml-core/src/package.rs`）、
Phase1 実装の lexer（`impl-xlsx-phase1`, `crates/ooxml-xlsx/src/formula/lexer.rs`）。
独立プローブ: POI `test-data/spreadsheet`（366 ファイル）に対し、全 XML パーツの全テキストノード・全属性から
既存シート名の sheet-prefix 言及を拾うスクリプト
（`/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr27/probe/locs.py`）と、
特定構文のバイト走査（`python3 -I`）。

## 結論（先に）

方向性（id を振り直さない、app.xml を再生成しない、POI の部分集合ではなく全参照を書き換える、
pivot は拒否）は正しく、根拠調査も丁寧。しかし **このままでは実装できない／黙って壊れる** 箇所が 3 つある。

1. **削除のカスケードが PR #14 の `remove_part` では表現できない**（B1）。leaf-first のどの一手も
   `StillReferenced` / `HasOwnRelationships` / `RegistryPart` で拒否される。VBA 削除も同じ。
2. **`check_*` 双子を個別に呼ぶ検証は、順序依存の複数手プランを検証できない**（B2）。シート part の削除は
   `workbook.xml` の `<sheet r:id>` を消した *後の* 状態でしか通らないが、twin は現状態しか見ない。
3. **rename の書き換え対象が列挙型で、しかも欠けている**（B3）。コーパスに Excel が書いた実例のある
   `connections.xml parameter/@cell`、テーブル `calculatedColumnFormula`、ピボットグラフ
   `c:pivotSource/c:name` が漏れており、未知の場所に対する安全網（拒否）も無い。テストオラクル（M7）も
   同じ列挙を共有するので検出できない。

加えて、`#REF!` の書き方（M1: コーパスの Excel 出力は裸の `#REF!` のみ、`#REF!A1` は 0 件で Phase1 lexer も
受け付けない見込み）、場所ごとの構文差（M2）、Phase1 の健全性（M3: `Everything` 式が rename/add/reorder の
依存に入っていない）、削除シート上のテーブルへの構造化参照（M4）が Major。

## 判定

**Accepted after Minor only: no.** B1–B3 と M1–M8 の反映後に round 2。

---

## Blocker

### B1. §5 の leaf-first カスケードは PR #14 の `remove_part` で実行不能

**根拠**（`core-remove-part/crates/ooxml-core/src/package.rs` `plan_removal`、ADR-0003 §4.3 契約）:

- `remove_part` は対象に自前の `.rels` があれば `HasOwnRelationships` で拒否（「cascading deletion is out of
  scope」と ADR-0003 §4.3 が明記）。
- `.rels` パーツ自体は `RegistryPart` で拒否され、**削除する手段がない**。関係を全部消しても空の `.rels`
  が残るので、親は永久に `HasOwnRelationships`。
- leaf から消そうとしても、親パーツの本文がまだ `r:id`/`r:embed`/`o:relid` を持つので `StillReferenced`。
  例: `xl/media/image1.png` を消そうとすると `drawing1.xml` の `r:embed="rId1"` で拒否。drawing を先に消そうと
  すると `HasOwnRelationships`。どの順序でも一手目から通らない。
- §10 の `remove_vba_project` / 置換も同じ。署名付きなら `vbaProject.bin` は `xl/_rels/vbaProject.bin.rels`
  を持つ。

ADR 本文の「`remove_part` is called leaf-first with the allowlist of the edge being cut. Its refusals … mean the
plan was wrong」は、**正しいプランでも必ず拒否される**ので成り立たない。

**推奨:** 新しいコア primitive を明示的に設計し、ADR-0001 の別 note とする（PR #14 note の「拡張」ではなく
置き換え）。例えば `Package::remove_part_set(set: &[PartName], allowed: &[(source_kind, rel_type)])`:

- 集合 S は「S の外の残るパーツから、S 内を指す関係が無い」閉包でなければならない（排他性の判定はコアで行う）。
- S 内パーツの `.rels` は S と一緒に消える。
- S の外の `.rels` から S 内を指す関係を取り除く。その関係の Id を持つ属性が S の外の残るソース本文に
  あれば `StillReferenced`（ソース本文は **ステージ後** の内容で判定する。B2）。
- Override は消す。Default は触らない。
- 残る id の振り直しはしない。

これで §5 の表、`vmlDrawing`→`image`、`chart`→`chartUserShapes`→`drawing`→`image` の連鎖、VBA と署名を一手で
扱える。Q1 の文言もこの primitive を前提にする。

### B2. `check_*` 双子の個別呼び出しでは「plan → validate → apply」が成立しない

**根拠:** シート削除で `check_remove_part("xl/worksheets/sheet2.xml", …)` を現在のパッケージに対して呼ぶと、
`workbook.xml` がまだ `<sheet … r:id="rId2"/>` を持つので `StillReferenced` になる（`refers_to_rel_id` は
`read_part(src)` の現内容を走査する）。apply では Layer 2 の `<sheet>` 削除 → part 削除の順なら通るが、
validate でそれを示せない。逆向きにも同じ問題がある。add は「`add_part` → `add_relationship` → 新 id を
`<sheet r:id>` に書く」で、`check_add_relationship` は自分より前の `add_part` を知らない。ADR-0003 §3.1 の
「plan が apply の成功を証明する」は、calcChain という 1 手で前提の無い操作だから成り立っていた。

§12 の「An `Err` leaves the package unchanged」は、この合成が無い限り保証されない（apply 途中の Err は Poisoned
になるだけ）。

**推奨:** コアに **ステージ済みの編集集合を一括検証する API** を置く。例: `PackageEdit` ビルダーに
`add_part` / `add_relationship` / `set_content_type` / `remove_part_set` / `stage_replace(part, bytes)` を積み、
`check(&Package)` が「全部適用した後の状態」で OPC 不変条件と `StillReferenced` を検証し、`apply` は失敗しない。
ステージされた `workbook.xml` やシート本文もそこへ渡す。`check_*` の単体双子はこの API の特殊例として残してよい。

### B3. rename の書き換え対象が不完全で、未知の場所に対する拒否が無い

**根拠（独立プローブ, POI 342 パッケージ）。** ADR §6 の表に無いのに、実ファイルでシート名を含む場所:

| 場所 | 実例 | 備考 |
|---|---|---|
| `xl/connections.xml` `connection/parameters/parameter/@cell`（`parameterType="cell"`） | `dataValidationTableRange.xlsx`: `cell="'County Ranking'!$B$5"` ×12 | Excel 出力（`refreshedVersion="6"`） |
| `xl/tables/tableN.xml` `tableColumn/calculatedColumnFormula`（と `totalsRowFormula`） | `StructuredRefs-lots-with-lookups.xlsx`: `…=List!…` | 列全体の計算式。Excel は rename で書き換える |
| chart `c:pivotSource/c:name` | `WithChartSheet.xlsx`: `[CVT23.tmp]Sheet2!PivotTable2` | `[file]Sheet!PivotName` 形式。数式ではない |

コーパスには無いが、スキーマ上シート参照を持つ場所（rename 時に黙って古い名前が残る）:

- 条件付き書式 2007 の `cfvo/@val`（`type="formula"`）。ADR は `cfRule/formula` しか挙げていない。
  `x14:cfvo/xm:f` は §6 の汎用 `xm:f` に含まれる。
- chart 拡張 `c15:f`、`c15:sqref`（`c15:datalabelsRange`、`c15:filteredSeriesTitle`/`…CategoryTitle`、
  `c15:fullRef`/`c15:formulaRef`）。survey は C 名前空間の `c:f` しか見ていない（`sheet_ops_survey.py`
  L273–279）。
- **chartEx**（`xl/charts/chartEx*.xml` の `cx:f`）。survey の正規表現 `xl/charts/chart\d*\.xml` に一致しない。
  削除時は §5 の「any other type → refused」で安全だが、rename では黙って漏れる。
- DrawingML 図形の `xdr:sp/@textlink`、chart userShapes の `cdr:sp/@textlink`（セルにリンクしたテキスト
  ボックス。プローブでは 269 個とも空だった）。
- フォームコントロール: `xl/ctrlProps/*.xml` の `@fmlaLink`/`@fmlaRange`/`@fmlaTxbx`/`@fmlaGroup`、VML
  `x:ClientData` の `x:FmlaLink`/`x:FmlaRange`/`x:FmlaTxbx`/`x:FmlaPict`、シートの
  `controlPr/@linkedCell`/`@listFillRange`。
- ActiveX の `LinkedCell`/`ListFillRange` は `activeX*.bin`（バイナリ）の中にあり、**書き換えも検出もできない**。
- `dataConsolidate/dataRefs/dataRef/@sheet`、pivotCache の `consolidation/rangeSets/rangeSet/@sheet`
  （どちらも生のシート名）。
- `webPublishItem/@sourceRef`/`@sourceObject`（未確認。要確認）。
- `app.xml` の "Named Ranges" ブロックのうち、シート名接頭辞付きのタイトル（`'1'!Print_Area`）。§6 の表は
  「app.xml titles (§11)」とだけ書いており、シートブロックだけなのか曖昧。

ADR の拒否規則は「**既知の場所** で lex できないテキストがあれば拒否」なので、**未知の場所** は拒否されずに
成功扱いになる。これは ADR 自身の「A silent miss is worse than a refusal」に反する。

**推奨:**

1. 書き換え対象を「既知の場所 → 書き換え関数」の表として ADR に明記する。少なくとも上記を追加し、それぞれ
   数式構文か生の名前かを書く（M2）。
2. **閉じた世界の安全網** を加える。rename の plan で、到達可能な全 XML パーツ（content type 不問）の全テキストと
   全属性を走査し、旧名の sheet-prefix 形（`'…'!`、`Name!`、`A:B!` を ST_Xstring デコード後・case-fold で）が
   既知の場所の外にあれば `Err(UnrewritableReference { part, location })` とする。例外は既知の非参照
   （`t="s"`/`t="str"` の値、sharedStrings、文字列リテラル、`[n]` 外部参照）だけ。プローブのスクリプトは
   Python で数十行、全コーパスでも数秒だった。
3. バイナリで参照を持ちうるパーツ（`activeX*.bin`、`oleObject` 埋め込み）があれば rename を拒否するか、
   docs に「ActiveX の LinkedCell は書き換えない」と明記する（Q6 と同じ扱い）。

---

## Major

### M1. `#REF!` の書き方: Excel が書くのは裸の `#REF!`。`#REF!A1` は Phase1 で lex できない見込み

**根拠（POI コーパス, `#REF!` を含む式・名前の形）:**

- セル式の `#REF!` 328 個、定義名 212 個は、すべて参照トークン全体が裸の `#REF!` に置き換わった形
  （例: `IFERROR(VLOOKUP(#REF!,…`、`IF(#REF!="verkauft",…`、名前 `#REF!,#REF!,#REF!`）。
- `#REF!A1` / `#REF!$A$1` の形は **0 件**。
- 別の形 `Summary!#REF!`、`'N'!#REF!` は行・列削除によるもので、シート削除とは別。
- UI 上の表示が `=#REF!A1` になる、という記述は Web にある（coddy.tech）。ただしそれはファイル上の形の根拠にならない。
- Phase1 lexer（`lexer.rs` L120, L446–451）は `#REF!` をエラーリテラルとして、`Sheet!#REF!` を `Area::RefError`
  として扱う。`#REF!A1` はエラーリテラルと参照が隣接した形になり、parser の構文に無い（推定）。そうなると
  以後その式は `Everything` になり、共有式の hand-off も `SharedFormulaMaster` で失敗する。

**推奨:** シート削除時は **シート接頭辞付き参照トークン全体（範囲を含む）を `#REF!` に置き換える**
（`Sheet2!A1:B2` → `#REF!`、`SUM(Sheet2!A1)` → `SUM(#REF!)`）。定義名・`c:f`・DV/CF・`xm:f` も同じ。
XL 項目 9(b) で、範囲・名前・チャート系列・3-D の各形を確認する。rename/delete 後のテキストが Phase1 lexer で
lex できることを不変条件とし、テストにも入れる。

### M2. 場所ごとの構文差（引用・エスケープ・外部参照）が規定されていない

§6 は「quoting is recomputed」の一文だけで、すべての場所を数式構文として扱っている。

- **生の名前で、引用しない場所:** `worksheetSource/@sheet`、`dataRef/@sheet`、`rangeSet/@sheet`、`app.xml` の
  シートタイトル、`c:pivotSource/c:name` の `]Sheet!` 部分（引用規則は未確認）。ここで `'Q1 2024'` と書くと
  別のシート名になる。survey 自身が `@sheet` を `'…'!A1` に包んで数式扱いしている（L287）のは調査なら良いが、
  実装へ持ち込んではいけない。
- **3-D スパンはまとめて引用する:** `'Jan:Q1 2024'!A1`。片方の端点だけ引用した `Jan:'Q1 2024'!A1` は不正。
  端点の一方を空白入りの名前に rename したら、スパン全体を引用し直す。
- **外部参照は書き換えない:** `[1]Sheet1!A1`、`'[1]Sheet 1'!A1` は他ブックのシート。名前が一致しても書き換え
  てはならない。自己参照する外部リンクがあり得ても、他ブック扱いのまま残す。§6 の「Not rewritten」に明記する。
- **ST_Xstring:** `sheet/@name`、`<f>`（ST_Formula は ST_Xstring 派生）、`vt:lpstr` は `_xHHHH_` エスケープを持つ。
  照合はデコード後に行う。書き出しでは、名前に含まれる制御文字と、リテラルの `_xHHHH_` 形
  （`_x005F_` で逃がす）をエスケープする。コーパスにも `_x000a_` 入りのシート名がある（`xssf-enum.xltx.xlsx`、
  spike README）。
- **引用の判定:** 「looks like a cell or R1C1 reference」だけでは不足する。Excel の規則を完全には再現できない
  ので、保守側に倒す。`^[A-Za-z_][A-Za-z0-9_.]*$` に一致し、A1/R1C1 の形でも `R`/`C` 単独でもないとき
  だけ引用を省き、それ以外（非 ASCII、数字始まり、`TRUE`/`FALSE` を含む）は常に引用する。引用は常に正当で、
  Excel は保存時に正規化する。

### M3. Phase1 の健全性: rename・add・reorder でも値の変わる `Everything` 式が依存に入っていない

ADR-0003 の原則は「precise と示せない式は `Everything`」。ところが §12 は、rename の seed を `CELL`/`INFO`、
add と reorder の seed を 3-D と `SHEET`/`SHEETS` に限っている。値が変わる反例:

- rename: `INDIRECT("Old!A1")` と `SHEET("Old")` は、新しい値が `#REF!`/`#N/A` になる。名前の中の XLM
  `GET.WORKBOOK(1)` はシート名一覧を返す。どれも ADR-0003 では `Everything` だが、seed が無いので依存に
  ならず、キャッシュ値が古いまま残る（calamine の CR はそれを読む）。
- **未知シート名の復活:** `=Foo!A1` のように存在しないシートを指す式は、ADR-0003 §2.4 で `Everything`。
  そこへ `Foo` を add するか、別シートを `Foo` に rename すると、指す先が実在するシートになる。
- reorder: Microsoft の文書は「If you reverse the end points, the 3-D reference changes the endpoint
  worksheet」と書いている。端点を逆転させる移動では、**Excel は 3-D 参照のテキストを書き換える**。§7 は
  テキストを変えず、ADR-0003 §2.4 の「どちらが先でも同じスパン」で解釈するので、同じ操作を Excel で行った
  結果と値がずれうる。

**推奨:** すべての構造操作（add・delete・rename・reorder）で、`Everything` 式全体を依存に入れる。ADR-0003 は
その閉包をキャッシュしているので追加コストは小さい。reorder で端点が逆転する場合は、Excel に倣って書き換える
か、書き換えずにスパンを「位置の間」と解釈することを明記し、XL 項目に追加する。

### M4. 削除シート上のテーブルへの構造化参照が宙に浮く

ADR-0003 の tokenizer は構造化参照を不透明な 1 トークンとして扱う（§2.1）。そのため、削除シート上の
`Table1` を他シートの式・定義名・DV/CF・テーブル列式から `Table1[Col]` で参照していても、§5 の書き換え対象に
ならない。消えたテーブル名を指す式が残り、Excel は修復ダイアログを出すか `#REF!` にする（XL 未確認）。
同じ穴が 2 つある。§5 の「a name that resolves to it」がテーブル名を含むか曖昧な、ピボットの
`worksheetSource/@name`。そして、他シートのスライサーの `x15:tableSlicerCache/@tableId`。

**推奨:** 削除シートに table パーツがあり、そのテーブル名（大小文字無視）が他所の構造化参照・ピボット
ソース・tableSlicerCache で使われていれば `Err(SheetHasUnsupportedParts)`（または `TableInUse`）にする。
Excel 同様に `#REF!` へ書き換えるのは XL で形を確かめてからにする。

### M5. id で結ばれた状態と共有ブック

- `customWorkbookView/@activeSheetId` は sheetId を持つ（`61495-test.xlsm`）。削除で宙に浮くうえ、§3 の
  「max+1」で過去セッションの最大 id が再利用されると、別シートを指してしまう。
- **共有ブックの変更履歴**（`xl/revisions/revisionHeaders.xml`、`header/sheetIdMap`。コーパス 6 箇所、
  `workbookProtection_workbook_revision_protected.xlsx` など）は sheetId 一覧を保持する。add・delete しても
  一覧が更新されない。`History` が予約名なのもこの機能のためで、ADR は予約名だけを扱い、機能そのものを
  扱っていない。
- `volatileDependencies.xml` の `tr/@s`（RTD。ECMA では sheet id。未確認）。

**推奨:** `revisions` 関係が存在すれば、Phase3 の全シート操作を `Err(SharedWorkbook)` で拒否する。削除で
`activeSheetId` が宙に浮くなら、その `customWorkbookView` を残し、activeSheetId を残存シートへ寄せるか拒否する
（XL）。sheetId の high-water は「入力の最大値 + セッション内削除」より強くできない。この限界を ADR に書く。

### M6. `add_part` が孤立した `.rels` を「継承」しうる

ADR は、OPC 同値な part 名が既に存在する場合だけ `add_part` を拒否する。入力に `xl/worksheets/_rels/sheet5.xml.rels`
だけが残っていれば（パーツは無く `.rels` だけの孤児。他ツールの削除残り）、新しい `sheet5.xml` は既存の関係を
自動的に持ってしまう。新シートに drawing・table・hyperlink が付くことになる。ZIP 内のパーツでないエントリ
（ディレクトリエントリ、大小違い）も同じ衝突空間にある。

**推奨:** `add_part(name)` は、`rels_part_name(name)` が（OPC 同値で）既に存在する場合も拒否する。§3 の名前
割り当ては、その名前も「使用中」とみなして番号を進める。オラクル 1 に「新規パーツの `.rels` は追加したもの
だけ」を加える。

### M7. 書き換えオラクルが実装と同じ列挙を共有し、完全性を検出できない

オラクル 4 は「census が見つけた formula-bearing location」を対象にする。B3 の漏れは、census 自体が知らない
場所にある。「misses one location class」のミュータントも、既知のクラスを 1 つ外すことしか試さない。さらに
openpyxl tokenizer は、`@sheet`・ハイパーリンク・`c:pivotSource/c:name` のような数式でない場所には使えない。

**推奨:** **残留言及オラクル** を加える。rename 後の出力の全パーツ（XML なら全テキスト・全属性、バイナリなら
UTF-8/UTF-16LE のバイト列）に、旧名の sheet-prefix 形が「許可された非参照の場所」以外に残っていないこと、
かつ新名の出現数が「旧名の出現数 + 入力にあった新名の出現数」と一致することを確かめる。実装とは独立した
正規表現走査で行い、本レビューのプローブと同じ方式にする。delete では、削除シート名が残っていないことを確かめる
（`hyperlink/@location` など明示された例外は除く）。合成フィクスチャとして、B3 の各場所に 1 個ずつ参照を置いた
ファイルを用意する（openpyxl では作れない場所は手書き XML）。

### M8. VBA: 署名パーツの特定、`.xlam`/テンプレート、拡張子チェックの退行

- 署名は 3 種類ある。関係型 `…/vbaProjectSignature`、`…/vbaProjectSignatureAgile`、`…/vbaProjectSignatureV3`
  と、それぞれの content type だ。「`vbaProjectSignature*.bin`」というファイル名パターンで消すのは誤りで、
  `vbaProject.bin` の `.rels` から関係型でたどる。Agile/V3 署名を残したままプロジェクトを差し替えると、
  署名の検証に失敗する。
- `set_vba_project(bin, signature: Option<bin>)` は旧形式の署名しか表せない。署名生成はスコープ外なので、
  引数を外すか、`Signature { legacy, agile, v3 }` の型にする。
- content type の対応は `xlsx↔xlsm` と `xltx↔xltm` で、`.xlam`（`application/vnd.ms-excel.addin.macroEnabled.main+xml`）
  は常に macro。remove のとき、xlam をどの型へ戻すかを書く（推奨: xlam からの remove は拒否）。
- `save` の `ExtensionMismatch`: 入力がすでに不一致（`.xlsx` の名前で中身が macroEnabled）なら、無変更の
  round-trip が新たに `Err` になり、Phase0 の退行になる。チェックは「このセッションで content type を変えた
  とき」に限るか、拡張子を変えた保存にだけ適用する。`to_bytes` は対象外と明記する。
- 削除・置換は B1 の新 primitive で行う（`vbaProject.bin.rels` を持つため）。

---

## Minor

- **m1 app.xml.** 「Excel は veryHidden を載せない」の根拠は veryHidden を含む 1 パッケージだけ（spike README）。
  XL 項目に入れる。"Named Ranges" に Excel が載せる名前の規則も未記述で、Print_Area/Print_Titles は載り、
  `_FilterDatabase` と hidden な名前は載らないと推定される。§9 の名前 CRUD で載せる・外す判定の基準にする。
- **m2 rId の high-water.** sheetId はセッション内で再利用しないのに、rId は「現存の最大 + 1」なので、最大の
  rId を持つシートを削除してから add すると同じ rId が再利用される。同じ方針（セッション内 high-water）に揃える。
  数値は `u64` で解釈し、溢れたら拒否する。
- **m3 名前の規則.** `R`/`C` の判定は大小文字を区別しない。R1C1 形（`R1`、`C2`、`RC`、`R[1]C`）も拒否する。
  予約接頭辞（`_xlnm.`、`_xlfn.`、`_xlpm.`、`_xlws.`、`_xludf.`）の `add_name` は拒否する。
- **m4 `docProps/custom.xml` の `linkTarget`.** コンテンツにリンクしたカスタムプロパティは定義名を指す
  （コーパス 9 箇所、`54206.xlsx`）。`rename_name` では書き換え、`delete_name` では `NameInUse` の数に入れる。
- **m5 残すパーツを明記する.** `persons.xml`（threaded comments、ブック単位）、customXml、`xmlMaps.xml`、
  `bin`/`png`/`vml` の Default は、シート削除で残す。chart → `image`/`package`（`c:externalData` の埋め込み
  ブック）、drawing → SmartArt `diagram*`/`chartEx`/`hdphoto`/`media` は「any other type → refused」に落ちる。
  過剰な拒否だが安全側なので、Consequences に件数とともに書く。
- **m6 マクロシート・ダイアログシートの削除.** XLM の `Auto_Open` と、`function`/`xlm` 属性付きの名前は
  マクロシートを指す。Phase3 では、マクロシートの削除を拒否するのが無難。
- **m7 VBA とシート名.** §6 の「VBA source (which uses codeName, unaffected)」は半分しか正しくない。
  `Worksheets("Sheet1")` は名前で参照する。docs で「rename は VBA 内の名前参照を書き換えない」と明示する。
  `remove_vba_project` 後も、`x:FmlaMacro`（コーパス 57 箇所、`62629_target.xlsm`）と図形の `@macro` は宙に
  浮いたまま残る。
- **m8 3-D の端点を内側へ移す規則.** 内側の隣が chartsheet の場合の扱いが無い。スパンが 1 枚に縮んだとき、
  `Sheet3!A1` と書くのか `Sheet3:Sheet3!A1` と書くのかも決まっていない。XL 9(b) に含める。
- **m9 ステージのコスト.** §12 は「staged clones of every touched Layer 2 part」と書くが、ADR-0003 §3.1 は
  「No part is cloned except workbook.xml」で、200k 式のシートの clone は 289 ms と測られている。rename・delete
  は参照を持つ全シートに触れる。plan で位置と置換文字列を確定し、apply で in-place に書く ADR-0003 の方式に
  揃えるか、clone のコストを見積もる。
- **m10 自己参照の接続とデータモデル.** `connections.xml` の `dbPr/@command`（`SELECT … FROM [Sheet1$]` で
  自ブックを読むもの）、Power Query の DataMashup（customXml 内の base64）、Power Pivot の `item.data` は
  書き換えない。docs に明記する。CUBE 関数（`CUBEVALUE("ThisWorkbookDataModel", …)`）は接続名と MDX しか
  持たないので、rename の影響は無い。
- **m11 タブのグループ選択.** 複数シートが `tabSelected="1"`（グループ選択）の場合、§5 と §8 で
  `tabSelected` をどう直すかを書く。
- **m12 survey の限界を README に書く.** chartEx、`c15:*`、ctrlProps、VML、tables、connections を走査して
  いない。§Context 7 の「Rename impact」は下限であることを明記する。

---

## 推奨（Q1–Q6）

- **Q1（ADR-0001）: 条件付きで yes。**
  - 追加系（`add_part`、`add_relationship`、`set_content_type`）は append-only と矛盾しないので認めてよい。
    条件は M6（孤児 `.rels`）と m2（id の high-water）。
  - 削除は、PR #14 の interim note を「広げる」形では成立しない（B1）。閉包を一括で消す新 primitive と、
    一括検証の API（B2）を別の core PR・別の ADR-0001 note として定義し、その文言を Q1 の承認対象にする。
  - 残るエントリの番号を振り直さない、は維持する。
- **Q2（削除時の書き換え）: Excel に倣う書き換え（提案どおり）に賛成。** ただし次の条件を付ける。
  - 形は裸の `#REF!`（M1）。
  - 削除シート上のテーブルが他所で使われていれば拒否（M4）。
  - 残留言及オラクル（M7）を必須にする。
  - 書き換えた場所の一覧（`Vec<RewrittenLocation>`）を戻り値で返し、呼び出し側が監査できるようにする。
  - 参照されたシートの削除をすべて拒否するモードは、`delete_sheet_strict` として併設するのが安価。
- **Q3（VBA ブックの新シート）: 提案どおり `codeName` は付けない。** XL 9(c) で、Excel が文書モジュールを
  黙って足すことを確認する。足さずに修復や VBA の喪失が起きるなら、次ラウンドで「既存 codeName と衝突しない
  `Sheet{n}` を付ける」に切り替える。
- **Q4（app.xml のブロック）: 提案どおり「構造を発明しない」。** app.xml は情報用で、Excel が保存時に再計算する。
  英語ラベルのブロックを足すほうが、ローカライズされたブックでは害が大きい。必要なブロックが無いときは、その
  操作について app.xml 全体を触らないことを明記する（部分更新で数が合わなくなるのを避ける）。
- **Q5（使用中の名前）: 拒否を既定とすることに賛成。** 使用数には、セル式に加えて DV/CF、`xm:f`、チャート
  `c:f`、テーブル列式、他の名前、`custom.xml` の `linkTarget`（m4）、`connections` の parameter を数える。
  数えられない場所（バイナリ、未知パーツ）に名前が現れたら、`force` でなければ拒否する。
- **Q6（ピボットなどを持つシート）: Phase5 まで拒否で可。** 対象に次を加える。
  - 他所で使われるテーブル（M4）。
  - 共有ブック（`revisions`。M5）。
  - ActiveX バイナリを持つブックの rename（B3-3。docs で明示するなら許容も可）。
  - chartEx・SmartArt を持つシートの delete（m5、既に「any other type」で拒否される）。

  rename はピボットのあるシートでも許可してよい。ただし `worksheetSource/@sheet` と `c:pivotSource/c:name` を
  書き換えることが前提になる（B3）。

## 出典

- Microsoft Support, "Create a reference to the same cell range on multiple worksheets"（3-D 参照の
  Insert/Delete/Move/Move an endpoint/Delete an endpoint、"If you reverse the end points, the 3-D reference changes
  the endpoint worksheet"）:
  <https://support.microsoft.com/office/create-a-reference-to-the-same-cell-range-on-multiple-worksheets-c906f8b4-c648-4aa0-8063-65d38d03370f>、
  <https://support.microsoft.com/en-gb/office/create-a-3-d-reference-to-the-same-cell-range-on-multiple-worksheets-40ca91ff-9dcb-4ad1-99d2-787d0bc888b6>
- `#REF!` の UI 表示の例（ファイル上の形の根拠ではない）: <https://coddy.tech/docs/excel/ref-error>
- PR #14: `core-remove-part/crates/ooxml-core/src/package.rs`（`plan_removal`: `HasOwnRelationships`、
  `RegistryPart`、`StillReferenced` を `read_part(src)` の現内容で判定）。
- Phase1 lexer: `impl-xlsx-phase1/crates/ooxml-xlsx/src/formula/lexer.rs`（L120 エラーリテラル、L446–451
  `Sheet!#REF!`）。
- POI test-data（`p2/poi`、366 ファイル）: `dataValidationTableRange.xlsx`、`StructuredRefs-lots-with-lookups.xlsx`、
  `WithChartSheet.xlsx`、`61495-test.xlsm`、`workbookProtection_workbook_revision_protected.xlsx`、`54206.xlsx`、
  `62629_target.xlsm`、`46535.xlsx`、`1_NoIden.xlsx`。
- プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr27/probe/locs.py`
