# PR #53 レビュー（Opus, round 3）

対象: `impl/xlsx-phase3` @ `09c5e40`（round 2 は `33b4771`）

見たもの: round 2 の修正 `5e2f480`、main の merge `df284bc` と `88bd8f0`（`git show --remerge-diff`）、テスト追加 `d42e4d0` と `09c5e40`。
プローブはスクラッチ領域で回し、コミットしていない。round 2 のプローブ `review53r2.rs`（Q1〜Q6）を再実行し、新しく `review53r3.rs`（S1〜S7）を書いた。S1〜S7 は、sst の count、Err のあとの byte-identical、styles registry、Phase4 との組み合わせ、F-8 を見る。
手書きの変異は 10 個（Y1〜Y10）入れた。`phase2`、`phase2_review1`、`phase3`、`phase3_review1`、`phase3_smoke`、`phase4` を `fault-injection` 付きで回した。CI は green である。

## 結論（先に）

**未収束（Major 1）。** R2-B1 は直った。`R&D`、`開発&設計`、`It's`、`D&#97;ta`、`a<b"c` の pivot ソースは書き換わり、`P&L` に rename してから戻す往復も通る。R2-m1〜m4 も挙動で直っている。

main の merge の手解決は、両側の挙動を残している。ただし、今回入った「structural op のあとで Phase2 の registry を捨てて作り直す」処理（`rebuild` の `self.registries = Default::default()`）に穴がある。作り直した registry は、同じセッションで追加済みの `si`、`xf`、`font` を再利用の候補に入れない。そのため、同じ文字列や書式を書くたびに重複した要素が追加される（M1）。ファイルは壊れないが、ADR-0006 の exact-form reuse に反する。また、in-session の状態が reopen したときの状態と一致しない。

sst の count、F-8 ゲート、Err ⇒ byte-identical、Phase4 の autofilter、merge、protection との組み合わせは、正しく動いた。

## 前ラウンドの解消状況

| 指摘 | 状態 | 確認 |
|---|---|---|
| **R2-B1** 生の名前の絞り込み | 直った | Q1 と Q5 を再実行した。`R&amp;D`、`It&apos;s`、`D&#97;ta` の `worksheetSource` は書き換わる（`R&amp;D` は残らない）。`rangeSet` と `dataRef` は `UnrewritableReference` で拒否される。`P&L`、`a<b"c`、`x\ny` に rename してから戻しても、`Back` が書かれる。旧の絞り込みに戻すと `raw_names_with_entities` が落ちる（PR 本文。ここでは Y 系で別の箇所を確かめた） |
| R2-m1 External の `#Sheet!` | 直った | Q3: `TargetMode="External"` の `#Data!A1` は rename、delete、strict のどれでも拒否される。`other.xlsx#Data!A1` は `Ok` のまま。Y5（除外を外す変異）はテストで死ぬ |
| R2-m2 名前の連鎖 | 直った | Q1 の最後（`Src2 = Src`、`Src = Data!…`）は `SheetIsPivotSource` になる。Y4（1 段で止める変異）は死ぬ。ループは集合が増えるときだけ続くので、循環しても止まる |
| R2-m3 テスト | 直った | 非 ASCII と `&` を含む名前、`c15:sqref`、`cx:f`、`cfvo/@val`、CF とスパークラインの `xm:f`、`totalsRowFormula`、`linkedCell`、`cdr:sp/@textlink` の行が入った。X1、X3、X12、X13、X22、X23 もテストで固定された |
| R2-m4 `_xHHHH_` の往復 | 直った | Q6: `x_x0041_y` に rename すると、`sheet_names()` は reopen の前も後も `x_x0041_y` を返す。戻す rename も通る。Y6（ローダーの decode を外す変異）は死ぬ |
| Nit: `@sheet` を ST_Xstring で書く | 直った（テストの穴あり） | `escape::encode` を通すようになった。ただし Y7（`encode` を外す変異）は生き残る。`_x` や制御文字を含む名前への rename を、pivot ソースで試すテストがない（n1） |
| Nit: 外部の pivot ソース、externalLink、webPublishItem | 直った | 外部 `r:id` 付きのソースは delete を止めない。`externalLink` の名前は scan で拒否されない。Y8 と Y9 は死ぬ。webPublishItem は ADR に記載された |

## main の merge（`88bd8f0`）の確認

`git show --remerge-diff` で手解決の部分を見た。

- `sml.toml`: Phase3 と Phase2〜4 の項目を合わせている。Phase4 の `definedNames`、`definedName`、`sheetPr` を削ったのは、Phase3 側の重複を除いただけである。`codegen --check` は CI で green である。
- `edit.rs`: `StripPlan` は `pub(crate)` のまま `shared_refs` を持ち、`strip_cells` は消した `t="s"` の数を返す。
- `error.rs`、`lib.rs`、`common/mod.rs`、`ApplyStep`: 両側を並べただけである。
- `structural_guard`: `check_poisoned` と `census.block` を `check_editable()` に置き換えた。`edit_refusal` の中身は同じなので、挙動は同値である。
- `workbook.rs` の `rebuild`: `registries` と `style_sensitive` を空に戻す。`style_sensitive` を残しても、structural op は style に依存する数式を増やさないので、拒否が増える側にしか転ばない。`registries` の問題は M1 で述べる。

## 統合のプローブ

| 項目 | 結果 |
|---|---|
| **sst `count`**（S1） | シートは 3 枚で、`t="s"` は 5 個（`Data` に 2 個、`A` に `Data!` を参照する `t="s"` の数式が 1 個）。delete では、`count` は実際の数 `2` になる。変わる part は、workbook、rels、Content_Types、`A`、sst の 5 つで、sst は `count` 属性だけが変わる。rename、move、add、hide では sst は変わらない。`count` が誤っている入力（99）は、delete のときだけ実数に直り、ほかの操作では残る。`count` がなければ足さない。どの操作のあとでも、`set_string` の `count` は、同じセッションでも reopen した後でも同じ値になる |
| Y1、Y2、Y3、Y10 | strip の数、削除したシートの数、`registries` の reset、引き算を、それぞれ外した。4 個とも `structural_edits_update_the_shared_string_count` で死ぬ |
| Err ⇒ byte-identical（S2） | Phase2 で `si` を足した直後に、不正な名前への rename（`InvalidSheetName`）と、最後の可視シートの delete（`LastVisibleSheet`）を呼んだ。`to_bytes` は、どちらも操作の前と同じバイト列になる |
| **F-8 ゲート**（S7） | `<row r="1"><c r="B2">` を含むブックで、7 つの操作（add、move、hide、tab color、rename、delete、strict）を試した。どれも `InvalidReference` で拒否され、パッケージは変わらない |
| Phase4（S4） | `Data` に autofilter、merge、protection を付けた。そのあと `move(1,0)` すると、`_xlnm._FilterDatabase` の `localSheetId` は 1 から 0 になる。rename で `D&ta 2` にすると、`'D&amp;ta 2'!$A$1:$C$5` になる。続く `set_autofilter` は、同じ名前を更新する（増えない）。`merged_ranges()` は move の後も正しい。add した新しいシートに merge できる。delete で、そのシートのローカル名は消え、ほかのシートの `localSheetId` は正しく残る。最後に `__index_fingerprint` が reopen と一致した |
| styles registry（S3、S5） | **M1** |
| copy | この PR に copy の操作はない（ADR-0009 の範囲外）。copy したシートの merge、DV、protection は確かめていない |

## Findings

### M1（Major）: structural op のあとに作り直した Phase2 registry が、同じセッションで足した要素を再利用せず、重複を追加する

- 根拠: `rebuild`（`workbook.rs:261`）は `registries` を空にする。次の書き込みで、registry は `Table::read`（`registry/mod.rs:62-66`）から作り直される。`Table::read` は再利用の鍵を `doc.source()[span]` から作るが、`source()` は**パースしたときの元のテキスト**である。同じセッションで挿入したノードには span がないので、鍵に入らない。`count` には数えられるので、添字は正しい。ファイルも壊れない。
  - S6: `set_string("y")`（新しい `si`）のあと、`set_tab_color` を呼び、もう一度 `set_string("y")` を呼ぶ。結果は `<si><t>y</t></si>` が 2 個になり、`uniqueCount="3"` になる。`A3` は新しい添字 2 を指す。
  - S5: 太字の書式を 2 つのセルに書くと、`cellXfs` は 2 のままである（再利用される）。reopen した後に書いても 2 のままである。ところが、`set_tab_color` を挟んでから書くと 3 になり、`font` と `xf` が 1 つずつ重複する。
  - S3: add、move、rename、delete を挟むたびに、2 から 3、3 から 4 と増える。
  - 影響: ADR-0006 の「exact-form reuse」に反する。in-session の registry の状態が reopen と一致しない（ADR-0009 の oracle 1a の考え方に反する）。structural op と書き込みを交互に繰り返すと、`cellXfs` や `fonts` が上限（`LIMITS`）に近づく。この経路は今回の merge で入ったもので、main にはない（main では registry を作り直さない）。
- 修正案: 次のどれか 1 つ。
  - `rebuild` で、変更済みの sst と styles の part を、シリアライズしたバイト列からパースし直してから registry を作り直す。
  - `Table::read` で、span のないノードをレンダリングして鍵にする。
  - styles.xml を変えない structural op では styles registry を残し、strings は `refs` だけを数え直す。

  いずれの場合も、S5 と S6 の形（書き込み → structural op → 同じ値の書き込みで、要素の数が増えないこと）をテストに足す。

### n1（nit）: `worksheetSource/@sheet` の ST_Xstring エンコードにテストがない

- 根拠: Y7（`set_sheet(&escape::encode(new))` を `set_sheet(new)` にする変異）は生き残る。`x_x0041_y` のような名前への rename を、pivot ソースで試すテストがない。
- 修正案: `raw_names_round_trip` に `x_x0041_y` の行を足す。書かれた値が `x_x005F_x0041_y` であることと、戻す rename が通ることを見る。

### n2（nit）: 他の open PR との干渉

- 根拠: #77（Phase4 part 2）は `protect_workbook`（`lockStructure`）、ハイパーリンク、DV、CF、views を追加し、`sml.toml`、`error.rs`、`lib.rs`、`range.rs` に触る。後から merge する側に、次の作業が要る。
  - `lockStructure="1"` のブックで、シートの追加、削除、rename、move、非表示を許すかどうかを決める（Excel の UI では禁止されている。この PR は `workbookProtection` を見ていない）。
  - #77 が書く DV、CF、ハイパーリンクの式は、この PR の rename では拒否されるか書き換わる。その組み合わせのテストを足す。

  #72（ignorable wrapper の拒否）は、`census.block` を通じて `structural_guard` に効くので、そのままでよい。#55 はこの PR の上に積まれている。
- 修正案: #77 か #53 のうち後から merge するほうで、`lockStructure` の方針を決めて（ADR-0009 か ADR-0014 に 1 行）、テストを 1 本足す。

## 判定

converged: no（Blocker 0 / Major 1 / Minor 0 / nit 2）

M1 を直して、S5 と S6 の形の回帰テストが入れば、収束とみてよい。
