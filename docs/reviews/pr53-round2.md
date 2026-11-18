# PR #53 レビュー（Opus, round 2）

対象: `impl/xlsx-phase3` @ `33b4771`（round 1 は `2886e5b`）。round 1 の指摘（`docs/reviews/pr53-round1.md`）への修正 `29b32eb` と、その後の main の merge、`33b4771` のテスト追加を見た。
照合したもの: ADR-0009（このブランチの版。implementation note 付き）、`sheet_ops/{mod,names,scan}.rs`、`tests/common/residual.rs`、`tests/phase3_review1.rs`、`xtask/src/emit.rs`、PR 本文。

リポジトリは変えていない（この文書だけ untracked で置いた）。作業はすべて `scratchpad/review-53r2-55/` で、`git clone` したコピーを使って行った。

- `pr53/crates/ooxml-xlsx/tests/review53r2.rs`: 新しいプローブ（Q1〜Q5）。round 1 のプローブ `review53.rs`（P1〜P9）と sweep `examples/r53sweep.rs` もそのまま再実行した。
- `scripts/mut2.py`: round 1 の mutant R1〜R34（コードが変わった R1、R20、R33 はパターンを付け替えた）と、round 2 の修正箇所に対する新しい mutant X1〜X23（20 個）。`phase3` + `phase3_review1` + `phase3_smoke` + `--lib` を `fault-injection` 付きで回した。結果は `m1.txt`〜`m4.txt`。
- `sweep53.txt`: POI corpus（`p2/poi`、366 ファイル、`47dcff76`）。

## 結論（先に）

**未収束（Blocker 1）。**

round 1 の B1、M1、M2、m1〜m7 は直っている。round 1 のプローブ P1〜P9 はすべて期待どおりの結果（書き換えか拒否）になり、corpus の数（rename 793/809、delete 615）も PR 本文どおり再現した。panic、reopen の不一致、外部参照と解析できない VML 以外の残留は 0 件である。

ただし **B2 の修正に穴が 1 つ残っている。** 生の名前の場所（`worksheetSource/@sheet`、`rangeSet/@sheet`、`dataRef/@sheet`）を探すときに、**XML をデコードする前のバイト列**で絞り込んでいる。そのため、シート名に `&`、`<`、`"` を含むと（Excel は属性値でこれらを必ずエスケープする）、pivot のデータ元は書き換えられず、rename は `Ok` を返す。consolidation の拒否も素通りになる。`P&L` や `R&D` のような名前で起きる。このライブラリ自身が `P&L` に rename した出力を、もう一度 rename しただけでも起きる（R2-B1）。

修正は 1 行で済む見込みである（絞り込みをデコード後に行うか、絞り込みをやめる）。そのほかは Minor と Nit だけである。

## Blocker

### R2-B1 [Blocker] rename: 生の名前の絞り込みが XML デコード前なので、`&` などを含むシート名の pivot のデータ元が古いまま `Ok` になる

`mod.rs:990-993`（`rewrite_raw_sheet_names`）:

```rust
let bytes = self.pkg.read_part(&p)?;
if !fold(&String::from_utf8_lossy(&bytes)).contains(&of) {
    continue;
}
```

`of` はデコード済みの名前（`r&d`）である。一方、part のバイト列では `sheet="R&amp;D"` になっている。そのため一致せず、part は丸ごと飛ばされる。この後に続く本来の判定（`n.attr(...)` でデコードした値を比べる）は正しいが、そこまで届かない。closed-world scan は sheet-prefix 形しか見ないので、この場所は scan でも捕まらない（ADR §6 の「Blind spots」）。

**再現**（`review53r2.rs` の `q1_raw_names_with_entities`、`q5_rename_to_entity_name`）:

| 入力 | 操作 | 結果 |
|---|---|---|
| シート `R&D`、`<worksheetSource ref="A1:B3" sheet="R&amp;D"/>` | `rename_sheet("R&D","New")` | `Ok`（書き換え 0 件）。`sheet="R&amp;D"` が残る |
| 同上、consolidation の `<rangeSet sheet="R&amp;D"/>` | rename | `Ok`（拒否されるべき場所）。残る |
| 同上、ワークシートの `<dataRef sheet="R&amp;D"/>` | rename | `Ok`（拒否されるべき場所）。残る |
| `sheet="It&apos;s"`、`sheet="D&#97;ta"` | rename | `Ok`、残る |
| `sheet="Data"` | `rename("Data","P&L")` の後に `rename("P&L","Back")` | 1 回目は `sheet="P&amp;L"` を正しく書く。2 回目は `Ok` だが `P&amp;L` が残る（`a<b"c`、LF を含む名前でも同じ） |

delete 側（`check_tables_and_pivots`）は part を解析してから比べるので、`R&D` の delete は `SheetIsPivotSource` で正しく拒否される。rename 側だけの穴である。

corpus（366 ファイル）には、`&` を含む名前のシートが pivot のデータ元になっている例はない。ただし、`&amp;` や `&quot;` を含むシート名は 2 ファイルにある（`CVLKRA-KYC_Download_File_Structure_V3.1.xlsx`、`xssf-enum.xltx.xlsx`）。「参照が古いのに `Ok`」は、round 1 の B2 と同じ種類で、同じ場所の問題である。そのため Blocker とする。

**修正案**: 絞り込みを、`crate::escape::decode` でデコードした後のテキストで行う。または、絞り込みをやめて、`XmlDocument::parse` の後の判定だけにする。part の数は多くないし、`pivotCacheDefinition` と、`dataConsolidate` を持つワークシートに限ってもよい。

**回帰テスト**: 上の表の 5 行。特に「`P&L` に rename してから、もう一度 rename する」往復を入れるとよい。

## Minor

### R2-m1 [Minor] scan の `.rels` の許可が、rewriter より広い（`TargetMode="External"` の `#Sheet!`）

`scan.rs:68` は、`.rels` の `Relationship/@Target` を、`TargetMode` にかかわらず許可する。一方 `hyperlink_targets`（`mod.rs:815`）が書き換えるのは `Internal` だけである。そのため、`<Relationship … Target="#Data!A1" TargetMode="External"/>`（自ブックの場所を指す、fragment だけの相対 URI）は、rename では `Ok` のまま残る。`delete_sheet_strict` でも数えられない（`Ok`）。

ADR §6 の表は「Internal relationships」と書いているので、表にない場所が、黙って通ることになる。B1 と同じ「scan の許可は rewriter が届く経路だけ」という原則に合わせて、External のときは `#` で始まらないもの（他のファイル `other.xlsx#Data!A1`）だけを許可し、`#…` は拒否するのがよい。Excel がこの形を書くかは未確認で、corpus には 0 件である（External の `#` target は 0）。そのため Minor とする。

### R2-m2 [Minor] M2 の解決が 1 段だけ（名前が名前を指す場合）

`check_tables_and_pivots` の `names_on_sheet` は、テキストが直接 `Data!` を含む名前だけを集める。`Src = Data!$A$1:$B$3`、`Src2 = Src` のとき、`<worksheetSource name="Src2"/>` で `delete_sheet("Data")` を呼ぶと、結果は `Ok` になる。`Src` は `#REF!` になり、pivot のデータ元は黙って壊れる（Q1 の最後）。

名前の推移閉包をとるか、`@name` が解決した名前のテキストを（Phase1 の lexer で）再帰的にたどるのがよい。まれな形なので Minor とする。

### R2-m3 [Minor・テスト] 生の名前の場所のテストが ASCII の名前だけ。synthetic fixture にない表の行

- `pivot_source_sheet_follows_a_rename`、`consolidation_sources_refuse_a_rename`、`pivot_source_through_a_defined_name_refuses_delete` の名前は、すべて `Data`/`data` である。R2-B1 を捕まえられなかったのはこのためである。4a の oracle は sheet-prefix 形しか見ないので、生の名前の場所は、場所ごとのテストだけが頼りになる（ADR の「Blind spots」）。
- ADR §6 の表には、synthetic fixture（`rows()`）にない行がある。
  - chart の `c15:sqref`（`c15:fullRef`/`formulaRef`）
  - chartEx の `cx:f`
  - 2007 の `cfvo/@val`
  - x14 のスパークラインと CF の `xm:f`（DV の `xm:f` しかない）
  - `totalsRowFormula`
  - `controlPr/@linkedCell`
  - `cdr:sp/@textlink`

  プローブ（Q2）では、どれも `UnrewritableReference` で正しく拒否された。ただ、行として固定しておけば、将来「書き換える」に変えたときの回帰を防げる。

### R2-m4 [Minor] `_xHHHH_` を文字として含む名前への rename が、読み戻しと合わない

PR #55 のレビューで見つかったものを、こちらで再現した（`review53r2.rs` の `q6_xstring_rename`）。

- `rename_sheet("A", "x_x0041_y")` を呼ぶと、`sheet/@name` には ST_Xstring で保護した `x_x005F_x0041_y` が書かれる（`mod.rs` の `escape::encode`）。
- ところが、その直後の `sheet_names()` も、reopen した後の `sheet_names()` も、`x_x005F_x0041_y` を返す。続けて `rename_sheet("x_x0041_y", "Z")` を呼ぶと `NoSuchSheet` になる。
- LF（`_x000a_`）は decode されて往復する。つまり、ローダーの decode が `_x005F_` の保護を外していない（または、encode と decode の対応がずれている）。

渡した名前と違う名前が一覧に出るので、API として一貫していない。ADR §6 の「ST_Xstring: 一致は `_xHHHH_` の decode 後に行う」に合わせて、ローダーの decode を encode の逆にそろえ、往復のテストを足すべきである。めったにない名前なので Minor とする。

## Nit

- **生の名前の書き込み**: `worksheetSource/@sheet` は `set_sheet(new)` で書く。LF を含む名前は `sheet="x&#10;y"` になる。`sheet/@name` 側は ST_Xstring の `_x000a_` で書く（ADR §6「ST_Xstring」）。`worksheetSource/@sheet` も `ST_Xstring` なので、そろえたほうがよい。読む側（`fold(sheet)`）も `_xHHHH_` をデコードしていない。
- **外部の pivot ソース**: `worksheetSource sheet="Data" r:id="rId1"`（他のブック）があると、`delete_sheet("Data")` は `SheetIsPivotSource` で拒否される（rename は正しく除外する）。拒否が増える側の問題である。
- **externalLink**: `xl/externalLinks/*.xml` は scan で `Other` になる。そのため、他のブックの `definedName/@refersTo="=Data!$A$1"` があると、同じ名前のシートの rename と delete が `UnrewritableReference` で拒否される。oracle（`residual.rs`）は externalLinks を「参照ではない」としているので、両者がずれている。拒否が増える側である。
- `webPublishItem/@sourceObject`（ワークシートの `webPublishItems`）は生の名前で、表にない。rename では `Ok` のまま残る。ADR の「未知の生の名前の場所は見逃す」という既知の blind spot の範囲で、corpus には 0 件である。表か「Not rewritten」に 1 行足しておくとよい。`dbPr/@command` の `[Data$]` は、ADR で「Not rewritten」と明記されている。
- ブランチは origin/main（#52、#54 など）より遅れている。`git merge-tree` では衝突なしに merge できる。

## round 1 の指摘の確認

| 指摘 | 状態 | 確認 |
|---|---|---|
| **B1** scan の許可 | 直った | `allowed`（`scan.rs:56-71`）は（名前空間, 経路）で判定する。セルの `f` は `worksheet/sheetData/row/c/f`（深さ 5）だけ、chart は `c:f`/`c15:f` だけである。プローブ Q2 で、`xm:f`（DV、CF、スパークライン、`dateAxis` の `xm:f`）、`c15:sqref`、chartEx の `cx:f`、`xdr:sp/@macro`、`graphicFrame/@macro`、ctrlProp の `fmlaRange`、queryTable、pivot の計算フィールドの `formula`、workbook の `extLst` の中、を試した。どれも `UnrewritableReference` になる。`extLst` の中の SML `f` は census が `FormulaScanFailed` で止める。`c15:f`（`datalabelsRange`）と `mc:Fallback` の中の `c:f` は書き換わる。definedName のテキストは書き換わる |
| **B2** 生の名前 | **一部** | ASCII の名前なら、`worksheetSource/@sheet` は書き換わり（P2）、`r:id` 付きのものは残り、`rangeSet` と `dataRef` は拒否される。エンティティを含む名前で抜ける（R2-B1） |
| **M1** oracle | 直った | `residual.rs` は、出力のすべての part を走査する（XML でないものは UTF-8 と UTF-16LE で）。例外は docProps、shared strings、comments、persons、externalLinks、セルの値だけである。synthetic fixture は 15 行（それに加えて、どの行にも名前が 2 つある）で、rename と delete の両方を検査する。R2-m3 を参照 |
| **M2** `@name` 経由の pivot ソース | 直った | ブック全体の名前とシートローカルの名前の両方で拒否する（P2 の後半）。`name="Data!Src"` は scan で拒否される。名前の連鎖は R2-m2 |
| m1 app.xml | 直った | ADR の implementation note、各操作の rustdoc、issue #60（OPEN）がそろっている。本体は #55 |
| m2 strict | 直った | P9: 自分の `Print_Area` だけがあるシートは strict でも `Ok([])` になる。P6: 3-D の中間と `hyperlink/@location` は `SheetInUse { count: 1 }` になる。Internal の `#Data!A1` も数える（External は R2-m1） |
| m3 TableInUse | 直った | 同じシートの中だけの使用は `Ok` になる。`tableSlicerCache`、DV、`displayName` は拒否される（テストあり） |
| m4 `move_child` | 直った | `LensMut::move_child` は削除され、#44 の `move_in_by` を使っている。プローブ Q4 では、4 シートの全 16 通りの `(from, to)` を試した（`<sheet>` の間に空白の item あり）。順序、`localSheetId`、`activeTab` はすべて正しかった。`move_writes_sheet_elements_verbatim` でバイト一致も見ている |
| m5 `firstSheet` | 直った | move では変えず、`activeTab` 以下に収める。属性がなければ書かない。add と delete では位置としてずらす（`33b4771` のテスト） |
| m6 PR 本文 | 直った | 数は再現した（下記）。Known gaps も事実と合う（R2-B1 を除く） |
| m7 `Element` 接尾辞 | 直った | `ELEMENT_SUFFIXED` で固定してある。`"sml::Bogus"` を足すと、`cargo xtask codegen --check` は名前の変化を報告して失敗した。元に戻すと通る |
| Nit `[0]Sheet!` | 直った | 拒否される（`names.rs:194-200`、テストあり） |
| Nit 解析できない chart | 直った | その chart が名前に触れていれば拒否する（テストあり） |

## corpus（POI 366 ファイル、`r53sweep`）

| 操作 | Ok | 拒否 |
|---|---|---|
| rename | **793** | 16（共有ブック 3、反復計算 3、ActiveX 2、`UnrewritableReference` 8） |
| delete | **615** | 194（`LastVisibleSheet` 146、`SheetHasUnsupportedParts` 22、`NotAWorksheet` 10、`SheetIsPivotSource` 5、`TableInUse` 3、`UnrewritableReference` 3、共有ブック 3、反復計算 2） |
| add（先頭／末尾） | 330 / 330 | 2 / 2 |
| hide | 660 | 149 |
| 逆順 | 327 | 5（`ReversesThreeDReference` 3） |

PR 本文の数とすべて一致した。panic と reopen の不一致は 0 件だった。docProps 以外の残留は rename で 8 件、delete で 6 件あった。内訳は `[n]Sheet!` の外部参照（`46535.xlsx`、`bug67784.xlsx`、`StructuredRefs-lots-with-lookups.xlsx`）と、`BrNotClosed.xlsx` の解析できない VML である。round 1 の B1 と B2 で挙げたファイル（`56644.xlsx`、`61652.xlsx`、`LIBRE_OFFICE-128382-0.xlsx`、pivot の 5 ファイル）は、拒否されるか、正しく書き換わる。生の `@sheet` の残留も 0 件だった。

## mutant

54 個を回した（round 1 の R1〜R34 と、新しい X1〜X23 のうち 20 個）。

- **R1〜R34**: 26 個が死んだ。生き残ったのは R2、R3、R4、R8、R10、R11、R19、R26 の 8 個で、PR 本文が「同値、または拒否が増える側」と論じた一覧と一致する。round 1 で「意味がある」とした生き残り（R1、R9、R12〜R18、R20〜R22、R24、R27、R29〜R34）は、すべて死んだ。
- **X1〜X23**（20 個）: 13 個が死んだ。B1 の経路の判定、B2 の書き換え、拒否、`r:id` の除外、M2、m2、m3、m5、`c15:f`、chart の解析失敗、`<` のエスケープ、`[0]` を壊すものである。生き残った 7 個は次のとおり。
  - X1（セルの `f` の深さ 5 の検査を外す）: PR 本文の N1 と同じもの。census の `FormulaScanFailed` が先に止めるので、同値である。
  - X3（scan の名前空間の比較を外す）: 経路の検査が残るので、`xm:f` は経路で拒否される。chart の中の c/c15 以外の `f` だけが通るようになるが、現実の chart には無い。ほぼ同値である。
  - X22（`v` をどこでも許可）、X23（`hyperlink/@location` を `hyperlinks` の外でも許可）: 該当する形が現実に無いので、ほぼ同値である。
  - **X9**（strict が `#Sheet!` の relationship target を数えない）: テストがない。プローブ Q3 では、Internal の `#Data!A1` で `SheetInUse { count: 1 }` になることを確かめた。動作は正しいが、固定されていない。
  - **X13**（workbook part では、どの要素のどの値でも許可）: テストがない。プローブ Q2 の「workbook の `extLst` の中の `Data!A1`」は、元のコードでは拒否される。この mutant の下では `Ok` になり、名前が残る。B1 と同じ種類の「許可が広すぎる」退行を捕まえるテストが、workbook part にない。
  - **X12**（`.rels` では、どの属性でも許可）: `Id` や `Type` が sheet-prefix 形になることは現実にないので、影響は小さい。

X9 と X13 のテスト（workbook の `definedName` 以外の場所に置いた名前が拒否されること、strict が Internal の `#Sheet!` を数えること）を、R2-m3 とあわせて足すことを勧める。

## 判定

**converged: no**

merge の前に必要なもの:

- **R2-B1**: 生の名前の絞り込みを、XML デコード後に行う（またはやめる）。エンティティを含む名前で回帰テストを足す。

R2-m1〜m3 と Nit は、この PR で直すことを勧めるが、merge を止める理由にはしない。R2-B1 の修正は局所的なので、次の round はその差分とテストの確認だけで足りる見込みである。
