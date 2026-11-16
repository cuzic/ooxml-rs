# PR #19 レビュー（Opus, round 1）

対象: `impl/pptx-phase0.1-copy` の HEAD `e340d8d`。PR #18 の `8413683` と、main（PR #16 depth limit）をマージ済み。ADR-0005 revision 4 §9 Phase 0.1 にあたる。

core 側の所見は `core/detach-insert` の `docs/reviews/pr18-round2.md` にまとめた。ここでは pptx 側への影響だけを書く。

検証には scratchpad の `review-pr18-19/` を使った。

- `cargo test --workspace --all-features`: すべて ok。
- `clippy -D warnings`: 警告なし。
- T3 の python 側: python-pptx 1.0.2 で `check.py` をローカルで走らせ、49 cases、1580 edits、0 failures。
- probe: `probe19.rs` の P1〜P8。
- mutants: `mut19.py` で `write.rs` に 16 個。

## 結論（先に）

- 判定: **converged: no**。Major が 1 件ある（N1: `smtId` の重複。原因は PR #18 の R2-1 と同じ）。
- それを除けば、ADR-0005 §9 のとおりに動く。確かめた点は次のとおり。
  - 書式は subtree ごと、そのまま複製される。外すのは直下の `a:hlinkClick` / `a:hlinkMouseOver` だけである。
  - N3-1 の規則を守っている。最後の段落自身の `a:endParaRPr` に書式があればそれを使い、なければ run の書式源を使う。
  - 複数の段階からなる編集は、どれも「全部を計画し、全部を検査してから変更」の順になっている。拒否されたときは何も変わらない。これは depth 上限ちょうどのケースでも成り立つ（P6〜P8）。
  - コメントのガードは fail closed になっている。
- ただし、depth の事前検査のうち 2 つ（段落の `check_depth`、本文の `a:rPr` の `created = 2`）は、外しても既存のテストで検出されない。外すと、`Err` なのに変更が途中まで残る編集が実際に作れる（N2、Minor）。コードは正しいので、足りないのはテストである。

## Findings

### N1（Major、まれ）: 新しい run や段落に、`a:rPr@smtId` が複製される

- 根拠:
  - probe P1: `poi/sample.pptx` の slide 1 で、タイトルの `a:rPr` に `smtId="4242"` を足す。
  - その上で `Paragraph::set_text("a\vb\vc")` を呼ぶと成功し、`smtId="4242"` が 3 つになる。`TextBody::set_text("a\nb\nc")` でも同じく 3 つになる。
  - ECMA-376 Part 1 §21.1.2.3.9 は、`smtId` をプレゼンテーション内で一意な smart tag ID と定めている（文言は記憶に基づくので、仕様書で確かめてほしい）。
  - T3 の Rust 側と python 側は、どちらも「subtree をそのまま複製する」規則に従って期待値を作る。そのため、この重複を検出できない。
  - 頻度: 0 でない `smtId` は、corpus の 83 ファイルにも fixtures にもなかった。
- 提案: 複製元（`a:rPr`、`a:endParaRPr`、`a:pPr/a:defRPr`）に `smtId` が `0` 以外で付いていれば、`FormatNotCopyable` にする。あるいは PR #18 の R2-1 (a) の core 側の除外で拒否する。どちらでも、T4 に合成入力を 1 つ足す。
  - `smtId` を外して複製する案は、「そのまま複製する」に反するので勧めない。
  - ADR-0005 §3 "Format sources" に 1 行書く。

### N2（Minor、テスト）: depth の事前検査のうち 2 つを外しても、どのテストも落ちない

- mutants の結果:
  - 次の 2 つを外したものは、テストを通ってしまう。
    - D1: `plan_paragraph` の `check_depth(p.doc, p.path, 2)`
    - D3: `TextBody::set_text` の `check_copies(doc, &self.path, 2, run_fmt.rpr…)` の `created` を 1 にする。
  - D2（本文の `check_depth`）と D5（`add_paragraph` の `check_depth`）も残った。
- 実際の影響（mutant を当てて確かめた。コピーは `write.orig.rs` から戻した）:
  - D1 を当てた状態で、248 段の group の下にある `<a:p/>` に `paragraph_mut(0).set_text("x")` を呼ぶ（P8 の k=249）。`a:r`（256）は入り、`a:t`（257）で `TooDeep` になる。結果は **Err なのに変更あり**（`unchanged=false`）。
  - D3 を当てた状態で、`a:endParaRPr` が `solidFill/srgbClr` を持ち、247 段の group の下にある空の段落に、`TextBody::set_text("\n\ny")` を呼ぶ（P7 の k=247）。これも **Err なのに変更あり**。
  - HEAD のコードでは、どちらも `unchanged=true` で拒否される。
- 既存の T4 `edits_that_would_pass_the_depth_limit_are_refused_before_any_change` で検出されない理由:
  - そのケースでは、どの操作でも先に copy の検査（`created = 1`）が拒否してしまう。
  - そのため `check_depth` と、本文側の `created = 2` が決め手になる入力がない。
- 提案: P7 の「`\n\ny`」と、P8（`<a:p/>` に `set_text("x")`）を T4 に足す。`add_paragraph` についても、上限ちょうどの 1 ケースを足す。

### N3（Minor）: 新しい段落に run の書式が要らない場合でも、run の書式源を調べて拒否する

- 根拠:
  - probe P5: 先頭の run の `a:rPr` に `a:blipFill r:embed` があり、最後の段落自身の `a:endParaRPr lang` には書式がある。
  - ここで `TextBody::set_text("x\n")` を呼ぶ。行 0 は既存の run を使い、行 1 は空なので、新しい run は作らない。
  - 新しい段落に要るのは `a:pPr` と `a:endParaRPr` だけで、`a:endParaRPr` は N3-1 により段落自身のものを使う。
  - それでも `new_para_format`（`write.rs:290`）が無条件に `run_format` を呼ぶため、`FormatNotCopyable("a:rPr")` になる。
- 安全側の差なので Minor とする。
- 提案: `extra` に空でない行があるときだけ、`run_format(...).rpr` を作る。`plan_paragraph` の `needs_fmt` と同じ考え方である。

### N4（Minor）: 書式の判定と、リンクを外す処理のテストが足りない

ミューテーションで、次の 7 つが検出されなかった。

- P4 / P5: `has_format` を「属性だけを見る」または「リンクも書式として数える」に変える。
- P8: 空の複製（属性も子もない）でも要素を作る。
- P9: N3-1 で段落自身の `a:endParaRPr` を使うとき、リンクを外さない。
- P10: `Lens::at` の代わりに、内側のノードに対する `Lens::root` を使う。この場合、祖先の Ignorable と `xml:space` が見えなくなる。
- P11: 空の段落の `a:endParaRPr` から run の書式を取るとき、リンクを外さない。

P9 と P11 は、外し忘れても r:id があれば `FormatNotCopyable` になるので、黙って壊れることはない。ただし、ADR が約束しているリンクを外す処理は、`a:endParaRPr` の側では確かめられていない。

提案: 次の合成入力をそれぞれ 1 つずつ T4 に足す。

- 子だけを持つ `a:endParaRPr`
- `a:hlinkClick` だけを持つ `a:endParaRPr`
- 書式と `a:hlinkClick` の両方を持つ `a:endParaRPr`
- 空の `<a:rPr/>`
- 祖先に `mc:Ignorable` があり、`a:rPr` がその名前空間を使っているもの

### N5（Minor、文書）: ADR-0005 とエラーの説明が、#18 round 2 で増えた拒否を反映していない

- `docs/adr/0005-pptx-phase0.md:304`（§3 "Format sources"）、`:430`（§6 の `FormatNotCopyable` の行）、`crates/ooxml-pptx/src/error.rs:136-142` の doc と `#[error]` 文言:
  - 挙げている拒否の理由は、`r:` 属性、MC、名前空間宣言の 3 つだけである。
  - `IgnorableNamespace`、`InheritedXmlSpace`、Strict / VML の関係 ID が抜けている。
- `:594-599`（§9 "Core primitive"）は、まだ round 1 の API のまま書かれている。
  - `C: SameType<S>` となっていて、`Detachable` がない。
  - 拒否は 3 種類となっている。
  - 「`SameType` is generated for allowlisted pairs」となっている。今は detachable なエントリの間だけで生成される。
- 細かい点: `a:pPr/a:defRPr/a:hlinkClick r:id` は直下の子ではないので外されず、`FormatNotCopyable("a:pPr")` になる（P3。変更の前に拒否される）。§3 の "Hyperlinks" に 1 行書くとよい。

## 確かめて問題がなかったこと

- **ADR §9 どおりの複製**:
  - 新しい run の `a:rPr` は、先頭の `a:rPr` から作る。run がなければ `a:endParaRPr` から作る。
  - 新しい段落の `a:pPr` は、最後の段落の `a:pPr` から作る。
  - `a:endParaRPr` は N3-1 の規則で作る。
  - 外すのは直下の `LINKS` だけである。
  - T3 の Rust 側（`oracle.rs` の `run_fmt` / `without_links` と N3-1）と、python 側（`check.py` の `copy_as` と `end_src`）は、同じ規則をそれぞれ別に実装している。`hyperlink-cells` のケースは、拒否されずに適用されることを assert している。P1〜P3 の mutant は検出された。
- **原子性**:
  - `Paragraph::set_text` と `TextBody::set_text` は、次の順に進む。
    1. 全段落の `plan_paragraph`（フィールド、内容の形、書式源、名前空間、depth）を作る。
    2. `new_para_format` と `check_copies` で検査する。
    3. 適用する。
  - 適用のときに残っている core の拒否は、次の理由ですべて排除されている。
    - `UnresolvedSibling` / `AlreadyPresent`: 内容の形を検査済み。
    - `InvalidXmlChar`: 文字を検査済み。
    - `NamespaceNotInScope`: 既存の DML 要素の下では必ず解決でき、複製は `check_insert_under` で検査済み。
    - `TooDeep`: `check_depth` と、`created` の段数をつけた検査で排除済み。
  - P6（k=238〜248、5 種類の操作）と P7（`a:endParaRPr` を `a:rPr` として複製し、depth が 1 段深くなる唯一の経路）では、`Err` はすべて変更なしで、`Ok` はすべて再び parse できた。
  - core 側の `check_insert_under` と挿入の結果は、総当たりの 80 ケースで一致した（#18 のレビューを参照）。
- **depth の事前検査は、core の拒否と正確に一致する**:
  - 新しい要素の深さは `path.len() + 1 + levels` で、core の `insert_at_slot` / `insert_detached` の式と同じである。
  - 余分に拒否することがあるのは、`plan_paragraph` が何も作らない場合でも `check_depth(p, 2)` を課す場合だけである。これは段落が depth 255 以上にあるときに限られ、実害はない。
- **T2/T3/python の独立性**:
  - python の `edit` は、python-pptx 自身の lxml ツリーの上で、deepcopy と remove を使って編集する。Rust の model の計算を写したものではない。
  - ただし、両者は同じ ADR の規則に従っているので、規則そのものの誤り（N1）は検出できない。これは本質的な限界である。
- **コメントのガードは fail closed（#16 m4）**: 次のどの場合も `scan_comments(None)` か `all = true` になり、スライドのすべての図形のテキストが lock される。
  - 関係を解決できない（外部ターゲットを含む）、パートがない、`read_part` が失敗する、parse が失敗する（`TooDeep` を含む。T4 `too_deep_comment_part_fails_closed`）。
  - ルートが `p188:cmLst` でない、未知の子がある、anchor がない、anchor の下に id がない。
  - slide の `.rels` 自体が壊れている場合は、`slide_mut` が `Err` になる。どちらの場合も変更はない。

## owner の問い

- **Phase 0.1 を今やるか**: 賛成する。ただし N1 を扱うことが条件である。
  - 理由: T2 で `FormatNotCopyable` が 217/599 と 156/394 から 0 になった。
  - 複製はパートの中で閉じていて、関係、ID、パートを新しく作らない。
  - 関係を参照する書式源（`r:`、Strict、`o:relid`）は、変更の前に拒否される。
- **継承された `xml:space` を拒否するか**: 賛成する。DrawingML の書式 CT は `xml:space` を宣言していないので、付け直すとスキーマに反する。corpus と fixtures に `xml:space` を含むスライドは 0 件だった。実際に拒否が起こるのは、祖先が preserve で、かつ書式要素の中が整形用の空白を含む場合だけである。

## 判定

**converged: no**。

- N1（`smtId`）を、pptx 側の `FormatNotCopyable` か core 側の除外で扱えば収束する。
- N2 は、データを壊すかどうかを左右する事前検査を守るテストなので、同じ PR で足すことを勧める。
- N3〜N5 は Minor である。
