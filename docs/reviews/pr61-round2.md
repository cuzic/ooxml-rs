# PR #61 レビュー（Opus, round 2）

対象: `impl/xlsx-phase2-part2` @ `30a260432345d5c889b57bdaa4411e4625d5481f`（round 1 は `0e52cd7`。修正 `20b4449`、main のマージ `ddaa458`・`998777a`・`30a2604`。最後のマージで #58 Phase4 と #67 census gate を取り込んだ）

検証には `git clone --reference` で作った複製を使った。プローブと変異はコミットしていない。CI（Rust 1.99 の test・clippy・`codegen --check`、python-pptx オラクル）はすべて緑だった。

## 結論（先に）

**収束（converged: yes）。Blocker 0、Major 0、Minor 2、nit 1。**

round 1 の M1・M2・m1〜m3 は、挙動で確かめて解消していた。main のマージの解決（`--remerge-diff`）でも、どちらの側の挙動も失われていない。残りは次の 2 点で、どちらも安全側である。

- `add_differential_format` が workbook 単位の edit gate（`check_editable`）を通らない。
- dxf の `numFmt` を seed するときの安全側ガードにテストが無い。

## 前ラウンドの解消状況

- **M1 解消**: `read_format` は、子が無い `xf` について `alignment`/`protection` を `Some(default)` で返すようになった。
  - round 1 のプローブ（Normal に `<alignment vertical="center"/><protection locked="0"/>`）では、B1 が `s=0` に戻り、`styles.xml` はバイト単位で変わらない。
  - 別の形も確かめた。`alignment` だけを持つ `xf`（`wrap`）が、Normal の `protection` を拾わずに同じ `s` に戻る。
  - 変異（`Format::new()` に戻す）は 2 件のテストで殺せた。
- **M2 解消**: `activate` は、既存の `dxf/numFmt` の `formatCode → numFmtId` を `dxf_codes` に seed するようになった。
  - `set_num_format("0.000").set_bold()` を 3 セッションで追加すると、index は 0・0・0、`dxf` は 1 個のままだった。
  - `numFmts` が同じ id を別のコードで定義している場合（`dxf` 164=`0.0000`、`numFmts` 164=`0.0`）は seed しない。このとき index は 1・1・1（165 で 1 回だけ追加）で、これも安定する。
  - 変異 2 件は殺せた。seed を消す変異と、`dxf_codes`/`custom_codes` の参照順を入れ替える変異である。
- **m1 解消（doc）**: `DxfFormat::fill` の doc に「solid の色は `bg` に置く」と書いた。
- **m2 解消**: `styles.xml` の `extLst/ext/x14:slicerStyles/…/x14:slicerStyleElement` の開始タグにある、接頭辞無しの `dxfId` を走査の対象から外した。
  - 名前空間と経路で照合し、それ以外は従来どおり保守的な suffix 走査にしている。
  - 除外を無効にする変異は `slicer_style_references_use_the_extension_dxfs` で殺せた。
- **m3 解消**: POI の `62272.xlsx`（Normal が `numFmtId` 164 と `alignment`/`protection` の子を持つ）で、read → write → 再読込の一致を確かめるテストが加わった。

## マージ解決の確認

- `30a2604` の衝突は 4 ファイルで、いずれも両側の和として正しく解決されている。
  - `worksheet.rs`: `plan_target` と `reusable` の 2 か所の `match` に、`CellOp::Clear`（main）と `CellOp::InlineRich`（PR）の両方がある。
  - `codegen/allowlist/sml.toml`: Phase2 part 2 の書き込み要素と Phase4 の要素の両方がある。重複していた `x:CT_RElt/x:r` は Read 側から外れて Write 側だけに残った。`mergeCells` は main の Write 側に残った。
  - `report.txt` と生成物: CI の `codegen --check` が緑である。
- `ddaa458` と `998777a` には衝突が無い。`git diff 8610086 HEAD -- crates/ooxml-xlsx/src` で消えた行は次の 3 つだけで、main 側の挙動は失われていない。
  - `font_from_node` を `read.rs` に移した。
  - `lib.rs` の re-export を広げた。
  - 上の 2 か所の `match` の腕を変えた。
- main 側の規則が `InlineRich` にも効くことを、挙動で確かめた。
  - **`check_editable` gate**: F-8 のブック（`<row r="1">…<c r="B2">`）では、`set_rich_string` は `InvalidReference` を返す。パッケージはバイト単位で変わらない。
  - **merged 範囲の規則**: `A1:B2` の非 anchor の B2 に rich string を書くと `MergedCell` になり、パッケージは変わらない。anchor の A1 には書ける。
  - **Phase4 `Clear`**: `merge_clearing` が rich のセルを空にする（sst の場合も inline `is/r` の場合も）。
    - sst では `count` が 1 減る。`refs_delta` の扱いは保たれている。
    - inline では `t="inlineStr"` と `is` が両方消える。
- テストの結果: `phase2_part2`・`phase2`・`phase2_review1`・`phase4`・`row_mismatch` がすべて通った。

## Findings

### m1（Minor）: `add_differential_format` が `check_editable` を通らず、`edit_refusal()` が `Some` のブックでも `Ok` で `styles.xml` を変える

- 根拠: `setters.rs:252`。`self.styles()?` は poisoning しか見ず、census の block は見ない。
  - プローブ: F-8 のブックでは `edit_refusal()` が `Some(InvalidReference)` を返す。それでも `add_differential_format(&DxfFormat::new().set_bold())` は `Ok(0)` を返し、出力は変わった。
  - `edit_refusal` の doc は「`Some(error)` if every edit of this workbook is refused」と書いている。#67 M2 は range 操作をこの gate に揃えた。
  - 変わるのは `styles.xml` だけで、セルの重複のような破損は起きない。そのため Minor とした。
- 修正案: 冒頭で `self.check_editable()?` を呼び、`row_mismatch.rs` の拒否一覧に `add_differential_format` を加える。

### m2（Minor）: dxf の `numFmt` を seed するときの安全側ガード 2 つにテストが無い

- 根拠: `styles.rs` の `activate` の seed にある 2 つの部分は、変異させても `phase2_part2`・`phase2`・`phase2_review1` が全通過した。
  - `st.codes.get(&id).is_none_or(|existing| existing == code)` を消す変異。消すと、`numFmts` が別のコードで定義している id を新しい dxf が再利用する。
  - `.min(id)` を「後勝ち」に変える変異。重複したコードのときに選ばれる id が、決定的でなくなる方向に変わる。
- 修正案: 次の 2 つのテストを加える。
  - `dxf` 164=`0.0000` と `numFmts` 164=`0.0` の組で、新しい dxf が 165 を使い、2 セッション目に同じ index になること（上のプローブ）。
  - 同じコードの `dxf` が 2 つ（id 170 と 166）あるとき、166 が選ばれること。

### n1（nit）: `cellXfs` が無いブックの `s=0` だけは、まだ `Format::new()`（inherit）を返す

- 根拠: `read.rs` の `cell_xfs.is_none() && s == 0` の分岐では、M1 の修正が効かない。`cellStyleXfs[0]` に子があると、書き戻したときに Normal の配置と保護をコピーする。
  - `cellXfs` が無くて `cellStyleXfs` だけがあるファイルは、まず無い。
- 修正案: ここでも `alignment`/`protection` を `Some(default)` にそろえる。

## 他の open PR との干渉

- **#68（P5-pre、`CellWritePlan`）**: ADR 補遺の main 差分一覧には「`CellOp::{Clear, Shared, Bool, Error, StyleOnly}`」とあり、`InlineRich` が入っていない。
  - #61 が先にマージされたら、#68 の多セル経路は `write_rich` の inline 経路（`Extra::default()` で `InlineRich`）も受ける必要がある。少なくとも `match` を網羅し、`refs_delta`（`Shared` だけを数える）を保つこと。
  - `match` は網羅的なので、取り込み漏れはコンパイルで捕まる。
- **#72（ignorable wrappers）**: `edit.rs`/`worksheet.rs` を触るが、`CellOp` を網羅する `match` を足していない。テキストの衝突が起きるだけである。
- **#53（Phase3）**: `codegen/allowlist/sml.toml` と生成物を共有する。マージ後に再生成が要る（いつもの手順）。

## 実施した検証

| 項目 | 結果 |
| --- | --- |
| CI（`gh pr checks 61`） | test・clippy・`codegen --check`、python-pptx オラクルがすべて pass |
| M1 のプローブ（round 1 の形、`wrap` だけの `xf`） | どちらも同じ `s` に戻り、`styles.xml` は変わらない |
| M2: 3 セッションで `numFmt` 付きの dxf を追加 | index は 0・0・0、`dxf` は 1 個 |
| M2: `numFmts` と id が衝突する dxf | seed しない。165 で 1 回だけ追加され、以後は 1・1・1 |
| F-8 のブックに `set_rich_string` | `InvalidReference`、バイト単位で同じ |
| F-8 のブックに `add_differential_format` | **`Ok(0)` で `styles.xml` が変わる**（m1） |
| merged 範囲の非 anchor に rich、anchor に rich、`merge_clearing`（sst と inline） | 規則どおり。sst の `count` が減り、`is` と `t` が消える |
| 変異: seed を消す、`Format::new()` に戻す、参照順を入れ替える、slicer の除外を無効にする | すべて殺せた |
| 変異: `codes` の衝突ガードを消す、`min` を後勝ちにする | **生き残る**（m2） |
| 変異: slicer の除外で、直前が空白かの判定を消す | 生き残る。接頭辞付きの名前を除外しないための保守側の判定なので、問題ない |

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 1）
