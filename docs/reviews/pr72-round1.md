# PR #72 レビュー（Opus, round 1）

対象: fix/xlsx-ignorable-wrappers @ 5a10bfb08062cb68e155ed71b371d335314ccbf0

## 結論（先に）
converged: no。セル値の書き込み（単一 `set_*`・`write_cells` バッチ・スタイルのみ・依存セルのキャッシュ除去）は main 取り込み後も拒否され、`Err ⇒ byte-identical` も保たれる。
しかし拒否を `plan_target` に置いたため、同じ `SheetIndex` を使う Phase4 の行プロパティ編集と結合が素通りし、`Ok` のまま重複 `row` を書く（M1）。
また判定が「SML 以外の名前空間の要素の下」に限られ、SML 名前空間の未知要素や `sheetData` 自体を包むラッパーでは同じ重複が起きる（M2）。

## 前ラウンドの解消状況
初回のため無し。

## 検証の範囲
- origin/main（#67・#68・#77 入り）をローカルでマージ（衝突なし）した木で、スクラッチのプローブテストを実行（コミットしない）。
- 既存テスト `tests/ignorable_wrappers.rs` はマージ後も通る。
- #67 との関係: census の F-8 判定（`census.rs` の「wrappers such as mc:AlternateContent」腕）は直下の `row` の中のラッパー内 `c` の行番号ずれだけを見る。ラップされた `row` は census の対象外、ラップされた `f` は `ScanFailed` で全体拒否。本 PR の拒否とは重ならず、`check_editable()`（census）が先に評価されるので両方当たるときは census のエラーが出る。衝突なし。
- 公開名: 新しい variant は無く、既存の `Core(UnresolvedSibling(sheetData))` を流用。生成物の変更なし。
- 他の open PR で `index.rs`/`worksheet.rs`/`edit.rs`/`layout.rs` に触るものは無い。

## Findings

### M1（Major）: 拒否が `plan_target` にしかなく、`row` を作る/読む他の経路が素通りする
- 根拠: `layout.rs` の `set_row`（main 取り込み後 653–680 行）は `plan_target` を通らず `crate::worksheet::locate` → `SheetIndex::target_rows` で行を探し、無ければ `row` を挿入する。プローブ:
  `<row r="1">…</row><q:w xmlns:q="urn:x" mc:Ignorable="q" mc:ProcessContent="q:w"><row r="2"><c r="A2"><v>5</v></c></row></q:w>` に対し
  `set_row_height(1, 30.0)` が `Ok` で `…</q:w><row r="2" ht="30" customHeight="1"/>` を追加（`row r="2"` が 2 つ）。`set_row_hidden` も `Ok`。`set_row_outline` も同じ経路。
  さらに `merge("A1:A2")` は `filled_cells`（`range.rs:151`、`target_rows` を使用）がラップされた A2 の値を見ないので `Ok`、`merge_clearing("A1:A2")` も A2 を消さずに `Ok`（値の入ったセルを黙って結合範囲の非先頭に残す）。
  セル読み取り（`worksheet.rs` の `cell` → `locate`）も `ProcessContent` 付きラッパー内の B1 を `Ok(None)` と答える（`mc:AlternateContent` では読みも拒否している方針と不一致）。
- 修正案: 判定を `SheetIndex::target_rows`（`index.rs:208`）に移し、`alternate` と同じ所で `wrapped_grid` なら `UnresolvedSibling(sheetData)` を返す（`plan_target`・`set_row`・`filled_cells`・ハンドオフ・読み取りが一度に塞がる）。`worksheet.rs:232` の呼び出しは不要になる。`strip_positions` の `check_edit`（`rows()` 経由のため）は残す。`set_row_height`/`merge`/`merge_clearing` に `Err ⇒ byte-identical` の回帰テストを足す。

### M2（Major）: SML 名前空間の要素や `worksheet` 直下のラッパーは「foreign」と見なされず、同じ重複が起きる
- 根拠: `contains_wrapped_grid`（`index.rs:94`）は名前空間が SML でも MC でもない要素の下だけを見る。index は `sheetData` 直下の `row` と `row` 直下の `c` しか辿らないので、次の入力はどれも `Ok` で重複を書く（プローブで確認）:
  - `<sheetData>…<foo><row r="2"><c r="A2"><v>5</v></c></row></foo></sheetData>`（SML の未知要素）→ `set_number(A2)` で 2 つ目の `row r="2"`。
  - `<sheetData>…<extLst><ext uri="{X}"><row r="2">…</row></ext></extLst></sheetData>` → 同上。
  - `<worksheet><q:w mc:Ignorable="q" mc:ProcessContent="q:w"><sheetData>…</sheetData></q:w></worksheet>` → index は `sheetData` 無しと判断し、2 つ目の `sheetData` を挿入して `A1` を重複させる（census 側は `sheetData` の親を見ないので数式セルを登録しており、index と食い違う）。
  いずれもスキーマ外の入力だが、Issue #71 の趣旨（index が辿らない要素の中の `row`/`c` で fail closed）に当たり、出力は入力より壊れる。
- 修正案: 名前空間ではなく「親の不一致」で判定する。`worksheet` 全体を走査し、SML の `sheetData` の親が `worksheet` でない、`row` の親が `sheetData` でない、`c` の親が `row` でない場合に `wrapped_grid = true`（`mc:AlternateContent` の下は既存の `alternate` に任せる）。`sheetData` が見つからない分岐（`index.rs` の `else` 腕）でも判定する。上記 3 入力を回帰テストに加える。

### m1（Minor）: 依存セルのキャッシュ除去での拒否は範囲が広い
- 根拠: `strip_positions`（`edit.rs`, PR 版 649 行）の `check_edit` により、数式を持たないラップされたセルが 1 つあるシートに依存元があると、別シートの編集まで拒否される（`tests/ignorable_wrappers.rs:39` がこの挙動を固定）。配列の結果セルがラップされている場合に古いキャッシュを残さないための安全側の拒否としては妥当。
- 修正案: 今のままでよいが、PR 本文の拒否条件に「依存する数式がそのシートにあると他シートの編集も拒否」と明記する（将来 `rects` が無いときだけ緩める余地を Open questions に）。

### n1（nit）: `CellMut::set_string` のドキュメントだけに拒否条件を追記している
- 根拠: `worksheet.rs` の `set_string` の doc にだけ追記。`set_number`/`set_formula`/`write_cells` と、M1 修正後の `set_row_*`・`merge` には無い。
- 修正案: 共通の拒否説明（`UnresolvedSibling(sheetData)`）を `Worksheet` 型の doc か ADR-0003 の post-acceptance note に 1 か所で書く。

## 判定
converged: no（Blocker 0 / Major 2 / Minor 1 / nit 1）
