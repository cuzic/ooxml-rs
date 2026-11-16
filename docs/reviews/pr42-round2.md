# PR #42 レビュー（Opus, round 2）

対象: `docs/status-index` @ `76e52df`（round-1 対応コミット）。基準は `origin/main` @ `8c54366`、`gh pr list --state all`（#1–#42）、各 crate のソース。

## 結論（先に）

round 1 の M1・M2 は解消した。m1・m3・m4・m5 も解消した。m2（manual_check の example 名）は、方針どおり #41 とのマージ順で扱う。
新しく入った文言に事実誤りが 1 つある（M2 対応の注記「review-until-converged の規則ができる前に」）。もう 1 つ、「second round of PRs」という表現が曖昧である。どちらも Minor なので、**判定: converged: yes**。下の n1・n2 は、この PR で直しても follow-up に回してもよい。

## 解消状況

| ID | 状態 | 確認内容 |
|---|---|---|
| M1 | 解消 | `ooxml-xlsx` の行は "Reads cell values (number, string, bool, …) and writes numbers and strings" になった。main の `worksheet.rs` の書き込み API は `write_string`/`write_number`/`set_number`/`set_string` だけで、bool は読み取り（`CellValue::Bool`）だけ。記述と一致する。"Opens Transitional, unencrypted workbooks" も正確。 |
| M2 | 解消（文言に n1・n2 が残る） | "every" は対象を絞った書き方に変わり、#6/#9/#10 に committed review がないことと、Phase0 は事後レビューだったことを注記で明示した。`docs/reviews/xlsx-phase0-impl-round{1,2,3}.md` は main にある。round 1 は 05:17 のコミットで、#11 のマージ（04:51）より後。注記の内容と合う。 |
| m1 | 解消 | 0003 の行は "rev 4 … in #20" になった。 |
| m2 | 対象外（マージ順で扱う） | README の 3 つの `--example manual_check` は main では正しい。#41 が後でマージされた場合は、#41 の側で README と process.md を更新する必要がある。PR 本文にはまだこの依存が書かれていないので、書いておくとよい。 |
| m3 | 解消 | "the first plan PRs (#3–#5) merged ADR-0003/0004/0005 while still Proposed" を追加した。#3–#5 のタイトルはどれも "(Proposed)" で、main 上の 0003 は今も Proposed (revision 2)。事実と合う。#22（ADR-0006）はタイトルが "(Proposed)" だが、マージ（07:42）より前の 07:21 に rev 3 Accepted のコミットが入っているので、例外に含めないのは正しい。 |
| m4 | 解消 | "Refuses encrypted and Strict packages at open; refuses parsing a part that carries a DTD or nests deeper than 256 levels (unparsed parts are copied through unchanged)" になった。`xml/parse.rs` の `Error::DtdNotAllowed`（L158）と `Error::TooDeep`（L214）はどちらもパース時に返る。記述と一致する。 |
| m5 | 解消 | ブランチ接頭辞に `chore/ci/test/spike` を追加した。*(u)* は "some ADRs use" に弱めた。spike は "occasionally … a separate draft PR" とし、#23 が実際に draft であることを確認した。ADR-0001 のレビューファイル名の例外も追記した。 |

## Findings

### n1（Minor）process.md 末尾の注記: 「規則ができる前に」は ADR については事実と違う

`docs/process.md` 末尾:

> the first foundation PRs (#6, #9, #10) were merged on CI before the review-until-converged rule was adopted

ADR のレビューラウンドは、#6（04:26 マージ）より前から行われている。ADR-0001 の round 1–3 は初回コミット（`a3ac455`）に含まれ、ADR-0002 の round 1 対応は 02:55 である（#2 は 03:40 マージ）。#6 より前になかったのは、「実装 PR にもこの規則を適用する」ことだけである。案:

```suggestion
Note: the first foundation PRs (#6, #9, #10) were merged on CI before review rounds were extended from ADRs to implementation PRs; Phase0 implementations merged on CI alone were reviewed afterwards (e.g. `docs/reviews/xlsx-phase0-impl-round*.md`).
```

### n2（Minor）process.md: "From the second round of PRs onward" が何を指すか分からない

"the second round of PRs" はどこにも定義されていない。読み手にはどの PR から始まったのか分からない。round 1 の案のように、PR 番号で書くほうが確かめやすい。例: "From #11 onward, every ADR and implementation PR is reviewed …"。#11 は事後レビューだが、レビューされたことは事実で、そのことは末尾の注記で説明している。

### n3（nit）改行幅

修正した行（process.md の Units of work、Adversarial review の 2 行、docs/adr/README.md の "Newer ADRs … the first" の行）だけが、他の行（約 100 桁で折り返し）より長い。docs/adr/README.md の行は "the first" の直後で改行していて読みにくい。直さなくても問題はない。

### 確認して問題がなかったこと

- 新しく入った文言を main と照合した: #23 は draft、`xlsx-phase0-impl-round*` は main にある、0006 は Accepted rev 3、#3–#5 は Proposed のままマージされた。どれも正しい。
- 対応コミットで変更されたのは README.md、docs/adr/README.md、docs/process.md の 3 ファイルだけ。リンクの追加や削除はない（round 1 で解決を確認したリンクはそのまま）。

## 判定

**converged: yes**

Blocker・Major はない。n1（事実の誤り）は 1 行で直せるので、マージ前に直すことを勧める。n2・n3 は任意。m2 は #41 とのマージ順で扱い、PR 本文に依存を一言書いておくこと。
