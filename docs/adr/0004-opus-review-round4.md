# PR #13 レビュー（Opus, ADR-0004 round 4 / 実装 round 3）

## 対象・方針

- 対象: PR #13（branch `impl/docx-phase0`, HEAD `db53cdf`）。`docs/adr/0004-docx-phase0.md` revision 4、`crates/ooxml-docx`（src / tests / examples）、`docs/roadmap/docx.md`。比較の基準は round 3 レビュー（`0004-opus-review-round3.md`, 実装 `498a47a` / レビュー commit `16475fd`）。
- 方法: 読み取り専用の敵対的レビュー。`git archive` で `db53cdf`（new）と `16475fd`（old = rev 3）を scratchpad の `review-docx-r4/` に展開して、次のことをした。
  - 既存テストを全部実行した（lib 2、adversarial 19、editing 6、fixtures 7、gates 10、oracles 4、readback 2、doctest 1。全 green）。
  - **走査の差分テスト**: rev 3 の反復版 `body.rs`/`fields.rs`（`r3_*`）と rev 2 の再帰版（`old_*`）をクレートに入れ、rev 4 と三者で突き合わせた（`src/difftest4.rs`）。
  - **コーパスのゲート差分**: 同じ例 `examples/gatescan.rs` を old と new の両方でビルドし、643 ファイルについて open の結果、`write_gates()`、全 run の `check_settable()` の種類と `in_field()` を出力して比べた。
  - 新しいプローブ `tests/r4.rs`（S01〜S20、結果を表示するだけ）。
  - ミューテーション 34 本（`mut/run.py`）。
  - コーパス走査 `scan4.py`（`python3 -I`、DTD を持つ part は読まない）。
  - fixture とコーパスは parse にだけ使った。リポジトリはこのファイル以外触っていない。
- 重大度: **Blocker/Major** = 破損、黙った内容変更、古いフィールドが残りうること、拒否の欠落。それ以外は **Minor**。判断が分かれるものには「判断」と付けた。*(u)* は Word で未確認の事項。

## 結論（先に）

N3 と m7〜m11 はすべて解消した。round 3 の指摘に「一部」は残っていない。

- N3: 入れ子のフィールドを含むか、引数が無い `INCLUDETEXT`/`INCLUDEPICTURE`/`LINK` は refused になる。入れ子が `w:ins` や `w:smartTag` の中にあっても拒否される。
- m7: field scan が body walk と同じ文法（`body::descend`）を使うようになった。round 3 の6形に加えて、新しく12形を試した。begin が walk から見えないものは、すべて `<malformed field>` になる。
- 共通化のリファクタで退行は無い。
  - 走査の差分テスト: 実コーパス 564 ファイルの 5,109 part（56,481 段落、82,677 run）と、ランダム生成 50,000 文書（292 万段落、820 万 run）で、rev 2・rev 3・rev 4 の path、wrapper、`in_field`、`open_fields_at_end` が全件一致した。
  - ゲート差分: 643 ファイルすべてで出力が **byte 一致**した。対象は open の結果、ゲート、run 77,728 本それぞれの settable / 拒否の種類 / `in_field`。
  - つまり、settable な run も拒否される文書も1件も変わっていない。ADR の「643 ファイルでゲートの結果は不変」は再現した。

Blocker/Major は見つからなかった。新しい指摘は Minor が3件で、どれもコーパスに 0 件の入力か、Word の挙動が *(u)* のもの。

- **m12**: DOCPROPERTY のカスタム名の照合が fail closed になっていない。
  - linkTarget を持つカスタムプロパティが組み込みと同じ名前（`Title`、`Company` など）だと、allow-list が先に効いて safe になる。
  - 前後の空白を `trim` するので、名前が空白だけ違う linked/unlinked の組で、unlinked のほうに一致する。
  - 引用符の中に曲引用符があると、引数がそこで切れる。
- **m13**: Word の「リンク貼り付け」が文書の中で作る自己参照の `LINK Word.Document.12 "<自分のパス>" "OLE_LINK1"` が safe になる。
  - ADR がリテラルのパスを *(u)* として受け入れている範囲ではある。ただ、Word 自身が書く形で、しかも構文だけで検出できる（ブックマーク名が文書内にある）。
  - `LINK` は第1引数が ProgID なので、「リテラルの第1引数」の検査はファイルを見ていない。
- **m14**: テストの検出力に小さな穴がある。生き残るミューテーションは M5、M20、M22、M32、M33、M34、M37 の7本。どれも、規則を外すと危険になるのにテストが無いか、別の規則と重複するもの。

**判定: converged = yes、ADR を Accepted にしてよい = yes。** m12〜m14 は Accepted の後で直してよい。m12 と m13 は、owner が Word で手動確認するときに一緒に確かめるとよい。

## round-3 指摘の解消状況

| ID | 内容 | 状況 | 根拠 |
|---|---|---|---|
| N3 | `INCLUDETEXT "{ FILENAME \p }" bm` が safe | **解消** | `Open.nested`（`fields.rs` の `begin` で、外側の field がまだ separate していなければ立てる）と `key()` の `FILE_SOURCE` 分岐。テストは `n3_file_source_must_be_a_literal`（Q05 そのもの。`set_run_text` が `StaleFieldRisk` になる。INCLUDEPICTURE、LINK、引数なしも）。プローブ S17: `w:fldSimple` を instruction の中に置き、結果が `""` になるものも refused。S18: 入れ子が `w:ins`、`w:smartTag` の中にあっても refused。ミューテーション M16/M17/M18 は落ちる（下記）。リテラルのパスで自分を指す場合は ADR に *(u)* として明記された。ただし Word の「リンク貼り付け」の形は検出できる（m13） |
| m7 | begin が walk から見えないと R1 が fail open | **解消** | `ScanMode::{Walk(Mode), Foreign}` と container 番号。`separate`/`end`/`instrText` の container が begin と違えば MALFORMED、run の直下にない `fldChar`/`instrText` も MALFORMED。テストは `m7_field_markers_outside_the_body_walk_are_malformed`（Q01 の6形と、run の外の instrText）。新しく試した形（S20）: end がテキストボックスにある、begin が `w:pPr` の偽 run にある、`w:rPr` の中にある、run 内の `mc:AlternateContent` にある、`w:footnote` を本文に埋め込んだもの、未知の inline/block 要素の中にある。これらは**すべて refused**。begin が `w:rubyBase`、`w:moveFrom`、`w:fldSimple` の結果、行レベルの `w:customXml` にあるものは walk が数えるので、RESULT の run は `InsideField` になる（正しい）。構造的にも、Foreign の container は walk が入らない部分木の中にしか無いので、container 0（本文）にある marker は walk が数えるものと一致する |
| m8 | A2 を空のブロックですり抜ける | **解消** | `ends_with_section_break` は「本文の流れで最後の段落の `pPr/sectPr`」だけで判定するようになった。テストは `m8_a2_with_blocks_without_paragraphs_after_the_break`（Q02 の5形と、orphan の `w:sdtContent`）。M6 と M36 は落ちる。判定が緩くなる方向の変化（段落を持つ表が後ろにある場合など）はコーパスで不変（643 ファイルの出力が一致） |
| m9 | DOCPROPERTY が拒否リスト方式、曲引用符、BARCODE | **解消**（照合の穴は新規 m12） | `DOCPROPERTY_SAFE` の allow-list、`QUOTES`（“ ” „ ‟ ＂）、`BARCODE` は `to_lowercase().contains("\\b")`。S09: `"Words"\* MERGEFORMAT`、`Words\*MERGEFORMAT`、`\* MERGEFORMAT Words`（引数なしとして refused）、タブ、全角空白、`„Words“`、`'Words'` はどれも refused。全角英字の `Ｗｏｒｄｓ` も refused。S11: app.xml の `TitlesOfParts`、`HeadingPairs`、`LinksUpToDate`、`DocSecurity`、`AppVersion`、`TotalTime` などは allow-list に無いので refused。`manual_check` の 11 番で、Word が曲引用符をどう読むかを確かめられる |
| m10 | M4 と M13 が生き残る | **解消** | `m10_unknown_type_rule_alone`（`Separate` と大文字で書いた形）で M4 は落ちる。M13 は ADR R1 と `body.rs` のコメントに「R4 と重複する防御」と書かれた（等価ミューテーションとして扱ってよい） |
| m11 | settings への関係が2本 | **解消** | `target(SETTINGS_RELATIONSHIP)` を全部読み、W1/W3 はどれか1つが on なら on。`updateFields`、`writeProtection`、custom-properties の part もまとめて扱う。テストは `m11_every_settings_part_counts`。M31/M32/M33 は落ちる。M34（custom part を1つしか読まない）は**落ちない**（m14） |

## task 2: 共通文法へのリファクタによる退行

- **走査の三者差分**（`difftest4.rs`）:
  - 実コーパス: POI 129、SDK 502、docx4j 12 のうち開ける 564 ファイル。main part と、その rels の XML part をすべて対象にした（5,109 part）。
  - ランダム生成: 50,000 文書。round 3 と同じ生成器で、`txbxContent`、`rt`、`m:oMath`、`mc:AlternateContent`、`fldSimple` などの位置に fldChar や instrText が混ざる。
  - どちらも、段落と run の path、wrapper、`in_field`、`open_fields_at_end` が rev 2、rev 3、rev 4 で全件一致した。`descend()` への切り出しは、walk の挙動を変えていない。
- **field scan の差分**:
  - 実コーパスでは、5,109 part すべてでキーの列が rev 3 と完全に同じだった。
  - `is_safe` の判定が変わったのは1 part だけ。docx4j の `field_numeric_formatting_switch.docx` で、`DOCPROPERTY currency` などのカスタム名が allow-list に無くなった。ただし custom.xml に名前があるので、ファイル単位のゲートは不変。
  - ランダム生成では、rev 3 が refused にして rev 4 が通すものは **0 件**だった（rev 4 で新しく refused になるものも、この生成器では 0 件）。
- **コーパスのゲート差分**（`gatescan`, 643 ファイル）:
  - old と new の出力が byte 一致した。
  - 内訳: 書き込める 516、W2 33、W1 11、W3 4、open で拒否 79（Strict 60、zip でない 13、暗号化 4、DTD 1、TooDeep 1）。
  - 全 run 77,728 本のうち、settable は 67,028 本。
  - 正当化の必要な変化は無い。

## task 3: DOCPROPERTY の allow-list と曲引用符への攻撃

プローブ S01〜S11 の結果（○ = safe として通る、× = refused）。

| プローブ | 入力 | 結果 | 評価 |
|---|---|---|---|
| S01 | linked のカスタム `Title`（`Company`、`Manager`、`Category`、`HyperlinkBase`、`Pages`、`Template` も）+ `DOCPROPERTY Title` | ○ | **穴（m12a）**。組み込みとカスタムが同名のとき、Word がどちらを返すかは *(u)*。fail closed なら refused にすべき。ADR は「properties linked to content は refused」と書いているので、記述とも食い違う |
| S02/S03 | linked `"Client "`（または `" Client"`、`"Client\u00A0"`）+ unlinked `Client`。フィールドは `DOCPROPERTY "Client "` | ○ | **穴（m12b）**。引数は `trim()` されるが、custom.xml の名前はされない。Word が引用符の中の空白を残すなら、linked のほうを返す *(u)* |
| S03c | linked `Client` だけ + `DOCPROPERTY "Client "` | × | 正しい |
| S04 | linked `A”B` + unlinked `A`。フィールドは `DOCPROPERTY "A”B"` | ○ | **穴（m12c）**。`"` で開いた引数が `”` で閉じる。Word が `"…"` の中の `”` を普通の文字として扱うなら、linked のほうを返す *(u)* |
| S05 | linked `Client Name` + unlinked `Client`。フィールドは `DOCPROPERTY Client Name`（引用符なし） | ○ | Word も最初の語だけを読むなら一致する *(u)*。参考 |
| S06、S07b | 大文字小文字の違い、`ß`→`SS` | × | 完全な大文字化なので、衝突は拒否される側に倒れる |
| S07 | linked `İD` + unlinked `ID`、`DOCPROPERTY "id"` | ○ | トルコ語ロケールの大文字小文字の同一視が絡む。非常に特殊なので、参考 |
| S07c、S08 | Kelvin 記号、NFD と NFC | ○ | Word も序数で比較するなら一致する。参考 |
| S09 | 上の表のスイッチと引用符の変種 | ×（`DOCPROPERTY Title Words`、`"Title" \* Upper "Words"` だけ ○） | 第1引数だけを読むのは Word と同じ。妥当 |
| S11 | `DOCPROPERTY Pages` | ○ | 設計どおり（pagination）。ただし app.xml の `Pages` は保存時の統計値で、`Words` と同じ種類のもの。レイアウトのたびに更新されるかは *(u)* で、round 1 から owner が受け入れている `NUMPAGES` の扱いと同じ。参考として、manual_check で `DOCPROPERTY Pages` も一緒に確かめるとよい |

- コーパスでの出現: `linkTarget` を持つ custom.xml は 0 件（custom.xml 自体は 37 件）。曲引用符も 0 件。DOCPROPERTY は docx4j の1ファイルだけで、すべて unlinked のカスタム名。

## task 4: Phase0 の編集や追加の後に古い結果が残りうるフィールドの探索

- **m13（下記）**: Word の「リンク貼り付け」で作られる自己参照の `LINK`。
- **調べたが問題なしとしたもの**:
  - pagination（`SECTION`/`SECTIONPAGES` は A2 があるので節の数が変わらない）。
  - `HYPERLINK \l`、`GOTOBUTTON`、`SET`/`ASK`（読む側の `REF` は refused）。
  - `EQ`、`DISPLAYBARCODE`、`NEXTIF`/`SKIPIF`。入れ子のフィールドはそれぞれ分類される。
  - `w:fldLock`: ロックされたフィールドは作者が凍結したもので、Phase0 によって古くなるのではない。コーパスに 1 件。
  - `FILENAME` を別名で保存した場合: Word も Save As の後は F9 まで更新しないので、Phase0 に固有の問題ではない。
  - ヘッダーやテキストボックスの中のフィールド（Foreign の container で分類される）。
  - 脚注、コメントの中の `REF`/`NOTEREF`。
  - 追加した段落に依存しうるもの（`=SUM(ABOVE)`、`NUMWORDS`、`PAGEREF`、`TOC`、`SEQ`、`LISTNUM`、`AUTONUM*`）。これらはすべて refused。
- **参考（指摘にしない）**:
  - `INCLUDETEXT "C:\\d\\` + `<w:fldSimple FILENAME>` + `" bm`（S17b）は safe になる。`w:fldSimple` は `nested` を立てないため。Word は instruction の中に fldSimple を書かず、パスの一部しか作れないので、参考にとどめる。
  - `INCLUDETEXT "self.docx" bm` のような相対パス（S16b）は、ADR の *(u)* の範囲。

## 指摘

### m12 [Minor, 判断] DOCPROPERTY のカスタム名の照合が fail closed になっていない

**根拠**（`fields.rs` `refused()` と `is_safe()`、プローブ S01〜S04）

- (a) `is_safe(c) || (custom …)` の順に判定するので、`DOCPROPERTY_SAFE` に載っている名前は、linked のカスタムプロパティがあっても safe になる。
  - 例: linked のカスタム `Company` と `DOCPROPERTY Company` の組で、gate は Ok、`set_run_text` も Ok。
  - ADR §6 の「everything else is refused: … properties linked to content」と食い違う。
- (b) `first_argument` は引数を `trim()` する（ASCII 以外の空白も）。一方、`CustomNames` は名前を大文字にするだけ。
  - そのため、linked の `"Client "` と unlinked の `"Client"` があると、`DOCPROPERTY "Client "` は unlinked のほうに一致して safe になる。
- (c) `"` で開いた引数は、`QUOTES` のどれでも閉じる。
  - そのため、名前に `”` を含む linked のプロパティ（`A”B`）を指す `DOCPROPERTY "A”B"` は `A` として読まれる。
- どれも、Word がその名前を linked のほうに解決するかは *(u)*。(b) と (c) は名前を作り込む必要がある。コーパスでの出現は 0 件（linkTarget が 0 件）。

**提案**（どちらか）

- 簡単な方法: custom.xml に linkTarget を持つプロパティが1つでもあれば、`DOCPROPERTY` はすべて refused にする。コーパスでの損失は 0。
- 精密な方法: linked の名前と引数を同じように正規化し（trim、大文字化、引用符を除く）、組み込みの allow-list より先に照合する。さらに「引数が linked の名前の接頭辞になっている」場合も refused にする。
- どちらの場合も、S01 と S02 を adversarial のテストに足す。

### m13 [Minor, 判断] 文書の中での「リンク貼り付け」による自己参照の `LINK` が safe になる

**根拠**（S15、S16）

- `{ LINK Word.Document.12 "C:\\x\\self.docx" "OLE_LINK1" \a \r }` と、本文のブックマーク `OLE_LINK1` の組で、gate は Ok になる。`\a` を外しても、ファイル名を `""` にしても同じ。
- Word で文書の一部をコピーし、同じ文書に「形式を選択して貼り付け → リンク貼り付け」をすると、この形ができる *(u)*。自分自身へのパスと、Word が付ける `OLE_LINKn` ブックマークを持つ。
  - ブックマーク内の run を Phase0 で編集すると、LINK の結果が古いまま残る。`\a` が付いていても、開くときにリンクを更新するかはユーザーの選択に任される。
  - コーパスに LINK は 0 件。ただし `OLE_LINK` ブックマークは SDK の4ファイルにあり、Word がこのブックマークを日常的に作ることが分かる。
- ADR はリテラルのパスによる自己参照を *(u)* として受け入れている。round 3 でも、検出できないものとして残すことを提案した。ただ、この形は**構文だけで検出できる**。第2引数（LINK）またはブックマーク引数（INCLUDETEXT）が、この文書の `w:bookmarkStart/@w:name` に一致すればよい。
- さらに、`LINK` の第1引数は ProgID である。そのため、`FILE_SOURCE` の「リテラルの第1引数があるか」の検査は、LINK についてはファイルの引数を見ていない（`LINK Word.Document.12` だけでも safe になる）。ADR の「or no argument at all」は、LINK については成り立っていない。

**提案**

- `INCLUDETEXT`/`LINK` の第2/第3引数のうち、ブックマーク名や項目名にあたるものが文書内のブックマーク名と（大文字小文字を区別せず）一致すれば、`<CODE> (self reference)` などのキーで refused にする。
- LINK の「ファイル引数が無い」は、第2引数で判定する。
- 合わせて手間が大きいなら、ADR の *(u)* の記述に「Word のリンク貼り付けがこの形を書く」と書き足し、owner が明示的に受け入れるのでもよい。
- Word で確かめて、`\a` のない self-LINK を Word が普通に書くと分かったら、Major に上げる。

### m14 [Minor] テストの検出力: 生き残るミューテーション

- **M5**（段落内の `mc:AlternateContent` の規則）:
  - rev 4 では、段落内の AC が別の container になる。そのため、`m1_fields_in_paragraph_level_alternate_content` の各形は container の規則でも捕まり、M5 では落ちなくなった。
  - この規則が単独で効くのは、AC の各分岐の中で完結したフィールドだけ。その結果の run は walk から見えないので、R1 の安全性には効かない。
  - 提案: この規則を「container の規則と重複する防御」と ADR に書くか、規則を外して文書を簡単にする。
- **M20 / M22**（separate と instrText の container の検査）:
  - end の container の検査（M21 で落ちる）があるので、separate だけを別の container に置く形は R1 には影響しない（M20 はほぼ等価）。
  - M22 では、別の container にある `w:instrText` が本文のフィールドのコードに連結される。分類が変わりうる。
  - 提案: begin、separate、end が本文にあり、instrText だけがテキストボックスにある形を MALFORMED とするテストを1本足す。
- **M32 / M33**（W1/W3 を「最後の settings が勝つ」にする）:
  - `m11_every_settings_part_counts` は、1本目が空で2本目が on なので区別できない。
  - 提案: 1本目が on で2本目が off（`w:val="0"`）の形も足す。
- **M34**（custom-properties の part を1つしか読まない）: テストが無い。2本目の custom part にある linkTarget を拒否するテストを足す。
- **M37**（`INFO`/`DOCPROPERTY` の `nested` 規則）:
  - この規則が無いと、`DOCPROPERTY "A{ QUOTE B }"` の key は `DOCPROPERTY A` になる（入れ子の結果は instruction に入らない）。unlinked の `A` があれば safe になる。
  - Word が引数を `AB` と読み、linked の `AB` を返すなら、結果が古いまま残る *(u)*。規則自体は正しいが、テストが無い。
  - 提案: この形を1本足す。

## task 5: ミューテーション

34 本。M1〜M15 のうち12本は round 3 と同じ規則を rev 4 のコードに合わせたもので、M7、M8、M10 は対象のコードが置き換わったので外した。M16〜M37 は rev 4 の新しい規則が対象。

| ミューテーション | 落ちるテスト |
|---|---|
| M1 rPr を落とす / M2 二重空白 / M3 隣の run に書く | 6 / 4 / 11 本（round 3 と同等） |
| M4 type 不正を MALFORMED にしない | `m10_unknown_type_rule_alone`（**round 3 の m10 は解消**） |
| **M5** 段落内の `mc:AlternateContent` の規則を外す（`!in_run` だけ残す） | **なし**（round 3 では `m1_fields_in_paragraph_level_alternate_content` で落ちた） |
| M6 A2 を外す | 2 本 |
| M9 計算型 FORMTEXT / M11 no-op の preserve / M12 rubyBase / M14 begin を数えない / M15 linkTarget | 1 / 1 / 1 / 6 / 1 本 |
| M13 `has_fld_char` を外す | なし（等価なミューテーション。ADR に記載済み） |
| M16 `nested` を key で無視 / M17 `nested` を立てない / M18 引数なしを許す | 2 / 1 / 2 本 |
| M19 run の外の fldChar を許す / M21 end の container を見ない / M23 run の外の instrText を許す / M24 新しい container を作らない | どれも 1 本（`m7_…`） |
| **M20** separate の container を見ない | **なし** |
| **M22** instrText の container を見ない | **なし** |
| M25 straight quote だけ / M26 拒否リストに戻す / M27 linked を見ない / M28 どの名前もカスタム扱い / M29 統計値の名前のカスタムを許す / M30 BARCODE をトークン一致に戻す | 2 / 3 / 1 / 2 / 1 / 2 本 |
| M31 settings を1つだけ読む | 1 本 |
| **M32** W1 を最後の settings で上書き / **M33** W3 を最後の settings で上書き | **なし** |
| **M34** custom part を1つだけ読む | **なし** |
| M35 A2 で本文直下の sectPr を見ない / M36 A2 を最初の段落で判定 | 2 / 2 本 |
| **M37** `INFO`/`DOCPROPERTY` の `nested` 規則を外す | **なし** |

生き残るもの（太字）の評価は m14 に書いた。安全に関わる主要な規則（N3、m7 の位置と end の container、m9、m11 の part の数え方、A2）は、どれも専用のテストで落ちる。

## 判定

- **converged（Blocker/Major が無い）: yes**
  - N3 と m7〜m11 は、本文、実装、テストのすべてで閉じている。round 3 の「一部」は残っていない。
  - 共通文法へのリファクタは、走査の三者差分（コーパスとランダム生成）でも、643 ファイルのゲートと run 単位の差分でも、変化 0。
  - 新しい指摘 m12〜m14 は Minor。m12 と m13 は Word の挙動が *(u)* で、コーパスに 0 件の入力。m14 はテストの補強。
- **ADR-0004 rev 4 を Accepted にしてよいか: yes**
  - 条件は付けない。m12〜m14 は Accepted の後の修正でよい。
  - owner の Word での手動確認（open question 1、`manual_check`）のついでに、次の3点を見ておくことを勧める。どれかが Word で成り立つと分かれば、該当する指摘を Major に上げて直す。
    1. 曲引用符（11 番、既存）。
    2. 組み込みと同名の linked カスタムプロパティを、`DOCPROPERTY` がどちらに解決するか（m12a）。
    3. 文書の中でのリンク貼り付けが自己参照の `LINK` を書くか（m13）。

### 再現手順

- 一式: `<scratchpad>/review-docx-r4/`
  - `new/`（`db53cdf`）、`old/`（`16475fd`）
  - `new/crates/ooxml-docx/src/{difftest4,r3_body,r3_fields,old_body,old_fields}.rs`
  - `new/crates/ooxml-docx/tests/r4.rs`
  - `gatescan.rs`、`scan_old.tsv`、`scan_new.tsv`
  - `mut/run.py`、`mut/out.txt`
  - `scan4.py`
- 走査の差分: `DIFF_DIRS=<POI>:<SDK>:<d4j> cargo test -p ooxml-docx --lib --release difftest4 -- --nocapture`（new/ で）
- ゲート差分: `new/target/release/examples/gatescan <dirs> > scan_new.tsv`、`new/target-old/release/examples/gatescan <dirs> > scan_old.tsv`、`cmp`
- プローブ: `cargo test -p ooxml-docx --release --test r4 -- --nocapture --test-threads 1`
- ミューテーション: `R=<scratchpad>/review-docx-r4 python3 -I mut/run.py [M16 …]`
