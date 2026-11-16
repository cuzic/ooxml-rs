# PR #14 レビュー（Opus, round 3）

対象: `core/remove-part` @ `a0e439d`（round 2 への修正）。
照合した文書: round 1・round 2（`docs/reviews/pr14-round1.md`、`pr14-round2.md`）、ADR-0001 の暫定 note、ADR-0003 rev 4（`impl/xlsx-phase1` @ `8331f11`）の §3.1 と §4.3、`crates/ooxml-core/README.md`、OPC の `opc-relationships.xsd`。
プローブ: scratchpad `review-14r3-21/`。round 2 の `probe_r2.rs`（Q1〜Q7、コーパス）を再実行し、新しく `probe_r3.rs`（R1〜R10）を書いた。mutant は `mutants3.py`（28 件）と `mutants3cm.py`（C と M）で動かした。作業コピーは `review-14r3-21/wt` で、worktree には本ファイル以外何も書いていない。

## 結論（先に）

**未収束（Major 1 件）。** round 2 の N1〜N6 は、どれも指摘した形では解消している。

- Q1（%エンコードと生 UTF-8 の Target）と Q2（`_RELS`）は、どちらも関係を消して `Ok` を返すようになった。
- Q3 では、`foo/_rels/.rels` は package の関係として扱われなくなった。
- mutant I、J、Q、H はすべて検出された。
- コーパス（2,094 パッケージ）の結果は round 2 と変わらない。calcChain 279 件はすべて `Ok`、過剰拒否は 0 件、panic は 0 件だった。

残る Major は 1 件である。

- **N7**: 同値性の扱いが Target の比較の 1 か所にしか入っていない。そのため、次の 3 つの経路で `Ok` のまま、ダングリングした `r:id` が残る。
  - (a) `.rels` から**ソース part** を探すとき（R1）。
  - (b) XML の空白を含む Target（`xsd:anyURI` は whiteSpace=collapse）（R5、R10）。
  - (c) 空白を含む Id（R10）。

  N1 と同じ類型（汎用 API が破損したパッケージを `Ok` で返す）なので、round 2 と同じ基準で Major とした。Phase1 の calcChain の経路は、事実上影響を受けない。

ほかは Minor が 4 件、メモがいくつかある。panic と、Err を返したのに状態が変わる経路は見つからなかった。

## 解消状況

| round 2 | 状況 | 根拠 |
|---|---|---|
| **N1** (a) `_RELS` | **解消** | `is_rels_part` は最後のディレクトリを `eq_ignore_ascii_case("_rels")` で判定する（`package.rs:639-643`）。`source_of_rels_part` も大小文字を区別しない。Q2 は `Ok([("xl/_RELS/workbook.xml.rels","rId2")])` を返し、関係は消える。mutant U、X、X2 は `rels_part_names` で検出された。 |
| **N1** (b) %エンコード / IRI | **解消（Target の比較に限る）** | `same_part_name`（`package.rs:699-720`）は、`resolve_target` の後で %デコードし、ASCII の大小文字を畳む。Q1 は 3 形とも関係を消す。mutant V と Q は `part_name_equivalence` で検出された。ただし、part の探索（`find_ci`）とソース part の探索には同値性が入っていない。→ N7(a) |
| **N2** `dir/_rels/.rels` | **解消** | `RelsSource::{Package, Part, Orphan}` を導入した。Q3 では、`main_document_part()` が削除の後も `Some("xl/workbook.xml")` のままである。mutant W は `remove_part_nested_dot_rels_is_not_the_package` で検出された。ただし mutant Z（Orphan の base を `None` にする）は生き残った。→ N9 |
| **N3** ADR の整合 | **解消** | ADR-0001 の note は「ADR-0003 の §4.3 が少なくともこれらの条件（型の allowlist と any-attribute 規則）を含むとき、それがこの note を置き換える」になった。ADR-0003 rev 4 の §4.3 にある引用文は、型の allowlist、全属性・全名前空間、unescape、OPC の同値性を含み、この条件を満たす。残る不一致は文言だけである（N10）。 |
| **N4** mutant I/J/Q/H | **解消** | I と H は `remove_part_guards_replaced_rels_and_dtd_sources` で、J は `remove_part_keeps_external_relationships_with_the_same_target` で、Q は `part_name_equivalence` で検出された。round 2 で使った mutant（A〜S）は T（等価）を除いてすべて検出された。新しく作った mutant のうち、Y と Z は生き残った（N9）。 |
| **N5** 型の定数と doc | **解消** | `opc::REL_CALC_CHAIN` と `REL_CALC_CHAIN_STRICT` を追加した。doc には「完全一致」「空の slice の場合」を書いた。テスト `calc_chain_relationship_types` がある。 |
| **N6** `opc.rs` の doc | **解消** | ADR-0001 の暫定 note を先に挙げる形に直った。 |

## 新しい指摘

### N7 [Major] 同値性が Target の比較にしか入っておらず、`Ok` のまま `r:id` がダングリングする経路が 3 つ残る

**(a) ソース part を同値性で探していない（R1）**
- 形 1: sheet の entry が `xl/sheet%E7%94%BB.xml` で、その `.rels` が `xl/_rels/sheet画.xml.rels`（rId1 → `media/i.png`）。sheet には `<pic r:embed="rId1"/>` がある。
- 形 2: 上と符号化を逆にした組。
- どちらの形でも、`remove_part("xl/media/i.png", &[IMAGE])` は `Ok([(…rels, "rId1")])` を返す。関係は消えるが、sheet の `r:embed="rId1"` は残る。つまり**参照されている関係を消している**。
- 原因: `plan_removal` のソースの探索（`package.rs:465-466`）は `self.find_ci(src)` を使う。この関数は ASCII の大小文字だけを畳み、%デコードはしない。ソースが見つからないと、孤立した `.rels` として扱われ、走査が丸ごと省かれる。
- OPC の ZIP item 名から part 名への写像（Part 2 の、非 ASCII を %エンコードする規則）では、この 2 つの entry は同じ part 名の対になる。N1(b) で Target の側を直した理由が、そのままソースの側にも当てはまる。
- 同じ原因で、自分の `.rels` の検出（`HasOwnRelationships`、`package.rs:393-394`）も外れる（R2）。こちらは孤立した `.rels` が残るだけで、ダングリングは生じない。

**(b) Target の前後の XML 空白（R5、R10）**
- `opc-relationships.xsd` では `Target` は `xsd:anyURI` で、whiteSpace は collapse である。したがって `Target=" media/i.png"` も `Target="media/i.png&#10;"`（属性値の正規化で空白になる）も、スキーマ上は `media/i.png` と同じ値である。.NET の `Uri` も、前後の空白を落として解決する。
- R10: 上の Target を持ち、sheet が `r:embed="rId1"` を持つパッケージで画像を消すと、`Ok([])` が返る。関係は**残り**、`relationships("xl/sheet.xml")` は今は存在しない part を指し続ける。
- R5（calcChain の場合）: `" calcChain.xml"` と `"calcChain.xml "` は、どちらも `Ok(0)` で関係が残る。

**(c) Id の前後の空白（R10）**
- `Id` は `xsd:ID` で、これも collapse される。`Id=" rId1 "` で sheet が `r:embed="rId1"` を持つ場合、`refers_to_rel_id(bytes, " rId1 ")` は一致を見つけられない。そのため `Ok([(…, " rId1 ")])` となり、参照されている関係を消す。

**影響の範囲**
- xlsx Phase1 の経路は、ソースが ASCII の `xl/workbook.xml` で、Target は Excel・LibreOffice・openpyxl のどれも `calcChain.xml` と書く。またコーパスで (a)〜(c) の形は 0 件だった。したがって実害の見込みは低い。
- それでも、doc（`package.rs:355-359`「Part names follow OPC equivalence」「After a successful validation the removal itself cannot fail」）と README は「何も参照していない」ことを保証している。汎用の pub API が `Ok` で破損を残すので、round 2 の N1 と同じ基準で Major とする。

**提案**（どれも局所的な修正）
1. `find_ci` を `same_part_name` で比べる形にするか、ソースの探索と自分の `.rels` の探索だけでもそうする。R1 と R2 はこれで解消する。
2. `Target` と `Id` を比べる前に、XML の空白（`#x20 #x9 #xA #xD`）を前後から trim する（anyURI と ID の collapse）。
3. round 2 で提案した安全網を入れる。走査の後、対象のファイル名（生、%エンコード、%デコードの各形、大小文字を問わない）が `.rels` の**生テキスト**に含まれていて、しかもどの関係とも一致しなかったら `Err` にする。これは拒否する方向にしか外れないので、(b) のように未知の表記ゆれ（`xl\calcChain.xml`、`%2E/calcChain.xml` など）もまとめて止められる。今の `mentions()` を、読めた `.rels` にも使うだけで済む。
4. R1、R10、R5 の空白のケースをテストにする。

### N8 [Minor] %同値な entry 名の組と、CT キャッシュのズレ（R3、R4、mutant Y）

- R3: `open` の重複検査（`package.rs:192`）は ASCII の大小文字だけを見る。そのため、`xl/a画.xml` と `xl/a%E7%94%BB.xml` を両方持つパッケージ（OPC では同じ part 名で、不正）を受け入れる。片方を消すと、`[Content_Types].xml` からは**両方の** Override が消える（XML では `same_part_name`）。残ったほうの part の型は、保存した後は Default の `application/xml` に落ちる。一方、キャッシュは `urn:B` のままで、メモリ上と保存結果が食い違う。
- R4: Override の PartName が `/xl/c%E7%94%BB.xml` で、entry が `xl/c画.xml` の場合。XML の Override は消えるが、`ContentTypes::remove_override`（`opc.rs:67-70`）は `eq_ignore_ascii_case` で比べるので、キャッシュに古いエントリが残る。part はもう存在しないので、実害は小さい。
- mutant Y（CT の比較を `eq_ignore_ascii_case` に戻す）は生き残る。%エンコードした PartName のテストがない。
- 提案: `remove_override` も `same_part_name` で比べる。`open` の重複検査も同じ同値性で比べる（さもなければ R3 は不正な入力として doc に書く）。R4 をテストにする。

### N9 [Minor] テストの穴（mutant Z、Y）

- Z: `RelsSource::Orphan` の base を `None` にしても検出されない。`remove_part_nested_dot_rels_is_not_the_package` は絶対パスの Target（`/x.xml`）しか使っていない。相対の Target（`foo/_rels/.rels` で `Target="x.xml"` → `foo/x.xml`。R8b では正しく動く）のケースを足す。
- Y: N8 に書いたとおり。
- T（check の段階で状態を変える）は round 2 と同じく等価な mutant なので除外した。

### N10 [Minor・文書] 文言の小さなズレ

- ADR-0003 rev 4 に、1 引数の API が残っている（これは PR #20 側の修正になる）。
  - §4.3 の本文（L586 付近）: 「This needs `Package::remove_part(name)` and `Package::check_remove_part(name)`」。
  - §3.1 の apply (i): `remove_part("calcChain.xml")`。
  - 契約の箇条書きは 2 引数になっているので、合わせる。
- `crates/ooxml-core/README.md` の `remove_part` の行は、まだ「Part names are case-insensitive.」だけである。%デコードと、ルートの `_rels/.rels` だけが package の関係である点を、doc（`package.rs:355-359`）に合わせて書き足す。

### メモ（指摘ではない）

- **Target の形**（R5）は、次のとおりすべて期待どおりだった。
  - 一致して消えるもの: `/xl/calcChain.xml`、`./`、`../xl/`、`../../../xl/`（root より上の `..` は RFC 3986 のとおり捨てる）、`#frag`、`?q`、大小文字違い、`calc%63hain.xml`、`worksheets/../calcChain.xml`。
  - 消えないもの: `TargetMode="External"` で同じ名前。`file:calcChain.xml`、空、`#`、`/`、不完全な `%`、`calcChain.xml%23x`（別の名前）。
  - 一致しすぎるもの: `calcChain.xml/` と `//xl/calcChain.xml`（RFC 3986 では authority つきの network-path）。どちらも、対象を正しく指していない関係を消すことになる。ただし、ソースがその Id を参照していれば `StillReferenced` で止まる。ダングリングは生じないので、指摘にはしない。
  - `%2E/` と `..%2F`: OPC は part 名に、非予約文字の %エンコードと `%2F` を禁じている。これらはどの part も指さないとみなせるので、`Ok(0)` は妥当である。N7 の安全網を入れれば拒否に変わる。
- **TargetMode**（R6）: スキーマ違反の値（`external`、` External`）は Internal として扱われ、関係を消す。型の allowlist とソースの走査を通った後なので、安全側である。
- **重複した Id**（R7）: ルートの `.rels` に `rId1` が 2 つある場合、`without_ids` は**両方**をキャッシュから消し、`main_document_part()` がメモリ上で `None` になる（開き直すと戻る）。`Id` は `xsd:ID`（一意）なので入力が不正であり、N2 と同じ「メモリ上だけ」の類型として、メモに留める。キャッシュを Id ではなく位置で更新すれば直る。
- **panic の探索**（R9）: `.rels`、`_rels/.rels.rels`、`é/_rels/.rels`、`_rels/é.rels`、`x/é_rels/a.rels`、`a/_rels/b/.rels`、`_rels/_rels/.rels`、`x/_RELS/.RELS` を、対象として消す場合と、走査の対象になる場合の両方で試した。panic はなく、結果はすべて妥当（`RegistryPart`、`RegistryNotEditable`、または対象外）だった。`source_of_rels_part` の `checked_sub` と `is_char_boundary` は機能している。
- **Err と状態の変化**: plan は `&self` のままで、apply に失敗し得る処理はない。lens で編集した `.rels`、Replaced な `.rels` と CT、DTD を持つソースのいずれでも、Err の後に状態の変化はなかった。

## コーパスの再走査

`PROBE_DIRS`（reference/fixtures、POI/calamine の xlsx、umya、docx/pptx コーパス、`p1/o3in`）を対象にした。

- opened 2,094 件、calcChain を持つもの 279 件、`check_remove_part` が `Ok` のもの 279 件。apply、保存、開き直しまで通り、panic は 0 件だった。
- 全 part の検査は 26,286 件で、`StillReferenced` は 5,989 件、`r:`/`o:` だけを見る規則と比べた余分な拒否は **0 件**だった。round 2 と同じ数字である。
- `cargo test -p ooxml-core` は 47 件すべて通過し、`clippy -D warnings` も通った。

## 判定

**converged: no**。

- Blocker: なし。
- Major: N7 の 1 件。ソース part の探索に同値性がないこと（R1）と、`Target`/`Id` の XML 空白（R10）のために、`Ok` のまま `r:id` がダングリングする。修正は局所的に済む。`find_ci` を `same_part_name` にすること、trim、round 2 で提案した安全網（拒否する方向にしか外れない）、そのテスト。
- Minor: N8（%同値な entry の組と CT キャッシュ）、N9（mutant Y と Z）、N10（ADR-0003 の 1 引数の残りと README）。
- round 2 の N1〜N6 は解消した。N7 を直してプローブ R1 と R10 がテストになれば、収束すると見込む。
