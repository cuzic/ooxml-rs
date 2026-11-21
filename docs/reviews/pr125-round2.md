# PR #125 レビュー（Opus, round 2）

対象: impl/xlsx-phase7-pre @ fe7715b2b863fec768af10cba5fc96603c24bf80

## 結論（先に）
converged: **no**。round 1 の Major 3 件と Minor・nit は、挙動で確かめて解消していた。コーパスは main と行ごとに突き合わせ、round 1 と同じ数値になった。新たに拒否されるのは `Protected` だけである。
一方、新しい `plan_writes_after` に Major が 1 件ある。index map を 1 つ渡すと、shared formula の hand-off・依存セル・calcChain の扱いが、プラン全体で止まる。そのため、**ずらしていない別シート**の shared master に書くと、`Ok` のまま残りのセルの数式が失われる。修正は小さい（判定を書き込みごとにする）。直せば Ready にしてよい。

## 前ラウンドの解消状況
- **M1 解消**:
  - 行とテスト:
    - `LOCATIONS` に L1〜L74 の全 id がある（L74 を含む）。
    - `OPERATION_RULES` に G1・Copy・テーブル名変更の列がある。
    - ADR に展開表（`BEGIN QUALIFIED LOCATIONS`）が追加された。`each_qualified_location_and_its_rules_match_the_adr` は、この表とデータを行ごとに文字列で比べる（part・scope・path・attr・syntax・3 列・rename・delete・URI）。
  - 変異（コミットはしない）:
    - L66 の attr を変える → 落ちる。
    - L30（Choice）の delete を Keep にする → 落ちる。
    - L69 の行を削除 → 3 件落ちる。その中に `structural_scan_checks_ignored_errors_uri` がある。
    - L69 の URI の対応を 68 にする → 落ちる。
  - sortState の退行:
    - commit の拡張走査が削除され、round 1 のプローブは Ok に戻った。
    - テスト `sheet_operations_preserve_sort_state_extensions_and_rewrite_wrapped_controls` がある。
    - ADR-0009 の note に「位置の拡張検査は構造編集だけ」と記録されている。
- **M1 のコーパス**: `OOXML_XLSX_CORPUS=/tmp/poi-xlsx` で、`phase7_probe` を origin/main（745f3a5）の worktree とこの head で実行し、行ごとに比べた。
  - 対象: 334 ブック、811 シート。
  - rename: 一致 760、同じエラー 10、ERR→OK 6、OK→`Protected` 35。
  - delete: 一致 582、同じエラー 191、ERR→OK 3、OK→`Protected` 33、`LastVisibleSheet`→`Protected` 2。
  - OK→ERR は `Protected` だけである。`#[ignore]` の 2 テストも通過した。
- **M2 解消**: `Data!#REF!` と `'My Data'!#REF!` を出力し、テストで固定している。
- **M3 解消（ただし下の M1 を参照）**:
  - `plan_writes_after(edit, writes, headers, registry)` がある。
  - `registry`（sst の予約、`refs_delta`）を受け継ぐ。
  - `workbook.xml` のステージは 1 つで、date1904 と fullCalcOnLoad の両方が同じ文書に入ることをテストが確かめている。
  - 列の写像を壊す変異（`predecessor` の Column を恒等写像にする）は落ちる。
- **m1 解消**: clause 4 を 334 ブック・811 シートで言い直し、10 件の内訳を ADR に書いた。
- **m2 解消**: clause 2 と clause 3 の重なりと、エントリ単位での比較を ADR に書いた。
- **m3 解消**: main を取り込み済み。`git merge-tree origin/main HEAD` は衝突しない。
- **m4 一部解消**: `rewrite_index(.., range_end)` が端に固定する。テスト `numeric_range_ends_pin_and_clip` がある。残りは下の m2。
- **m5 解消**: GUID のテストがある。読み飛ばしを無効にする変異は落ちる。
- **m6 解消**:
  - 単一の `mc:AlternateContent` の Choice と Fallback にある `controls` は書き換わる。main は拒否していた。
  - プローブ: 名前を `A&B<'x` にすると `linkedCell="'A&amp;B&lt;''x'!B1"` になり、削除では `#REF!` になる。
  - 残りは下の m3。
- **m7 解消**: `x14::URI_SPARKLINE` を共有している。#129 との関係は下の m5。
- **n1 解消**: `partial_escape` を使い、属性の区切り文字の引用符だけを `&quot;`/`&apos;` にする。プローブで、CF の `"a&amp;b"` がそのまま残った。
- **CI**: `gh pr checks 125` はすべて pass（test/clippy/codegen を含む）。

## Findings

### M1（Major）: `plan_writes_after` に index map を渡すと、すべてのシートで shared formula の hand-off が止まる。ずらしていないシートの shared master に書くと `Ok` で数式が失われる
- 根拠:
  - 該当箇所は `edit.rs` の `plan_cell_writes`。次の 3 つを、プラン全体の `edit.is_none()` で判定している。
    - shared group の hand-off: `if edit.is_none() && let Some(fid) = target`
    - 依存セル: `let d = if edit.is_none() && …`
    - calcChain: `formula_set_changes && edit.is_none()`
  - 書き込み先のシートが `edit.sheet` かどうかは見ていない。
  - doc コメントの約束は「**ずらした座標**については呼び出し側が materialization を計画する」である。ずらしていないシートへの書き込みは、その約束の範囲外になる。
  - プローブ（unit test を一時的に追加。コミットはしない）:
    - "Shifted" と "Other" の 2 シートのブックを作った。"Other" の中身は `o1_shared.xlsx` のシートで、B2 が `B2:B5` の shared master である。
    - "Shifted" に `Shift::Insert { row, at: 5 }` の map を渡し、"Other"!B2 に Text を書いた。
    - map なし（`None`）: B3 が `<f t="shared" si="0" ref="B3:B5">A3*2</f>` になる（hand-off される）。
    - map あり: B3 は `<f t="shared" si="0"/>` のまま。si=0 の master が無くなり、B3:B5 の数式が失われる。結果は `Ok`。
  - 公開 API からはまだ呼べないが、P7b/P7c がそのまま使う Delivery 0 の成果物である（round 1 の M2 と同じ扱い）。
- 修正案:
  - hand-off・依存セル・calcChain の判定を、書き込みごとの `shifted.is_none()` に変える。
  - または、`edit` があるときは全書き込みが `edit.sheet` 上にあることを検査して拒否する。
  - どちらの場合も、上のケースのテストを足す。

### m1（Minor）: index map の行方向と `successor` がテストで固定されていない
- 次の変異はどちらも全テストを通る（`edit::tests`）。
  - `predecessor` の `Axis::Row` を恒等写像にする。
  - `successor` を恒等写像にする。`successor` は `ArrayResultCell` の anchor の報告に使う。
- 既存の行方向テスト（`Delete { at: 0 }`）は、写像の前後どちらにも census が無いセルにしか書かない。
- 行方向で古いセルに当たるケースと、ずらした配列の結果セルに書いて anchor が新しい座標で報告されるケースを足す。

### m2（Minor）: `rewrite_index` は 0 始まりで、範囲の端が削除範囲に入ると `Remove` を返す
- 0 始まりの問題:
  - `col/@min`・`@max`（L8）は 1 始まりである。`col max="16384"` をそのまま渡すと `index == limit - 1` に当たらず `OffSheet` になる。
  - テストは `MAX_COLS - 1` でしか確かめておらず、round 1 で求めた `col max="16384"` のケースが無い。
- 削除範囲の問題:
  - `rewrite_index(4, Column, Delete{at:2,count:4}, true)` は `Remove` になる。
  - 始点が残る範囲なら、範囲の規則（§3.2）では `at-1` に切り詰めるのが正しい。テスト名は "clip" だが、切り詰めは挿入のはみ出しにしか効かない。
- 対処: P7a で使う前に、1 始まりの変換をどこでするかを決める。`range_end` のときは削除範囲の内側を切り詰める。両方にテストを付ける。

### m3（Minor）: Excel が実際に書く入れ子の `mc:AlternateContent`（controls → AC → Choice → control）は拒否されたまま
- プローブ:
  - `worksheet/AC/Choice/controls/AC/Choice/control/controlPr@linkedCell="Data!B1"` に対して、rename と delete はどちらも `UnrewritableReference` になった。
  - main でも同じ拒否なので、退行ではない。
- ADR-0009 の note は「both `mc:AlternateContent` branches」と書いている。入れ子の形が対象外であることを一文足すか、P7 の follow-up に回す。

### m4（Minor）: §2.2 の手書きの要約表のうち、Place・Scope・シート操作の列はデータと照合されていない
- テストが比べているのは次の 2 つだけ。
  - 展開表（データとほぼ同じ形のダンプ）とデータ。
  - 要約表の G1・Copy・Table rename の 3 列（`fields[5..8]`）。
- 要約表と展開表の間で、シートの rename/delete 列は食い違うことがある。例:
  - L63: 要約は X、展開表は Unaffected。
  - L58、L59: 要約は XX、展開表は Unaffected。
- 実際の挙動は、汎用の接頭辞走査や別のガードで拒否されていて、正しい（L63 はプローブで、main と同じく拒否された）。
- 展開表の前文に「シート操作の列は XML 走査の分類で、拒否は別のガードによる」と、該当する行を書いておくとよい。

### m5（Minor）: #129（P5d）と `crates/ooxml-xlsx/src/edit.rs` の 1 箇所で衝突する
- `git merge-tree HEAD pr129`（#129 @ b61cef8）で衝突が出る。場所は依存セルの計算の `let d = if …`。
  - この PR は `edit.is_none() &&` を足した。
  - #129 は `CellOp::DynamicFormula` の分岐を足した。
- 解消では両方を残す。M1 を直すなら、その判定は `shifted.is_none()` になる。
- #129 は sparkline の URI を、自前の `const URI`（`4fd2` は小文字）で持っている。`x14::URI_SPARKLINE` に寄せるのは #129 の側の作業である。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 5 / nit 0）

M1 を直してテストを付ければ、Draft を外して Ready にしてよい。Minor は follow-up でよい。
