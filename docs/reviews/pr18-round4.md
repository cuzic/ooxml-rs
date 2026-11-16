# PR #18 レビュー（Opus, round 4）

対象: `core/detach-insert` の HEAD `cca838e`。round 3（`c1ca8ae`）以降のコミットは次の 1 つ（ほかに review 文書の `53df28f`）。

- `cca838e` `a:CT_EffectContainer@name` を unique にする（R3-1）、seal ではないという文言（R3-2）、複製先の親と子孫・祖先の走査のテスト（R3-3）、生成器の未解決 `unique_attrs` をエラーにする、depth 検査の saturating、親を名乗る `WrongElement`（n1〜n3）

検証は、worktree を scratchpad の `review-18r4-19r3/` にコピーして行った（リポジトリは読むだけ）。

- `cargo test --workspace --all-features`: すべて ok。`clippy --all-targets -D warnings`: 警告なし。`cargo xtask codegen --check`: "7 generated files up to date"。
- 再生成した `dml.rs` は同じコミットに入っている。R3-4（単独でビルドできないコミット）の再発はない。
- probe: `review-18r4-19r3/probes/zz_probe18r4.rs`（effect container 名の到達経路 8 通り）
- 仕様の列挙: `review-18r4-19r3/scripts/uniq_scan.py`
  - ECMA-376 Part 1（2016）のテキスト（scratchpad の `pptx-p1/ecma/part1.txt`、読むだけ）を使った。
  - 要素の節ごとに "content model (CT_…)" で型を割り当て、`reach.py` の到達型集合と突き合わせて、uniq / identif / ID を含む行を全部抜き出した。
- mutants: `review-18r4-19r3/scripts/mut18r4.py`。62 個（round 3 の 55 個を今のコードに合わせたもの＋新しい 7 個）。

## 結論（先に）

- 判定: **converged: yes**。
- R3-1（effect container の名前）は直っていて、仕様との再照合でも漏れはない。
  - 到達型（CT_TextCharacterProperties と CT_TextParagraphProperties から 96 型）のうち、ECMA が節を持つ 96 型すべてを走査した。
  - 一意性を求める文言は `smtId`（§21.1.2.2.3 / §21.1.2.3.2 / §21.1.2.3.9 "This ID is unique throughout the presentation"）と `CT_EffectContainer@name`（§20.1.8.20 / §20.1.8.25 "Shall be unique across all effect trees and effect containers"）の 2 つだけだった。
  - これ以外に "unique" を含む行は 3 つあった。どれも文書内で一意な値ではない。
    - `a:tab`（§21.1.2.2.13）の "4 unique tab stops" は例の説明である。
    - `a:ext@uri` は拡張の種類を表す URI である。`extLst` はもともと拒否される。
    - `a:hlinkClick` などの `r:id` / `r:embed` / `r:link` はもともと拒否される。
- R3-2（「sealed」の文言）は、doc と ADR では直った。古い言い方が 2 か所に残っている（nit）。
- R3-3 の 4 つの mutant（DS5、DS6、DS8、N17）は、すべて検出されるようになった。
- n1〜n3 も直った。n2 と n3 の修正には、どちらも検出するテストがある。
- 新しい Major / Minor はない。

## round 3 の解消状況

| 項目 | 状況 | 根拠 |
|---|---|---|
| R3-1 `CT_EffectContainer@name` | **解消** | 生成された 3 型の `UNIQUE_ATTRS` に `a:cont@name` と `a:effectDag@name`（`default: None`）が入った。<br>`detach_dml.rs` の `effect_container_names_are_refused` が、`a:rPr` 直下の `effectDag@name`、`effectDag/cont@name`、`a:pPr/a:defRPr/effectDag@name` の拒否と、名前のない container の複製を確かめている。<br>probe（E）で、テストにない経路も確かめた。すべて `UniqueAttribute` で拒否された。<br>・`a:rPr/a:blipFill/a:blip/a:alphaMod/a:cont@name`<br>・`a:pPr/a:defRPr/a:blipFill/a:blip/a:alphaMod/a:cont@name`<br>・`effectDag/blend/cont@name`<br>・`a:endParaRPr` の下の `effectDag@name`<br>・同じ名前空間に別の prefix（`q:`）を付けた `effectDag@name`<br>・`name=""`（拒否が増える側なので問題ない）<br>参照である `a:effect@ref` は、正しく複製される。<br>`dml.toml` のコメントと ADR-0002 note に、§20.1.8.20 / §20.1.8.25 と、検討した結果一意ではないもの（`bmk`、`a:effect@ref`、`a:snd@name`）が書かれた。 |
| R3-2 seal の文言 | **解消**（古い言い方が 2 か所に残る。n-a） | `schema.rs` の `Detachable` の doc は「This is **not** a seal」になり、下流のクレートでも両方のトレイトを実装できることと、それはサポート外であることが書かれた。ADR-0002 note も同じ。 |
| R3-3 mutants | **解消** | テストが 3 つ増えた。<br>・`destination_checks_cover_the_parent_itself_and_descendants`<br>・`the_ancestor_walk_does_not_stop_at_an_ignorable`<br>・`check_insert_under_saturates_and_names_the_parent`<br>DS5、DS6、DS8、N17 は、今回の実行ですべて検出された。 |
| R3-4 履歴 | 対応不要（記録済み） | 今回のコミットは、再生成した `dml.rs` を同じコミットに入れている。コミットメッセージにも R3-4 が書かれている。 |
| n1 生成器が黙って落とす | **解消** | `filter_map` → `map(...ok_or_else(...)).collect::<Res<_>>()?`。<br>読み込み側（`emit.rs:135-149`）も、次の場合はエラーになる。<br>・型名が解決できない<br>・属性がない<br>・SDK にその型の要素がない |
| n2 overflow | **解消** | `saturating_add` になった。`created = usize::MAX` で `TooDeep` になるテストがある。<br>mutant D6（`wrapping_add` に戻す）は検出された。 |
| n3 `WrongElement` の名前 | **解消** | `expected` は `(parent element at parent_path)` になった。テストもあり、mutant R3（`C::QNAME` に戻す）は検出された。 |

## Findings

Major / Minor はない。

### 細かい点（任意）

- **n-a（文言の残り）**: R3-2 で直した方針と食い違う言い方が 2 か所に残っている。
  - `crates/ooxml-core/src/schema.rs:237`: `/// Seal of [`super::Detachable`]; implemented by generated code only.` は、すぐ上の「not a seal … any crate can implement both traits (the core tests do)」と矛盾する。たとえば "Supertrait of `Detachable`, implemented by generated code (not a seal: see `Detachable`)." にするとよい。
  - `crates/ooxml-core/tests/detach.rs:49`: `see `Detachable`'s seal`。
  - トレイト名 `Sealed` 自体は、`#[doc(hidden)]` の中なので、そのままでもよい。
- **n-b（数）**: `dml.toml` のコメントと ADR-0002 note の「all 90 complex types reachable from the three types」は、数が合わない。
  - `reach.py` を、リポジトリの `reference/ecma-376-transitional-xsd`、scratchpad の `ecma/x/xsd`、`foundation/x-ecma-inner` の 3 つの XSD で走らせた。どれも 96 型（`CT_TextCharacterProperties` からは 81 型）だった。
  - 90 は round 3 のレビューで私が書いた数で、それが写されたものである。訂正する。
  - 結論（一意なのは 2 つだけ）は変わらない。直すなら「96」にする。
- **n-c（mutant、対応は不要）**: 次は生き残ったが、いずれも等価か、拒否が増える側である。
  - R4（`ParagraphProperties` の一覧から `cont` だけを落とす）、R6（`EndParagraphRunProperties` の一覧から 2 つとも落とす）
    - 生成器は 3 型に同じ一覧を出す。`detach_as` は、複製元と複製先の型の一覧をつないで使う（`E::UNIQUE_ATTRS.chain(C::UNIQUE_ATTRS)`）。
    - そのため、生成されたコードでは等価になる。U4、U5、G1（round 3）と同じ種類である。
    - 生成器が型ごとに違う一覧を出すようになったら、「3 型の一覧が等しい」ことを 1 行で assert すると、まとめて検出できる。
  - U6（local 名だけで照合する。拒否が増える側）、S2（ノード自身の `mc:Ignorable` は、MC 属性としてもともと拒否される）。round 3 と同じ。

## mutants の結果（62 個）

- 検出された: 55 個
  - round 1〜3 の M1〜M13、N1〜N17、D1〜D5、U1〜U3、X1、X2、DS1〜DS8、S1、E1
  - 新しい D6（n2）、R1（`RunProperties` の一覧から `effectDag` を落とす）、R2（`cont` を落とす）、R3（n3）、R5（`ParagraphProperties` の一覧から `effectDag` を落とす）
- 生き残った: 7 個。すべて等価か、拒否が増える側である（上の n-c）。
  - U4、U5、U6、G1、S2、R4、R6
- R3-3 で挙げた DS5、DS6、DS8、N17 は、すべて検出された。

## 改めて確かめて問題がなかったこと

- **重複する id**:
  - 仕様の全走査（上記）で、一意性を求められる値は `smtId` と effect container の `name` だけで、どちらも拒否される。
  - `bmk` と `a:effect@ref` は参照なので、重複してよい。
  - `a:snd@name` は表示名である。それに、`a:snd` は `r:embed` を持つので、リンクを skip しなければ `RelationshipAttribute` で拒否される。
- **宙に浮く relationship**: `r:`（Transitional と Strict）と `o:relid` は、引き続き拒否される（M5、N13〜N15 を検出）。
- **`Err` なのに変更が残る編集**: `insert_detached` は、slot、depth、`place`、複製先の検査、prefix の順に調べ、変更はそのあとに行う。この並びは変わっていない（DS1、DS2、D3、D4 を検出）。
- **途中まで適用される編集**: 1 回の `insert_detached` は、1 つの部分木を 1 回の `children.insert` で入れる。途中で失敗する経路はない。
