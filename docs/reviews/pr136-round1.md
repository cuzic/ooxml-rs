# PR #136 レビュー（Opus, round 1）

対象: fix/followups-pr129 @ 721874e613d3e5266d4b7d53f062d554ba0effae

## 結論（先に）
Issue #133 の 4 項目（129-m1〜m3、n1）はすべて挙動で解消を確認した。`fCollapsed` は census から外れて
`ensure_block` の再利用判定だけに使われ、孤立した `xl/metadata.xml` があるときは番号付きの空き名に part を足す。
壊しにいったプローブ（Err ⇒ byte-identical、局所性、変わったターゲットパス、複数の metadata 関係）でも
Blocker と Major は見つからなかった。**converged: yes**。残りは nit 2 件。

## 前ラウンド（PR #129 round 1）の解消状況
- 129-m1: 解消。census は `fDynamic` だけで判定（`scan_dynamic_array_cms(xml, |_| Ok(true))`）、`fCollapsed` は
  `ensure_block` のクロージャだけ。`fCollapsed="1"` のブックで無関係なセルの `set_number` が通り、metadata はバイト不変。
- 129-m2: 解消。重複を許すテスト（`sparkline_groups_can_share_a_destination`）と ADR-0017 P5d 注記の 1 行。
- 129-m3: 解消。`Exact("xl/metadata.xml")` が `NameInUse` のときだけ `Numbered("xl/metadata{}.xml")` に切り替える。
  孤立 part はバイト不変、workbook の関係と `sheetMetadata+xml` の Override は新しい名前に張られる。
- 129-n1: 解消。P5d 注記に `UnrewritableReference` の 1 行。

## 確認したこと（プローブ。コミットしていない）
- **m1 の cm/vm の整合**: `C1` を動的配列にして `fCollapsed="1"` に書き換えたブックで、`E1`・`G1` を同じセッションで書くと
  新しい future `bk`（index 1、`fCollapsed="0"`）と cellMetadata `bk`（`rc t="1" v="1"`）が 1 つだけ足され、
  `E1`/`G1` は `cm="2"`、`C1` は `cm="1"` のまま。`futureMetadata/@count=2`、`cellMetadata/@count=2`。
  再読込して `I1` を書くと `cm="2"` を再利用し、metadata はバイト不変。既存の collapsed ブロックは無変更。
- **m1 の混在**: future `bk0` が collapsed、`bk1` が active で、cellMetadata は `bk0` を指す 1 個だけのブック:
  新しい書き込みは future を増やさず `bk1` を再利用し、`rc t="1" v="1"` の cell ブロックだけを足す（`count=2`）。
- **m1 の拒否**: collapsed のブックで `set_dynamic_formula("SORT(A1:A3)")`（接頭辞なし）は Err で保存結果はバイト不変。
- **m3 の衝突回避**: `xl/metadata.xml` と `xl/metadata1.xml` が両方とも孤立し、どちらも `sheetMetadata+xml` の
  Override を持つブック: 新しい part は `xl/metadata2.xml`、関係は 1 本（`metadata2.xml`）、Override は 3 つとも正しい型、
  孤立 part 2 つはバイト不変。同じセッションの 2 回目の書き込みは新しい part を再利用（part の追加は 1 回）。
  再読込で `edit_refusal()` は `None`、次の書き込みは `metadata2.xml` を再利用して part はバイト不変。
  `Numbered` は既存の part・Override・関係ターゲット（dangling を含む）の最大番号 + 1 を取る（core の仕様）ので、
  rel target や Override との衝突は起きない。`Exact` の失敗は `RegistryPlan::step` がコピー上で行うため、
  失敗した 1 回目の試行は plan に何も残さない。
- **変わったターゲットパス**: workbook の `sheetMetadata` 関係が `../custom/meta/m.xml` を指すブック:
  その part を編集して再利用し、`xl/metadata*.xml` は作らない。関係は不変、再読込で `cm="1"` が解決される。
- **複数の metadata 関係**: 同じ part への `sheetMetadata` 関係が 2 本: `PartNotEditable`（ambiguous）で、保存結果はバイト不変。
- **変異**: census のクロージャを「uncollapsed のみ」に戻す → `collapsed_dynamic_anchor_allows_edits_without_reusing_its_block`
  が `set_number` で落ちる。`ensure_block` のクロージャを `Ok(true)` にする → 同じテストが新ブロックの `cm` で落ちる。
  m3 は PR 本文のとおり修正前のコードで `NameInUse` になる。
- CI（test/clippy/codegen、python-docx/pptx、openpyxl、LibreOffice）はすべて pass。

## Findings
### n1（nit）: P5d 注記の「the first compatible XLDAPR cell metadata block」の compatible が未定義
- 根拠: `docs/adr/0017-xlsx-phase5-objects.md` P5d 注記。m1 の修正で「`fDynamic` が真かつ `fCollapsed` が無いか偽」が
  再利用の条件、census のアンカー判定は `fDynamic` だけ、と分かれたが、注記には書かれていない（PR 本文にはある）。
  同様に、孤立 `metadata.xml` があるときの `metadataN.xml` への切り替えも PR 本文だけにある。
- 修正案: 注記に「compatible = fDynamic 真かつ fCollapsed 偽/無し。名前が使用中なら `metadataN.xml`」を 1 文足す。

### n2（nit）: m2 のテストは同じセッション内の 2 グループだけ
- 根拠: `sparkline_groups_can_share_a_destination` は新規ブックで 2 回 `add_sparklines`。round 1 の m2 で挙げた
  「既存（再読込後）のグループと同じセルへの追加」は固定されていない。現状の挙動は `Ok` で正しい。
- 修正案: 必要なら、再読込後に同じ宛先へ足すケースを 1 つ足す。

## 他の open PR との干渉
`gh pr diff 125 --name-only` を含め、open PR（#125, #131, #132, #135）は `dynamic.rs`・`census.rs`・`phase5d.rs`・ADR-0017 に触れていない。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
