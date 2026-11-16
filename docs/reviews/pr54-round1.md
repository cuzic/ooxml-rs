# PR #54 レビュー（Opus, round 1）

対象: `fix/docx-phase0-followups` @ `0a6acda`（base `ab1b0b6` = `origin/main`）。PR #34 review round 1 の m15、m16、n1–n3 と、PR #43 review round 1 の m1、n1、n2 への対応。
リポジトリは変えていない。確認はすべて scratchpad の作業コピー（`review-52-54/`）で行った。

## 結論（先に）

**収束（Blocker/Major なし。Minor 1 件、nit 2 件）。** 指摘した項目は、どれも指摘した形では閉じている。

- 5 つのプローブ形（m15 の表）は、どれも `<CODE> (self reference)` で refused になる。M16 と N13 は、それぞれ専用のテストで落ちる。
- ゲートの出力は、821 ファイル（fixtures、POI、SDK、docx4j、templates）で `origin/main` と完全に一致した。m15 で refused が増えたファイルは無い。
- 3 つの example は動き、表示どおりの編集をする。pptx の run の置き換えは、保存したファイルで確認できた。
- 残るのは 1 件。item の前や中に `\` が 1 つだけある形（`"\OLE_LINK1"`、`x\OLE_LINK1`）は、まだ safe になる（m1）。Word が書かない形で、Word がどう読むかも未確認 *(u)* なので Minor とする。

## 確認できたこと（指摘ではない）

| 観点 | 結果 | 根拠 |
|---|---|---|
| ビルドとテスト | ok | `cargo test -p ooxml-docx --release` 全件 pass（adversarial 27 本）。`cargo test --doc`（xlsx、pptx、docx）pass。`cargo clippy --workspace --all-targets --all-features -- -D warnings` clean |
| m15 の 5 形 | 解消 | 下の表の 1–5 行目。どれも key が `(item …OLE_LINK1…)` になり、`OLE_LINK1` ブックマークがある文書で refused |
| m15 の追加プローブ | おおむね解消 | 下の表。引用符の種類（`“…”`、`＂…＂`）、全角空白、`\* MERGEFORMAT` の後、`\c` の引数の前、小文字の item、ファイル名と item が同じ名前（`"OLE_LINK1" OLE_LINK1`）、ProgID が item と同じ名前は、どれも refused。閉じていない引用符と、引用符の直後に続く文字は `(non-literal source)`。例外は m1 の 4 形 |
| m16 / M16 | 解消 | `fields::tests::codes` に `key(…, true)` の 2 本が入った。M16（`nested ||` を外す）→ `fields::tests::codes` が落ちる |
| m16 / N13 | 解消 | 小文字のブックマーク `ole_link1` と `"OLE_LINK1"` の組。N13（ブックマーク名を正規化しない）→ `m13_self_referencing_link_is_refused` が落ちる |
| m15 の副作用 | なし | `write_gates()` を 821 ファイルで `origin/main` と比べ、差分 0（`scan_main.tsv` / `scan_54.tsv`） |
| n1（`is_safe` の doc） | 解消 | `<CODE> (item X)` はここでは safe で、文書を開いたときに照合する、と書かれた |
| n2（README の refusal 表） | 解消 | self reference と、linked のカスタムが組み込みを shadow することが入った |
| n3（ADR の 643） | 解消 | 2 か所とも「643 Word files (560 `.docx`, 80 `.dotx`, 3 `.docm`)」 |
| #43 m1 | 解消 | `replace_text` は run を持つ段落（index ≥ 1）を探す。`sample.pptx` では slide 1 / shape 3074 の段落 1。保存した `slide2.xml` の `a:t` は `First paragraph, replaced`、`first run replaced` になり、変わった part は `slide1.xml` と `slide2.xml` だけ。README の説明もこれに合う |
| #43 n1 | 解消 | README の 2 例と `presentation.rs` の rustdoc は `.first()` で受け、placeholder の無いスライドで panic しない。rustdoc は doctest で pass。README の 2 例は、`lib.rs` に一時的に `#[doc = include_str!("../README.md")]` を足して compile できることを確かめた（docx の README も同様） |
| #43 n2 | 解消 | `edit_cells` は `Core(StrictNotSupported(_))`、`replace_text` は `LineBreakInParagraph` を型で受ける。どちらも実行して期待どおりの行が出る |

### m15 のプローブ（本文に `OLE_LINK1` ブックマーク）

| # | フィールド | key | 結果 |
|---|---|---|---|
| 1 | `LINK Word.Document.12 "self.docx" \a "OLE_LINK1" \r` | `LINK (item OLE_LINK1)` | refused |
| 2 | `LINK Word.Document.12 "self.docx" \a OLE_LINK1` | `LINK (item OLE_LINK1)` | refused |
| 3 | `INCLUDETEXT "self.docx" \! OLE_LINK1` | `INCLUDETEXT (item OLE_LINK1)` | refused |
| 4 | `INCLUDETEXT C:\\x\\self.docx OLE_LINK1` | `(item X␟SELF.DOCX␟OLE_LINK1)` | refused |
| 5 | `LINK Word.Document.12 C:\\x\\self.docx OLE_LINK1` | 同上 | refused |
| 6 | `INCLUDETEXT "OLE_LINK1" OLE_LINK1` | `(item OLE_LINK1)` | refused |
| 7 | `LINK OLE_LINK1 "a.docx" OLE_LINK1` | `(item OLE_LINK1)` | refused |
| 8 | `INCLUDETEXT "self.docx" OLE_LINK1 \c MSWord8` | `(item OLE_LINK1␟MSWORD8)` | refused |
| 9 | `INCLUDETEXT "self.docx" \* MERGEFORMAT OLE_LINK1` | `(item MERGEFORMAT␟OLE_LINK1)` | refused |
| 10 | `INCLUDETEXT “self.docx” “OLE_LINK1”`、`＂OLE_LINK1＂`、全角空白区切り | `(item OLE_LINK1)` | refused |
| 11 | `INCLUDETEXT "self.docx" "\OLE_LINK1"` | `(item \OLE_LINK1)` | **safe**（m1） |
| 12 | `INCLUDETEXT "self.docx"\OLE_LINK1` | `(item LE_LINK1)` | **safe**（m1） |
| 13 | `INCLUDETEXT "self.docx" x\OLE_LINK1` | `(item X␟LE_LINK1)` | **safe**（m1） |
| 14 | `INCLUDETEXT self.docx\OLE_LINK1` | `(item LE_LINK1)` | **safe**（m1） |

（␟ は `ITEM_SEPARATOR`、U+001F。）

### mutant

| mutant | 落ちるテスト |
|---|---|
| M16 `nested` を FILE_SOURCE の key で無視 | 1（`fields::tests::codes`） |
| N13 ブックマーク名を正規化しない | 1（`m13_…`） |
| P1 候補の先頭だけを照合 | 1 |
| P4 `\` で token を切らない | 1 |
| P5 最初のスイッチ以降の token を捨てる | 2 |
| P6 引用符の中身を token にしない | 2 |
| P7 ProgID とファイルを候補から外さない | 1（`codes`。false positive の側） |
| **P2** 候補の末尾だけを照合 | **なし**（n1） |
| **P3** ProgID・ファイルと同じ名前の token をすべて外す（最初の 1 つだけでなく） | **なし**（n1） |

## 指摘

### m1 [Minor, (u)] `\` が 1 つだけ付いた item は、まだ見落とす

**場所**: `crates/ooxml-docx/src/fields.rs` L308–L340（`item_tokens`）、L112–L116（`ITEM_ARGUMENT` の doc）

- `item_tokens` は、引用符の外では `\` と次の 1 文字を捨て、引用符の中では `\` をそのまま残す。そのため上の表の 11–14 行目は、key が `\OLE_LINK1` や `LE_LINK1` になり、`OLE_LINK1` ブックマークと一致しない。
- Word の引用符つき引数では、`\` は次の文字をエスケープする（パスの `\\` がその例）。`"\OLE_LINK1"` を Word が `OLE_LINK1` と読むなら、self reference を見落とす *(u)*。引用符の外の `x\OLE_LINK1` も同じ疑いがある。
- どれも Word が書かない形なので、m15 と同じく Minor とする。ただ doc の「whichever token Word reads as the item」は、この 4 形については言い過ぎである。

**提案**（fail closed の側に倒すだけで、正しい読み方を決める必要はない）

```rust
// item_tokens: token ごとに、`\` を外した形も候補に足す。
//  - 引用符の中: "\OLE_LINK1" → `\OLE_LINK1` と `OLE_LINK1`
//  - 引用符の外: `x\OLE_LINK1` → 今の `X`, `LE_LINK1` に加えて、`\` の次の文字から始まる `OLE_LINK1`
//    （`\` で切ったあと、次の文字を捨てずに新しい token の先頭にした版も出す）
```

ゲートの差分は、今のコーパスでは 0 のままのはずである（m15 と同じで、Word が書く形の key は変わらない）。

### nit

- n1: mutant P2 と P3 が生き残る。テストの item は、どれも最後の候補で、ファイル名とも違う名前だからである。`tests/adversarial.rs` の `m13_…` の表に次の 2 行を足せば、どちらも落ちる（どちらも今のコードでは refused）。
  - `(r#"INCLUDETEXT "self.docx" OLE_LINK1 \c MSWord8"#, "INCLUDETEXT (self reference)")`（P2）
  - `(r#"INCLUDETEXT "OLE_LINK1" OLE_LINK1"#, "INCLUDETEXT (self reference)")`（P3: ファイル名と同じ名前の item を、`skip` が 1 つしか外さないこと）
- n2: PR #43 review の n3（README を doctest に入れる）は、この PR では扱っていない。review で「別 PR でよい」とした項目なので、それで構わない。上のとおり、今の README の例は compile できる。

## 判定

- **converged: yes**
  - #34 の m15、m16 と #43 の m1 は、指摘した形ではすべて閉じた。nit もすべて対応済み。
  - m1 は、Word が書かない形での fail open *(u)* が少し残っている、というもの。n1 はテストの補強。どちらも次の修正でまとめて直せばよく、マージを止める理由にはならない。

### 再現手順

- 一式: `<scratchpad>/review-52-54/`
  - `p54/`（PR のコピー。`tests/adversarial.rs` の末尾に `zz_review54_probes` を、`examples/zz_gatescan.rs` を足した）
  - `main0/`（`origin/main` のコピー。`examples/zz_gatescan.rs` を足した）
  - `m54/run.py`、`m54/out.txt`、`m54/mutrepo/`（mutant）
  - `scan_main.tsv`、`scan_54.tsv`（ゲートの出力。`diff` は空）
  - `out54/`（3 つの example の出力と保存ファイル）
- プローブ: `cargo test -p ooxml-docx --release --test adversarial zz_review54 -- --nocapture`
- mutant: `R=<scratchpad>/review-52-54/m54 python3 m54/run.py [M16 N13 P1 …]`
- ゲートの差分: 両方のコピーで `cargo build --release -p ooxml-docx --example zz_gatescan` を実行し、821 ファイルの出力を `diff` で比べる
