# PR #163 レビュー（Opus, round 1）

対象: impl/xlsx-ej-t5b @ d223e7b
converged: yes

## 結論（先に）
Blocker・Major なし。`XfContent` は P（`journal/project.rs` の `Styles::resolve_xf`）が `s` について比べるもの
（id 以外の `xf` 属性、numFmt コード、font/fill/border の木、`alignment`/`protection`、`xfId` 経由の
`cellStyleXfs` と名前）をすべて持ち、Base/Session の分け方も 1 セッション内の replay・truncation で安定している。
指摘は空テーブル時の重複追加（P では見えない）、防御的チェック、文書の 3 点の Minor と nit 2 点。

## 前ラウンドの解消状況
初回。

## 検証したこと（プローブ。コミットしていない）
- `cargo test -p ooxml-xlsx --all-features --lib xf_content_tests`: 8 件 pass。
- 失敗する要求（Session font + `NumFmtRef::Code` 新規 + 不正属性 `bogus`）: `Err(Malformed)`、`to_bytes` は同一。
  その後に同じコードで `set_num_format` すると id 165 になる（`next_numfmt` も巻き戻っている）。
- テーマ色 + tint、`charset`、`vertAlign`、`condense`/`extend`、`doubleAccounting`、10.5pt の font、
  `darkGray` + indexed/theme 色の fill、diagonal 付き border: どれも同じ workbook で元の index に戻り、
  styles.xml は変わらない（builder 出力 → reader → builder が同じバイト）。
- 空の styleSheet（fonts/fills/borders/cellXfs なし）: 往復は元の index、別 workbook への replay は P で等しい
  （ただし m1 の重複が出る）。
- dxf 用に採番したカスタムコードの後、同じコードをセルに設定: `Code` として取得され replay できる。
- Base `xf` の `borderId` がぶら下がっている場合: `set_style_index` も `set_font`（derive）も `Ok`、
  derive した session `xf` の `xf_content` は `DanglingReference`（m3）。

## 実装者の疑問への判定
- **属性を schema 順に書き直す**: 問題なし。P は属性を `BTreeMap` で比べ順序を見ない。書き直しは追加時だけで、
  既存の Base `xf` は `xf_key`（順序を無視）で再利用される（`round_trip_custom_styles` の xf 5 がこれを固定している）。
- **無い id を `Base(0)` として取り、明示的に `"0"` を書く**: 問題なし。P（`id()` は無ければ 0）も `xf_key` も
  無い id を 0 と見る。Base テーブルが空で id が無い場合だけ取得が `DanglingReference` になる（安全側の拒否）。
- **`base_style_counts` を open 時に呼ぶ必要**: 正しい。ただし Commit 後にも取り直す必要がある（m2）。
- **session numFmt を最初の session id で分類**: 健全。`next_numfmt` は `scan_ids` がパッケージ中の **すべての**
  `numFmtId`（`numFmts`、`xf` の参照、`dxf` を含む）の最大 + 1 から作るので、Base の `xf` が参照する id は必ず
  それ未満。dxf が採番した id は `numFmts` に入らず `cellXfs` からは参照されない。
- **空 fills テーブルの予約 fill が未テスト**: fills は `ensure_reserved` を `entry` より先に呼ぶので正しい。
  未テストで実際に崩れるのは fonts/borders と予約 `xf` 0 の方（m1）。
- **内容での dedupe がバイト再利用とは違う index を返す**: P では見えない。`xf_key` が等しければ 4 つの id・
  その他の属性・子が等しく、P の解決結果も等しい。返る index（`j < i`）が違っても F12 で session index は
  View に出ない。oracle 1 は §4.1 末尾どおり P2 の index を生成しないので、比べる対象にならない。
  一点だけ J 側への注意: Journal は live の shadow にも P2 を **内容で**（`set_cell_xf`）適用すること。
  live だけ `Index(i)` で適用すると、replay との間で参照される `xf` が変わり、R4 後のバイト（oracle 13/14）がずれる。

## Findings

### m1（Minor）: 空の fonts/borders/cellXfs への replay が予約エントリの重複を追加する
- 根拠: `crates/ooxml-xlsx/src/registry/styles.rs:1072-1092`。fonts と borders では `entry` の再利用判定が
  `ensure_reserved` より先に走るため、空のテーブルに `Session(Font::default())` を入れると予約 font 0 が作られた後に
  同じバイトの font 1 が追加される。結果として予約 `xf` 0 とも一致せず、新しい `xf` も追加される。
  プローブ: fonts/fills/borders/cellXfs のない workbook で `xf_content(0)` を別の空 workbook に `set_cell_xf` すると
  `s = 1`（0 にならない）、font と border も重複する。P では等しく、R4 が保存時に落とすのでファイルは壊れない。
  ただしテスト（1）の「追加しない」は空テーブルの Base では成り立たない。
- 修正案: `Part::Session` の font と border でも fill と同じく `entry` の前に `self.ensure_reserved(wb, Which::Fonts | Borders)`
  を呼ぶ（または `content` の最初で `ensure_reserved(CellXfs)` を呼ぶ。これは 3 つの依存テーブルも予約する）。
  そのうえで空 styleSheet の往復・replay テストを足す。

### m2（Minor）: `base_style_counts` の doc が「open 時」だけで、Commit 後の取り直しに触れていない
- 根拠: `styles.rs:1130`。ADR §10.1 では保存後に session items が新しい Base の番号の Base items になり、
  ログは空になる。古いカウントのままだと、保存後に生き残った session font を `Session(value)` と取得してしまう
  （P は壊れないが、F12 の「保存後は Base の番号」という前提と食い違う）。
- 修正案: 「open 時と、Commit が成功するたび（§10.1）に呼ぶ。Journal は Base と一緒に持つ」と書く。
  保存後のテストは Journal（J 系タスク）に回してよい。

### m3（Minor）: `Part::Base(i)` を Base のカウントではなく現在のテーブル長で検査している。またぶら下がった参照は取得できない
- 根拠: `styles.rs:1050-1057`（`base_ref` は `t.count`、つまり session の追加分を含む長さで比べる）。
  プローブ: session font を 1 つ追加した後で `Part::Base(base.fonts)` を渡すと `Ok` になり、黙ってその session
  エントリを指す。`xf_content` の出力からは起きないので Major ではない。逆に、Base の `xf` の `borderId` が
  ぶら下がっていて（`styles.rs:1212`）、そこから derive した session `xf` は、順次 API では `Ok` なのに
  取得が `DanglingReference` になる。P2 の op としては安全側の拒否で、出力は変わらない。
- 修正案: `XfContent`（または `set_cell_xf`）が `BaseStyleCounts` を受け取り、`Base(i)` を `i < base.*` で検査する。
  ぶら下がった参照は拒否のままにするなら、`xf_content` の doc の拒否一覧に追記する。

### n1（nit）: id の空白の扱いが `xf_key` と P で違う
- 根拠: `styles.rs:233` と `xf_content` は `trim()` してから parse するが、P（`journal/project.rs:175`）は trim しない。
  `fontId=" 1"` は registry では 1、P では 0。そのため、`" 1"` と `"1"` の Base `xf` が並ぶファイルでは、内容での
  再利用が P の上では違う `xf` を返しうる。実際のファイルではまず起きない。
- 修正案: P 側も `trim()` する（#162 か J 系のタスクで）。こちらはそのままでよい。

### n2（nit）: テストの `resolve` は P の近似で、`xfId` を `cellStyleXfs` まで解決しない
- 根拠: `styles.rs` のテスト `resolve`。`xfId` を属性の値として比べるだけ。`cellStyleXfs` はセッション中に
  追加されないので今は同じ意味になるが、受け入れ条件（2）は「P の解決」と書いている。
- 修正案: コメントに「`xfId` は Base の index（`cellStyleXfs` はセッションで追加されない）なので値の比較で足りる」と書く。

## 他の open PR との干渉
#162（J0b-2）は `journal/project.rs` の `s` の読み方（`StyleTable`）を変えるが、`resolve_xf` の比較内容は同じ。
このファイルとの衝突はない。#144 は pptx。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 3 / nit 2）
