# PR #34 レビュー（Opus, round 1）

対象: `fix/docx-phase0-minors` @ `dfb02a0`（base `1cf8132`）。ADR-0004 round 4 の Minor m12〜m14 の修正。
リポジトリは変えていない。プローブ、mutant、コーパス走査はすべて scratchpad の作業コピー（`review-34-43/`）で実行した。

## 結論（先に）

**収束（Blocker/Major なし、Minor 2 件と nit 3 件）。** m12、m13、m14 はどれも、round 4 で指摘した形では閉じている。

- m12a/b/c（linked カスタムの shadowing、空白の正規化、曲引用符）、m13（Word のリンク貼り付けが書く self-LINK、`LINK` のファイル引数の位置）、m14（M22/M32/M33/M34/M37）は、どれも専用のテストで落ちる。
- コーパスのゲート差分は 0。対象は 1,628 ファイル（開けるもの 1,340）で、round 4 の 737 ファイルを含む。refused になる文書の集合は変わっていないので、説明が要る変化は無い。
- 残る問題は 2 件。
  - item 引数の照合が「最初のスイッチより前の、バックスラッシュを含まない引数」に限られる（m15）。Word が普通に書く形は捕まる。
  - PR の `LINK` 引数位置の変更で、N3 の `nested` 規則（M16）を落とすテストが無くなった（m16）。

## 確認できたこと（指摘ではない）

| 観点 | 結果 | 根拠 |
|---|---|---|
| ビルドとテスト | ok | `cargo test -p ooxml-docx --release` 全件 pass（adversarial 27 本）。`origin/main`（`848b057`）に当てても pass。`cargo clippy --workspace --all-targets --all-features -D warnings` も clean。#43 とも main とも conflict なし（`git merge-tree`） |
| m12a | 解消 | `refused()` の `classify` で、linked の照合が `is_safe` より前にある。S01 の 7 つの名前は refused、unlinked なら safe。プローブ: linked の `company`（小文字）+ `DOCPROPERTY Company` → refused。`TITLE`/`INFO Company` は組み込みしか読まないので、影響は無い（正しい） |
| m12b | 解消 | `normalize_name` を引数、custom.xml の名前、ブックマーク名の 3 つに同じように使う。`"Client "`、`" Client"`、NBSP、全角空白、内部の連続空白も対象。接頭辞の規則で `DOCPROPERTY Client Name`（引用符なし）+ linked `Client Name` も refused。ZWSP（White_Space ではない）は、接頭辞の規則が fail closed の側で拾う |
| m12c | 解消 | `closes()` で、`"`/`＂` は同じ文字でしか閉じない。`arguments()` は、引数の中の引用符と、閉じた引用符の直後に続く文字を ambiguous として `None` にする。プローブ: `“Client””`、`"Client""Name"`、`Title "x` はすべて refused。`'Client'` は名前として一致しないので refused |
| m13 | 解消 | `LINK` のファイルは第 2 引数、item は第 3 引数。`INCLUDETEXT` の item は第 2 引数。ブックマークは本文とすべての story から集める（`bookmark_names` は再帰）。ブックマークが小文字でも一致する。round 4 で参考にした S17b（instruction の中の `w:fldSimple`）も、item の照合で refused になった |
| m14 | 解消 | 再実行した mutant: M22、M32、M33、M34、M37 は、それぞれ新しいテストで落ちる（下表） |
| M5/M20 の扱い | 妥当 | 「defence in depth」として ADR R1 の注記と revision 5 に書かれた。round 4 の提案どおり |
| コーパス | 変化 0 | `gatescan`（round 4 と同じもの）を base `1cf8132` と PR の両方でビルドして比べた。対象は `corpus/files`、`docx/{d4j,sdkfiles}`、`docx-p1/corpus/{d4j,poi,sdk,tpl}`。open の結果、`write_gates()`、全 run の `check_settable()` と `in_field()` のハッシュが、1,628 行すべてで一致した。StaleFieldRisk は 68 ファイルで、どれも TOC/PAGEREF/SEQ などによるもの。DOCPROPERTY、LINK、INCLUDETEXT による拒否は 0 件 |

### mutant（round 4 の 34 本を新しいコードに合わせたものと、PR の新しい規則に対する 16 本）

| mutant | 落ちるテスト |
|---|---|
| M1〜M4、M6、M9、M11、M12、M14、M15、M17〜M19、M21〜M37 | すべて 1 本以上。M22 は `m14_instr_text_in_another_container_is_malformed`、M32/M33 は `m14_first_settings_part_on_second_off`、M34 は `m14_second_custom_properties_part_counts`、M37 は `m14_docproperty_with_a_nested_field_in_its_argument` |
| M5、M20 | なし（ADR で defence in depth と明記。妥当） |
| M13 | なし（等価。ADR に記載済み） |
| **M16** `nested` を FILE_SOURCE の key で無視 | **なし**（round 4 では `fields::tests::codes` と `n3_…` で落ちていた。m16） |
| N1 linked の照合を allow-list の後にする / N2 接頭辞の規則を外す / N3 正規化を外す / N4 linked の名前だけ正規化しない / N5 引数を trim だけにする | 1 / 1 / 2 / 1 / 1 本 |
| N6 どの引用符でも閉じる / N7 bare token 内の引用符を許す / N8 閉じ引用符の直後に続く文字を許す | 2 / 2 / 2 本 |
| N9 LINK のファイルを第 1 引数にする / N10 LINK の item を見ない / N11 INCLUDETEXT の item を見ない / N12 story のブックマークを見ない / N14 self reference を拒否しない / N15 ブックマークを再帰で集めない / N16 本文のブックマークを見ない | 2 / 2 / 2 / 1 / 1 / 1 / 1 本 |
| **N13** ブックマーク名を正規化しない（大文字小文字を区別する） | **なし**（m16） |

## 指摘

### m15 [Minor, 判断] item 引数を、スイッチの後とバックスラッシュの後で見落とす

**場所**: `crates/ooxml-docx/src/fields.rs` L182–L215（`arguments`）、L263–L274（`key` の FILE_SOURCE 分岐）

`arguments()` は、最初の `\` で位置引数の列を打ち切る。bare token も `\` で切る。そのため、本文に `OLE_LINK1` ブックマークがあっても、次の形は gate が Ok になる（プローブ）。

| フィールド | key | 結果 |
|---|---|---|
| `LINK Word.Document.12 "self.docx" \a "OLE_LINK1" \r` | `LINK` | safe |
| `LINK Word.Document.12 "self.docx" \a OLE_LINK1` | `LINK` | safe |
| `INCLUDETEXT "self.docx" \! OLE_LINK1` | `INCLUDETEXT` | safe |
| `INCLUDETEXT C:\\x\\self.docx OLE_LINK1`（引用符なしのパス） | `INCLUDETEXT` | safe（ファイルは `C:`、item なし） |
| `LINK Word.Document.12 C:\\x\\self.docx OLE_LINK1` | `LINK` | safe |
| `INCLUDETEXT self.docx OLE_LINK1` | `INCLUDETEXT (self reference)` | refused（比較用） |

- Word がスイッチの後の位置引数を item として読むか、引用符なしの `\\` をパスの一部として読むかは *(u)*。Word 自身が書く形（引用符付きのパス、item はスイッチの前）は捕まるので、Minor にとどめる。
- ただ、PR の方針は「どちらにも読める形は fail closed にする」である（m12c、DOCPROPERTY の `\* MERGEFORMAT Words` は引数なしとして refused）。この形だけが fail open に倒れていて、方針と合わない。

**提案**（どちらか）

```rust
// key() の FILE_SOURCE 分岐で、item の候補を「位置引数の第 n 個」に限らない:
// instruction 中の引用符付き文字列と bare token をすべて正規化し、どれかが
// ブックマーク名に一致すれば self reference とする（スイッチ名とその引数も含む。
// `MERGEFORMAT` という名のブックマークでも誤検出するだけで、fail closed）。
```

- または、`arguments()` で、スイッチの後に位置引数が続く場合と、bare token の中に `\\` がある場合を ambiguous（`None`）にする。こちらは FILE_SOURCE なら `(non-literal source)` で refused になる。DOCPROPERTY の扱いと揃う。
- どちらの場合も、上の表の 5 形を `m13_self_referencing_link_is_refused` に足す。
- 直さないなら、ADR §6 の FILE_SOURCE の行に「item はスイッチの前の位置引数だけを見る *(u)*」と書く。

### m16 [Minor] テストの検出力: M16（N3 の `nested` 規則）と N13 が生き残る

**場所**: `crates/ooxml-docx/src/fields.rs` L594–L595（`fields::tests::codes`）、`tests/adversarial.rs` の `m13_…`

- **M16**: round 4 では `key("LINK Excel.Sheet.12 \"\" \"R1C1\"", true)` が M16 を落としていた。当時は LINK の第 1 引数（ProgID）がリテラルだったので、`nested` だけが refused の理由だった。
  - この PR で LINK のファイルが第 2 引数（`""`）になったため、`nested` が無くても refused になる。INCLUDETEXT の 2 本も `""` なので同じ。
  - その結果、`nested` 規則を外しても落ちるテストが無い。
  - 規則はまだ必要である（プローブで M16 を当てて確認した）。次の 3 形は、規則があれば refused、無ければ safe になる。
    - `INCLUDETEXT "{ FILENAME \p }x" bm`
    - `INCLUDETEXT "C:\\d\\{ DOCPROPERTY Title }.docx" bm`
    - `LINK Word.Document.12 "C:\\x\\{ FILENAME }" "OLE_LINK9"`
  - `INCLUDETEXT "a.docx" { QUOTE OLE_LINK1 }` も、`nested` だけが refused の理由になる。
- **N13**: `bookmark_names` の `normalize_name` を外しても落ちない。テストのブックマーク名がすべて大文字（`OLE_LINK1`）だからである。小文字のブックマーク `ole_link1` と `"OLE_LINK1"` の組は、PR のコードでは refused になる（プローブ）。
- ADR revision 5 の m14 の項にある「each killed by its own mutant」は、M16 と N13 については成り立たない。

**提案**

```rust
// fields::tests::codes に
assert_eq!(key("INCLUDETEXT \"a.docx\" bm", true), "INCLUDETEXT (non-literal source)");
assert_eq!(key("LINK Word.Document.12 \"a.docx\" \"R1C1\"", true), "LINK (non-literal source)");
```

- `m13_self_referencing_link_is_refused` に、`w:name="ole_link1"` のブックマークと `"OLE_LINK1"` の組を 1 行足す。

### nit

- n1: `fields.rs` L280–L287 の `is_safe` の doc に、`<CODE> (item X)` は「ここでは safe、文書を開いたときにブックマークと照合する」ことを書くとよい（`DOCPROPERTY` のカスタムの説明と同じ形）。`fields` は `pub` で、`is_safe(field_key(..))` を直接呼ぶ利用者には self reference が見えない。
- n2: `crates/ooxml-docx/README.md` L38 の refusal 表は、`LINK (self reference)` と、linked のカスタムが組み込みを shadow することに触れていない。1 句足すとよい。
- n3: ADR L115–L116「737 scanned files (643 `.docx`)」の 643 は、`.docx` 560、`.dotx` 80、`.docm` 3 の合計である。「643 Word files」とするのが正確。

## 判定

- **converged: yes**
  - m12〜m14 は、本文、実装、テストのすべてで閉じている。コーパスのゲートも不変だった。
  - m15 は、Word が書かない形での fail open で *(u)*。m16 はテストの補強。どちらも Minor で、次の修正でまとめて直せばよい。

### 再現手順

- 一式: `<scratchpad>/review-34-43/`
  - `fix-docx-phase0-minors/`（PR のコピー。`tests/adversarial.rs` の末尾に `zz_probes` を足した）
  - `base34/`（`1cf8132`）
  - `mut/run.py`、`mut/out.txt`、`mutrepo/`
  - `scan_base34.tsv`、`scan_fix-docx-phase0-minors.tsv`
  - `m34/`（`origin/main` に PR を当てたもの）
- プローブ: `cargo test -p ooxml-docx --release --test adversarial zz_probes -- --nocapture`
- mutant: `R=<scratchpad>/review-34-43 python3 mut/run.py [M16 N13 …]`
- ゲートの差分: 両方のコピーで `cargo build --release -p ooxml-docx --example gatescan` を実行し、`gatescan <dirs> > scan_*.tsv` の出力を `diff` で比べる
