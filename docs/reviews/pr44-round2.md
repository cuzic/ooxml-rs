# PR #44 レビュー（Opus, round 2）

対象: PR #44 `core/move-subtree`（HEAD `5b8c0d7`、round 1 の指摘への修正 `b0b1bf9..5b8c0d7`）。検証は `git clone` したコピーで行った。
プローブは `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-44r2-45r2/probes44/probe_r2.rs` にある。
この中に round 1 のプローブ P1–P13 の移植、新しいプローブ、ランダムな brute force と独立オラクルを入れた。
mutant は `.../review-44r2-45r2/scripts/mut44.py` で作り、結果は `.../review-44r2-45r2/mut44.log` に残した。環境は Rust 1.99.1。

## 結論（先に）

M1 は解消した。属性値の中のプレフィックスは、移動先でも束縛が保たれる。

- 対象は `mc:Ignorable`/`MustUnderstand`/`ProcessContent`/`PreserveElements`/`PreserveAttributes` と `mc:Choice/@Requires`。
- 部分木の中の宣言と再宣言は、要素ごとのスコープで追跡している。
- round 1 の m1–m6 も、すべて解消または妥当な形で文書化された。

独立オラクルで検証した。

- 方法: 生のプレフィックスから束縛、Ignorable、`xml:space` を自前で解決する。移動の前と、出力を再パースした後で、部分木の意味を比べる。
- 規模: ランダムな文書 1500 個、296,916 通りの移動。
- 結果:
  - check と move は全件で一致した（エラー文言まで同じ）。
  - 意味が変わるのに受理した移動（under-refusal）は **0 件**だった。
  - 拒否した移動では、木は変わらず、dirty も立たない。

残りは Minor が 3 件と Nit が 3 件で、どれも誤った出力にはならない。

- 新しい rustdoc の「Batches」の主張は、条件付きでしか成り立たない（m1）。
- `MoveSource<S>` の `S` を実行時に照合していない（m2）。
- 安全を守る分岐のうち 2 つに、テストがない（m3。mutant が 2 体生き残る）。

**converged: yes**（Blocker/Major なし）。Minor は、マージ前に直すのが望ましいが、収束の条件ではない。

## 解消状況

| round 1 | 状態 | 根拠 |
|---|---|---|
| **M1** MC 属性値のプレフィックス | **解消** | `Usage::walk`（`move_subtree.rs` L105-155）が、5 つの MC 属性と `mc:Choice/@Requires` の値をトークンに分ける。各トークンの `:` の前を、未宣言のプレフィックスとして数える。P1・P2 は `NamespaceBinding` になる。追加プローブも同じ結果になった（Q3: `mc` 以外のプレフィックスで書いた MC 属性。Q4: タブ区切りの 2 トークン目だけが未束縛。Q5: `Requires` のプレフィックスが移動先で別の URI に束縛される）。Q2（`mc:AlternateContent` 自身に `xmlns:y`）は受理され、正しい。mutant N1/N2/N2b/N3/N11 は殺された |
| 再宣言されたプレフィックス | 解消 | `declared` に要素ごとの宣言を積み、`truncate(mark)` で戻している。P12 と Q1（兄弟の宣言が次の兄弟に漏れない）は正しい。ただし Q1 を確かめるテストがない（m3） |
| **m1** 変異体 A〜F の生存 | **解消** | A/B/C/D/F/G/H/I/K はすべて殺された（`mut44.log`）。brute force は文脈の違う木も回すようになった（`brute_force_over_differing_contexts`） |
| **m2** complex type の確認 | 解消（型で） | `MoveSource<S>` と `S: SameType<C>` を導入した。`compile_fail` の doctest が `SameType` の診断で失敗することを確かめた。同じ例を `RunRPr` で書くと通る。ただし `S` の名前を実行時に照合していない（m2'） |
| **m3a** 自前の宣言での過剰な拒否 | 解消 | P3 は `Ok` になる。#45 n4（移動先に同じ URI の宣言がある場合）もテストされている |
| **m3b** インデントでの過剰な拒否 | 解消 | P6 は `Ok` になる。要素の子を持つ要素の中の、空白だけのテキストを数えない。葉の空白（`<x:item>  </x:item>`）は引き続き数える。オラクルの比較でも、方針どおりの差しか出なかった |
| **m4** 古いパスと複数の移動 | 解消（文書） | rustdoc、README、ADR note に、パスがずれる範囲と「後ろから動かす」を書いた。テスト `moving_several_children_last_first` もある。ただし、新しく書いた「Batches」の段落が不正確である（m1） |
| **m5** `mc:AlternateContent` からの移動 | 解消 | `MoveRefusal::AlternateContent` を追加した。プローブで確かめた結果: 移動元が分岐の中なら拒否される（テストあり）。移動先が `mc:Choice` の場合と、`mc:Choice` の中の `x:item` の場合も拒否される（Q6/Q6b。テストなし → m3）。AC を**含む**要素の移動は受理される（Q7。正しい） |
| **m6** #18 との重複 | 解消 | `Scope::at`、`MC_IGNORABLE` の複製、独自の `element_at` と height を削った。代わりに `ancestor_context`/`declarations`/`effective`/`element_at`/`subtree_height`（lens.rs L223, L256, L294, L994, L1013）を使っている。#18 との `xml:space` の違い（move は両方向）は、ADR note に理由付きで書かれた |
| Nit（未使用の `let a` など） | 解消 | テストを書き直した |

## Findings

### m1（Minor）「Batches」の主張は、先行する移動が後の移動の祖先や部分木を変える場合に成り立たない

- 該当箇所:
  - rustdoc `move_subtree.rs` L179-185: 「`NamespaceBinding` … `TooDeep` depend only on the ancestors' attributes and the subtree, which no move changes: checked once, they stay valid」。
  - ADR-0002 note L240-244 も同じことを書いている。
- 根拠（`r2_batch_claim`）: どちらも、最初に事前検証して `Ok` だった移動が、先行する移動の後で拒否された。
  - **B1:** 次の 2 つを事前検証すると、どちらも `Ok` になる。
    - (1) `y` を使わない item A を box0（`y=urn:a`）から box1（`y=urn:b`）へ移す。
    - (2) `y:k` を持つ item I を A の子 D へ移す。
    - (1) を実行した後で (2) を実行すると、`NamespaceBinding` になる。D の祖先が変わったためである。
  - **B2:** 次の 2 つを事前検証すると、「A を T へ」は `Ok` になる。
    - B を A の中へ移す。
    - A を T の中へ移す。
    - 1 つ目を実行した後で 2 つ目を実行すると、`TooDeep` になる。A の高さが増えたためである。
- 主張が成り立つ場合もある。次の 2 つを満たせば正しい。
  - 移動される部分木の**中**の要素では、属性と宣言が一緒に動く。
  - 移動元の束縛と Ignorable は、自分の check で保証済みである。
- 実害は無い。apply が毎回 check し直すので、誤った出力にはならない。失われるのは原子性だけである。
  - ADR-0013 の `merge` は、セルの `a:p` を別のセルの `txBody` へ移す。移動先が、移動される部分木の中に入ることはない。だから、この利用者には影響しない。
  - それでも、note は「一度の事前検証で足りる」と一般化して読める。
- 推奨: 条件を 1 文で足す。例えば「… stay valid as long as no earlier move of the batch moves an ancestor of a later destination, or moves into a later moved subtree; otherwise check on a clone」。

### m2（Minor）`MoveSource<S>` の `S::QNAME` を照合していない

- 該当箇所: `plan_move`（L257-264）は、`element_at(.., C::QNAME)` と `node.is(C::QNAME)` だけを見ている。rustdoc（L34-35）は「the move then refuses a path that no longer leads to an `S::QNAME` element」と書いており、実装と合わない。
- 根拠（`r2_same_type_different_qname`）。テストの spec で、`impl SameType<RPr> for EndRPr` を定義した。
  - (a) `MoveSource<EndRPr>` を `move_in::<RPr>` に渡す。これはコンパイルが通るが、必ず `WrongElement { expected: rPr, found: endRPr }` になる。move は名前を変えられないからである。生成コードにも実例がある（`ooxml-schema/src/generated/dml.rs` L973: `SameType<RunProperties> for EndParagraphRunProperties`）。
  - (b) 古くなった `MoveSource<EndRPr>` のパスが `rPr` を指していると、受理されて移動する。このとき `S` は「その要素が `S` の型を持つ」証拠として働いていない。
- 誤った出力にはならない。移動されるのは `C::QNAME` の要素で、`C` のスロットに入るからである。ただし、型の約束が実行時に一段弱い。
- 推奨: `plan_move` を `S` についても generic にする。そのうえで `S::QNAME != C::QNAME` なら拒否し（rustdoc に「renaming pairs of `SameType` cannot be moved」と書く）、`node.is(S::QNAME)` を照合する。rustdoc の文言も実装に合わせる。

### m3（Minor）安全を守る分岐のうち 2 つにテストがなく、under-refusal 側の mutant が生き残る

| mutant | 結果 | 生き残ったときの挙動（プローブで確認） |
|---|---|---|
| N4: `declared.truncate(mark)` を削る（兄弟の宣言が漏れる） | **生存** | Q1 `<x:item><x:sub xmlns:y="urn:own"/><x:sub y:k="1"/></x:item>` を、`y` が無い box へ移す移動が `Ok` になる。出力の `y` は未束縛になる |
| N7: AC の判定を移動元だけにする | **生存** | Q6/Q6b: `mc:Choice` の中（または、その中の item）へ移す移動が `Ok` になる |
| N12: ルート要素自身の MC を見ない | 生存 | ルートは MC 要素にならないので、等価に近い。テストは不要 |
| N14: `s < index` を `s <= index` にする | 生存 | `skip == index` は起こらないので、等価（round 1 と同じ） |

- 推奨: Q1 と Q6（移動先が `mc:Choice`）をテストに加える。どちらも数行で書ける。プローブの `Ac`/`Choice` spec を流用できる。

### n1（Nit）部分木自身の `mc:Ignorable` で、過剰な拒否が起きる

- 独立オラクルの比較で、意味が変わらないのに拒否した移動が 1641 件あった。すべて `IgnorableNamespace` で、原因は 1 種類だった。
  - 部分木の要素自身が `mc:Ignorable="y"` を持つ。このため、`y` の URI は移動元でも移動先でも ignorable になる（例: seed 5 の i7、seed 48 の i5）。
  - それでも、祖先の Ignorable の状態だけを比べて拒否している。
- 安全側の拒否である。OOXML では `mc:Ignorable` は通常ルートにしか置かれない。doc に 1 文あれば足りる。

### n2（Nit）別の文書から取った `MoveSource` も黙って受理する

- Q8: 別の `XmlDocument` から取った `MoveSource` を渡すと、同じパスの同名の要素が移動する。`MoveSource` は文書に結び付いていない。
- 古いパスの注意（L32-35）に「or taken from another document」を足すだけでよい。

### n3（Nit）`AlternateContent` のエラーに載る名前

- `name` には、パスの最も深い要素が入る（Q6 では `mc:Choice`、Q6b では `x:item`）。どちらの場合も、AC 自身や移動する要素とは限らない。
- 移動する要素の名前か、AC の名前に揃えると読みやすい。

## 検証

- 再実行した round 1 のプローブの結果は次のとおり。いずれも期待どおりだった。
  - P1/P2/P4/P5 は拒否された。
  - P3/P5b/P6/P9/P12 は `Ok` になった。
  - P7 では、空白テキストが移動元に残った。
  - P8 は上限ちょうどで `Ok` になった。
  - P10 では、作成済みノードが正しく描画された。
  - P13 では、出力がバイト同一だった。
- 独立オラクルを使ったランダムな brute force（`random_brute_force_with_oracle`、`SEEDS=1500`）。
  - 規模は次のとおり。
    - 文書の生成要素: `xmlns:y` を 3 種、`mc:Ignorable`/`ProcessContent`/`MustUnderstand`、`xml:space` を 3 種、葉の空白と非空白のテキスト、インデント、`mc:AlternateContent` + `Requires`。
    - 移動の対象: すべての item を、すべての box と item の 3 位置へ移す。
  - 結果は次のとおり。
    - `Ok` は 167,787 件、`Err` は 129,147 件だった。
    - check と move の不一致は 0 件だった。
    - `Ok` の場合はすべて pure move だった。移動先が self-closing の 11,580 件は、再描画されるので除外した。出力はすべて再パースできた。
    - under-refusal は 0 件だった。
    - over-refusal は 1641 件で、内容は n1 のとおりだった。
    - インデントの `xml:space` が変わる移動は 4032 件受理された。これは方針（m3b）どおりである。
- mutant 25 体のうち、21 体が殺された。生き残ったのは N4・N7（m3）と、等価な N12・N14 である。
- `compile_fail` の doctest が、意図した理由（`SameType` の診断）で失敗することを確かめた。
- `cargo test -p ooxml-core` は、PR head で通った。
- 最新の `origin/main`（`ab1b0b6`）を試験的にマージした。コンフリクトは無かった。マージ後の `cargo test --workspace --all-features` は 369 件通過、失敗 0 だった。`cargo clippy -p ooxml-core --all-targets` の警告も 0 件だった。

## 判定

**converged: yes**（Blocker/Major なし）。

- m1（「Batches」の条件）と m2（`S::QNAME` の照合と rustdoc）は、文書かコードを数行直せば済む。
- m3 はテストを 2 件足せば済む。
- Nit 3 件は任意である。
