# PR #42 レビュー（Opus, round 1）

対象: `docs/status-index` @ `08f89e1`（base `origin/main` @ `8c54366`）。README.md、docs/adr/README.md（新規）、docs/process.md（新規）。

## 結論（先に）

構成・リンク・コマンドはほぼ正確。main のスクラッチコピー（Rust 1.99.1）で README のコマンドはすべて成功し、相対リンクもすべて解決する。ADR 0001–0016 の番号・タイトル・PR 番号は各ブランチの ADR ヘッダーと合っている。
ただし、main にない機能を書いている箇所が 1 つ（xlsx の bool 書き込み）、process.md に実態より強い言い切りが 1 つあり、事実の誤りも 1 つある（ADR-0003 の rev）。**判定: converged: no**（Major 2、Minor 5）。どれも数行で直せる。

## 確認したこと

- `gh pr list --state all`（#1–#42）と ADR ヘッダーを照合した。main 上は 0001 Accepted rev 3、0002 Accepted rev 5、0003 Proposed rev 2、0004 Accepted rev 4、0005 Accepted rev 3、0006 Accepted rev 3。0007–0016 は main になく、各 PR ブランチで Proposed。PR 番号 #25/#28/#27/#32/#31/#30/#33/#35/#39/#37 はすべて正しい。表の review リンクは main 上のファイルと一致する。
- main のスクラッチコピー（`/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr42/`）で次を実行した。結果はすべて exit 0。
  - `cargo test --workspace --all-features`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo xtask codegen --check`
  - `cargo xtask stats`
  - `cargo run -p ooxml-{xlsx,docx,pptx} --example manual_check -- <outdir>`（出力は 11/11/10 ファイル + CHECKLIST.md）
  - `cargo bench -p ooxml-bench --bench baseline --bench breakdown --no-run`
- CI の内容（test/clippy/codegen と pptx-readback job）、`o3-recalc.yml`（workflow_dispatch、LibreOffice は版と SHA-256 で固定）、`codegen/allowlist/`、各 crate の README、`.cargo/config.toml` の xtask alias は記述どおり存在する。
- 3 ファイルの相対リンクは壊れていない。
- マージした PR はすべて `cuzic` がマージしている（"The owner merges PRs" は事実）。

## Findings

### M1（Major）README: xlsx が bool を書けると書いているが、main にはない

`README.md` の crate 表、`ooxml-xlsx` の行:

> Reads and writes cell values (number, string, bool) on the first visible sheet.

main の `crates/ooxml-xlsx/src/worksheet.rs` にある setter は `set_number`/`set_string`/`write_number`/`write_string` だけで、bool の書き込み API はない。bool を返すのは読み取り（`CellValue::Bool`）だけである。

```suggestion
| [`ooxml-xlsx`](crates/ooxml-xlsx/README.md) | Phase0 | Opens a workbook and reads cell values (number, string, bool, error) on the first visible sheet; writes numbers and strings. Creates a workbook from an embedded template. Refuses formulas, merged ranges, table headers and similar cases with a typed error. … |
```

あわせて "Opens any workbook" は言い過ぎ（暗号化パッケージと Strict は拒否する）なので、"Opens a workbook" にする。

### M2（Major）process.md: 「すべての実装 PR がレビューされ、レビューファイルがコミットされる」は実態と合わない

`docs/process.md` の「Adversarial review rounds」に次の 2 文がある。

> Every ADR and every implementation PR is reviewed by an independent Opus reviewer …
> **Review files are committed** with the work they review …

マージ済みの実装・core PR のうち、#6（foundation-v0）、#9（`parsed_part`）、#10（wildcard `ChildOf`）には、専用のレビューファイルが `docs/reviews/` にも ADR レビューにもない。ADR についても、0008/0013/0014/0016 の PR ブランチにはまだレビューファイルがない（これはレビュー待ちとして説明できる）。
冒頭で "describes current practice" と宣言している以上、"every" は弱めるべき。案:

```suggestion
- ADRs and implementation PRs are reviewed by an independent Opus reviewer that tries to break
  them … (since #11; the earliest foundation PRs #6, #9 and #10 have no committed review).
```

### m1（Minor）ADR index: ADR-0003 は #20 では rev 3 ではなく rev 4

`docs/adr/README.md` の 0003 の行は "rev 3 and its implementation are in #20" と書いている。`origin/impl/xlsx-phase1` の ADR ヘッダーは `Proposed (revision 4)` で、レビューも round 1–3 まである。#20 の PR タイトル（"ADR-0003 rev 3"）に引きずられたとみられる。"rev 4 (rounds 2–3) and its implementation are in #20" にする。

### m2（Minor）manual_check の example 名: #41 とのマージ順への依存

README の 3 つの `cargo run -p ooxml-* --example manual_check` は **main では正しい**（実行して確認済み）。一方、#41 は example 名を `xlsx_manual_check`/`docx_manual_check`/`pptx_manual_check` に変え、`cargo xtask manual-check <outdir>` を追加する。ところが #41 は README.md も docs/process.md も変更しない。
どちらを先にマージしても git の衝突は起きず、README が黙って古くなる。対処は次のどちらか。

- #42 を先にマージする場合: #41 の側で README の「Building and testing」節（3 コマンドを `cargo xtask manual-check <outdir>` と新しい example 名に置き換える）、xtask の行（`manual-check` を追加）、process.md の "Each format crate's `manual_check` example" を更新する。
- #41 を先にマージする場合: #42 で同じ修正をする。

PR 本文にこの依存を書いておくこと。

### m3（Minor）docs/adr/README.md: 「レビュー中の ADR は PR ブランチにだけある」が 0003 と食い違う

"ADRs that are still in review live only on their PR branch until it merges" とあるが、0003 はレビュー中（Proposed）のまま main にある。初期の plan PR #3–#5 では Proposed の ADR がそのままマージされた。0004/0005 も Proposed でマージされ、その後 Accepted になっている。
1 文足す案: "Early plan PRs (#3–#5) merged ADRs while still Proposed; since then ADRs stay on their branch until Accepted."
process.md の "The ADR is then marked Accepted, or the implementation PR is merged" も、この経緯と矛盾しない書き方にする。

### m4（Minor）README: ooxml-core の「DTD を含むパッケージを拒否する」は不正確

DTD の拒否は `XmlDocument::parse` で起きる（`xml/parse.rs` の `Error::DtdNotAllowed`）。パースしない raw パートに DTD があっても、パッケージは開けてそのまま保存される。暗号化と Strict は `Package::open` で拒否されるので、性質が違う。
"Refuses encrypted and Strict packages, and DTDs or nesting deeper than 256 levels in parsed parts." にする。

### m5（Minor）process.md: 細部の言い過ぎ

- **ブランチ接頭辞の列挙:** 実際には `chore/`、`ci/`、`test/`、`spike/` も使われている（#1、#8、#21、#23、#36、#41）。「例:」とするか、列挙に加える。
- **"Claims … are marked *(u)*":** main の ADR でこの記法を使っているのは 0004 だけで、ブランチでも 0007 に限られる。"Word ADRs mark …" とするか、"should be marked" と規範として書く。
- **"One PR per milestone. A design (ADR plus spike)":** #22 の設計と #23 の spike（マージしない prototype）は別 PR になっている。「spike は同じ PR か、マージしない別 PR」とする。
- **ADR-0001 のレビューファイル名:** `NNNN-opus-review-roundK.md` の命名規則に対して、0001 だけ `opus-review-roundK.md`（番号なし）。docs/adr/README.md の冒頭か process.md に一言あるとよい。

## 判定

**converged: no**

M1（main にない bool 書き込みを書いている）と M2（"every" で言い過ぎている）を直せば収束する見込み。m1–m5 は文言の修正で済む。m2 は #41 とのマージ順に依存するので、どちらの PR で直すかを PR 本文に明記すること。
