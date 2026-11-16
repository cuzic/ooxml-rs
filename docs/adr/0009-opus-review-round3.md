# ADR-0009 レビュー（Opus, round 3）

対象: PR #27 `docs/adr/0009-xlsx-phase3-sheet-operations.md`（Proposed, revision 3, `ffe612c`）。
照合したもの:

- round 2 レビュー（`0009-opus-review-round2.md`）。
- ADR-0003 **revision 5**（Accepted、`impl-xlsx-phase1` worktree `98e394f`）。§2.4、§3.1、§3.3、§4.3、§5、§7.3 を見た。
- PR #38 の実際の API（`core-add-part` `252dc8e`、`crates/ooxml-core/src/package/add.rs`）と、#38 round 2 レビュー（`docs/reviews/pr38-round2.md`）のプローブ。

#38 側のプローブは、scratchpad の `review-38r2-27r3/probe/crates/ooxml-core/tests/probe38r2.rs` で実行した。main をマージしたコピーで動かしている。リポジトリは変えていない。

## 結論（先に）

**収束した。Minor だけが残る。**

- R2-M1 と R2-M2 は、どちらも設計として解消した。
- R2-M1: `#Sheet!A1` の Target は、#38 に入った `set_relationship_target` で書き換える。この API の実物で、`#'My Sheet'!A1` や、`'`、`&`、`"`、`<` を含む fragment が書けることを確かめた。保存して開き直しても同じ値に戻り、`Id`、`Type`、`TargetMode` は変わらない（P3）。
- R2-M2: 1 操作で変わるものを、1 つの plan の staged clone に集めた（1 part につき 1 clone）。apply の後は、メモリ上の状態を作り直し、作り直しの失敗では poison する。作り直した状態と、開き直した状態を比べるオラクルも入った。
- R2-m1〜m7 と Q7 も反映された。

残りは Minor 6 件である。主に ADR-0003 rev 5 と #38 の現状に合わせる文言で、設計の変更は要らない。

#38 round 2 では Major が 2 件見つかった（N1: 編集していない clone の検出をすり抜けられる。N2: `set_relationship_target` の後の `remove_parts` が、書き換え後の Target を見ない）。ただし、ADR-0009 の手順はどちらにも当たらない（R3-m5）。

## 判定

- **converged: yes**
- **Accepted after Minor only: yes**（R3-m1〜m6 を入れて Accepted にしてよい）

---

## round 2 指摘の解消状況

| 指摘 | 状態 | 確認内容 |
|---|---|---|
| R2-M1 `#Sheet!` Target を書き換える API がない | **解消** | §2 の registry steps と §6 の表の行が、`set_relationship_target`（`Id`/`Type`/`TargetMode` は不変）を使う。#38 の実際のシグネチャ `set_relationship_target(&pkg, Option<&str>, id, &str)` と合っている。fragment だけの Internal target はそのまま書かれる（`#` で始まり、制御文字を含まないこと）。P3 では、`#'My Sheet'!A1` と `#'a&b"<c>'!A1` が、エスケープされて往復した。Q7 として記録された |
| R2-M2 transaction の外にある変更 | **解消** | §2 が全体を列挙している。workbook.xml の 1 clone には `<sheets>`、`definedNames`、`bookViews`、`calcPr` を入れる。シートの 1 clone には書き換えと strip を入れる。ほかに chart/drawing/pivot/table/connections/ctrlProps/VML、`sharedStrings.xml`（`sst/@count`）、`app.xml` を stage する。`xml_part_mut` は使わない。apply の後は census、シート一覧、依存グラフ、Phase2 の registry を作り直す。失敗したら ADR-0003 §3.1 の poison をかける。オラクル 1a として「作り直した index = 出力を開き直した index」を足した。ADR-0003 §5 の guard（`IterativeCalculation`、`MacroSheetDependent`、`RichValueCell`、workbook 全体の block）も §12、§13、Consequences に入った |
| R2-m1 VBA を同名で置き換える | **解消** | 同じ plan で `remove_parts` の後に `add_part(Exact("xl/vbaProject.bin"))` を呼ぶ。#38 の `remove_then_add_in_one_plan` で通ることを確かめた。古い `Override` は消え、新しい型で登録し直される（`content_type` の view も新しい型を返す）。content type の文言も #38 の規則に揃った |
| R2-m2 closed set の中は書き換えない | **解消** | §5 の手順 3 と scan の対象から set を除いた。孤児の判定は、fragment だけの target を無視する。#38 も、`kept_targets` と `incoming_relationships` から fragment だけの target を外した（#38 m10。test `fragment_only_targets_name_no_part`） |
| R2-m3 scan の死角 | **解消** | 解析できない XML part にはバイト列で scan する（UTF-8/UTF-16LE）。ActiveX 以外のバイナリは scan しない。生の名前の場所は表だけに頼る。この 3 点が明記された |
| R2-m4 fold | **解消** | ADR-0003 §2.4 と同じ fold（Unicode の upper の後に lower）。ただし参照先の revision は R3-m1 を見よ |
| R2-m5 sheet-prefix token の範囲 | **解消** | `Sheet2!Name` と `Sheet2!#REF!` は delete で `#REF!` になり、rename で prefix を書き換える。`Sheet2!Func(` は delete で `UnrewritableReference` になる |
| R2-m6 すでに逆順の 3-D スパン | **解消** | move が両端点の相対順序を **変える** ときだけ拒否する |
| R2-m7 `allowed_rel_types` | **解消** | §2 に操作ごとの表がある。closed set に入ってくる型だけを渡すことが明記された |
| Q7 | 記録済み | 推奨は書き換え（option 1） |

## Minor

### R3-m1 ADR-0003 の revision を 5（Accepted）に揃える

- Depends on の行（L12）と、R2-m4 の記録（L71、L537）は「ADR-0003 **revision 4** (on `impl/xlsx-phase1`)」と書いている。ADR-0003 は revision 5 で Accepted になった（`98e394f`）。
- rev 5 の R4-M1 は、strip の定義を「依存セルの **すべての** `v` と `is` を消す（スキーマ違反の重複も含む）」に改めた。ADR-0009 §2 の「Phase1's cache stripping」は、この定義を指すと明記する。
- §8 のテストの「strip と rewrite を同じ clone に入れる」不変条件に、rev 5 の O1 フィクスチャ `o1_duplicate_caches.xlsx` の形（重複した `v`、2 つの `is`）を含める。

### R3-m2 ADR-0003 §3.3 の calc mode と `KeepStaleCaches` を Phase3 でどう扱うか

- §2 は、workbook.xml の clone に「Phase1's `calcPr/@fullCalcOnLoad`」を入れるとだけ書いている。ADR-0003 §3.3 には分岐がある。
  - manual 計算モードでは `fullCalcOnLoad` を足さない。
  - `autoNoTable` は auto と同じに扱う。
  - `KeepStaleCaches` を選ぶと strip しない（Excel 専用、既定ではない）。
- Phase3 がこれをそのまま継ぐのかを 1 行で書く。継ぐのが自然である。
- 構造操作では `Everything` 全体を seed するので、`KeepStaleCaches` を選ぶとキャッシュがすべて古いまま残る。それを許すのか、Phase3 では無視するのかを決める。

### R3-m3 poison をかける時点と、fault injection を Phase3 に広げる

- ADR-0003 §3.1（R3-m3）は、apply を **始める前に** poison をかけ、成功してから外す。そのため、apply の中で panic しても poison が残る。rev 5 の R4-m6 は、panic の注入テストも足した。
- ADR-0009 §2 の「Failure after apply」は、「作り直しが失敗したら poison」としか読めない。
- 次のように書き換える。
  - `Package::apply` の前に poison をかけ、作り直しが成功してから外す。
  - §8 のテストで、ADR-0003 §7.3 の fault injection（`fault-injection` feature、`catch_unwind`）を Phase3 にも使う。注入する場所は 3 つで、apply の前、apply と作り直しの間、作り直しの途中である。
- `Package::apply` 自体は `StalePlan` 以外で失敗しない。それでも、apply の後で作り直しが失敗すると、package は変わったのに index は古い状態になる。だから poison が要る。この理由を 1 文で書いておく。

### R3-m4 改名先の名前を Target に書けない場合

- `set_relationship_target` は、fragment に制御文字（TAB、LF、CR を含む）があると `InvalidTarget` で拒否する（P3: `#a\tb` は `Err`）。
- §6 によれば、コーパスには `_x000a_` を含むシート名がある。そのような名前への rename で、図形のハイパーリンクの Target を書き換えようとすると、core が拒否する。
- 決めることは 2 つある。
  - rename 全体を `RegistryRefused` で拒否するのか、その Target だけを書き換えずに `UnrewrittenLocation` として返すのか（Q7 の案 2 と同じ扱いになる）。
  - Excel が `.rels` の Target にこの文字をどう書くかは、XL の項目にする。
- §13 のエラー一覧、または §6 の注記に 1 行で書く。

### R3-m5 #38 の現状に合わせた注意書き（ADR-0009 の手順は影響を受けない）

#38 round 2 の Major 2 件が、ADR-0009 の手順に当たらないことを確かめた。そのうえで、当たらない理由を規則として書いておく。

- **(a) stage する clone の作り方。**
  - #38 は、`stage_part` に渡された木を「現在の内容の、編集していない clone」と判定したら、元の zip エントリをそのまま書く。判定は、dirty でなく、`source()` が `read_part` と一致すること、である。
  - ところが、そのパーツが同じセッションで `set_part` や `xml_part_mut` で変わっていると、`read_part` を **パースし直した** 木がこの判定を通ってしまう。すると、検証は新しい内容で行われ、保存されるのは元の内容になる（#38 N1。P1）。
  - Phase3 が stage するのは編集した clone だけなので、dirty になり、この経路には入らない。
  - それでも、Phase1/2 は同じセッションで `xml_part_mut` を使う。「clone は `pkg.xml_part(name)?.clone()` から取り、`read_part` をパースし直さない」と、§2 に 1 行書く。
- **(b) retarget と remove を同じ plan で混ぜない。**
  - #38 の `remove_parts` は、同じ plan の `set_relationship_target` を見ない（#38 N2）。
  - ADR-0009 は、retarget を rename だけで行い、対象は fragment だけの target（part を指さない）である。delete は Target を書き換えない。だから当たらない。
  - 「1 つの plan で retarget と remove_parts を同時に使わない」ことを不変条件として書けば、#38 の修正を待たずに済む。
- **(c) VBA の置き換え。**
  - 同じ plan で `Exact` で作り直した `vbaProject.bin` の `.rels` では、`Id` が `rId1` からやり直しになる（#38 n4）。
  - 署名は作らないので害はない。記録だけでよい。

### R3-m6 §5 の calcChain の削除を、どの `remove_parts` で行うか

- §5 の手順 4 は「closed set を `remove_parts` で消し、`calcChain.xml` も消す」と書く。§2 の表では、`allowed_rel_types` は worksheet と calcChain で別々の行になっている。
- 次のどちらかに決めて書く。
  - 1 回の `remove_parts` で、set ∪ {calcChain} を `[worksheet, calcChain]` で消す。
  - 同じ plan で 2 回呼ぶ。
- 前者にすると、calcChain の型が set 全体に許されてしまう。後者を勧める。2 回目の呼び出しは、1 回目の削除を view で見る。
- Strict の twin を別に扱うことは、すでに書いてある。

## #38 の実際の API との整合

| ADR-0009 rev 3 の前提 | #38 `252dc8e` の実際 | 判定 |
|---|---|---|
| `set_relationship_target`。`Id`、`Type`、`TargetMode` は変わらない | そのとおり。Internal の fragment は verbatim に書く。part を指すものは view で解決できることを確かめる。External は URI | 整合 |
| 全変更を 1 plan に入れる。stage は 1 part につき 1 clone。後で stage した方が勝つ | `stage_part` は置き換えで、view も apply も後の内容になる（`staging_twice_keeps_the_later_content`）。`Arc` を共有するので、step ごとの複製はない | 整合 |
| `xml_part_mut` を使わなければ `StalePlan` にならない | generation を進めるのは `xml_part_mut`、`set_part`、`apply`（`remove_part` などの convenience を含む）だけで、`xml_part` は進めない（`every_change_makes_plans_stale`） | 整合 |
| `RegistryPlan::incoming_relationships` で closed set を決める。読めない `.rels` では fail closed | plan の view に対して定義され、`Result` を返す。読めない、または帰属できない `.rels` がファイル名に言及していれば `Err`。コーパスで 7 件が fail closed になった | 整合 |
| 同じ plan で `Exact` を再利用する（VBA） | 許される（`remove_then_add_in_one_plan`）。`Numbered` は再利用しない | 整合。R3-m5 (c) |
| rId: session の high-water と、ダングリングな `r:id` を採らないこと | `relationship()` が、source の view にある全属性の `rId<digits>` を used に加える（M7） | 整合 |
| 孤児判定で fragment だけの target を無視する | `ViewRel::part()` は `#…` に対して `None` を返す | 整合 |
| retarget の後の closed-set 削除 | `removal()` は retarget を見ない（#38 N2） | ADR-0009 の手順では起きない。R3-m5 (b) |

## 出典

- ADR-0009 rev 3（`ffe612c`）の §2（L257–321）、§5（L404–420）、§6（L506–557）、§10（L635–663）、§12（L721–731）、Consequences（L894–899）。
- ADR-0003 rev 5（`impl-xlsx-phase1` `98e394f`）の Revision 5（L102–114）、§3.1（L390–430）、§3.3（L494–503）、§5（L646–663）。
- PR #38 `add.rs` の `set_relationship_target`（L1169–1213）、`stage_part` の detach 判定（L1897–1901）、`removal` のループ（L1771–1776）。
- プローブ: `scratchpad/review-38r2-27r3/probe/crates/ooxml-core/tests/probe38r2.rs`（P1、P2、P3、P5）。
