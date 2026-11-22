# PR #148 レビュー（Opus, round 1）

対象: fix/docx-seq-limit @ 99d8f0d9a8560c105a14dfcaf6afcc57fb19af73

## 結論（先に）
converged: yes。SEQ 識別子の上限は Unicode スカラー値の数で数えており、`図`×40 は受理、41 は `InvalidValue` で拒否し、保存結果はバイト一致する。上限を外す変異と、バイト数で数える変異の両方で新しいテストが落ちる。ADR の記述は実装と Microsoft の文書に合っている。Blocker と Major はない。

## 前ラウンドの解消状況
初回ラウンド。PR #142 round 1 の指摘（Issue #146）の解消状況:
- m1（SEQ 識別子の上限 40）: 解消。`insert_field.rs:304` に `identifier.chars().count() > 40` を追加。`bookmarks.rs:62` の上限と同じ数え方。
- n1（number symbol）: 解消。ADR に `②` `½` `²` `Ⅻ` を明記し、`54-field-seq-number-symbols.docx` を手動確認と `lo-check/expected.json` に追加。LibreOffice CI は pass。
- n3（日付型 DOCPROPERTY）: 解消。§7 の Open question を owner の質問 11 に移し、Freshness に opt-in 不要の例外を明記。`53-…` は ADR、`manual_check.rs`、`expected.json` から引き続き参照されており、消えた見出しへのリンクは残っていない。

## プローブ（コミットしない）
- `A`×40 は Ok、`A`×41 は Err。`Fig`+`②`×37（40 文字、114 バイト）は Ok、`Fig`+`②`×38 は Err。`図`×39+`_` は Ok。
- 拒否はすべて `to_bytes()` が呼び出し前と一致。opt-in なしでは 40 文字の識別子も拒否され、入力とバイト一致。
- 変異 1: 上限の条件を `false` にすると `sequence_identifier_is_limited_to_40_unicode_characters` が `assert_refusal` のバイト一致比較（phase3e.rs:45）で落ちる。
- 変異 2: `identifier.len() > 40`（バイト数）にすると、同じテストが `図`×40 の受理（phase3e.rs:344）で落ちる。
- 読み込みと更新: 既存の SEQ の識別子は `count_sequence`（fields.rs:527）で数えるだけで検証しない。既存のフィールド命令を編集する API はない（ADR-0015 §7 の「Not in Phase3」）。Word が書いた文書を長さで拒否する理由はないので、挿入時だけ上限をかける今の形が正しい。
- Microsoft の SEQ の文書（ADR のリンク先、HTTP 200）に「The name must start with a letter and is limited to 40 characters (letters, numbers, and underscores)」とあり、ADR の記述と合う。

## Findings
### m1（Minor）: BMP 外の文字は 1 文字と数えるが、Word は UTF-16 の 2 単位と数えるかもしれない
- 根拠: `"\u{2000B}".repeat(40)`（CJK 拡張 B、alphabetic）は Ok。Rust の `chars()` はスカラー値を数える。Word の「40 characters」が UTF-16 単位なら 80 単位になる。`add_bookmark` も同じ数え方なので、この PR の後退ではない。
- 修正案: ADR に「Unicode スカラー値で数える。BMP 外の文字の扱いは *(u)*」と書くか、ブックマークと合わせて follow-up で手動確認する。

### m2（Minor）: `tools/lo-check/expected.json` が open PR #144 と衝突する
- 根拠: `git merge-tree --write-tree HEAD pr144` で `expected.json` だけが content conflict。どちらも `pptx/01-…` の直前に項目を足している。GitHub 上は main に対して MERGEABLE。
- 修正案: 後からマージする方で両方の項目を残して解消する。

### n1（nit）: §7 の cached result の箇条が date 型 DOCPROPERTY の空キャッシュを書いていない
- 根拠: 「`DocProperty` takes the property's current value」のままで、空キャッシュは Freshness の例外にしか書かれていない（変更前からの記述）。
- 修正案: 「date-valued properties: empty cache, see Freshness」を添える。

## CI
`gh pr checks 148`（99d8f0d9）: 5 件すべて pass（`test, clippy, codegen --check (Rust 1.99)`、libreoffice、docx/pptx/xlsx read-back）。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 1）
