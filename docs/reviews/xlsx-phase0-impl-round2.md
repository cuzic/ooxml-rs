# xlsx Phase0 実装レビュー（Opus, round 2: PR #17）

## 対象/方針

- 対象: PR #17「xlsx Phase0: fixes from implementation review round 1」。ブランチ `fix/xlsx-phase0-review1`（`e538dce`）。`de5d6ae..HEAD` の差分のうち、`crates/ooxml-xlsx/src/worksheet.rs`、`README.md`、`tests/{refusals,edit}.rs`、`tests/common/mod.rs`。
- 照合した文書: round 1 レビュー（`docs/reviews/xlsx-phase0-impl-round1.md`）、ADR-0001、ADR-0002 §4、`docs/roadmap/xlsx.md` §7、`crates/ooxml-xlsx/README.md`。
- 方法:
  1. 差分と、`write()`/`preflight()` の失敗しうる経路を読む。`ooxml-core` の `LensMut::place`/`set_attr`/`set_text` まで追った。
  2. scratch 上のコピーに探索テスト `tests/adversarial2.rs` を足した。対象は次のとおり（いずれも `fixture_minimal` か合成シート）:
     - `preflight` の各拒否条件
     - 重複した子を持つセル
     - 既存の行の間への新しい行
     - 行の途中への新しいセル
     - `x:` prefix
     - 型付きセルへの `set_number`
     - Err の後の same-path save
     - `dimension/@ref` の表記ゆれ
     - コーパス全 26 件への影響
  3. fixer の mutation script（`mutate.py`、29 変異）をそのまま再実行した。さらに、新しい変異を 8 件足して実行した。
- リポジトリ本体は変更していない。このファイルだけを追加した（untracked）。フィクスチャは信頼しない入力として扱い、パースするだけにした。
- 重大度: Blocker/Major は次のいずれかに当たるもの。それ以外は Minor。
  - Err なのに文書が変わる
  - 黙ったデータ変更
  - 保証違反
  - コア保証が未テスト

## 結論（先に）

- round 1 の F2、F3、F5、F7 は解消した。F4 と F6 も、指摘した範囲では解消した。
- F1 は**部分的な解消**にとどまる。修正は `c` 直下の `mc:AlternateContent` という一つの形だけを `preflight` で塞いだ。「削除してから挿入し、挿入が失敗しうる」という構造は残っている。
  - スキーマ違反の重複子（`v` か `is` が 2 つ）と `extLst` を持つセルでは、今も **Err を返したうえで値が消える**（N1）。
  - 新しい doc comment と README は「preflight が Ok なら `write` は途中で失敗しない」「Err なら part は dirty でない」と明言している。この経路はその主張を反証する。
- **行の途中への新しいセルの挿入（列順）がテストされていない**。セルを常に行末へ追加する変異が全スイートを生き残る（N2）。実装は正しい。ただし README が保証する挿入順序が CI で守られていない。
- F7 の拒否は、実コーパス 26 件のうち **0 件**にしか影響しない。F6 の読み取り拒否も 0 件。回帰はない。
- F4 の規則変更は、README とテストには反映されている。roadmap §7 の文言は古いまま残っている（N4、Minor）。
- fixer の mutation script は再実行で同じ結果になった（27/29 KILLED。生き残りは M7b と M16）。新しい変異 8 件のうち 3 件が生き残った（M28、M29、M35）。

**判定: converged = no**（Major 2 件: N1、N2）。

## round-1 解消状況

| ID | 状態 | 根拠 |
|---|---|---|
| F1（Err の前に変更している: `c` 内の AC） | **部分的** | `preflight` に `refuse_alternate_content(c.raw(), Cell)` が入った（`worksheet.rs:288`）。テスト `f1_err_inside_cell_changes_nothing` が追加され、変異 M21 は KILLED になる。私の round-1 再現も Err で無変更になった。ただし同じ根本原因の別の経路が残る（N1） |
| F2（workbook 全体の formula 拒否が未テスト） | **解消** | `f2_formula_on_another_sheet_only`、`f2_formula_in_macro_or_dialog_sheet_only`（CT 経路と rel 経路を個別に）、`f2_calc_chain_only`、`f2_unscannable_other_sheet`（not-well-formed と UTF-16）が入った。M1、M2、M3、M3b、M19、M26、M27 はすべて KILLED |
| F3（table header の変種） | **解消** | `e11b_table_header_variants`（2 表、`headerRowCount="2"` と `"0"`）。M4、M5、M18 は KILLED |
| F4（空白 trim と継承された `xml:space`） | **解消**（文書に残りあり） | `text_of` は `t` 自身の `xml:space` だけを見るようになった（calamine 0.38 `xlsx/mod.rs:3160-3178` と同じ規則。ASCII 4 文字だけを trim）。README も更新された。`f4_whitespace_like_calamine` で M12 と M25 は KILLED。roadmap §7 の文言は古いまま（N4） |
| F5（`c` の順序） | **解消** | `f5_cells_out_of_order`。M11 は KILLED |
| F6（`sheetData` 内の AC で row が「ない」と扱われる） | **解消**（`sheetData`、row、`sheetData` のない worksheet） | `locate` が 3 か所で拒否するようになった。M22、M23、M34 は KILLED。`c` の中の AC は読み取りでは今も黙って Empty を返す（N3、Minor） |
| F7（解釈できない `dimension/@ref`） | **解消** | `preflight` で `Malformed` を返して拒否する。`f7_dimension_ref` で M24、M31、M36 は KILLED。コーパスへの影響は 0/26 件（下記「回帰の確認」） |

## 新しい findings

### N1 (Major) — 重複した `v`/`is` と `extLst` を持つセル: Err を返したうえでセルが変わる（F1 と同じ根本原因）

**根拠**

- `worksheet.rs:386-391`（`set_number`）と `:416-421`（`set_string`）: closure は先に `remove_data_type` / `remove_cell_value` / `remove_inline_string` で**最初の一つだけ**を削除し、その後で `get_or_insert_*` が `ooxml-core` の `place()` に入る。
- `crates/ooxml-core/src/lens.rs:345-347`: `place()` は、兄弟がスキーマの順序にない（後ろの slot の後に前の slot が来る）と `UnresolvedSibling` を返す。
- `preflight` が検査するのは AC だけで、セルの子が配置可能かどうかは検査しない。そのため `worksheet.rs:261-265` の doc comment「once this returns `Ok`, `write` cannot fail part-way」と、README の「Every refusal is decided before the sheet is touched」は成り立たない。

**再現**（`adversarial2.rs::p1_err_after_mutation_duplicates`、`fixture_minimal` の A2 を置き換えた入力）:

| A2 の入力 | 操作 | 結果 | part の状態 | A2 の出力 |
|---|---|---|---|---|
| `<c r="A2" t="s"><v>1</v><extLst/><v>1</v></c>` | `set_string("new")` | `Err(UnresolvedSibling(c))` | **dirty** | `<c r="A2" t="inlineStr"><extLst/><v>1</v></c>` → 読み取りは **Empty**（元は "Alice"） |
| `<c r="A2" t="inlineStr"><is><t>a</t></is><extLst/><is><t>b</t></is></c>` | `set_number(5)` | `Err(UnresolvedSibling(c))` | **dirty** | `<c r="A2"><extLst/><is><t>b</t></is></c>` → Empty |

Same-path save（`p4_same_path_save_after_err`）: 「失敗したセルは飛ばし、別のセルを書いて同じパスに保存する」という書き方では、A2 が Empty のまま保存された。round 1 F1 と同じ形のデータ消失である。

**付随する問題**: Ok を返すのにデータが黙って変わる例もある。

- `<c><is>a</is><is>b</is></c>` に `set_number(5)` → Ok。出力は `<c r="A2"><v>5</v><is><t>b</t></is></c>`。本 crate の読み取りは 5 を返すが、**calamine は `"b"` を返す**。
- `<c t="s"><v>1</v><v>0</v></c>` に `set_string` → 古い `v` が残る（`t="inlineStr"` と `v`、`is` が同居する）。

**なぜ Major か**: 入力はスキーマ違反（CT_Cell の `v`/`is` は maxOccurs=1）で、まれだ。それでも round 1 の F1（`c` 内の AC）と同じく「Err なら何も変わらない」という保証に反する。修正後の文書がまさにその保証を明言しているので、Major とする。

**提案**: 個別の形をこれ以上塞いでいくより、構造で閉じるほうがよい。

- (a) `preflight` でセルの子が「スキーマ順で、`v`/`is` がそれぞれ高々 1 つ」であることを確かめ、違えば `Malformed`/`UnresolvedSibling` で拒否する。core に `place` の dry-run（例: `LensMut::can_insert::<C>()`）を足すのが筋がよい。
- (b) closure を「挿入（`get_or_insert`）を先に、削除を後に」の順に変える。加えて、`v`/`is` は最初の一つだけでなく全部を削除する。
- (c) `f` の前に cell の `RawNode` を退避し、Err なら戻す（ADR-0003 の plan → validate → apply）。

上の 2 つの入力を `refusals.rs` の回帰テストにし、`assert_unchanged` と「保存後も "Alice"」を確かめる。

### N2 (Major) — 既存の行の途中への新しいセルの挿入（列順）がテストされていない

**根拠**

- `worksheet.rs:327-329`: `insert_cell_by` の述語 `c.col > col`。
- 変異 **M29**（述語を常に false にして、新しいセルを常に行末に追加する）が全スイート（`--lib`、`edit`、`package`、`refusals`、`oracle`）を**生き残る**。

既存の書き込みテストは次のどれかに当たり、新しいセルの右に既存のセルがある書き込みを一つも含まない。

- 既存セルの上書き
- 新しい行
- 行末への追加

**影響**: README は「New rows/cells: Inserted in row/column order」と保証している。この変異の下では次のようになる。

- `B1` が `C1` の後ろに入る。
- Excel では修復ダイアログが出る。
- 本 crate の `locate` もその行を `OutOfOrder` にし、読み取りも書き込みもできなくなる。

いずれも CI では検出されない。実装は正しい（`p2_row_and_sheet_order` の「between cells」で `A1,B1,C1` の順に入るのを確認した）。それでもコアの書き込み保証が未テストなので、round 1 F2 と同じ基準で Major とする。行側の M30（常に末尾）は `e12`/`save_to_the_same_path` で KILLED になる。

**提案**: `edit.rs` に、`fixture_minimal` の行 2（A2..D2）の前に新しいセルを書くケースを足し、T2 と CR を確かめる。例: `<row r="2"><c r="B2"/>…` から `A2` を消して `A2` を書く。合成の `<row r="1"><c r="A1"/><c r="C1"/></row>` に `B1` を書くケースでもよい。

### N3 (Minor) — `c` の中の `mc:AlternateContent` は、読み取りでは今も黙って Empty/`""` を返す（F6 のセル版）

**根拠**: `locate` が見るのは `sheetData` と row だけで、セルの AC を拒否するのは書き込み（`preflight`）だけだ。README の表も「of the target cell (writes)」と書き分けている。

**再現**（`p7_ac_elsewhere_reads`）:

- `<c r="A2" t="s"><mc:AlternateContent><mc:Choice Requires="x"><v>0</v></mc:Choice><mc:Fallback/></mc:AlternateContent></c>` → `cell(1,0)` は `Ok(Some(Empty))` を返す。
- `<is><mc:AlternateContent><mc:Fallback><t>hidden</t>…` → `Ok(Some(String("")))`。その後の `set_number` は Ok になり、AC ごと置き換わる（書き込みとしては妥当）。

**なぜ Minor か**: round 1 F6 と同じく、読み取り専用で入力がまれだからだ。

**提案**: `cell()` でもセル（と `is`）直下の AC を `UnresolvedSibling` にする。あるいは、README に「セル内部の AC は読み取りでは見ない」と明記する。

### N4 (Minor) — F4 の規則変更が roadmap §7 に反映されていない

**根拠**

- `docs/roadmap/xlsx.md:522-523` は今も「a `t` without `xml:space="preserve"` **in effect** has its outer ASCII whitespace trimmed」と書いている。これは継承を含む旧規則で、実装（`t` 自身の属性だけを見る）、README、`f4_whitespace_like_calamine` と食い違う。
- 同じ行の「(and, as far as is known, Excel)」も、継承された preserve について Excel で確かめたわけではない。

Phase0 の既存テストと `examples/manual_check.rs` には、この変更で期待値が変わるものはなかった。

- E14（`52348.xlsx`）は全 `t` が自身に preserve を持つ。
- manual_check の `"  padded "` は書き込み側の話。
- 全スイートは pass する。
- 全フィクスチャで calamine と照合する `reads()` も pass する。

**提案**: roadmap の文言を README に合わせる。manual check（08）に「`si xml:space="preserve"` の下の空白を Excel がどう読むか」を 1 項目足すと、calamine 準拠の判断を Excel で裏付けられる。

### N5 (Minor) — 重要だが未テストの小さな挙動（変異が生き残る）

| 変異 | 結果 | 意味 | 提案するテスト |
|---|---|---|---|
| M28: `trim_matches([' ', '\t', '\r', '\n'])` → `trim()` | **SURVIVED** | NBSP や全角空白（U+3000）が先頭・末尾にある文字列を黙って削る。日本語データでは現実に起こる。calamine は ASCII 4 文字だけを trim する | `<t>　全角　</t>`（preserve なし）が `"　全角　"` のまま読めること |
| M35: escape から U+FFFE/U+FFFF を外す | **SURVIVED** | `set_string("\u{FFFF}")` が `set_text` の `InvalidXmlChar` に達する。この変異の下では `remove_cell_value` などの後で失敗するので、N1 と同じ Err-with-mutation の経路になる | `escape::tests` に U+FFFE/U+FFFF の round-trip を足す。`set_string` で書き込みが Ok になり、読み戻せること |
| M7b: 範囲の内側に書いても `dimension` を書き直す | SURVIVED（round 1 から継続） | `$A$1:$D$2`、`a1:d2`、`D2:A1` は内側への書き込みでも正規化されてしまい、T2 の「触れない」に反する | 内側（B2）への書き込みで、`ref="a1:d2"` がバイト単位で変わらないこと |
| M16: セル単位の formula ガードを削除 | SURVIVED | workbook 全体のガードと重複しているので問題ない（round 1 と同じ判断） | 不要 |

### N6 (Minor) — `dimension/@ref` の受け入れ範囲（F7 の付随事項）

`p6_dimension_spellings` で F9 に書き込んだ結果:

| `ref` | 結果 |
|---|---|
| `$A$1:$D$2`、`A1:A1`、`D2:A1` | Ok（`A1:F9` に正規化される） |
| `" A1:D2"`、`"A1:D2 "`、`A1 B2`、`Sheet1!A1:D2`、`A0:D2`、`A1:XFD1048577` | `Malformed` で拒否、無変更 |

- ST_Ref は `xsd:string` で、空白は collapse されない。前後に空白がある場合の拒否は保守的な選択として妥当だ。
- `$` 付きの表記が受け入れられ、範囲を広げるときに正規化されることは、README の「a lower-case `ref` comes out upper-case」と同じ扱いで README に一言あるとよい。
- 実コーパスに該当するファイルはない。

### N7（推奨のみ）— `cargo fmt --check` を CI gate にするか

**現状**

- `cargo fmt --all --check` は 56 ファイルで差分を出す。`rustfmt.toml` はない。
- 内訳:
  - codegen が出力する `generated/*.rs`: 4 ファイル
  - xtask、core、pptx、xlsx の手書きコード
- `max_width` を 120 や 140 にしても 37 ファイルが残る。つまり、コードベースは rustfmt の既定とも、単一の設定とも一致していない。
- PR #17 で足されたテストも未整形の行を含む。ただし、これは既存の慣習に合わせた結果だ。

**判断**: 今すぐ gate にするのは**反対**する。次の順序で進めるのを推奨する。

1. 実際のスタイル（`max_width` など）を書いた `rustfmt.toml` を置く。
2. 生成ファイルを codegen 内で rustfmt に通すか、`#![cfg_attr(rustfmt, rustfmt_skip)]` などで対象から外す。そうしないと `codegen --check` と衝突する。
3. 整形だけの PR を一度通す。
4. そのうえで `cargo fmt --all --check` を CI に足す。

PR #17 の収束条件にはしない。

## 回帰の確認

- **コーパス 26 件**（`all_zip_fixtures`、`p5_corpus_impact`）:
  - F7 の `Malformed` で書き込みを拒否するファイル: **0 件**。`dimension` があるファイルの `ref` はすべて解釈できる。ないファイルは 5 件。
  - F6 による読み取りエラー: **0 件**（全既存セルを読んだ）。
  - A1 と Z99 への書き込みが Err になった場合、part はすべて無変更だった。拒否の理由は formula 6 件、`MissingReference` 2 件、merged/vm 各 1 件で、いずれも従来どおり。
- **型付きセルへの `set_number`**: `inlineStr`（rich）、`s`、`b`、`e`、`str`、`d` のいずれでも、`t` が消え、`is` が消え、`s` が残る。calamine の読み戻しも一致した。
- **空文字列と空白だけの文字列**: `""` は `<is><t/></is>`、`"  "` は `xml:space="preserve"` 付きになる。いずれも calamine と一致した。
- **既存の行の間への新しい行**（B2 を行 1 と行 3 の間に書く）: 正しい位置に入り、`xml:space="preserve"` が付き、`dimension` は変わらない（もともと含んでいる）。末尾への新しい行（E7）では `A1:E7` に広がる。
- **`x:` prefix**: self-closing の `x:sheetData` への書き込みで `x:row`/`x:c`/`x:is`/`x:t` が入る。default namespace が別の URI に束縛されていても `x:` を選ぶ。
- **row の中で `extLst` が `c` より前にある場合**（スキーマ順に違反）: 新しいセルへの書き込みは `UnresolvedSibling` になり、無変更（挿入より前に変更がないため）。sheetData がなく子が順不同の worksheet も同様に無変更で Err になる。
- **テスト**: 全スイートが green（unit 6、edit 18、oracle 5、package 8、refusals 17、doc 2）。

## 再現物

scratch ディレクトリ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-xlsx-p0-r2/`

| ファイル | 内容 |
|---|---|
| `repo/crates/ooxml-xlsx/tests/adversarial2.rs` | 探索テスト（p1–p7） |
| `adv2.out` | その出力 |
| `mut_result.txt` | fixer の 29 変異の再実行結果 |
| `mutate2.py` | 新しい変異 M28–M36 |
| `mut2_result.txt` | その結果 |

## 判定

- Blocker: 0
- Major: 2（N1、N2）
- Minor: 5（N3–N6、それに推奨の N7）
- round 1 からの持ち越し: F1 が部分解消（N1 として再掲）。ほかの 6 件は解消。

**converged = no**。N1（重複子を持つセルでの Err-with-mutation。構造で閉じる修正を推奨）と N2（列順の挿入のテスト）が直れば収束と判断できる。
