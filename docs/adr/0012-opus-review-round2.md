# ADR-0012 レビュー（Opus, round 2）

対象: PR #30 `docs/adr/0012-pptx-phase2-slides-and-shapes.md`（Proposed, revision 2, commit `4766425`）。
照合先: round 1 レビュー（`0012-opus-review-round1.md`）、core ブランチ `core/add-part`（worktree
`core-add-part`、HEAD `31906a9`、作業ツリーはクリーン）の `crates/ooxml-core/src/package/add.rs` と `package.rs`、
同ブランチの ADR-0001 note「registry writes」（draft）、ADR-0005 rev 4（`impl-pptx-phase0-1-copy`）、ADR-0016
round 1（`plan-pptx-phase3`）、vendored `pml.xsd`。
プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-30r2-37/probe/`
（`python3 -I`、zip/XML の読み取りだけ）。コーパスは round 1 と同じ POI `test-data/slideshow`、
`reference/fixtures/pptx`、ADR-0008 の追加コーパス（`pptx-p1/`）。

## 結論（先に）

round 1 の B1 と M1–M4 は解消された。§2 は core ブランチの実 API と一致している。`RegistryPlan::stage_part`、
`copy_relationships`（同じ `Id` のまま、Target を再計算）、`remove_parts`（集合単位、型 allowlist、staged
内容の走査）、`incoming_relationships`、`part_name_in_use` は、どれも ADR に書かれた意味でブランチに実装済み。
ADR-0001 note「registry writes」も core 側に 1 本だけあり、ADR-0012 が求めた条件（型 allowlist、集合での検証、
staged 内容での参照走査、振り直しをしないこと）をすべて含んでいる。

B1 の「拒否時にバイト同一」は、コードで確認すると成り立つ。手順 (1) で `xml_part` を使ってパースすると、
パートは `Raw` から `Parsed` に変わる。しかし `write_to` は、dirty でない `Parsed` パートを `raw_copy_file` で
書く（`package.rs` 695–700 行）。`apply` は、変更のない registry 編集を `continue` で捨てる。そのため、拒否された
後も圧縮バイトまで同一になる。

rev 2 で新しく 1 点だけ実装できない箇所がある（N1）。§4.4 の手順 4「コピーからコメント関係を
`remove_relationship` で外す」は、core が拒否する。どちらも小さな修正で直せる（core への要求を 1 行足すか、
コメント付きスライドの duplicate を拒否する）ので、Minor として扱う。そのほかは文言と境界条件の Minor で、
round 3 は不要と判断する。

## 判定

**Accepted after Minor only: yes.** ただし N1 は受理前に必ず反映すること（「Requested from the core PR」に
1 行足すか、`comments` を §4.4 の許可タイプから外す）。N2–N9 は rev 3 でまとめて直せばよい。

---

## round 1 指摘の確認

| 指摘 | 判定 | 根拠 |
|---|---|---|
| B1 | 解消 | §2「Transaction shape」が clone → `stage_part` → 1 plan → `apply` になった。core の `remove_relationship` と `removal` は `view_bytes`（staged 内容を優先）で `refers_to_rel_id` を判定する。`stage_part` は、plan 内で削除した `Id` を名指す内容を `ConflictsWithPlan` で拒否する。`apply` が失敗するのは `StalePlan` のときだけ。T4 には「core が拒否するケース」が追加された |
| M1 | 解消（ただし N1） | core の `copy_relationships(pkg, src, dst, target_map)` は、`relationship_with_id` で元の `Id` を保ったまま新しい `.rels` に書き、内部ターゲットを `dst` からの相対パスに直す。`#_ftn1` 型の扱いは「Requested from the core PR」に書かれた |
| M2 | 解消 | §4.2 は `incoming_relationships` で一般化された（`SlideIsReferenced { from_part, rel_type }`）。viewProps のアウトライン項目は外す。Slide Zoom や AC/extLst 内の数値 `sldId` は拒否する |
| M3 | 解消 | core ブランチの ADR-0001 に note「registry writes」が 1 本だけあり、interim note を置き換える。ADR-0012 §2 は要件を並べるだけになった |
| M4 | 解消 | §2 の表の名前と引数は `add.rs` の公開 API と一致する（`add_part`、`add_default_content_type`、`add_relationship`、`copy_relationships`、`remove_relationship`、`remove_parts`、`stage_part`、`Package::incoming_relationships`、`part_name_in_use`）。C5 の根拠も「古い参照を新しいパートに解決させない」に直り、core note の表と一致する |
| m1–m13 | 解消 | 改訂履歴の表のとおり。m1（`@spid`、`_x0000_s`、`ac:*Mk@id`、`i32::MAX`）、m2（session high-water）、m3（External と解決できないターゲットは関係だけ）、m6（add/duplicate も `ShowRangeByPosition`）、m7（プレースホルダー規則）、m9（`Notes`/`HiddenSlides`）、m10、m11、m13 を本文で確認した。m4 と m5 は N1 と N3 に残りがある |

---

## 新しい指摘（rev 2）

### N1（受理前に必須）§4.4 手順 4 のコメント関係の除去は core では書けない

**根拠:**

- §4.4 の手順 3 は `copy_relationships(original, new, target_map)` で元スライドの関係をすべて写す。手順 4 では、
  写したうちの `comments` 関係を `remove_relationship` で外すとしている。
- core の `RegistryPlan::remove_relationship` は、`pkg.find_ci(&rels_name)` で、パッケージに既にある `.rels`
  パートを探す（`add.rs` 869 行以降）。複製先の `.rels` は同じ plan で作る新しいパート（`NewContent::Rels`）
  なので、`RelationshipNotFound` になる。doc コメントにも「Refused when it does not exist (or was added in
  this plan)」と書かれている。
- `copy_relationships` の引数は `target_map` だけで、round 1 の M1 で推奨したフィルタ（関係タイプの除外）は
  ない。
- 素直に実装すると、拒否される（コメント付きの 3 スライドで duplicate が `Core(…)` になる）か、コメント関係を
  外さずに残すかのどちらかになる。残した場合、複製と元スライドが同じ legacy `comments` パート、またはモダン
  コメントのパートを共有する。モダンコメントは `pc:sldMk` で元スライドの `sldId`/`creationId` を名指すので、
  共有すると矛盾した状態になる。

**推奨（どちらか一方）:**

- 「Requested from the core PR」に、`copy_relationships` の除外引数を足す。例:
  `skip: &[&str]`（関係タイプ）。「除外した関係は書かない。`Id` は欠番として残る（再利用しない）」と書く。
- または、core が対応するまで `comments` を §4.4 の許可タイプから外す（`UnsupportedSlidePart(comments)`）。

### N2 `remove_parts` に渡す allowed types を列挙する（`slide` が要る）

- core の `removal` は、残るパートの `.rels` にある、集合内を指す関係について、型の allowlist を先に検査する。
  plan 内で既に外した `Id` を飛ばす `continue` は、その後にある。
- そのため、§4.2 の順序（先に `remove_relationship` で presentation → slide を外す）でも、`slide` 型を allowed
  に入れないと `RelationshipTypeNotAllowed` で拒否される。viewProps → slide（アウトライン項目）も `slide` 型で
  ある。
- §6 は「`allowed_rel_types` is that set」としか書いておらず、表の `slide` 行は「relationship only」になって
  いる。これでは読み手が `slide` を除外しかねない。
- `allowed_rel_types` を明示的に列挙する: `slide`、`notesSlide`、`comments`（2 種）、`image`/`media`/`audio`/
  `video`/`hdphoto`、`chart` 系、`diagram*`、`oleObject`、`package`、`vmlDrawing`、`tags`、`themeOverride`。
- あわせて、`remove_relationship` の手順は省けることを書く。`remove_parts` は、staged の `presentation.xml` が
  `Id` を名指さなくなっていれば、その関係を自分で外す。残しても害はない。

### N3 duplicate でノートスライドと tags の関係タイプを検査していない

- §4.4 の許可タイプはスライド本体の関係にしか適用されない。手順 3 の「then the same for the notes copy」は、
  ノートの関係（`image`、`hyperlink`、まれに `chart`/`oleObject`/`tags`）を無条件に共有する。
- ノートの関係も同じ許可タイプで検査する。ノートの `tags` も、スライドの tags と同じく複製する。
- tags パートが自分の `.rels` を持つ場合（まれ）は、`copy_relationships` を連鎖させるか拒否するかを書く。
- 同じ tags パートを複数の `r:id` が指す場合は「異なるターゲットごとに 1 つ複製し、`target_map` に入れる」と
  書く。

### N4 参照チェックが読めない `.rels` を黙って飛ばす（fail-open）

- `Package::incoming_relationships` は、読めない・パースできない関係パートを `continue` で飛ばす（`add.rs`
  1239 行以降）。
- §4.2 の `SlideIsReferenced` 判定もこれに頼るので、壊れた `.rels` からの参照は見えない。最後の安全網は
  `remove_parts` の `mentions`（パート名の文字列を含むなら `RegistryNotEditable`）だけになる。
- ADR-0005 §6 の「読めないものはロック（fail closed）」に合わせ、読めない関係パートがあれば `delete_slide` を
  拒否する（`SlideIsReferenced { from_part: <rels>, rel_type: "?" }` か専用のエラー）と明記する。

### N5 viewProps 編集が allowlist に載っていない

- §4.2 は `p:outlineViewPr/p:sldLst/p:sld` を外して `viewProps.xml` を stage する。しかし §11 の allowlist 追加
  リストに viewProps の要素がない。`[write]`/remove の対象として追加する。
- `p:sldLst` が空になるのはスキーマ上合法（`CT_OutlineViewSlideList` の `sld` は `minOccurs="0"`、
  `pml.xsd` 1610 行）。空の `p:sldLst` は残す、と書く。

### N6 session high-water の持ち主

- §3 の `sldId` の「every id allocated or removed in this session」は、`ooxml-pptx` 側の状態である。core が
  `removed_names` と `removed_rel_ids` を `Package` に持つのとは違う。
- 「session = `Presentation` 値の寿命」と定義する。`into_package()` などで手放して開き直すと失われ、session を
  またぐ再利用と同じ限界になる、と 1 文書く。
- shape id には high-water が不要（Phase 2 にシェイプの削除はない）ことも書いておくと、ADR-0016 以降が迷わない。

### N7 画像の content type の fallback

- §4.5 の「`Default` … or an `Override`」の分岐条件を、core の挙動に合わせて書く。
- 拡張子に既存の `Default` があり、型が一致する場合 → 何も足さない。
- 型が違う場合（例: `bin`）→ `Override`。
- `add_default_content_type` が `WouldRetypePart` で拒否する場合（その拡張子に `Override` のないパートが既に
  ある）→ `Override` に倒し、エラーにしない。

### N8 自分自身へのジャンプリンクの複製

- 元スライドが自分自身へ `slide` 関係を持つ場合（「このスライドを繰り返す」ボタンなど）、`copy_relationships`
  の後、複製はリンク先として元スライドを指したままになる。
- PowerPoint の複製がどちらにするかは未確認。`target_map` に `(original, new)` を入れるかどうかを決めて書く。
  入れない場合は、その旨を Consequences に書く。

### N9 ADR-0016 との整合

- ノート作成の API が二重になっている。§4.6 は `ensure_notes_slide(slide)`、ADR-0016 §8 は
  `notes_mut_or_create(slide)`。ADR-0016 側を「`ensure_notes_slide` がノートマスターも作るよう拡張する」に
  寄せるよう、ADR-0016 round 1 で指摘した。ADR-0012 は名前をこのまま維持すればよい。
- ADR-0016 §8 は `app.xml` の `Notes` に触れていない。ADR-0012 §7 の規則（ノート作成で `Notes` を増やす）が
  Phase 3 の作成経路にも適用される、と §7 に 1 文足す。

---

## Q1–Q5 への推奨

- **Q1–Q4:** round 1 の推奨が既定値として記録されており、変更はない。賛成。
- **Q5（"apply to all" フッター）: 再現しないことに賛成。**
  - 再現するには、`p:hf` と、マスターの `dt`/`ftr`/`sldNum` プレースホルダーの文字列から「すべてに適用」状態を
    推測する必要があり、推測で書き込むことになる。
  - 呼び出し側は、Phase 0 のテキスト API でフッターを明示的に足せる。手動確認で差が見えたら、後のフェーズで
    `add_slide_with_footers` を検討すればよい。
