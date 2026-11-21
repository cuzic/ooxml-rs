# PR #99 レビュー（Opus, round 2）

対象: impl/docx-phase3c-revisions @ cb1d4077b4dd1cc63613986af35ec9b0c3ce8cd8

## 結論（先に）
round 1 の M1・M2 は挙動で解消を確認した。m1〜m6 と指示された Nit も直っている。
新しいコード（行のリビジョン範囲チェック、入れ子の行への削除マーク、生きている行の数え方）と
origin/main（#112〜#119）のマージを壊しにいったが、Blocker と Major は見つからなかった。
残りは Minor 2 件と nit 1 件で、follow-up でよい。converged: yes。

## 前ラウンドの解消状況
- M1 解消: `pPr/sectPr` を持つ段落の `delete_paragraph` は `TrackedSectionBreakDeletion` を返し、出力はバイト同一（プローブ）。sectPr 段落の直前の段落は従来どおり削除できる。§5.2 の表に段落削除の行が追加された。
- M2 解消: `check_deletable_paragraph` が各 run（hyperlink の中も）に `check_text_only` をかける。ruby、`mc:AlternateContent`、hyperlink の中の ruby はすべて `RunNotTextOnly` を返し、出力はバイト同一（テストとプローブ）。`instrText` は `StaleFieldRisk` で先に拒否される。
- m1 解消: `track_changes(a, None)` で `w:date` にプロセスのローカル時刻が入る（プローブ）。
- m2 解消: ADR-0004 に post-acceptance note が追加された（`rPrChange`、`pPrChange` または段落記号のリビジョン、move/customXml の範囲）。
- m3 解消: `format_view` で書式変更の accepted view と rejected view を比べ、セル内 append のテストもある。N1、段落の挿入と削除、行の挿入と削除、書式変更に `assert_local` が付いた。
- m4 解消: 範囲の外に生きている行が無ければ `TableWouldBeEmpty` を返す。0..1 を削除した後の 1..2 はバイト同一で拒否される。
- m5 解消: `row_positions_in_revision_ranges` で、範囲内の行、テンプレート行、挿入位置（表の末尾を含む）を拒否する。プローブした配置: 本文から表をまたぐ範囲、行の間で始まる範囲、行の間で終わる範囲、セルの段落の中で始まる範囲、`</w:tbl>` の直前で始まる範囲。どれも正しく拒否されるか、正しく通った（範囲の外への挿入と、範囲の外の行の削除）。`append_paragraph` の P のチェックにはコメントが付き、テストもある。
- m6 解消: XPath 3 件と、本文の途中に section break がある case 36 が追加された。CI の libreoffice ジョブは pass。
- Nit: n2（日付のエラー文言）、n3（rustdoc）、n4（入れ子の行も `trPr/del`。パスがずれないよう逆順に書く）、n6（`#[non_exhaustive]`）、n1（ADR-0002 のコメント）は対応済み。n5 と n7 は未対応で、Codex の判断に任されている。どちらも fail-closed。

## Findings

### m1（Minor）: tracked `delete_rows` は縦結合を直さないので、accepted view が untracked の削除と食い違う
- 根拠: 1 列の表で、行 0 が `vMerge='restart'`、行 1 と行 2 が `vMerge` continue の状態で、`delete_rows(t, 0..1)` を呼ぶ。
  - tracked: `Ok`。行 0 に `trPr/del` が付くだけで、行 1 の continue はそのまま残る。Accept All の後は、restart の無い continue から表が始まる。
  - untracked（`Document::delete_rows`）: 行 1 を `restart` に直す（`tables.rs:552-611`）。
  - T-REV の「accepted view = untracked の編集」がこの入力では成り立たない。ADR §5.2 の行削除の行は縦結合について何も書いていない。
  - 先頭の continue を Word が restart として描くなら、見た目の差は無い *(u)*。データは失われない。
- 修正案: 削除する範囲に restart があり、その結合が範囲の外へ続くときは拒否する（fail closed）。あるいは、§5.2 の行削除の行と post-acceptance note に、Accept での結合の扱いは Word に任せると書き、manual-check のケースを追加する。

### m2（Minor）: `insert_rows` の新しい 2 つの拒否を守るテストが無い
- 根拠: `tables.rs` で次の 2 つを手で無効にしても、`phase3c` の 19 テストはすべて通った。
  - 挿入位置のチェック `index.check_row_revision_range(&[path, &[pos]].concat())`
  - テンプレート行の中の範囲マーカーを探す `find_desc(tr, range_marker)`
- 既存のテストでは、範囲がテンプレート行を囲むので、テンプレート行のチェックが先に拒否してしまう。
- 挿入位置のチェック自体は正しく動く。`<w:tr/><w:tr/><w:customXmlInsRangeStart/></w:tbl>`（範囲の終わりは表の後ろ）に対する `insert_rows(t, 2, 1, Above)` は `InsideRevision` を返し、出力はバイト同一。チェックを外すと `Ok` になり、挿入した行が範囲の中に入る。
- AGENTS.md は、新しい拒否ごとに Err ⇒ byte-identical のテストを求め、新しいコードごとに落ちる変異を求めている。
- 修正案: 上の配置と、セルの段落の中に範囲マーカーがあるテンプレート行のケースをテストに追加する。

### n1（nit）: 他の open PR との干渉
- #122 も ADR-0015 の末尾に post-acceptance note を追加するので、後からマージする方でテキストの衝突が起きる（手で解ける）。
- #121 と #102 は `codegen/report.txt` と `tools/lo-check/expected.json` に触れる。衝突したら再生成とマージで解く。

## main のマージ（ebd2425）
- `git show --remerge-diff` は空で、手で解いた衝突は無い。main は docx に触れていない（allowlist dml/pml/sml と report だけ）。
- 手元で `cargo xtask codegen --check` が通る（9 files up to date）。
- `expected.json` の差分のうち、PR 由来のものは docx のエントリだけ。pptx 08 の削除と p3c-links、xlsx 39 は main 由来。
- CI: libreoffice、python-docx、python-pptx、openpyxl の各ジョブは pass。test/clippy/codegen のジョブはこのレビューの時点で pending。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 1）
