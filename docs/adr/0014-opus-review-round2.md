# ADR-0014 レビュー（Opus, round 2）

- 対象: PR #35 `docs/adr/0014-xlsx-phase4-range-features.md` revision 2（86b5ef6）
  - あわせて読んだもの: ADR-0001 の post-acceptance note、`spikes/xlsx-phase4/range_survey.py`、README
- 照合先:
  - ADR-0009 rev 2（`plan/xlsx-phase3` 94f8783）と、その round 2 レビュー（未コミット `0009-opus-review-round2.md`: R2-M1, R2-M2）
  - PR #38 `core/add-part`（31906a9。`package.rs` 352-391、`add.rs` 471-499）
  - `origin/main`（848b057）の ADR-0003、ADR-0006 と、`crates/ooxml-xlsx`（`worksheet.rs` 354、`tests/refusals.rs` `r6_merged`、`tests/edit.rs` 15、`README.md` 59）
  - `impl/xlsx-phase1` の `edit.rs` 103-105
- コーパスの再現: PR の `range_survey.py` を `python3 -I` で実行した。対象は、POI `test-data/spreadsheet` を `47dcff769bb395034dddfd59c4609659b6dbc6a4` で取り直したもの（round 1 と同じ `review-pr35/dl/poi`、368 エントリ）。出力は `scratchpad/review-31r3-35r2/35/survey.out`

## 結論（先に）

**round 1 の B1、B2、M1〜M5、m1〜m11 はすべて反映された。** コーパス表は、ピン留めしたコミットで**完全に再現した**。

- 対象は 367 ファイルで、読める 342 パッケージの値が表と全行一致した（`.xltx` 1 件はスクリプトの対象外）。
- §12 の数式サイト表は、ADR-0009 rev 2 §6 の表（L433-439）と §5（bare `#REF!`、`hyperlink/@location` の例外、ローカル名の削除）に包含されている。
- §12 のハイパーリンクの 4 段の順序は、PR #38 の世代管理と整合する。ADR-0011 rev 3 も同じ順序を採っている。

マージ範囲のアンカーへの書き込みを許す緩和（M1）は、**安全である**。

- 非アンカーへの書き込みは引き続き拒否されるので、隠れ値は作られない。
- 配列、テーブル見出し、`vm` のガードは、マージのガードより後で評価される。アンカーがそれらに当たる場合は、従来どおり拒否される。

ただし、適用範囲（値の書き込みか、スタイルだけの書き込みか）と、既存テストの扱いを書いておく必要がある。

残りは Minor だけである。

- **R2-1:** M1 の緩和の範囲、セッション内での `merges` の更新、書き換えるテストを明記する。
- **R2-2:** §2 の表で、セルに書き込む呼び出しに Phase1 の差分（従属セルの `v`、`calcPr`）が入っていない。`opts.text` が inline string なのか sst なのかも食い違っている。
- **R2-3:** 「表から生成する」と書いているが、表を機械可読にする形が決まっていない。
- **R2-4:** `remove_autofilter` が `_FilterDatabase` を消すことが、fact 5（Excel は名前を残す）と食い違う。
- **R2-5:** ADR-0009 は round 2 で R2-M2 が未解決である。依存先を「rev 2」に固定するのをやめ、Accept の条件を確認し直す。

## 解消状況

| ID | 指摘（round 1） | 状況 | 根拠（rev 2） |
|---|---|---|---|
| B1 | `_FilterDatabase` の重複 | **解消**（細部は R2-4） | §7 L425-446。シートローカルのものが 1 個なら書き換え、2 個以上なら `DuplicateBuiltInName`、ブック範囲のものは触らない。既存の `autoFilter` は、`filterColumn` も `sortState` も無いときだけ `@ref` を書き換え、それ以外は `AutoFilterExists`。オラクルに 3 ファイルを追加 |
| B2 | Phase3 依存の不健全 | **解消**（ADR-0009 側の残りは R2-5） | (a) §12 の表に `cfvo/@val` を含む全サイトがあり、ADR-0009 rev 2 L433-439 に対応する。closed-world scan による拒否も明記。(b) PR #38 を使った 4 段の順序（L596-611）。(c) bare `#REF!` |
| M1 | merge 後のアンカーに書けない | **解消**（明記の不足は R2-1） | §3 L284-295、ADR-0001 note L82-90。Alternatives 8 に却下理由がある |
| M2 | CF の他シート参照 | 解消 | §6。`formula`、cellIs のオペランド、`cfvo` を `CrossSheetReference` で拒否。DV の Q2 と同時に解除する |
| M3 | `remove_autofilter` の半端な状態 | 解消（名前の扱いは R2-4） | §7 L448-458。`FilterActive` で拒否し、`filterMode` を消し、シート直下の `sortState` は残す |
| M4 | legacy `password` が残る | 解消 | §11。ハッシュ系の属性をすべて置き換える。`legacy_hash` を選ぶと SHA 系を消す |
| M5 | allowed-diff の列挙が不完全 | **ほぼ解消**（R2-2, R2-3） | §2 の表。round 1 で挙げた 10 項目は全部入った |
| m1 | URL の分類と escape | 解消 | §4 |
| m2 | ハイパーリンク削除の理由 | 解消 | §4、Alternatives 5 |
| m3 | DV の細部 | 解消 | §5（`""`、`ListItemHasComma`、範囲参照も 255 文字、x14 の count と ext） |
| m4 | CF の細部 | 解消 | §6（`stop_if_true`、新ブロック、`RemovedWithBrokenX14Link`、pivot） |
| m5 | ADR-0006 の参照 | 解消 | §6（§5 item 8 / item 3、AC 拒否の通過） |
| m6 | `cols` | 解消 | §9 |
| m7 | ペイン | 解消 | §8 |
| m8 | ページ設定 | 解消 | §10 |
| m9 | 保護の符号化 | 解消 | §11、オラクル 4（`pwd`） |
| m10 | merge の細部 | 解消 | §3 |
| m11 | コーパス数値 | **解消・再現済み** | 下記 |

**m11 の再現結果**

- `# 342 readable packages of 367`。`r2:dtd_part` は 4。
- 次の値が、ADR の表と README のとおりに一致した。

  | 項目 | 結果 |
  |---|---|
  | merge | 43 / 54,468 / 76 シート、非アンカー値 1 / 12 |
  | CF | 20 / 108 |
  | `cf:dxf` | 89 |
  | `stopIfTrue` | 4 / 16 |
  | `cfvo_formula` | 1 |
  | `cf_formula_ref_error` | 3 / 5 |
  | `cf_formula_other_sheet` | 0 |
  | x14 DV | 3 / 7 |
  | x14 DV のソース | 他シート 5、`#REF!` 2 |
  | autofilter | 3 / 3 |
  | `_FilterDatabase` | 6 / 8 |
  | ローカルで autoFilter の無いもの | 3 / 4 |
  | シート直下の `sortState` | 5 / 7 |
  | `col` | 14,556 / range 3,478 / hidden 798 / outline 24 |
  | 行属性 | 107 / 11 / 26 |
  | ページ設定 | 329 / 170 / 53 / 73 |
  | シート保護 | flags 10 / legacy 2 / SHA 2 |
  | ブック保護 | 23 / 1 / 2 |
  | `ws_alternate_content` | 2 |

- 補足: ディレクトリには `.xltx` が 1 件ある（368 エントリ）が、スクリプトは `.xlsx`/`.xlsm` だけを数える。README に「367 = `.xlsx` + `.xlsm`」と 1 語足すとよい。
- 補足: 旧カウンタ `x14:dv_other_sheet 3/7` が、まだ出力に残っている。r1 の誤り（全部他シート）の元なので、削除するか、改名する。

## 新規指摘（すべて Minor）

### R2-1. M1 の緩和: 範囲、セッション内の状態、書き換えるテストを明記する（§3 L284-295）

安全性は確認した。

- `edit_cell` のガードの順序（`impl/xlsx-phase1` `edit.rs` 103-122）は、merge → テーブル見出し → 配列アンカー → 配列結果である。アンカーが配列の範囲やテーブル見出しに当たる場合は、後段のガードがそのまま拒否する。
- Phase1 の数式をアンカーに書くと、通常の値の書き込みと同じく従属セルが無効化されるだけである。
- `merge` 自体は、配列範囲や `vm` との重なりを拒否する。

そのうえで、次の 3 点が書かれていない。

1. **スタイルだけの書き込みを含むか。**
   - ADR-0006 §5.3（main L716-）の「style-only writes」も、Phase0/1 のガードを通る（L707）。そのため現状では、マージ範囲のどのセルにも書式を設定できない。
   - rev 2 の文言は「a write to a non-anchor cell stays refused」なので、マージ範囲の外枠の罫線（Excel では端のセルそれぞれの書式）を設定できないままになる。
   - 推奨: 非アンカーの拒否は、**値と数式の書き込みだけ**に掛ける。スタイルだけの書き込みは、マージ範囲のどのセルにも許す。隠れ値は生じない。選ばない場合でも、どちらにしたかを ADR に書く。
2. **セッション内での `merges`。** ガードは `SheetContext.merges`（open 時の走査）を見ている。次の 2 点を ADR に書き、テストも加える。
   - `merge()` / `unmerge()` は、同じ apply の中でこの一覧を更新する。
   - `merge` の直後に非アンカーへ書くと `MergedCell` になり、`unmerge` の後なら通る。
3. **既存テストと文書の書き換え。** main 上で、緩和により期待が変わるものを列挙しておく。そうしておけば、実装 PR の差分を「意図した変更」としてレビューできる。
   - `tests/refusals.rs` `r6_merged`: `B1` はアンカーなので、Ok に変わる。
   - `tests/edit.rs` L15 の総当たりの期待値 `Expect::Merged`: アンカーを除く。
   - `crates/ooxml-xlsx/README.md` L59 の「top-left cell included」。
   - ADR-0003 は L400 の行だけでなく、L31（「refusal of every cell of a merged range」）も書き換える。

### R2-2. §2 の表: セルに書き込む呼び出しの差分（L236, L238）

1. `merge_clearing` と、`set_hyperlink` に `opts.text` を付けた場合は、どちらもセルに書き込む。そのため、ADR-0003 の Phase1 が許す差分も一緒に起こる。
   - 起こる差分:
     - **従属する数式セル（他シートを含む）の `v` / `t` の除去**
     - `workbook.xml` の `calcPr/@fullCalcOnLoad`（ADR-0003 L278。`calcPr` の作成を含む）
     - `calcChain.xml`
   - 表には `calcChain` しか無い。このままでは、正しい実装がオラクルで落ちる。
   - 推奨: この 2 行に「plus ADR-0003 §7 の、そのセル書き込みに対する allowed diff」と書き、合成すると明記する。
   - `merge_clearing` の「cleared `v`/`is`/`f`」には、`t` 属性の除去も含める。
2. `opts.text` について、§4 L319-320 は「inline string through Phase0」としているのに、§2 は「`sst`/style registries (Phase2)」としている。
   - inline string なら `sharedStrings.xml` は変わらず、style は「Hyperlink」スタイルを opt-in したときだけ変わる。どちらかに揃える。
3. （細部）次の 2 つの差分が、表で明示されていない。いずれも許すなら、表に書く。
   - `protect_sheet` は、既存の要素で「既定値と同じ属性を省略する」（§11）。その結果、既存の属性が削除されうる。
   - `remove_autofilter` で `sheetPr` が空になった場合、その要素は残る。

### R2-3. 「オラクルは表から生成する」の形を決める（L223）

表のセルは散文（例: 「the `col` elements of the range (edited, split, new)」）なので、このままでは生成できない。推奨は次のとおり。

- **表をデータとして持つ。** たとえば `crates/ooxml-xlsx/tests/allowed_diff.rs` に、呼び出しごとの `&[Allowed]` の const 表を置く。各要素は `{ part, element path, kind: Create | Remove | EditAttrs(&[..]) | Children }` とし、合成する場合は `Compose(Phase1CellWrite)` を使う。
- **ADR の表とデータ表の一致を検査する。** ADR の表とデータ表の呼び出し名の集合が一致することを、テストで確かめる。生成する向きは、データ → ADR の表でも、その逆でもよい。
- **判定方法を書く。** オラクルは、Layer 2 の span を使って「許された要素の span の外のバイトは同一」と判定する。§2 の注意書き「a call whose changes would fall outside its row is a bug」を、機械的に検査できる形にする。
- **変異テストを置く。** 各行について、表に無い兄弟要素を 1 つだけ触る変異体がオラクルで落ちることを確かめる。

### R2-4. `remove_autofilter` と `_FilterDatabase`（§7 L453, §2 L246）

- fact 5 のとおり、Excel は autofilter を外しても名前を**残す**。B1 の再利用規則も、名前が残っていることを前提にしている。
- 名前を消すと、次の 2 つの問題が出る。
  - allowed diff が `workbook.xml` まで広がる。
  - ADR-0009 §9 の名前削除規則（使われている名前の削除は `NameInUse`）との関係が決まらない。
- 推奨: **名前は残す**（Excel と同じ）。消すことにするなら、ADR-0009 §9 の使用数の検査を通し、使われていれば `NameInUse` にする。
- 同じ理由で、`set_autofilter` で名前のテキストを書き換えるのは、ADR-0009 §9 の「名前の編集」にあたり、従属する数式が無効化の対象になる。実際にはまず起きないが、R2-2 と同じく表に書いておく。

### R2-5. ADR-0009 への依存を「rev 2」に固定しない（L15-19, L571-588）

- ADR-0009 は round 2 で、まだ収束していない（R2-M1、R2-M2）。
  - **R2-M1**（drawing の `.rels` の `#Sheet!A1`）は、Phase4 のサイトには関係しない。
  - **R2-M2**（1 操作の変更全体と `StalePlan`）では、ADR-0009 rev 3 が「Phase1 の無効化も plan に含める（`stage_part` 方式）」へ移る可能性がある。
- ADR-0014 の 4 段の順序（plan には関係だけを入れ、in-place の編集は apply の後に行い、失敗したら poison する）は、それ自体で整合している。PR #38 の `xml_part_mut` だけが generation を進める、という仕様とも矛盾しない。
- ただし、ADR-0009 が別の方式を採った場合、Phase3 と Phase4 で原子性の扱いが分かれる。
- 推奨: 次の 2 点を書く。
  - 依存先を「ADR-0009 rev 2 **以降の Accepted 版**」とし、§12 の包含条件は、ADR-0009 が Accepted になる時点でもう一度確かめる。
  - ADR-0009 が R2-M2 を `stage_part` で解決した場合は、ハイパーリンクの手順も同じ方式に揃える。揃えない場合は、その理由を 1 行書く。
- あわせて、ブランチを `origin/main` に追従させる。main には #25、#30、#40〜#42 が入っている。

## 判定

**Accepted after Minor only: yes.**

R2-1〜R2-5 は、どれも明記・表の補完・テスト設計で済み、設計は変わらない。ただし、Accept には次の条件がある。

- ADR-0009 が Accepted になる時点で、§12 の包含条件を確かめ直す（R2-5）。
- 実装 PR が、R2-1 の 3 で挙げたテストの変更を明示する。
