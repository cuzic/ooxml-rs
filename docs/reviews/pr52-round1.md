# PR #52 レビュー（Opus, round 1）

対象: `impl/docx-phase1` @ `49c3928`（`origin/main` `ab1b0b6` をマージ済み）。ADR-0007（Accepted）の §1、§2、§6.1、§7 の実装。stories、cross-run の `find`/`replace`/`replace_at`/`replace_matches`、per-operation の W2、`StaleMatch`。
リポジトリは変えていない。確認はすべて scratchpad の作業コピー（`review-52-54/`）で行った。

## 結論（先に）

**収束（Blocker/Major なし。Minor 2 件、nit 4 件）。** 実装は ADR §1–§2、§6.1、§7 のとおりに動く。調べた範囲では、fail open も、`Err` なのに出力が変わる場合も、書式の破壊も見つからなかった。

- 70 の手書きプローブを 2 つの書式ルールで試した。対象は rsid/w14 属性、タブ・改行・`sym`・field・`footnoteReference`・`drawing`・AC・oMath、bidi、wrapper、revision、marker、空白。refusal はどれも ADR §1.1 の分類と §1.4 の理由に合っていた。`Err` のときは、どれも `to_bytes()` が入力と同じだった。
- 821 ファイル（fixtures、POI、SDK、docx4j、templates）で `find` → `replace_at` → 保存 → 再読込 を試した。適用 3,400 件、refused 3,435 件で、失敗は 0 だった。見たのは、文字列の oracle、編集した part 以外がバイト単位で同じこと、run の開始タグ・`w:rPr`・marker の並びが変わらないこと、`Err` ならバイト単位で同じことである。`Match::status` と `replace_at` の結果は全件で一致した。
- main 以外の story にある settable な run 2,800 本すべてに、`set_run_text_at` をかけた（上と同じコーパス）。失敗は 0 で、再読込のテキストも一致した。
- T7 の hint の揃え方（中身が空になった `rFonts` を捨てる規則）は、ADR §1.3 と、ADR の数値を出した spike（`p1common.py` `_rpr_norm`）の両方に合っている（下の「T7」の項）。
- 指摘は 2 件。
  - ADR §9 の T2/T5/T5b/T3 は「すべての story に広げる」としているが、テストとしては入っていない（m1）。上のプローブで振る舞いは確かめた。
  - `Match` は利用者が作れるので、範囲が空の match を作ると、黙って挿入になるか、何も書かずに `replaced: 1` を返す（m2）。

## 確認できたこと（指摘ではない）

| 観点 | 結果 | 根拠 |
|---|---|---|
| ビルドとテスト | ok | `cargo test -p ooxml-docx --release` 全件 pass。**debug ビルドでも全件 pass**（`Flow::build` の `debug_assert!` 2 本は、fixtures の全段落で成り立つ。t7 は debug で 518 秒）。`cargo clippy --workspace --all-targets --all-features -- -D warnings` clean |
| 書式の保持 | ok | `w:p` の `w14:paraId`/`textId`/`rsidR`、`w:r` の `rsidR`/`rsidRPr`、`w14:themeColor` を含む `w:rPr` は、バイト単位で変わらない。残す `w:t` は `set_text` だけで、必要なときに `xml:space="preserve"` を足す（`"name}} ok"` → `" ok"`）。既にある `preserve` は消さない |
| run の中身 | ok | 置換の中のタブ・改行は消え、置換文字列の `\t` は `w:tab` になる。外側の `w:tab`、`w:cr`、`w:noBreakHyphen`、`w:softHyphen` は要素のまま残る。`w:t` の中の生の TAB/LF は文字のまま残る（n1） |
| 境界 | ok | `sym`/`fldChar`/`footnoteReference`/`commentReference`/`drawing`/run 内の AC だけを持つ run → `MatchCrossesBoundary { "w:r (…)" }`。同じ run に文字がある場合は `RunNotTextOnly`。段落直下の AC、`m:oMath`、未知の要素（`w14:foo`）、XML コメント → boundary。空の `w:hyperlink` や隣り合う 2 つの hyperlink も boundary |
| wrapper | ok | `smartTag`、`customXml`、`sdt`、`dir`、ruby → `UnsupportedContext`。`fldSimple` → `InsideField`。hyperlink の内側だけの match は accept、またぐ match は refused |
| revision | ok | `ins`/`moveTo` → `InsideRevision`。match の間にある `del`/`moveFrom` → boundary。端にある `del` は accept されて、そのまま残る。`rPrChange` → `InsideRevision { context: None }`。段落記号の `w:pPr/w:rPr/w:ins` は run に影響しない（正しい） |
| marker | ok | `proofErr` は transparent。collapsed な `_GoBack` は transparent。id が違う隣り合った start/end は anchor。内側の `bookmarkStart`/`commentRangeStart`/`permStart`/`moveFromRangeStart` → `MarkerInsideMatch`。値を囲むブックマークは、置換後も値を囲む |
| bidi / complex script | ok | `w:rtl` の有無、`w:lang/@w:bidi` の違い、`w:hint="cs"`、明示した `w:hint="default"` → `MixedFormatting`。同じ `rtl` の run どうしのアラビア語の placeholder は accept |
| 空白 | ok | 残す側に `preserve` の無い端の空白がある → `UnpreservedWhitespace`（prefix 側・suffix 側とも）。空白が match の中だけにある場合は accept |
| `StaleMatch` | ok | 別の段落の編集の後でも match は使える。同じ段落なら stale。別の `Document` の match も stale。no-op の `replace` と no-op の `set_run_text_at` では generation が増えない。`append_paragraph` の後でも、既存段落の match は使える |
| `OnRefused::Skip` | ok | 同じ段落に accept 1 件と `MixedFormatting` 1 件 → 片方だけ書き、`skipped` に 1 件 |
| per-operation W2 | ok | §6.1 の表は `per_operation_stale_field_risk` が全行を確かめる。入れ子の `REF` は `SEQ` の下でも refused。text の列に format の列を混ぜる mutant は落ちる |
| `manual_check` | ok | 16 個の出力と `CHECKLIST.md` が作られる。Phase1 の項（12–16）が入っている |

### T7 の hint の揃え方（依頼の項目）

- ADR §1.3 は、`w:hint` の違いを無視する条件を 2 つ置く。両方の hint が `eastAsia` か無いこと、そして「hint の違う run」の、match に入る文字がすべて hint-neutral であること。production（`same_formatting`）も reference（`formatting_ok`）も、「hint の違う run」を「r₀ と比べた rᵢ」と読む。調べるのは rᵢ 側の文字だけである。
  - これは正しい。r₀ の文字は r₀ 自身の hint で描かれていて、置換もその hint を受け継ぐ。そのため、見た目が揃っていたかどうかは、rᵢ の文字が hint に左右されないかで決まる。
  - プローブで確かめた。`’` が r₁ にあれば `MixedFormatting`、r₀ にあれば accept。r₁ 側を調べる代わりに r₀ 側を調べる mutant（Q24）は `formatting_rule` で落ちる。
- 「中身が空になった `w:rFonts`/`w:lang` を捨てる」規則は、ADR の数値を出した spike（`p1common.py` `_rpr_norm`、`if not a and not sub and … and k.attrib: continue`）と同じである。ADR の「27.9% が noProof/lang/hint の違いだけ」という測定も、この規則が前提になっている。だから reference をこの規則に合わせたことは、ADR §1.3 の読み方として妥当で、implementation notes に書いたのも正しい。
  - ただ、T7 の差分が独立した二人の読みとして働いたのは、この 1 点の手前までである。今回の一致は、ADR の注記と spike に照らして確かめた結果であり、偶然の一致ではない。
- 属性の無い `<w:rFonts/>` と、`rFonts` が無いこととは、別物として扱われる（`MixedFormatting`）。fail closed の側なので問題は無い。ただし、これを試すテストは無い（mutant Q20 が生き残る。n3）。

### mutant（作者の 7 本とは別に、25 本）

| mutant | 落ちるテスト数 |
|---|---|
| Q1 内側の boundary を見ない / Q2 内側の anchor を見ない | 2 / 2 |
| Q3 どの hint も無視できる / Q4 書式を 2 番目の run とだけ比べる | 1 / 1（T7） |
| Q5 / Q6 `UnpreservedWhitespace` の suffix 側 / prefix 側を外す | 1 / 2 |
| Q8 `rPrChange` を見ない / Q12 separator note を拒否しない | 1 / 1 |
| Q9 doc id を見ない / Q10 generation を見ない / Q11 `set_run_text_at` で bump しない / Q25 `replace` で bump しない / Q21 重なりを見ない | 各 1 |
| Q13 text の列に format の列を混ぜる / Q14 `Refuse` を無視して書く / Q15 残す `w:t` を書かない | 2 / 4 / 14 |
| Q16 first header を常に displayed にする / Q18 header の継承をやめる | 1 / 1 |
| Q19 U+00A0 を neutral にする / Q22 wrapper の終わりを boundary にしない / Q24 hint の文字を r₀ 側で見る | 1 / 1（T7） / 1 |
| **Q7** collapsed の判定で start 側の id を見ない | **なし**。等価な mutant で、end 側の判定が id を見るため、組は anchor に戻る |
| **Q17** even header を常に displayed にする | **なし**（n3） |
| **Q20** 属性の無い `<w:rFonts/>` を「無い」と同じに扱う | **なし**（n3。緩める側だが無害） |
| **Q23** `w:p` 直下の空白以外のテキストを boundary にしない | **なし**（n3） |

## 指摘

### m1 [Minor] ADR §9 の story への T2/T3/T5/T5b が、テストとして入っていない

**場所**: `crates/ooxml-docx/tests/`（`oracles.rs`、`readback.rs` はこの PR で変わっていない）。ADR-0007 §9 の T2「*Runs:* every settable run in every story of every writable fixture gets the Phase0 double edit」、T3、T5「extended」（docx-rs で headers/footers/comments）、T5b（roxmltree で notes と、最終 section から辿れない header/footer）。

- story の編集を確かめているのは、`replace.rs::stories_are_listed_and_edited` の単体文書（header 1 本の `set_run_text_at`、footnote と comment への `replace`）と、T7 の (iii) locality だけである。
- `with_run` の経路は story の種類ごとに違う（`Footnotes → Footnote → …`、`Header` 直下の表など）。fixture 全体の double edit も独立した read-back も無いので、経路の回帰を拾うのは T7 だけになる。T7 は `replace` の経路しか通らない。
- 今の実装に問題は無い。レビューのプローブ（`examples/zz_t2.rs`）で、821 ファイルの、main 以外の story にある settable な run 2,800 本を 1 本ずつ書き換え、次を確かめた。失敗は 0 だった。
  - 編集した story 以外の part がバイト単位で同じこと
  - 段落テキストの長さ
  - 再読込しても同じテキストになること
- PR の説明にも、ADR の implementation notes にも、これを後の part に回すとは書かれていない。

**提案**: `oracles.rs` の T2 を `doc.stories()` 全体に広げる（`set_run_text_at`）。readback に docx-rs の header/footer/comments と、roxmltree の notes を足す。part 2 に回すなら、ADR の implementation notes に「§9 T2/T5/T5b の story 拡張は part 2」と 1 行書く。

### m2 [Minor] 範囲が空の `Match` を作れる。黙って挿入されるか、何も書かずに `replaced: 1` が返る

**場所**: `crates/ooxml-docx/src/replace.rs` L217（`pub struct Match`、`#[non_exhaustive]` なし、フィールドはすべて `pub`）、L918–L927（`replace_matches` の鮮度チェック）

`Document::id()` と `generation()` は公開されているので、利用者は `find` を通さずに `Match` を作れる。`range: 2..2, text: ""` は鮮度チェック（`text().get(range) == Some(text)`）を通る（プローブ `crafted_matches`）。

| 本文 | match | 結果 |
|---|---|---|
| `<w:t>abcde</w:t>` … | `2..2` に `"XY"` | `Ok(replaced 1)`。`abXYcde` が書かれる（挿入。`EmptyNeedle` を迂回する） |
| `abcde` + `footnoteReference` の run + `fgh` | `5..5` に `"XY"` | `Ok(replaced 1)`。何も書かれない（どの run にも当たらない） |

- 2 行目は、ADR §1.4 の「skipping is never silent」に反する。書いていないのに `replaced` に数えている。
- 1 行目は、ADR が `find` で禁じている空の needle を、別の入口から許していることになる。

**提案**: 次のどちらかにする（両方でもよい）。

- `replace_matches` で `m.range.is_empty()` を `Err(EmptyNeedle)` にする。
- `Match` に `#[non_exhaustive]` を付け、作れるのを `find` だけにする。ADR §7 のスケッチはフィールドを `pub` にしているが、`non_exhaustive` を付けても読む側は困らない。

### nit

- n1: 置換のセマンティクスに関わらない観察。空の run が `w:lastRenderedPageBreak` だけを持ち、match の内側にあるとき、その `w:lastRenderedPageBreak` は消える（プローブ「lastRenderedPageBreak empty run inside」→ `<w:r></w:r>`）。§1.2 の規則 1（strictly inside の zero-length child は消す）の意図には合う。ただ、その run は r₀…rₖ ではない（§1.1 の「transparent … stays where it is」）。implementation notes の「Edge `w:lastRenderedPageBreak`」に「r₀…rₖ の間にある、テキストの無い run の中のものも含む」と 1 句足すと、T2/T7 の locality の範囲（r₀ の始まりから rₖ の終わりまで）と合っていることが読み取れる。
- n2: 段落直下の空白以外のテキストと XML コメントは boundary（`"text"`/`"markup"`）になる。fail closed なので問題は無い。ただし `RefusalReason::MatchCrossesBoundary` の doc の例（`w:hyperlink`、`m:oMath`、`w:r (inside a field)`）には、この 2 つが無い。
- n3: mutant Q17、Q20、Q23 が生き残る。次の単体文書を足せば落ちる。
  - Q17: `settings` に `evenAndOddHeaders` が無い文書の even header → `displayed: false`
  - Q20: `<w:rFonts/>` と `rFonts` 無し → `MixedFormatting`
  - Q23: `<w:p>…{{</w:r>stray<w:r>x}}…` → `MatchCrossesBoundary`
- n4: PR の説明では、`OOXML_T7_FULL=1` の結果がまだ出ていない（「still running」）。マージ前に結果をコメントで残すこと。

## 判定

- **converged: yes**
  - ADR §1、§2、§6.1、§7 の振る舞いは、プローブ、コーパス、mutant のどれから見ても正しい。`Err` でも出力は変わらない。
  - m1 はテストの範囲の不足で、振る舞いはレビューのプローブで確かめた。m2 は、利用者が自分で作った入力でだけ起きる API の穴である。どちらも Minor で、次の修正で直すか、ADR の implementation notes で part 2 に回すと明記すればよい。

### 再現手順

- 一式: `<scratchpad>/review-52-54/`
  - `p52/`（PR のコピー）。足したもの:
    - `tests/zz_probes.rs`（`probes`、`stale_and_ids`、`crafted_matches`）
    - `examples/zz_corpus.rs`（find → replace_at → 保存 → 再読込）
    - `examples/zz_t2.rs`（story の run への `set_run_text_at`）
  - `probes52.txt`、`corpus52.txt`、`t2_52.txt`（出力）
  - `m52/run.py`、`m52/out.txt`、`m52/mutrepo/`（mutant）
  - `p52-debug.log`（debug ビルドでのテスト結果）
- プローブ: `cargo test -p ooxml-docx --release --test zz_probes -- --nocapture --test-threads 1`
- コーパス: `CAP=40 target/release/examples/zz_corpus <821 files>`、`zz_t2 <821 files>`（対象は `docx-p1/corpus/{poi,sdk,d4j,tpl}` と `reference/fixtures/docx`）
- mutant: `R=<scratchpad>/review-52-54/m52 python3 m52/run.py [Q1 Q2 …]`
