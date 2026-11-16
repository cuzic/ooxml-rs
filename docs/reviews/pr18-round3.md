# PR #18 レビュー（Opus, round 3）

対象: `core/detach-insert` の HEAD `c1ca8ae`。round 2（`8413683`）以降のコミットは次の 3 つ。

- `bb9a13c` ooxml-core: `UNIQUE_ATTRS`（sealed `Detachable`）、`extLst` の拒否、複製先の検査、`check_insert_under` が panic しない
- `2073ca9` codegen: `[detach] unique_attrs`、`a:CT_TextCharacterProperties@smtId`
- `c1ca8ae` ADR-0002 note の更新

検証は、worktree を scratchpad の `review-18r3-19r2/` にコピーして行った（リポジトリは読むだけ）。

- `cargo test --workspace --all-features`: すべて ok。`clippy -D warnings`: 警告なし。`cargo xtask codegen --check`: 差分なし。
- probe:
  - `review-18r3-19r2/probes/zz_probe18r3.rs`（E1〜E4）
  - `review-18r3-19r2/probes/sealprobe_main.rs`（下流のクレートを想定）
- mutants: `review-18r3-19r2/scripts/mut18r3.py`。55 個（round 1〜2 の 35 個を今のコードに合わせたもの＋新しいコード向けの 20 個）。
- 仕様: ECMA-376 Part 1 第 5 版の PDF をテキストにしたもの（scratchpad にあるもの）。到達可能な型の列挙には `review-18r3-19r2/scripts/reach.py` を使った。

## 結論（先に）

- 判定: **converged: no**。Major が 1 件ある（R3-1）。直すのは 1 行で済む。
- R2-1（`smtId`）の直し方は正しい。
  - `[detach] unique_attrs` から、型ごとの `UniqueAttr` が生成される。
  - `detach_as` は、subtree のどこにあっても default 以外の値を拒否する。
  - 仕様の文言も確かめた（§21.1.2.2.3 / §21.1.2.3.2 / §21.1.2.3.9: "This ID is unique throughout the presentation"）。
- ただし、許可リストが「文書内で一意な属性はすべて列挙した」と言うには、もう 1 つ足りない。
  - `a:rPr` / `a:endParaRPr` / `a:defRPr` の下には `a:effectDag` と `a:cont`（`CT_EffectContainer`）を置ける。
  - その `@name` について、仕様は "**Shall** be unique across all effect trees and effect containers" と定めている。
  - 今はこれがそのまま複製される（R3-1）。
- R2-2（`extLst`）、R2-3（複製先の検査）、R2-5（panic しない）は解消した。
  - 複製先の検査を、組み合わせ 280 通りで確かめた。`check_insert_under` と `insert_detached` の結果が食い違うケース、期待と違うケース、`Err` なのに変更が残るケースは、どれも 0 だった。
- R2-4（ミューテーション）は、round 2 の残りのうち N6、N7、N9、N11 が検出されるようになった。
  - ただし、新しい複製先の検査に対する 3 つの mutant と N17 は残っている（R3-3、Minor）。いずれも拒否が減る方向の mutant である。
- R2-6（seal）は、doc の注記としては解消した。しかし「sealed」と呼んでいるのに、実際には封じられていない（R3-2、Minor）。
- `bb9a13c` は単独ではビルドできない（R3-4、Minor）。

## round 2 の解消状況

| 項目 | 状況 | 根拠 |
|---|---|---|
| R2-1 `smtId` | **解消**。ただし同じ種類の漏れが R3-1 | 生成された `UNIQUE_ATTRS` は 3 型すべてで同じで、`a:defRPr`、`a:endParaRPr`、`a:rPr`、`cs:defRPr` の `smtId`（default `"0"`）を含む。これは SDK にある `a:CT_TextCharacterProperties/*` の 4 エントリすべてにあたる。子孫の `a:pPr/a:defRPr@smtId` も拒否される（`detach_dml.rs`）。<br>mutant U1（検査を外す）、U2（ルートだけを検査する）、U3（default を無視する）は検出された。<br>字句の揺れ（`"00"`、`" 0"`、`"+0"`）は拒否される（E4）。拒否が増える側なので問題ない。<br>`bmk` は参照なので対象外とした判断は正しい（§21.1.2.3.9 "link target name … in a custom XML part"）。 |
| R2-2 `extLst` | **解消**（拒否する方針） | `extLst` という local 名の要素は、名前空間を問わず拒否される（`ExtensionList`）。`skip` で外した直下の子は対象外になる。<br>mutant X1 と X2 は検出された。<br>corpus（scratchpad の 94 ファイル＋fixtures、開けたのは 106 ファイル）で、`a:rPr` / `a:endParaRPr` / `a:pPr` は 13,497 個（子を持つものは 6,560 個）ある。そのうち、リンクの外に `extLst` を持つものは 0 個だった。拒否による実害はない。 |
| R2-3 複製先の側 | **解消** | `check_destination` を、`insert_detached`（`place` の後、変更の前）と `check_insert_under` の両方で呼んでいる。<br>E2 の行列で確かめた。複製元は「`xml:space` の 3 通り × 空白と拡張名前空間の 4 通り」、複製先は「`xml:space` の 4 通り × `mc:Ignorable` の 7 通り」で、次を含む。<br>・親（`a:r`）自身に付いた Ignorable<br>・prefix を下で別の URI に束縛し直す場合<br>・Ignorable を宣言した要素の上で、別の URI に束縛されている場合<br>280 通りで、`check == insert`、期待どおりの拒否理由、`Err` なら変更なし、のいずれにも反するものは 0 だった。<br>`created = 1`（E3）でも一致した。<br>doc（`IgnorableNamespace` と `InheritedXmlSpace`）も直っている。 |
| R2-4 mutants | **一部解消** | round 2 で生き残った 5 個のうち、N6、N7、N9、N11 は検出されるようになった。N17 は今も残る（R3-3）。round 1〜2 の残りはすべて検出された。 |
| R2-5 panic | **解消** | `element_at` を共通にして、不正な path は `Err(WrongElement)` になる。テストは 3 種類の path を確かめている。<br>`Lens::lens_mut` と `LensMut::root` で depth が違うことも doc に書かれた。 |
| R2-6 seal | **doc としては解消**。言い方は R3-2 | 「保証の対象は生成された impl だけ。手で書いた impl は書いた人の責任」と明記された。ただし seal としては働かない。 |

## Findings

### R3-1（Major、まれ）: `a:CT_EffectContainer@name`（`a:effectDag` / `a:cont`）が `unique_attrs` に入っていない

- 仕様: ECMA-376 Part 1 §20.1.8.20 `cont` と §20.1.8.25 `effectDag` の `name` 属性は、"Specifies an optional name for this list of effects, so that it can be referred to later. **Shall be unique across all effect trees and effect containers.**" と定めている。`smtId`（"is unique"）より強い規範的な文言である。
- どこから届くか:
  - `CT_TextCharacterProperties` は `EG_EffectProperties`（`effectLst | effectDag`）を持つ。`a:rPr`、`a:endParaRPr`、`a:pPr/a:defRPr` のどれからでも `a:effectDag` に届く。
  - `a:cont` は、`effectDag`、`blend`、`alphaMod` の下にある。
  - `reach.py` で `CT_TextCharacterProperties` と `CT_TextParagraphProperties` から到達できる 90 の CT の全属性を見て、仕様の説明と突き合わせた。一意性が要求されているのは `smtId` と `CT_EffectContainer@name` の 2 つだけだった。
    - `bmk`、`a:snd@name`（"original name or given short name"）、`a:effect@ref`（名前への参照）は一意性を要求されない。
    - `r:*` はもともと拒否される。
  - SDK data の属性（`a:CT_TextCharacterProperties/`、`a:CT_TextParagraphProperties/`、`a:CT_EffectContainer/`）も XSD と一致した。
- 再現（E1）: `<a:rPr lang="en-US"><a:effectDag name="E2" type="sib"><a:cont name="C2"/></a:effectDag></a:rPr>` を、`detach_as::<RunProperties>` → 新しい `a:r` への `insert_detached` と進めると、成功する。結果は `name="E2"` が 2 個、`name="C2"` が 2 個になる。`a:pPr/a:defRPr/a:effectDag@name` と、`EndParagraphRunProperties` として複製する場合も同様に成功する。
- 影響: `Detachable` の doc（"every remaining attribute that must be unique in the document listed in `UNIQUE_ATTRS`"）、`[detach]` のコメント、ADR-0002 note が言っていることが偽になる。R2-1 と同じ種類の問題である。
- 頻度: corpus＋fixtures の 106 ファイルで、テキストの書式の下にある `effectDag` / `cont` は 0 個だった。PowerPoint は通常 `effectLst` を書く。
- 提案:
  - `unique_attrs = ["a:CT_TextCharacterProperties@smtId", "a:CT_EffectContainer@name"]` とする。default は `None` なので、値があれば必ず拒否される。今の `UniqueAttr { default: None }` の扱いで、そのまま動く。
  - `detach_dml.rs` に 1 ケースを足す。
  - `dml.toml` のコメントと ADR-0002 note に §20.1.8.20 / §20.1.8.25 を足す。
  - 「reviewed and not unique」の一覧に `a:effect@ref`（参照）と `a:snd@name` を足すと、レビューの範囲がはっきりする。

### R3-2（Minor、文書）: `Detachable` は「sealed」と書かれているが、下流のクレートから実装できる

- 再現（`sealprobe_main.rs`。ooxml-core と ooxml-schema に path で依存する別のクレート）:
  - 次を実装する。
    - `impl ooxml_core::schema::__private::Sealed for MySp {}`
    - `impl Detachable for MySp {}`
    - `impl ChildOf<pml::ShapeTree> for MySp {}`
    - `MySp` の `QNAME` は `p:sp`、`ATTRS` と `CONTENT` は `pml::Shape` のものを使う。
  - 実在の `p:sp` に対して `Lens::<MySp>::at → detach_as → insert_detached` と進めると、ビルドも実行も通る。結果は `id="2"` が 2 個になる。
- `ooxml-schema` は別のクレートなので、`__private` は `pub` にせざるをえない。そのため、本当の seal はこの構成では作れない。
- 一方、テスト（`detach.rs`）自体もこの裏口を使っている。
- 保証の範囲（生成された impl だけ）は doc に書かれているので、保証が嘘になっているわけではない。直すべきなのは言葉である。
- 提案: schema.rs の doc と ADR-0002 note の「Sealed」「sealed through」を、「`#[doc(hidden)]` の supertrait。実装すること自体は可能だが、サポートしない」のような表現にする。

### R3-3（Minor、テスト）: 新しい複製先の検査と、祖先の走査に対する mutant が生き残る

55 個のうち、生き残ったのは次のとおり。

- **DS5**: 複製先の `uses` が子孫に再帰しない。
- **DS6**: 複製先の文脈から親自身を外す（`parent_path[..len-1]`）。
- **DS8**: 複製先で要素名の ignorable を検査しない（属性だけを見る）。
- **N17**: 祖先の走査を、ignorable を 1 つ見つけたところで止める。それより下の `xml:space` と Ignorable を見落とす。

いずれも拒否が減る方向である。round 2 で N17 を「安全側」と書いたのは誤りだったので、ここで訂正する。

- コードは正しい。E2 の行列には、親自身に Ignorable がある場合と、ignorable な名前空間を使う子孫がある場合も含めたが、不一致は 0 だった。
- テストの入力が、その形を含んでいないだけである。
  - `destination_ignorability_and_xml_space_are_checked` は、Ignorable を `x:p` に置き、挿入先は `x:r` である。そのため親自身は無関係になる。
  - ignorable な名前空間を使うのは、ルートの属性 `e:new` だけである。
- 提案: 次の 3 つを足す。
  - 複製先の `x:r` 自身に `mc:Ignorable` があるケース
  - 複製の子孫の要素（`<e:z/>`）が、複製先で ignorable になるケース
  - 祖先の上の方に Ignorable があり、その下に `xml:space="preserve"` があって、テキストを持つケース
- 等価または安全側の mutant（対応は不要）:
  - U4 / U5: 生成された 3 型のリストは同じである。
  - U6: local 名だけで照合する。拒否が増える側である。
  - G1: `a:rPr` の下に `a:defRPr` は現れない。
  - S2: ノード自身の `mc:Ignorable` は、もともと MC 属性として拒否される。

### R3-4（Minor、履歴）: `bb9a13c` は単独ではビルドできない

- `git archive bb9a13c` を展開して `cargo build --workspace --all-features --tests` を実行すると、`ooxml-schema` が `E0277`（``RunProperties: …::__private::Sealed` is not satisfied``）を 3 件出す。
  - core で supertrait を足したのに、生成された `dml.rs` の再生成が次の `2073ca9` に分かれているためである。
  - `2073ca9` ではビルドが通る。
- 判断:
  - コードの正しさには影響しない。
  - リポジトリの方針（PR ブランチは main のマージで更新し、force-push しない）を考えると、履歴を書き直す必要はない。
  - bisect がこのコミットに当たったら `git bisect skip` すればよい。
  - 今後は「core のトレイトを変えるコミット」と「再生成したファイル」を 1 コミットにまとめると、`codegen --check` も各コミットで通る。
  - PR #19 はこの 2 コミットをマージしているので、#19 側の bisect にも同じことが言える。

### 細かい点（任意）

- n1: `emit.rs` の `unique_attrs` は、要素の prefix が解決できないと `filter_map` で黙って落とす。今のデータでは 4 つとも解決できるが、一意性の宣言が黙って消えるのは避けたい。`Err` にしたほうがよい。
- n2: `check_insert_under` の `parent_path.len() + 1 + created + self.height()` は、`created` が非常に大きいと overflow する。debug ビルドでは panic し、release ビルドでは折り返して `Ok` になる。`saturating_add` にするとよい（挿入そのものは別に depth を検査するので、壊れることはない）。
- n3: `check_insert_under` が不正な `parent_path` に対して返す `WrongElement` は、`expected` に複製の型 `C::QNAME`（例: `a:rPr`）を入れている。path が指すのは親なので、紛らわしい。

## 確かめて問題がなかったこと

- 拒否の順序:
  - `insert_detached` は、slot、TooDeep、`place`（読むだけ）、複製先の検査、prefix、変更の順に進む。
  - `check_insert_under` は、path、TooDeep、複製先の検査、prefix の順で、上と同じ並びである。
  - どちらも、拒否はすべて変更の前に起こる（E2 と E3。`Err` なのに変更が残ったケースは 0）。
- `text_inherits_space` の扱い:
  - 複製元で `xml:space` が default で、テキストがあるときだけ立つ。
  - 複製先で preserve が効いていれば拒否される。default に戻されていれば許可される。複製のルート自身に `xml:space` があれば、それが運ばれるので検査しない。
  - 行列のすべての組み合わせで、期待どおりだった。
- round 1〜2 の mutants（M1〜M13、N1〜N16、D1〜D5、N11、N12）は、今のコードに合わせたうえで、すべて検出された。新しい mutant のうち、U1〜U3、X1、X2、DS1〜DS4、DS7、S1、E1 も検出された。

## 判定

**converged: no**。

- R3-1（`a:CT_EffectContainer@name` を `unique_attrs` に足し、テストを 1 つ足し、ADR に 1 行書く）を直せば収束する。
- R3-2〜R3-4 と n1〜n3 は Minor である。R3-3 のテスト 3 つは、同じ PR で足すことを勧める。
