# PR #59 レビュー（Opus, round 2）

対象: `impl/docx-phase1-part2` @ `172030ceda63f1d8ef17610e9e51a3264361711b`

## 結論（先に）

**converged: yes** です。round 1 の Major（M1: 範囲外参照で panic、M2: フォント名の不正文字で部分書き込み）は、どちらも挙動で解消を確認しました。Minor と nit もすべて直っています。
今回の重点（rPr / pPr のスロット順、toggle と複合スクリプト版、style id の検索、`updateFields` の書き込み範囲、Err ⇒ byte-identical、局所性、`mc:AlternateContent`）では、Blocker と Major は見つかりませんでした。新しい指摘は Minor 3 件と nit 1 件で、follow-up に回して構いません。
ただし base が main になった今、ブランチは main と **生成物 2 ファイルで衝突** します（`codegen/report.txt` と `crates/ooxml-schema/src/generated/dml.rs`）。マージ前に main を取り込み、`cargo xtask codegen` で再生成してください。下の「main との統合」で試したとおり、再生成すれば通ります。

## 前ラウンドの解消状況

- **M1** 解消。`set_paragraph_style(p(5), ..)`、`set_run_style(r(0,3), ..)`、`set_run_style(r(4,0), None)` は、いずれも `NoSuchParagraph` / `NoSuchRun` を返し、出力は byte-identical でした。`run_container` / `para_container` は `.get()` を使う形になっています。回帰テストは `tests/format.rs` L176-180 です。
- **M2** 解消。`plan_run` が計画の段階で `first_invalid_xml_char` を検査します（`format.rs` L509）。2 run に対する `format_paragraph_runs` は `InvalidXmlChar` を返し、何も書きません（L184）。手で入れた変異（検査を外す）はテストで落ちました。
- **m1** 解消。未定義の `pStyle` は既定の段落スタイルにフォールバックします。変異（フォールバックを外す）はテストで落ちました。
- **m2** 解消。Clear は上書き属性と以前の `0` 打ち消しも消します。プローブでは、`leftChars="0"` と `beforeAutospacing="0"` が入った段落を Clear すると `w:pPr` ごと消え、継承値に戻りました。3 つの変異（`ind` の chars、`spacing` の sup、`firstLine` の chars）はすべて落ちました。
- **m3** 解消。ルビのテスト（`ruby_runs_after_an_rpr_insert`）と `cstheme` のテストが追加されています。
- **m4** 解消。ADR の Part 2 notes に既知の制限として書かれています。
- **n1** 解消（ただし run 側はテストの穴。下の m3）。
- **n2** 解消。変異（常に `lineRule` を書く）はテストで落ちました。
- **n3** 解消。2 本目の styles relationship は `AmbiguousStylesPart` になります。変異（検査を外す）はテストで落ちました。
- **n4** そのまま。実害はありません。

## 検証したこと

- CI（head `172030c`）: `test, clippy, codegen --check (Rust 1.99)` と `pptx T3 read-back` はどちらも pass でした。手元で回した `cargo test -p ooxml-docx` もすべて green です。
- プローブ（未追跡の `tests/probe59r2.rs`。コミットしていません）で次を確かめました。
  - **rPr のスロット順。** `[rStyle, caps, spacing, lang]` を持つ `w:rPr` に 12 種類すべての setter を書きました。結果は `rStyle, rFonts, b, bCs, i, iCs, caps, strike, color, spacing, sz, szCs, highlight, u, vertAlign, lang` で、CT_RPr の順序どおりです。
  - **pPr のスロット順。** `[pStyle, numPr, outlineLvl, rPr, sectPr]` を持つ `w:pPr` では、`keepNext` / `keepLines` / `spacing` / `ind` / `jc` がスキーマ順に入り、mark の `rPr` と `sectPr` より前に置かれました。
  - **toggle。** `<w:b w:val="off"/>` に `Set(false)`、`<w:i w:val="1"/>` に `Set(true)` を書くと no-op で、byte-identical でした。解析できない値の `<w:bCs w:val="bogus"/>` に `Set(true)` を書くと `<w:bCs/>` になります。`bold_cs` / `italic_cs` / `size_cs` は、ラテン側と独立に書かれます。
  - **`mc:AlternateContent`。**
    - `w:rPr` / `w:pPr` の子にある場合: 要素の挿入は `Core(UnresolvedSibling)` になり、byte-identical です。
    - 2 run の呼び出しで 2 本目だけが該当する場合: 呼び出し全体が拒否され、1 本目も書かれません。
    - run を包む `mc:AlternateContent` の場合: 中の run は索引されず、`format_paragraph` は `UnresolvedSibling` で拒否されます。
  - **style id。**
    - 型の不一致は両方向とも `StyleTypeMismatch` になります。
    - `w:aliases` は参照されません。ADR どおりで、`UnknownStyle` になります。
    - styles.xml の中で `mc:AlternateContent` に包まれた `w:style` は `UnknownStyle` になります（fail closed）。
  - **`updateFields`。**
    - 変わるのは `word/settings.xml` だけで、他の part は zip のエントリ単位で同一でした。
    - settings.xml の中で変わるのは挿入した要素だけです。`<w:zoom  w:percent='100' />\r\n` のような書式もそのまま残ります。
    - `mc:AlternateContent` を持つ settings は `UnresolvedSibling` になり、byte-identical で、opt-in も記録されません。
  - **継承値の打ち消し。** 属性ごとに最も具体的なレベルが勝ちます。`basedOn` の子が `leftChars="0"` を持っていれば打ち消しを書きません。`docDefaults` の `beforeAutospacing` は `0` で打ち消します。
- 手で入れた変異 10 件の結果: killed 8 / survived 2（MB → nit、MF → m3）。

| # | 変異 | 結果 |
|---|---|---|
| MA | M2 の計画時の XML 文字検査を外す | killed |
| MB | `set_style` の対象チェックを外す | **survived**（`para_container` / `run_container` が同じエラーを返すため。nit） |
| MC | `AmbiguousStylesPart` を無効化 | killed |
| MD | 未定義の `pStyle` のフォールバックを外す | killed |
| ME / ME2 | Clear で `*Chars` / spacing の上書き属性を消さない | killed |
| MF | 空になった `w:rPr` を消さない（run 側） | **survived**（m3） |
| MF2 | 空になった `w:pPr` を消さない | killed |
| MG | `Auto` で常に `lineRule` を書く | killed |
| MH | `first_line` の Clear で `*Chars` を消さない | killed |

## main との統合（試行のみ。push していません）

スクラッチの worktree で `origin/main`（`add51c5`）をマージしました。衝突した生成物 2 ファイルは `cargo xtask codegen` で再生成しました。その状態で次の結果になりました。

- `codegen --check` は up to date でした。
- `cargo test -p ooxml-docx` はすべて green でした。#69 の `story_oracles` などを含みます。
- `cargo clippy -p ooxml-docx --all-targets -- -D warnings` は警告 0 でした。

手で書く衝突解消は不要です。#62 と #66 はこのブランチに積まれているので、#59 に main を取り込んだ後、それぞれが #59 を取り込み直す必要があります。

## Findings

### m1（Minor）: `set_paragraph_style(p, Some(""))` が `<w:pStyle w:val=""/>` を書いて `Ok` を返す

`styles.xml` に `w:styleId` の無い `w:style` があると、`style_info` の `id` は `""` になります（`styles.rs` L67）。`resolve_style("")` はそれを「一致する style」として受け入れます。

プローブの結果:

```
<w:style w:type="paragraph"><w:name w:val="noid"/></w:style> がある状態で
set_paragraph_style(p(0), Some("")) → Ok, <w:pPr><w:pStyle w:val=""/></w:pPr>
```

`w:styleId` の無い style がなければ `UnknownStyle` になります。参照先として意味の無い id を、黙って書いていることになります。

- 修正案: `set_style` の先頭で、空の id を `InvalidValue { what: "style id", .. }` で拒否してください。

### m2（Minor）: 大文字小文字だけ違う id に `UnknownStyle { latent_only: true }` を返し、原因を取り違える

`Heading1` が定義済みで、latent に `heading 1` がある文書で `set_paragraph_style(p, Some("heading1"))` を呼びました。結果は `UnknownStyle { id: "heading1", latent_only: true }` でした（`styles.rs` L111-130）。

`same.len() == 1` なので、大文字小文字だけ違う定義が存在します。それなのに「定義が無く latent のみ（作成は Phase2）」と報告します。拒否そのものは ADR §5 の「exact `w:styleId`」どおりで、安全側です。ただ、呼び出し側は誤った対処に誘導されます。

- 修正案: 大文字小文字を無視すると一致する定義があるときは、`latent_only: false` にしてください。または、その id をエラーに載せてください（例: `UnknownStyle { .., did_you_mean: Some("Heading1") }`）。

### m3（Minor）: `mc:AlternateContent` を子に持つ `w:rPr` / `w:pPr` では、既存要素の変更と削除が AC の中身を見ずに通る

要素の挿入は core が拒否します。しかし既存要素に対する属性の変更と Clear は、dry run（`op.existing.is_none()` のときだけ検査、`format.rs` L834 付近）を通りません。

プローブの結果:

```
<w:rPr><w:b/><mc:AlternateContent><mc:Choice Requires="w14"><w:b w:val="0"/></mc:Choice>…</w:rPr>
format_run(bold: Set(false)) → Ok
<w:rPr><w:b w:val="0"/><mc:AlternateContent>…<w:b w:val="0"/>…</w:rPr>
```

Choice を選ぶ reader から見ると、`w:b` が 2 つあることになります。これは §4 の `DuplicateProperty`（どちらが勝つか未規定なので fail closed）と同じ状況ですが、`Planner::existing`（L357）は AC の中を数えないので見逃します。コーパスでの頻度は低いはずなので、Minor とします。

- 修正案: `Planner::new` で、コンテナが `mc:AlternateContent` を子に持つなら、変更・削除を含むすべての書き込みを `Core(UnresolvedSibling)` で拒否してください。あわせて Err ⇒ byte-identical のテストを 1 本追加してください。

### m4（Minor, テストの穴）: run 側の「空になった `w:rPr` を消す」（n1）がテストで守られていない

変異 MF（`write_run_plans` の `if emptied { r.remove::<RunProperties>() }` を無効化）は、既存テストを生き残りました。pPr 側（MF2）は killed です。

- 修正案: `<w:rPr><w:b/></w:rPr>` に `bold: Clear` を書くと `<w:r><w:t>…` になることを確かめるテストを 1 本追加してください。

### Nit

- **n1** 変異 MB（`set_style` の対象チェックを外す）が生き残りました。範囲外の参照は `para_container` / `run_container` でも同じエラーになるので、実害はありません。ただ、「no-op でも対象を検査する」（M1 の後半）を守るテストが無い状態です。たとえば `InsideField` の段落に対する `set_paragraph_style(p, None)`（`pStyle` が無いので no-op）が `InsideField` を返すことを確かめるテストを 1 本足すと、この変異が落ちます。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 4 / nit 1）。Minor 4 件と nit 1 件は follow-up で構いません。マージの前に main を取り込み、生成物を再生成してください。
