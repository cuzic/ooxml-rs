# PR #122 レビュー（Opus, round 3）

対象: impl/core-set-leaf-text @ f9faf748f934bdd78d6c3728ca6769252d45b9ce

## 結論（先に）
round 2 の M1 は直った。拒否リストは vendored SDK/XSD の全ターゲット名前空間（150 個）から、ADR-0015 §6.2 の表に載せた 6 個の custom XML store の語彙を除いて生成される。VML・`w14`・`c:`・`a14`・`ds:`・Strict・OPC のメタデータは拒否され、出力はバイト単位で同一になる。同期テストは両方向の変異で失敗する。
m1・m2・m3・n1 も挙動で確認した。新しい Blocker と Major は無い。
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）。

## 前ラウンドの解消状況
- M1: 解消。`emit_typed_element_namespaces` は SDK の `target_namespace` と XSD の `tns` を `BTreeSet` に集め、`CUSTOM_XML_STORE_NAMESPACES` を除いて出力する。allowlist に依存しなくなった。テスト `refuses_vendored_schemas_and_strict_without_mutation` は VML、`w14`、`c:`、`a14`、`ds:`（customXml）、Strict の `w:` を対象にする。check と set の両方が `TypedNamespace` を返し、dirty にならず、出力はバイト単位で同一。cover page、bibliography、`metadata/contentType`、`customXsn`、`longProperties` への書き込みは `Ok` のまま。同期テスト `leaf_text_namespaces_partition_the_reference_schemas` は次の 3 つの変異でどれも失敗した（どれもコミットしていない）:
  - 生成リストから `office:excel` を消す（どちらのリストにも無い名前空間ができる）→ L259 の union の比較で失敗。
  - 例外リストに `urn:bogus` を足す → L259 で失敗。
  - VML を例外リストに移し、`cargo xtask codegen` で再生成する → L269 の ADR 表との比較で失敗。
- m1: 解消。`origin/main`（7cf5eca。#121 と #123 を含む）とのマージは衝突せず、`cargo xtask codegen --check` は「10 generated files up to date」。#124 はまだ open（未マージ）だが、その head も重ねてマージすると衝突は `crates/ooxml-pptx/examples/manual_check.rs` だけで、これは無関係のファイルである。`codegen --check` は通る。リストが allowlist に依存しなくなったので、#124 の `dml.toml` の変更はこの表に影響しない。
- m2: 解消。テスト `writing_the_same_value_preserves_source_spelling` は CDATA、CRLF、`&amp;lt;`、`&#x41;` を対象にする。早期リターンを `value.is_empty() && …` に変える変異を入れると、このテストが失敗する。
- m3: 解消。`.rels` と `[Content_Types].xml` を `xml_part_mut` で取って書くと、`TypedNamespace` が返り、パッケージ全体のバイト列は入力と同一になる（`package.rs`）。OPC の分岐を消す変異では `package.rs:77` が失敗し、Strict の分岐を消す変異では `leaf_text.rs:230` が失敗する。ADR の note に記載されている。
- n1: 解消。`smoke.rs:66` と `fuzz-smoke.yml` の matrix に `leaf_text` が入っている。

## Findings

### n1（nit）: `metaAttributes` の例外に書き込みテストが無い
- 根拠: `edits_custom_stores_without_generated_element_types` は、6 つの例外のうち 5 つしか通していない（`metadata/properties/metaAttributes` が無い）。挙動は例外リストから決まるので正しい。
- 修正案: 配列に 1 行足す（follow-up でよい）。

### n2（nit）: `leaf_text` の smoke 呼び出しが `lens_ops` のテスト関数の中にある
- 根拠: `crates/ooxml-robustness/tests/smoke.rs:66`。失敗すると `lens_ops` の失敗として表示される。`guarded` の第 1 引数で区別はできる。
- 修正案: 必要なら独立した `#[test] fn leaf_text()` に分ける。

## 確認して問題がなかった点
- 決定性: 生成器は `BTreeSet` で集めるので、入力の列挙順に依存しない。`cargo xtask codegen` を 2 回実行しても差分は無かった。同期テストは昇順も検査する。`binary_search` の前提はこれで守られる。
- 過剰拒否: §6.2 で Supported の store は、custom XML part（6 個の vendored 語彙と、vendored でない任意の名前空間）と `docProps/core.xml`（`cp`、`dc`、`dcterms`）である。どちらの名前空間も生成リストに無い。SharePoint の `metadata/properties` は参照データに無いので、書ける。リストに残る `customDocumentInformationPanel` や `mipLabelMetadata` などは §6.2 の表に無く、拒否は ADR L595-597 と矛盾しない。なお、spike の数え上げ（tables.md）に、「other 37」の store が使う名前空間の内訳は無い。そのため、この点は SDK のファイルでは確かめていない。
- 他の open PR: #124 は allowlist と `dml.rs` だけを変える。#125 は `lens.rs` だけを変える。どちらも生成リストと `leaf_text.rs` に触れない。
- 対象のテスト: `ooxml-core` の `leaf_text` 12 件と `package` 35 件、`xtask` 9 件が pass。CI（Rust 1.99 の test、clippy、codegen --check、python-pptx、python-docx、openpyxl、LibreOffice）はすべて pass（`gh pr checks 122`）。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
