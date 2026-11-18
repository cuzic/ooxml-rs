# PR #59 レビュー（Opus, round 1）

対象: `578de21`（#52 の上に積まれた Word Phase1 part 2: ADR-0007 §4 / §5 / §6.3）

## 結論（先に）

**Major が 2 件あるため収束していません。** どちらも修正は数行で済みます。

- **M1** スタイル setter が範囲外の `ParaRef` / `RunRef` を受け取ると panic します。
- **M2** フォント名に XML で使えない文字があると、Err なのに部分的に書き込まれます（Err ⇒ byte-identical の破れ）。

設計（スロット配置、上書きする属性の除去、継承値の `0` 打ち消し、W2 の lift 範囲、`updateFields` の配置）は ADR どおりです。自前のコーパススイープ（795 ファイル、53,075 書き込み）でも違反はありませんでした。

## 検証したこと

- `cargo test -p ooxml-docx` は全て green です。`clippy --all-targets` は PR のコードについては警告 0 です。`cargo xtask codegen --check` は up to date でした。
- プローブ（`review-pr59/repo/crates/ooxml-docx/tests/probe59.rs`、未追跡）で次を確かめました。
  - スロット順: `w:pPr` の新しい子は mark の `w:rPr` / `sectPr` / `pPrChange` より前に入ります。
  - toggle の no-op: `<w:b w:val="false"/>` に `Set(false)` を書いても byte-identical です。
  - `ff0000` と `Rgb(255,0,0)` の比較は no-op になります。
  - `rPrChange` は `TrackedPropertyChange` になります。
  - linked スタイルの `StyleTypeMismatch` は `linked: Some("H1Char")` を返します。
  - `Heading2` / `heading2` に対する `latent_only` は true です。
  - Unicode の大文字小文字を無視した名前検索（`ÄBC` / `äbc`）は両方を返します。
  - opt-in の後は format / style の W2 が lift され、R1（`InsideField`）は残ります。
  - 表の中のセル段落で `w:pPr` を挿入し、続けて hyperlink 内の run と別段落の run を整形し、最後に `replace` まで実行しました。対象がずれることはありませんでした。
- toggle 系のプロパティ: 直接書式の `<w:b/>` / `w:val="0"` は絶対値です。スタイルチェーン上の XOR（ECMA §17.7.3）は直接書式には及ばないので、ADR の「`Set(false)` は常に `0` を書く」で問題ありません。複合スクリプト版（`bCs` / `iCs` / `szCs`）が独立しているのも ADR どおりです。
- `w:rFonts`: `hint` を残すこと、同じスロットの theme 属性（`cstheme` は小文字）だけを消すことを確認しました。

## Findings

### M1（Major）: `set_paragraph_style` / `set_run_style` が範囲外の参照で panic する

`crates/ooxml-docx/src/styles.rs` L145-159 の `set_style` は、対象のチェックより先に `plan_style_ref` を呼びます。その中の `format.rs` L745-761（`run_container` / `para_container`）は `paragraphs[at.index].runs[at.run]` を直接添字で引いています。

プローブの結果:

```
set_paragraph_style(p(5), ..)  → panic format.rs:758 "index out of bounds: the len is 1 but the index is 5"
set_run_style(r(0,3), ..)      → panic format.rs:747
set_run_style(r(4,0), None)    → panic format.rs:747
```

`ParaRef::new` の doc には "checked on use" とあり、`format_*` の方は `NoSuchParagraph` / `NoSuchRun` を返します。もう一つの問題として、no-op の計画（同じ id を書く場合や、`None` で既に無い場合）は対象チェックを通らずに `Ok` を返します。

修正案（`styles.rs` L152 付近）:

```suggestion
        match target {
            StyleTarget::Para(p) => { self.paragraph_at(p).ok_or(Error::NoSuchParagraph(p.index()))?; }
            StyleTarget::Run(r) => { self.run_at(r).ok_or(Error::NoSuchRun { paragraph: r.para().index(), run: r.index() })?; }
        }
        if let Some(id) = id {
```

`run_container` / `para_container` も `.get()` に変えて `Result` を返す形にしておくと、同じ種類の panic を防げます。panic しないことの回帰テストを 1 本追加してください。

### M2（Major）: フォント名の不正な XML 文字が書き込み段階で見つかり、Err なのに部分的に書き込まれる

`format.rs` L486-494 の `plan_run` が弾くのは空文字だけです。`set_attr` の `check_xml_chars` は apply 段階（L653-659）でやっと失敗しますが、その時点では `w:rPr` と `w:rFonts` が既に挿入されています。

```
format_paragraph_runs(p(0), bold + ascii "A\u{1}")
→ Err(Core(InvalidXmlChar)), identical=false
  <w:r><w:rPr><w:rFonts/><w:b/></w:rPr><w:t>x</w:t></w:r>
```

複数の run を対象にした呼び出しでは、前の run は書き込み済みのまま残ります。これは ADR §4 の「every insertion … dry-run first」と、テストの不変条件「Err ⇒ byte-identical」に反します。

修正案（L489）:

```suggestion
                if name.is_empty() {
                    return Err(Error::InvalidValue { what: "font name", value: String::new() });
                }
                if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(name) {
                    return Err(Error::InvalidXmlChar { ch, offset });
                }
```

`refusals_before_any_write` にこのケース（`format_paragraph_runs`、2 run）を追加してください。

### m1（Minor）: 未定義の `pStyle` で既定の段落スタイルにフォールバックしない

`styles.rs` L196-203 では、`pStyle` の id が存在しないと `by_id` が `None` を返し、チェーンが即 `docDefaults` に飛びます。Word はこの場合に既定の段落スタイル（Normal）を適用します。

プローブでは、Normal に `firstLineChars="100"` がある状態で次のようになりました。

- `pStyle="Missing"` の段落: `<w:ind w:firstLine="0"/>` だけが書かれ、継承した値が打ち消されませんでした。
- `pStyle` の無い段落: `w:firstLineChars="0"` も書かれました。

コーパスでは未定義の `pStyle` は 6 段落です。修正案は、`style` が `Some` でも `by_id` が `None` なら既定の段落スタイルを使うことです。

### m2（Minor）: Clear が上書き属性（`*Chars` / `*Lines` / `*Autospacing`）を残すため「継承に戻す」が成立しない

`format.rs` L547 / L578 / L592 で、`Clear` が消すのは `before` / `left` + `start` / `firstLine` + `hanging` だけです。

```
<w:ind w:left="720" w:leftChars="200" w:firstLine="100" w:firstLineChars="50"/><w:spacing w:before="100" w:beforeLines="50"/>
→ Clear ×3 → <w:ind w:leftChars="200" w:firstLineChars="50"/><w:spacing w:beforeLines="50"/>
```

その結果、直接書式の値がそのまま有効です。以前の `Set` で書いた `firstLineChars="0"` も残るので、継承値が打ち消されたままです（既存テスト L143 はこれを期待値にしています）。`Prop::Clear` の doc は "the value is inherited again" と書いており、フォントの Clear は theme 属性も消しています。これと揃えて、Clear でも上書き属性を消してください（または ADR と doc に現状の挙動を明記してください）。

### m3（Minor, テストの穴）: `write_run_plans` の `reindex` と `cstheme` がテストで守られていない

- 変異 **M3**（`format.rs` L835 の `self.reindex` を削除）は既存テストを生き残りました。しかしルビを持つ run（`w:r/w:ruby/w:rubyBase/w:r`）を整形すると、外側の run に `w:rPr` が入り、`rubyBase` の中の run のパスがずれます。変異を入れた状態で、続けて段落の run を整形すると `document.rs:720` で panic しました。日本語の文書に関わるので、回帰テストを 1 本追加してください（プローブ `k_ruby_reindex` がそのまま使えます）。
- 変異 **M8**（`"cstheme"` → `"csTheme"`）も生き残りました。`superseding_attributes_are_removed` に `w:cs` + `w:cstheme` のケースを追加してください。

### m4（Minor, (u)）: 継承値の探索が numbering と表スタイルを見ていない

`inherited_ppr_attr` は「段落スタイル → basedOn → `docDefaults`」だけを見ています。`numPr` → `lvl/pPr/w:ind` の `*Chars`、および表スタイルの `pPr` は見ていません。コーパスではどちらも 0 件（`lvl` の ind `*Chars`: 0、表スタイルの ind `*Chars` / spacing の上書き属性: 0）なので、ADR の implementation notes に既知の制限として 1 行書いておけば十分です。

### Nit

- **n1** `Clear` / `None` で中身が空になった `<w:rPr></w:rPr>` / `<w:pPr></w:pPr>` が残ります（プローブ E2、既存テスト `styles_by_id`）。無害ですが、属性要素は空になると消しているので、それと揃えて消すのが一貫します。
- **n2** `Set(LineSpacing::Auto(n))` を、`w:line="n"` があり `lineRule` が無い要素に書くと、`lineRule="auto"` が追加されます。意味は同じですが byte-identical ではありません。
- **n3** styles の relationship が 2 本ある場合、黙って 1 本目を使います（`document.rs` の `target(STYLES_RELATIONSHIP).into_iter().next()`）。settings の `AmbiguousSettings` と揃えるなら fail closed にするか、コメントを残してください。
- **n4** 変異 M9（autospacing の `!= Some(false)` → `== Some(true)`）が生き残りました。違いが出るのは解析できない値だけなので、実害はありません。

## 変異テスト（手で入れた 12 件）

| # | 変異 | 結果 |
|---|---|---|
| M1 | 継承値の `0` 打ち消しを削除 | killed |
| M2 | `DuplicateProperty` を無効化 | killed |
| M3 | `write_run_plans` の `reindex` を削除 | **survived**（m3） |
| M4 | `write_para_plan` の `reindex` を削除 | killed |
| M5 | mark の `ins` チェックを削除 | killed |
| M6 | `AmbiguousStyle` を無効化 | killed |
| M7 | 同じ色のときに theme 属性を消さない | killed |
| M8 | `cstheme` → `csTheme` | **survived**（m3） |
| M9 | autospacing の判定を変更 | survived（n4。実害なし） |
| M10 | opt-in で W2 を lift しない | killed |
| M11 | settings の dry run を削除 | survived（等価変異: `insert` が同じ判定を持つ） |
| M12 | `latent_only` を常に false にする | killed |

## コーパススイープ（独立に実施）

`docx-p1/corpus` の 795 ファイル（sdk / tpl / poi / d4j）を対象にしました。各ファイルで opt-in を行い、最大 400 段落について、段落ごとに次を実行しました。

- `format_paragraph`（全 setter を `Set`）
- `format_paragraph_runs`（`b` / `iCs` / `color auto` / `sz` / `u Clear` / `eastAsia Set` / `ascii Clear`）
- 最初の 5 段落のみ `set_paragraph_style`

結果:

- 成功した書き込みは 53,075 件です。拒否は次のとおりで、どれも型付きでした: `UnsupportedContext` 4,941 / `TrackRevisionsOn` 1,131 / `InsideField` 1,080 / `Core` 61 / `InsideRevision` 29 / `DocumentProtected` 15。
- panic、Err の後に byte-identical でない、2 回目の書き込みが no-op でない、出力で `rPr` / `pPr` のスロット順が崩れる、保存して開き直すとテキストが変わる、のいずれも 0 件でした。
- スロット順のチェックが反応したのは docxtemplater の 5 ファイルだけです。どれも入力時点で `[sectPr, pStyle, bidi, jc, rPr]` という順序で、ADR §4 が挙げている既知の 5 ファイルです。`format_paragraph` は `Core(UnresolvedSibling)` で拒否しており、PR が順序を崩したものではありません。

## 判定

**converged: no**（Major 2 件: M1, M2）。M1 と M2 を直し、m3 のテスト 2 本を足せば、次のラウンドで収束する見込みです。m1 と m2 も同じ PR で直すのが望ましいですが、Minor なので後続に回しても構いません。
