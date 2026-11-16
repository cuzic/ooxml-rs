# ADR-0014 レビュー（Opus, round 1）

対象: PR #35 `docs/adr/0014-xlsx-phase4-range-features.md`（Proposed, rev 1）、`spikes/xlsx-phase4/`。
照合先: ADR-0001/0002、ADR-0003 rev 4 と Phase1 実装（`impl-xlsx-phase1`）、ADR-0006 rev 2（`plan-xlsx-phase2`）、
ADR-0009 rev 1 とその round-1 レビュー（`plan-xlsx-phase3/docs/adr/0009-opus-review-round1.md`, B1–B3）、
進行中のコア transaction API（`core-add-part`, `crates/ooxml-core/src/package/add.rs` の `RegistryPlan`）。

独立検証（すべて `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr35/`）:

- POI `test-data/spreadsheet` を `47dcff769bb395034dddfd59c4609659b6dbc6a4` で取り直した（`dl/poi`、368 ファイル）。
  解析には `python3 -I` の独自プローブ `probe/p.py` を使った。PR のスクリプトもこのコピーで再実行した。
- SHA-512 シート保護ハッシュを `probe/h.py`（hashlib）で検算した。
- rust_xlsxwriter 0.102.0（crates.io tarball）、openpyxl 3.1.5（PyPI sdist）、POI @ 47dcff76 のソースを取り直し、
  ADR が依拠する行を照合した（`dl/rx`、`dl/op`、`dl/poi`）。

## 結論（先に）

範囲機能を要素単位で局所編集し、byte-exact オラクルで確かめるという骨格は正しい。merge の既定拒否、x14 の保持、
CF の `max+1`、SHA-512 を既定にする判断も妥当で、ライブラリ引用も主要部分は正確だった（末尾の照合表）。

ただし、**このままでは壊れたファイルを書く箇所が 1 つ**ある。また、**Phase3 への依存が、round 1 で不健全と判定された
ADR-0009 rev 1 の primitive と書き換え表に乗っている**。

1. **B1:** `set_autofilter` が既存の `_xlnm._FilterDatabase` を再利用しない。Excel は autofilter を外しても
   この名前を残す。そのため、コーパスでは autoFilter の無いシートに名前だけ残ったファイルが 3 つある。そこへ新しく
   書くと、同じスコープに同名の定義名が 2 つでき、Excel の修復対象になる。
2. **B2:** §12 の「ここで作る数式サイトはすべて ADR-0009 §6 が rename で書き換える」は誤り。`cfvo/@val`（`type="formula"`）が
   漏れている。関係追加も、ADR-0009 の `add_relationship` と `check_*` 双子を前提にしている。後者は ADR-0009 B2 で
   不成立とされ、コアでは `RegistryPlan` に置き換えが進んでいる。

Major は 5 件ある。

- merge 後にアンカーへ書けない（Phase0 のガードが範囲全体を拒否する）。
- CF の他シート参照を 2007 形式で書けてしまう。
- `remove_autofilter` が行を隠したまま残す。
- 保護の置き換えで旧 `password` が残る（コーパスの「シートに legacy は無い」は誤り）。
- allowed-diff の列挙が不完全。

## 判定

**Accepted after Minor only: no.** B1–B2 と M1–M5 を反映して round 2 に進む。B2 は ADR-0009 rev 2 の内容に依存する。

---

## Blocker

### B1. `set_autofilter` が `_xlnm._FilterDatabase` を重複させる

**根拠（POI @ 47dcff76, `probe/p.py`）:**

| ファイル（Excel 製） | `_FilterDatabase` | シートの `autoFilter` |
|---|---|---|
| `57523.xlsx`（14.0300） | `localSheetId=0`、`=1` の 2 個 | 無し（`sortState` だけ） |
| `bug66827.xlsx`（12.0000） | `localSheetId=0` | 無し |
| `absolute-anchor-over-empty-sheet.xlsx`（16.0300） | `localSheetId=0` が `#REF!`、**ブック範囲**のものも `#REF!` | 無し |

- Excel は autofilter を解除しても名前を消さない。§7 は「名前を書く（writes）」としか書いておらず、既存の名前を探す
  手順が無い。上の 3 ファイルで `set_autofilter` を呼ぶと、同じ `localSheetId` に 2 個目ができる。定義名は
  スコープ内で一意でなければならず、Excel は修復ダイアログで名前を落とす。
- POI は `getBuiltInName(BUILTIN_FILTER_DB, sheetIndex)` を再利用している（`XSSFSheet.java` `setAutoFilter`, L4417–4441）。
  ADR の参照表にもそう書いてあるのに、Decision に反映されていない。
- シートに `autoFilter` が既にある場合の `set_autofilter` も規定が無い。`ref` だけ差し替えると、`filterColumn/@colId`
  （範囲の左端からの相対列）が別の列を指すようになる。POI はこの形で古い条件を残す。

**推奨:**

- シートローカル（`localSheetId` = 位置）の `_FilterDatabase` が 1 個あれば、テキストをその場で書き換える
  （allowed diff は名前のテキストと `hidden` だけ）。2 個以上あれば `Err(DuplicateBuiltInName)` にする。
  ブック範囲の `_FilterDatabase` は触らない。
- 既存の `autoFilter` があるときの扱いを決める。`filterColumn`/`sortState` を持つものは `Err(AutoFilterExists)`、
  持たないものは `ref` の書き換えだけを許す。
- オラクル 1 に上の 3 ファイルを入れる。

### B2. Phase3 依存が ADR-0009 rev 1 の不健全な部分に乗っている

**(a) 数式サイトの漏れ。** §12 は「DV, CF, x14 `xm:f`, `location`, `_FilterDatabase` は ADR-0009 §6 が書き換える」と
している。しかし ADR-0009 §6 の表は `cfRule/formula` しか持たず、**`cfvo/@val`（`type="formula"`）** が無い。

- コーパスでは `NewStyleConditionalFormattings.xlsx` が実例（`probe/p.py` の `cfvo_formula`）。
- §6 自身も colorScale/dataBar/iconSet を「2007 形式で書く」としている。したがって Phase4 が新しく作るサイトを、
  Phase3 の rename が黙って取りこぼす。
- ADR-0009 レビュー B3 は、同じ漏れと「未知の場所への安全網が無い」ことを指摘したまま未解決である
  （`plan-xlsx-phase3` は 2ff6879 から更新が無い）。

**(b) 関係追加。** §4 は ADR-0009 の `add_relationship` と「plan で各 guard を check」に依存する。

- ADR-0009 レビュー B2 は、`check_*` 双子を個別に呼ぶ方式では複数手の plan を検証できない、と判定した。
- コアは代わりに `RegistryPlan` を実装中である（`core-add-part` `package/add.rs` L1–31, L449–, L757）。
  `add_relationship` が plan 内で Id を返し、`Package::apply` は `StalePlan` 以外では失敗しない。
- ハイパーリンクは「関係 Id を予約する → シート XML に `r:id` を書く」の 2 手で、autofilter は workbook.xml と
  シートの 2 パーツにまたがる。ADR はどちらについても、Id の取得元と、適用の順序・原子性を書いていない。

**(c) シート削除時の形。** §12 は「cross-sheet DV/CF は `#REF!` になる（ADR-0009 §5）」としている。§5 の形は `#REF!A1`
だが、ADR-0009 レビュー M1 は、Excel が書くのは裸の `#REF!` で、`#REF!A1` は Phase1 lexer が受け付けない見込みだとしている。

**推奨:**

- ADR-0014 に「Phase4 が作る数式・参照サイト一覧」を自前の表として置く。少なくとも次を含め、それぞれ数式構文か生の参照かを書く。
  - DV `formula1/2`、x14 DV `x14:formula1/2/xm:f`
  - CF `cfRule/formula`、`cfvo/@val`（formula）、x14 `xm:f`、`x14:cfvo/xm:f`
  - `hyperlink/@location`
  - `_FilterDatabase`
- ADR-0009 rev 2 の書き換え表と残留言及オラクル（ADR-0009 レビュー M7）がこの表を包含することを、Accept の条件にする。
- 関係追加は `RegistryPlan` を前提に書き直す。順序は次のとおり。
  1. plan で Id を得る。
  2. Layer 2 の編集を plan に含めて検証する。
  3. `apply` → in-place 書き込みの順で適用し、失敗したら poison する。
- 「ADR-0009 `add_relationship`」への参照は外す。
- `#REF!` の形は ADR-0009 rev 2 に従うと明記する。

---

## Major

### M1. merge 後のアンカーに書けない。§4 の `opts.text` とも矛盾する

**根拠:** Phase0/1 の実装は `ctx.merges.iter().find(|m| m.contains(at))` で、アンカーを含む範囲全体を
`Err(MergedCell)` にしている（`impl-xlsx-phase1/crates/ooxml-xlsx/src/edit.rs` L103–105）。

- §3 はこのガードを「引き続き全 merged cell を拒否」と明記している。そのため `merge()` 後はアンカーの値を
  変えられない。
- §4 の「merged range ではアンカーにのみハイパーリンク可、`opts.text` は Phase0 経由で書く」は、このガードで常に失敗する。

**推奨:** Phase4 でガードを「非アンカーセルへの書き込みのみ拒否」に緩め、ADR-0003 の guard 表を更新する（Phase0 の
挙動変更として明記する）。非アンカーへの書き込みは引き続き拒否し、隠れ値を作らない。

### M2. CF の他シート参照を 2007 形式で書けてしまう

- §6 は「他シート参照を持つルールは x14 専用で、書き込みは範囲外」と言いながら、数式は「§5 の規則に従う」とし、
  §5 は他シート参照を解決して許可している。そのため `Sheet2!$A$1>0` の expression ルールが 2007 の `cfRule/formula` に
  書かれる。
- Excel 2010 以降は他シート参照の CF を x14（`xm:f`）で書く。EPPlus は「この形式（2007）の数式は自シートしか
  参照できない」として他シート参照を x14 へ移している（DV について。出典参照）。2007 形式の他シート参照 CF を Excel が
  受理する根拠は、ADR にも本レビューにも無い。

**推奨:** CF の `formula` と `cfvo/@val` に他シート参照（定義名経由は除く）が含まれる場合は
`Err(CrossSheetReference)` にする。XL で 2007 形式の受理が確認できたら緩める。DV は Q2 として扱う（下記）。

### M3. `remove_autofilter` がフィルター状態を半分残す。シート単位の `sortState` を消す

- §7 がフィルター条件の書き込みを外した理由は「シートが自分自身と食い違う」ことだった。解除にも同じ理由が当てはまる。
  `filterColumn` がある状態で解除すると、除外された行は `row/@hidden="1"` のまま残る。`sheetPr/@filterMode="1"` も
  残り、利用者には「データが消えた」ように見える。
- §7 は「その範囲を参照する `sortState` も消す」としている。しかしシート直下の `sortState` は、autofilter と独立した
  「最後の並べ替え」の記録である（`57523.xlsx`、`bug66827.xlsx` は autoFilter 無しで `sortState` を持つ）。
  `autoFilter` の子の `sortState` とは別物で、消す理由が無い（T2 の不要な拡大）。

**推奨:**

- `filterColumn` があれば `Err(FilterActive)` にする。行の再表示まで含めるなら、別 API
  （`remove_autofilter_showing_rows`。どの行を戻したかを返す）にする。
- 解除時は `sheetPr/@filterMode` を消す（allowed diff に追加する）。
- 消すのは `autoFilter` 要素（とその子）と `_FilterDatabase` だけにし、シート直下の `sortState` は残す。

### M4. 保護の置き換え: シートにも legacy `password` がある

**根拠:** ADR の表は「no legacy `password` on sheets」としているが、ピン留めコミットでは次の実例がある。

- `57181.xlsm`（Excel 14.0300）は 3 シートとも `<sheetProtection password="EBF6" sheet="1" objects="1" scenarios="1"/>` を持つ。
- `CVLKRA-KYC_Download_File_Structure_V3.1.xlsx` は、SHA-512 のシートと `password="B5EC"` のシートが混在している。

§11 には、既に保護されたシートへの `protect_sheet` の挙動が書かれていない。属性単位で上書きすると、旧
`password` と新 SHA 属性が同じ要素に並び、どちらで検証されるかが未定義になる。

**推奨:** `protect_sheet`/`protect_workbook` は、既存要素の **ハッシュ系属性をすべて置き換える**。対象は `password`、
`algorithmName`、`hashValue`、`saltValue`、`spinCount`（ブックは `workbookPassword` と `workbook*`）。フラグは呼び出し側の
指定で上書きし、未知の属性（`revisions*` など）は残す。`legacy_hash` を選んだときは SHA 系を消す。

### M5. byte-exact の allowed-diff 列挙が不完全

§2 の列挙は、§3–§11 自身が書く変更を含んでいない。このままでは、正しい実装がオラクルで落ちるか、実装者が
その場その場で許可を広げることになる。漏れているのは次の変更である。

- `sheetPr/pageSetUpPr/@fitToPage`（§10）。`sheetPr` 自体の作成も含む。
- `sheetPr/@filterMode`（M3）。
- `sheetFormatPr` の作成（`defaultRowHeight` は `use="required"`、`sml.xsd` L2225）。
- `workbook.xml` の `definedNames` コンテナの作成（B1）。
- シートの `.rels` パーツの作成（と `rels` の `Default` が無い場合の扱い）。
- x14 DV 削除時の `x14:dataValidations/@count`、空になった `ext` の除去（§6 は CF についてだけ書いている）。
- `add_conditional_format_first` での **x14-only `x14:cfRule/@priority`** の振り直し（§6 は「every other rule」としか
  書いておらず、`extLst` 内も含むか曖昧）。
- `sheetView` の作成と `selection` の置き換え（§8）。
- `rowBreaks/colBreaks` の `count`/`manualBreakCount`。
- `merge_clearing` に伴う `calcChain.xml` の除去（Phase1 由来）。

**推奨:** 機能ごとに「触れてよい要素・属性・パーツ」の表を ADR に置き、オラクルはその表から生成する。

---

## Minor

- **m1 URL の扱い（§4）。** 「scheme 必須、そのまま保存」は、Excel 自身が書くリンクを拒否する。
  - `WithMoreVariousData.xlsx`（Excel 12）は `Target="WithVariousData.xlsx"`（相対ファイル）を持つ。
  - RX は `mailto:`、相対パス、UNC、`C:\` を区別し、空白や `%`、`"`、`<>[]^{}` を escape する。ファイルリンクでは
    `#` を `%23` にする（`url.rs` L387–465, 469–486, 523–558）。
  - 推奨: RX と同じ分類と escape を規定する。`#fragment → location` は URL とファイルリンクに限り、`Link::Location`
    だけを Phase1 の tokenizer で検証する。

- **m2 ハイパーリンク削除の根拠が古い。** `core-add-part` の ADR-0001 note は、参照の無い関係の削除
  （`RegistryPlan::remove_relationship`、`StillReferenced` 検査付き、`add.rs` L863–910）を認めている。関係を残す結論は
  維持してよい。複数の `hyperlink` が同じ `r:id` を共有しうるうえ、無害だからである（Excel 製のコーパスでは未使用の
  hyperlink 関係が 0 件で、Excel は保存時に掃除する）。ただし理由は「widening」ではなくこの点に書き換える。

- **m3 DV の細部（§5）。**
  - inline list 内の `"` は `""` に二重化する（RX `data_validation.rs` L561–565）。
  - 項目内の `,` は表現できないので拒否する。
  - x14 DV 削除時の count と ext の扱い（M5）。
  - `formula1` の 255 文字制限は、範囲参照の式にも適用する（EPPlus wiki）。

- **m4 CF の細部（§6）。**
  - `Rule` に `stopIfTrue` を入れる（コーパス 16 規則）。
  - 新規ルールの置き場所を決める（同じ `sqref` の既存ブロックに追記するか、新ブロックか。局所性からは新ブロックを推奨）。
  - `BrokenX14Link` は「片割れが既に壊れている」場合なので、2007 側の削除はむしろ修復になる。拒否ではなく許可し、
    戻り値で報告する案を検討する。
  - `conditionalFormatting/@pivot="1"` のブロックは削除対象外にする。

- **m5 ADR-0006 との接続。**
  - `dxfs` の規定は ADR-0006 の **§5 item 8**（L649）にあり、§8 は vendor extensions である。参照を直す。
  - `dxfs` 内に `mc:AlternateContent` があるファイル（`style-alternate-content.xlsx`）では、ADR-0006 §2 により追加が
    拒否される。そのエラーを §13 に通す。
  - `dxf/numFmt/@numFmtId` はパッケージ全体で一つの id 空間なので（ADR-0006 Context 10）、同じ割り当て器を使うと明記する。

- **m6 `cols`（§9）。**
  - `cols` は `maxOccurs="unbounded"`（`sml.xsd` L2124）。複数ある場合と、既存の `col` 範囲が重なる場合
    （openpyxl 出力）は拒否する。
  - 新しく作る `col` には **常に `width`** を書く。hidden や outline だけを設定する場合は、既定幅を書いて `customWidth`
    は付けない。コーパスの `col` 14,558 個は全部 `width` を持ち、RX も常に書く（`worksheet.rs` L19949–19982）。
  - `sheetFormatPr` を作る場合は `defaultRowHeight` が必須になる。

- **m7 ペイン（§8）。**
  - `selection` を置き換えるときは、既存の `activeCell`/`sqref` を新しい active pane の `selection` に引き継ぐ。
  - `freeze_panes(0,0)` は `Err` にする。
  - `sheetView/@view="pageLayout"` では固定できないので拒否する。
  - コーパスには `selection` が 0 個の frozen ペインが 3 件あるので、読み取りは 0 個でも受理する。

- **m8 ページ設定（§10）。**
  - ヘッダー・フッターの 255 文字制限は、RX ではコード込みの**文字列全体**に掛かる（`worksheet.rs` L13592）。
    「セクションごと・コード除外」との差を XL で確認し、確認までは厳しい方（全体 255）を採る。
  - 印刷設定バイナリ（DEVMODE）を残したまま向きや用紙を変えた場合に、XML 属性が優先されることを XL 項目に入れる。

- **m9 保護（§11）。**
  - `hashValue`/`saltValue` は **base64**（`xsd:base64Binary`、`sml.xsd` L2890–2891）と明記する。
  - フラグは「1 = 保護（禁止）」で、既定値は属性ごとに異なる（`formatCells` は既定 true、`selectLockedCells` は既定 false）。
    API は「allow_*」で受け、反転して書くことを明記する。
  - オラクル 4 の未確定パスワードは **`pwd`**（POI `TestXSSFSheet.java` L1357–1358）。本レビューで、
    H(salt‖UTF-16LE("pwd")) の後に H(prev‖LE32 i) を 100,000 回繰り返して、ファイルの `hashValue` と一致することを
    確かめた（`probe/h.py`。iterator を先に置く順序では一致しない）。

- **m10 merge（§3）。**
  - 「`unmerge` 後の非アンカーは空のまま」は、隠れ値のあるファイル（`60512.xlsm`）では誤りで、値が再び表示される。
    docs にそう書く。
  - 「値がある」の定義を `v`、`is`、`f` のいずれかの存在とする。
  - PivotTable の `location/@ref` と queryTable の出力範囲との重なりも拒否する（Excel の UI も拒否する）。
  - 非アンカーの 1 セルだけに値がある場合に Excel が値をアンカーへ移すかどうかを XL 項目に入れる
    （`merge_clearing` が「Excel と同じ」と言えるか）。

- **m11 コーパス数値がピン留めコミットで再現しない。** PR の `range_survey.py --summary` を 47dcff76 の取り直しに
  掛けた結果は次のとおりで、表と食い違う。
  - 343 readable / 367
  - merge 43 ファイル / 54,468
  - CF 20 / 108
  - autofilter 3 / 3
  - `_FilterDatabase` 6 / 8
  - legacy シート保護 2 / 5

  さらに Fact 2 の「x14 DV 7 件はすべて他シートのリスト」は誤りで、`51850.xlsx` は `#REF!`、`61652.xlsx` は
  `Model!#REF!` である。README の「350 files」がどのコピーを指すかを明記し、表を再生成する。

---

## 推奨（Q1–Q4）

- **Q1（merge の既定）:** 現行どおり、**拒否が既定、`merge_clearing` はオプトイン** を維持する。消える値は Phase1 の
  依存無効化を伴うデータ損失なので、明示した呼び出しに限る。`MergeWouldHideValues { cells }` が対象セルを返すので、
  呼び出し側は判断できる。
- **Q2（他シートのリスト DV）:** 2007 形式で書くことは条件付きで可とする。
  - 根拠の状況: openpyxl・RX は 2007 形式で他シート参照を素通しで書く（RX `data_validation.rs` L638–643）。
    一方で EPPlus は「2007 形式の数式は自シートしか参照できない」として x14 へ移す。Excel 自身も x14 で書く
    （コーパスの `56644.xlsx`、KEIKAI-464）。2007 形式を Excel 2010 以降が修復なしに受理する一次資料は、見つからなかった。
  - したがって **XL を出荷ゲートにする**（2007 形式の他シートリストを Microsoft 365 Excel で開いて修復が出ないこと、
    ドロップダウンが動くこと、保存後にどちらの形式で書き直されるか）。
  - XL が通るまでは、他シート参照の DV を `Err(CrossSheetReference)` にする。回避策として、他シート範囲を指す定義名を
    `formula1` に使う方法を docs に書く（2007 形式で正当で、Excel も extLst へ移さない）。
- **Q3（CF の優先度）:** 現行どおり、**既定は `max+1`**、先頭への挿入は `add_conditional_format_first` を推奨する。
  局所性（T2）を優先し、Excel の UI 順は明示した呼び出しで得る。`_first` は x14-only の優先度も振り直すと明記する（M5）。
- **Q4（salt）:** salt は秘密ではないので、**呼び出し側が salt を渡せる公開オプション**（`Protection::with_salt([u8;16])`）を
  置き、既定は OS の CSPRNG にする。byte-exact オラクルとハッシュベクタの両方がこれを必要とする。spinCount も
  `with_spin_count` で可変にしてよい。上限は POI と同じく 10,000,000 とし、それを超える値は拒否する。

---

## 引用の再検証（独立に取得したソースで照合）

| 主張 | 照合結果 |
|---|---|
| RX: URL ≤ 2,080、tooltip ≤ 255 | `url.rs` L13, L363–375（一致） |
| RX: merge が非アンカーに書式付き空白を書く、重なりは `MergeRangeOverlaps` | `worksheet.rs` L5014–5050（一致） |
| RX: inline list ≤ 255 UTF-16 | `data_validation.rs` L555–576（一致。`"` の二重化もある） |
| RX: `_FilterDatabase` は local かつ `hidden` | `workbook.rs` L3648–3657（一致） |
| RX: legacy ハッシュは UTF-8 バイト | `utility.rs` L1435–1453（一致） |
| OP: worksheet の extLst を警告して捨てる | `worksheet/_reader.py` L323–329（一致） |
| OP: merge が非アンカーを `MergedCell` に置き換える | `worksheet.py` L605–614（一致） |
| OP: CF の priority は `max_priority+1`、未設定のものだけ | `formatting/formatting.py` L64–79（一致） |
| OP: margins 0.75/0.75/1/1/0.5/0.5、legacy は code point でゼロ埋め無し | `worksheet/page.py` L167、`utils/protection.py` L14–22（一致） |
| POI: merge は array formula と既存 merge の重なりを拒否 | `XSSFSheet.java` L338–368（一致） |
| POI: `removeHyperlink` は関係を残す | L3657–3659（`hyperlinks.remove` のみ。一致） |
| POI: 新規リンクのみ外部関係を追加 | `XSSFHyperlink.java` L151–160（一致） |
| POI: `_FilterDatabase` の再利用 | `XSSFSheet.java` `setAutoFilter`（一致。B1 の根拠） |
| POI: SHA-512 = H(salt‖UTF-16LE)、H(hash‖LE32 i)、salt 16 バイト | `CryptoFunctions.java` L123–150（`iteratorFirst=false`）、`XSSFPasswordHelper.java` L61–75（一致。実ファイルでも検算） |

## 出典

- ECMA-376 Part 1 Transitional `sml.xsd`（`reference/ecma-376-transitional-xsd/`）: `cols` maxOccurs unbounded（L2124）、
  `CT_SheetFormatPr/@defaultRowHeight` required（L2225）、`CT_Col`（L2238–2249）、`CT_Hyperlink`（L2744–2750）、
  `CT_SheetProtection`（L2887–）。
- EPPlus wiki, "Data validation – Exceptions" (EPPlus 5): <https://github.com/EPPlusSoftware/EPPlus/wiki/Data-validation-Exceptions/Hyperlinks>
- ZK Keikai tracker KEIKAI-464（Excel が他シートのリスト DV を x14 で書く例）: <https://tracker.zkoss.org/browse/KEIKAI-464>
- PhpSpreadsheet issue #2149（x14 の他シート DV が読めなかった件）: <https://github.com/PHPOffice/PhpSpreadsheet/issues/2149>
- XlsxWriter docs, "Working with Data Validation"（title 32、message 255、list 255）: <https://xlsxwriter.readthedocs.io/working_with_data_validation.html>
- rust_xlsxwriter 0.102.0（crates.io）、openpyxl 3.1.5（PyPI sdist）、Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4`。
- Phase1 実装 `impl-xlsx-phase1/crates/ooxml-xlsx/src/edit.rs` L103–105。コア `core-add-part/crates/ooxml-core/src/package/add.rs`。
- プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr35/probe/p.py`、`probe/h.py`。
