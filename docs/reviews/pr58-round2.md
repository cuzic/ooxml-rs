# PR #58 レビュー（Opus, round 2）

対象: `impl/xlsx-phase4` の HEAD `879fe76`。round 1（`bafe55e`、`docs/reviews/pr58-round1.md`）からの差分は次のとおり。

- `9203b69`: M2
- `23a70b0`: M3、m7
- `c5cbcfd`: m1、m2
- `03017e2`: m3〜m5
- `cc5620c`: m6、m7
- `879fe76`: main のマージ

基準: ADR-0014 rev 3、および round 1 の指摘。

検証は、worktree を scratchpad の `review-pr58-r2/repo` に `git clone` したもので行った。worktree で変更したのはこのファイルだけである。

- `cargo test -p ooxml-xlsx --release`: 16 バイナリ、141 passed、0 failed。
- corpus sweep（POI `47dcff76`、`OOXML_XLSX_CORPUS`、`--ignored`）: **335 packages、1,613 ok、panic 0**。
  - PR 本文の値は 1,607 である。round 1 と同じく、使ったディレクトリのファイル数が 1 つ多いためと思われる（round 1 も 335 対 334）。
  - 拒否の内訳: `set_autofilter: Core` 57、`AutoFilterExists` 1、`merge: MissingReference` 3、`InvalidReference` 1、`merge(used)` / `merge_clearing(used)` の `MergeWouldHideValues` / `MergeOverlap` / `TableOverlap` / `ArrayAnchor` / `SharedFormulaMaster` / `IterativeCalculation`。どれも想定どおりの拒否で、バイトは変わらない。
  - 適用した回数: `unmerge` 37、`remove_autofilter` 2、`merge(used)` / `merge_clearing(used)` 290。
- probe: `review-pr58-r2/repo/crates/ooxml-xlsx/tests/probe58r2.rs`（3 件）。
- mutants: `review-pr58-r2/scripts/mut.sh` で 10 個を作った。round 1 で生き残った 2 個と、今回の修正箇所である。

## 結論（先に）

- 判定: **converged: yes**。Blocker 0、Major 0、Minor 3（いずれも follow-up でよい）。
- **M1 は解消した。** pivot part は `merge` のときに初めて読む。
  - 壊れた part や location のない part があっても、ブックは開ける。保存したときも入力と同じバイトになる。
  - merge を拒否するのは、その part を持つシートだけである。
  - pivot が 2 つあるシート、ほかのシートの pivot の source になっているシート、dangling な relationship も確かめたが、正しく動いた。
- **M2 は解消した。** round 1 の mutant（ほかの definedName の本文と `hidden`、`calcPr` の `calcMode` と `calcId`）は、すべて oracle で Err になった。
  - 新しく作られる要素の中身は、oracle では判定しない。これは文書に書かれた制限であり、behaviour test がその中身を固定している（mutant A1 と A2 は検出された）ので、受け入れてよい。
  - ただし、`EditAttrs` が値を見ないことによる小さな穴が残っている（n2）。
- **M3 は解消した。** ADR-0014 §4 と §13 に、拒否が実際に及ぶ範囲と数字が書かれた。2026-11-16 の判断（拒否を残し、core の follow-up で緩める）も記録され、README にも Known limit として載った。owner に伝えたことと、core の follow-up を予定していることも確認した。
- m1〜m7、getrandom、Debug、空の password は、すべて解消した。

## 解消状況

| round 1 | 状態 | 確認内容 |
|---|---|---|
| M1 pivot を先に読む | 解消 | 下の「M1 の確認」を参照。 |
| M2 oracle が path 単位 | 解消（制限つき） | 下の「M2 の確認」を参照。 |
| M3 AlternateContent の範囲 | 解消 | 下の「M3 の確認」を参照。 |
| m1 重複した merge | 解消 | `merges.remove(position)` で 1 つだけ消す。テスト `unmerge_of_a_duplicated_ref_keeps_the_other` がある。mutant U1（`retain` に戻す）は検出された。 |
| m2 生き残った mutant | 解消 | K5（dry-run の loop を消す）は `merge_clearing_checks_every_cell_first` で検出された。K4 は、spill を census の `Array`（`cm` つき）として扱うようにしたことで `dynamic` の arm がなくなった。代わりの mutant K4'（`Array if c.dynamic => continue`）は `merge_over_a_spill_is_refused`（`xlookup.xlsx`）で検出された。 |
| m3 getrandom | 解消 | `[workspace.dependencies] getrandom = "0.4"` を `{ workspace = true }` で引いている。`cargo tree -i` で見ると、通常の依存にある getrandom は 0.4.3 の 1 つだけだった。0.2 と 0.3 は、`ooxml-bench` の dev-dependency（umya-spreadsheet、ahash）から来るものだけである。 |
| m4 Debug | 解消 | `Protection { password: Some("<redacted>"), … }` と表示される。password がないときは `None`。mutant D1 は検出された。 |
| m5 空の password | 解消 | 詳しくは表の下に書く。 |
| m6 sweep の範囲 | 解消 | sweep に `unmerge`、`remove_autofilter`、`merge(used)`、`merge_clearing(used)` が加わった。`cleared_cells` は、範囲の外のセルについて、Phase1 がキャッシュを消す（strip）変化しか許さない。 |
| m7 文言 | 解消 | ADR-0014 §3 に、query table のあるシートでは merge をすべて拒否すると注記した。README も同じ。`TRUE` という名前のシートは manual check 28 に入った（XL で確かめる項目）。 |

m5 の詳細:

- `password("")` は password なしを意味し、`<sheetProtection sheet="1" objects="1" scenarios="1"/>` を書く。これは、Excel の「シートの保護」で password を空にしたときに書かれるものと同じ形である。hash を書かないのは正しい。
- password つきで保護したシートを、空の password でもう一度保護すると、hash の属性はすべて消える。
- `password("x").password("")` も password なしになる。
- password が `" "`（空白 1 つ）のときは hash を書く。Excel でも空白は password として扱われるので、正しい。
- password がないときの `with_spin_count(u32::MAX)` は、使わない値なので確かめないことが doc に書かれた。
- mutant E1 は検出された。

### M1 の確認

probe `m1_pivot_shapes`（`ExcelPivotTableSample.xlsx`）の結果は次のとおり。

- (a) 壊れた `pivotTable2.xml`（Tabelle3）:
  - 開ける。
  - Tabelle3 の遠い範囲への merge は `PivotOverlap { location: "…pivotTable2.xml (no readable location)" }` になる。
  - その後の `to_bytes()` は入力とバイト単位で同じだった。parse に失敗した part は `Raw` のまま残るためである。
  - そのあと別のシートで merge しても、壊れた part はバイト単位で同じだった。
- (b) source sheet（Tabelle1。ほかのシートの pivot の cache が参照している）の空き範囲への merge は Ok で、pivot と cache の part は変わらない。
  - pivot の位置はシートの relationship から得るので、source sheet は対象にならない。これで正しい。
- (c) pivot のあるシート:
  - 重ならない `F3:G4` は Ok で、pivot part は変わらない。
  - `D7:E8` は `PivotOverlap { location: "A3:D7" }` になる。
- (d) 1 つのシートに pivot が 2 つあるとき（relationship を足し、2 つめを `K20:L24` に置いた）:
  - `K24:L25` と `D7:E8` は、それぞれ対応する pivot との重なりで拒否される。
  - `F3:G4` と `X50:Y51` は Ok。
- (e) 2 つのうち 1 つに location がないときは、遠い範囲でも拒否される。安全側である。
- (f) location の値を変えた場合:
  - `A3`（単一セル）、`a3:d7`、`$A$3:$D$7` は正しく解釈される。
  - `Tabelle2!A3:D7` と `""` は読めないので、重なるとみなして拒否する。
- (g) 存在しない part を指す pivot relationship は、開いても無視される。これは round 1 の前と同じ挙動である。
- (h) Phase1 のセル書き込みだけをする利用者は pivot part を parse しない。保存しても part はバイト単位で同じだった。

mutants:

- P1（読めないものを重ならないとみなす）は検出された。
- P2（parse のエラーを伝える）は検出された。
- **P3（最初の pivot しか見ない）は生き残った**（n1）。

### M2 の確認

probe `m2_round1_mutants` の結果。「Err」は oracle が検出したもの、「Ok」は見逃したものである。

| mutant | oracle |
|---|---|
| ほかの名前（`_xlnm.Print_Area`）の本文 | Err |
| `Other` に `hidden="1"` を足す | Err |
| 作られた名前の `localSheetId` を違う値にする | Err（selector から外れ、残りの部分に出る） |
| 再利用した名前から未知の属性（`comment`）を落とす | Err |
| `calcPr` の `calcId="1" calcMode="manual"` | Err |
| `calcPr` に `calcMode="manual"` を足すだけ | Err |
| 作られた名前の本文、`hidden` を落とす、属性を足す | Ok（新しく作られた要素なので判定しない。文書どおり） |
| 作られた名前を 2 つにする | Ok（`Create` は部分集合しか見ない） |
| `fullCalcOnLoad="0"`、または `fullCalcOnLoad` を消す（入力に `="1"` があるときも） | **Ok**（n2） |
| 入力になかった `calcPr` が作られ、そこに `calcMode="manual"` がある | Ok（作られた要素） |
| `calcPr` を 2 つにする | Ok（n2） |
| `dimension/@ref` を縮める（`A1`）、`zz` にする、属性を消す | **Ok**（n2） |
| 作られた `mergeCell` の `ref`、作られた `sheetProtection` の `sheet="0"` | Ok（作られた要素） |

- 作られた要素の中身は、behaviour test が固定している。
  - mutant A1（`hidden` を書かない）と A2（名前の本文を壊す）は、`autofilter_creates_or_reuses_the_filter_database_name` で検出された。
  - mergeCell の `ref` は `merged_ranges`、保護の属性は round 1 の flag の mutant で固定されている。
  - したがって、`a_disallowed_change_inside_an_allowed_path_fails_the_oracle` の doc comment に書かれた制限は受け入れてよい。

### M3 の確認

- README の `set_autofilter` の行に、**Known limit** として数字（349 個中 61 個）を書いた。
- ADR-0014 §4 は「`maxOccurs=1` slot では、AC 子要素が親のどこにあっても挿入を止める。隣にあるときだけではない」と書き直された。
- ADR-0014 §13 には、**Decision 2026-11-16** として次が記録された。
  - 拒否を残す。
  - 規模（81 個、61 個、sweep の 57 件）。
  - 既存の `definedNames` に足す場合は影響を受けない。
  - PR #58 の後に、core の follow-up で緩める（`check_*` twins と test を含む）。
- sweep の `set_autofilter: Core` 57 件は round 1 と同じで、バイトは変わらない。
- owner に伝えたこと、core の follow-up を予定していることを確認した。

## Findings（Minor のみ）

- **n1. pivot が 2 つ以上あるシートのテストがない。**
  - mutant P3（`ctx.pivots…take(1)`）が生き残った。
  - 実装は正しい（probe d、e）。`unreadable_pivot_location_does_not_block_open` に「同じシートの 2 つめの pivot とだけ重なる merge」を 1 つ足せば閉じる。relationship を 1 本足すだけで作れる（probe d を参照）。
- **n2. `EditAttrs` が値を見ないので、oracle に小さな穴が残っている。**
  - `calcPr` の `EditAttrs(["fullCalcOnLoad"])` は、`"0"` も削除も通す。そのため commit message にある「calcPr only gains fullCalcOnLoad」は、oracle では保証されていない。
  - 新しく足した `dimension` の行（`EditAttrs(["ref"])`）は、縮めることも、壊れた値にすることも、属性を消すことも通す。実装は Phase1 の `range.union(at)` だけを使うので、広げることしかしない。つまり、この行を広げたことは実装の側から見て安全である。ただし oracle は縮めることを許してしまう。
  - `Create` は部分集合しか見ないので、同じ要素が 2 つになっても通る。
  - 修正案（follow-up でよい）:
    - `EditAttrs` に値の条件を足す（`fullCalcOnLoad` は出力で `"1"`）。
    - `dimension` は、出力の範囲が入力の範囲を含むことを確かめる。
    - `Create` のある path では、要素の数が増えるのは高々 1 つまでにする。
- **n3. doc comment が重複している。**
  - `tests/allowed_diff.rs:277` が `/// Corpus sweep (ignored;/// Corpus sweep (ignored; …` になっている。編集のときに起きたもので、実害はない。

## 判定

- **converged: yes**。
- round 1 の Major 3 件と Minor 7 件は、すべて解消した。
- n1〜n3 は merge を止めるものではない。follow-up に回すか、次の push で直せばよい。
