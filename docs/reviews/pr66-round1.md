# PR #66 レビュー（Opus, round 1）

対象: impl/docx-phase2-part2 @ ad627fa83e6979b5e13531789c09a730798c2b96（#62 との差分 `origin/impl/docx-phase2...origin/impl/docx-phase2-part2` のみ）

## 結論（先に）
converged: no。Major が 3 件ある。`delete_rows` の vMerge 修復が別々の縦結合をつないでしまう。
`merge_cells` と `delete_column` は、変更履歴を持つセルを `Ok` のまま消す（ADR-0011 §7 の M6 に違反）。
`insert_image` は範囲外の EMU（最大 `u64::MAX`）を書く。
次の点は問題なかった: tblGrid と gridSpan の整合、行挿入時の vMerge 継続、入れ子の表、メディアの再利用と rId の再利用、
Default/Override の判定、docPr id の走査（ヘッダー、`mc:Fallback`）、alt のエスケープ、各拒否での Err ⇒ byte-identical。CI は緑。

## 前ラウンドの解消状況
なし（round 1）。

## Findings

### M1（Major）: `delete_rows` が、範囲内で始まる縦結合の continue を、上の別の結合にくっつける
- 根拠: `tables.rs:505-507`。`continued_from_above` は「範囲のすぐ上の行が、その列で何らかの vMerge に属するか」しか見ていない。
  「`next` が続けている結合の restart が範囲内にあるか」を見ていない。
  再現: 1 列の表を 4 行（A restart / continue / B restart / continue）にして `delete_rows(t, 1..3)` を呼ぶ。
  結果は `Ok` で、残った 2 行は `restart(A)` と `<w:vMerge/>` になる。B の結合だった最後の行が A の結合に吸収される（§7.2 の修復規則に反する）。
  このガードを外す変異もテストで落ちないので、この経路にはテストが無い。
- 修正案: `range.end - 1` から上へ、`c.start` の列で vMerge が continue でない最も近い行を探す。その行が削除範囲内なら restart に直す。
  上の 4 行のケースを回帰テストに加える。

### M2（Major）: `merge_cells` と `delete_column` が、変更履歴を持つ「空」セルを Ok で消す
- 根拠: `cell_is_empty`（`tables.rs:234`）は段落に `w:pPr` しか無いことを見るだけで、`w:pPr` と `w:tcPr` の中身は見ない。
  `merge_cells`（`:808-813`）と `delete_column`（`:715-722`）は `check_removable` を呼ばない。
  プローブ: 消える側のセルに `tcPr/cellIns`、`tcPr/tcPrChange`、段落記号の `pPr/rPr/ins`、`pPr/pPrChange` のどれかを置くと、
  `merge_cells(t, 0..1, 0..2)` も `delete_column(t, 1)` も `Ok` を返し、その要素は消える。
  同じセルの行を `delete_rows` で消そうとすると、正しく拒否される。
  §7（revision 2, M6）は、`delete_column` と `merge_cells` についても「除去範囲に変更履歴があれば `InsideRevision`」と定めている。
  `trackRevisions` が off だと W1 では止まらず、変更履歴を黙って承認または却下したことになる。
- 修正案: 消すセル（merge では左上以外で除去するセル、delete_column では Remove 対象）に `check_removable` を適用する。
  4 種類の要素それぞれについて、Err ⇒ byte-identical のテストを付ける。

### M3（Major）: `insert_image` の extent が ST_PositiveCoordinate（0..27273042316900）を超えても Ok になる
- 根拠: `images.rs:92`。pHYs が 1〜19 px/m だと、`round(x*0.0254)` は 0 dpi になる。`emu()`（`:143`）で 0 除算が起きて ∞ になり、
  `as u64` で飽和するので `u64::MAX` になる。
  プローブ: pHYs 1 px/m の PNG を `ImageSize::Auto` で入れると、`<wp:extent cx="18446744073709551615" cy="18446744073709551615"/>` が書かれ、`a:ext` も同じ値になる。
  `ImageSize::Emu { cx: u64::MAX, .. }` もそのまま通る（`:216-222` は 0 しか拒否しない）。
  この値は xsd:long の範囲も超えるので、Word は開けない。
  ooxml-docx 自身は再読込できてしまうので、既存のテストでは気づけない。
- 修正案: 丸めた dpi が 0 なら「DPI 無し」（96）として扱う。cx と cy が 1..=27273042316900 に収まらなければ、変更前に `InvalidValue` で拒否する。
  境界値のテストを付け、ADR の note に一行加える。

### m1（Minor）: `merge_cells` の下端チェックに対応するテストが無い
- 根拠: `tables.rs:815-819`（下の行が rectangle 内の列を continue していれば `MergeOverlaps`）を `&& false` にする変異が、tables/images の両テストを通ってしまう。
  挙動は正しい。
- 修正案: 1 行だけの水平結合で、下の行がその列を continue している表を拒否するテストを加える。

### m2（Minor）: span を広げる・狭める操作で tcW が更新されない
- 根拠: `insert_column` の Widen（`:644-648`）と `delete_column` の Narrow（`:742-750`）は `gridSpan` だけを変える。
  `merge_cells` では、幅の無い gridCol が 1 つでもあると、左上セルの元の `tcW` がそのまま残る（`:820`、§Part 2 note のとおり）。
  壊れたファイルにはならない（tcW は推奨幅にすぎない）が、tblLayout が fixed の表では見た目が崩れる。
- 修正案: tcW が dxa で、対象の gridCol に幅がそろっていれば、加減して直す。直さない場合は、その旨を note に明記する。

### m3（Minor）: `codegen/allowlist/dml.toml` と生成物 `dml.rs` が他の PR と衝突する
- 根拠: #63（`dml.toml` の names 末尾に追記し、`dml.rs` を再生成）と #51（`dml.rs`）が同じファイルを触る。
  #63 は `a:CT_DataModelExtension/a:ext`、本 PR は `a:CT_PositiveSize2D/a:ext` を追加する。
- 修正案: 後からマージする側が `cargo xtask codegen` で再生成し、同じ名前 `a:ext` から作られる型名が衝突しないことを確認する。
  ADR-0005 の owner による a: entries の確認は、ADR note に書かれたとおり残っている。

## 判定
converged: no（Blocker 0 / Major 3 / Minor 3 / nit 0）
