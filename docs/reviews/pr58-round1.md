# PR #58 レビュー（Opus, round 1）

対象: `impl/xlsx-phase4` の HEAD `bafe55e`（`f089c73` codegen、`1eeefa8` 実装、`66c34a5` テストと文書、main `4f8e294` のマージ）。
基準: ADR-0014 rev 3（Accepted）、owner decisions Q1–Q4。

検証は、worktree を scratchpad の `review-pr58/repo` に `git clone` したもので行った（worktree は読んだだけで、変更したのはこのファイルだけ）。

- `cargo test -p ooxml-xlsx --release`: 全バイナリ ok。
- corpus sweep（POI `47dcff76`、`OOXML_XLSX_CORPUS`）を再実行した。
  - 結果は 335 packages、1,278 ok。拒否の内訳は `set_autofilter: Core` 57、`AutoFilterExists` 1、`merge: MissingReference` 3、`InvalidReference` 1 で、PR 本文と同じだった。
  - PR 本文は 334 / 1,274 と書いている。使ったディレクトリのファイル数が 1 つ違うだけと思われる。
- probe: `review-pr58/repo/crates/ooxml-xlsx/tests/probe58.rs`（11 件）。main と比べる probe: `review-pr58/mainclone/.../probe_main.rs`。
- mutants: 自分で 18 個を作った（`review-pr58/scripts/mut.sh`）。PR の 30 個とは別のものである。
- 暗号: `review-pr58/scripts/hash.py`、`cmp.py`（Python `hashlib` / `base64` と比べた）。

## 結論（先に）

- 判定: **converged: no**。Blocker は 0、**Major は 3 件**（M1〜M3）、Minor は 7 件。
- 中心の機能は正しい。
  - merge の拒否集合、`merge_clearing` を「全部確認してから適用する」こと、anchor の緩和、`_FilterDatabase` の再利用と `localSheetId`、SHA-512 の保護、いずれも ADR どおりに動く。
  - 暗号は独立実装と一致した。SHA-512 は長さ 0〜600 バイトの 601 個、base64 は同じく 601 個で、どちらも不一致 0。Excel fixture の `hashValue` は、`hashlib` で別に計算しても password `pwd`、hash‖iterator の順でだけ一致する（iterator‖hash では一致しない）。
- **M1:** PivotTable の `location` を、sheet context を読むときに先に読むようになった。そのため、壊れた pivot part を持つブックは **`Workbook::from_bytes` 自体が失敗する**。main では開けて編集もできたので、退行である。
- **M2:** allowed-diff oracle の行が path 単位でしか効かない。そのため `set_autofilter` が**ほかの definedName の本文や `hidden` を変えても**、`merge_clearing` が **`calcPr` のほかの属性（`calcMode="manual"` など）を変えても** oracle を通る。mutation check はシートの `sheetData` 属性しか変えていない。
- **M3:** `mc:AlternateContent` による `set_autofilter` の拒否は、ADR §13 が想定した「隣にあるとき」ではない。`maxOccurs=1` slot では、親のどこにあっても拒否される。
  - corpus の `workbook.xml` にある AlternateContent 81 件は、すべて Excel 2013+ の `x15ac:absPath` である。そのうち 61 件は `definedNames` を持たない。
  - つまり「名前を定義していない、ローカルに保存された最近の Excel ファイル」の大半で、`set_autofilter` が使えない。
  - コードのバグではない。ただし ADR の判断（素通しにする）は、規模が分からないまま下されている。owner の判断か、core の緩和が必要である。

## 確認した項目（問題なし）

| 項目 | 確認内容 |
|---|---|
| merge の拒否集合 | 次をすべて拒否し、拒否したときはバイトが変わらない（`phase4.rs` と sweep の `assert_unchanged`）: 単一セル、merge・table・pivot との重なり、query table のあるシート、array・data table・spill、`vm`、隠れる値と式（`v`/`is`/`f`）。`CellRange::parse` は逆順の範囲（`B2:A1`）を正規化する。`$F$1:$G$2` と小文字の `h1:i1` も受け付けた。 |
| `merge_clearing` | 各セルは Phase1 の `CellOp::Clear` として消される。依存先のキャッシュは**別シートのものも**消える（probe: `T!A1 = S!C3+S!D3` の `<v>` が消え、`fullCalcOnLoad="1"` になった）。shared master は、範囲内の anchor 以外のセルにあれば `SharedFormulaMaster` で拒否される。master が anchor にある場合や、member だけを消す場合は Ok で、master の `ref` は残る（Excel が受け付ける形）。「全部確認してから」も成り立つ: `iterate="1"` で 2 つめのセルにだけ依存先があるとき、`Err(IterativeCalculation)` を返しバイトは変わらない（probe）。dry-run とその後の本番の間で判定が変わる経路（census block、hand-off、macro sheet）は見つからなかった。7,999 セルで 70 ms で、ほぼ線形だった。 |
| anchor の緩和 | `edit.rs:120` の `m.first != at`。セッション中の `merge` / `unmerge` が `SheetContext.merges` を同じ apply の中で更新する。ADR が列挙したテストと文書の変更（`r6_merged`、`edit.rs` の 6×3×2、README、ADR-0001 と ADR-0003）は、すべてそのとおりに変わっている。style-only の書き込みは main にまだ setter がないので先送りで、PR 本文にそう書いてある。 |
| `_xlnm._FilterDatabase` | `localSheetId` は `<sheets>` の中の位置（chartsheet も数える）である。sheet 1 で作ると `localSheetId="1"`。同じセッションで set → set → remove → set としても、名前は 1 つのまま再利用された。workbook scope の名前には触れない。シート名の escape: `'My Sheet'`、`'O''Brien'`、`'日本語'` を確かめた。シートの並べ替え API はまだない（Phase3 のシート操作は main にない）ので、並べ替え後の `localSheetId` は今の時点では問題にならない。Phase3 を実装するときに、ADR-0009 の側で付け直す必要がある。 |
| autofilter の拒否と remove | `AutoFilterExists`（`filterColumn` または `sortState` の子）、`TableOverlap`、`DuplicateBuiltInName`、`NameInUse`（式と definedName の両方）。remove は名前とシートの `sortState` を残し、`filterMode` を消す。 |
| 保護 | `sheet="1"`。flag の極性と schema の既定値は `CT_SheetProtection` と一致した（`objects` / `scenarios` / `selectLockedCells` / `selectUnlockedCells` の既定が false、ほかは true）。既定で `objects="1" scenarios="1"` を書く（Excel と同じ）。legacy の `password` は消え、未知の属性（`x:foo`）は残る（probe）。spin count の上限は `InvalidSpinCount` で、乱数の失敗は `Error::Random`。どちらも変更の前に返る。 |
| `std::random` | 1.99.1 では `std::random::random` が `E0658 unstable library feature random` になり、`DefaultRandomSource` は存在しない。依存を足す理由としては正しい（ただし m3）。 |
| 自分の mutants 18 個 | 15 個は検出、2 個は生き残り（m2）、1 個はコンパイルエラーだった。検出された主なもの: `localSheetId` を無視、`hidden` を書かない、sortState を busy と見ない、query table、空になった container、セッションの unmerge、`filterMode`、shared master、flag の既定値 2 種、legacy `password` を消さない。 |

## Major

### M1. pivot part を先に読むので、ブックが開けなくなる（退行）

`workbook.rs:270-274`（`load_context`）。

```rust
let doc = self.pkg.xml_part(&p)?;
let loc = doc.root().elements().find(|e| e.name().local() == "location").and_then(...);
pivots.push(loc.ok_or_else(|| Error::Malformed(format!("{p}: PivotTable without location/@ref")))?.to_owned());
```

- `load_context` は、最初に見えるシートについては `Workbook::from_bytes` の中で呼ばれる（`:242`）。
- probe `pivot_without_location_blocks_open`: `location` のない `pivotTableDefinition` が 1 つあると、`from_bytes` が `Err(Malformed("…PivotTable without location/@ref"))` になる。
- 同じ入力は、main（`4f8e294`）では開けて、A1 の書き込みも `Ok` だった（`probe_main.rs`）。
- pivot part が well-formed でないときや、root が違うときも、同じように開けなくなる。
- 正常なファイルでも、シートを開くたびに、そのシートの pivot part（大きいことがある）をすべて parse する。merge を使わない Phase0 と Phase1 の利用者にも、そのコストがかかる。
- この情報は merge の重なり判定にしか使わない。しかも `plan_merge` はすでに「読めない location は重なるとみなす」と書いている（`range.rs:98`）。読み込みでの扱いと判定での扱いが食い違っている。
- **修正案:**
  - `pivots: Vec<Option<String>>` にする（読めない場合は `None`）か、`load_context` では relationship の target だけを覚えておき、最初の `merge` で遅延して読む。
  - `None` は今の `map_or(true, …)` と同じく、重なるとみなして拒否する。
  - 回帰テスト: location のない pivot part があっても、開けることと A1 に書けることを確かめる。

### M2. allowed-diff oracle が path 単位で、ほかの要素の変更を見逃す

`tests/allowed_diff.rs:27-28, 34-39`、`tests/common/p4.rs` の `judge`。

- `workbook("definedNames/definedName", Kind::Content)` と `EditAttrs(&["hidden"])` は、**すべての** definedName に効く。`Create` の multiset key も、`Content` 行があると開始タグだけになる（`content_free`）。
  - probe `oracle_other_name_text_change`: 出力の `_xlnm.Print_Area` の本文を `S!$Z$9` に変え、`Other` に `hidden="1"` を足した。結果は **`Ok(())`** だった。
- `workbook("calcPr", Kind::Container)` は開始タグを丸ごと切り取る。そのため `EditAttrs(&["fullCalcOnLoad"])` は意味を持たず、calcPr のどの属性が変わっても通る。
  - probe `oracle_calcpr_other_attr`: `calcId="1" calcMode="manual"` に変えた結果は **`Ok(())`** だった。
  - 比較として、`sheetData` の A1 の値を変えたものは、正しく Err になった。
- `a_sibling_change_fails_the_oracle` が変えるのは、編集したシートの `sheetData` の属性だけである。workbook.xml 側の行にも、許された path の中の「許されていない変更」にも、mutant がない。
- ADR §2 は「行の外の変更はバグとして oracle が報告する」と約束している。この PR でいちばん重い保証が、`set_autofilter` と `merge_clearing` の workbook.xml 側では効いていない。
- **修正案:**
  - `Allowed` に要素の選択条件を足す。`definedName` なら `@name` が `_xlnm._FilterDatabase` で、かつ `@localSheetId` がそのシートのもの、という条件にする。
  - `calcPr` は、入力にないときだけ `Create`、あるときは `EditAttrs(["fullCalcOnLoad"])` だけにする（ADR-0003 §7 の意味どおり）。
  - mutation check に次を足す。workbook.xml のほかの名前の本文、calcPr のほかの属性、`mergeCells` の中の既存の `mergeCell` の `ref`、`sheetProtection` の未知の属性を落とすこと。

### M3. `mc:AlternateContent` による `set_autofilter` の拒否が、ADR の想定より広い

- ADR §13 は「core の `UnresolvedSibling` が **`mc:AlternateContent` の隣**で出たら素通し」と書いている。
- 実際の拒否はこうなっている。`definedNames` は `maxOccurs=1` の slot なので、`check_insert_by` が全体の greedy 割り当て（`lens.rs` の `place`）を通り、**親のどこかに** AlternateContent があれば拒否する。
  - probe: `<sheets>` の**前**にある `x15ac:absPath` だけで `Err(Core(UnresolvedSibling(workbook)))` になった。挿入位置は `<sheets>` の後なので、隣ではない。
- 規模（`scripts/ac.py`、`workbook.xml` 349 個）:
  - AlternateContent は 81 個で、**81 個すべてが `x15ac:absPath`** だった。Excel 2013 以降でローカルに保存したファイルには、ほぼ必ず付く。
  - そのうち 61 個は `definedNames` を持たない。sweep の 57 件の拒否はすべてこれだった。
- つまり、名前のない最近の Excel ファイルでは、`set_autofilter` はほぼ使えない。安全側の拒否であり、バイトも変えない。それでも、ADR を受け入れたときの前提（まれな隣接ケース）とは違う。
- **解消の方法（どちらか）:**
  - (a) owner が既知の制限として受け入れる。そのうえで、ADR §13 の文言（「隣」ではなく「`maxOccurs=1` slot では親のどこでも」）と、README の `set_autofilter` 行に、数字つきで書く。
  - (b) core を緩める。AlternateContent の `Choice` と `Fallback` の子が、すべて対象とは別の slot の既知要素で、しかもすべての読み方で対象 slot より前（または後）にある場合は、slot の数え方にも順序の判定にも影響しない。だから、拒否しないで位置を決められる。absPath は `workbookPr` の後で `sheets` より前なので、この規則で解ける。
  - (b) は core の ADR-0002 §4.3 に関わる。そのため、この PR では (a) にして、(b) を issue に分けるのが現実的である。

## Minor

- **m1. 同じ `ref` の merge が重複していると、セッションの状態と tree がずれる。**`range.rs:233-252`。
  - `unmerge` は tree からは 1 つだけ消すのに、`merges.retain(|m| *m != r)` は**両方**消す。
  - probe: `A1:B2` が 2 回あるファイルで `unmerge("A1:B2")` を呼ぶと、tree にはまだ `A1:B2` が残る。それでも B2（anchor ではない）への書き込みが `Ok` になり、新しい隠れ値ができた。
  - 入力自体が Excel の修復対象（重なり）なので、Minor とする。
  - 修正案: 最初の 1 つだけ消す。または、同じ ref の要素をすべて消す（tree とセッションを同じにする）。
- **m2. mutant が 2 つ生き残った。**
  - (K4) `check_array_overlap` から `_ if c.dynamic => c.range` を消しても、どのテストも落ちない。spill（dynamic array）と重なる merge の拒否にテストがない。
  - (K5) `merge_clearing` の dry-run の loop（`check_edit_cell`）を消しても落ちない。PR が売りにしている「全部確認してから」を確かめるテストがない。上に書いた `iterate="1"` の probe をそのままテストにできる。
- **m3. `getrandom = "0.3"` ではなく `"0.4"` にするべきである。**
  - `zip 9` がすでに通常の依存として `getrandom 0.4.3` を引いている（`cargo tree -i getrandom@0.4.3`）。0.3 を足すと、同じ crate の版が 2 つになる。
  - `"0.4"` に変えると、`getrandom::fill` の API はそのままでビルドでき、増える crate は 0 だった。
  - あわせて、ほかの依存と同じように `[workspace.dependencies]` から引くかどうかも揃えたい。
- **m4. `Protection` の `#[derive(Debug)]` が password を平文で出す。**
  - probe: `Protection { password: Some("hunter2"), … }`。ログに出やすいので、手書きの `Debug` で `password: Some("<redacted>")` にしたい。
- **m5. 空の password でも hash を書く。**
  - `password("")` を指定すると、`""` の SHA-512 hash を書く。Excel の「シートの保護」で password を空にすると、hash は書かれない。
  - `""` を password なしとして扱うか、そのまま書くことを文書に明記するか、どちらかに決めたい。
  - password なしで `with_spin_count(u32::MAX)` を指定しても、黙って Ok になる。値を使わないので害はない。
- **m6. corpus sweep の範囲が狭い。**
  - sweep は、最初のシートで遠い範囲への `merge` と `set_autofilter`、それに `protect_sheet` と `unprotect_sheet` しか試していない。
  - 既存の merge 54,468 個に対する `unmerge`、既存の autoFilter 3 個に対する `remove_autofilter`、使われている範囲への `merge` と `merge_clearing`（拒否の経路と Phase1 との合成）は、corpus では試されていない。
  - 少なくとも次を足したい。sheet ごとの最初の merge の `unmerge`、`remove_autofilter`、used range の先頭 2×2 への `merge`（拒否はバイト不変）。
- **m7. ADR と実装の文言がずれている。**
  - ADR §3 は「query table の出力範囲と重なるとき」と書いている。実装と README は「query table を持つシートなら、どの範囲でも拒否」である。安全側なので問題はないが、ADR に注記がほしい。
  - `TRUE` のような名前のシートは、`TRUE!$A$1:$B$2` と引用符なしで書かれる。Excel が引用符を付けるかどうかは確かめていない。将来 XL で確かめる項目として残しておきたい。

## 判定

- **converged: no**。
- M1（開けなくなる退行）と M2（oracle の穴）はコードの修正で閉じる。
- M3 は owner の判断（a）か、core の緩和（b）が必要である。
- Minor は次の round までに対応するか、follow-up に回せばよい。
