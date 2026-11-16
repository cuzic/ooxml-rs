# PR #19 レビュー（Opus, round 2）

対象: `impl/pptx-phase0.1-copy` の HEAD `ec41725`。round 1（`e340d8d`）以降のコミットは次の 4 つ。

- `9e4aa2e` core/detach-insert のマージ（#18 の `bb9a13c`、`2073ca9`、`c1ca8ae`）
- `ecddf62` ooxml-pptx: core の `smtId` / `extLst` の拒否に従う。N3、段落の depth 検査を正確にする。
- `bd40be9` T4 のテスト
- `ec41725` ADR-0005 rev 4（N5）

core 側の所見は `core/detach-insert` の `docs/reviews/pr18-round3.md` にまとめた。ここでは pptx 側への影響だけを書く。

検証は、worktree を scratchpad の `review-18r3-19r2/` にコピーして行った（リポジトリは読むだけ）。

- `cargo test --workspace --all-features`: すべて ok。`clippy -D warnings`: 警告なし。`codegen --check`: 差分なし。
- T2（`locality`）: 3,676 edits。拒否されたのは `MergedCell`、`LastParagraph`、`Animated*`、`FieldInParagraph` だけだった。**`FormatNotCopyable` は 0、`NotDetachable` も 0** で、複製先の検査を足しても拒否は増えていない。
- T3 の python 側: python-pptx 1.0.2 で `oracle_cases` を出力し、`check.py` で照合した。49 cases、1,580 edits、0 failures。
- probe:
  - `review-18r3-19r2/probes/zz_probe19r2.rs`: depth の fuzz
  - `zz_probe19r2b.rs`: 複製先の `xml:space`
  - `zz_probe19r2c.rs`: `a:p` で宣言された名前空間を `a:pPr` が使う場合
- mutants: `review-18r3-19r2/scripts/mut19r2.py`。`write.rs` に対して 33 個。

## 結論（先に）

- 判定: **converged: no**。ただし、残っている Major は #18 R3-1（`a:effectDag` / `a:cont` の `@name` の重複）を受け継いだ 1 件だけである。
  - pptx 側で書くコードはない。core の修正をマージし、T4 に 1 行、ADR §3 に 1 語を足せば収束する。
- round 1 の N1〜N5 は、すべて解消または実質的に解消した。
- depth の fuzz で原子性を確かめた。次の組み合わせで、計 2,310 回の編集を行った。
  - 本文 9 種類
  - 操作 22 種類（段落と本文の `set_text`、`add_paragraph`、`add_run`、`add_break`、run の `set_text`）
  - グループの入れ子 k = 236〜252
  - 結果、**`Err` なのに変更が残る編集は 0**、`Ok` で depth が 257 以上になるか parse できなくなる出力も 0 だった。
- 一方で、外しても既存のテストで検出されない事前検査が 2 つある。外すと、`Err` なのに変更が残る編集が実際に作れる（R2-2、Minor）。
  - `add_run` の depth 検査
  - `TextBody::set_text` の `a:pPr` / `a:endParaRPr` の複製に対する `check_copies`
  - どちらも、コードは正しい。足りないのはテストである。

## round 1 の解消状況

| 項目 | 状況 | 根拠 |
|---|---|---|
| N1 `smtId` | **解消**（core 側）。ただし同じ種類の R2-1 が残る | `UniqueAttribute` → `FormatNotCopyable`。T4 の `smart_tag_ids_and_extension_lists_are_not_copied` は、次を確かめている。<br>・3 つの操作が、変更なしで拒否される<br>・`smtId="0"` は複製される<br>・`a:pPr/a:defRPr@smtId` は拒否される<br>・`extLst` は拒否される |
| N2 depth の事前検査のテスト | **解消**。ただし同じ種類の残りが R2-2 | `depth_pre_checks_decide_alone` を追加した。mutant D1（段落の検査を外す）、D2（本文の検査を外す）、D3（本文の `a:rPr` の created を 2 から 1 にする）、D5 / D5b（`add_paragraph`）は検出された。<br>段落の検査は、作る段数だけを数えるようになった（`levels`）。これで、テキストだけを書き換える編集が depth を理由に拒否されることはなくなった。D11（常に 2 にする、つまり以前の挙動）も検出された。 |
| N3 使わない run の書式源を調べる | **解消** | `new_para_format(…, needs_run)` で直った。T4 の `an_unused_run_source_is_not_consulted` がある。N3a / N3b / N3c は検出された。N3d は残る（R2-3）。 |
| N4 書式の判定とリンクを外す処理 | **解消** | P4、P5、P8、P9、P10、P11 はすべて検出された（T4 の `end_paragraph_format_sources` と `ancestor_ignorable_namespaces_are_seen_through_the_slide`）。 |
| N5 文書 | **解消**。ただし細かい点が R2-4 | `error.rs` の doc と `#[error]` 文言、ADR §3 "Format sources" と "Hyperlinks"、§6 の行、§9 "Core primitive" が、#18 r2 の API と拒否の一覧に揃った。 |

## Findings

### R2-1（Major、まれ。#18 R3-1 を受け継ぐ）: `a:effectDag@name` / `a:cont@name` が、新しい run や段落に複製される

- ECMA-376 Part 1 §20.1.8.20 と §20.1.8.25 は、この属性について "Shall be unique across all effect trees and effect containers" と定めている。
- `a:rPr`、`a:endParaRPr`、`a:pPr/a:defRPr` のどれからでも届き、今は複製される（#18 の probe E1 で `name` が 2 個になった）。
- 頻度: corpus＋fixtures の 106 ファイルで 0 個。
- 提案:
  - core が `a:CT_EffectContainer@name` を `unique_attrs` に足したら、それをマージする。
  - T4 の `smart_tag_ids_and_extension_lists_are_not_copied` に、合成入力を 1 つ足す（`a:rPr` の下の `<a:effectDag name="E"/>` → `FormatNotCopyable("a:rPr")`）。
  - ADR §3 と §6 の拒否の一覧に「名前付きの effect container」を足す。

### R2-2（Minor、テスト）: 原子性を守る事前検査のうち 2 つを、外しても検出できない

mutant の結果:

- **D6 / D6b**: `ParagraphMut::add_run` の `check_depth(…, 2)` を外す、または 1 にする。
  - これを当てた状態で fuzz を回すと、`k=249` で `<a:p/>`（段落が depth 255）に `add_run("x")` を呼んだとき、`a:r`（256）が入り、`a:t`（257）で `TooDeep` になる。
  - 結果は **`Err` なのに変更あり**。本文が 1 段落の場合と 2 段落の場合の、2 ケースで起こった。
  - HEAD のコードでは、変更なしで拒否される。
- **P7b / P7c**: `TextBody::set_text` の `check_copies(doc, &self.path, 1, ppr … end …)` を外す、または created を 0 にする。
  - これを当てた状態で、タイトルの `a:p` が `xmlns:x14="urn:ext"` を宣言し、その `a:pPr` が `x14:k="1"` を持つ入力（`zz_probe19r2c.rs`）に、`set_text("a\nb")` を呼ぶ。
  - 新しい `a:p` を追加したあと、`a:pPr` の挿入で `NamespaceNotInScope` になる。結果は **`Err` なのに変更あり**。
  - HEAD のコードでは、`Err(Core(NamespaceNotInScope("urn:ext")))` で、変更はない。
- 提案: T4 に 2 つのケースを足す。
  - `nested_title(249, "<a:p/>")` に対する `add_run`
  - 上の `x14` の入力に対する `TextBody::set_text("a\nb")`
- 等価な mutant（対応は不要）:
  - D8 と D12（`first_lacks_t`）、D9（改行の段数）: どの場合も、最初の変更（`a:t` または `a:br` の挿入）そのものを core が拒否する。そのため、変更が途中まで残ることはない。D12 は拒否が増える側でもある。
  - K1: `detach` が `NotDetachable` 以外を返すことはない。

### R2-3（Minor、テスト）: `needs_run` の計算に対する mutant が生き残る（N3d）

- `needs_run` を「追加する行の、最初の segment が空でない」に弱めても、テストは通る。
- その場合、追加する行が `"\vb"` のときに `needs_run = false` となり、新しい run が `a:rPr` なしで作られる。データは壊れないが、書式が黙って失われる。
- T3 にも、このような行の形はない。
- 提案: 最後の段落の書式を持つ本文に `set_text("x\n\u{b}b")` を呼び、新しい run が `a:rPr` を持つことを確かめる assert を 1 つ足す。

### R2-4（Minor、文書と分類）: 複製先で拒否されると、`FormatNotCopyable` ではなく `Core(NotDetachable)` になる

- `check_insert_under` は、#18 で複製先の `mc:Ignorable` と `xml:space` も検査するようになった。しかし pptx の `CheckInsert` は、その結果を `Error::Core` に包んで返している。
- 再現（`zz_probe19r2b.rs`）:
  - タイトルの `a:p` に `xml:space="preserve"`、先頭の `a:r` に `xml:space="default"` を付ける。`a:rPr` は整形されていて、空白のテキストを持つ。
  - ここで `Paragraph::set_text("a\vb")` を呼ぶと、`Err(Core(NotDetachable { reason: InheritedXmlSpace, … }))` になる（変更はない）。
  - `TextBody::set_text("a\nb")` は、新しい段落が preserve の外に作られるので、`Ok` になる。これは正しい。
- `ParagraphMut::set_text` の doc と ADR §6 に、この error は載っていない。ADR §3 の「`detach_as` が拒否する場合に限り `FormatNotCopyable`」という記述は、文字どおりには正しい。しかし呼び出し側から見ると、同じ「書式を複製できない」が 2 種類の error に分かれている。
- 頻度: corpus に `xml:space` は 0 個である。
- 提案: `CheckInsert::check` で `NotDetachable` を `FormatNotCopyable(what)` に写す。あるいは doc と §6 に `Core(NotDetachable)` を足す。

### R2-5（Minor、任意）: 新しい空の段落でも、depth を 3 段分要求する

- fuzz で、拒否が過剰になる例が 5 つあった。すべて k=249（本文が depth 254）で、空の段落を作るだけの編集である。
  - `add_paragraph("")`
  - `TextBody::set_text("\n")`、`TextBody::set_text("\n\n")`
- 実際に作るのは `a:p`（1 段）と、同じ depth の書式の複製だけである。しかし `check_depth(body, 3)` は固定の 3 段を要求する。
- 拒否が増える側なので、害はない。
- ADR rev 4 の注記は「段落の検査は作る段数だけを数える」と言っていて、これは段落については正しい。本文の検査と `add_paragraph` はまだ固定値であることを 1 行書くか、段落と同じく作る段数で数えるとよい。

### R2-6（Minor、履歴）: いくつかのコミットで、ビルドまたはテストが通らない

- 次のコミットでは、T4 の `copied_namespaces_must_be_in_scope_before_any_change` が落ちる（refusals.rs:441）。どちらもビルドは通る。
  - `9e4aa2e`（マージ）
  - `ecddf62`
- 理由: core が `extLst` を先に拒否するようになったのに、テストの入力（`a:extLst` の中に `x14:k`）を直したのが、次の `bd40be9` だからである。
- また、マージで取り込んだ #18 の `bb9a13c` は、単独ではビルドできない（#18 R3-4）。
- 判断: リポジトリの方針（force-push しない）を考えると、履歴は書き直さず、bisect のときは skip すればよい。今後は、core の挙動の変化に追随するテストの修正をマージコミットか同じコミットに入れると、各コミットで `cargo test` が通る。

## 確かめて問題がなかったこと

- **depth の fuzz**（`zz_probe19r2.rs`）:
  - 本文は次の 9 種類。
    - 空の `<a:p/>`
    - `a:t` を持つ run
    - `a:t` を持たない run
    - 深い `a:rPr`（`solidFill/srgbClr/alpha`）
    - 深い `a:endParaRPr`
    - 深い `a:pPr/a:defRPr` と `a:endParaRPr`
    - それらと `a:br` の混在
    - 2 段落
    - 空の 2 段落
  - 操作は 22 種類、k は 236〜252 の範囲で動かした。
  - `Err` なのに変更が残るものは 0、`Ok` で depth が 257 以上か parse できなくなるものも 0 だった。
  - 段落の `levels` は、次のとおり作られるものと一致する。
    - `a:r` と `a:t`、または既存の run に足す `a:t`: 2
    - `a:br` だけ: 1
    - テキストの書き換えだけ: 0
  - 本文側の複製（`created` が 1 か 2）は、core の式と一致する。
- **書式源の規則**（N3 / N3-1）:
  - `needs_run` が偽なら、`a:rPr` は作らない。
  - 最後の段落自身の `a:endParaRPr` に書式があれば、それを使う。ない場合だけ run の書式源を調べる。
  - T4 と P3 / N3a〜c で確かめた。
- **python の oracle の独立性**: `tests/oracle/` は round 1 から変わっていない（`git diff e340d8d..HEAD` に含まれない）。python 側は今も、python-pptx の lxml ツリーの上で、ADR の規則から独立に期待値を作っている。0 failures だった。限界は round 1 と同じで、両者が共有する規則の誤り（`smtId` や effect container の名前のような一意性）は検出できない。その部分は T4 の合成入力が担っている。
- **拒否が増えていないこと**: T2 で `FormatNotCopyable` も `NotDetachable` も 0 だった。corpus＋fixtures の書式要素 13,497 個の中に、リンクの外の `extLst` は 0 個、0 でない `smtId` も 0 個、`xml:space` も 0 個だった。

## 判定

**converged: no**。

- #18 R3-1 の core の修正をマージし、R2-1 の T4 の 1 ケースと ADR の 1 語を足せば収束する。
- R2-2（原子性を守る事前検査のテスト 2 つ）は、同じ PR で足すことを勧める。
- R2-3〜R2-6 は Minor である。
