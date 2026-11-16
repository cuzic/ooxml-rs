# ADR-0005 レビュー（Opus, round 2: ADR rev 2 + 実装）

## 対象/方針

対象:
- `docs/adr/0005-pptx-phase0.md` revision 2（Status: Proposed）
- その実装 `crates/ooxml-pptx/`
- core の変更（PR #10: `InvalidXmlChar`、wildcard `ChildOf`）

いずれも main `de5d6ae` の状態を見た。

方針: 読み取り専用の敵対的レビュー。round 1（`0005-opus-review-round1.md`）の B1, B2, M1〜M5, m1〜m10 が ADR の本文とコードの両方で解消されたかを判定し、rev 2 と実装に新しい問題が無いかを探した。根拠は次の5つ。

- **既存テストの実行**: rustc/cargo 1.99.1 で `cargo test -p ooxml-pptx` を実行し、全件が通った。あわせて次も確認した。
  - `cargo clippy -p ooxml-pptx --all-targets -D warnings` と、`ooxml-schema` の `pml` 単独・`dml` 単独の clippy で警告 0。
  - `cargo xtask codegen --check` が up to date。
  - `cargo package --list -p ooxml-pptx` には `src/`、`Cargo.toml`、`README.md` しか入らない。
- **python-pptx での照合**: python-pptx 1.0.2 を scratchpad の site dir に置き、`examples/oracle_cases` の出力に `tests/oracle/check.py` を実行した。結果は `25 cases, 338 edits, 0 failures`。出力 25 個と manual_check の 10 個に含まれる XML パート 1,553 個は、すべて整形式だった。
- **数値の再現**: round 1 と同じ 105 ファイルのコーパスで `spikes/pptx/probe.py` を実行した。ADR が引用する数値（3,418 / 1,737 / 6,121 / 2,510 / 1,665 / 3,512 / 2,333 / 469 / 4,980 / 1,074 / 209 / 43 / 34 / 49 / 374 / 99 / 106）はすべて一致した。
- **敵対的プローブ**: scratchpad に別クレートを作り、fixture をメモリ上で書き換えて試した。対象は、`a:p` が 0 個の本体、`a:br` が先頭にある段落、同じ本体への連続編集、グループをまたいだ id の重複、comment の rel の先が欠けている／External の場合、順序の崩れた `a:p`、編集せずに `slide_mut` した場合。
- **ミューテーションテスト**: リポジトリを scratchpad に `git archive` し、`write.rs` にわざと誤りを5種類入れて T2/T3/T4 を回した（後述 R2-2）。

worktree・ADR・コードには触れていない。fixture は解析しただけで、実行していない。

重大度は round 1 と同じ。**[Blocker]**/**[Major]** は「誤った・壊れた出力になる」か「実装できない」場合だけに使い、それ以外は **[Minor]** とした。

## 結論（先に）

round 1 の Blocker 2 件と Major 5 件は、ADR の本文でも実装でも解消されている。

- **B1**: comment のアンカーを、リストの種類と「平らに並ぶ moniker」の前提で分類するようになった。分からないものは拒否する。単体テストと、メモリ上で `txMkLst` に書き換えた T4 で確認されている。
- **B2**: 文字の扱いを、すべての setter で共通の表1つにまとめた。core 側にも、`set_text`/`set_attr` で XML 外の文字を拒否する保険が入った。その2つ以外に値を書き込む経路が core に無いことも、コードで確かめた。
- **M1〜M5**: 実装に当たって確かめた。
  - 既存の段落を再利用する。
  - 書式は属性だけを写し、写せないものは `FormatNotCopyable` にする。
  - フィールドの扱いを1つの規則にまとめた。
  - T4 にメモリ上で書き換えた入力を加えた。
  - T3 を、デッキ全体について期待値と比較する形にし、python-pptx でも照合する。
  - wildcard の `ChildOf` を追加した。

実装は ADR の規則どおりに動く。「すべての検査を最初の書き換えより前に行う」という主張も、各 setter のコードを追って成り立つことを確かめた。core の残りの拒否についても、次のとおり実際に起こりえない。

- `UnresolvedSibling` と `AlreadyPresent`: `ordered()` による内容の検査で起こらなくなる。
- `NamespaceNotInScope`: 親が DrawingML の要素であれば、その名前空間は必ずスコープ内にある。

整形式でない出力、別の図形・段落・ラン・セルへの書き込み、参照の取り違えにつながる経路は、プローブでもミューテーションでも見つからなかった。

新しい指摘は Minor だけである。

- 実害があるのは **R2-1** の1件。`a:p` を1つも持たない text body（スキーマ違反、コーパスでは 0/4,272）に `TextBody::set_text` をすると panic する。出力は壊れないが、型付きのエラーで拒否するという約束が崩れる。
- オラクルについての指摘が **R2-2**。T3 のモデルは `a:rPr`/`a:pPr`/`a:endParaRPr` の**子要素**を比較しない。そのため、「先頭ランの書式を保つ」という Phase 0 の中心的な約束を、どのオラクルも検査していない。わざと書式の子要素を落とす誤りを入れたところ、T2 も T3 も通過した。Phase 0.1 に copy primitive を入れるなら、その前にこれを直す必要がある。
- そのほかは、文言の矛盾、新しい段落の `endParaRPr`、ADR の列挙漏れ、T4 で検査していないもの。

**Q2（Phase 0.1 の copy-subtree）について: 推奨は「入れる」。** T2 の網羅実行では、2 区切りの `Paragraph::set_text` の 217/599（36 %）と、2 行の `TextBody::set_text` の 156/394（40 %）が `FormatNotCopyable` で拒否される。後者はテンプレートを埋める用途の本体である。同じパートの中でのコピーに限れば Layer 3 も新しい ID も要らず、core に閉じた小さな primitive で済む。API の形は「推奨」に書いた。

判定: **ADR を Minor の修正だけで Accepted にしてよい: yes。実装は ADR に適合している: yes。**

---

## round-1 解消状況

| ID | 内容 | 判定 | 根拠 / 残り |
|---|---|---|---|
| B1 | comment アンカー検査が発火しない | **解消** | ADR §6 L324-339。`refs.rs` `scan_comments`: P188 の `pos/replyLst/txBody/extLst` 以外の子をアンカーとして扱う。`sldMkLst`、および `txMk`/`cp`/`len` を持たない `deMkLst` だけを安全とする。それ以外は配下にある `ac:*@id` をすべてロックし、id が無ければスライド全体をロックする。cm にアンカーが無い場合、未知の子がある場合、パースできない場合、rel の先が欠けている場合もスライド全体をロックする。単体テストは `comment_anchor_classification`。T4 は `modern_comments`（`deMkLst`→`txMkLst`+`txMk` に書き換えた入力で `CommentAnchoredText`、他の図形は編集可、unknownAnchor、壊れたパート）。プローブでは、存在しないパートを指す rel と External の rel のどちらでも `CommentAnchoredText` になった。グループ内の図形は、自分の id とグループの id の両方で照合する（`Guard::new(ids)`） |
| B2 | 制御文字 | **解消** | ADR §3 L185-198 の表と `text.rs` が1対1に対応する。5つの setter すべてが最初に検査を呼ぶ（`write.rs` L511, 558, 688, 701, 751）。core の `check_xml_chars` は `set_attr` L473 と `set_text` L518 にある。core の中で `RawText`/`RawAttr` の値を書く箇所はこの2つだけ（grep で確認）。T4 `characters` は setter 5 種 × {U+0000, U+0001, U+001F, U+FFFE} × {`\n`, `\r`, `\v`} を検査している |
| M1 | 空段落、フィールドだけの段落、複数行 | **解消** | §3 L215-219、L228-240。`plan_paragraph` の `needs_fmt` により、書式の参照は新しいランを作るときだけ行う（L255）。件数は §8 にあり、`probe.py` で再現した。残り: 文言の矛盾 → R2-4、新しい段落に `endParaRPr` が付かない → R2-5 |
| M2 | フィールドの扱いの不一致 | **解消** | §3 L242-247。`plan_paragraph` L251、`TextBodyMut::set_text` L570、`remove_paragraph` L538 で `FieldInParagraph` を返す。`remove_run` は `a:r` しか数えない。T4 は `layouts.pptx` で、拒否されること、`remove_field` の後なら書けることを確認している |
| M3 | T4 の fixture | **解消（一部未検査）** | `refusals.rs` は本物の fixture と `SYNTHETIC:` の入力を分けている。`bug62513` の記述は ADR、roadmap L320、fixture README L45 で直っている。残り: `Core(StrictNotSupported)` などが T4 で検査されていない → R2-7 |
| M4 | オラクル | **解消（判定力に穴）** | T3 はデッキ全体を `apply` の期待値と比べ、python-pptx でも3通りに照合する（実行して 0 failures）。python-pptx についての記述は直っている。`tests/oracle/readback.py` は `content_children` と `p.text` の一致を assert し、part 名を `sldIdLst` から先に読む。T2 は段落と本体の編集も検査する。残り: 書式の子要素を比較しない。python 側の `apply` は Rust 側と構造が同じ → R2-2, R2-3 |
| M5 | allowlist と `ChildOf` | **解消** | `codegen/allowlist/{pml,dml}.toml` は ADR §5 の表と一致する。生成コードは `impl ChildOf<GraphicData> for Table` を含む（`dml.rs` L278）。`Cursor` は型付きでたどる（`write.rs` L59-76）。`allowlist_check.py` は toml を `tomllib` で読む。id は構造をたどって読む（`read.rs` `c_nv_pr`） |
| m1 | allowlist の書式 | **解消** | `[write]`/`[read]` の `names` 形式で、group はファイルで決まる。overrides は無い |
| m2 | `DmlPercent` | **解消** | Phase 0 から外した（ADR §5 L298-303） |
| m3 | `\r` | **解消** | `TextBody::set_text` では正規化し、それ以外では拒否する（`text.rs`、T4） |
| m4 | `app.xml` | **解消** | §6 L367-369、manual_check 07 |
| m5 | autofit の件数、`spAutoFit` | **解消** | 43/34/49/374 を `probe.py` で再現した |
| m6 | 再現できない数値 | **解消** | 本文が引用する数値は `probe.py` の出力とすべて一致した（105 files） |
| m7 | `a:p` の内容、AC の代表 | **解消** | `ordered()` で `UnsupportedContent` を返す（プローブ: 順序の崩れた `a:p` で拒否され、パートは変わらない）。AC の代表は Fallback の最初の図形 |
| m8 | 生成器の警告 | **解消** | clippy `-D warnings` で警告 0（pml 単独、dml 単独、pptx） |
| m9 | fixture のライセンス | **概ね解消** | `include` で crate に入らないことを `cargo package --list` で確認した。round-1 の「足りないこと」のうち、1（トップ README に `reference/fixtures/**` は第三者のファイルという一文）と 3（xlsx/pptx での `SHA256SUMS` の置き方の統一）は手つかず。害は無い |
| m10 | API の細部 | **解消** | `idx() -> Result<u32>`。`InvalidShapeId` は対象と祖先のグループで検査する（`shape_mut` L381）。hyperlink の扱い（§3 L256-260）と前提（Context）を明記した |

---

## R2-1 [Minor] `a:p` が 0 個の text body で `TextBodyMut::set_text` が panic する

### 根拠
- ADR §3 L202 は、内容の形を「body = `a:bodyPr [a:lstStyle] a:p+`」と定め、それ以外は `UnsupportedContent` とする。
- ところが `body_paragraphs`（`write.rs` L178-181）の `ordered()` は、各要素の順位を検査するだけで、`a:p` が1個以上あることは確かめない。
- `TextBodyMut::set_text` は段落数 0 でも先に進み、`extra` が空でないと L581 の `paras.last().expect("minOccurs 1")` で panic する。
- プローブ（`sample.pptx` の subtitle の本体を `<p:txBody><a:bodyPr/><a:lstStyle/></p:txBody>` と `<p:txBody/>` に置き換えた入力）の結果:

  ```
  op=set_text("x")         panicked at write.rs:581:54: minOccurs 1
  op=remove_paragraph(0)   Err(ParagraphNotFound(0))
  op=add_paragraph("y")    Ok(())
  ```
- コーパス（105 ファイル、`p:txBody`/`a:txBody` 4,272 個）にこの形は 0 件。スキーマ違反の入力でだけ起こる。

### なぜ問題か
- 出力は壊れない（panic するので保存まで進まない）。ただ、信頼できない入力でライブラリが panic することになり、§6 の「名前付きのエラーで拒否する」という約束と食い違う。
- ADR の規則（`a:p+`）とコードの検査がずれている唯一の箇所である。

### 提案
- `body_paragraphs` で、`a:p` が 0 個なら `UnsupportedContent("text body without a:p")` を返す（`add_paragraph` もこの入力では拒否されることになる。規則と揃うので問題ない）。または、`TextBody::set_text` で段落 0 個のときは `extra` を全部 `fill_new_paragraph(None, None)` で作る。前者の方が規則と一致する。
- T4 に、メモリ上で書き換えた入力として「`a:p` を外した本体」を1件加える。

## R2-2 [Minor] T3 のモデルが書式の子要素を見ないので、「書式を保つ」約束をどのオラクルも検査していない

### 根拠
- `tests/oracle/model.rs` L47・L54-55 と `tests/oracle/readback.py` の `attrs()` が比べるのは、`a:rPr`/`a:pPr`/`a:endParaRPr` の**属性だけ**。子要素（`a:solidFill`、`a:latin`、`a:sym`、`a:hlinkClick`、箇条書き、`a:spcAft` など）は比較しない。T2 も「差分が段落の範囲に収まるか」しか見ない。
- ミューテーションテストの結果（scratchpad のコピーの `write.rs` に誤りを1つずつ入れて実行）:

  | 入れた誤り | T2 | T3 (Rust) | 備考 |
  |---|---|---|---|
  | `ParaLoc::child` が段落 0 の代わりに最後の段落を返す | **検出** | — | |
  | `RunMut::set_text` がラン 0 の代わりに最後のランに書く | **検出** | — | |
  | `table_cell_mut` が行と列を取り違える | **検出** | — | |
  | `Paragraph::set_text` が `a:endParaRPr` も消す | 通過 | **検出** | |
  | `Paragraph::set_text` が残す先頭ランの `a:rPr` の**子要素を全部消す** | **通過** | **通過** | T4 `format_sources` が偶然落ちただけ。子が消えて写せるようになったため、`FormatNotCopyable` が出なくなった |
- python 側も同じモデルなので、`check.py` でも検出できない。

### なぜ問題か
- Phase 0 が python-pptx 方式（Alternative 2）を退けた理由は、「見た目の書式を失わない」ことだった。ところが、その性質を直接確かめるオラクルが無い。今の実装は先頭ランの `a:rPr` に触らないので正しいが、回帰しても気付けない。
- Phase 0.1 で subtree をコピーするなら、コピーした子要素が元と等しいかを確かめるオラクルが必須になる。

### 提案
- モデルの `rPr`/`pPr`/`end` を「属性 + 子要素の正規形（Clark 名、属性はソート、子は順序どおりの再帰 JSON）」に広げる。Rust 側は `RawNode`、python 側は lxml の要素から同じ JSON を作る。`apply` の新しいラン・段落の期待値は、Phase 0 では「属性だけ（子は空）」になる。これだけで上の誤りは T3 で検出できる。
- `t3_detects_wrong_target` に、図形の付け替えだけでなく「段落の付け替え」「ラン index の付け替え」の 2 ケースを加える（T2 では検出できているが、T3 の判定力を固定しておくため）。

## R2-3 [Minor・判断] T3 の「独立性」は python-pptx が読む部分にあり、`apply` には無い。網羅実行は1本体につき1編集だけ

- `check.py` の `apply` は「§7 から独立に書いた」とされている。しかし `segs`/`build`/`runs`/`run_fmt`/`para_set` の構成が `tests/oracle.rs` と1対1に対応しており、同じ PR で同じ書き手が書いている。仕様の解釈を誤っていれば、両方が同じように誤る。独立しているのは読み手（python-pptx）の方で、ADR §7 もそう書いた方が正確になる。
- 網羅実行（`cases.rs` `op_for`）は、本体ごとに編集を1つだけ当てる。ラン index は 0（`RemoveRun` だけ 1）で、同じ本体への連続した編集は named case の2件にしか無い。連続編集で index が古くなる型の誤りは、T3 では検査されていない。プローブで `set_text` → 段落 6 の `set_text` → `remove_paragraph(0)` → `remove_run` → `set_text` を手で確認したところ、正しかった。
- 提案: 本体ごとに 2〜3 編集を連続で当てる網羅実行を1つ加える（`apply` はすでに列を順に当てられる）。

## R2-4 [Minor] §8 の「1 行の編集は影響を受けない」は、空の段落を埋める場合に誤り。T2 の本体の拒否率も書く

- §8 L456-466 は前半で「空段落の約 5 つに 1 つは拒否される」と書いている。ところが L464-465 は「The common single-line edits (run text, paragraph/body text with no more lines than paragraphs) are not affected」と書く。
- 空のプレースホルダーに1行を入れる操作（段落数を超えない本体のテキスト）は、`a:endParaRPr` に子要素があれば拒否される。T4 `format_sources` の `sample_pptx_grouping_issues.pptx` がその例で、コーパスでは 469/2,510 にあたる。
- T2 の実測では、2 行の `TextBody::set_text` が 156/394（40 %）拒否されている（`locality.rs` の stderr、`body set: FormatNotCopyable 156`）。§8 は 217/599 しか引用していない。
- 提案: 括弧の中を「run text, and paragraph/body text that only rewrites paragraphs which already have a run」に直し、156/394 を追記する。Q2 の根拠としてはこちらの数の方が強い。

## R2-5 [Minor] `TextBody::set_text` が作る段落には `a:endParaRPr` が付かない

- `fill_new_paragraph`（`write.rs` L283-288）は、`a:pPr` の属性を写し、ランには run の書式源を当てる。しかし `a:endParaRPr` は作らない。
- その結果、空の行（例: `"a\n\nb"` の 2 行目）や `\v` で終わる行の高さは、写した `sz` ではなくリストスタイルの既定値で決まる。PowerPoint で Enter を押すと `endParaRPr` も引き継がれる。
- 見た目の差だけで、データは失われない。ADR §3 の「each with a copy of … its run format source」は、ランについてだけ正しい。
- 提案: 書式源が属性だけで写せるときは、新しい段落の `a:endParaRPr` にも同じ属性を写す（規則は「ランを作るとき」と同じ）。または §3/§8 に「新しい段落に `endParaRPr` は付かない」と明記する。期待値のモデル（`"end": null`）も合わせる。

## R2-6 [Minor] 「残りの core の拒否は起こりえない」という列挙から `IncompleteSpec` が漏れている

- ADR §3 L173-177 は、残りの拒否を `UnresolvedSibling`、`AlreadyPresent`、`NamespaceNotInScope` だけ挙げている。
- 実際には、`append_run`/`fill_new_paragraph` は `insert` の後に `set_attr` を呼ぶ。そして `set_attr` は、`E::ATTRS_INCOMPLETE` のとき `IncompleteSpec` を返す（`lens.rs` L462-464）。そうなると、ランや段落を挿入した後で失敗し、途中まで書き換えた状態が残る。
- 今の生成コードは、どの要素についても `ATTRS_INCOMPLETE` を上書きしていない（`dml.rs`/`pml.rs` に 0 件。既定は false）。したがって現状では起こらない。ただし、生成をやり直したときに静かに壊れうる前提になっている。
- 提案: `ooxml-pptx` に `const _: () = assert!(!RunProperties::ATTRS_INCOMPLETE && !ParagraphProperties::ATTRS_INCOMPLETE);` を置き、ADR の列挙に「`IncompleteSpec`（const assert で排除）」を加える。

## R2-7 [Minor] T4 の「§6 のすべての error を `refusals.rs` で発生させる」は、文字どおりには成り立たない

- ADR §7 L423 の主張に対して、`refusals.rs` で発生させていないもの:
  - `Core(StrictNotSupported)`（core のテストにはある）
  - 祖先のグループの id が欠けている場合の `InvalidShapeId`（自分の id の欠落だけを検査している）
  - 順序が崩れている場合の `UnsupportedContent`（検査しているのは `a14:m` だけ）
  - comment の rel の先が欠けている場合と、`p188:cm` にアンカーが無い場合（後者は `refs.rs` の単体テストだけ）
  - `p:txEl` の未知の子と、数値でない `spid`（単体テストだけ）
- プローブで確かめたところ、どれも正しく拒否され、パートは変わらなかった。欠けているのは検査だけである。
- 提案: メモリ上で書き換える入力を 4〜5 件加える。または §7 を「each error variant at least once; sub-conditions listed in §6 are covered by unit tests in `refs.rs`」に書き換える。

---

## core の変更（PR #10）について

問題は見つからなかった。確認したことは次のとおり。

- `is_xml_char` は XML 1.0 の `Char` と一致する。surrogate は `char` に現れない。`check_xml_chars` は `set_attr` でも `set_text` でも、最初の書き換えより前に呼ばれる。`escape_attr` は TAB/LF/CR を文字参照にするので、写した属性値が正規化で変わることも無い。
- 生成器（`model.rs` `wildcard_admits`）と core（`wildcard_admits`、`Wildcard::matches`）で、`##other`/List の意味が一致している。`##other` は名前空間なしも tns も除く。
- 生成された wildcard は、pml/dml/sml/wml 全体で `a:graphicData` の `Any` の1つだけ（`Wildcard::Other` は 0 件）。sml の生成結果が変わらないことは、`codegen --check` で確認した。
- 付記（判断、指摘ではない）: `assert_child` が `content_admits` を使うようになったので、`##any` の親については、ビルド時の検査が生成器の誤りを止めなくなった。代わりに「SDK の `Children` に載っているものだけ」という生成器側の制限が効いている。`ChildOf<GraphicData> for Slide/Presentation` があるため、`graphic_data.insert::<pml::Slide>()` はコンパイルが通り、実行時にも wildcard slot に入る。スキーマ上は正しいが、意味の無い構造になる。Phase 0 は `graphicData` に挿入しないので影響は無い。挿入を使う段階で、format crate 側の規則として書いておけば足りる。

## 推奨

1. **Accepted の前（Minor、どれも小さい）**: R2-1（`a:p+` の検査）、R2-4（§8 の文言と 156/394）、R2-6（const assert）。R2-5 と R2-7 は文言だけ直すのでもよい。
2. **Phase 0.1 の前提**: R2-2（モデルに書式の正規形を入れる）。これが無いと、copy primitive の正しさを検査できない。
3. **Q2: copy-subtree primitive を Phase 0.1 に入れることを推奨する。**
   - 根拠:
     - T2 の網羅実行で、2 区切りの段落の 36 %、2 行の本体の 40 % が `FormatNotCopyable` になる。後者はテンプレートを埋める用途の本体である。
     - コーパスでは、先頭の `a:rPr` の 47 %、`a:pPr` の 38 %、`endParaRPr` の 19 % が子要素を持つ。
     - 同じパートの中でのコピーなら、新しいパート、rel、ID はどれも要らない。Phase 0 の境界（Layer 3 を使わない）の内側に収まる。Phase 2 まで待つ理由は、add-slide の registry とは無関係である。
   - ADR-0002 の用語で書いた API の形（ADR-0002 の post-acceptance note として追加する。決定は変えない）:

     ```rust
     // ooxml-core
     /// Span-less, owned copy of an element subtree, carrying namespace URIs
     /// (not prefixes). Taken through a read lens, so no &mut borrow conflict.
     pub struct Detached<C: ElementSpec> { /* RawNode clone, spans/dirty cleared */ }

     impl<'a, S: ElementSpec> Lens<'a, S> {
         /// Copy of this element, renamed to `C`'s QName. `C: SameType<S>` is
         /// generated for allowlisted pairs with one SDK complex type
         /// (`a:endParaRPr` → `a:rPr`: CT_TextCharacterProperties; `a:pPr`).
         /// `skip`: child QNames to leave out (format policy, e.g. links).
         pub fn detach_as<C: SameType<S>>(&self, skip: &[QName]) -> Result<Detached<C>>;
     }
     impl<'a, E: ElementSpec> LensMut<'a, E> {
         /// Inserts the copy in C's slot exactly like `insert_by` (same
         /// placement and refusals), re-choosing every prefix at the
         /// destination with the ADR-0002 §3 rules; usual dirty duties
         /// (created nodes: own_dirty, re-rendered).
         pub fn insert_detached_by<C: ChildOf<E>>(&mut self, d: Detached<C>,
             before: impl FnMut(usize, &RawNode) -> bool) -> Result<LensMut<'_, C>>;
         pub fn insert_detached<C: ChildOf<E>>(&mut self, d: Detached<C>) -> Result<LensMut<'_, C>>;
     }
     ```
   - 拒否する場合（`detach_as` の時点、つまり何も書き換える前）:
     - subtree に `mc:AlternateContent` がある（ADR-0002 の AC 規則に合わせる）。
     - `r:` 名前空間の属性がある（`skip` で外していない `a:hlinkClick@r:id` など）。rel を共有する重複参照は OPC では合法だが、Layer 3 の参照を増やすので Phase 0.1 では扱わない。
     - `xmlns` の宣言を持つ要素がある（宣言の位置を変える規則を持ち込まないため）。
     - `insert_detached` では、使っている URI の prefix が挿入先に無ければ `NamespaceNotInScope` になる。この拒否は最初の挿入でしか起こらないので、Phase 0 の「拒否なら無変更」は保たれる。
   - pptx 側の規則: 書式源の規則を「属性だけを写す」から「`detach_as` で写す、`skip = [a:hlinkClick, a:hlinkMouseOver]`」に変える（リンクを新しいランへ広げない。挙動は §3 に書く）。`FormatNotCopyable` は上の拒否の場合だけ残す。R2-5 の `endParaRPr` も同じ手段で写す。
   - オラクル: R2-2 の正規形モデルで、「新しいランの書式 = 書式源の正規形から `skip` を除いたもの」を Rust と python の `apply` に入れる。T2 は変えなくてよい。差分は段落の範囲に収まる。
   - 記録の仕方: ADR-0005 rev 3（Phase 0.1 の節）と ADR-0002 の post-acceptance note（`Detached`/`SameType`）。coordinator の既定値「Phase 2 に延期」は、上の数値を根拠に owner が上書きする形で記録する。
4. Q5（本物の `ac:txMk`）は今のままでよい。分類は保守的で、未知のものはスライド全体をロックするので、fixture が見つからなくても安全側に倒れる。

## 判定

- **ADR-0005 を Minor の修正だけで Accepted にしてよい: yes**（Blocker/Major は 0。R2-1〜R2-7 はすべて Minor）
- **実装は ADR に適合している: yes**（R2-1 の panic はスキーマ違反の入力に限られ、出力は壊れない。それ以外の規則と拒否は、すべてコードとテストで確認した）
