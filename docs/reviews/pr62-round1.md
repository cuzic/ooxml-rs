# PR #62 レビュー（Opus, round 1）

対象: impl/docx-phase2 @ eef4f3f3656b5bc721334a05d3015c6a94ebbbcd（差分は `origin/impl/docx-phase1-part2...origin/impl/docx-phase2` のみ）

## 結論（先に）
converged: no。ハイパーリンク（rels 追加・エスケープ・`r` 名前空間・id 重複回避）と section break 本体、A2 の移動は、
プローブした範囲では正しく、拒否はバイト同一だった。ただし、A2 の文書で `insert_paragraph(After(最後の段落))` を呼ぶと、
A2 が防いでいた「既定の新しい最終セクション」がそのまま作られる（M1）。複数段落にまたがるフィールドの空段落では、
3 つの操作すべてがフィールド結果の中に書き込む（M2）。新しい拒否 3 つにはテストが無く、手書き変異が生き残る（M3）。

## 前ラウンドの解消状況
（round 1 のため無し）

## Findings

### M1（Major）: A2 の文書で `insert_paragraph(After(最後の段落))` が既定設定の新しい最終セクションを作る
- 根拠: 本文 `<w:p>one</w:p><w:p><w:pPr><w:sectPr><w:pgSz … w:orient="landscape"/></w:sectPr></w:pPr>…last</w:p>`（本文直下の `w:sectPr` 無し）に
  `insert_paragraph(Place::After(p(1)), "x", Plain)` → `Ok(p(2))`。出力は `…last</w:p><w:p><w:r><w:t>x</w:t></w:r></w:p>` で、
  新しい段落は最後の `sectPr` の後ろ（ADR-0004 §3.2 が述べる「横向き文書が縦向きの既定ページで終わる」状態）。
  `LikeNeighbour` でも `sectPr` を落とした `<w:pPr/>` になるだけで同じ。ADR-0011 §9.1 は「ADR-0004 の append は
  『最後の本文段落の後』の特殊ケース」と書いており、append と同じく `sectPr` を移すか拒否すべき。
  `structure.rs:268-330` に A2 相当の判定が無い。
- 修正案: アンカーが本文の流れの最後の段落で A2 条件（`ends_with_section_break`）を満たすときは `append_moving_section` と同じ移動を行う
  （内容コントロール内なら `SectionBreakAtEnd`）。あわせて、途中のセクション末段落の `After` は次のセクションに入ることを API 文書に書く
  （プローブ: `<w:p><w:pPr><w:sectPr>…` の後ろに入る）。

### M2（Major）: 段落をまたぐフィールドの中の空段落で、3 つの操作がフィールド結果に書き込む
- 根拠: `HYPERLINK`（SAFE）のフィールドが p0（begin/instr/separate/res1）→ p1（`<w:p/>`）→ p2（res3/end）にまたがる本文で、
  - `insert_paragraph(Before(p(1)), "x")` → `Ok`。p0 と p1 の間、つまりフィールド結果の中に `x` の段落が入る
    （`After(p(0))` は p0 の run が `in_field` なので `InsideField` で拒否され、同じ位置への挿入で結果が食い違う）。
  - `insert_hyperlink(EndOf(p(1)), …)` → `Ok`。フィールド結果の中に `w:hyperlink` が入る。
  - `insert_section_break(p(1), NextPage)` → `Ok`。フィールド結果の途中でセクションが切れる。
  `check_structure_neighbour`（`structure.rs:224-250`）は段落の run の `in_field` しか見ないため、run の無い段落では素通りする。
  `INCLUDETEXT` など SAFE なフィールドの結果は空行を含むことが多く、現実に起きる。ADR-0007 R1（フィールド内の書き込みは
  `InsideField`）と、ADR-0004 append の「本文末が閉じていないフィールド」の拒否と同じ考え方で拒否すべき。
- 修正案: 段落の境界でのフィールド状態（直前までに開いている `fldChar` の深さ）を body index から求め、挿入位置・段落・リンク位置が
  フィールド内なら `InsideField` で拒否し、Err ⇒ byte-identical のテストを付ける。

### M3（Major）: 新しい拒否 3 つにテストが無く、手書き変異が生き残る
- 根拠: 次の変異で `structure` と `adversarial` のテストが全部通る。
  - `structure.rs:397` の `tables_between` を無効化: 本文 `<w:p/><w:tbl>…<w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/>…</w:tbl><w:p/><w:sectPr><w:pgSz w:w="2"/>`
    で `insert_section_break(p(0))` が `Ok` になり、p0 に誤ったセクション（`w:w="2"`、実際に p0 を含むのは表内の `w:w="1"`）がコピーされる。黙った誤り。
  - `structure.rs:419` の `r` 名前空間チェック（`NamespaceNotInScope`）を無効化。
  - `structure.rs:652`（append の移動での `RelIdReferenced`）を無効化。
  AGENTS.md の「新しい拒否には必ず Err ⇒ byte-identical のテストを付ける」「新しいコードを壊す変異を用意する」に反する。
  特に 1 つ目は、無いと誤ったファイルを書く分岐。
- 修正案: 3 つそれぞれに `refused(...)` 形式のテスト（表内 `sectPr`、`xmlns:r` が `headerReference` 上にだけある文書、
  append で未定義の `r:id` を持つ最終セクション）を足す。

### m1（Minor）: セクションの最後の段落で区切ると、段落の無い最終セクションになる
- 根拠: `<w:p>a</w:p><w:sectPr><w:pgSz w:w="1"/></w:sectPr>` に `insert_section_break(p(0), NextPage)` → `Ok`。
  出力は `<w:p><w:pPr><w:sectPr>…</w:sectPr></w:pPr>a</w:p><w:sectPr><w:type w:val="nextPage"/>…`。後半のセクションに段落が無く、
  Word が空白ページを出すのか補正するのかは未確認 *(u)*。ADR-0011 §6 はこの場合に触れていない。
- 修正案: S との間に段落が無いときは拒否するか、挙動を API 文書と `manual_check` に載せる。

### m2（Minor）: 区切り・継続区切りの脚注（`w:type="separator"` など）にも段落を挿入できる
- 根拠: `footnotes.xml` の `w:footnote w:type="separator"` の段落に `insert_paragraph(Before(…), "x")` → `Ok`。
  区切り線の脚注に本文が入る。ストーリー索引が separator の脚注も含むため。
- 修正案: `separator` / `continuationSeparator` / `continuationNotice` の脚注・文末脚注は `UnsupportedContext` にする。

### m3（Minor）: 区切りで後半 S を持つ段落が変わっても generation が変わらない
- 根拠: S が後ろの段落の `w:pPr/w:sectPr` のとき（`section_breaks` テストの 2 例目）、その段落のバイトは変わる（`w:type` 追加、
  `pgNumType/@start` 削除）が、`insert_section_break` は `self.bump(after)` しか呼ばない（`structure.rs:470`）。
  `Document::generation` の文書（「その段落を変える書き込みは値を変える」）と食い違う。run の位置は変わらないので実害は小さい。
- 修正案: S が段落内にあるときはその段落も `bump` する。

### n1（nit）: append の移動で `w:pPrChange` を `w:sectPrChange` として報告する
- 根拠: `structure.rs:640-641`。`<w:pPr><w:sectPr/><w:pPrChange …/></w:pPr>` の append は
  `TrackedPropertyChange { element: "w:sectPrChange" }`（拒否とバイト同一は正しい）。
- 修正案: どちらが原因かで `element` を分ける。

## 確認して問題なかったこと
- 外部リンク: `&` `"` `<` を含む URL は `.rels` で `&amp;` `&quot;` `&lt;` にエスケープされ再読込できる。空白は core が `InvalidTarget`。
  関係はリンクごとに新規、`TargetMode="External"`、id は一意で、ぶら下がった `r:id` の値を避ける。
- `xmlns:r` が別 URI に束縛された文書では `xmlns:r1` を宣言して `r1:id` を書く（衝突しない）。
- `xmlns:r` が `sectPr` 上にだけある文書の区切りは `NamespaceNotInScope` で拒否、バイト同一。
- 段落内・本文直下の `mc:AlternateContent` は `UnresolvedSibling` / `UnsupportedContext` で拒否、バイト同一。
- `w:fldSimple` を挟んだ run の後ろへのリンク挿入、返す `RunRef` の index は正しい。
- `pgNumType/@start` 削除・`lnNumType` の `continuous` 化はテストで守られている（変異は落ちる）。
- CI（test/clippy/codegen --check、python-pptx オラクル）は緑。

## 他の PR との干渉
#66（Word Phase2 part 2）がこのブランチに積まれ、`structure.rs` に触る。M1/M2 の修正は #66 にも取り込む必要がある。

## 判定
converged: no（Blocker 0 / Major 3 / Minor 3 / nit 1）
