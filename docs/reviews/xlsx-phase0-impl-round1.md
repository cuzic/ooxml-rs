# xlsx Phase0 実装レビュー（Opus, round 1）

## 対象/方針

- 対象: マージ済み PR #11（`crates/ooxml-xlsx/` の src, tests, examples, README, template）。`main` の `de5d6ae` 時点。
- 照合した仕様: ADR-0001（post-acceptance notes: first visible worksheet、要素名による formula 検出を含む）、ADR-0002、`crates/ooxml-core/README.md`、`docs/roadmap/xlsx.md` §4・§7、PR #11 本文の coordinator defaults と deviations。
- 方法:
  1. コードを読み、ADR と §7 の保証・拒否を一つずつ実装と対応させる。
  2. リポジトリのコピー（scratchpad、リポジトリ本体は無変更）に敵対的テスト `tests/adversarial.rs` を足して挙動を見る。
  3. 実装に変異を入れ、既存スイート（`--lib` と `edit`/`package`/`refusals`/`oracle`）が落ちるかを見る（mutation test）。
- フィクスチャはパースするだけにした。変異版は、scratch 上の `ooxml-xlsx` のソースを置換して作った。
- 重大度: Blocker/Major はデータ破損、黙った欠損、保証の違反、コア保証が未テストのもの。それ以外は Minor。

## 結論（先に）

実装は全体として堅い。

- first visible worksheet、QName による `f` 検出、merged/vm/table header の拒否、`MissingReference`、self-closing の row/cell、`x:` prefix、same-path save、テンプレートはどれも意図どおり動く。
- L1/T2/CR oracle も、わざと壊した出力を検出する。

収束を妨げる Major は 2 件。

- **F1**: 「`Err` なら何も変わらない」という保証が破れる経路がある。セルの中に `mc:AlternateContent` があると、`set_string`/`set_number` は値を消した**後で** `UnresolvedSibling` を返す。その後に保存すると元の値が黙って消える。
- **F2**: ADR-0001 R2-M3/R3-M1 の要である workbook 全体の formula 拒否がテストされていない。対象は 4 つ:
  - 別シートにだけある formula
  - macro sheet の formula
  - formula がなく calcChain だけがある場合
  - 対象外シートの scan 失敗

  実装は正しい（私の再現テストはすべて pass）。ただし、これらを壊す変異が既存スイートを**すべて生き残る**。

ほかに Minor が 5 件ある。PR に書かれた deviation はすべて許容できる。

**判定: converged = no**（Major 2 件）。

## Findings

### F1 (Major) — 拒否（`Err`）の前にセルを変更している: `mc:AlternateContent` を子に持つ `c`

**根拠**

- `crates/ooxml-xlsx/src/worksheet.rs:260-287`: `write()` は `preflight` の後、closure `f` の中で変更を始める。
- `worksheet.rs:341-343`（`set_number`）と `worksheet.rs:368-371`（`set_string`）: 先に `remove_data_type` / `remove_cell_value` / `remove_inline_string` が走る。その後の `get_or_insert_*` が `ooxml-core` の `place()` に入る。
- `crates/ooxml-core/src/lens.rs:341`: `place()` は兄弟に `mc:AlternateContent` があると `UnresolvedSibling` を返す。
- `preflight`（`worksheet.rs:235-257`）は、セルの子の配置が解決できるかを確かめていない。

**再現**（`tests/adversarial.rs::a1_err_after_mutation_inside_c`。`fixture_minimal` の A2 だけを差し替えた入力）:

```rust
let sheet = text(&input, "xl/worksheets/sheet1.xml").replace(
    r#"<c r="A2" t="s"><v>1</v></c>"#,
    r#"<c r="A2" t="s" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><v>1</v><mc:AlternateContent><mc:Fallback/></mc:AlternateContent></c>"#,
);
let mut wb = Workbook::from_bytes(rebuild(&input, &[("xl/worksheets/sheet1.xml", sheet)])).unwrap();
let r = wb.worksheet().unwrap().cell_mut(1, 0).unwrap().set_string("new");
assert!(r.is_err());
assert_eq!(wb.package().state("xl/worksheets/sheet1.xml"), Some(PartStateKind::Parsed { dirty: false })); // ← 失敗
```

観測結果:

```
A1 set_string result: Err(Core(UnresolvedSibling(... local: "c")))
A1 state after Err: Some(Parsed { dirty: true })
A1 A2 after Err + save: <c r="A2" t="inlineStr" ...><mc:AlternateContent><mc:Fallback/></mc:AlternateContent></c>
A1 A2 value after Err + save: Empty (was "Alice")
```

値を持たないセル（`<v>` なし）に `set_number` を書いても、同じように `t` が消えた後で `Err` になる。

**なぜ問題か**

README（`## Refusals (Err, nothing changed)`）、ADR-0001、roadmap §7.5 は「拒否なら何も変えない」を保証している。テストの `assert_unchanged` もそれを前提にしている。

呼び出し側が「失敗したセルは飛ばして保存する」という自然な書き方をすると、**元の値（"Alice"）が黙って消える**。`t="inlineStr"` で `<is>` のない空セルが保存されるからだ。

`c` の中の AC は Excel が書かない形なので、入力としては珍しい。ただし MCE（Part 3）は任意の要素内容に AC を許し、フィクスチャは信頼できない入力として扱う方針でもある。保証違反なので Major とする。

**提案**（どれか一つ）:

- (a) `preflight` で、セル（と、既存の row）の子に `mc:AlternateContent` があるか、slot 順が解決できるかを非変更で検査し、問題があれば先に `Err` を返す。core に `LensMut::can_insert::<C>()` のような dry-run API を足すのが筋がよい。
- (b) closure の順序を変え、失敗しうる操作（`get_or_insert_*`）を削除より前に行う。`v` と `is` は同じセルに共存できるので、この順序変更が可能。
- (c) 変更前に cell の `RawNode` を退避し、`Err` なら戻す（ADR-0003 rev.2 M2 の plan → validate → apply と同じ考え方）。

あわせて `a1_err_after_mutation_inside_c` を `refusals.rs` の回帰テストにする。

### F2 (Major) — workbook 全体の formula 拒否（ADR-0001 R2-M3/R3-M1）がテストされていない。変異が生き残る

**根拠**

- `crates/ooxml-xlsx/src/workbook.rs:268-295`（`formula_census`）。
- 拒否系のテスト `tests/refusals.rs:32`（`r1_to_r3_formula_present`）と `:135`（`malformed_shapes`）で使うフィクスチャや合成シートは、**すべて対象シート（sheet1）自身に `f` がある**。
  - `shared_formulas`、`MatrixFormulaEvalTestData`、`NewStyleConditionalFormattings` は calcChain と sheet1 の `f` の両方を持つ。
  - `xlookup` は sheet1 にも `f` がある。
- そのため次の 4 つの経路はどれも検証されていない:
  - 別シートにだけ formula がある場合（ADR が R2-M3 で挙げた `Sheet2!B1 = Sheet1!A1*2` そのもの）
  - macro sheet / dialog sheet の formula
  - calcChain だけで拒否する fast path（ADR の「calcChain を含む workbook はすべて拒否」）
  - 対象外シートの `FormulaScanFailed`

**変異テスト**（`tools/mutate.py`。既存スイートで実行）:

| 変異 | 既存スイート | 追加テスト |
|---|---|---|
| M1: formula scan を `*sheet1.xml` だけに限る | **SURVIVED** | `a2_formula_on_other_sheet_only` で KILLED |
| M2: calcChain の fast path を削除 | **SURVIVED** | `a2c_calc_chain_only` で KILLED |
| M3b: macro/dialog sheet を scan しない（CT 経路と rel 経路の両方） | **SURVIVED** | `a2b_formula_in_macrosheet_only` で KILLED |
| M19: `ScanFailed` を「formula なし」として扱う | **SURVIVED** | （未作成） |

**再現**（実装は正しく、追加テストは現行コードで pass する。抜粋）:

```rust
// a2: 51850.xlsx の sheet2 に <row r="99"><c r="A99"><f>Sheet1!A1*2</f><v>0</v></c></row> を挿入
assert!(matches!(wb.edit_refusal(), Some(Error::FormulaPresent { ref part }) if part == "xl/worksheets/sheet2.xml"));
// a2b: xlmmacro.xlsm の macrosheet の <sheetData/> → <row r="1"><c r="A1"><f>RETURN()</f></c></row>
// a2c: shared_formulas.xlsx の sheet1 から <f> をすべて除去（calcChain は残す）
assert!(matches!(wb.edit_refusal(), Some(Error::FormulaPresent { ref part }) if part.ends_with("calcChain.xml")));
```

**なぜ問題か**

Phase0 の「round-trip 編集」は「workbook のどこにも formula がないときだけ許す」と定義されている。その安全性を支えるのが、まさにこの 4 経路だ。いずれかが将来の変更で壊れても CI は通る。その場合、別シートの cached 値が古いまま残り、エラーも出ない（ADR が防ぐと明言した silent stale）。コア保証が未テストなので Major とする。

**提案**

- 上の `a2`/`a2b`/`a2c` を `refusals.rs` に移す。
- 次の 2 つを合成テストとして足す:
  - 対象外シートが scan できない場合（例: sheet2 を not-well-formed や UTF-16 BOM にする）に `Err(FormulaScanFailed)` を返し、読み取りは通ること
  - dialog sheet（CT `dialogsheet+xml`）

### F3 (Minor) — table header ガードの変種がテストされていない

**根拠**

- `workbook.rs:308-334`（`load_tables`）と `worksheet.rs` の preflight。
- 既存テストは `ExcelPivotTableSample` の単一表（`headerRowCount` 既定値 1）だけを使う。
- 変異 M4（先頭の表だけを見る）、M5（`headerRowCount` を無視して常に 1）、M18（複数行ヘッダの先頭行だけを見る）はすべて SURVIVED した。

**再現**: `a3_tables`。表 `T2`（`J5:K8`、`headerRowCount="2"`）を追加すると、次のようになる:

- J5 と K6 は `TableHeaderCell` になる。
- J7 と E2 は Ok になる。
- table1 を `headerRowCount="0"` にすると、E1 は Ok になる。

現行実装はすべて正しい。

**提案**: `a3_tables` を `edit.rs` の E11 の横に追加する。

### F4 (Minor) — 読み取りの空白 trim 規則がテストされておらず、継承された `xml:space` で calamine と食い違う

**根拠**

- `worksheet.rs:53`。変異 M12（trim しない）が SURVIVED した。
- 全フィクスチャで calamine と照合する `reads()` も落ちない。外側に空白を持ち、preserve されていない `t` がフィクスチャに存在しないためだ。
- `a5_shared_strings_edge` の `<si xml:space="preserve"><t> Alice </t></si>` では、本実装が `" Alice "`（XML の継承規則どおり）、calamine が `"Alice"` を返す。
- README と roadmap §7.7 は「calamine に従う」と書いている。
- rich run と `rPh` を無視する点は正しく動いた（`"Name"`）。

**提案**:

- 合成テストを 1 つ足す（preserve なしの外側空白は trim、継承された preserve は保持）。
- README の文言を「XML の `xml:space` 継承に従う。calamine は継承を見ない」に直す。

### F5 (Minor) — `c` の順序チェック（`OutOfOrder`）がテストされていない

**根拠**: `worksheet.rs:181`。変異 M11（列順チェックを削除）が SURVIVED した。`malformed_shapes` は row の逆順しか試していない。

**提案**: `<row r="1"><c r="C1"/><c r="A1"/></row>` への書き込みと読み取りが `OutOfOrder` になるテストを足す。

### F6 (Minor) — `sheetData` 直下の `mc:AlternateContent` に入った row は読み取りで「存在しない」と扱われる

**根拠**: `worksheet.rs:162`。`locate` は `Row` の QName だけを列挙する。`a4_rows_inside_alternate_content` では A1 の row を AC で包むと次のようになる:

- `cell(0,0)` は `Ok(None)` を返す。値は AC の中にあるのに、黙って「ない」と答える。
- 書き込みは `UnresolvedSibling` で正しく拒否され、変更もない。

**なぜ問題か**: ADR-0002 §4.2 が `MissingReference` で塞いだのと同じ種類の「silent None が値を隠す」問題だ。ただし読み取り専用で、入力も珍しいので Minor とする。

**提案**: `sheetData` か対象 row に AC の子があれば、読み取りも `Err(UnresolvedSibling)`（または専用の variant）にする。

### F7 (Minor) — 解釈できない `dimension/@ref` は黙って古いまま残る

**根拠**: `worksheet.rs` の `write()` 末尾にある `if let Some(Ok(range)) = … parse`。`a6_dimension_edges` で F9 に書き込んだ結果:

| 元の `ref` | 書き込み後 |
|---|---|
| `A1:D`、`A1:D2:E3`、`""` | 変わらない（壊れたまま） |
| `a1:d2` | `A1:F9`（正規化される） |
| `A1` | `A1:F9` |
| `A1:XFD1048576` | 変わらない（すでに含む） |

**なぜ問題か**: §4 の「P0 recompute」が黙って行われない。元から不正な入力ではあるので Minor とする。

**提案**: 解釈できない `ref` は `A1` 相当として union するか、書き込みを拒否する。どちらにするかを README に書く。

## 問題なしと確認したもの（抜粋）

- **formula の誤検出と見逃し**:
  - `x:f` は検出する。`formula`/`from`/`fill`/`filterColumn`/`xm:f` は検出しない（unit test）。
  - E3、E9、E13 は通る。
  - 別シート、macro sheet、calcChain だけの場合は正しく拒否する（F2 の追加テスト）。
- **first visible worksheet**:
  - hidden、veryHidden、macro sheet は飛ばす。
  - 関係（relationship）と CT を両方確認する。
  - `NoVisibleWorksheet` を返す。
- **merged**（anchor を含む）、**vm**、**`MissingReference`**:
  - 各変異（M10、M9、M17）は既存スイートで KILLED になる。
  - 別の row に `r` のない `c` があっても、行 2 への書き込みは Ok になる。行 1 への書き込みは `MissingReference` になる（ADR-0002 §4.2 どおり）。
- **`x:` prefix**: 新しい row と cell は `<x:row r="50"><x:c …><x:is><x:t>` として正しい名前空間で入る（`a9`）。
- **xml:space と escape**:
  - outer whitespace のときだけ preserve を付ける。
  - `\r` は `&#13;` として書く。
  - `_x005F_` で保護する。
  - 32,767 の上限と NaN/∞ は preflight より前に拒否する。
- **same-path save**: `Package::open` がファイル全体を `fs::read` するので安全。17 件のフィクスチャでテストされている。
- **テンプレート**:
  - `fills count="2"`（none と gray125）。
  - `cellXfs`/`cellStyleXfs`/`cellStyles` が揃っている。
  - theme と app.xml がある（TitlesOfParts は `Sheet1`、整合している）。
  - CT override がすべての部品にある。
  - `core.xml` は `Replaced` として作り直す（W3CDTF の UTC、creator なし）。
  - Excel での検証はまだしておらず、manual check（08）に委ねている（PR に明記されている）。
- **oracle**:
  - `oracle.rs` の 4 本は、壊した出力で実際に落ちる。
  - dimension を壊す変異（M6、M7、M20）は E1/E2/E4/E15 で KILLED になる。
  - T2 は要素名を見ない。それでも名前空間の誤りは、自前の read-back と re-open（QName 照合）で捕まる。
  - `reads()` が黙って飛ばすのは 59746、InlineString、SimpleStrict、xlmmacro の 4 件だけで、いずれも理由が正当。
- **ガードの冗長性**: 変異 M16（セル単位の formula ガードを削除）は workbook 全体のガードと重複しているため生き残る。これは問題ではない。

## PR の deviation の判定

| Deviation | 判定 |
|---|---|
| P0-E2: merged の anchor も拒否する | 許容（ADR-0001 の文言どおり。Q3 の coordinator default） |
| P0-PKG-1/3: Strict は `Package::open` で拒否する | 許容 |
| P0-PKG-5: 名前による lookup は Phase1 | 許容 |
| P0-R8: `set_formula` がない | 許容（表現できないこと自体が保証になる） |
| P0-R9: `dimension` を挿入しないため、AC があっても `sheetData` があれば書ける | 許容（Q6 と整合）。ただし AC が `c`/row の内側にある場合は F1/F6 を参照 |
| サンプリング（E3 は 20 セル、R1–R3 は 5 セルごと） | 許容（roadmap が認めている） |
| formula なしでも calcChain があれば拒否 | 許容（ADR の fast path）。ただし未テスト → F2 |
| scan できないシートは編集を拒否 | 許容（保守的）。ただし未テスト → F2 |
| 読み取りは calamine に従う | 許容。ただし継承された `xml:space` で食い違い、テストもない → F4 |
| API 名、`Result<Option<_>>`、0-based | 許容 |

## 判定

- Blocker: 0
- Major: 2（F1、F2）
- Minor: 5（F3–F7）

**converged = no**。F1 は実装の修正と回帰テスト、F2 はテストの追加だけで閉じる見込み。

## 付録: 再現物の場所

この 2 つはリポジトリの外（scratchpad）にあり、一時的。

- 敵対的テスト: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-xlsx-p0/repo/crates/ooxml-xlsx/tests/adversarial.rs`。`a1`–`a10` の 11 本。`rebuild()` はフィクスチャの zip を、エントリを差し替えて組み直す補助関数。
- 変異テスト: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-xlsx-p0/tools/mutate.py`。`ONLY=M1 python3 -I tools/mutate.py <repo>` のように実行する。
