# PR #19 レビュー（Opus, round 3）

対象: `impl/pptx-phase0.1-copy` の HEAD `6869825`。round 2（`ec41725`）以降のコミットは次の 3 つ（ほかに review 文書の `685624e`）。

- `1b95c06` core/detach-insert のマージ（#18 の `53df28f`、`cca838e`）
- `6869825` ooxml-pptx: 名前付きの effect container を複製しない（R2-1）、複製先での拒否を `FormatNotCopyable` にする（R2-4）、テスト（R2-2、R2-3）、ADR の注記（R2-5）

core 側の所見は `core/detach-insert` の `docs/reviews/pr18-round4.md` にまとめた。#19 の core 部分（`crates/ooxml-core`、`ooxml-schema`、`xtask`、`codegen`）は `cca838e` と差分 0 である。

検証は、worktree を scratchpad の `review-18r4-19r3/` にコピーして行った（リポジトリは読むだけ）。

- `cargo test --workspace --all-features`: すべて ok。`clippy --all-targets -D warnings`: 警告なし。`codegen --check`: 差分なし。
  - マージ `1b95c06` を単独で展開しても、`cargo test --workspace --all-features` は失敗 0 だった。
- **T2**（`locality`）: 3,676 edits。
  - 拒否されたのは `MergedCell`、`LastParagraph`、`AnimatedParagraphs`、`FieldInParagraph` だけだった。
  - **`FormatNotCopyable` は 0**、`Core(…)` も 0 だった。
- **T3 の python 側**: python-pptx 1.0.2（scratchpad の `pptx-py`）で照合した。
  - `cargo run --example oracle_cases` の出力を `check.py` に渡した。
  - 結果は **49 cases、1,580 edits、0 failures**。
  - `tests/oracle/` の最後の変更は `c6d8b4a`（round 1 より前）のままである。独立性は round 1〜2 と変わらない。python 側は python-pptx の lxml ツリーの上で、ADR の規則から独立に期待値を作っている。
- probe: `review-18r4-19r3/probes/zz_probe19r3.rs`
  - 複製先での拒否の分類と名前（`p:txBody` の `xml:space`）
  - effect container の名前（`a:pPr/a:defRPr`、`blend/cont`）
- mutants: `review-18r4-19r3/scripts/mut19r3.py`。`write.rs` などに対して 37 個（round 2 の 32 個＋新しい 5 個）。

## 結論（先に）

- 判定: **converged: yes**。Major はない。
- R2-1〜R2-5 は、すべて解消した。
  - R2-2 の D6、D6b、P7b と、R2-3 の N3d は、新しいテストで検出されるようになった。
  - P7c は今も生き残る。ただし、これは **等価な mutant** だった。round 2 で「`Err` なのに変更が残る」と書いたのは誤りで、ここで訂正する（下の「mutants」）。
- 新しい所見は Minor が 1 件（R3-1）である。
  - R2-4 の修正（複製先での拒否を `FormatNotCopyable` に写す）の副作用で、P10（複製元の lens を `Lens::at` から `Lens::root` にして、祖先を見なくする）が検出されなくなった。
  - コードは正しい。足りないのはテストで、1 ケース足せば直る。
- 重複する id、宙に浮く relationship、`Err` なのに変更が残る編集、途中まで適用される編集は、どれも見つからなかった。

## round 2 の解消状況

| 項目 | 状況 | 根拠 |
|---|---|---|
| R2-1 effect container の名前 | **解消** | core の `cca838e` をマージした。<br>T4 の `smart_tag_ids_and_extension_lists_are_not_copied` に、`a:rPr/a:effectDag@name` → `FormatNotCopyable("a:rPr")`（変更なし）が足された。<br>probe で、テストにない経路も確かめた。<br>・`a:pPr/a:defRPr/a:effectDag/a:cont@name`: `TextBody::set_text("Lorem\nb")` と `("Lorem\n")` が `FormatNotCopyable("a:pPr")`（変更なし）になる。`Paragraph::set_text("a\vb")` は、`a:pPr` を複製しないので `Ok` になる。これは正しい。<br>・`a:rPr/a:effectDag/a:blend/a:cont@name`: `FormatNotCopyable("a:rPr")`。<br>ADR §3、§6 と `error.rs` の doc・`#[error]` に、名前付きの effect container が足された。 |
| R2-2 `add_run` の depth と本文の複製の検査 | **解消** | `add_run_depth_and_body_copy_checks_decide_alone` で、D6、D6b、P7b が検出されるようになった。P7c は等価だった（下記）。 |
| R2-3 `needs_run`（N3d） | **解消** | `a_new_line_starting_with_a_break_still_gets_the_run_format` で、N3d が検出されるようになった。 |
| R2-4 複製先での拒否の分類 | **解消**（名前に細かい点。n-a） | `CheckInsert::check` が、`NotDetachable` を `FormatNotCopyable(element_name(C::QNAME))` に写すようになった。depth と名前空間の拒否は `Core(…)` のままである。<br>`destination_refusals_of_a_copy_are_format_not_copyable` で、mutant C1（写すのをやめる）と C2 が検出された。<br>probe で、`p:txBody xml:space="preserve"`、先頭の `a:r xml:space="default"`、整形された `a:rPr` という入力を試した。<br>・段落の `set_text("a\vb")` と、本文の `set_text("Lorem\nb")` は `FormatNotCopyable` になり、変更はない。<br>・`set_text("Lorem\n")` は、自分の `a:endParaRPr`（テキストなし）を複製するだけなので `Ok` になる。これは正しい。<br>ADR §3 と §6 も更新された。 |
| R2-5 空の段落の 3 段 | **解消**（文書化） | ADR の注記に、本文側の検査が固定 3 段であることと、それは拒否が増える側として残すことが書かれた。 |
| R2-6 履歴 | 対応不要 | 今回のマージ `1b95c06` は、単独で全テストが通る。 |

## Findings

### R3-1（Minor、テスト）: R2-4 のあと、祖先を見る複製（P10）を守るテストが効かなくなった

- mutant **P10**（`detach` で `Lens::<S>::at(root, &path)` の代わりに `Lens::<S>::root(node)` を使い、複製元の祖先の `mc:Ignorable` と `xml:space` を見なくする）が生き残る。round 2 では検出されていた。
- 理由:
  - `ancestor_ignorable_namespaces_are_seen_through_the_slide` は、コメントで「N4 (P10)」と名乗っている。しかし、`mc:Ignorable` を `p:sld` に置いている。
  - `p:sld` の Ignorable は、複製先（同じ段落の新しい `a:r`）の祖先でもある。そのため、複製元で見落としても、`check_insert_under` が拒否する。
  - R2-4 より前は、この複製先での拒否が `Core(NotDetachable)` になり、`FormatNotCopyable("a:rPr")` を期待する assert が落ちていた。R2-4 で同じ error に写すようになったので、区別できなくなった。
- 実害の再現（P10 を当てたコピー）:
  - 入力: `p:sld` が `xmlns:e="urn:ext"` を宣言する。タイトルの先頭の **`a:r` 自身**に `mc:Ignorable="e"` を付け、その `a:rPr` に `e:k="1"` を付ける。
  - HEAD: `Paragraph::set_text("a\vb")` も `TextBody::set_text("a\nb")` も、`FormatNotCopyable("a:rPr")` で変更なし。
  - P10: どちらも `Ok`。`e:k="1"` が 2 個になる。新しい `a:r` には Ignorable がないので、ignorable だった属性が ignorable でない場所に複製される。
  - 複製元の `a:r` に `xml:space="preserve"` を付け、整形された `a:rPr` を持たせる場合も同じである。HEAD では拒否され、P10 では `Ok` になる。
- コードは正しい（HEAD では拒否される）。足りないのはテストである。
- 提案: 上の入力（`mc:Ignorable` を先頭の `a:r` だけに置く）を、`ancestor_ignorable_namespaces_are_seen_through_the_slide` に足すか、その入力に置き換える。これで P10 が再び検出される。

### 細かい点（任意）

- **n-a（名前）**: 複製先での拒否は、複製元ではなく、複製の型の名前を返す（ADR に "naming the copy's element" と書かれている）。そのため、文書にない要素の名前が出ることがある。
  - probe: `p:txBody xml:space="preserve"` で、最後の段落に `a:endParaRPr` がなく、run の `a:rPr` が整形されている入力に `TextBody::set_text("Lorem\n")` を呼ぶ。
  - 結果は `FormatNotCopyable("a:endParaRPr")` になる。複製元は `a:rPr` で、段落に `a:endParaRPr` はない。
  - 同じ複製元を複製元の側で拒否した場合は、`"a:rPr"` になる。
  - 直すなら、`Detached` と一緒に `what`（複製元の名前）を持ち回る。直さないなら、`error.rs` の doc に「複製先での拒否は、複製の要素名」と 1 行足す。
  - あわせて、`error.rs` の `FormatNotCopyable` の doc は、今も「the copy primitive refuses (`detach_as`)」としか書いていない。`check_insert_under`（複製先）も対象であることを足すとよい。
  - ADR §3 の「exactly where `detach_as` refuses」も、後ろの文で複製先が足されたので、「where `detach_as` (or, at the destination, `check_insert_under`) refuses」とすると矛盾がなくなる。
- **n-b（テスト）**: mutant C3、C4（`element_name` の `endParaRPr` / `pPr` の腕を変える）が生き残る。複製先での拒否を確かめるテストが `a:rPr` の 1 ケースだけだからである。上の n-a の入力に対して `"a:endParaRPr"` を確かめる assert を足せば、C3 が検出される。

## mutants の結果（37 個）

- 検出された: 28 個
  - P1〜P9、P11、P7、P7b
  - D1〜D7、D10、D11
  - N3a〜N3d
  - C1、C2
- 生き残った: 9 個
  - **P10**: 実際に見落としが起こる（R3-1）。
  - **P7c**（本文の `a:pPr` / `a:endParaRPr` の検査で、`created` を 1 から 0 にする）: **等価**。round 2 の記述を訂正する。
    - 新しい段落の `a:pPr` / `a:endParaRPr` は、複製元（最後の段落の、同じ名前の子）と同じ depth に入る。run の `a:rPr` から `a:endParaRPr` を作る場合は、複製元のほうが深い。
    - どちらにしても、複製元が parse できた以上、depth の項は検査を決めない。
    - 名前空間と複製先の検査は、`created` に依存しない。
    - 確認: `k = 246〜250` で、`a:endParaRPr` か `a:pPr/a:defRPr` に 3 段の書式を持たせた本文に `set_text("\n")` を呼んだ。HEAD と P7c で結果は同じだった。`k = 248` では、入力そのものが depth 257 で parse 時に拒否される。
    - round 2 で `Err` なのに変更が残ったのは、P7b（検査を丸ごと外す）のほうである。
  - **D8、D9、D12、K1**: round 2 と同じく等価か、拒否が増える側である。
  - **C3、C4**: n-b。
  - **G1**（生成された `RunProperties` の一覧から `effectDag` を落とす）: 生成されたコードでは等価である。
    - `run_format` は、同じ `a:rPr` を `EndParagraphRunProperties` としても複製する。そのため、もう一方の型の一覧で拒否される。
    - 生成器は 3 型に同じ一覧を出す（#18 round 4 n-c）。

## 改めて確かめて問題がなかったこと

- **`Err` なのに変更が残る編集**: R2-4 の写しは `map_err` だけで、検査の順序は変わらない。probe の 4 種類の入力 × 5 操作で、`Err` はすべて変更なしだった。
- **重複する id**: `smtId`（0 以外）と、effect container の `name` は、`a:rPr`、`a:endParaRPr`、`a:pPr/a:defRPr` のどこから届いても拒否される。仕様の全走査（#18 round 4）で、ほかに一意性を求められる属性はなかった。
- **宙に浮く relationship**: 直下の `a:hlinkClick` / `a:hlinkMouseOver` は skip され、それ以外の `r:*` は拒否される（P1、P2、P9、P11 を検出）。
- **途中まで適用される編集**: depth の事前検査（D1〜D7、D10、D11）と複製の事前検査（P6、P7、P7b）は、どれも単独で効いている。生き残った D8、D9、D12、P7c は等価である。
- **拒否が増えていないこと**: T2 で `FormatNotCopyable` は 0。round 3 の corpus の走査では、テキストの書式の下にある `effectDag` / `cont` は 0 個だった。

## 判定

**converged: yes**。

- R3-1（P10 を検出するテストを 1 つ）は、同じ PR で足すことを勧める。
- n-a と n-b は任意である。
