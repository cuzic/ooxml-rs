# PR #53 レビュー（Opus, round 1）

対象: `impl/xlsx-phase3` @ `2886e5b`（#38 `core/add-part` @ `f9862b5` の上に積んである）。
照合したもの: ADR-0009 rev 4（main @ `a0f2717`。このブランチには入っていない）、`sheet_ops/{mod,names,scan}.rs`、`workbook.rs` の rebuild と poison、`edit.rs`、`graph.rs`（`Everything` の閉包）、`ooxml-core` の `LensMut::move_child`、`xtask/src/emit.rs`、生成された `sml.rs`、`tests/phase3*.rs`、PR 本文。

リポジトリは変えていない（この文書だけ untracked で置いた）。作業はすべて `scratchpad/review-38r3-53/pr53/` で、`git clone` したコピーを使って行った。

- `src/`: プローブ。`tests/review53.rs`（P1〜P9）と、corpus sweep 用の `examples/r53sweep.rs`。sweep は実装とは独立の残留スキャン付きで、出力のすべての XML part の全テキストと全属性を見る。
- `mutrepo/` と `scripts/mut.py`: 新しい手書き mutant 34 個（R1〜R34）。結果は `mut.txt`。
- `sweep2.txt`: POI corpus（`p2/poi`、366 ファイル、`47dcff76`）の結果。

## 結論（先に）

**未収束（Blocker 2、Major 2）。**

骨格は ADR-0009 のとおりにできている。

- 1 操作につき `RegistryPlan` が 1 つ、part ごとの clone も 1 つである。
- poison は apply の前に立て、rebuild の後に外す。
- quoting と 3-D span の書き換えは正しい。
- 共有されている image や chart は残る。
- PR 本文の corpus の数（rename 796 Ok、delete 613、add 330/330）は再現した。panic は 0 件、reopen の不一致も 0 件だった。

しかし、**closed-world scan に穴が 2 つあり、そこを通ると rename や delete が `Ok` を返したまま古い参照を残す。** これは ADR の「黙って見逃すよりは拒否する」に正面から反する。POI corpus の実ファイルでも起きている（rename 8 ファイル、delete 2 ファイル）。

- **B1**: scan は、`f` という local name の要素なら名前空間を問わず「書き換える場所」とみなす。しかし実際に書き換えるのは `sheetData/row/c/f` と `c:f` だけである。そのため、x14 の `xm:f`（データの入力規則、条件付き書式、スパークライン）と chart の `c15:f` は、書き換えも拒否もされない。
- **B2**: pivot の `worksheetSource/@sheet` は、`!` を含まない生の名前である。そのため scan では見えず、書き換えもされない。PR 本文は「拒否する」と書いているが、実際には `Ok` になる。

テストの oracle がこの 2 つを捕まえられないのは、ADR 4a の「出力のすべての part を走査する残留 oracle」と「場所ごとに 1 つ参照を置いた synthetic fixture」が実装されていないからである（M1）。新しい mutant 34 個のうち 28 個が生き残った。

app.xml（ADR-0009 §11）を実装していない点は、**merge を止める理由にはしない**（m1。条件付き）。

## Blocker

### B1 [Blocker] scan が local name `f` を名前空間なしで許可する。`xm:f` と `c15:f` は書き換えも拒否もされない

- `scan.rs:38-43`: `PartKind::Worksheet` は `("f", None)` を、`PartKind::Chart` は `("f", None)` を許可する。比べているのは local name だけである（`scan.rs:86`）。
- `mod.rs:832-845`: 書き換えるのは `sheetData/row/c/f` だけである。
- `mod.rs:895`: chart は `CHART_NS` の `f` だけを書き換える。

その結果、次のものが scan を通ってしまい、そのまま残る。

- ワークシートの `extLst` の中の `x14:dataValidation/x14:formula1/xm:f`、`x14:cfRule/xm:f`、`x14:sparkline/xm:f`
- chart の `c15:f`（`c15:datalabelsRange`、`c15:filteredSeriesTitle` など）

**再現**（`tests/review53.rs`）:

- P1: シート A に x14 の入力規則 `<xm:f>Data!$A$1:$A$3</xm:f>` を置き、`rename_sheet("Data","New")` を呼ぶ。結果は `Ok`（書き換え 0 件）で、`Data!` が残る。`delete_sheet("Data")` も `Ok` で、`Data!` が残る。スパークラインでも同じだった。
- P3: chart に `c:f` と `c15:f` を置いて rename する。`c:f` は書き換わるが、`c15:f` には `Data!$C$1:$C$3` が残る。

**corpus**（独立した残留スキャン、`sweep2.txt`）:

| ファイル | 場所 | 件数 | rename | delete |
|---|---|---|---|---|
| `56644.xlsx` | x14 の入力規則、`lists!…` | 5 | `Ok` | `Ok` |
| `61652.xlsx` | x14 の入力規則、`Model!#REF!` | — | `Ok` | `Ok` |
| `LIBRE_OFFICE-128382-0.xlsx` | スパークライン、`'Basic features'!…` | 10 | `Ok` | — |

ADR は §6 の表で `xm:f` を「書き換える場所」とし、`c15:f` も表に入れている。PR 本文は「Known gaps」で、`xm:f` を含む DV/CF は「書き換えずに拒否する」としている。実際には、どちらにもなっていない。

**修正案**: 許可は (名前空間, 経路) で判定する。たとえば、`SML_NS` の `f` で、親が `c`、そのさらに親が `row` のものだけ、chart では `CHART_NS` の `f` だけを許可する。それ以外の `f`（`xm:f`、`c15:f`）は拒否にする。`xm:f` と `c15:f` を書き換える実装を入れるなら、それでもよい。

回帰テストには、P1 と P3 の形を入れる。M1 の oracle も直せば、これは自動で捕まる。

### B2 [Blocker] 生の名前の場所（`worksheetSource/@sheet` など）は、書き換えも拒否もされない

- scan は sheet-prefix 形（`!` 付き）しか探さない（ADR §6「Blind spots」）。
- ADR は、生の名前の場所を「表だけで扱う」としている。表にある `worksheetSource/@sheet`、`rangeSet/@sheet`、`dataRef/@sheet` は、rename で書き換えるか、明示的に拒否する必要がある。
- ところが実装は、そのどちらもしていない。`check_tables_and_pivots` が `worksheetSource` を見るのは delete のときだけである（`mod.rs:1159-1175`）。rename にはこの検査がない（`mod.rs:714-768`）。

**再現**: P2。`pivotCacheDefinition` に `<worksheetSource ref="A1:B3" sheet="Data"/>` を置き、`rename_sheet("Data","New")` を呼ぶ。結果は `Ok`（書き換え 0 件）で、`sheet="Data"` が残る。pivot のデータ元は、存在しないシートを指すことになる（`refreshOnLoad` があれば、開いた時点で更新に失敗する）。

**corpus** では 5 ファイル（7 個の pivot cache）で rename が `Ok` になり、古い `@sheet` が残った。

- `54436.xlsx`
- `64450.xlsx`
- `ExcelPivotTableSample.xlsx`（ADR の XL 9(a) の対象ファイル）
- `LIBRE_OFFICE-116306-0.xlsx`
- `WithChartSheet.xlsx`（3 個）

PR 本文の「pivot `worksheetSource/@sheet` は拒否する」とは、事実が違う。

**修正案**: いちばん小さい修正は、rename の前に、pivot cache の `worksheetSource/@sheet`、`consolidation/rangeSets/rangeSet/@sheet`、`dataConsolidate/dataRefs/dataRef/@sheet` を fold して比べ、一致すれば `UnrewritableReference` で拒否することである。`r:id` を持つ外部ソースは除く。ADR Q6 は「pivot のあるシートでも rename できる」ことを前提にしているので、本命は書き換えの実装である。

## Major

### M1 [Major・テスト] 残留 oracle が ADR 4a より狭い。新しい mutant の 28/34 が生き残る

- `tests/phase3.rs:64-95` の残留 oracle が見ているのは、calamine のセル式と `definedName` だけである。
- ADR 4a は、次を要求している。
  - **出力のすべての part** の全テキストと全属性を走査すること（バイナリは UTF-8/UTF-16LE で）。
  - §6 の表の**各行に参照を 1 つずつ置いた synthetic fixture** を用意すること（解析できない VML や、図形ハイパーリンクの `#'Sheet 1'!A1` も含む）。
- この 2 つがないので、B1 と B2 はすり抜けた。

新しい mutant（`scripts/mut.py`）では、`phase3` + `phase3_smoke` + `--lib` を `fault-injection` 付きで回し、34 個のうち 6 個が死んだ。生き残ったもののうち、意味のあるものは次のとおり。

- R9: 解析できない part を素通しにする（ADR が名前を挙げた blind spot）
- R12: `displayName` を見ない
- R13: `CustomViewReferencesSheet` の検査を消す
- R14: ActiveX の拒否を消す
- R15: `c:f` の splice で `&` をエスケープしない（壊れた XML を書くのに、テストが落ちない）
- R16 と R17: `sheetId` の high-water から `customWorkbookView` と calcChain を外す
- R18: manual モードで `fullCalcOnLoad` を立てる
- R19: 書き換え後の lex invariant を外す
- R20: strict を lenient にする（`delete_sheet_strict` のテストがない）
- R21: ワークシートのフォルダを決め打ちにする
- R22: 行の高さを決め打ちにする
- R24: 32 文字の名前を許す
- R26: 3-D の順序検査で `pa != pz` を外す
- R27: census block を無視する
- **R29: move で `localSheetId` を remap しない**
- R30: hide で `tabSelected` を残す
- R31: `KeepStaleCaches` を無視して strip する
- R32: 子要素を持つ `c:f` を許す
- R33: 解析できない pivot cache を素通しにする
- **R34: cascade 表にない関係型を無視する**（pivot table や OLE のあるシートを消してしまう）

同値、または拒否が増える側の mutant: R2 と R4（`''` は状態を 2 回反転させるので、結果は同じ）、R3、R8、R10、R11。

PR 本文の「25 個すべて死んだ」は、作者が書いた mutant についての話としては正しい。ただし、テストが ADR §8 の要求（「1 つの場所クラスを見落とす mutant は落ちなければならない」）を満たしていないことは、上の結果が示している。

**必要なこと**: ADR 4a の全 part 残留 oracle と synthetic fixture を入れる。R1 と R12〜R34 のうち意味のあるものには、テストを足す。

### M2 [Major] delete: `worksheetSource/@name` が、削除するシート上の**定義名**を指していても拒否しない

- ADR §5 は、「`@name` が、そのシート上の名前またはテーブルに解決されるなら拒否」と定めている。
- 実装が見ているのはテーブルだけである（`mod.rs:1169`）。

**再現**: P2 の後半。`Src = Data!$A$1:$B$3` を定義し、pivot cache を `<worksheetSource name="Src"/>` にして `delete_sheet("Data")` を呼ぶ。結果は `Ok` で、`Src` は `#REF!` に書き換わり、pivot のデータ元は黙って壊れる。

**修正案**: `@name` を定義名として解決する（ブック全体の名前と、シートローカルの名前）。参照先に削除するシートが含まれていれば、`SheetIsPivotSource` で拒否する。

## Minor

### m1 [Minor・文書] app.xml（ADR-0009 §11）を実装していない。merge を止める理由にはしない（条件付き）

P4 で確認した。add の後も `TitlesOfParts` は 1 シートのまま、`HeadingPairs` の数も古いままである。rename の後も、古い名前が残る。

判断と理由は次のとおり。

- app.xml は情報用である。Excel は開くときに使わず、保存するときに作り直す。calamine と LibreOffice も、シートの一覧をこれで決めない。古いまま残っても、データは壊れず、開けなくなることもない。
- Q4 の「構造を作り出さない」は、触らないことを禁じていない。ADR 自身も、形を認識できない app.xml は**丸ごと触らない**としている（L1）。今の状態は、すべての app.xml をその扱いにしたものにあたる。
- B1/B2 とは違い、「参照が壊れたのに `Ok`」という種類の問題ではない。

ただし、ADR rev 4 は Accepted で、§11 を Phase3 のスコープに含めている。そのため、次の 3 点を merge の条件とする。

1. ADR-0009 に implementation note を書く。§11 は後続 PR に回すこと、それまでの app.xml の扱いを記録する。
2. 各操作の rustdoc に「`docProps/app.xml` は更新しない」と書く。
3. 追跡用の issue を立てる。テストの残留 oracle では、`docProps/*` を「明示した例外」として列挙する。今は scan が黙って NonReference に分類しているだけである（`mod.rs:957`）。

### m2 [Minor] strict と、返す `RewrittenLocation` の意味がずれる

- **P9**: シート B にローカルな `_xlnm.Print_Area`（`B!$A$1:$C$10`）があると、`delete_sheet_strict("B")` は `SheetInUse { count: 1 }` を返す。その名前は削除とともに消えるのだから、これは「使われている」ことにはならない。印刷範囲のあるシートは、ほぼ strict で消せないことになる。
  - lenient の `delete_sheet` は、この消えた名前を「`#REF!` に書き換えた」と報告する。監査用の戻り値として正しくない。
  - 原因: `rewrite_everywhere` が、削除するシートのローカル名を除外していない（`mod.rs:808`、`1032`）。
- **P6**: 逆に、3-D span の中間にあるシート（`SUM(B:D!A1)` の C）や、`hyperlink/@location` でしか参照されていないシートは、strict でも `Ok` になる。値も結果も変わるのに、である。

ADR Q2 の「参照されているシートを拒否する」に合わせて定義し直し、テストを足すべきである（R20 も生き残っている）。

### m3 [Minor] `TableInUse` の判定

- **P9**: テーブルを使っている式が同じシートの中にしかなくても（`SUM(T1[x])`）、`TableInUse` になる。ADR §5 の対象は、ほかの場所で使われている場合だけである。拒否が増える側の問題である。
- ADR が挙げる `x15:tableSlicerCache/@tableId`、ほかのテーブルの `calculatedColumnFormula`、DV などは見ていない（`mod.rs:1148-1157`）。

### m4 [Minor・core API] `LensMut::move_child` に検査がなく、テストもない

`lens.rs:1199` の `move_child` は public だが、次のことをしない。

- slot や順序の検査（ADR-0002 の content model）をしない。
- `mc:AlternateContent` を拒否しない。
- 対象がテキストかコメントかを区別しない（item のインデックスで動かす）。

そのため、`<sheetData>` を `<sheetPr>` の前に動かす、といった不正な木を作れる。`ooxml-core` にテストは 1 本もない。#44 で、検査付きの同一 part 移動 `move_in_by` が main に入っている。

**修正案**: `move_in_by` を使うか、`move_child` を `pub(crate)` にする。公開を続けるなら、`place_skipping` で slot を検査し、要素であることを確かめ、core にテストを足す。

`<sheets>` の並べ替えという今の使い方については、確認した結果は正しい（P7）。

- 空白の item を挟んでも、手で追った前方と後方の移動の位置計算は合う。
- 動かした `<sheet>` はバイト単位で同じである。
- 変わったエントリは `workbook.xml` だけである。

### m5 [Minor・ADR/XL] move で `firstSheet` を順列で写すと、タブの帯がスクロールする

P7 は、3 シートで `move_sheet(0,2)` を呼んだものである。もともと `firstSheet` 属性はなかった（= 0）のに、移動後は `firstSheet="2"` が書かれる。結果として、Excel のタブの帯は先頭 2 枚が隠れた位置から始まる。

`firstSheet` は、タブの帯の表示位置である。シートそのものを指してはいない。ADR §7 の文面（「順列で remap する」）には従っているが、POI の `setSheetOrder` は `firstSheet` を動かさない。

ADR の側で「範囲に収める（`≤ activeTab`）だけにする」に改めるか、XL 項目に足すことを勧める（R1 も生き残っている）。

### m6 [Minor・文書] PR 本文の数と記述

- rename の拒否は「12」とあるが、内訳を足すと 13（3 + 3 + 2 + 1 + 1 + 1 + 2）である。sweep の結果も 13 で、796 + 13 = 809 だった。
- 「Known gaps」は、`xm:f` と pivot の `worksheetSource/@sheet` を「拒否する」と書いている。B1 と B2 のとおり、事実ではない。
- `rename_sheet` の rustdoc（`mod.rs:707-713`）は「charts … refuses」と書いているが、`c:f` は書き換えている。古い記述である。

### m7 [Minor・codegen] `Element` を付ける規則は公開 API を壊さないが、安定ではない

- origin/main と生成された `sml.rs` を比べた。公開されている型名、`impl` ごとの `pub fn` 名のどちらも、**消えたものは 0 件**だった。`ReadOnly` から `Edit` に移った型は、`ViewMut` が増えただけである。下流の crate では、`sml::WorkbookView` を `Workbook` の view として使い続けている（`workbook.rs:558`）。
- ただし、この規則は allowlist の中身で決まる。たとえば、将来 `x:CT_Sheet` を足すと、すでに公開されている marker の名前が `…Element` に変わる。また、規則はグループの中しか見ないので、`read` と `edit` のグループをまたぐ衝突は、codegen では捕まらず、rustc のエラーになって初めてわかる。
- `emit.rs` のコメントか、codegen の `--check` に「名前が変わったら失敗する」検査を入れておくとよい。

## Nit

- `[0]Old!A1`（自ブックを指す外部インデックス 0）は書き換えない（`split_prefix` が `[` で始まるものを外部として扱う）。一方 scan は `f` の中なので許可する。corpus には 0 件である。
- chart が quick-xml では読めるのに `XmlDocument::parse` では弾かれる場合（DOCTYPE、深すぎる入れ子など）、`rewrite_everywhere` は `continue` で飛ばす（`mod.rs:889`）。scan のほうは `c:f` を許可するので、黙って残る。Excel の出力では起きない。
- `sst/@count`、chart sheet の削除、VBA、名前 API が後回しであることは、PR 本文に書かれている（Phase2 の registry がまだ無いことも含めて）。

## 確認した点（問題なし）

- **1 操作 1 plan**: `commit` と `finish` は、`RegistryPlan` を 1 つだけ作る。clone は `Change::doc` で part ごとに 1 つで、`xml_part().clone()` から取る（R3-m5 a）。chart は `bytes` の側に入る。staged の内容を先に入れてから removal を行う。`set_relationship_target` は rename でしか使わず、`remove_parts` は delete でしか使わない（R3-m5 b）。calcChain は 2 回目の `remove_parts` で消す（R3-m6）。
- **poison**: apply の前に立て、`StructuralApply`、`StructuralRebuild`、`StructuralReindex` の 3 点で fault を入れられる。rebuild は open と同じ loader を使う。
- **quoting**（P3 の 16 個の名前）: `It's`、`A&B <x>`、`2024`、`R1C1`、`rc`、`Ünïcødé`、`a b`、`TRUE`、`x.y`、`A1B`、`Sheet!x`、`"q"`、`a;b`、`_xlnm`、`LOG10`、NBSP を試した。セル式、名前、`c:f` のすべてで正しく、3-D は span 全体を quote する。`c:f` は `&`、`<`、`>` をエスケープし、chart はどれも parse でき、calamine での読み戻しも一致した。LF を含む名前（P8）は、`sheet/@name` では `_x000a_`、`hyperlink/@location` では `&#10;` になり、reopen すると同じ名前に戻る。
- **共有 part**（P5）: 2 つの drawing が共有する image と chart は残り、削除したシートの drawing とその `.rels` は消える。
- **`Everything`**: `graph.dependents` は、seed が空でも `Everything` の閉包を必ず含める。そのため、add や rename で「未知のシートが解決するようになる」式の cache も消える（ADR §4、§6）。
- **`r:` の宣言**（#45）: 新しい `<sheet>` に `insert_by_with_ns` で宣言する。
- **POI corpus**（`r53sweep`）:

  | 操作 | 結果 |
  |---|---|
  | rename | Ok 796、拒否 13 |
  | delete | Ok 613 |
  | add（先頭） | 330 |
  | add（末尾） | 330 |
  | hide | 660 |
  | 逆順 | 327（`ReversesThreeDReference` 3） |

  panic は 0 件、reopen の fingerprint の不一致も 0 件だった。残留スキャンで実際に見つかったものは、B1 と B2 に挙げたものだけである。そのほかの検出は、外部参照（`[1]…`、`[3]…`）と、解析できない VML の誤検出だった。
- LibreOffice はこのホストにない（`soffice` がない）。そのため O3（再計算）は回していない。calamine による読み戻しは、`phase3` テスト（delete の CR）とプローブで行った。

## 判定

**converged: no**

merge の前に必要なものは、次のとおり。

- B1: scan の許可を (名前空間, 経路) で判定する。または `xm:f` と `c15:f` を書き換える。
- B2: 生の名前の場所を、rename で書き換えるか拒否する。
- M1: 全 part 残留 oracle、synthetic fixture、生き残った mutant を潰すテスト。
- M2: `worksheetSource/@name` が削除するシート上の定義名を指すとき、拒否する。

m1（app.xml）は、ADR の implementation note、rustdoc、issue の 3 点がそろえば、後続 PR に回してよい。
