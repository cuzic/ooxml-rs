# ADR-0007 レビュー（Opus, round 2）

対象: PR #25 `plan/docx-phase1` @ `46fa5ee`（ADR-0007 revision 2）。round 1 は `0007-opus-review-round1.md`。
照合したもの:

- PR #26 `core/insert-at` @ `631fc90`（round 1 の F1〜F5 を直した版）の `lens.rs` の `place` / `check_insert_at` / `set_attr` / `set_text`。
- Phase0 の `ooxml-docx`（`fix-docx-phase0-minors` @ `dfb02a0`）の `text.rs`（`map_text`、`run_text`）。
- 生成済みの `wml.rs` の `Run` の slot 表と、SDK JSON の `CT_R`、`CT_PPr`、`CT_RPr`、`CT_Settings`。
- スパイクの `slot_order.py`（tpl と SDK で再実行し、ADR の数字と一致した）と、`replace_sim.py` の差分。

プローブは `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-25r2-31r2/25/` にある。

- `slots.py`: 書き込み先の親が持つ slot の形を調べる。
- `wt_ctrl.py`: `w:t` の中のタブ・改行・CR を数える。
- `nsprefix.py`: WML の名前空間が既定の名前空間（prefix なし）で宣言されている part を数える。

コーパスは zipfile と ElementTree で読んだだけで、エンティティは解決していない。

## 結論（先に）

- **round 1 の Major 4 件はすべて解消した。** Minor 10 件も解消した。
  - M1（古い `Match`）: 世代カウンタと一致文字列の照合で解決した。
  - M2（原子性）: 書き込みの前に、すべての挿入を dry run する。
  - M3（一致範囲外の正規化）: 一致文字を含む子要素だけを書き換え、オラクル (iv) を足した。
  - M4（`SEQ \s`）: style 列を外し、残りの列をスイッチごとに監査した。
- **ADR-0007 は、PR #26 の F1 のバグの挙動には依存していない。**
  - ADR-0007 が挿入する親は `w:r` の内容、`w:rPr`、`w:pPr`、`w:settings` である。どの親でも、同じ QName が 2 つの slot に出ることはない（SDK JSON と生成済みの `Run` で確かめた。`w:r` の内容は上限なしの slot 1 つ）。そのため F1 の `any` → `all` の修正の前後で結果は変わらない。
  - ただし、`check_insert` ができるまでのつなぎ（`check_insert_at` を使う）が健全なのは、**F2 の修正（上限つきの slot は `place()` で親全体を割り当てる）より後の PR #26 だけ**である（n6）。
- **rev 2 で新しく見つかったのは Minor 7 件と細かい点 3 件で、Major はない。**
  - 一番重いのは n1。一致範囲**外**の文字を `map_text` に通してしまうので、M3 と同じ種類の変化が小さく残る。
  - n2: 世代カウンタは文書全体で 1 つなので、粒度が粗い。
- **判定: Minor だけ直せば Accepted（yes）。**

## 解消状況

| ID | 指摘（round 1） | 状況 | 根拠（rev 2） |
|---|---|---|---|
| M1 | `replace_at` が古い `Match` を黙って使う | **解消** | §7 l.816-823: `generation` と `text()[range] == m.text` の両方を確かめる。T4 に 2 ケースある（l.952-953）。粒度については n2 |
| M2 | 「チェック後は失敗しない」が誤り（`UnresolvedSibling`） | **解消（Minor 残り）** | §0 の 4 段の順序、§7 l.825-853 の dry run、§4 の `DuplicateProperty`、T4 の tpl パターン。`slot_order.py` を再実行して数字が一致した（tpl `w:pPr` 9 / settings 18、SDK settings 1）。属性の書き込みが失敗しうる点は n4、PR #26 の版の固定は n6 |
| M3 | 一致範囲外の run の内容を正規化する | **解消（Minor 残り）** | §1.2: 一致文字を含む子要素だけを書き換える。T7 (iv) を足した。ただし、一致の前後の文字が `map_text` を通る点は n1、(iv) が子要素の順序を見ない点は n3 |
| M4 | style 列が `SEQ \s` を許す | **解消（Minor 残り）** | §6.1 から style 列を外し、スイッチごとの監査表を足した。ただし §4 の表に `run_style` / `paragraph_style` が残っている（n5） |
| m1 | `w:hint` の前提が非 ASCII・非 CJK の文字で崩れる | **解消（Minor 残り）** | §1.3 の表と根拠を直し、`lang/@val` の説明も直した。ただし文字範囲の定義が ADR にない（n7） |
| m2 | T7 の参照実装が独立でない。スパイクが仕様とずれている | **解消** | §9 T7(b): 別の作者が書き、スパイクの件数は参考値にとどめる。スパイクの差分（`w:del` を除く、子の順序を比べる、ruby に降りる）を確かめた |
| m3 | 継承された上書き属性 | **解消** | §4 Rules: `*Chars="0"` を書いて打ち消す。スタイルの連鎖の上限は 32。manual_check に足した。`w:cstheme` も明記した |
| m4 | `fitText/@w:id` | **解消** | §3.2 の `UnsupportedMarkProperties`、§8.2 |
| m5 | §8.2 の生成ルールの文言 | **解消** | 両側から skip を除くと 50 slot ずつで一致する（57−7、51−1。計算が合う） |
| m6 | `set_paragraph_text` の仕様の穴 | **解消** | §3.1: `TextOptions`、`w:r` の有無で分ける、`""` は何もしない、hyperlink / fldSimple / oMath は `ParagraphNotSimple`。Consequences に 1 pt の例がある |
| m7 | `CellRef.row` の数え方 | **解消** | §3.3: 包まれた行も数えたうえで拒否する。`gridBefore` は raw から読む |
| m8 | ヘッダーが表示されるかどうか | **解消** | §2.3 `SectionUse.displayed` |
| m9 | `updateFields` の往復保存 | **解消** | §6.3 と Q5 に MS-OI29500 の引用と確認項目を足した |
| m10 | 細かい点 | **解消** | 引数 `scope` を 1 つにした。大文字小文字は `char::to_lowercase` で比べる。`AmbiguousStyle` を足した。POI の引用行も確認した |

## 新規指摘

### Blocker

なし。

### Major

なし。

### Minor

#### n1. 一致の前後の文字が `map_text` を通るので、`w:t` の中の生のタブ・改行が一致範囲の外で要素に変わる

- **問題:**
  - §1.2 step 2（l.175-182）は、`r₀` で「一致の前の部分＋置換文字列（＋ k=0 のときは一致の後の部分）」を ADR-0004 §3.2 の表（`map_text`）でまとめて piece に分ける。
  - Phase0 の `run_text`（`text.rs:74-87`）は `w:t` の中身を**そのまま**返す。そのため、`w:t` の中の生の `\t` / `\n`（`&#9;`、`&#10;`、`&#13;` も含む）は、`Paragraph::text()` 上では `w:tab` / `w:br` と区別できない。
  - その結果、一致の前の部分にある生のタブは `w:tab` 要素に、生の CR は `w:br` に変わる。どちらも呼び出し側が触れていない文字である。
  - `rₖ` の後ろの部分は「`set_text` to that suffix」（l.186-187）で、`map_text` を通らない。`r₀` の扱いと食い違っている。
  - T7 の (i)(ii) は文字で比べるので、この変化に気づかない。(iv) はその子要素が「一致文字を含む」ので検査の対象外になる。
- **根拠:**
  - コーパスでは 0 件だった（`wt_ctrl.py`。`w:t` は POI 28,405、SDK 48,972、tpl 2,778 個）。
  - しかし POI の `XWPFRun.setText` は値をそのまま `w:t` に入れる（`XWPFRun.java:470-477`）。そのため、プログラムが作った文書には普通に現れうる。
- **推奨:** 次のどちらかにする。
  - (a) 一致の前後の部分は元の文字のまま `w:t` に書き（`set_text` で）、`map_text` を通すのは置換文字列だけにする。
  - (b) 一致の前後の部分に U+0009 / U+000A / U+000D がある `w:t` は、`UnsupportedRunContent` で拒否する（fail closed）。

  どちらの場合も、その形の単体文書を T4 / T7 に足す。

#### n2. 世代カウンタが文書全体で 1 つなので、`replace_at` の用途が狭い。回避策の説明も誤っている

- **問題:**
  - `generation` は書き込みが成功するたびに 1 増える（l.750、l.821）。そのため、1 回の `find` で得た `Match` のうち `replace_at` に使えるのは**1 つだけ**になる。別の段落・別のストーリーの `Match` も古いものとして拒否される。
  - ADR が挙げる用途は「ユーザーに見せた出現箇所を置換する」（Alternatives 12）。これは、複数の出現に別々の値を入れる使い方だが、そのたびに `find` をやり直し、対応する `Match` を探し直す必要がある。
  - l.822-823 は、回避策として `replace` を `Scope::Paragraph` で呼ぶよう書いている。しかし `replace` は、needle の**すべての**出現を**同じ**値で置き換えるので、回避策にならない。
- **推奨:**
  - (a) 世代を**段落ごと**に持つ（`ParaRef` は Phase1 の間ずっと変わらない。段落を足すのは本文の末尾への `append_paragraph` だけである）。T4 の「同じ段落で 2 回」はそのまま `StaleMatch` になり、別の段落の `Match` は使い続けられる。
  - (b) または、`replace_matches(&[(Match, &str)])` のような、まとめて原子的に置換する API を足す。

  あわせて、`replace_at` は `m.status` を使わず、自分の `opts` で §1.4 の検査とゲートをすべてやり直すことを明記する（`find` を `FirstRun` で、`replace_at` を既定値で呼ぶと結果が変わるため）。何もしなかった書き込み（no-op）や、`Skip` で 0 件だった `replace` で世代が増えるかどうかも書く。

#### n3. オラクル (iv) は子要素ごとのバイト列しか比べず、順序と位置を見ない

- **問題:**
  - (iv) は「一致文字を含まない子要素はそれぞれ同じバイト列」と定める（l.1000-1003）。
  - しかし、長さ 0 の `w:lastRenderedPageBreak` が置換文字列の前に残るか後ろに残るかは、(i)〜(iv) のどれでも検出できない（文字数が 0 で、バイト列も同じなので）。
  - §1.2 step 1 の「端にあるものは残す」が正しく実装されたかを確かめる手段がない。
- **推奨:** (iv) を「影響を受けた子要素の並びを 1 つの印に置き換えたとき、`r₀ … rₖ` の子の並び（バイト列）が前後で一致する」と定める。これで、順序と、置換文字列との相対的な位置も検査できる。

#### n4. 「set-attribute は失敗しない」（l.853）は成り立たない場合がある

- **問題:**
  - core の `set_attr` は、新しい名前空間つき属性に、**空でない** prefix が束縛されていることを求める（`lens.rs:577-590`。なければ `NamespaceNotInScope`）。
  - WML を既定の名前空間（`xmlns="…/main"`）で書いた part では、たとえば次のようになる。
    - 新しく作った `jc` に `w:val` を付けようとすると失敗する。
    - `w:color` に `w:val` を付けようとしても失敗する。
  - このとき、要素の挿入（`check_insert` は prefix を確かめる）は通ってしまい、その**後**に失敗する。一部だけ書き換わった状態が残る。
- **根拠:**
  - コーパスでは 0 件（`nsprefix.py`。WML の part は POI 782、SDK 3,286、tpl 1,464 個）。
  - ただし、XML として正しい入力である。
- **推奨:** dry run に、属性の prefix の検査を加える。domain 側で「対象の要素で `w` の URI が空でない prefix に束縛されている」ことを確かめるか、core に `check_set_attr` を足す。あるいは、l.853 の主張を、この前提つきの書き方に弱める。

#### n5. §4 の表に `run_style` / `paragraph_style` が残っていて、§6.1 の format 列と矛盾する

- **問題:**
  - §4 の setter 表に `run_style`（l.494）と `paragraph_style`（l.506）が残っている。
  - 一方、§6.1 の format 列は「§4 の setter」に `SEQ … \s n` を許し、その根拠を「§4 の部分集合はスタイルを書かない」（l.611）としている。
  - §7 の `RunFormat` / `ParagraphFormat` にスタイルの項目はないので、実装の意図は明らかである。それでも、表を文字どおりに読むと M4 が戻ってくる。
- **推奨:** 2 行を §4 の表から §5 に移すか、「§5 の関数。§6.1 の format 列ではなく、文書全体の W2 で判定する」と注記する。

#### n6. PR #26 の依存先を、F1/F2 を直した後の版に固定する

- **問題:**
  - §7 のつなぎ（l.837-838）は、「`check_insert` ができるまでは、domain が slot 表から index を計算し、`check_insert_at` と `insert_at` を使う」としている。
  - これが、順序の壊れた親（tpl の `[sectPr, pStyle, bidi, jc, rPr]`）を拒否できるのは、`631fc90` で上限つきの slot が `place()` を呼ぶようになったからである（F2/F3 の修正。`w:rPr` / `w:pPr` / `w:settings` の slot はどれも `max_each` が 1）。
    - `place()` は親全体を貪欲に割り当てる。AC がどこにあっても拒否し、順序が壊れていても拒否する（`lens.rs:338-376`）。
  - 修正前の `31827b5` では、上限の計数が順序を見ない。隣が合えば、壊れた `w:pPr` への挿入を `Ok` にしてしまう。その場合、T4 の「最初のプロパティを書く前に失敗する」は成り立たない。
  - また §7 l.829 の `check_insert_at` の説明（「近傍の slot 順」）は、上限のない slot（`w:r` の内容）にしか当てはまらない。
- **推奨:**
  - Sources（l.1208）と Core dependencies に「PR #26 @ `631fc90` 以降（review round 1 の F1〜F3 の修正を含む）」と書く。
  - §7 に「上限つきの slot では `check_insert_at` も親全体を検査する」と書く。

  PR #26 自体は、まだ round 2 で収束していない。

#### n7. hint を無視してよい「ASCII か CJK」の範囲を、ADR に code point で書く

- **問題:**
  - §1.3（l.225、l.234-236）は範囲を言葉でしか書いていない。
  - スパイクの `hint_neutral` は特定の表を使っている。たとえば次のものは**含まない**。
    - BMP 外の CJK 拡張 B 以降
    - Hangul Jamo（U+1100–11FF）
    - Kana 拡張（U+31F0– など）
    - CJK Compatibility Forms（U+FE30–FE4F）
  - T7 は、別の作者が ADR **だけ**から参照分類器を書く（m2）。そのため、範囲が書かれていないと、(a) と (b) の差が実装上の解釈の違いから出てしまう。
  - `w:hint="cs"` のときに ASCII の slot が変わらないかも未確認である *(u)*。
- **推奨:** code point の表を ADR に載せる（fail closed なので、スパイクの表のままでよい）。hint の値が `eastAsia` と `default` 以外のときは、無視しないことにしてもよい。

### 細かい点

- **n8.** §1.2 step 2 の「directly after **it**」（l.182）は、最初の影響を受けた子が `w:t` でないとき（`w:tab` の位置から一致が始まる場合）や、残す `w:t` がないとき（置換文字列がタブで始まる場合）に、何を指すかが決まらない。「最初の影響を受けた子要素があった位置に」と書く。
- **n9.** 「各子要素は 1 回だけ書く」（l.192）は、`{{a}} {{b}}` が 1 つの `w:t` にある、最もよくある場合に、計画をまとめる必要があることを意味する。step 2〜3 は一致ごとの手順として書かれているので、同じ子要素に複数の一致があるときの合成（一致の前後の部分は、まとめた計画の上で取る）を 1 文足す。
- **n10.** `generation` はどの `Document` でも 0 から始まる。そのため、別に開いた文書の `Match` も、世代と文字列が合えば受け付けてしまう。害はないが、文書ごとの識別子を持たせるか、ドキュメントに注記する。

## 確かめて問題なしとした点

- **ADR-0007 の挿入先の slot の形。** `CT_PPr` 36、`CT_RPr` 51、`CT_Settings` 103 は、子の QName に重複がない。`w:r` の内容は上限のない slot 1 つで、曖昧な名前はない（生成済みの `Run`）。そのため、§7 の「1 つの親への複数の挿入を、変更前の木で別々に確かめれば足りる」という論証は成り立つ。
- **`slot_order.py` の再実行。** tpl の `w:pPr` 9 / settings 18、SDK の settings 1（`m:mathPr` が `w:rsids` の後にある）は、ADR と一致した。settings の重複 2 件（`w:compat` が 2 つ）は `updateFields` の挿入では数えられず、壊れた状態はそのまま残る。拒否はしないが、悪化もしない。
- **§8.2 の slot 数。** 57 − 7 = 50、51 − 1 = 50 で計算が合う。
- **§6.1 のスイッチ監査。**
  - text / append / format のどの操作も、見出し、段落の数、ブックマーク、注の位置を変えない。そのため `SEQ \s`、`SEQ` のブックマーク参照、`NOTEREF \p` を許すのは健全である（n5 の表の矛盾を除く）。
  - `NUMWORDS` などを format 列だけに置き、`Lines` / `Pages` を除くのも妥当である。
- **`set_text` の `xml:space`。** core は付けるだけで外さない（`lens.rs:625-631`）。§1.2 の「既存のものは外さない」と一致する。
- **原子性の順序**（ゲート → 対象の検査 → dry run → 書き込み）と、拒否された呼び出しで世代が増えないこと。

## 判定

**Accepted after Minor only: yes.**

- Blocker・Major はない。
- n1〜n7 を rev 3 で反映すれば（または理由を付けて見送れば）、round 3 は要らない。
- 優先順位は n1（一致範囲外の変化が残る）、n2（API の使い勝手と誤った回避策）、n6（依存する版）の順。
