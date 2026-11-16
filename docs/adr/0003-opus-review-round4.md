# PR #20 レビュー（Opus, ADR-0003 round 4 / 実装 round 2）

対象: PR #20 `impl/xlsx-phase1` @ `0298520`（worktree `impl-xlsx-phase1`）。ADR-0003 revision 4（Status: Proposed）、`crates/ooxml-xlsx` の Phase1 実装、O3 の期待値データ（`reference/fixtures/xlsx/o3/expected.tsv`、run 43821205800、LibreOffice 25.2.7.2）。

方針: 読み取り専用の敵対的レビュー。worktree を scratchpad（`review-pr20-r4/wt`）にコピーし、次のものを走らせた。

- round 3 の probe（`zz_probe.rs`）の再実行。
- 新しい probe（`zz_probe4.rs`、Q1〜Q32）。
- O3 の集計と、output のハッシュを見ない意味論版（`zz_o3.rs`）。
- ミュータント15件（`mutants.py`、`mut.sh`）。
- ベンチ（`phase1_bench`、通常と `WORST=1`）。

重大度は依頼の基準に従う。

- **Blocker/Major**: stale 値が読める、`Err` なのに変更が残る、破損、保証が試験されていない。
- **Minor**: それ以外。
- 実機で確かめていないものには「要検証」と書いた。

## 結論（先に）

round 3 の指摘はすべて解消された。

- **R3-M1**: probe P1、P2、P3、P4、P13 は、どれも `Everything` で剥がれるようになった。
- **R3-M2**: oracle がバイト比較になった。round 3 のミュータント2件（`s` の除去、`row/@ht` の書き換え）と、新しく作った hand-off 側のミュータントは、どれも試験で落ちる。
- **O3** は空振りではなくなった。
  - canary があり、672ケースすべてにデータがある。
  - 4,032セルを比較している。
  - 「依存先を取り逃がせば O3 が検出する」セル（入力の baseline が LibreOffice と一致し、編集で LibreOffice の値が変わるもの）は **345セル、183ケース**ある。
  - 依存規則のミュータント（SUMIF の resize の除去、シートをまたぐ参照の無視、推移の打ち切り、3-D、名前、列全体、共有式のオフセット）は、O3 単独でも意味論のレベルで検出された。
- `cargo test -p ooxml-xlsx --all-features` は全件 pass。`phase1_o3` も pass し、`expected.tsv` は現在のコードの出力とハッシュで一致する。

ただし、新しい Major が1件ある。

- **R4-M1**: 依存先を剥がす処理（apply (iii)）は、`v` と `is` を**1つずつ**しか消さない。重複した `<v>` を持つ依存先（schema 違反の入力）では、編集後も古い `<v>` が残り、読める。target の書き込み（`write_cell`）は、まさにこの理由で `while` ですべて消している。直すのは数行で済む。

**判定: converged = no**（R4-M1。実装だけの修正で済む）。**ADR を Accepted にできるか = yes**。ADR の文面にある Minor（R4-m4、R4-m5）を直すことが条件で、設計の変更は要らない。

---

## round-3 指摘の解消状況

| ID | 内容 | 判定 | 根拠 |
|---|---|---|---|
| R3-M1 (a) | ローカル名の中の、シート名の無い参照 | **解消** | `precedents.rs` の `Walker` は、名前の中で `own: None` になる。P4 は `S1!A1` の編集で `S1!C1` を剥がす（`still_has(<v>7</v>)=false`）。回帰試験 `p4_…`。ただし ADR の根拠の記述は誤り → R4-m4 |
| R3-M1 (b) | 同じ `si` の master が2つ | **解消** | `census.rs` の `ambiguous_group` → `precedents_of` が `Everything` を返し、hand-off は `Err`。`by_si` は先勝ちになった。P1 は4セルとも依存先になり、`A2` の `<v>2</v>` は消える |
| R3-M1 (c) | folding で衝突するシート名、定義名 | **解消** | `fold` = 大文字化してから小文字化。`Env::sheet` と `resolve` は一意でなければ `None` を返す。`sheet_index` は完全一致を優先し、曖昧なら `NoSuchSheet`。P2、P3、P13 で確認した。全角/半角は別の扱い → R4-m7 |
| R3-M2 | 許された差分が試験されていない | **解消** | `assert_allowed_diff` は、sheet part 全体、`workbook.xml`、両方のレジストリをバイト比較する。round 3 のミュータント2件は O1、`phase1_formulas`、`phase1_fixtures` で落ちる（各 7〜12 件）。hand-off の新しい master の `s` を消すミュータントも `shared_master_hand_off_1d_and_not_top_left` で落ちる |
| R3-m1 | シート修飾の衝突の平行移動 | **解消** | P5 は `Err(SharedFormulaMaster)` になり、sheet は不変 |
| R3-m2 | `"==A1"` | **解消** | P6 は `Err(InvalidFormula)` になり、不変 |
| R3-m3 | poisoning の抜け道 | **解消（試験は一部）** | apply の前に poison し、成功したら解除する。`package()` は doc で警告する。panic の経路の試験は無い → R4-m6 |
| R3-m4 | 最悪ケースの性能 | **解消** | `Everything` の閉包を `OnceCell` でメモ化し、`add` で無効化する。無効化の漏れは見つからなかった（削除は `census.get` で除くので上位集合になり安全。hand-off は辺を変えない）。実測は通常 2.38 ms/edit、`WORST=1` で 5.98 ms/edit（剥がし済みは 4.80 ms）。ADR の表（2.0 / 6.7 ms）と同じ桁である |
| R3-m5 | O3 の記述、揮発性の除外 | **解消（小さな穴あり）** | ファイル名と workflow は一致した。揮発性の関数を含む workbook は丸ごと除外する（9ケース）。ただし定義名の中の揮発性の関数は見ていない → R4-m3 |
| R3-m6 | 試験の穴 | **解消** | P7 の回帰試験がある。fault injection は、hand-off、calcChain が無い workbook、`KeepStaleCaches` の3種類で、6つの step すべてを試す |
| Owner Q2 | O3 のデータ | **完了** | CI run 43821205800 の結果が、canary とヘッダつきでコミットされた |

---

## O3 の評価（依頼 (2)）

**空振りでないか**:

- **canary**: Python 側と Rust 側の両方で検査している（`Sheet1!B1` = `2`）。
- **ファイルごとの網羅**: 672ケースすべてに input と output の行があり、無ければ panic する。
- **比較の対象**:
  - 比較したセル: 4,032（下限は `compared > 100`）。
  - 1セル以上を比較したケース: 654/672。
  - 揮発性として除外したケース: 9。
  - 比較が0のケース: `70005-countifs` と `o1_no_calcpr`。どちらも、すべてのセルが剥がされるため。

**現在のコードとの一致**: `phase1_o3` は pass する。672ケースすべてで、output の FNV が一致した（`out_hash_mismatch 0`）。

**依存ロジックをどれだけ試しているか**（`zz_o3.rs` の集計）:

- 「感度のあるセル」は 345 ある。これは、baseline が一致し、かつ LibreOffice の値が編集の前後で変わるセルで、これを取り逃がすと O3 が検出する。
- 実装が剥がしたセルは 870 で、そのうち値が実際に変わったのは 345（残りの 525 は、`Everything` のための過剰な剥がしか、値が同じだったもの）。
- 感度のあるセルの内訳: O2 の標本で 177（200 seed のうち 51）、`o1_precise` で 63、`NewStyleConditionalFormattings` で 57、`o1_shared` で 21、`o1_arrays` で 9、その他。
- `o1_everything_*` の感度は 0 である。これは `Everything` が過剰であることの確認にしかならない。

**ミュータント**（O3 列は「output のハッシュ照合を外し、意味論だけで比べた版」。`phase1_o3` 本体は、出力のバイトが変わるだけで必ず落ちる）:

| ミュータント | 単体 | O1 系 | O2 | O3（意味論） |
|---|---|---|---|---|
| M1 SUMIF の resize を除去 | 検出 | 検出 | 検出 | 検出 |
| M2 他シートの参照を無視 | 検出 | 検出 | 検出 | 検出 |
| M3 推移をたどらない | – | 検出 | 検出 | 検出 |
| M4 spill の潜在領域を seed しない | – | 検出 | **見逃し** | **見逃し** |
| M5 共有式 member のオフセットを0にする | – | 検出 | **見逃し** | 検出 |
| M6 3-D の先頭シートだけ | 検出 | 検出 | 検出 | 検出 |
| M7 範囲演算子 `a:b` の右辺を捨てる | 検出 | **見逃し** | **見逃し** | **見逃し** |
| M8 名前を無視 | 検出 | 検出 | 検出 | 検出 |
| M9 列全体を1行にする | – | 検出 | 検出 | 検出 |
| M10 array/spill の result を剥がさない | – | 検出 | **見逃し** | **見逃し** |
| M11 相対参照を含む名前を精密に扱う | 検出 | 検出 | **見逃し** | **見逃し** |
| M12 `sum_range` を左上だけにする | 検出 | – | – | – |

- M12 は等価ミュータントである（resize 後の矩形が読まれる範囲を覆うので、健全性は変わらない）。
- 全体では、どのミュータントも何らかの試験で落ちる。独立した oracle の穴については R4-m1 で述べる。

---

## R4-M1 [Major] 重複した `<v>`/`<is>` を持つ依存先で、stale 値が残る

**根拠**: `edit.rs` の apply (iii)（L310-322 付近）は、`c.remove_cell_value(); c.remove_inline_string();` を**1回ずつ**しか呼ばない。

一方 `worksheet.rs` の `write_cell` は、「a schema-invalid duplicate would otherwise survive and be read by others」として、すべての `f`、`v`、`is` を消している。census も（`has_cache` によって）そのセルを依存先として扱い、strip の計画に入れている。

**再現**（`zz_probe4.rs`）:

- **Q1**: `B1: <f>A1</f><v>1</v><v>1</v>`。`A1` を 99 にすると、`dependents=[B1]`、`Ok(())` で、出力は `<c r="B1"><f>A1</f><v>1</v></c>` になる。
- **Q1b**: 同じことが、`t="str"` の依存先と、CSE の result セル（`C2: <v>0</v><v>0</v>` → `<v>0</v>` が残る）でも起きる。
- **Q2**: `<is>` が2つあると、`t` は消えるが、2つ目の `<is><t>y</t></is>` が残る。

**影響**: 出力には、`v` を1つだけ持つ schema 的に正しいセルとして古い値が残り、calamine などの非計算 reader はそれを読む。入力は schema 違反だが、§3.3 の主張（依存先はキャッシュを失う）にはこの前提が書かれておらず、同じ crate の中で `write_cell` は対処している。

**提案**:

- apply (iii) を `while c.remove_cell_value() {}`、`while c.remove_inline_string() {}` にする。
- O1 の合成 workbook に、重複した `v` を持つ依存先と result セルを1つずつ足す。

バイト比較の oracle（`stripped` は `v`/`is` をすべて取り除く）は、この fixture があれば検出できる。

---

## Minor

### R4-m1 独立した oracle が届かない規則がある

- **O3**: `kept()` は `f` を持つセルしか比べない。そのため、array/spill の **result セル**（`f` を持たない `v`）と、spill の潜在領域の規則（M4、M10）は、O1 の手書きの期待値でしか守られていない。O2 も、評価器が動的配列を持たないため見逃す。
  - 提案: O3 の比較に、入力で `f` を持つか result range に含まれるセルを加える。O1/O3 の合成 workbook に、`SEQUENCE` の spill が編集で伸びるケースを加える。
- **範囲演算子 `Expr::Range`**（M7）: 単体試験でしか試されていない。O2 の生成器は `Sheet1!A1:Sheet1!B3` のような、2つの参照トークンを `:` でつなぐ形を作らない。
  - 提案: 生成器にこの形を足す。
- **M11**（相対参照を含む名前）: O2/O3 では評価できず、O1 と単体試験で守られている。設計どおりの限界なので、記録だけでよい。

### R4-m2 O3 が output のバイトに縛られている

- `phase1_o3` は output の FNV が違うと落ちる。そのため、意味の変わらないリファクタ（剥がし方の変更、属性順など）のたびに CI での再生成が要る。
- ミュータントも、意味論ではなくハッシュの不一致で落ちる。
- ADR §7.1 は「a change to the O3 inputs fails」と書いているが、実際には output の変化でも落ちる。

LibreOffice の hard recalc の結果はキャッシュに依存しない。そのため、照合キーを「input のハッシュ + output の式テキストと非式の値（キャッシュを除く）のダイジェスト」にすれば、意味論の検査を保ったまま、再生成を本当に必要なときに限れる。

### R4-m3 O3 の揮発性の除外が、定義名を見ていない

`has_volatile` は sheet part しか走査しない。`definedName` の中の `OFFSET(`/`INDIRECT(` を経由する揮発性は除外されない。起きるのは偽陽性（試験が落ちる）だけで、今のデータは pass している。

### R4-m4 ADR §2.4 (a) の根拠が、O3 のデータと食い違う

- ADR は「LibreOffice relative to the calculating sheet」と書いている（round 3 の私の推定に由来する）。
- O3 の `o1_name_scope-S1_A1-v0` では、`S1!A1` を 1000 にしても、LibreOffice は `S1!C1`（`=S2!Loc`、`Loc` は `S2` のローカル名で `$A$1`）を **7**（= `S2!A1`）と計算した。LibreOffice は、名前のスコープのシートに束ねている。
- 規則（`Everything`）は安全側なので、そのままでよい。根拠の文を「Excel の挙動は未検証、LibreOffice はスコープのシート（O3 で観測）。読み手で食い違いうるので `Everything`」に直す。

### R4-m5 ADR の文面に、rev 3 以前の記述が残っている

- Consequences: 「Until its data is committed, O3 is an ignored test」は、もう事実ではない。
- Open question 2: 「the JSON」とあり、データ生成はすでに完了している。Open questions 1〜4 には、round 3 の推奨（`KeepStaleCaches` は既定ではなく doc で警告、O3 は CI、閾値は不要、manual モードは現状維持）に沿った決定を書き、Accepted の時点で閉じる。
- revision 4 の注記: 「the data is generated … before Phase1 merges」→ 生成済み（run 43821205800）と書く。

### R4-m6 panic による poisoning の試験が無い

apply の前に poison し、成功したら解除する、という作りは、コードを読む限り正しい。しかし試験は `Err` の注入だけである。

`fault-injection` に panic を注入する変種を足し、`catch_unwind(AssertUnwindSafe(..))` の後で `to_bytes()` が `Err(Poisoned)` になることを確かめる試験を1本足すとよい。`package()` は doc で警告するだけなので、型で塞ぐ（poisoned なら `Err` を返すなど）かどうかは判断に委ねる。

### R4-m7 `fold` が Excel の同一視の上位集合であることは示されていない（要検証）

Q27: シート `Ａ`（全角）と `A` が両方ある場合、`Ａ!A1` と `a!A1` は別のシートに精密に解決される。

日本語版の Excel が名前の照合で幅を無視する（`NORM_IGNOREWIDTH` 相当）なら、Excel ではこの2つが曖昧になる。Excel 自身はそういうファイルを書かないので、実害は第三者が書いたファイルに限られる。

XL の確認項目に「全角/半角だけが違うシート名」を足す。

---

## 健全性ハント（依頼 (3)）で穴が見つからなかったもの

以下は、どれも依存先に入る（`Everything` を含む）か、`Err` になって何も変わらないことを確かめた（`zz_probe4.rs`）。

- **`Everything` になるもの**: `LET`/`LAMBDA`（`_xlpm.` を含む）、`A1#`、`_xlfn.ANCHORARRAY`、`_xlfn.SINGLE`、構造化参照、`@A1:A3`、空白による交差、`INDEX(..):A5`、`XFE1`、`A0`、`Sheet1!#REF!`、相対参照を含む名前、ローカル名の衝突（`L2` は参照の形 → `Everything`）。
- **依存先が正しく求まるもの**:
  - `SUM((A1,C1))` は精密に求まる。
  - 3-D（`S1:S3!A1` は `S2` の編集で依存先になる）。
  - 3シートをまたぐ連鎖（`S1→S2→S3→S1`）。
  - data table（無関係な編集でも result と依存先が剥がされる）。
  - 小文字の `r`。
  - `ref` の無い master。
  - stream 順で master より前にある member。
- **`Err` になり、出力が不変のもの**:
  - `sheetData` の中の `mc:AlternateContent` で包まれた行やセル。census には見えている。他のシートの編集でも、`strip_positions` との不一致で `Err` になる。
  - `r` の無いセルや行（`EditBlock`）。
  - `sheetData` が2つあるもの（census と tree の不一致）。
- **共有式の hand-off**: 範囲外への移動、名前との衝突、master が2つの場合は、どれも `Err` で不変である。fault injection では6つの step すべてで poison される。
- **calcMode manual**: 剥がすが、`fullCalcOnLoad` は立てない（O1）。O3 でも、`o1_manual` の感度のあるセル3つは剥がされている。
- **`Everything` の閉包のメモ化**: 無効化（`add`）が無くても、変化は削除（上位集合なので安全）か hand-off（辺が不変）だけで、stale を生む経路は無い。

## Err-with-mutation とバイト比較の oracle（依頼 (4)）

- plan の検査は apply の前にすべて終わる。apply の中の `?` は、plan が成功を示したものだけである。strip は要素の位置（行とセルの item index）を変えないので、target の計画の位置は有効なままである。
- 「`Err` なら何も変わらない」は、P5、P6、Q24b、Q24c、Q29 で確認した。
- バイト比較の oracle（`assert_allowed_diff`）は、次のすべてをバイト単位で比べる。
  - 対象シート（T2 + `dimension`）
  - 他のシート
  - `workbook.xml`
  - `[Content_Types].xml` とワークブックの `.rels`（要素1つと、その前の空白）
  - それ以外の part（L1）

  例外は hand-off の新しい master の span だけで、これも別の試験が `s` の消失を検出した。oracle の残りの穴は fixture の側にある（R4-M1 の重複 `v` が無い）。

## 判定

- **converged: no.** R4-M1 が1件（数行の修正と、fixture 1つ）。
- **ADR Accepted: yes.** R4-m4（§2.4 (a) の根拠の文）と R4-m5（古い記述と Open questions の締め）を直すことが条件。設計と規則の変更は要らない。R4-M1 は §3.3 の記述どおりに実装を合わせるだけである。

## 再現物

scratchpad の `review-pr20-r4/`:

- `wt/crates/ooxml-xlsx/tests/zz_probe.rs`: round 3 の probe。
- `wt/crates/ooxml-xlsx/tests/zz_probe4.rs`: Q1〜Q32。
- `wt/crates/ooxml-xlsx/tests/zz_o3.rs`: 集計と、意味論版の O3。
- `mutants.py`: M1〜M12。
- `mut.sh`: strip/hand-off の属性ミュータント。
- ベンチ: `examples/phase1_bench.rs`（通常と `WORST=1`）、release build、この sandbox で計測。
