# PR #61 レビュー（Opus, round 1）

対象: `impl/xlsx-phase2-part2` @ `0e52cd796ba5465ed1ffb5aba1bdafdee98409fc`（ADR-0006 Accepted rev 3 の part 2: rich string、format read-back、`dxfs` registry。base の #50 は main にマージ済み）

検証には `git clone --reference` で作った複製を使った。プローブと変異はコミットしていない。CI（Rust 1.99 の test・clippy・`codegen --check`、python-pptx オラクル）はすべて緑だった。

## 結論（先に）

**未収束（converged: no）。Blocker 0、Major 2、Minor 3。**

rich string の書き込みは堅い。次の点はプローブで確かめた。

- エスケープ（`&<>"`、`rFont` 属性値）、`xml:space="preserve"`、`_x005F_` 保護、制御文字の `_xHHHH_` が正しい。
- `x:` 接頭辞付きの sst でも正しく書ける。
- 2 セッション目の exact-form reuse が効く。
- plain と rich は衝突しない。
- calamine で読み戻せる。

`dxfs` を schema 順に挿入する処理（`tableStyles` の前、`colors`/`extLst` の前）も正しい。

Major は次の 2 件である。

- format read-back は、`alignment`/`protection` を持たない `xf` を `None` として返す。`None` は「inherit」を意味するので、書き戻すと Normal スタイルの配置と保護がセルに入る。見た目が黙って変わる。
- `numFmt` を含む `dxf` は、セッションをまたいで再利用されない。セッションのたびに `dxf` と `numFmtId` が増える。

## 前ラウンドの解消状況

round 1 のため、該当なし。

## Findings

### M1（Major）: read-back の `alignment`/`protection` が「無し」でなく「inherit」になり、書き戻すと見た目が変わる

- 根拠: `crates/ooxml-xlsx/src/registry/read.rs:347-352`。`xf` に `alignment`/`protection` の子が無いと、`f.alignment`/`f.protection` は `None` のまま残る。
  - `Format` では `None` が inherit を意味する（ADR-0006 §3）。そのため `styles.rs` の `Planner::format`（`spec.alignment = match … None => default…alignment.clone()`）が `cellStyleXfs[0]` の子をバイトでコピーする。
- プローブ: テンプレートの `cellStyleXfs[0]` に `<alignment vertical="center"/><protection locked="0"/>` を加え、A1（`s` 無し、`cellXfs[0]` に子は無い）に数値を書いた。
  - `ws.format(0,0)` は `Expressible`（`alignment: None, protection: None`）を返した。
  - その `Format` を B1 に `set_format` すると、新しい `xf` `<xf …><alignment vertical="center"/><protection locked="0"/></xf>`（`s="1"`）が追加された。
  - 元のセルは下揃えでロックされていたが、書き戻したセルは中央揃えでロックが外れた。
- 違反している規則:
  - `FormatRead::Expressible` の doc「Writing this `Format` to another cell gives the same look」に反する。
  - PR 本文の「resolves to the **same `xf`**」にも反する。
  - ADR が Context fact 11 で前提にするモデル（Excel はセルの `xf` から書式を決める）では、`Ok` での黙った誤りになる。
- 影響範囲: Normal に `vertical="center"` を持つ日本語 Excel ファイルは POI に 41 件ある（ADR §3）。そのうち子を持たない `xf` が直撃する。
- テストが通る理由: テンプレートの Normal に子が無い。
- 修正案: 子が無いときは `Some(Alignment::default())` と `Some(Protection::default())` を返す。どちらも子を書かないことはプローブで確かめた。Normal に子があるファイルでは、read → `set_format` で同じ `s` に戻ることをテストに加える。

### M2（Major）: `numFmt` を含む `dxf` がセッションをまたいで再利用されず、毎回増える

- 根拠: `styles.rs:693-728` の `dxf_num_fmt` は、新しいコードを `custom_codes`（`numFmts` のコード）と `dxf_codes`（このセッションで割り当てたもの）だけで探す。
  - 既存の `dxf/numFmt` は seed されないので、2 セッション目は `next_numfmt`（パッケージ全体の最大値 + 1）を新しく割り当てる。
  - 正規形のバイトが変わるため、exact-form reuse が成立しない。
- プローブ: `DxfFormat::new().set_num_format("0.000").set_bold()` を追加した。

  | 時点 | 結果 |
  | --- | --- |
  | session 1 | index 0（`numFmtId="164"`）。同じセッション内の 2 回目も 0 |
  | session 2 | index **1**（`numFmtId="165"`、中身は同じ） |
  | `numFmt` の無い dxf | session 2 で 2、session 3 でも 2（再利用される） |

- 違反している規則:
  - ADR-0006 §2 の「repeated batch runs therefore stop growing」に反する。§5.8 は dxfs を「Same registry shape」と定める。
  - PR 本文の「exact-form reuse within a session and across sessions」は、`numFmt` 付きの dxf には成り立たない。
  - Phase4 の条件付き書式で数値書式を使うと、実行のたびに `dxfs` と `numFmtId` が増える。
- テストの穴: `self.next.dxf_codes.get(code)` の参照を消す手書き変異を入れても、`phase2_part2` は全通過した。同じセッション内の `numFmt` 付き dxf の再利用にもテストが無い。
- 修正案: 活性化のときに、既存の `dxf/numFmt` の `formatCode → numFmtId` を `dxf_codes` に seed する（重複時は最小 id）。ただし、その id が `numFmts` で別のコードに定義されていないものに限る。session 2 で index が同じになるテストと、`dxf_codes` を消す変異を殺すテストを加える。

### m1（Minor）: `DxfFormat.fill` に `Format` 流の solid fill（`fg` のみ）を渡すと、`patternType` 無しで `fgColor` だけの dxf になる

- 根拠: `build_dxf` は、`solid` のとき `patternType` を省き、`fg`/`bg` をそのまま書く。
  - Excel の dxf の solid 色は `bgColor` から取られる。
  - 一方 `Format` の solid fill は `fg` に色を置く（read-back もそう返す）。
  - そのため、`Format`/`FormatRead` の `Fill` をそのまま `DxfFormat.fill` に入れると、色が表示されない可能性が高い。
  - rust_xlsxwriter も dxf では入れ替えないので、同じ形になる。挙動はそれと同等である。
- 修正案: `DxfFormat::fill` の doc に「solid の色は `bg`」と明記する。`fg` だけの solid を拒否するか `bg` に移すかは Phase4 の Open question に書く。

### m2（Minor）: `dxfId` の走査が `x14:dxfs`（slicer style 用）への参照も本体 `dxfs` への参照として数える可能性がある

- 根拠: `scan_ids` は、すべての XML part の `…dxfId`/`…DxfId` を一律に本体 `dxfs` の参照として最大値を取る。
  - `x14:slicerStyleElement/@dxfId` が `styleSheet/extLst` の `x14:dxfs` を指す場合、本体 `dxfs` より長いと誤って `DanglingReference` になる。
  - 安全側の過剰拒否である。PR 本文のコーパス集計では dxf の拒否が 3 件（dangling または AC）あり、内訳は未確認。
- 修正案: 仕様を確認する。該当するなら、`styles.xml` の `extLst` 内の参照を本体の対象から外す。外さないなら、その旨を ADR note に書く。

### m3（Minor）: read-back の round-trip テストが、テンプレート由来の `xf` だけを対象にしている

- 根拠: 「same `s`」を確かめるマトリクスはすべて from-scratch ブックの正規形 `xf` で書かれている。Excel が書いた `xf`（`apply*` 付き、Normal に子があるもの）での read → write はテストに無い。M1 はこの穴から漏れた。
- 修正案: POI の日本語ファイルを 1 件使い、read → `set_format` で見た目が同じになることを確かめる（`alignment`/`protection` の子のバイトが同じこと）。sweep に加えてもよい。

## 他の open PR との干渉

- **#68（P5-pre、`CellWritePlan`）**: `edit.rs`/`worksheet.rs` で `edit_cell` の流れを組み替えている。#61 の `CellOp::InlineRich` と `edit_cell_ext` とは衝突するが、`match` は網羅的なので、取り込み漏れはコンパイルで捕まる。後からマージする側は、`refs_delta`（`Shared` だけを数える）の扱いを保つこと。
- **#53（Phase3）、#58（Phase4 part 1）**: `dxfId` を書く API を持たない。`styles.xml`/`sharedStrings.xml` の registry とは直接は衝突しない。`codegen/allowlist/sml.toml` と生成物は双方が触るので、マージ後に再生成が要る（いつもの手順）。

## 実施した検証

| 項目 | 結果 |
| --- | --- |
| CI（`gh pr checks 61`） | Rust 1.99 の test・clippy・`codegen --check`、python-pptx オラクルがすべて pass |
| rich string のエスケープと空白（`"  a&<>\"b "`、`rFont="A\"&<B"`、`_x0041_`、`\u{1}`、`\t`、空白だけの run） | 正しい。calamine と ooxml-xlsx の両方で同じ文字列に戻る。2 セッション目に再利用される（`uniqueCount=1`、`count=2`） |
| `x:` 接頭辞付きの sst に rich string を書く | `x:r`/`x:rPr`/`x:t` で正しい |
| plain `"x"` と、`Font::default()` の 1 run `"x"` | 別の `si` になる（`<t>` と `<r><t>`） |
| `dxfs` が無く、`tableStyles`/`colors`/`extLst` がある `styles.xml` | `dxfs` が `tableStyles` の前に `count="1"` 付きで挿入される |
| 変異: `"DxfId"` の needle を消す | `dxf_references_under_other_attribute_names_count_as_dangling` が落ちる（殺せた） |
| 変異: `dxf_codes` の参照を消す | **生き残る**（M2） |

## 判定

converged: no（Blocker 0 / Major 2 / Minor 3 / nit 0）
