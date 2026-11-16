# PR #38 レビュー（Opus, round 3）

対象: `core/add-part` @ `f9862b5`。round 2 の修正（N1、N2、n3〜n5、n8 と N28/N29/N37 のテスト）と、origin/main のマージ `68a02c1` を見た。
照合したもの: `add.rs`、`package.rs`、`tests/registry_review2.rs`、README の rules table、ADR-0001 の note「registry writes」、round 2 レビュー（`pr38-round2.md`）、作者の mutant スクリプト（`scratchpad/mut38r3/mutate.py`）。

リポジトリは変えていない。作業はすべて scratchpad の `review-38r3-53/pr38/` で行い、`git clone` したコピーを使った。

- `repo/`: `f9862b5` のまま。次のものを置いた。
  - `tests/probe38r3.rs`: プローブ A〜E。
  - `tests/fuzzx.rs`: round 2 のモデル。作者が足した例外 2 か所は戻してある。
  - `tests/fuzzr3.rs`: fuzzx に 3 つを足したもの。
    - `set_relationship_target`（同じ id への複数回を含む）と、その結果の検査。
    - plan の前に、セッションで part を変える。`set_part` の場合と、`xml_part_mut` のレンズ編集の場合がある。
    - `read_part` を parse し直した木を stage する（N1 のすり抜けの形）。
  - `tests/corpus38.rs` と、#14 round 6 の `probe_r5.rs`。
- `mut/`: レビュアーのテストを入れていないコピー。`mutate_r3.py` の新しい mutant 15 個と、作者の 5 個（N1c、n4、N28、N29、N37）を流した。
- `mergeprobe/`: `f9862b5` に現在の origin/main（`a0f2717`。#37、#44、#45、#46、#27 を含む）をマージしたもの。
- `m49/`: `mergeprobe/` に、さらに #49（`core/detach-same-part`）をマージしたもの。

## 結論（先に）

**収束（Blocker 0、Major 0）。** round 2 の Major 2 件（N1、N2）は閉じた。Minor の n3、n4、n5、n8 も閉じた。

- N1: 「編集していない」と判定されるのは、元のエントリとバイト単位で同じ木だけになった。すり抜けを 7 通り試したが、どれも「検証した内容 = 保存した内容」になった。
- N2: removal は retarget 後の target を読む。round 2 の 4 ケースに加えて、a→b→a、fragment への retarget、package の関係も正しく判定した。
- 回帰は見つからなかった。
  - fuzz（例外を戻したモデル）: 50,000 本で違反 0 件。
  - fuzz（retarget と N1 のすり抜けを足したモデル）: 50,000 本で、実装の違反は 0 件。落ちた 1 本はモデル側の誤りだった（後述）。
  - corpus38 と、#14 round 6 の sweep: round 2 と同じ数か、それより良い。
  - 現在の main とのマージ: 衝突はなく、workspace の 48 binary がすべて通った。clippy `-D warnings` も警告なしだった。#49 を重ねても衝突はなかった。

残りは Minor 3 件と Nit 3 件で、どれもマージを止めない。

## 解消状況（round 2 の指摘）

| 指摘 | 状態 | 確認内容 |
|---|---|---|
| N1 「編集していない」判定のすり抜け | **解消** | `stage_part` は `d.is_dirty() \|\| read_original(part) != d.source()` のとき detach する（`add.rs:1920-1925`）。detach しない木は、clean で、source が元のエントリとバイト単位で同じである。その場合、保存される元のエントリは T1 によって `serialize(d)` と同じになる。下の A1〜A7 を見よ |
| N2 `removal` が retarget を見ない | **解消** | `add.rs:1789-1792` は、`set_attrs` の最後の値を読む。`view_rels_of` と同じ規則である。round 2 の P2、P2b、P2c、P2d は `n2_removal_sees_retargets` に入った |
| n3 retarget の検査と正規形 | **解消** | registry part は `RegistryPart` で拒否する。part を指す target は、`internal_target` で canonical な相対形に書き直す（fragment と query は保つ）。`../../../xl/media/b.png`、`./media/./b.png`、`media//b.png`、`/xl/…`、`%62`、大小文字の違いは、すべて `media/b.png` で書かれる。`""`、`.`、`media/`、`?q`、Internal の `http:`/`mailto:` は拒否される |
| n4 作り直した part の Id | **解消（範囲に注意 → n9）** | part と一緒に消えた `.rels` の Id は、`removed_ids` に入る。`Exact` で作り直すと `rId4` から続く（`n4_…`）。fuzz の例外 (2)(3) を戻した 50,000 本でも、違反は 0 件だった |
| n5 大小文字の違う `.rels` | **解消** | `Package::relationships` は `find_ci` で引く。corpus38 の C3「incoming != brute」は 1 件から 0 件になった。C1 の `RelationshipTypeNotAllowed` 1 件も消えた（どちらも同じファイル） |
| n6 main のマージ | **解消** | 後述の「main とのマージ」を見よ |
| n7 生存した mutant（N28、N29、N37） | **解消** | 作者のスクリプトで、N28、N29、N37、N1c、n4 はすべて検出された |
| n8 文書 | **解消** | `Numbered` の doc に「or plan」が入った。README の「A name is taken when」には、`Exact` の例外が 1 文入った。`let _ = plan;` は消えた |

## N1 の検証（すり抜けの試み）

`probe38r3.rs::a_n1_spoofs`。

| # | 手順 | 結果 |
|---|---|---|
| A1 | `set_part(PLAIN)` の後で、**元の** `EMBED` を parse して stage し、画像を削除する | `StillReferenced`（view は staged の `EMBED`）。apply すると元のエントリが保存される。検証した内容と同じである |
| A2 | `xml_part_mut` で `<pic>` を消した後で、元のバイトを parse して stage し、削除する | `StillReferenced`。保存されるのは元の内容で、staged と同じ |
| A3b | `*xml_part_mut(s) = parse(PLAIN)` の後で、その clone を stage し、削除する | source が元と違うので detach される。保存は `PLAIN` で、ダングリングは 0 |
| A5 | 元のエントリが BOM 付き。stage するのは BOM 付き / BOM なし | BOM 付きは detach されず、元のエントリ（BOM 付き）が保存される。BOM なしは detach され、BOM なしが保存される。どちらも staged と一致する |
| A6 | plan 1 で dirty な clone を stage する（detach）。plan 2 で元を parse して stage し、削除する | `StillReferenced`。保存は元の内容。`detached \|= …` は sticky で、clean な `serialize` は元と一致する |
| A7 | 元のバイトを `parse_with(max_depth=3)` で parse して stage する | 保存は元と一致する。parse options の違いは結果に影響しない |
| — | `read_original` が失敗する場合（サイズ上限など） | `map_or(true, …)` なので detach される（fail closed） |

`release_part` はこのブランチにない（ADR-0010 の範囲）。`XmlDocument` は UTF-16 を `UnsupportedEncoding` で拒否する。そのため、エンコーディングの違いが「source は同じなのに保存バイトが違う」経路になることはない。

## main とのマージ（#40/#44/#45/#49 まわり）

- 作者のマージ `68a02c1` は、#29 の時点の main（`ab1b0b6`）を取り込んでいる。#40（`read_original` のサイズ上限、BOM の span）は入っている。`read_part`、`parse_raw`、`write_to` は `read_original(&Part)` を通る。
- 現在の main（`a0f2717`。#44 move_in、#45 insert_with_ns、#37、#46、#27）とのマージは、**衝突なし**で通った（`mergeprobe/`）。
  - `cargo test --workspace --release`: 48 binary がすべて ok で、失敗は 0。
  - `cargo clippy --workspace --all-targets -- -D warnings`: 警告なし。
  - main との差分で `crates/ooxml-core/src` から消える行は、#14 の `RemovalPlan`/`RegistryEdit` を `remove_parts` の 1 要素版に置き換えた部分と、`lib.rs` の re-export、doc だけだった。#40、#44、#45 の行は消えていない。
- ADR-0001: main の interim note は残っている。#38 の note「registry writes」は、その後ろに足されている。
- #49（`core/detach-same-part`、open）も、上に重ねて衝突なしで通った（`m49/`）。`ooxml-core` と `ooxml-schema` のテストはすべて通った。
- #53 の作業ツリーにある `ooxml-core` は、このマージ結果と比べて `lens.rs` の `move_child`（#53 の追加分）だけが違う。#38 の部分は同じである。

## fuzz と corpus

- **fuzzx**（round 2 のモデルで、作者の例外 (2)(3) は戻したもの）:
  - `FUZZ_ITERS=50000` で `failures: 0/50000`。
  - round 2 の n4 による 9 本は消えた。
- **fuzzr3**（retarget と、セッションでの part 変更、`read_part` の parse し直しを stage する操作を足したもの）:
  - 50,000 本で、実装の違反は 0 件。
  - 落ちた 1 本（seed 33933）は、モデル側の誤りである。`StageCur("p/S5.xml")` は、plan の前の `pkg.read_part` を読む。ところが同じ plan で `p/S5.xml` は削除され、`p/s5.xml` として作り直されている。逐次実行の側では、`read_part` が大小文字の違う名前で `PartNotFound` になる。
  - このモデルは、mutant R3（removal で最初の retarget が勝つ）を 3 本で検出する（新しいダングリングと、batch と逐次の差）。R15（part を指さない関係の Id を記録しない）も 1 本で検出する（関係のない `r:id`）。どちらも作者のテストでは生存した（n10）。
- **corpus38**（4,176 ファイル、開けたもの 4,055）:
  - C1: `remove_parts` の Ok は 27,220 件。単体版との差は表記だけだった。
    - `HasOwnRelationships` と `StillReferenced` の差が 1 件ある。どちらも拒否で、単体版の方が先に検査するだけである。
  - C2: copy の Ok は 18,441 件で、拒否は 0 件。
  - C3: 総当たりとの差は **0 件**（round 2 では 1 件）。fail closed は 7 件。
  - C4: Ok 4,050 件、`RegistryNotEditable` 1 件、`SourceNotScannable` 2 件。round 2 と同じである。
- **#14 round 6 sweep**: `files=4176 opened=4055 panics=0 removed_ok=23291 dangling=0 reopen_fail=0 err_with_mutation=0`。round 2 と同じ数である。

## mutant（新しいもの 15 個、作者のテストだけ、release）

| mutant | 結果 | 判定 |
|---|---|---|
| R2 N1 を現在の内容との比較に戻す | 検出 | — |
| R5 N2 元の target が勝つ | 検出 | — |
| R6 n3 suffix を落とす / R8 n3 そのままの表記 | 検出 | — |
| R9、R10 n4 の Id 割り当て | 検出 | — |
| R13 n5 完全一致で引く | 検出 | — |
| **R3** removal で最初の retarget が勝つ（`.rev()` を外す） | **生存** | 実害あり。a→b→a の後で a を削除すると、`Ok` になって `rId1 → a` がダングリングする。n10 |
| **R4** view で最初の retarget が勝つ | **生存** | R3 と同じ穴。こちらは view（`incoming_relationships`、copy）側である。n10 |
| **R15** part を指さない関係（external、fragment）の Id を `gone_ids` に入れない | **生存** | 作り直した part が、外部ハイパーリンクの旧 Id を再利用する。n10 |
| **R1** `read_original` が失敗したとき detach しない | **生存** | fail closed の経路にテストがない。n10 |
| R11 stage の removed-id 検査で、走査できない内容を通す | 生存 | XML の content type の内容は、先に `MalformedXml` で拒否される。非 XML のバイトについてだけ、拒否の側に倒れる。安全側の mutant |
| R7 retarget の `\` 検査を外す | 生存 | ほぼ等価。`internal_target` が canonical を作り直すからである（round 2 の N12 と同じ） |
| R12 `detached \|=` を `=` にする / R14 detach の判定から `is_dirty()` を外す | 生存 | 等価。detach しない木は、source が元と同じである。dirty なら、`write_to` は状態を見てシリアライズする |

作者の 5 個（N1c、n4、N28、N29、N37）は、すべて検出された。

## Minor

### n9 [Minor] 「作り直した part の内容は、旧 Id を名指しできない」規則は、`stage_part` で、同じ plan の中でしか効かない

`probe38r3.rs::d_n4_more`。README（「A name is taken when」と「Content of existing parts」の行）の書き方は、この規則が一般に成り立つように読める。しかし実装は次のとおりである。

- **D1**: part を削除し（`rId1`〜`rId3` を持つ `.rels` ごと）、`add_part(Exact, content)` の **content** で `r:embed="rId3"` を渡すと、`Ok` になる。続く `add_relationship` は `rId4` を返す。保存したファイルでは、`rId3` が関係のない `r:id` になる。
  - 同じ内容を `stage_part` で渡すと、`ConflictsWithPlan` で拒否される（`n4_…` テスト）。
- **D2**: apply した後の**次の plan** で、同じセッションで削除された `rId3` や `rId2` を名指しする内容を stage すると、`Ok` になる。
  - 検査は `plan.removed_ids` だけを見る。`pkg.removed_rel_ids` は見ない。Id の割り当ては、セッション全体の high-water を見ている。
- **D3**: 作り直した part に `copy_relationships(q → s)` すると、q の Id（`rId1`）をそのまま使う。「作り直した part の関係は旧 Id より上から続く」と食い違う。
  - その後で q と同じ内容を stage すると、`ConflictsWithPlan` になる。
  - `add_part` の content で渡した場合は通る。

どれも、呼び出し側が渡した内容の `r:id` の話である。core は、staged の `r:id` が解決されることを一般には保証していない（round 2 の判定）。だから Major ではない。

提案（どちらか）:

- 検査を `add_part` / `add_related_part` の content にも当てる。`pkg.removed_rel_ids` も見る。`copy_relationships` は、旧 Id と衝突する場合に拒否する。
- README の 2 か所を、「`stage_part` で、同じ plan で外した Id に限る」と書き直す。

いまの呼び出し元（ADR-0009 の Phase3、VBA の差し替え）はどれにも当たらないので、後者で足りる。

### n10 [Minor・テスト] 生存した mutant R3、R4、R15、R1 にテストを足す

- **R3/R4**: 同じ関係を 2 回 retarget して「最後が勝つ」ことを確かめるテストがない。
  - B1（a→b→a の後で b を削除すると `Ok([])`、a を削除すると `StillReferenced`）を足せば、両方とも検出される。
  - 実装は正しい。removal、view、apply のどれも、最後の値を使う。
- **R15**: `n4_…` の `.rels` に、external か fragment-only の関係を 1 つ足す。作り直した後の `add_relationship` が、その Id より上になることを確かめる。
- **R1**: `open_with(max_part_bytes = 小)` で開き、上限を超える part に新しい木を stage する。保存されるのが staged の木であることを確かめる。
- 推奨: 作者の fuzz に、`set_relationship_target` の操作を足す（レビュアーの `fuzzr3.rs` の `Op::Retarget` と、`retargets` の検査を参照）。R3 と R15 は、これで検出される。

### n11 [Minor・既存・範囲外] `*pkg.xml_part_mut(name)? = other_doc` とすると、編集が黙って失われる

A3 の結果:

- 代入した木は clean で、`detached` も false である。そのため、`read_part` と `write_to` は**元のエントリ**を返す。一方、`parsed_part` は新しい木を返す。
- format crate が読む内容と、保存される内容が食い違う。
- registry の view は元のエントリを読むので、#38 の不変条件（「検証した内容 = 保存した内容」）は破れない。
- `xml_part_mut` が `&mut XmlDocument` を返すことから来る、#38 より前からの問題である。

提案は次のどれか。#38 の範囲外として issue にしてよい。

- `xml_part_mut` で、木を差し替えたかどうかを検出する。
- 返り値を、差し替えのできないラッパにする。
- doc に「木を差し替えない（新しい内容は `set_part` か `stage_part` で渡す）」と書く。

## Nit

- **n12**: retarget の fragment に `#` が 2 つあっても受け付ける（`media/b.png#a#b`）。URI として不正である。`#` や `#a b` のような fragment-only の target も通る。ADR-0009 の用途では害はない。
- **n13**: `Package::from_bytes` は、package の関係を `rels_part_name(None)` の**完全一致**で読む。
  - `_rels/.RELS` という名前だと、`package_relationships()` は 0 件で、`main_document_part()` は `None` になる。
  - それでも、registry は `find_ci` でその part を編集する。`add_relationship(None, …)` の後、キャッシュは 1 件、読み直すと 2 件になり、食い違う。
  - n5 の兄弟にあたる。format crate はこのファイルを開けないので、実害は小さい。
- **n14**: 非 ASCII の part 名（`xl/media/あ x.png`）は、`add_relationship` でも `set_relationship_target` でも `InvalidTarget` になる。`relative_target` がパーセントエンコードしないためである。fail closed で、両者の振る舞いも一致しているが、README の「Not yet」に 1 行書くとよい。

## 判定

**converged: yes**

- Blocker: なし。
- Major: なし。
- Minor: n9（旧 Id の規則の範囲。README を直すか、検査を広げる）、n10（mutant R3、R4、R15、R1 のテスト。retarget の fuzz を推奨）、n11（既存の問題で範囲外。issue でよい）。
- Nit: n12〜n14。

Minor はどれもマージ後の follow-up でよい。n10 の B1 テスト（2 回の retarget）は数行なので、マージ前に入れることを推奨する。
