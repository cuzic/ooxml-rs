# PR #57 レビュー（Opus, round 1）

対象: `impl/pptx-phase1b-tables` @ `2b3e343`（ADR-0013 rev 4 §2–§3、ADR-0002 post-acceptance note 追加）

## 結論（先に）

**Blocker なし、Major 1 件（M1）。converged: no。**

表構造編集の中核（行列 insert/delete の merge 規則、span→1 の除去、edge form への正規化、merge/unmerge のテキスト移動、a16 id 採番、clone-and-install の原子性）は、著者とは独立に作ったランダム連鎖モデルで **1,500 表 × 30 操作 = 45,000 操作（成功 25,445、拒否 19,555）で不一致 0**、python-pptx 読み戻しも 1,500/1,500 で問題なし。コーパス sweep・python-pptx oracle も再現した。codegen 変更は generator 経由で再生成と一致し（`codegen --check` OK、再生成で差分 0）、wml/sml の生成物は変わっていない。ビルトイン 74 style id は LibreOffice `predefined-table-styles.cxx` と完全一致。

残る Major は 1 件: `set_style` が大小文字・前後空白を無視して受理した文字列を**そのまま**書くため、ST_Guid パターン（大文字 hex）に違反する値や、`tableStyles.xml` の定義と綴りが違う id が出力される（M1）。修正は小さい（照合に成功した正規の綴りを書く）。他は Minor。

## 検証したこと（再現手順は scratchpad `review-51-57/`）

| 項目 | 結果 |
|---|---|
| `cargo xtask codegen --check`（git clone 上） | inputs verified, 7 files up to date。`cargo xtask codegen` 再実行で差分 0 |
| 生成物差分の範囲 | `dml.rs` / `pml.rs` のみ。`wml.rs` / `sml.rs` / `parts.rs` は無変更 |
| `cargo test -p ooxml-pptx --release` | 全 pass |
| 独立ランダム連鎖モデル（レビュアー作、`m57/scripts/{gen,check}.py` + `examples/rev_model.rs`） | python-pptx で 1,500 デッキ生成（1–5×1–5、python-pptx の edge form merge 0–3 個、30% は flag-only に変換、50% に a16 id（基数 1 / 10000 / 4294967290 で `u32::MAX` 近傍のフォールバックも通す）、20% のセルに tcPr 書式）。各表に insert/delete row/col・merge・unmerge・set_row_height・set_column_width のランダム**連鎖** 30 手。拒否ごとにバイト不変を assert。lxml で独自に XML を読み、拒否種別・全セルの段落テキスト列・merge 属性（touched merge は edge form 完全一致、untouched は flag と origin span）・行高/列幅・frame `cx`/`cy`・a16 id（保持 id 不変、新 id の一意性と削除済み id の非再利用）を照合 → **45,000 操作、不一致 0** |
| python-pptx 読み戻し（同 1,500 出力） | origin の span 範囲がすべて `is_spanned`、孤立/二重 spanned 0 |
| コーパス sweep（`table_sweep`、手元の LO 837 + POI 94 + ShapeCrawler 100 + fixtures = 617 distinct decks） | 114 表（著者は 633 decks / 119 表。OpenXML SDK・python-pptx test files が手元に無い差）。成功はすべて再オープン＋`check()` OK、拒否はすべてバイト不変。`FormatNotCopyable` 0、`NoTextBody`（merge）4 件 |
| python-pptx oracle `tests/oracle/tables.py` | sweep 出力 470 件 ok、`table_cases` 176 件 ok |
| ビルトイン style id | 74 件、ソート済み（`binary_search` 前提 OK）、LO ソースの GUID 集合と diff 0 |
| 手動 mutant（`cargo-mutants` 未導入のため 14 個を手で投入） | 著者 suite で非等価 survivor 3（X5, X7, X9）、下表 |


## Findings

### Major

**M1. `set_style` が受理した id を正規化せずに書く — ST_Guid 違反・定義との綴り不一致**
`crates/ooxml-pptx/src/write/table.rs:931-960`（照合 933-937、書き込み 952）、`crates/ooxml-pptx/tests/tables.rs:413-415`

照合は `trim()` + `eq_ignore_ascii_case`（定義側）／`trim().to_ascii_uppercase()`（ビルトイン側）なのに、書き込みは呼び出し側の文字列そのまま。プローブ:

```
set_style(Some(" {073a0daa-6af3-43ab-8588-cec1d06c72b9} "))
→ Ok, <a:tableStyleId> {073a0daa-6af3-43ab-8588-cec1d06c72b9} </a:tableStyleId>
```

- `a:tableStyleId` は `s:ST_Guid`（パターン `\{[0-9A-F]{8}-…\}`、**大文字のみ**、前後空白なし）なので、出力がスキーマ違反になる。既存テスト（413-415 行）は小文字がそのまま書かれることを**期待値として固定**している。
- LibreOffice の built-in 解決は GUID 文字列での表引きなので、小文字や空白付きでは built-in として解決されない可能性が高い（ADR-0013 §3 が根拠にしている「LO も built-in を描画する」が崩れる）。
- `tableStyles.xml` に定義された id と大小文字だけ違う綴りを受理した場合も、出力は定義と一致しない。
- 早期 return の比較（`current.as_deref() == id`、942 行）も生の文字列比較で、同じ style を別綴りで指定すると書き換えが走る。

提案: 照合でヒットした**正規の綴り**を書く。定義側は `self.styles` の該当要素（`trim()` 済み）、ビルトイン側は `BUILTIN_TABLE_STYLES` の要素（大文字）。`is_builtin` を `fn builtin(id) -> Option<&'static str>` にするのが簡単。

```rust
let canon: Option<String> = match id {
    None => None,
    Some(id) => Some(
        self.styles.iter().map(|s| s.trim()).find(|s| s.eq_ignore_ascii_case(id.trim())).map(str::to_owned)
            .or_else(|| crate::builtin_styles::builtin(id).map(str::to_owned))
            .ok_or_else(|| Error::TableStyleNotDefined(id.to_owned()))?,
    ),
};
// 以降は canon を比較・書き込みに使う
```

テスト 413-415 は「小文字で指定 → 大文字で書かれる」に変更し、空白付きのケースも追加する。

### Minor

**m1. `a:tblPr` に inline の `a:tableStyle` がある表で `set_style` が想定外の挙動になる**
`crates/ooxml-pptx/src/write/table.rs:938-960`

`CT_TableProperties` では `tableStyle` と `tableStyleId` が choice（生成 slot も `max_total: Some(1)`）。プローブ結果:
- `set_style(Some(builtin))` は `Err(Core(AlreadyPresent(tableStyleId)))`。原子性は保たれるが、型の付かない core エラーになる。
- `set_style(None)` は `Ok(())` を返すが、inline style は残る（`current` が `None` なので早期 return）。

コーパスでは 0 件なので Minor。提案: inline `a:tableStyle` があれば `set_style` を型付きエラー（例 `TableStyleInline`）で拒否するか、置き換えると決めて ADR-0013 §3 に一文を足す。

**m2. origin セルに `a:txBody` が無いと、移すテキストが無くても `merge` が `NoTextBody` で拒否される**
`crates/ooxml-pptx/src/write/table.rs:793`

`a:tc` の `txBody` は任意要素で、LibreOffice 出力（例: POI コーパスの `LIBRE_OFFICE-100610-0.pptx` は txBody の無いセルが 178 個、`bug60993.pptx` は 11 個）に普通にある。sweep でも merge 拒否 4 件の原因はこれ。ADR-0013 §2 の merge 拒否条件は「merged cell を含む／grid 外」だけ。
提案: `keep` が空なら origin の body は要らないので、そのまま進める。移すものがあるときは、origin に `TextBody`（`bodyPr` + 移した段落）を作る。少なくとも ADR の拒否一覧に明記する。

**m3. 表ロックで「未知の anchor list」が、その anchor の指す id に関係なく slide 上の全表をロックする**
`crates/ooxml-pptx/src/refs.rs:170-194`（特に 173 行の `KNOWN` と 191 行）

プローブでは、`ac:spMkLst` / `ac:picMkLst`（別 shape id=99 を指す）が付いた slide で、表 id=4 の `insert_row` が `CommentAnchoredTable` になった。Phase 0 の `scan_comments` は、未知の名前でも `ac:*@id` があればその id だけをロックする。表ロックは同じ入力で `all = true` になり、ADR-0005 §6 と粒度が揃っていない。安全側なので Minor。
提案: 未知の名前でも、`inside` でなく ac id があるときは「その id の shape だけ」をロックする（表でない id ならロック不要）。現状のままにするなら、ADR-0013 §2 の「cannot verify」がこの扱いを含むことを明記する。あわせて 179 行・191 行の `|| local == "gridColMkLst"` は冗長（`TABLE_LISTS` に含まれ、`KNOWN` に入れれば済む）。

**m4. 新たに書き込む型が R2-6 の compile-time assert に入っていない**
`crates/ooxml-pptx/src/write.rs:449-467`

`set_attr` / `set_text` は `ATTRS_INCOMPLETE` の型を実行時に拒否する（`IncompleteSpec`）。今回 `TableCell`、`TableRow`、`GridColumn`、`TableProperties`、`TableCellProperties`、`{Left,Right,Top,Bottom}BorderLineProperties`、`RgbColorModelHex`、`SchemeColor`、`Extension`、`RowIdIdentifier`、`ColIdIdentifier`、`dml::Extents`、`TableStyleId` に set_attr/set_text するが、assert は Phase 0 の型のままになっている。現状の生成物には `ATTRS_INCOMPLETE = true` の型が 1 つも無いので実害はない。ただ、将来 allowlist や SDK の更新で incomplete になったときに、コンパイル時ではなく実行時の拒否になる。
提案: 同じ `const _: () = assert!(…)` に上記の型を追加する（table.rs 側に置いてもよい）。

**m5. wildcard-parent 規則で `ChildOf<GraphicData>` が `RowIdIdentifier` / `ColIdIdentifier` にも生成される（ADR-0002 note に記載なし）**
`crates/ooxml-schema/src/generated/dml.rs:678-679`、`docs/adr/0002-schema-driven-code-generation.md:319-348`

`a16:rowId` / `a16:colId` を allowlist に入れた結果、既存の「wildcard parents」規則によって、`a:graphicData` 直下に typed insert できる型にもなっている（`graphic_data.row_id_identifier_iter()` 等も生成される）。既存の `wml::Document` 等と同じ規則による副作用で、害は小さい。ただ、note は「ChildOf<a:ext> exists for the two ids」としか書いていない。ADR-0002 の到達型レビューの規律に照らすと、記載するか、除外ルールを入れるべき。
提案: note の (1) に「wildcard parents 規則により `ChildOf<a:graphicData>` も生じる（意味上は不正だが format crate は使わない）」と一文足す。可能なら generator 側で、global element でも `a16:CT_Identifier` は wildcard-parent 対象から外す。

**m6. 入力の a16 id の重複を `check()` が検出しない**
`crates/ooxml-pptx/src/write/table.rs:437-446`

「一部の行だけ id がある」は拒否するが、重複（例 `[1, 4, 3, 4]`）は通る。新しい id は used ∪ deleted の max+1 なので出力側で新たな重複は作らない。ただし、重複 id の行を消すと deleted 集合と生存行の id が重なり、comment anchor の意味も曖昧になる。
提案: `ids()` で重複を `TableNotWellFormed("duplicate a16:rowId")` にする（ADR-0013 §2 の well-formed 定義にも一語足す）。

**m7. コメントと実装の不一致（nit）**
`crates/ooxml-pptx/src/write/table.rs:679-680`

`// new cells sit one level shallower than new rows' cells` と書いてあるが、`check_formats(&fmts, 0)` で行と同じ深さを仮定している（安全側なので動作は正しい）。コメントを「conservatively the same levels as rows」に直す。

**m8. テストの穴 — 著者 suite で 3 つの非等価 mutant が生き残る**
`crates/ooxml-pptx/tests/tables.rs:140-165, 430-448`、`crates/ooxml-pptx/src/write/table.rs:504-510`（`edit`）

- X5: `delete_row` / `delete_column` で frame の `cy` / `cx` が縮むことを検証していない。`frame_ext` の assert は insert と set_* にしかない。
- X9: `insert_column` が左の列から幅と書式（`tcPr` 等）を取ることを検証していない。行側は検証している。
- X7: clone-and-install（PR 本文の「any refusal, ours or the core's, leaves the slide byte-identical」）を検証していない。事前チェックで弾けない core 側のエラーを、closure の途中で起こすケースが無い。

提案: delete 後の `frame_ext` assert を足す。左右で書式の違う列を持つ表で `insert_column(at>0)` を検証する。closure 内の途中失敗は、`#[cfg(test)]` のフォールト注入（例: N 回目の `apply_attrs` 書き込みで `Err`）で検証する。

## 手動 mutant 結果（14 個）

| # | mutation（`write/table.rs`） | 著者 suite（lib + tables + table_model） | レビュアー連鎖モデル |
|---|---|---|---|
| X1 | `canon` の origin 行 covered セルの rowSpan を常に 1 | killed | killed |
| X2 | insert_row の merge 内判定 `at < r0+nr` → `<=` | killed | killed |
| X3 | delete_row で nr=1 の merge origin 行も拒否 | killed | killed |
| X4 | `new_id` が deleted を無視 | killed | killed |
| X5 | delete_row で frame `cy` を縮めずに伸ばす | **survived** | killed |
| X6 | 表コメントロック無効化 | killed | （対象外）|
| X7 | `edit()` を clone せず直接適用（非原子化） | **survived** | survived |
| X8 | merge で単一空 origin 段落を残す | killed | killed |
| X9 | insert_column の書式/幅の取得元を常に列 0 | **survived** | killed |
| X10 | set_border で隣接セル側の線を書かない | killed | （対象外）|
| X11 | delete_column で c0 列の merge も縮める | survived | survived（等価: c0 列で nc>1 は事前に拒否）|
| X12 | unmerge で covered の flag を残す | killed | killed |
| X13 | parse_grid が edge/flag-only 混在を許す | killed | （対象外）|
| X14 | set_style の built-in 照合を無効化 | killed | （対象外）|

著者 suite の非等価 survivor は X5・X7・X9 の 3 件（→ m8）。

## 判定

**converged: no** — M1（style id の正規化）を直せば、次ラウンドで収束見込み。Minor は同じ PR で直すのが望ましいが、収束条件には含めない。
