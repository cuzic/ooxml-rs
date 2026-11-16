# PR #18 レビュー（Opus, round 2）

対象: `core/detach-insert` の HEAD `8413683`。round 1（`89ac802`）以降のコミットは次の 4 つ。

- `b532b7e`
- `6e97fd1`
- `b302c0b`
- `8413683`: main（PR #16 depth limit）のマージ。

PR #19（`impl/pptx-phase0.1-copy` の `e340d8d`）は、この API の使い方を確かめるためにだけ読んだ。

検証には、worktree を scratchpad の `review-pr18-19/` にコピーしたものを使った。

- `cargo test --workspace --all-features`: 24 バイナリすべて ok。
- `cargo xtask codegen --check`: 差分なし。
- probe: `review-pr18-19/probe18.rs` と、PR #19 側の `probe19.rs`。
- mutants: `review-pr18-19/mutants2.py`。35 個。

## 結論（先に）

- 判定: **converged: no**。残っている Major は R2-1 の 1 件だけである。
- round 1 の F1 と F2 は、設計としては解消している。
  - F1 は `[detach]` 許可リストと `Detachable` marker で直された。
  - F2 は祖先の `mc:Ignorable` を拒否することで直された。
- m1〜m5 も解消した。m4 は depth を含めて解消した。
- 新しい問題が 1 つある。許可リストに載った `a:CT_TextCharacterProperties` は `@smtId` を持つ。ECMA-376 では、これは文書内で一意であるべき smart tag ID である。そのため「`[detach]` に載る型は文書内で一意な値を持たない」という保証が、載っている 3 型すべてで成り立たない（R2-1）。
  - 実際のファイルでこの値が使われることはまれである（後述の corpus では 0 件）。
  - それでも、この PR の中心的な主張（Detachable はレビュー済みで、一意な ID を含まない）を偽にするので Major とする。
- それ以外は Minor（R2-2〜R2-6）。
- `check_insert_under` と `insert_detached` の結果が一致するかを総当たりで確かめた。名前空間と depth について、created 0 と 1、depth 上限 2〜7 の 80 ケースで、不一致は 0 だった。

## round 1 の解消状況

| 項目 | 状況 | 確認内容 |
|---|---|---|
| F1 ID の重複 | **解消（ただし R2-1）** | `Detachable` は `[detach]` に載った `a:rPr` / `a:endParaRPr` / `a:pPr` にだけ生成される（`generated/dml.rs`）。`detach_as` は `E: Detachable, C: SameType<E> + Detachable` を要求する。`p:sp` / `p:grpSp` / `p:spTree` / `p:txBody` の `SameType` は消えた。`compile_fail` doctest は、その理由（`` `Shape` is not detachable ``）で失敗することを確かめた（スニペットを通常のテストとしてビルドした）。`[detach]` に載せるには `[write]` か `[read]` にも載っている必要がある（`config.rs`）。 |
| F2 ignorable 名前空間 | **解消（複製元の側）**、複製先の側は R2-3 | 祖先の `mc:Ignorable` を、その要素で有効な宣言を使って URI に解決している（`lens.rs:213-245`）。probe Q3 で次を確かめた。prefix を下で別の URI に束縛し直した場合は許可され、元の URI を別の prefix で使った場合は拒否される。親の `x:r` 自身に付いた Ignorable も拒否される。 |
| m1 Strict / VML の関係属性 | 解消 | `RELATIONSHIP_ATTRIBUTES` を追加した。`o:relid` だけが対象で、`o:title` は対象外である。どちらもテストがある。 |
| m2 継承される `xml:space` | 解消（拒否する方針） | `InheritedXmlSpace` を追加した。祖先で `preserve` のあと `default` に戻る場合や、ノード自身が `default` の場合は Ok になる（probe Q5）。 |
| m3 ミューテーション | 解消 | round 1 の 13 個はすべて検出された（M1、M2、M11 を含む）。`same_type_partners` に xtask の単体テストがある。 |
| m4 使い勝手 | 解消 | `insert_detached(&Detached)` で参照を渡すようになった。`check_insert_under(doc, path, created)` を追加した。pptx 側の自作の `check_scope`（約 60 行）は消えた。 |
| m5 文書 | 解消 | `NotDetachable { reason, name }` の表記を直した。`SameType` の doc で、`cast` が前提であることを明記した。`x:t` と `x:v` の組は消えた。 |

## Findings

### R2-1（Major、まれ）: Detachable な `a:CT_TextCharacterProperties` に、文書内で一意な `smtId` が含まれる

- 根拠:
  - XSD の `dml-main.xsd:2941` に `smtId` がある（`xsd:unsignedInt`, default `0`）。
  - 生成された `RunProperties::ATTR_SMT_ID` は `generated/dml.rs:706` にある。
  - ECMA-376 Part 1 §21.1.2.3.9（`rPr`）の `smtId` の説明は、要旨が「smart tag の ID。プレゼンテーション全体で一意であり、smart tag の補助情報を参照する」である。この文言は記憶に基づくので、仕様書で確かめてほしい。
  - `a:pPr` も、`a:defRPr` を通じて同じ CT を持つ。
  - 再現: PR #19 の probe `p1_smt_id_duplicated`。`smtId="4242"` を持つ `a:rPr` に対して `Paragraph::set_text("a\vb\vc")` または `TextBody::set_text("a\nb\nc")` を呼ぶと、成功し、`smtId="4242"` が 3 つになる。
  - `[detach]` 節のコメント（`dml.toml`）、`Detachable` の doc、ADR-0002 note は、どれも「文書内で一意な値を持たないことをレビューで確かめた」と言っている。その主張がこの属性で崩れている。
  - 頻度: scratchpad の corpus（pptx 83 ファイル）と `reference/fixtures/pptx` で、`smtId` が 0 でないスライドは 0 件だった。smart tag は Office 2010 以降で廃止されている。
- 提案（いずれか）:
  - (a) 型ごとの除外を許可リストに書けるようにする。たとえば `[detach] unique_attrs = ["a:CT_TextCharacterProperties@smtId"]` とし、`detach_as` が値 `0` 以外を `NotDetachable` で拒否する。
  - (b) 最小限の対応: PR #19 側で `smtId` が `0` 以外なら `FormatNotCopyable` にする。あわせて `[detach]` のコメントと ADR-0002 note に、レビューで見つけた例外として `smtId` を書く。
  - どちらの場合も、`bmk`（bookmark のリンク先の名前）は一意な ID ではなく参照なので、複製してよいと明記する。

### R2-2（Minor）: `a:extLst` は開いた内容なので、レビュー済みという主張が及ばない

- 根拠: probe P2 で、`a:rPr/a:extLst/a:ext` 内の `<q:tag q:id="77" q:target="rId9"/>` が拒否されずに複製された。
  - 関係 ID は、`r:` 名前空間とそれに準ずる属性を使う限り拒否される。
  - しかし、未知の拡張が `r:` 以外の属性に ID や関係 ID を入れていると、それは検出できない。
- 提案: `[detach]` のコメントと ADR note に、レビューの範囲はスキーマが知っている内容だけであり、`extLst` の中身は `RELATIONSHIP_ATTRIBUTES` と MC の拒否だけで守られる、と 1 行書く。

### R2-3（Minor）: 複製先の側の ignorability と `xml:space` を検査していない。doc はそれを検査しているように読める

- 根拠:
  - probe Q2: `e:new` を ignorable でない複製元から取り、`mc:Ignorable="e"` の祖先を持つ複製先へ入れると、`Ok` になる。
  - probe Q5: `xml:space` が default の複製元から、preserve の複製先へ入れると、`Ok` になる。空白だけのテキストが、意味のある値になる。
  - `error.rs:155-157` の `IgnorableNamespace` の doc は「(or, at the destination, gain) ignorability」と書いている。しかし、複製先で ignorable になる側を `insert_detached` は検査していない。
  - PR #19 の使い方では、複製先の祖先は「body より上（複製元と共通）＋新しく作った要素」なので、このケースは起こらない。文書をまたぐコピーで初めて起こる。
- 提案: 次のどちらか。
  - doc を、拒否するのは複製元の側だけで、複製先の側は呼び出し側の義務だ、と直す。
  - `insert_detached` / `check_insert_under` に対称な検査を足す。

### R2-4（Minor）: 新しいコードのミューテーションで、5 個が検出されない

全 35 個のうち 30 個が検出された。round 1 の 13 個と、depth に関する D1〜D5 はすべて検出された。生き残ったのは次の 5 個である。

- **N11**: `Lens::at` から `check_root::<E>` を外す。テストがない。これを外すと、任意の要素を `RunProperties` などとして detach できてしまう。安全性に関わる唯一のもの。probe Q4 は、今の実装が正しく `WrongElement` を返すことを示している。
- **N7**: `mc:Ignorable` の prefix を、最も外側の宣言で解決する（正しくは最も内側）。probe Q3 は今の実装が正しいことを示しているが、テストがない。
- **N6 / N9 / N17**: `xml:space="default"` で preserve を解除する処理、`xml:space` と Ignorable の組み合わせ。いずれも拒否が増える方向だけの差なので、安全側である。
- 提案:
  - `Lens::at` に間違った要素の path を渡すと `WrongElement` になることを確かめる assert を 1 つ足す。
  - probe Q3（prefix の束縛し直し）をテストにする。

### R2-5（Minor）: `Detached::check_insert_under` が、不正な path で panic する

- 根拠:
  - probe Q1: 範囲外の index や、テキストを指す path を渡すと、`lens.rs` の `node_at` / `declarations` の `expect` / index で panic する。
  - 同じ入力に対して、`Lens::at` は `Err(WrongElement)` を返す。
  - 公開 API なので、`Result` を返すほうが揃う。
- 付記:
  - `check_insert_under` は `doc.max_depth()` を使う。一方 `LensMut::root(raw)` で作った lens は `DEFAULT_MAX_DEPTH` を使う。
  - そのため、両者の結果が一致するのは、`XmlDocument::lens_mut` で作った lens に対してだけである。
  - doc に一言書くとよい。PR #19 は `lens_mut` を使っているので問題はない。

### R2-6（Minor、文書）: `Detachable` は sealed ではない

- 下流のクレートは、QName が `p:sp` の自前の `ElementSpec` を作り、それに `Detachable` と `ChildOf<pml::ShapeTree>` を実装できる（Self が自分の型なので孤児規則に当たらない）。そうすれば図形を detach できる。
- 意図した逃げ道（テストの `impl Detachable for R`）なので、それ自体は構わない。
- 提案: `Detachable` の doc に「保証の対象は生成された型だけ。手で実装する場合の責任は実装者にある」と書く。

## 確かめて問題がなかったこと

- depth（PR #16 のマージ分）:
  - `insert_detached` は、slot の判定の後、`place` の前に `path.len() + 1 + height > max_depth` を検査する。これは変更より前である。
  - `check_insert_under` の式（`parent_path.len() + 1 + created + height`）は、`created` 段の要素を作った後に挿入する場合と、正確に等価である。
  - D1〜D5 はすべて検出された。
  - 総当たりの結果（`probe18.rs` の `q6`）は不一致 0 だった。
  - prefix の Ok / Err は親の prefix に依存しない（`element_prefix_in` の近道は、走査でも必ず見つかる束縛にだけ当たる）。そのため、`created` 段の下でも名前空間の判定は変わらない。
- 拒否はどれも変更の前に起こる。`detach_as` は読むだけである。
- `Lens::at` は、範囲外、テキストを指す path、別の要素のいずれでも `Err` を返し、panic しない（probe Q4）。
- `has_text` は空白だけのテキストも数える。そのため、継承された preserve の下にある整形された `a:rPr` は拒否される（Q5 の最後の例）。拒否が増える方向であり、corpus に `xml:space` を含むスライドは 0 件なので、実害はない。

## owner の問い（PR #19 と共通）: 継承された `xml:space` を拒否してよいか

賛成する。理由は次の 3 つ。

- `CT_TextCharacterProperties` / `CT_TextParagraphProperties` は `xml:space` を宣言していない。複製のルートに付け直すと、スキーマに反する。
- 拒否が起こるのは、祖先が preserve で、かつ subtree にテキストがある場合だけである。DrawingML の書式要素には、もともとテキストの内容がない。
- corpus（83 ファイル）と fixtures で、`xml:space` を含むスライドは 0 件だった。

## 判定

**converged: no**。

- R2-1（`smtId`）を、(a) core の許可リストで除外するか、(b) PR #19 側で拒否してこの PR の文書に例外として書くか、どちらかで扱えば収束する。
- R2-2〜R2-6 は Minor で、同じ PR で直しても後回しにしてもよい。
