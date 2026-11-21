# PR #140 レビュー（Opus, round 1）

対象: impl/xlsx-phase7a-rows-cols @ 7c5c750cb7fceda2c378546f98eda2ad02ce0698

## 結論（先に）
converged: **no**。挙動の誤りは見つからなかった。確かめた範囲は、全シートの数式、名前、shared・array・data-table、§3.5 の範囲機能、拒否の byte 一致、コーパスである。
残るのは Major 1 件で、ADR の 3 つの規則（§3.3.4 CF/DV のアンカーの hand-off、§3.3.4 複数範囲の拒否、§3.5 merge の新アンカーの拒否）をどのテストも固定していない。変異を入れても全テストが通る。テストを足すだけで直る。
Minor は 4 件で、follow-up でよい。CI は全 5 ジョブ pass。`git merge-tree` で origin/main（58decb5）と衝突しない。

## 前ラウンドの解消状況（PR #125 round 4）
- **m1 解消**: `edit.rs` の判定は `let unshifted = edit.is_none();` に戻った。テスト `shifted_write_skips_old_master_dependents_and_chain` は、`handoffs`・`strip` が空で `chain` が `None` であることを確かめる。
- **m2 解消**: 次の 3 つは削除済み。
  - `plan_strip_with_index` の shifted 分岐
  - `PartNotFound` の許容
  - `refs_delta += strip.shared_refs - stripped_refs`
- **P7b 向けの注意**: 動的配列の書き込み経路は P7a に無い。shifted sheet への `CellWritePlan` も P7a は使わない。注意はそのまま P7b に持ち越す。

## 確かめたこと（指摘ではない）

### 1. 数式の書き換え（プローブ、コミットしない）
- **3-D 参照は `UnrewritableReference` で拒否、出力は byte 一致**。次の 4 箇所で確かめた。
  - セル: `SUM(S1:S3!A5)` で S2 を編集（中間）。`S1:S2!A5` で S2 を編集（端）。
  - 名前: `S1:S3!$A$5`。
  - 他シートの CF: `SUM(S1:S3!$A$5)`。
- **`[0]` の自己参照も拒否**。
- **他シートの shared group**: `D!A1` の group で D の行を挿入・削除すると、規則（§3.3.2 step 3）どおりに分割される。削除で消えた参照は `D!#REF!` になる。
- **2 次元の group**: master の列や行を削除すると、row-major の先頭の生き残りが master になる。`#REF!` は translation で等しいとみなされ、group は保たれる（B2:B3 と B2:C2）。
- **名前**:
  - sheet scope と workbook scope の名前を書き換える（`'D'!$A$5:$A$9` → `$A$8`）。
  - Print_Area を丸ごと削除すると `ReferenceDeleted` で拒否、出力は byte 一致。
- **端の扱い**:
  - `A2:A1048576` と `B1:XFD1` の終端は固定される（列の挿入で `C1:XFD1`）。
  - `$A$1048576` を含む数式があると、行の挿入は `ShiftOffSheet` で拒否（ADR どおり）。
  - 末尾 7 行を削除すると `SUM(#REF!)`。他シートでは `D!A1048570:A1048576` → `D!#REF!` になり、prefix が残る。
- **census の事前フィルタ**: 参照を値として読まない関数でも、書き換え漏れは無かった。
  - 対象の関数: `ROW`・`ROWS`・`ISREF`・`CELL`・`OFFSET`・`FORMULATEXT`・`SUMIF` の第 3 引数・`LET`・`IF(FALSE,…)`・`INDEX(…):A10`・spill `A10#`・交差・union など 31 種。
  - 編集したシートと他シートの両方に置いて、すべて書き換わった。
- **data table**:
  - `r1` は移動する。
  - 入力セルを削除すると `ReferenceDeleted`。
  - 内側に挿入すると `ArrayFormulaSplit`。
  - 丸ごと削除すると要素ごと消える。

### 2. 範囲機能（プローブ）
次はすべて ADR どおりだった。
- merges: 範囲をまたぐ削除で縮み、1 セルになるか丸ごと消えると要素を削除。`count` も更新。
- CF/DV: `sqref` を書き換え、空になった規則を削除。アンカーが消えたときは translation してから書き換える（`B3>A3` が保たれる）。
- hyperlinks:
  - `ref` が消えた要素を削除。
  - 他シートの `location` は書き換える。丸ごと消えるときは `ReferenceDeleted`。
- breaks: `id`、`count`、`manualBreakCount` を更新。`max="16383"` は固定。
- `ignoredErrors`: 書き換える。
- `protectedRange`: 空になるときは `StructureBlocked` で拒否。
- autofilter:
  - 先頭列を削除すると `colId` を詰める。
  - フィルタ列を削除すると拒否する。
  - ヘッダー行を削除すると拒否する。
- views:
  - frozen pane の `ySplit` を増減する。
  - `topLeftCell` と `activeCell` は削除時に clamp される。
- `customSheetViews`: pane、selection、breaks、autoFilter を書き換える。
- `cols`:
  - 内側への挿入で分割し、新しい列は書式なし。
  - `max="16384"` は固定される。
  - 削除で縮む。

### 3. 拒否
- 保護:
  - `sheet="1" insertRows="0"` なら挿入は通る。
  - 削除と列の挿入は `Protected { scope: "worksheet" }` で拒否、出力は byte 一致。
- コーパスの 996 件の拒否は、テスト自身がすべて byte 一致を assert している。今回の再実行でも通った。

### 4. calcChain と Phase1/Phase2
- edited sheet を読む数式は、すべての dependents のキャッシュを落とす。`seed_all` を使うので、帯より前を読む数式も含む（ADR §3.7 の「edited sheet に触れる数式」どおり）。
- 他シートだけを読む数式（`1+1`）はキャッシュを保つ。
- `fullCalcOnLoad="1"` は既存の `calcPr` に足される。
- calcChain は数式セルが動いたときに外される。
- `sst/@count` の減算はテスト済み。

### 5. コーパス（`OOXML_XLSX_CORPUS=/tmp/poi-xlsx`、`--ignored`、release、54 s）
- 件数は PR 本文と完全に一致した。success 2,248、StructureBlocked 633、Protected 168、MissingReference 60、UnrewritableReference 47、NotAWorksheet 40、IterativeCalculation / SharedWorkbook 各 12、FormulaScanFailed 8、ArrayFormulaSplit 6、ShiftOffSheet 5、PartNotEditable 4、ReferenceDeleted 1、open refused 32。
- **4 操作すべてを拒否するシートは 235**。位置に依らない分類は次のとおり。
  - MissingReference 15
  - oleObjects 6 / controls 2 / AC 2
  - dataConsolidate 1
  - 合計 26 で、ADR の Consequences（26 of 813、ただし ole/controls/AC は 8）と合う。
  - `Protected` は 42 シート（lockStructure 35 + シート保護 7）。
  - 残りは P7b の対象である。内訳は anchor 59、chart の `f` 18、pivot `location` 11、表、`singleXmlCell` など。
- **openpyxl 3.1.5 の読み戻し**: 2,237 / 2,248 が開けた。数式は、独立に書いた正規表現オラクル（行・列の挿入・削除の規則）で照合した。
  - 照合した数式は 202,913 個、不一致は 33 出力。
  - 不一致はすべてオラクル側の限界だった。名前（`NR_Global_B2`）、列全体・行全体の参照、openpyxl の shared formula の translation（改行や空白の扱い）、`=` で始まる文字列セルである。
  - 抜き出して原本と比べると、どれもこのツールの出力が正しかった。例:
    - `NewlineInFormulas` で、shared group を分割して `B1\n+#REF!` を書く。
    - `conditional_formatting_multiple_ranges` で、アンカーが残る複数範囲の CF の相対参照が `#REF!` になる。

## Findings

### M1（Major）: ADR §3.3.4 と §3.5 の 3 つの規則を固定するテストが無い
- 根拠: 変異を 1 つずつ入れて、`cargo test -p ooxml-xlsx --release --lib --test phase7_rows_cols` を走らせた（コミットしない）。
  - **M-a**: `reanchor` で translation をしない（`if !deleted(anchor) || !others.is_empty()` → `if true`）。
    - lib 84、phase7 12 が全 pass。
    - 単一範囲の CF/DV でアンカー行を消すと、規則が `#REF!` になるか、別のセルを指す。`Ok` のまま黙って誤る。
  - **M-b**: 複数範囲の `sqref` の拒否を外す（`if !others.is_empty() && …` → `if false && …`）。全 pass。
  - **M-c**: merge の新アンカーの値の検査を外す（`if has_value(…)` → `if false && …`）。
    - 全 pass。隠れていた値が表に出る（review m14 の理由）。
  - 対照: `cols` の分割を外す変異（M-d）は `column_formats_split_and_new_columns_are_unformatted` で落ちた。
- 今の挙動は正しい。`B3:B8` / `B3>A3` で 3 行目を削除すると `B3:B7` / `B3>A3` になる。
- Issue の変異リストに無い規則だが、どれも `Ok` での黙った誤りを防ぐ規則である。
- 修正案: 3 つのテストを足す。
  - 単一範囲の CF と DV（x14 を 1 つ含める）で、アンカー行を削除して数式を確かめる。
  - 複数範囲で片方のアンカーを消すと `StructureBlocked` になり、出力が byte 一致することを確かめる。
  - merge の先頭行を消して新アンカーに値があると `StructureBlocked` になり、出力が byte 一致することを確かめる。

### m1（Minor）: `lockStructure` で G1 を拒否するのは、ADR 本文（§2.4、Q13、Consequences）と違う
- ADR の Q13 と §2.4 では、`lockStructure` で拒否するのは `copy_sheet`（と ADR-0009 のシート操作）だけである。G1 はシート保護の属性で決まる。
- Excel でも、ブックの構成の保護は行の挿入を止めない。
- 実装では、`sheet_structure_guard` によって G1 も `Protected { scope: "workbook" }` になる。コーパスでは 140 操作（35 シート）にあたる。
- 安全側の過剰拒否だが、Accepted ADR の規則を P7a の実装注記だけで変えている。
- 修正案: どちらかにする。
  - オーナーに確認して、§2.4・Q13・Consequences の本文を直す。
  - G1 からこの guard を外す。

### m2（Minor）: 1 行挿入が 2.44× になる主因は、構造編集の後の rebuild がシートを 2 回直列化すること
- 再計測（release、同じ入力）: baseline 1.76〜1.95 s、insert 4.40〜4.49 s、delete 2.59〜2.63 s。PR の数値と合う。
- 内訳（時間計測を一時的に入れた）:
  - validate 0.18 s
  - objects と completeness 0.08 s
  - apply 0.25 s
  - **`rebuild()` 1.17 s**
  - save 0.93 s
- `rebuild` の中の `Loaded::load` → `census::scan_sheet(&pkg.read_part(part)?)` は、dirty な DOM を一度直列化してから走査する。save でもう一度直列化する。
- 比較の基準:
  - セル 1 つの書き込み + save は 1.43×（dirty なシートの直列化と deflate の下限）。
  - **最終行への 1 行挿入でも 2.0×**。
  - なので、今の設計では目標の 2× に届かない。
- ADR §3.8 は rebuild を「open cost: 0.29 s」と見込んでいたが、実際は約 1.2 s。
- 非ゲート（Q10）なので follow-up でよい。
- 修正案（どちらか）:
  - rebuild で作った直列化の結果を、save まで dirty でなければ使い回す。
  - census を DOM から直接走査する。
  - どちらも約 1 s（0.5〜0.6×）減る見込み。

### m3（Minor）: 整形された XML での削除は O(k·n) になる。結果は正しく、拒否でもない
- 再現: ベンチの入力の行の間に `\n    ` を入れ、`delete_rows(1, n)` を計測した。

  | n | 整形なし | 整形あり |
  | ---: | ---: | ---: |
  | 5,000 | 3.17 s | 4.02 s |
  | 40,000 | 1.59 s | 7.56 s |
  | 99,999 | 0.85 s | 5.43 s |

- 原因: `remove_indices` は連続した child index だけをまとめる。空白のテキストが間にあると 1 行ごとに `remove_range` を呼び、そのたびに `Vec` をずらす。
- 出力は正しい。openpyxl で読み戻すと、行 1 の次が旧 40001 行。`dimension` も正しい（`A1:J60000`）。
- 削除した行の間の空白テキストは残る（無害）。
- ADR §3.8 の「親ごとに 1 回で削除」とは違う。100 万行の整形済みシートでは二乗で伸びる。
- 修正案: 削除する要素どうしの間にある空白だけのテキストも、run に含めて一緒に消す。core の bulk retain を待つ必要はない。

### m4（Minor）: 最終列だけの `col`（`min=max=16384`）があると、列の挿入が `ShiftOffSheet { what: "numeric range" }` で拒否される
- 根拠: `51585.xlsx` Sheet1 の `insert_cols`。最後の `col` は `min="16384" max="16384"`。
- ADR §3.2 は、`cols` のように内容を持たない範囲は「拒否せず、固定または clip する」としている。
- `numeric_shift` は、範囲の始点が端を越えると拒否する。
- 安全側の過剰拒否である。エラーの `what` も、どの要素かを示していない。
- 修正案: どちらかにする。
  - 始点が端を越える `col` は削除する（clip）。
  - 固定にする。
  - どちらの場合も、`what` に `col 16384:16384` のように要素を書く。

### nit
- `selection/@sqref` が `A2:A2` のまま書かれる（1 セルに正規化しない）。有効な値ではある。
- PR 本文の「801 ワークシート」と ADR Consequences の「813」、ole/controls/AC の「10」と「8」の違い。理由を ADR の実装注記に一行書くとよい。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 4 / nit 2）

M1 のテストを足せば収束する見込み。m1 は、マージ前にオーナーの判断を ADR に記録することを勧める。
