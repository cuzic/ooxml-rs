# PR #38 レビュー（Opus, round 2）

対象: `core/add-part` @ `252dc8e`。round 1 の修正 `958c8ac` と、mutant を潰す `252dc8e` を見た。
照合したもの: `add.rs`、`package.rs`、`tests/registry.rs`、`tests/registry_review1.rs`、README の rules table、ADR-0001 の note「registry writes」、round 1 レビュー（`pr38-round1.md`）、作者が手を入れた fuzz と corpus ハーネス（`scratchpad/fz38/repo/…/{fuzz38,corpus38}.rs`）。

リポジトリは変えていない。作業はすべて scratchpad の `review-38r2-27r3/` で行い、`git clone` したコピーを使った。

- `repo/`: `252dc8e` に origin/main（#14 マージ済み）をマージしたもの。衝突は手で解消した（後述 n6）。fuzz、corpus、#14 round 6 の sweep はここで回した。
- `probe/`: プローブ用。`tests/probe38r2.rs`（P1〜P6）と、fuzz のモデル変更を戻した版 `tests/fuzzx.rs`。
- `mut/`: mutant 用。レビュアーのテストを除いた状態で、`mutate_r2.py` の新しい mutant 37 個を流した。

## 結論（先に）

**未収束（Major 2 件）。** round 1 の M1〜M7 と m1〜m10 は、すべて解消した（下表）。main とマージしても、#14 のテストと round 6 の corpus sweep は通る。sweep では Ok が 23,291 件で、ダングリングは 0、check と remove の差は 0 だった。fuzz は 50,000 本で違反 0 件、corpus での add/copy/incoming も問題はなかった。

ただし、round 1 の 2 つの修正に、それぞれ穴が 1 つずつある。

- **N1（M2 の修正をすり抜けられる）**: 「編集していない clone」は、`!d.is_dirty() && read_part(現在の内容) == d.source()` で判定している。ところが、detach しないときに保存されるのは、**元の zip エントリ**である。
  - `set_part` や `xml_part_mut` で変わった part を `read_part` からパースし直して stage すると、判定は「編集していない」になる。
  - その結果、検証は新しい内容に対して行われ、保存されるのは元の内容になる。
  - 実例（P1）: `r:embed` がダングリングして残る。
- **N2（新しい `set_relationship_target` を `remove_parts` が見ない）**: `removal()` のループは、edit の木の `Target` をそのまま読み、`set_attrs` を当てていない。
  - 同じ plan で `rId1` の target を a から b に変えた後、b を削除すると `Ok` になり、保存後に `rId1 → b` がダングリングする（P2、P2d）。
  - 逆に a を削除すると、書き換えたばかりの関係を黙って消す（P2c）か、誤って拒否する（P2b）。

どちらも数行で直り、回帰テストは各 1〜2 本で足りる。ほかは Minor 6 件である。

## 解消状況（round 1 の指摘）

| 指摘 | 状態 | 確認内容 |
|---|---|---|
| M1 二重デコード、ロジックの複製 | **解消** | `.rels` の名前から source と base を出す処理を、`package.rs` の `source_of_rels_part` と `rels_kind` の 1 か所にまとめた。`view_rels` と `removal` はそれを使う。テストは `m1_percent_encoded_source_is_scanned` と `m1_base_folder_is_decoded_once`（おとりの part がある場合も含む）。main（#14 の N17〜N21 修正済み）とマージしても、#14 側の `tests/package.rs` はすべて通る |
| M2 dirty でない木を stage すると保存されない | **部分的に解消 → N1** | 新しく作った木は `detached` になり、保存される（`m2_staged_fresh_tree_is_saved`）。ただし「編集していない clone」の判定をすり抜けられる |
| M3 読めない `.rels` で fail-open する | **解消** | `mentions()` は、読めない part を「言及あり」とみなす。corpus（`invalid_relationship_errors.xlsx`）で、`remove_parts` が `RegistryNotEditable`、`incoming_relationships` が `Err` になった（C3 の 7 件）。`check_remove_part` と `check_remove_parts` の差は 0 になった |
| M4 `copy_relationships` が view を読まない | **解消** | `view_rels_of` と、plan で作った `NewContent::Rels` を読む（`m4_copy_reads_the_plans_view`） |
| M5 `remove_relationship` に allowlist がない | **解消** | `allowed_rel_types` 引数を足した。plan で追加した関係も外せる |
| M6 解決しない target と fragment | **解消** | 同じフォルダの中なら verbatim に複製し、フォルダが違えば拒否する。fragment は保つ。`skip_types` も足した。corpus C2 では `copy Ok 18,441` で、拒否は 0 件だった（round 1 では 69 + 4 件） |
| M7 ダングリングな `r:id` を拾う | **解消** | source の view にある全属性の `rId<digits>` を used に加える。XML の source が解析できなければ `SourceNotScannable` になる |
| m1 step ごとの clone | 解消 | `Arc<PartContent>` と `Arc<XmlDocument>` を使う（`steps_do_not_clone_staged_trees`） |
| m2 bump の漏れ、apply の黙った破棄 | 解消 | `touch()` に一本化した。apply は、計画に使った木をそのまま install する。parse options も揃えた。stale の経路は 6 つテストされている。下の「stale」節も見よ |
| m3 同じ plan での remove + `Exact` | 解消（仕様を変更） | `Exact` は削除済みの名前を再利用できる。`Numbered` は再利用しない。ただし n4 |
| m4 Default が削除を見ない | 解消 | `defaults_see_the_plans_view` |
| m5 plan で追加した part への操作 | 解消（一部） | `set_content_type` は通る。`remove_parts` は今も `PartNotFound` だが、README に明記された |
| m6 `incoming_relationships` の完全性 | 解消 | `Result` を返す。plan の view 版もある |
| m7 2 つの実装 | 解消 | `plan_removal` は `remove_parts` の 1 要素版になった。corpus C1 で、ラベル表記の差を除けば判定の差は 1 件だけで、それは n5（大小文字の違う `.rels`）である |
| m8 note の文言 | 解消 | 「in this session or plan」に直した。#14 の条件 3 つを足した。fail closed も明記した |
| m9 生き残った mutant | 解消 | round 1 の生存 mutant に対応するテストが入った。到達しないコードの削除は下で判定する |
| m10 fragment だけの target | 解消 | `part_target()` は `#…` に対して `None` を返す |

## Major

### N1 [Major] 「編集していない clone」の判定は**現在の**内容と比べるのに、保存するのは**元の**エントリである

**根拠**（`add.rs:1897-1901`、`package.rs:308`/`537`、`probe38r2.rs::p_m2_spoof_after_set_part`）

```rust
PartContent::Xml(d) => {
    d.is_dirty() || pkg.read_part(&pkg.parts[i].name).map_or(true, |cur| cur != d.source().as_bytes())
}
```

- `read_part` は、part が `Replaced` ならその新しいバイトを返し、dirty な `Parsed` ならシリアライズした結果を返す。
- `detach == false` で apply すると、`Parsed(d)` になる。`d` は dirty でなく、`detached` も false である。そのため、`write_to` と `read_part` は **`read_original`（元の zip エントリ）** を書く。
- P1 の手順:
  1. `set_part("xl/s.xml", <s/>)`（`r:embed` なし）。
  2. 新しい plan を作り、`XmlDocument::parse(&pkg.read_part("xl/s.xml"))` を stage する。
  3. `remove_parts(["xl/media/i.png"], [IMG])` は **`Ok`** を返し、`rId1` を消す。
  4. apply すると、`read_part` も保存結果も **元の `<pic r:embed="rId1"/>`** になり、`.rels` は空になる。保存したファイルで `r:embed` がダングリングする。
- `xml_part_mut` で編集した part（dirty な Parsed）を `read_part` からパースし直した場合も、同じ経路を通る。この場合は、`xml_part_mut` での編集が黙って消える。
- 「どう判定しているか」への答え: dirty でなく、`source()` のバイト列が `read_part` と一致すれば、「編集していない clone」とみなしている。part が元のエントリのままかどうかは見ていない。
- 「すり抜けられるか」への答え: すり抜けられる。part が同じセッションで `set_part` や `xml_part_mut` を受けていれば、`read_part` をパースし直すだけで足りる。
- ADR-0009 の Phase3 は、編集した clone（dirty）しか stage しないので当たらない。しかし、Phase1/2 は同じセッションで `xml_part_mut` を使う。

**提案**: 比べる相手を元のエントリにする。part が手つかずであることも条件にする。

```rust
let pristine = match &pkg.parts[i].state {
    PartState::Raw => true,
    PartState::Parsed(cur) => !cur.is_dirty() && !pkg.parts[i].detached,
    PartState::Replaced(_) => false,
};
let detach = d.is_dirty() || !pristine || pkg.read_original(&pkg.parts[i]).map_or(true, |o| o != d.source().as_bytes());
```

テストは 2 本。`set_part` の後でパースし直して stage する場合と、`xml_part_mut` の後でパースし直して stage する場合で、それぞれ「検証した内容 = 保存した内容」になることを確かめる。

### N2 [Major] `removal()` が、同じ plan の `set_relationship_target` を見ない

**根拠**（`add.rs:1771-1776` は `attr(e, "Target")` を edit の木から直接読む。`view_rels_of` の L676-678 は `set_attrs` を当てる。`probe38r2.rs::p_retarget_then_remove_new_target`）

| 手順（同じ plan） | 結果 | 正しい結果 |
|---|---|---|
| P2: `rId1` を a.png から b.png に retarget し、b.png を削除する（source は `r:embed="rId1"`） | **`Ok(relationships: [])`**。保存後、`xl/s.xml rId1 → xl/media/b.png` がダングリングする | `StillReferenced` |
| P2d: 同じ手順で、source に `r:id` がない（型で参照される）場合 | **`Ok([])`** で、ダングリングする | `rId1` を外す（型が allowed なら） |
| P2c: retarget してから、旧 target の a.png を削除する（`r:id` なし） | `Ok([rId1])`。**書き換えた関係が消える** | `Ok([])`。`rId1` は b を指して残る |
| P2b: P2c で、source に `r:embed` がある場合 | `StillReferenced` で拒否される（誤り） | `Ok` |

- 同じ plan の `incoming_relationships(b)` は `["rId1"]` を返す（view は正しい）。それなのに、削除だけが古い target で判定している。
- 「後の step は前の step を見る」という README の原則と、「`Ok` ならダングリングしない」という保証の両方が破れる。
- ADR-0009 の手順は、retarget を rename だけで行い、対象が fragment（part を指さない）なので当たらない。それでも、公開 API の組み合わせとして成り立つ。

**提案**:

- `removal()` のループで、Existing の entry の target を、`set_attrs` の最後の値で上書きしてから判定する。`view_rels_of` と同じ処理なので、`ViewRel` の列から `(index, ViewRel)` を返すヘルパーにまとめるとよい。
- 安全網（`UnresolvedMention`）も、書き換えた後の target で判定する。
- テストは、上の表の 4 行。

## Minor

### n3 [Minor] `set_relationship_target` の Internal target の検査が、`add_relationship` より緩い

`probe38r2.rs::p_retarget_validation_and_escaping` で次を確かめた。

- **registry part を指せる。**
  - `../[Content_Types].xml`、`_rels/s.xml.rels`、`../_rels/.rels` は、どれも `Ok` で保存される。
  - `internal_target()`（`add_relationship` が使う）は、`RegistryPart` で拒否する。
- **正規形でない表記を、そのまま書く。**
  - `media/%62.png`（unreserved 文字を %エンコードしたもの。OPC の名前としては不正）と `media/B.PNG` は、どちらも `Ok` になる。
  - `add_relationship` は、`relative_target` で往復できる形しか書かない。
  - 読み手によっては解決できない。
- 問題ないもの:
  - fragment の `'`、`&`、`"`、`<` は、エスケープされて往復する。
  - TAB や LF は拒否される。
  - `Id` は大小文字を区別する（`RID3` は `RelationshipNotFound`）。
  - External から Internal への切り替えはできない。

提案: Internal で part を指す場合は、`view_part` で解決した canonical 名から `relative_target` で書き直す（fragment と query は保つ）。registry part は拒否する。または、往復の検査（`same_part_name(resolve(t), canonical)` と、`%` が正規形であること）を入れる。

### n4 [Minor・文書] 作り直した part の `.rels` は `Id` を 1 から振り直す。fuzz のモデル変更 2 か所はこれを前提にしている

- P5 の手順:
  1. `rId1`〜`rId3` を持つ `xl/s.xml` を、`.rels` ごと削除する。
  2. 同じ plan で、`Exact` で作り直す。
  3. `add_relationship` すると **`rId1`** になる。
  4. さらに、古い `rId3` を `r:embed` に含む内容を stage すると、`Ok` になる。
- `stage_part` の `ConflictsWithPlan` は、「残る `.rels` から外した ID」（`removed_ids`）しか見ない。part と一緒に消えた `.rels` の ID は見ていない。
- README の rules table には「an Id removed in the plan or earlier in the session … is not reused: the high-water is per relationships part and per session」とある。実装はこれと食い違う。

**作者が変えた fuzz モデル 3 か所の判定:**

| 変更 | 判定 |
|---|---|
| (1) M7: source の内容（元の内容、または stage した内容）にある rId を used に加える | **正当な仕様変更**。round 1 の M7 で求めたとおりである |
| (2) `Exact` で作り直した part について、`seen_ids` を捨てる。content type の期待値を消す。「元のバイトが変わった」の検査から外す | content type と変更検査は正当である（新しい part だから）。**`seen_ids` を捨てるのは、上記の README の規則と矛盾する実装に、モデルを合わせたもの**である |
| (3) 作り直した part では「関係のない `r:id`」の検査を外す | 作り直した part に stage する内容は呼び出し側のもので、core は staged の内容の `r:id` が解決されることを保証していない。モデルの範囲を絞ったものとしては正当である。ただし、(2) と同じ穴を隠している |

確認のため、(2) の `seen_ids` と (3) を元に戻して 20,000 本を回した。9 本が落ち、すべてこの経路だった（`rel id rId1 …, model rId5` が 4 本、`r:id without relationship` が 5 本）。バグを覆い隠しているわけではなく、文書化されていない規則をモデルに書き込んだもの、と判定する。

提案（どちらか）:

- 削除した part の `.rels` の `Id` を、`removed_ids`（と session の `removed_rel_ids`）に加える。こうすると (2) と (3) の例外は不要になり、README のとおりになる。1 行で直り、推奨する。
- README に「作り直した part の relationships part は新しいものとして始まる」と書く。

### n5 [Minor] 大小文字の違う `.rels`: registry は OPC の同値で引くが、`Package::relationships` は完全一致で引く

- 対象は `123233_charts.xlsx` の形で、part が `xl/worksheets/Sheet1.xml`、`.rels` が `xl/worksheets/_rels/sheet1.xml.rels` である。
- P4 の結果:
  - `incoming_relationships(a)` は `Sheet1.xml` を source として報告する。
  - `add_relationship(Some("…/Sheet1.xml"))` は、既存の `sheet1.xml.rels` に `rId2` を足す（正しい）。
  - しかし、`pkg.relationships("…/Sheet1.xml")` は **`[]`** のままである（`contains(&name)` が完全一致で引くため）。format crate は、いま足した `rId2` を解決できない。
  - `check_remove_part(Sheet1.xml)` は `HasOwnRelationships` を返す。一方、`relationships()` は「関係なし」と言う。corpus C1 の唯一の判定差（`single=HasOwnRelationships set=StillReferenced`）がこれである。
- registry の側は正しい（OPC の同値）。直すのは `Package::relationships` の方で、`find_ci` で引けば済む。#38 の範囲外とするなら、README の「Not yet」に書く。

### n6 [Minor・手順] origin/main とのマージに衝突がある

- `error.rs`、`lib.rs`、`package.rs` の 3 ファイルが衝突する。原因は、main 側（#14 マージの後の PR）が `read_original(&Part)` にサイズ上限（`max_part_bytes`）を入れたことと、`lib.rs`/`error.rs` の re-export と variant の追加である。
- 解消の要点:
  - `read_part` は `!d.is_dirty() && !part.detached` と `read_original(part)` を組み合わせる。
  - `parse_raw` は `read_original(&self.parts[i])` に直す。これでサイズ上限も効く。
  - `lib.rs` は両方の re-export の和集合にする。
- 解消したコピー（`repo/`、`255225b`）では、次のとおりだった。
  - `cargo test --workspace --release` の 37 binary がすべて通った。
  - `cargo clippy --workspace --all-targets -D warnings` も警告なしだった。
  - #14 の round 6 corpus sweep（`probe_r5.rs::corpus5`、4,176 ファイル、開けたもの 4,055）の結果: `removed_ok=23291 dangling=0 reopen_fail=0 err_with_mutation=0 panics=0`。round 6 と同じ数である。
  - 唯一の差は、チェックサムが壊れた 3 件が `OTHER I/O error` から `SourceNotScannable` に変わったことで、fail closed なので問題ない。
- origin/main を merge して追従する（rebase と force push はしない）。

### n7 [Minor・テスト] mutant と到達しないコード

**到達しないとして消したコード（A3/A4/A11/A16/R3/R5/R22）の判定**:

| 削除したもの | 判定 | 理由 |
|---|---|---|
| A3/A4: rId の大小文字衝突のループ | **到達しない（正しい）** | `rId{max+1}` と大小文字だけが違う ID は、数字が同じ `rId<digits>` であり、prefix を大小文字無視で数えている max に入る。先頭の 0（`rId05`）は文字列として別物で、u64 を超える桁は `rId{max+1}` と桁数が合わない。A3（+1 を外す）は、ループがなくなったので重複 ID として検出される |
| A11: Numbered が plan で削除した名前を避ける | **冗長（正しい）** | plan で削除した part は、まだ `pkg.parts` にある（plan は pkg を変えない）ので、max に入る |
| A16: set 内の source の検査 | **到達しない（正しい）** | set の part の `.rels` は `live(rels_part_name(name))` で `gone` に入り、走査の対象から外れる。`.rels` から source への帰属と `rels_part_name` → `find_ci` は、同じ正規化（1 回のデコードと大小文字の無視）を使い、同値な重複は open で拒否される |
| R3: `locate_relationship` の、外した ID の検査 | **冗長（正しい）** | Existing は `removes` で飛ばす。Appended と New は物理的に消える |
| R5: rId の used から `removed_ids` を外す | **冗長（正しい）** | 外した entry は、計画に使った木に残っている。Appended と New を外しても、`new_ids` は縮まない |
| R22: `copy_relationships` の、外した ID の検査 | **冗長（正しい）** | `view_rels_of` が `removes` を除く |

**新しい mutant 37 個**（`mutate_r2.py`、release、作者のテストだけ）:

- 実行したのは 36 個（N33 は対照のため除いた）。検出 31 個、**生存 5 個**。生存した 5 個は、作者が手を入れた `fuzz38`（3,000 本）を足しても生存した。fuzz の操作に retarget がないためである。
- 検出されたもの: detach の判定（N1、N2、N19）、view の retarget（N3）、fail closed（N4、N24、N34、N35）、M7（N5、N6、N30）、allowlist（N7）、plan 内の関係の除去（N8、N27）、retarget の検査（N9〜N11）、copy（N13〜N15）、fragment（N16）、Default の view（N17）、`Exact` の再利用（N18）、apply の dirty と `set_attrs`（N20、N36）、`touch`（N21、N22）、parse options（N23）、stage の衝突（N26）、追加した関係と削除の衝突（N31）、`HasOwnRelationships`（N32）。
- **生存したもの**:
  - **N28**: plan で作った `.rels` の関係（`RelAt::New`）を retarget しても、何も変わらない。
  - **N29**: plan で既存の `.rels` に追加した関係（`RelAt::Appended`）を retarget しても、何も変わらない。
    - N28 と N29 の場合、`Ok` が返るのに、view と保存結果の target は古いままになる。`set_relationship_target_rewrites_one_target` は、既存の entry と package の関係しか試していない。
  - **N37**: package の関係を、plan で追加した後に retarget しても、`package_rels` のキャッシュに反映されない。そのため、`main_document_part()` などが古い target を返す。
  - N25: 安全網（`UnresolvedMention`）の「plan で追加した part に解決するなら対象外」が外れても、どのテストも落ちない。外れると拒否が増える方向なので、安全な側の mutant である。
  - N12: retarget の `\` の検査を外しても落ちない。`\` を含む Internal target が view の part に解決することは、実際にはない（part 名は `\` を含めない）。ほぼ等価な mutant である。
- 提案: N28、N29、N37 にテストを 1 本ずつ足す（plan で追加した関係を retarget して、view、保存結果、`main_document_part` を確かめる）。fuzz に retarget の操作を足す（N2 も、これで見つかる）。

### n8 [Minor・文書] 細かい点

- `PartNameRequest::Numbered` の doc（L102-108）は、「names removed in this session」と書いているが、「plan」が抜けている（実装はどちらも避ける）。
- README の Part names の「Why」には「a part name identifies one part across sessions」とある。一方で、`Exact` は削除済みの名前を再利用できる。VBA の差し替えのための意図的な例外として、1 文を足す。
- `copy_relationships` の `verbatim` クロージャに `let _ = plan;` が残っている（未使用の引数）。

## stale（「6 経路以外に、bump しない mutator はあるか」）

- `Package` の `&mut self` の公開メソッドは、`xml_part`、`xml_part_mut`、`set_part`、`remove_part`、`apply` と、registry の convenience（`add_part`、`add_related_part`、`add_relationship`、`add_default_content_type`、`remove_relationship`、`set_relationship_target`、`set_content_type`、`remove_parts`）である。main とマージした後も増えていない。
- convenience はすべて `apply` を通るので、`touch()` を通る。
- `xml_part` は、Raw を Parsed にするだけで内容を変えない。計画に使った木と同じなので、bump しなくても無害である。
- `Package` は `Clone` ではないので、複製した package を介して同じ `(id, generation)` を偽ることもできない。
- **bump しない mutator はない。** mutant N21（`set_part`）と N22（`xml_part_mut`）は検出された。
- ただし、stale の検出は「パッケージが変わったか」しか見ない。「plan が前提にした内容が、保存される内容と同じか」は N1 の範囲である。

## fuzz と corpus

- **fuzz**（作者が手を入れた `fuzz38.rs` を、main とマージしたコピーで実行）: `FUZZ_ITERS=50000` で `failures: 0/50000`。モデルの変更の判定は n4 を見よ。
- **corpus38**（4,176 ファイル、開けたもの 4,055）:
  - C1: remove_parts の Ok は 27,220 件。単体版との判定差は、表記を除けば n5 の 1 件だけだった。
  - C2: copy の Ok は 18,441 件で、拒否は 0 件。
  - C3: 総当たりとの差は 1 件（n5 と同じファイル。registry の方が正しい）。fail closed は 7 件。
  - C4: L1 の追加は 4,050 件が Ok だった。`RegistryNotEditable` が 1 件（壊れた `.rels`）、`SourceNotScannable` が 2 件（DTD 付きの `document.xml`。M7 の走査は fail closed なので妥当）。
- **#14 round 6 sweep**: 上記 n6 のとおり、0 dangling で、check と remove の差も 0 だった。

## 判定

**converged: no / Accepted after Minor only: no**

- Blocker: なし。
- Major: N1（編集していない clone の検出をすり抜けられ、検証した内容と違う元のバイトが保存される）と、N2（`removal` が `set_relationship_target` を見ない）。
- Minor: n3〜n8。

N1 と N2 は、それぞれ数行の修正と 2〜4 本のテストで閉じる。n4 は 1 行で直すことを推奨する。n6 は main のマージが必要である。これらが入れば、次の round で収束と判定できる見込みである。
