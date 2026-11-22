# PR #152 レビュー（Opus, round 1）

対象: impl/pptx-ac-lifts-u4 @ 984d2b4753ffc90136f05640d9901bc9bd53eea3

## 結論（先に）
converged: yes。Blocker と Major は見つからなかった。AC の隣で `add_table_style` を追記する経路で、どの分岐が読まれても新しい定義は正しい位置に入る。`styleId` の衝突は Choice・Fallback・入れ子・`Requires` 付きの分岐のどこにあっても検出され、それ以外の拒否はすべてバイト一致だった。#149 m2 の 3 つの直接テストと `Of16-01.pptx` のテストは、どれも core の §4.2 の拒否まで実際に届いている（変異で確認）。指摘は #144 との機械的な衝突（nit）とテストの穴（nit）だけ。

## 前ラウンドの解消状況
- 初回ラウンド。#149 round 1 の m2（pptx の 3 か所）は解消した。core の `place_skipping` にある §4.2 の同スロット拒否（`lens.rs:772`）を `if false && …` に変えると、`section_insert_…`、`completing_adjustments_…`、`cache_run_insert_…` の 3 テストがすべて失敗する。SmartArt の公開テストは `UnsupportedContent` だけを受け付けるように固定された。

## 確認したこと（プローブと変異、コミットしない）
- **table style の追記。** PR のテストは Choice のみ・Fallback のみ・入れ子の 3 形を使う。これに次の 5 形を足して `add_table_style` を実行した。
  - Fallback 側の ID を大文字にし、前後に空白を付けたもの → `Ok`、衝突なし
  - Fallback が無く、Choice が 2 つあり、2 つ目に同じ ID があるもの → `Ok`、衝突なし
  - AC を既存の直下 `a:tblStyle` より前に置いたもの → `Ok`、末尾に追記
  - `Requires` の接頭辞が未束縛のもの → core の `UnresolvedSibling`、出力はバイト一致
  - ベンダー要素だけの分岐（unranked）→ core の `UnresolvedSibling`、出力はバイト一致

  PR のテストは、追記した要素の外側がバイト一致することを確かめるので、`def` の保存もそこで担保される。
- **`styleId` の変異。** 次の 3 つの変異は、どれも `table_style_append_reserves_ids_in_all_branches_and_preserves_wrapper` で失敗した。
  - 分岐の ID を予約から外す
  - 入れ子へ再帰しない
  - Fallback を見ない

  UUID のシードは直下の ID だけから作るので、AC の無い文書では従来と同じ順で ID が生成される。
- **`p:transition`。** 既存テスト `deckread.rs` の `core_slide_transition_limit_counts_alternate_content`（#149）が `bug68703.pptx` で `AlreadyPresent` とバイト一致を確かめている。PR は ADR-0019 に注記を足した。
- **`Of16-01.pptx` の位置指定挿入。** §4.2 の変異でこのテストが失敗するので、core の拒否まで届いている。pptx には `p:spTree` へ挿入する公開操作がまだ無いので、core の lens を直接呼ぶテストで足りる。
- **「Stays」の行。** 既存テストが拒否とバイト一致を確かめている。
  - 分岐内の書式・変形・表: `formatting.rs:566`、`shape_properties.rs:218`、`tables.rs:646`
  - theme: `styles_theme.rs:486`、`:509`
  - `notesMasterIdLst`: `notes_creation.rs:372`
  - リンク: 今回 `links.rs:541` が追加された（`refused` ヘルパーでバイト一致を確認）

  目的のテスト（lib、refusals、styles_theme、links、smartart、deckread）はローカルで全件通った。
- **オラクル。** CI の 5 ジョブがすべて緑だった。python-pptx の T3 は、`u4-alternate-table-style.pptx` の分岐と追記した定義を、python-pptx で再保存した後も読み戻せている。公開 API は増えていない（追加は private な `insert_cache_run` とテストモジュールだけ）。`codegen --check` も CI で通っている。

## Findings
### n1（nit）: #144 とテキスト上で衝突する（`oracle_cases.rs` と `check.py`）
- 根拠: `git merge-tree HEAD pr144` で、`crates/ooxml-pptx/examples/oracle_cases.rs` と `crates/ooxml-pptx/tests/oracle/check.py` の `main()` がぶつかる。どちらも末尾に呼び出しを 1 行足しているだけ（`check_alternate_table_style(outdir)` と `comments.main(...)`）。`slides.rs` は自動でマージされ、`mod tests` も 1 つだけになる。意味の衝突は無い。
- 修正案: 後からマージする側で、両方の行を残して解消する。

### n2（nit）: 分岐の ID の空白と大文字小文字を扱う経路にテストが無い
- 根拠: PR のテストは小文字の ID しか使っていない。`trim()` と `eq_ignore_ascii_case` の経路は、上のプローブ（`"  {UPPER}  "`）で正しく動くことを確かめたが、回帰テストは無い。
- 修正案: 3 形のどれかの ID を `"  {大文字}  "` にする（任意）。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
