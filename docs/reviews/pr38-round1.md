# PR #38 レビュー（Opus, round 1）

対象: `core/add-part` @ `31906a9`（#14 `core/remove-part` @ `54309a7` の上に積まれている。#14 の round 5 修正 `96af1ff` は**入っていない**）。
照合したもの: `crates/ooxml-core/src/package/add.rs`、`package.rs` の差分、`tests/registry.rs`、README の rules table、ADR-0001 の draft note「registry writes」、#14 round 5 レビュー（N17〜N20）、ADR-0009 rev 2（`plan-xlsx-phase3`）、ADR-0011 rev 2（`plan-docx-phase2`）、ADR-0012 rev 2（`plan-pptx-phase2`）、ADR-0015（`plan-docx-phase3`）、ADR-0010 rev 2（`release_part`）。
リポジトリは変えていない。プローブ、ファジング、mutant はすべて scratchpad の作業コピー（`review-38-27r2/`）で実行した。

- `tests/probe38.rs`: 個別のプローブ（N17 の継承、`stage_part`、`copy_relationships` の view、generation、Default と Override、rId、性能）。
- `tests/fuzz38.rs`: ランダムなパッケージとランダムな plan 列を素朴な参照モデルと比べる（後述）。
- `tests/corpus38.rs`: round 5 と同じ 4,103 ファイル（開けたもの 3,985）で、`remove_part` と `remove_parts` の差分、`copy_relationships`、`incoming_relationships`、add の L1 を確かめる。
- `mutate_author.py`（作者の mutant 27 個を再実行）と `mutate_rev.py`（レビュアーの mutant 36 個）。

## 結論（先に）

**未収束（Major 7 件）。** 設計の骨格は正しい。

- 1 つの plan に、検証済みの step を積む。
- `Err` の step は plan を変えない。
- `apply` は `StalePlan` 以外では失敗しない。
- plan は `Package` を借用しない。

ランダムな plan 列 50,000 本で、不変条件の違反は 0 件だった。素朴なモデルとのずれも 0 件、batch と逐次 apply の差も 0 件だった（後述）。コーパスでの add の L1 違反も 0 件である。

ただし、次の 3 点で「`Ok` ならダングリングしない」「検証した内容が保存される」という中心の保証が破れる。

- **M1**: #14 の N17（二重デコード）を**そのまま継承している**。しかもロジックを `add.rs` に複製しているので、#14 の修正 `96af1ff` をマージしても直らない。
- **M2**: 汚れていない（dirty でない）`XmlDocument` を `stage_part` すると、検証は新しい内容で走るのに、保存されるのは元のバイトになる。
- **M3**: 読めない `.rels` を「言及なし」とみなす。実在のコーパスファイルで、`remove_part` は拒否するのに `remove_parts` は `Ok` を返す。

ほかの Major は次の 4 件である。

- M4: `copy_relationships` が plan の view を読まない。
- M5: `remove_relationship` に型の allowlist がない。ADR-0001 note の条件 (b) と矛盾する。
- M6: `copy_relationships` が、解決できない Internal target を拒否し、fragment を落とす。ADR-0012 の要求を満たさない。
- M7: 新しい `rId` が、source に残るダングリングな `r:id` を拾ってしまう。ADR-0011 の要求を満たさない。

Minor は 10 件ある。

## 確認できたこと（指摘ではない）

| 観点 | 結果 | 根拠 |
|---|---|---|
| 各 step が「パッケージ + それまでの step」に対して検証されるか | おおむね満たす（例外は M2、M4、m4、m5） | fuzz の参照モデル。Numbered 名と rId の予測が 126,151 件の `Ok` ですべて一致した。新しいダングリング関係、関係のない `r:id`、content type の消失・変化はすべて 0 件 |
| `Err` の後に plan が変わらないか | 満たす | `step()` は複製に対して実行する。fuzz では、拒否 47,000 件超（`ConflictsWithPlan` 19,739、`StillReferenced` 8,605、`Remove(…)` 7,768 など）のすべてで、拒否の前後で `format!("{plan:?}")` が一致した |
| batch と逐次 apply の差 | 0 件 | 全 step が `Ok` だった 19,679 本の plan 列で、1 plan で apply した結果と、1 step = 1 plan で逐次 apply した結果を比べた。zip の全エントリがバイト単位で一致した |
| `apply` が失敗しないか | `StalePlan` 以外では失敗しない。ただし m2 の前提がある | `edit.install`、`removes` の降順適用、`set_ct` が先で `removes`/`appends` が後、`parts.remove` が逆順、の順序はどれも正しい |
| Layer 1 のパススルー | 満たす | C4: 3,982 ファイルで、`Default` の追加、Numbered の part、main からの関係を 1 plan で入れた。`[Content_Types].xml` と main の `.rels` 以外の元エントリは、(名前, CRC, 圧縮バイト) がすべて一致し、違反は 0 件だった。走査しただけの registry は Raw のまま残る（`remove_closed_set_with_own_relationships`、A22 は検出される） |
| Package を借用しないか | 満たす | `RegistryPlan` は `package: u64` と `generation: u64` だけを持ち、参照を持たない |
| stale の検出 | 次の 4 つで bump する: `xml_part_mut`（呼んだ時点で）、`set_part`、`remove_part`、`apply`（空の plan のときを除く） | `xml_part`（パースのみ）は bump せず、install の木と同じ内容なので無害。失敗した `xml_part_mut`、`set_part`、`remove_part` は bump しない（プローブで確認）。ADR-0010 rev 2 は `release_part` が bump すると明記している（M4 対応）。別 package の plan は `id` で拒否される（R36） |
| max + 1 と smallest-free のどちらを採るか | **max + 1 が正しい** | 理由は README の「Why」のとおりで、ダングリングな target や削除済みの名前を蘇らせないためである。ADR-0011 rev 2 は smallest-free を撤回し、core の max + 1 に揃えた（§1、Q1）。ADR-0011 に残る「smallest free number」はスタイル ID（§3、M4）の話で、part 名ではない。ADR-0012 §3 と ADR-0009 §2 も max + 1 である。前提: 名前に `u64` の最大値の番号があると、`Exhausted` で以後の Numbered がすべて拒否される（敵対的な入力のみ。注記でよい） |
| ADR-0001 の draft note が #14 の安全条件を保っているか | 主要な 2 条件（型の allowlist、任意の属性で ID を参照していないこと）は保っている。ただし、`remove_relationship` で破られる（M5）。ほかの条件の明記も欠けている（m8） | |

## 指摘

### M1 [Major] #14 N17 の二重デコードを継承している。ロジックが `add.rs` に複製されているので、#14 の修正では直らない

**根拠**（`add.rs:1215-1221` の `relationship_targets`、`1241-1258` の `incoming_relationships`、`1391-1449` の `removal` のループ、`1350` の planned 関係。どれも `source_of_rels_part(&normalize_part_name(..))` の結果をもう一度 `find_ci`/`same_part_name`/`resolve_target` に渡している。`probe38.rs::p_n17`）

- source が `xl/a%2541.xml`（OPC として正しい名前）で、`<pic r:embed="rId1"/>` から `media/i.png` を参照している。
  - `remove_parts(["xl/media/i.png"], [image])` は **`Ok`** を返し、`.rels` から `rId1` を消す。保存後も source には `r:embed="rId1"` が残る。
  - 同じ構成で source が `xl/plain.xml` なら `StillReferenced` になる。
- `incoming_relationships` の `source` は `"xl/a%41.xml"` と報告される。これは実在しない名前である。
- base の二重デコード（フォルダが `x%2541/`）:
  - おとりがない場合: `UnresolvedMention` で拒否される（安全な側に外れる）。
  - **おとりの part `xa/media/i.png` がある場合**:
    - `remove_parts(["x%2541/media/i.png"])` は `Ok(relationships: [])` を返す。関係は残り、ダングリングする。
    - 続けて `remove_parts(["xa/media/i.png"])` を呼ぶと、**別の part を指している `x%2541/_rels/s.xml.rels` の `rId1` を消す**。
  - round 5 ではコードから導いただけだったが、今回プローブで確かめた。
- #14 側の修正（`96af1ff`）は、`package.rs::plan_removal` だけを直している。`add.rs` には同じ判定が 4 か所に独立してあり、マージ後もこちらは古いままになる。

**提案**:
- 「`.rels` part の列挙 → (source, base, 解決済みの target)」を `package.rs` の 1 関数（raw 名から source と base を切り出す、`96af1ff` の版）にまとめる。`removal`、`relationship_targets`、`incoming_relationships`、`planned_relationships` の 4 か所はそれを呼ぶ。
- #14 を先にマージし、#38 は origin/main をマージして追従する。
- 回帰テスト: `xl/a%2541.xml` が `StillReferenced` になること。おとりがある場合に、正しい関係だけが消えること。

### M2 [Major] dirty でない `XmlDocument` を `stage_part` すると、検証したのとは違うバイトが保存される

**根拠**（`apply` の `PartState::Parsed(d)`。`read_part`/`write_to` は `!d.is_dirty() && !part.added` のとき**元のバイト**を書く。`view_bytes` は `d.serialize()` を読む。`probe38.rs::p_stage_fresh_doc`）

- `stage_part("xl/s.xml", XmlDocument::parse(r:embed を含まない新しいバイト))` の後に `remove_parts(["xl/media/i.png"])` を呼んだ。
  - plan の view では参照がないので `Ok` になり、`rId1` を消す。
  - 保存後の `xl/s.xml` は**元のまま `r:embed="rId1"`** で、`.rels` は空になる。
  - `apply` 後の `read_part` も元のバイトを返す。
- doc には「clone して lens で編集」と書いてあり、その場合は dirty なので正しく動く。しかし、`PartContent::Xml` を受け取る API である以上、テンプレートから組み立てた木（ADR-0009/0012/0015 の `app.xml`、`people.xml`、新しい `comments.xml` の雛形など）を既存の part に stage する使い方は自然である。その場合、黙って元に戻る。
- `add_part` の場合は `added = true` なので問題ない。既存 part だけで起きる。

**提案**: `staged` を apply するときに `PartState::Replaced(d.serialize())` にするか、`Part` に「元のエントリとは無関係」の印（`added` と同じ扱い）を付ける。clone した木の span による byte 局所性を保ちたいなら、後者で `serialize()` を使えばよい（`serialize` は dirty でない部分木を verbatim に書く）。テストは、`XmlDocument::parse(other)` を stage して保存し、その内容が出ることを確かめる。

### M3 [Major] 読めない `.rels`（`read_part` が I/O エラー）を「言及なし」とみなし、`remove_parts` が `Ok` を返す。`remove_part` は拒否する

**根拠**（`add.rs:1384`: `pkg.read_part(..).unwrap_or_default()`。#14 の `package.rs` は `.unwrap_or(true)` で、読めなければ言及ありとみなす。`corpus38.rs` の C1）

- `xlsxfx/calam/invalid_relationship_errors.xlsx` の `xl/_rels/workbook.xml.rels` は、zip のエントリが壊れている（「File is larger than its declared uncompressed size」）。
- `xl/theme/theme1.xml`、`xl/sharedStrings.xml`、`xl/styles.xml` について:
  - `check_remove_part` は `RegistryNotEditable` で拒否する。
  - `check_remove_parts` は **`Ok`** を返す。
  - 読めない `.rels` が実際にこの 3 part を指しているので、削除すると関係がダングリングする。
- コーパスで、2 つの実装の判定が食い違ったのはこの 3 件だけである。ほかの差は、エラー種別の表記と、元から壊れた zip（チェックサム）だけだった。

**提案**: `unwrap_or(true)` に揃える（読めなければ言及ありとして拒否する）。根本的には m7 のとおり、`remove_part` を `remove_parts` の 1 要素版にして、判定の実装を 1 つにする。

### M4 [Major] `copy_relationships` が plan の view ではなくパッケージを読み、黙って関係を落とす

**根拠**（`add.rs:1555-1563`: `pkg.find_ci(&src_rels)` と `pkg.read_part`。plan の `edits[..].appends` と `NewContent::Rels` を見ない。`probe38.rs::p_copy_view`）

- (a) src が、この plan で追加した part で、plan で関係を 1 本足してある場合: `copy_relationships(src, dst)` は **`Ok([])`** を返す。
- (b) src が既存の part で、plan で External の関係 `rId2` を足した場合: **`Ok(["rId1"])`** を返し、`rId2` を落とす。
- どちらも `Err` にならないので、複製した XML の `r:id` は `apply` 後にダングリングする。ADR-0012 §4.4 の duplicate（新しいノートスライドの複製に、その関係をさらに複製する）は、(a) の形を使う。

**提案**: src の関係の集合を「パッケージの `.rels` − `removed_ids` + plan で追加した関係（`planned_relationships` のうち src のもの）」として作る。テストは (a) と (b) の 2 形。

### M5 [Major] `remove_relationship` に型の allowlist がない。ADR-0001 draft note (b) の条件を満たさない

**根拠**（`add.rs:871-910`。`probe38.rs::p_remove_rel_implicit`）

- note (b) は、「relationship を on its own で外すのは、型が呼び出し側の allowlist にあるときだけ」（parts found by type are referenced implicitly）としている。
- ところが `remove_relationship(source, id)` は、source の属性に `Id` が残っていないことしか見ない。
- `fixture_minimal.xlsx` で `workbook.xml → styles`（`rId2`）と `→ sharedStrings`（`rId3`）を外すと、どちらも **`Ok`** になる。workbook の XML には `r:id` がないので、型で見つける暗黙の参照が保護されない。Excel は、styles や sharedStrings を失ったブックを修復ダイアログで開く。
- README の rules table の「Changing entries」は、「removing a relationship nobody refers to」とだけ書いていて、note とずれている。

**提案**: `remove_relationship(pkg, source, id, allowed_rel_types)` にする（`remove_parts` と同じ規則）。または note と table を「属性による参照だけを見る。暗黙の参照は呼び出し側の責任」に改める。ADR-0012 §2 の要求（「the caller must allow each removed relationship's type」）と合うのは前者である。

### M6 [Major] `copy_relationships` が、解決できない Internal target を拒否し、fragment を落とす（ADR-0012 §2 の要求が未対応）

**根拠**（`add.rs:1571-1576`: `resolve_target` で `#`/`?` 以降を捨て、`Target::Part` として `relationship()` に渡す。`p_copy_view` (c)、コーパス C2）

- コーパスで、`.rels` を持つ part 17,935 件をそれぞれ新しい part に複製した。
  - **`TargetNotFound` 69 件**: `#_ftn1` → `"ppt/slides"`、ダングリングな `slide16.xml`、`NULL` など。
  - **`InvalidTarget` 4 件**: docxtemplater や POI の、空白などを含む target。
  - `RegistryNotEditable` 2 件（壊れた `.rels`）。
- ADR-0012 は「resolve しない Internal target は **verbatim に**複製する」ことを明示的に求めている（`with_japanese.pptx` の 4 件）。
- `media/i.png#frag` は `Ok` で複製されるが、target は `media/i.png` に**書き換わり、fragment が失われる**。
- 同じ原因で、`RemovedParts.kept_targets` に `#Sheet1!A1` から作った偽の part `"xl/drawings"` が入る（ADR-0009 round 2 のプローブ P3b）。

**提案**:
- `copy_relationships` は、元の `Target` 文字列を相対で書き直せる場合（解決先が part のとき）だけ書き直す。それ以外（解決しない、`#` で始まる、fragment 付き、URI として不正）は、**元の文字列を verbatim に**コピーする。dst が src と同じフォルダでない場合は、相対の dangling target の意味が変わるので、そのときは拒否する。
- `kept_targets` と `incoming_relationships` は、fragment だけの target を Internal の part として扱わない。

### M7 [Major] 新しい `rId` が、source に残るダングリングな `r:*` 参照を拾う（ADR-0011 §1 要求 4、M3）

**根拠**（`next_relationship_id` は `.rels` と plan と session の `Id` しか見ない。`probe38.rs::p_rid_refs`）

- source の XML に `r:id="rId2"`（対応する関係がない）がある。
- 1 本目の `add_relationship` は `rId1`、**2 本目は `rId2`** になる。
- 既存のダングリング参照が、新しい無関係な関係に解決されてしまう。
- ADR-0011 は「new values never adopt a dangling reference」と、`r:`-namespace 属性の値を used とみなすことを core の rules table に求めている。Word のコーパスでは 0 件だが、ADR-0012（pptx）や ADR-0009（xlsx）でも同じ危険がある。README の「Why」は「gaps may be relationships another tool removed while a stale `r:id` survived」と書いていて、ちょうどこの場合である。

**提案**: source（staged の内容があれば、その view）の全属性のうち、`R_NS` の属性と、rId の形をした値（VML の `o:relid` を含む）を、ID の集合に加える。`refers_to_rel_id` と同じ走査で足りる。読めない source は、`SourceNotScannable` で拒否するか、無視するかを table に明記する。

### m1 [Minor・性能] `step()` が、staged の木を含む plan 全体を step ごとに clone する

- **根拠**（`p_perf_staged_clone`、release）: 100,000 行の sheet の木を 1 つ stage した後、`add_relationship` を 50 回呼ぶと **6.75 s**（1 step 約 135 ms）かかる。stage しない場合は 2.3 ms である。
- ADR-0009（rename で、数式を書き換えた sheet をすべて stage する）、ADR-0015（`document.xml` と `settings.xml` を stage する）では、これが実際に起きる。`copy_relationships` は内部でもう一度 clone する。
- **提案**: `staged` を `Arc<PartContent>` にするか、undo log にする（失敗した step で追記を巻き戻す）。

### m2 [Minor] stale 検出と「apply は失敗しない」が、bump の漏れに弱い

- `apply` は、`install == None` の edit で part が Parsed でないとき、`debug_assert!(false); continue` で**黙って edit を捨てる**。release ビルドでは、new part が登録されないまま保存される。
- 今は、すべての `&mut self` 経路が bump する（ADR-0010 の `release_part` も bump する予定）。しかし、将来の `&mut` API が 1 つ bump し忘れるだけで、この経路に入る。
- `xml_part_mut` は、どの part でも plan を stale にする。これは粗い（ADR-0009 round 2 の R2-M2 で、Phase1 の cached value の削除がこれに当たる）。
- `parse_raw` は `XmlDocument::parse`（既定のオプション）を使い、`pkg.parse_options` を使わない。
- **提案**:
  - (a) 状態を変える処理を `fn touch(&mut self)` 経由に一本化し、テストで全 `pub fn (&mut self)` を列挙する。
  - (b) `apply` は、`edit.part` の状態が想定と違ったら install を使い直す（Raw に戻っていても、plan が持つ木で上書きできる）。
  - (c) 粗い stale 判定は、doc の「Typical use」に、lens 編集はすべて clone して stage する、と明記する。

### m3 [Minor] 同じ plan で削除した名前を `Exact` で作り直すと拒否されるが、次の plan では通る

- `check_free` の `is_gone` は、plan 内では `NameInUse` を返す。ところが、`removed_names` は Numbered でしか使われないので、`apply` した後の plan では `Exact` で同じ名前を作り直せる。
- ADR-0009 の VBA project の差し替え（同じ plan での remove + add）が通らない（ADR-0009 round 2 の R2-m1）。max + 1 の理由（削除した名前を蘇らせない）とも、セッションをまたぐと一貫しない。
- どちらかに揃え、table に明記する。案は 2 つある。
  - plan 内の remove + `Exact` を許す。`Override` は付け直す。
  - session 中は `Exact` も拒否する。

### m4 [Minor] `add_default_content_type` が、同じ plan で削除する part を見て `WouldRetypePart` で拒否する

- 根拠: `xl/a.foo` だけを持つパッケージで、`remove_parts(["xl/a.foo"])` の後に `add_default_content_type("foo", ..)` を呼ぶと、`WouldRetypePart { part: "xl/a.foo" }` になる（`p_ct`）。
- 安全な側に外れているが、view の原則（後の step は前の step を見る）に反する。`names` から `self.gone` を除く。

### m5 [Minor] plan で追加した part に対する `set_content_type`、`remove_parts`、`remove_relationship` が `PartNotFound` になる

- doc には書いてある。しかし、ADR-0012 の duplicate や ADR-0015 の新しい `comments.xml` で、作った part の型を後で変えることはない。`stage_part` は pending の part を受け付けるので、API の非対称が目立つ。
- 少なくとも README の table で「plan 内で追加した part に対して使える step」を列挙する。

### m6 [Minor] `incoming_relationships` は Package だけを見て、読めない `.rels` を黙って飛ばす

- `p_incoming` では、壊れた `.rels` が画像を指していても `[]` を返す（`remove_parts` の方は拒否するので、全体としては安全な側に外れる）。
- ADR-0012 §6 は、これを「orphan かどうか」の判定に使う。読めない `.rels` があることを返す（`Result` にする、または `unknown: Vec<String>` を付ける）べきである。
- また、plan で追加・削除した関係を反映しないので、`RegistryPlan::incoming_relationships(&pkg, name)` 版を用意する。
- コーパスの C3（総当たりとの比較）では、`123233_charts.xlsx` で `incoming_relationships` の方が正しかった（`Sheet1.xml` と `sheet1.xml.rels` の大小文字の違い。既存の `Package::relationships` が大小文字を区別して引くため）。#38 の不具合ではない。

### m7 [Minor] `remove_part`（#14）と `remove_parts`（#38）が別実装で、すでに食い違っている

- 実際の食い違いは M3 の 1 件で、N17 の修正もどちらか一方にしか入らない（M1）。
- ADR-0001 の note は、「accepted なら `remove_part` は `remove_parts` の 1 part 版になる」と書いている。それを今やる。`RemovedPart` への変換だけを残し、判定は 1 つにする。

### m8 [Minor・文書] ADR-0001 の draft note の文言

- 「A new relationship `Id` … is never one that existed in the package」は言い過ぎである。実装は、ファイル内の現存の ID とセッション中の削除だけを見ていて、保存前のファイルにあった ID は分からない。「in the package or removed in this session」にする。
- #14 で積み上げた条件のうち、次の 3 つが note に書かれていない。どれも安全性の一部なので、1 行ずつでよいから入れる。
  - OPC の名前の同値（ASCII の大小文字、%デコード）
  - `UnresolvedMention` の安全網
  - 読めない `.rels` が言及していれば拒否すること
- 「`incoming_relationships(part)` answers "is it an orphan now"」は、m6 のとおり正確ではない。

### m9 [Minor・テスト] mutant の生存

作者の mutant 27 個を再実行した結果（debug、作者のテストだけ）:

- 検出 21 個。A13 は、パターンが 2 か所に一致するようになったため（`stage_part` にも同じ検査が入った）適用できなかった。
- **生存 4 個**:
  - A3（rId = max、+1 なし）: 実質的に等価な mutant である。max は必ず使用中なので、ループが max + 1 まで進む。
  - **A4**: 大小文字だけが違う ID の衝突（`RID2` の後に `rId2` を割り当てる）を確かめるテストがない。
  - **A11**: 同じ plan で削除した名前を Numbered が避けることを、確かめていない。
  - **A16**: set 内の source が、参照の走査から外れることを確かめていない。
- 作者のコミット `31906a9` が検出したと書いている B3 は、確かに検出された。

レビュアーの mutant 36 個の結果（release、作者のテストだけ。生き残ったものは、さらに fuzz38 を加えて再判定した）:

- 作者のテストだけで検出: R4、R8、R9、R11、R12、R18、R24、R25、R30、R33（10 個）。
- fuzz38 を加えると、さらに R7、R10、R15 の 3 個が検出される。
- **両方とも生存したもの（23 個）**:
  - R1: view が、plan で追加した part の内容を読まない。
  - R2: staged の part を削除できてしまう。
  - R3: 同じ plan で外した ID の二重削除。
  - R5: plan で外した ID の rId の再利用。
  - R6: `check_free` が `is_gone` を見ない。
  - R13: `add_default` が plan で追加した part を見ない。
  - R14: `content_type` の view が削除を見ない。
  - R16: 削除する source の新しい `.rels` を見落とす。
  - **R17、R19、R20**: `apply`、`set_part`、`remove_part` の generation の bump を外す。stale 検出のテストは `xml_part_mut` の 1 経路しかない。
  - R21: `remove_part` が削除した名前を Numbered が避ける。
  - R22: `copy_relationships` が、plan で外した関係を複製しない。
  - R23: `incoming_relationships` が OPC の同値で比べる。
  - R26: `rId` の接頭辞の大小文字。
  - R27: 自己終了の root に子を足したときの `own_dirty`。
  - **R28**: 同じ part を 2 回 stage すると、view は古い方を読み、`apply` は新しい方を書く。
  - R29: 生成した target の往復検査。
  - R31: 前の `remove_parts` で消えた `.rels` を再び走査する。
  - R32: 削除する part 自身の `.rels` の `Override`。
  - R34: `AlreadyPlanned`。
  - R35: 重複した `Override` のすべてを書き換える。
  - **R36**: 別 package の plan を拒否する。
- 太字は stale 検出と view の一貫性に関わるもので、M2、M4 と同じ種類の退行を防ぐテストがない。R17、R19、R20、R36 は `stale_plans_are_refused` を 4 経路に広げれば検出できる。R1、R28 は `view_bytes` のテスト 2 本で検出できる。

提案: 生存した mutant をそれぞれ検出するテストを足す。R3 と R5 は、1 つの plan で remove → add を行うテストで、R13 と R14 は m4 のテストで検出できる。あわせて、fuzz38 のようなモデルとの比較テストを、`#[ignore]` か小さい反復回数で取り込む。

### m10 [Minor] fragment と target の解決

- `resolve_target` は `#`/`?` を捨てる。そのため、`Target="#_ftn1"` は source のフォルダ（`ppt/slides`）に解決される。
- `kept_targets`、`incoming_relationships`、`relationship_targets`（名前が使われているかの判定）が、フォルダ名を part 名のように扱う。
- 実害は、M6 と、`kept_targets` の偽の項目に限られる。fragment だけの target は、Internal であっても part を指さないものとして除く。

## 4 つの ADR の要求に対する充足状況（ギャップ一覧）

| 要求 | 出典 | 状況 |
|---|---|---|
| 1 plan で stage、add、remove、apply は失敗しない | ADR-0009 §2、ADR-0011 §1、ADR-0012 §2、ADR-0015 §1 の 1〜3 | 満たす（M2、M4 を除く） |
| 存在しない part の編集は初期の木として表す | ADR-0015 §1 の 4 | 満たす（`PartContent::Xml`） |
| 参照の走査は staged の内容を読む | ADR-0012 §2 | 満たす |
| closed-set の削除。staged の part から set への参照 | ADR-0009、ADR-0012 §6 | 満たす（staged の source は view で走査する。staged の part 自身を set に入れると `ConflictsWithPlan` になり、ADR-0009 round 2 の R2-m2 で不便が指摘されている） |
| 型の allowlist による暗黙の参照の保護 | ADR-0001 note、ADR-0012 §2 | **`remove_relationship` で未対応（M5）** |
| `copy_relationships`: 同じ ID、target の相対化、`target_map` | ADR-0012 §4.4 | 満たす（ただし M4） |
| `copy_relationships`: 解決しない target を verbatim に複製する | ADR-0012 §2「Requested」 | **未対応（M6）**。コーパスで 69 + 4 件が拒否される |
| 新しい rId が `r:*` のダングリング参照を避ける | ADR-0011 §1 の 4 | **未対応（M7）** |
| 既存の関係の target だけを書き換える（`#Sheet!A1` の hyperlink を rename に追従させる） | ADR-0009 rev 2（round 2 の R2-M1） | **API がない**。`.rels` への `stage_part` は `RegistryPart`、`add_relationship` は `#` を拒否する。`set_relationship_target(source, id, target)` を、Changing entries の (c) として足すかどうかを決める |
| `incoming_relationships` の完全性 | ADR-0012 §4.2、§6 | Package だけを見て、読めない `.rels` を黙って飛ばす（m6）。二重デコード（M1） |
| 作成した要素の局所的な名前空間宣言（`declare_namespace`） | ADR-0011 §1「What else core needs」の 1、ADR-0015 §3.3 | #38 の範囲外（README の「Not yet」）。lens 側の別 PR として追跡が必要である。registry の新しい要素は、root の名前空間と prefix を継ぐので問題ない |
| Word は `Default` を足さない、`Override` で登録する | ADR-0011 §1 の 2 | 満たす |
| `Exact` から `Numbered` へのフォールバック | ADR-0011 §1 の 3 | 満たす（呼び出し側で 2 回呼ぶ） |
| plan 内での VBA の差し替え（remove + 同名の add） | ADR-0009 rev 2 | 不可（m3） |

## ランダム化した差分テストの内容（`fuzz38.rs`）

- **生成器**: `p/s{n}.xml`、`p/S{n}.xml`、`m/i{n}.png` を 2〜6 個作る。関係の target は、ダングリング、大文字の相対 target、暗黙の型（`urn:t:implicit`、`r:id` で参照しないもの）、External を混ぜる。ID は `rIdN` と `R{k}x` を混ぜる。ほかに、ダングリングな `r:id` 参照、残った `Override`、`Default png` の有無も混ぜる。
- **操作**: `add_part`（`Exact`、`Numbered`）、`add_relationship`（plan で追加した part との間も含む）、`remove_relationship`、`remove_parts`（1〜2 part）、`stage_part`（`r:id` を 1 つ減らした内容）、`set_content_type` を、1〜7 step ランダムに並べる。
- **検査**:
  1. 拒否された step の前後で、plan が一致する。
  2. 保存したものを開き直せる。
  3. 新しいダングリング関係がない。
  4. 関係のない `r:id` が新しく生じていない。
  5. 各 part の content type が、モデルの期待どおり（元の型、`set_content_type` の型、追加時の型）である。
  6. stage しなかった元の part が、バイト単位で同一である。
  7. Numbered の名前が、素朴なモデルの max + 1 と一致する。素朴なモデルの「見たことのある名前」は、元の part、`Override`、解決済みの target、追加した名前である。
  8. 新しい rId が、モデルの max + 1 と一致する。モデルは、source ごとに見たことのある ID の集合を持つ。
  9. 全 step が `Ok` の plan 列で、batch と逐次の出力が一致する。
- **結果**: 50,000 本、`Ok` の step 126,151 件、比較 19,679 本で、**違反 0 件**。
- **範囲の限界**: %エンコードした名前（M1）と、dirty でない `Xml` の stage（M2）は生成しないので、そちらはプローブで見た。

## 判定

**converged: no / Accepted after Minor only: no**

- Blocker: なし。
- Major:
  - M1: N17 の継承と、ロジックの複製。
  - M2: dirty でない `Xml` の stage が保存されない。
  - M3: 読めない `.rels` で fail-open する。
  - M4: `copy_relationships` が view を読まない。
  - M5: `remove_relationship` に型の allowlist がない。
  - M6: `copy_relationships` が、解決しない target と fragment を扱えない。
  - M7: 新しい rId がダングリングな `r:*` を拾う。
- Minor: m1〜m10。

M1、M3、m7 は、`.rels` の列挙と削除の判定を #14 の修正済みの 1 実装にまとめれば、まとめて片付く。M2 と M4 はそれぞれ数行の修正で直る。M5 と M7 は、rules table と note の文言とあわせて決める。M6 は ADR-0012 から明示的に頼まれている。これら 7 件を直し、それぞれに回帰テストを足せば、残りが Minor だけでも次の round で収束と判定してよい。
