# PR #122 レビュー（Opus, round 2）

対象: impl/core-set-leaf-text @ 826ffbb48cd2c4a649e76fa064120a185f4076c0

## 結論（先に）
round 1 の M1 は直り、cover page などの store に書けるようになった。m2、m3、n1 と、m1 のマージも挙動で確認できた。
ただし、拒否リストを「allowlist で生成している名前空間」（21 個）に絞った結果、絞りすぎになった。ADR-0015 §6.2 (r4) が拒否すると明記している VML と MS 拡張（`w14` など）、さらに chart（`c:`）や `a14` などが `Ok` で書けるようになり、`<c:autoTitleDeleted val='1'>X</c:autoTitleDeleted>` のようなスキーマ違反を出力する（M1）。
この方向は round 1 の修正案（「生成している名前空間に絞る」）が招いたものなので、修正案を改める。
converged: no（Major 1）。

## 前ラウンドの解消状況
- M1: 解消（ただし絞りすぎ → 本ラウンドの M1）。cover page、bibliography、SharePoint `metadata/contentType`、`customXsn` の store で check と set が `Ok` になり、差分は葉のテキストだけだった。WML への書き込みは、ルートからの経路でも途中の要素でも `TypedNamespace` で拒否され、出力はバイト単位で同一だった。MC（`mc:Fallback`）も拒否される。同期テスト `typed_element_namespaces_match_the_generator` は、生成済みリストから 1 行消した場合も、`wml.toml` から `w16cex:` の 2 項目を消した場合も失敗した。
- m1: 解消。`git show --remerge-diff d17db40` で、衝突は ADR-0015 の追記だけ。D3c（#98）の後に #105 の note が置かれている。
- m2: 解消。`yes` と `""` は `NilElement` になり、出力はバイト単位で同一。`false` と ` 0 ` は書ける（テスト追加済み）。
- m3: 解消。`fuzz_targets/leaf_text.rs` と、`lens_ops` からの呼び出しを確認した。`check_set_leaf_text` に「値に `q` を含むなら `Ok`」という変異を入れると、`cargo +nightly fuzz run leaf_text` が 60 秒以内に `preflight and write disagree: (Ok(()), Err(NoSuchElement))` で落ちた。
- n1: 解消。
- （別レビューの m4: 同じ値を書いたときに綴りが保たれるテスト）未対応。今回のタスク仕様に入っていなかったため、本ラウンドの m2 として残す。

## Findings

### M1（Major）: 生成している名前空間だけを拒否するため、ADR が拒否と定める VML・MS 拡張・chart などに `Ok` で書けてしまう
- 根拠:
  - ADR-0015 L595-597 (r4) は、拒否する名前空間を「the namespaces the generator has schemas for (WML, SML, PML, DML, VML, MC and the MS extensions such as `w14`)」と列挙している。新しい `generated/typed_element_namespaces.rs` は 21 個で、VML と `w14` を含まない。旧リストは 154 個だった。ADR の note（L1143〜）は、この縮小を記録していない。
  - 一時プローブ（コミットしていない）の結果。どれも check と set が `Ok`:
    - chart: `[c:chartSpace, c:chart, c:autoTitleDeleted]` → `<c:autoTitleDeleted val='1'>X</c:autoTitleDeleted>`（CT_Boolean は空要素なので、スキーマ違反になる）
    - VML: `[xml, v:shape, x:ClientData, x:Row]` → コメントのアンカー行が書き換わる
    - `w14:checked`、`a14:hiddenFill` → 空要素型にテキストが入る
    - `ds:datastoreItem/ds:schemaRefs`（customXml の itemProps）→ テキストが入る
  - `Package::xml_part_mut` は公開されているので、`xl/charts/chart1.xml` や `xl/drawings/vmlDrawing1.vml` にこの API で書ける。ADR (r4) が `document.xml` について防ごうとした「型付き要素へのテキスト書き込み」が、名前空間を変えるだけで通る。
  - 原因は round 1 の修正案。cover page の store を通すために「生成している名前空間」へ絞ったが、ADR の定義は「スキーマを持つ名前空間」だった。
- 修正案: 拒否リストを、参照データ（`reference/openxml-sdk-data` と Transitional XSD）のターゲット名前空間すべてから生成する（旧リスト相当）。ADR §6.2 が Supported とする custom XML store の語彙（coverPageProps、bibliography、`metadata/*`、customXsn など）だけを、名前付きの除外リストとして外す。同期テストでは、参照データと「拒否リスト ∪ 除外リスト」が過不足なく一致することを検査する。chart と VML の拒否テストも足す。生成している名前空間だけを拒否するという現在の線を残すなら、ADR §6.2 L595-597 の書き換えが必要で、これは owner の判断になる。

### m1（Minor）: main（#121）を取り込むと `codegen --check` が失敗する
- 根拠: 一時 worktree で `origin/main`（99e9658）をマージすると、テキストの衝突は無い。しかし `cargo xtask codegen --check` が `out of date: crates/ooxml-core/src/generated/typed_element_namespaces.rs` で失敗する。#121 が `xltc:`（threadedcomments）の allowlist を足したためである。このまま main に入れると、main の CI が赤くなる。また、再生成するまでは `xltc` の要素も書けてしまう。open な #124 も allowlist を触るので、後からマージする側も同じく再生成が要る。
- 修正案: M1 の修正と一緒に origin/main をマージし、`cargo xtask codegen` で再生成する。生成元を参照データにすれば（M1 の案）、allowlist を変えてもこの表は変わらなくなる。

### m2（Minor）: 同じ値を書いたときに綴りが保たれることをテストしていない（別レビューの m4 の持ち越し）
- 根拠: `leaf_text.rs` の `if target.text() == value { return Ok(()) }` を、空の値のときだけ早期リターンするように変えても、`tests/leaf_text.rs` は通る（別レビューでの変異）。note（L1159）は「A same-value edit preserves the source spelling」と書いている。
- 修正案: CDATA や `&amp;` を含むテキストに同じ値を書き、出力がバイト単位で同一で、dirty にならないことを確かめるテストを足す。

### m3（Minor）: OPC のメタデータと Strict の名前空間がリストに入っていない
- 根拠: `_rels/*.rels` と `[Content_Types].xml` は `xml_part_mut` で取得できる（`package.rs:39` の `is_opc_metadata` の分岐）。`[Relationships, Relationship]` と `[Types, Default]` への書き込みはどちらも `Ok` になり、`<Relationship …>X</Relationship>` のように空要素にテキストが入る。Strict の `w:`（`http://purl.oclc.org/ooxml/wordprocessingml/main`）も `Ok` になる（別レビューの n2。パッケージは Strict を開かないので、`XmlDocument::parse` を直接呼んだ場合だけ）。どちらも round 1 からあった問題で、退行ではない。
- 修正案: M1 の生成リストに `package/2006/relationships` と `package/2006/content-types`、それに`STRICT_NS_PREFIX` で始まる名前空間の拒否を加える。

### n1（nit）: `leaf_text` の fuzz が stable の smoke と cargo-fuzz の matrix に入っていない
- `crates/ooxml-robustness/tests/smoke.rs` は `check_leaf_text` を呼ばない。`fuzz-smoke.yml` の matrix にも `leaf_text` が無い（`lens_ops` 経由で週 1 回だけ実行される）。smoke に 1 行と、matrix に `leaf_text` を足す。

## 確認して問題がなかった点
- 生成器の `emit_typed_element_namespaces` は、entry の要素の接頭辞（`Entry.prefix`、型の接頭辞ではない）から名前空間を引く。`a:CT_TextBody/p188:` や `a:CT_TextBody/dsp:` のように型と要素で名前空間が異なる entry も、要素側の `p188`、`dsp` がリストに入っている。
- check と set は `validate_leaf_text` を共有している。fuzz の Err 分岐は、パースできない入力のときに fallback の文書とその元のバイト列を比べており、誤検出しない。
- CI（Rust 1.99 の test、clippy、codegen --check、python-pptx、python-docx、openpyxl、LibreOffice）はすべて pass（826ffbb、`gh pr checks 122`）。ただし #121 が入る前の main とのマージ結果で走ったもの（m1）。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 3 / nit 1）
