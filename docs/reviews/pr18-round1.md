# PR #18 レビュー（Opus, round 1）

対象: `core/detach-insert`（bc09233）。`Lens::detach_as` / `LensMut::insert_detached[_by]` / 生成された `SameType` / ADR-0002 post-acceptance note "detach/insert"。
pptx 側の使い方（`impl/pptx-phase0.1-copy` の `crates/ooxml-pptx/src/write.rs:192-390`）は、API が用途に合うかを判断するためだけに読んだ。

## 結論（先に）

- 判定: **converged: no**。Major が 2 件ある。
- 動作は正しい。次の点を確かめた。
  - 拒否はどれも変更の前に起こる。
  - 挿入はその位置への純粋な挿入になり、dirty の扱いは `insert_at_slot` と同じである。
  - 属性とテキストのエスケープ（`\t` `\n` `\r` は文字参照で書く）、CDATA、コメント、PI は正しく複製される。複製した Markup に元の span は残らない。
  - 挿入先での prefix の選び直しは正しい。
  - `codegen --check` は差分なし。feature を 1 つずつ（dml/pml/sml/wml）と pml+dml で組んでも通る。`cargo test --workspace` も通る。
- 問題は、この primitive が保証していないことにある。
  1. **文書内で一意であるべき ID** を、拒否も文書化した呼び出し側の義務もないまま複製できる（F1）。
  2. 祖先にある **`mc:Ignorable` の効果**が、複製先で変わってしまう（F2）。
- round 2 の Q2 は「同じパート内のコピーなら … ID … 要らない」としていた。この前提が成り立つのは `a:rPr` / `a:pPr` / `a:endParaRPr` だけである。ところが #18 の API は、反射的な `SameType` と、許可リストの間で生成される `ChildOf` によって `p:sp` / `p:grpSp` まで扱える。
- `SameType` の生成そのものは健全である。`shape()` は次をすべて比べている。
  - 基底クラス分を含めて解決済みの属性（名前・型・既定値）
  - `attrs_incomplete`
  - 内容モデル
  - 子の SDK 名すべて（許可リストにないものも含む）

  同じ SDK complex type なら XSD 上の属性と内容は同じなので、この比較で十分である。孤児規則により、下流のクレートが外部の 2 つの型の間に `SameType` を実装することもできない。

## Findings

### F1（Major）: ID 属性が黙って複製され、重複 ID が作れる

- 根拠:
  - `crates/ooxml-core/src/schema.rs:192` の反射的な blanket impl と、`crates/ooxml-schema/src/generated/pml.rs:1022,1025` の `ChildOf<ShapeTree> for {GroupShape, Shape}` がある。そのため `detach_as::<pml::Shape>` で取った `p:sp` を同じ `p:spTree` へ `insert_detached` できる。結果として `p:cNvPr id` と `p:ph` が重複する。
  - 再現: scratchpad `review-pr18/wt/crates/ooxml-schema/tests/probe_detach.rs`（`p1_duplicate_shape_id`、`--features pml,dml`）。成功して `<p:cNvPr id="2" name="Title 1"/>` が 2 つ、`<p:ph type="title"/>` が 2 つになる。
  - 同じことは次の場合にも起こる。
    - `p:spTree` ↔ `p:grpSp`：ルートの `id="1"` を持ち込む。
    - グループ内の `a:stCxn/@id` と `a:endCxn/@id`：これらは図形 ID への参照で、複製すると元の図形を指したままになる。
  - `skip` は直下の子要素にしか効かない（`lens.rs:254`）。属性を外す手段はない。
  - `Detached` には書き換え API がない。直すには挿入後に `LensMut` で書き換えるしかなく、それが途中で失敗すると変更が部分的に残る。
  - ADR-0002 の note と README は、ID について何も書いていない。
- 提案（いずれか。推奨は (a)）:
  - (a) `codegen/allowlist/*.toml` に `[detach]` 節を足す。その節に載った型（今は `a:rPr` / `a:endParaRPr` / `a:pPr`、必要なら `a:txBody`）にだけ、生成器が marker `Detachable` を出す。`detach_as` には `E: Detachable, C: SameType<E> + Detachable` を要求する。ID を含みうる型かどうかを、型ごとにレビューで決めることになる。
  - (b) 最小限の対応として ADR note と README に書く。内容は「`detach_as` はスキーマ上の型しか保証しない。文書内で一意な属性（`cNvPr@id`、`p:ph@idx`、`w14:paraId` など）と、図形 ID への参照を直すのは呼び出し側の義務である」。あわせて `Detached` に挿入前の属性書き換え（たとえば `set_attr_at(path, q, v)`）を用意し、重複を作る例をテストに残す。

### F2（Major、起こる頻度は低い）: ignorable 名前空間が、複製先で ignorable でなくなる

- 根拠:
  - `detach_node`（`lens.rs:232-262`）が拒否するのは、残す subtree 内の `mc:*` だけである。祖先の `mc:Ignorable` が名前を挙げている名前空間を、subtree の要素や属性が使っていても検査しない。
  - `with_prefixes`（`lens.rs:755`）は、URI に束縛があるかどうかしか見ない。
  - 再現（scratchpad `review-pr18/wt/crates/ooxml-core/tests/detach.rs` の `probe_ignorable_namespace_loses_ignorability`）:
    - 複製元: `mc:Ignorable="e"` のもとにある `e:new="1"` と `<e:fx/>`
    - 複製先: `xmlns:f="urn:ext"` はあるが Ignorable はない
    - 結果: `<x:rPr f:new="1"><f:fx/></x:rPr>` で成功する。
  - MCE（Part 3）の規則では、理解できず ignorable でもない名前空間に出会った消費者はエラーにしなければならない。逆に、複製元では ignorable でない URI が、複製先で Ignorable に載った prefix に当たると、内容が黙って捨てられる。
  - PR の説明は「`mc:Ignorable` は prefix を名指しするので拒否する」としている。同じ理由が、祖先にある `mc:Ignorable` にも当てはまる。
- 提案:
  - `detach_as` で、複製元の祖先にある `mc:Ignorable` を URI に解決する。残す subtree がその URI を使っていれば拒否する（`DetachRefusal::IgnorableNamespace`）。これがいちばん簡単で、MC の要素や属性を拒否する方針とも揃う。
  - 別の案として、ignorable な URI の集合を `Detached` に持たせる方法もある。`insert_detached` では、複製先の in-scope Ignorable と状態が一致しない URI があれば変更前に拒否する。

### m1（Minor）: 関係を指す属性の検出が Transitional の `r:` だけである

- 根拠:
  - `lens.rs:241` は `R_NS` しか見ていない。
  - probe `probe_strict_and_vml_relationship_attrs_not_refused` では、Strict の `http://purl.oclc.org/ooxml/officeDocument/relationships` の `id` と、VML の `o:relid` が拒否されずに `Ok` になった。
  - Strict のパッケージは `Package::open` が拒否する。ただし `XmlDocument::parse` と `Lens::root` を直接使う経路には、その防御がない。VML は今の許可リストに含まれていない。
- 提案: Strict の relationships 名前空間と `o:relid` を拒否の対象に足す（数行で済む）。足さない場合は、対象外であることを README に書く。

### m2（Minor）: 祖先から継承した `xml:space` が複製先へ持ち越されない

- 根拠: probe `probe_inherited_xml_space_lost`。複製元の祖先は `xml:space="preserve"` で、subtree に空白だけのテキスト `<x:c>  </x:c>` がある。これは preserve のない複製先へ、そのまま入る。
- 今の許可リスト（pptx/xlsx）では実害はない。
- 提案: 実際に効いている `xml:space` を `detach_as` で記録する。挿入先と異なり、かつ空白だけのテキストがある場合は、複製のルートに `xml:space` を付けるか、拒否する。少なくとも README に書く。

### m3（Minor）: テストがミューテーションを見逃す

- 結果（`review-pr18/mutants.py`、`cargo test -p ooxml-core`）: 13 個中 10 個は検出された。生き残ったのは次の 3 つ。
  - **M1**: `skip` を孫以下にも適用する（`lens.rs:254` の `&[]` を `skip` にする）。直下の子だけという仕様が、テストされていない。
  - **M2**: `with_prefixes` の子に、挿入先の親の prefix を渡す。ほぼ等価な変更だが、規則を固定するテストがない。
  - **M11**: 要素の prefix を消さない。`raw()` は「prefix がない」と文書化されているが、テストはルートしか確かめていない。
- 生成器の `SameTypeMismatch` の経路（`xtask/src/emit.rs:204,446`）にはテストがない。`shape` の比較を外しても、今は検出されない（今日の時点では等価）。
- 提案:
  - 孫の位置に `skip` と同じ名前を持つ要素を置き、それが残ることを確かめるテストを足す（probe `probe_skip_is_not_recursive` をそのまま使える）。
  - `raw()` の全要素に prefix がないことを確かめる。
  - xtask に、形が違う 2 つのエントリを入れて diagnostic が出ることを確かめる単体テストを足す。

### m4（Minor）: API の使い勝手

- 根拠:
  - `insert_detached` は `Detached` を値で受け取るため、Err のときにも消費される。このため、使う側は毎回 `clone` している（pptx `write.rs:327,383,387`）。
  - 複数回挿入しても原子的であるように、pptx 側は約 60 行の `check_scope`（`write.rs:266-322`）を書いている。これは core の prefix 規則を作り直したものになっている。
- 提案: 次のどちらかを core に置く。
  - 変更せずに検査する `LensMut::check_detached(&Detached<C>) -> Result<()>`
  - `insert_detached(&Detached<C>)`

### m5（Minor）: 文書の細部

- `docs/adr/0002-schema-driven-code-generation.md:100` は `Error::NotDetachable(DetachRefusal)` と書いているが、実際の variant は `{ reason, name }` である。
- `SameType` の doc は「a renamed copy is a valid `Self`」と言う。しかし `Lens::cast`（`lens.rs:165`）を使えば、QName が同じで CT が違う spec へ移れる。そこから反射的に `detach_as` すると、この保証は成り立たない。保証はスキーマ上の型としてであり、`cast` の規律が前提だと書くべきである。
- `x:t` ↔ `x:v` はスキーマ上は同じだが、`x:v` の意味は `c@t` で決まる。意味まで等しいわけではないと一言書くとよい。
- 見た目だけの点がある: probe `probe_nested_foreign_prefix_with_children` では、同じ複製の中で `x:rPr` と `z:c` のように、同じ URI に別の prefix が使われる。整形式であり、§3 の規則どおりである。

## 確かめて問題がなかったこと

- 拒否は変更の前に起こる。`detach` は読むだけである。`insert` は slot、配置、prefix をすべて決めてから変更する。
- `NamespaceNotInScope` と `AlreadyPresent` のとき、出力は変わらず、dirty も付かない。
- `own_dirty` が付くのは、自己終了の親が最初の子を得たときだけである（`lens.rs:552` は `insert_at_slot` の `:471` と同じ）。
- 文書間のコピーでも、テキストの span は消される（M7 は検出された）。
- 属性の prefix は属性の規則で選ばれる。既定の名前空間は使われない（M8 は検出された）。
- 名前空間を持たない要素を、既定の名前空間がある複製先へ入れると `NamespaceNotInScope` になる。
- 層をまたぐ `cfg(all(feature = "dml", feature = "pml"))` は、どちらの側にも正しく出ている。

## 判定

**converged: no**。F1 と F2 を直せば収束する見込みである。F1 は (a) の marker か、(b) の文書化と書き換え API で直せる。F2 は ignorable な URI を拒否すれば直せる。m1〜m5 は同じ PR で直しても、後回しにしてもよい。
