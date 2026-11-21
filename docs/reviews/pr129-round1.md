# PR #129 レビュー（Opus, round 1）

対象: impl/xlsx-phase5d-sparklines-dynamic @ f0240138555f9734a27df51bf45540b766b3f2f9

## 結論（先に）
§8・§9 の主経路は ADR どおり。プローブでも Blocker と Major は見つからなかった。
x14 `extLst` への追記（既存の ext の順序とバイトを保持、`xmlns:x14`/`xmlns:xm` は局所的に宣言、
別プレフィックスで束縛された x14 も再利用）、`metadata.xml` の新規作成と再利用、`cm`、`f t="array" ref=アンカー`、
`fullCalcOnLoad`、calcChain の削除、拒否時のバイト不変は、いずれも期待どおりに動いた。
**converged: yes**。残りは Minor 3 件と nit 1 件で、follow-up に回してよい。

## 前ラウンドの解消状況
初回のため該当なし。

## 確認したこと（プローブ。コミットしていない）
- 空のブックに 2 回 `set_dynamic_formula`: `metadata.xml` は 1 個（`cm="1"` を再利用）。workbook の `.rels` に
  `sheetMetadata`、CT に `sheetMetadata+xml` の Override、`calcPr fullCalcOnLoad="1"`、`dimension` は `A1:D5`。
  アンカーへの上書きは `ArrayAnchor`。スピル予定範囲（C2）への値の書き込みは通る（ADR どおり）。
- `poi/xlookup.xlsx`（既存の XLDAPR）: `metadata.xml` はバイト一致で、`calcFeatures` の ext も保持される。
- 既存の sparkline ext で x14 が `q:` に束縛されている場合: 新しいグループは `q:sparklineGroup` として
  既存の `q:sparklineGroups` に追記され、宣言は重複しない。
- `'data''s'!$A$1:C1`（大文字小文字違い）: 実際のシート名 `'Data''s'` に正規化され、`$` は保持される。
- 参照先シートの rename/delete（再読込後、同一セッション、自己参照）: すべて `UnrewritableReference`。
  これは ADR-0009 の注記（x14 の `xm:f` は書き換えずに拒否）どおり。
- 共有文字列のセルを動的配列で上書きすると `sst/@count` が減らない。これは Phase1 の `set_number` と同じ既存の
  挙動で、P5d による退行ではない。
- x14 の代替 fixture: 取得済みの POI `ConditionalFormattingSamples.xlsx` の 18 シートには、`<ext>` が 1 つもない
  （確認済み）。代わりに使った `NewStyleConditionalFormattings.xlsx` は Excel が書いたファイルで、ルートに
  `mc:Ignorable="x14ac"` を持ち、x14 の CF ext もある。`verify_extension_append` は残りのバイトの一致まで
  見ているので、代替として十分である。既存の sparkline ext への追記は、合成ケースとコーパスの
  `LIBRE_OFFICE-128382-0.xlsx`（最初のシートに sparkline あり）で通っている。

## Findings
### m1（Minor）: census の動的アンカー判定が狭まり、`fCollapsed="1"` を含むブック全体が編集不可になる
- 根拠: `census.rs` の `dynamic_array_cms` が `crate::dynamic::active`（`fDynamic` が真で、`fCollapsed` が無いか偽）を使うようになった。
  `xlookup.xlsx` の `metadata.xml` で `fCollapsed="0"` を `"1"` に変えると、無関係なセル（U61）への `set_number` が
  `Err(UnknownCellMetadata { cell: "C2" })` になる。元のファイルなら `Ok`。main ではこの判定に `fDynamic` しか見ていないので、
  このブックは編集できていた。ADR-0003 のアンカー判定は `fDynamic="1"` で、`fCollapsed="0"` は ADR-0017 §9 の
  **再利用**の条件にすぎない。安全側の過剰拒否だが、Phase1 の契約の変更が ADR の注記に書かれていない。
- 修正案: census は `fDynamic`（と xda 名前空間・パス）だけで判定する。`fCollapsed` の条件は `ensure_block` の
  再利用判定だけに使う（再利用用の集合を別に持つ）。`fCollapsed="1"` のブックが編集できることを確かめるテストを足す。

### m2（Minor）: 同じセルへの sparkline の重複を受け入れる
- 根拠: `.add("A1", …).add("A1", …)` の 1 グループ、既存グループと同じ `A1` への別グループの追加、どちらも `Ok`。
  Excel の UI では 1 セルに 1 sparkline しか置けない。重複がある場合に修復ダイアログが出るか、どちらか一方が
  黙って捨てられるかは、未確認（XL 項目）。
- 修正案: 同じシートの既存の `xm:sqref` と新しいグループ内で宛先が重複したら、適用前に拒否する
  （拒否のテストはバイト不変で）。Excel で問題がないと分かれば、その旨を ADR の注記に書いて閉じる。

### m3（Minor）: 関係を持たない `xl/metadata.xml` があると、動的配列が拒否される
- 根拠: 空のブックに、どこからも参照されない `xl/metadata.xml` を zip で足す。この状態で `set_dynamic_formula` を呼ぶと
  `Err(Core(RegistryRefused { reason: NameInUse }))` になる。保存結果はバイト不変（確認済み）なので安全側。
  原因は `PartNameRequest::Exact` で名前を決め打ちしていること。
- 修正案: `Numbered` で名前を割り当てる（`metadata2.xml` など）か、この拒否を ADR の注記に書く。

### n1（nit）: ADR-0017 §8 の「Phase3 rename/delete must rewrite `xm:f`」が実装と食い違う
- 根拠: 実装と ADR-0009 の注記では、rename/delete を `UnrewritableReference` で拒否する。この挙動は PR 本文にはあるが、
  P5d の post-acceptance note には書かれていない。
- 修正案: P5d の注記に 1 文を足す（x14 の `xm:f` が参照するシートの rename/delete は拒否する）。

## 他の open PR との干渉
#125（P7-pre）が `crates/ooxml-xlsx/src/{edit.rs,error.rs,lib.rs}` に触れている。どちらかが先にマージされたら、
もう一方で main を取り込む必要がある。

## Excel で確認する項目（XL）
- `42-p5d-sparklines.xlsx`: `colorSeries` などの色要素を持たない `x14:sparklineGroup` が、修復なしで描画されるか。
- `43-p5d-dynamic-array.xlsx`: `xmlns:xda` が `metadata.xml` のルートではなく `ext` で宣言されたファイルを修復なしで開き、
  C1:C3 にスピルするか。保存し直したときに Excel が何を書くか（`ref` の範囲、`fCollapsed`）。
- 既存の CF/DV/slicer の ext の後に sparkline の ext が追加された順序を Excel が受け入れるか（ADR の m11）。
  POI の `NewStyleConditionalFormattings.xlsx` に追加したもので確認する。
- 同じセルに sparkline が重複した場合（m2）。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 3 / nit 1）
