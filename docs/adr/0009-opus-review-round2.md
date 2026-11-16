# ADR-0009 レビュー（Opus, round 2）

対象: PR #27 `docs/adr/0009-xlsx-phase3-sheet-operations.md`（Proposed, revision 2, `94f8783`）。
照合先: round 1 レビュー（`0009-opus-review-round1.md`）、PR #38 の実際の API
（`core-add-part` worktree `31906a9`, `crates/ooxml-core/src/package/add.rs`）、ADR-0003 revision 4
（`impl-xlsx-phase1` worktree, §2.4, §3.1, §5）、Phase1 実装（`impl-xlsx-phase1`,
`crates/ooxml-xlsx/src/edit.rs` `apply`, `formula/lexer.rs`）。

独立プローブ（すべて `scratchpad/review-38-27r2/adr0009/`）:

- `cw_scan.py` / `cw_scan2.py`: §6 の closed-world scan を ADR の文言どおりに実装した。既知の場所の表と
  既知の非参照（`t="s"`/`str`/`inlineStr` の値、sharedStrings、comments/threadedComments、`.rels` の
  `#…` Target）を除き、旧名の sheet-prefix 形が残れば拒否とする。これを POI `test-data/spreadsheet`
  （366 ファイル、解析可能 343 パッケージ）の **全シート × rename** の 838 通りで実行した（`python3 -I`）。
- `raw_probe.py`: `!` を伴わない生のシート名の出現と、XML として解析できないパーツを調べる。
- `wt/crates/ooxml-core/tests/adr0009_probe.rs`: PR #38 のコピー上で、ADR §2/§5/§10 の手順を
  `RegistryPlan` で実際に組み立てる（P1–P5）。

## 結論（先に）

round 1 の **B1–B3、M1–M8、m1–m12 はすべて反映された**。PR #38 の `stage_part` → `remove_parts`
の順で、シート削除のカスケードが 1 回の plan で通ることも、実 API で確かめた（P3b）。closed-world scan の
**誤拒否は POI コーパスで 0/838**（343 パッケージ）だった。設計の方向はこれで固まった。

ただし、PR #38 の実 API と突き合わせると、新たに Major が 2 つある。

1. **R2-M1:** rename の表にある「drawing `.rels` の `#Sheet!A1` Target の書き換え」を表現する API が
   PR #38 に無い。`.rels` は `stage_part` できず（`RegistryPart`）、`add_relationship` は `#` を含む
   Target を拒否する（`TargetNotFound`）。影響は 838 通り中 18 通り（2.1%）、3 パッケージで、どれも Excel
   が書いた図形のハイパーリンク。
2. **R2-M2:** 「1 操作 = 1 transaction、`Err` なら何も変わらない」と言いながら、Phase1 の無効化
   （`v`/`t` の除去、`calcPr`）、Phase2 の `sst/@count`、census の再索引が transaction に入っていない。
   plan の後に `xml_part_mut` を 1 回でも呼ぶと、apply は `StalePlan` になる（P4）。

## 判定

- **converged: no**
- **Accepted after Minor only: no.** R2-M1 と R2-M2 を反映した後、round 3 で収束する見込み（Minor のみ）。

---

## round 1 指摘の反映確認

| 指摘 | 状態 | 確認内容 |
|---|---|---|
| B1 カスケード不能 | 解消 | §5 は `remove_parts` による closed set で、各パーツの `.rels` も一緒に消える。P3b: `stage_part(workbook.xml)` → `remove_parts([sheet2, drawing1, chart1], [worksheet])` が通り、`workbook.xml.rels` の `rId2` と Override 3 個が消える。staging しないと `StillReferenced`（P3c）で、ADR §2 の説明と一致する |
| B2 一括検証 | 解消 | §2 は PR #38 の仮想ビュー（staged content を走査）に基づく。`RegistryPlan::step` はコピー上で検証し、成功したときだけ反映するので、`Err` なら plan は変わらない |
| B3 rename の場所 | 解消 | §6 は場所と構文の表、closed-world scan、ActiveX での拒否を持つ。誤拒否の実測は下記 |
| M1 `#REF!` | 解消 | 裸の `#REF!`。Phase1 lexer で lex できることを不変条件とし、テストもする |
| M2 構文差 | 解消 | formula / raw / `[file]Sheet!Name`、3-D はまとめて引用、`[n]` は書き換えない、ST_Xstring、保守的な引用 |
| M3 Phase1 健全性 | 解消（合成は R2-M2） | 全操作で `Everything` 全体を seed する。未知シート名が解決される場合も含む。3-D の端点が逆転する move は拒否 |
| M4 テーブル | 解消 | `TableInUse` |
| M5 sheetId 系 | 解消 | `activeSheetId` は採番の high-water に入れ、削除時は拒否する。`revisions` は `SharedWorkbook`。跨セッションの限界も明記された |
| M6 孤児 `.rels` | 解消 | PR #38 `check_free` が `rels_part_name(name)` の存在で `NameInUse` を返す（コードで確認） |
| M7 オラクル | 解消 | 4a 残留言及オラクル、合成フィクスチャ |
| M8 VBA | 解消（手順は R2-m1） | 署名を関係型 3 種でたどる。`.xlam` は拒否。`ExtensionMismatch` はセッション中の変更時だけ |
| m1–m12 | 解消 | m1 は XL 9(g)、m2 は PR #38（`4b03e93`）、m3 は §9、m4 は `linkTarget`、m5 は Consequences（≤16/342）、m6 はマクロシートの削除を拒否、m7 と m10 は「Not rewritten」、m8 は §5、m9 は Staging cost、m11 は `tabSelected`、m12 は spike README L17–20 |

## closed-world scan の測定

`cw_scan2.py` は §6 の仕様どおりに実装した。全 XML パーツの全テキストと全属性を対象に、XML アンエスケープと
`_xHHHH_` のデコードを行い、ASCII case-fold で照合する。sheet-prefix 形は `'…'!`、`Name!`、3-D の端点。

| 項目 | 値 |
|---|---|
| 対象 | 343 パッケージ、全シートの rename 838 通り |
| 既知の場所・既知の非参照の外でヒットし、拒否されるもの | **0 通り（0 パッケージ）** |
| 既知の場所のうち `.rels` の `#Sheet!…` Target に当たったもの | **18 通り / 3 パッケージ**（`ConditionalFormattingSamples.xlsx` 16 シート、うち `Home` は 17 箇所。`47504.xlsx` `Sheet1`。`bug63189.xlsx` `Sheet3`）。すべて `xl/drawings/_rels/drawingN.xml.rels` の図形ハイパーリンクで、TargetMode は Internal（→ R2-M1） |
| 参考: `.rels` を表から外した場合 | 同じ 18 通りが拒否になる（初回の分類ミスで観測した） |

ADR の表と非参照リストで、**実ファイルに誤拒否は出ない**。一方で scan は sheet-prefix 形しか見ないので、
`!` を伴わない **生の名前** の場所には安全網が無い（`raw_probe.py`）。コーパスで見つかった生の名前の出現は
次のとおりで、未知の場所は無かった。

- 既知の `worksheetSource/@sheet`（5）。
- 他ブックの `externalLink sheetName/@val`（5）。
- `codeName` や、`1` のような数字名と数値属性の偶然一致（ノイズ）。

これは設計上の限界として書いておくべきだ（R2-m3）。

---

## Major

### R2-M1. `.rels` の `#Sheet!A1` Target を書き換える API が PR #38 に無い

**根拠:**

- §6 の表の行「`hyperlink/@location`; `#Sheet!…` targets in sheet and drawing `.rels` | location」は、
  rename でこれを書き換えると言っている。
- コーパスの実例: `ConditionalFormattingSamples.xlsx` の `xl/drawings/_rels/drawing1.xml.rels`
  `<Relationship Id="rId8" Type="…/hyperlink" Target="#'FY months'!A1"/>`（TargetMode なし = Internal）。
  ほかに `drawing2…18.xml.rels` の `Target="#Home!A1"`、`47504.xlsx` と `bug63189.xlsx` にも 1 件ずつある。
- PR #38 の API で試すと、どの手段も使えない（`adr0009_probe.rs` P2）:
  - `plan.stage_part(&p, "xl/drawings/_rels/drawing1.xml.rels", …)` は
    `Err(RegistryRefused { reason: RegistryPart })`。
  - `plan.add_relationship(&p, Some("xl/drawings/drawing1.xml"), REL_HYPERLINK, Target::Part("#Renamed!A1"))`
    は `Err(… TargetNotFound { resolved: "#Renamed!A1" })`。`add.rs` L791 は Target に `#` が含まれると
    拒否する。
  - `Target::External` で足すと `TargetMode="External"` になり、意味が変わる。
  - 「消して新 Id で足し、drawing 本文の `r:id` を付け替える」も、足す段で同じ理由により拒否される。
- §2 は「registry rules are those of PR #38 … not restated here」と言っており、ADR 単体では実装経路が無い。

**影響:** rename 838 通り中 18 通り（2.1%）、3/343 パッケージ。すべて Excel が書いた目次シートのボタン形の
リンク。

**推奨（どれかを ADR と PR #38 で決める）:**

1. **推奨:** PR #38 に `RegistryPlan::set_relationship_target(pkg, source, id, Target)` を足す。
   既存関係の Target だけを書き換え、Id・Type・TargetMode は変えない。Internal の「同一パーツ内
   フラグメント（`#…`）」を `Target::Fragment(String)` として明示的に許す。Layer 3 の例外リスト
   （モジュール docs の「The exceptions」）に 1 行足す。
2. 代替: rename の表からこの行を外し、delete と同じく「書き換えず、残留オラクルの許可例外」とする。
   リンク切れを黙って残すことになるので、`RewrittenLocation` と並べて `UnrewrittenLocation` を返す。
3. 代替: 該当 rename を `Err(UnrewritableReference)` で拒否する。誤拒否率は 2.1% になる。

どれを選ぶかは ADR の Q（Q2/Q6 と同列）として owner に上げる。オラクル 4a の許可例外の書き方も、
選んだ案に揃える。

### R2-M2. Phase1/Phase2 の無効化と census の更新が transaction の外にある

**根拠:**

- §2 は「stages the edited `workbook.xml`, the `app.xml` update, and every part whose formula text is
  rewritten」と列挙するだけだ。§12 は「All operations run as one PR #38 transaction. An `Err` leaves the
  package unchanged」と言う。
- ところが同じ操作には、さらに次の変更が伴う（§4–§7、§12 の表）。
  - Phase1 の無効化: 依存セルの `v`/`t` の除去（多数のシートにわたる）と、`workbook.xml` の
    `calcPr/@fullCalcOnLoad`。
  - Phase2: `sharedStrings.xml` の `sst/@count`（delete）。
  - census（`FormulaCensus`）とシート一覧の更新。
- Phase1 の実装（`impl-xlsx-phase1` `edit.rs` `apply`）は、これらを `self.pkg.xml_part_mut(p)` で
  **その場で** 書き、strip の位置を `(sheet index, row index, cell index)` で持つ。census の各セルは
  `c.sheet`（`<sheets>` 内の位置）をキーにしている。
- PR #38 の plan は、`Package::generation` が変わると `StalePlan` になる。**どのパーツ** でも
  `xml_part_mut` を呼べば変わる。P4 では、`stage_part(workbook.xml)` の後に無関係な `sheet1.xml` を
  `xml_part_mut` しただけで、`apply` が `Err(StalePlan)` になった。
- そのため、実装は次のどちらかになるが、ADR はどちらとも決めていない。
  - (a) Phase1/Phase2 の変更も staged clone に合流させる。`workbook.xml` は、シート削除と `calcPr` を
    **同じ 1 つの clone** にする必要がある。`stage_part` は置き換えなので、2 回呼ぶと先の変更が消える。
    書き換え対象と strip 対象が重なるシートも同じ。
  - (b) registry の `apply` の後で、Phase1 の strip を **適用後の状態** に対して行う。この場合は、strip が
    失敗しないことを plan 段で示す必要がある。削除されたシートの strip は除く。位置は書き換え後の木でも
    変わらない（式テキストだけを変えるので）ことを不変条件にする。
- census の再索引もどこにも書かれていない。
  - delete/add/move で `c.sheet` と `localSheetId` の対応がずれる。
  - rename では census の `text` が古くなる。
  - 3-D スパンの所属と、未知シートの `Everything` 判定が変わる。
- 古い census のまま次の値編集を plan すると、ADR-0003 の健全性（precise な precedent が正しいこと）が
  崩れる。

**推奨:**

- §2 に「1 操作の変更の全体」を列挙する。
  - staged parts: `workbook.xml`（`<sheets>`、`definedNames`、`bookViews`、`calcPr` を 1 clone に）、
    書き換えと strip を合わせたシート clone、`sharedStrings.xml`、`app.xml`。
  - registry steps。
  - その後の、メモリ上だけの更新: census の再構築または再索引、`sheets` 一覧、registries。
- 推奨は (a) と、apply 後の census の **再構築**。再走査のコストは Phase1 の open 時と同じなので、構造操作
  なら許容できる。再索引を選ぶなら、オラクルに「操作後の census = 出力を開き直した census」を加える。
- ADR-0003 §5 の guard を Phase3 にも適用するかを明記する。`Everything` を全部 seed するので、
  `IterativeCalculation`（依存が空でなければ拒否）や `MacroSheetDependent` が構造操作を拒否しうる。
  コーパスの該当は `iterate="1"` が 2 パッケージ（`47889`, `57535`）、マクロシート・ダイアログシートが
  2 パッケージ、`vm` セルが 0。拒否で構わないが、エラー一覧（§13）と Consequences の件数に入れる。

---

## Minor

- **R2-m1 VBA の置き換えは「消して同名で足す」では通らない。** §10 は「Replacing an existing project
  removes the old one in the same plan, as a closed set」とし、続けて `xl/vbaProject.bin` を足す。
  P1 では、同じ plan で `remove_parts(["xl/vbaProject.bin"])` の後に
  `add_part(Exact("xl/vbaProject.bin"))` を呼ぶと `NameInUse { by: "a part removed in this plan" }` に
  なった。通る手順は次のとおり（P5 で確認）。
  - 既存の `vbaProject.bin` を `stage_part(bytes)` で置き換え、署名パーツだけを
    `remove_parts(sigs, [Signature, SignatureAgile, SignatureV3])` で消す。バイナリのソースでも、関係型が
    allowed にあれば通る。空になった `vbaProject.bin.rels` は残る。
  - または `Numbered("xl/vbaProject{}.bin")`（P5c で `vbaProject1.bin`）。

  ADR は前者を書く。あわせて「adds … with an **`Override`**」を PR #38 の規則に合わせる。正しくは「`bin`
  Default が厳密に `vbaProject` 型なら Override は付かない。それ以外は Override」で、意味は同じだが文言を
  揃える。
- **R2-m2 削除する closed set の中のパーツは書き換えない。** 削除シートの drawing の chart が自シートを
  参照するのは普通のケースだ。§5 の手順 2（参照を書き換えたパーツを stage）が、手順 3 で消すパーツまで
  stage すると、`remove_parts` が `ConflictsWithPlan` を返す（P3）。書き換え対象と closed-world scan の
  対象から closed set を除く、と明記する。また P3b の `kept_targets` には、`#Sheet1!A1` ハイパーリンクから
  偽の target `"xl/drawings"` が入る（PR #38 側の問題。#38 のレビューで扱う）。§5 の孤児判定は、
  パーツでない target を無視すること。
- **R2-m3 scan の対象外を規定する。**
  1. XML として解析できない XML 系パーツ: Excel が書く旧 VML は `<br>` を閉じないことがある
     （`BrNotClosed.xlsx` `vmlDrawing1.vml`、`x:FmlaMacro` 入り）。このようなパーツには、バイト列の
     正規表現 scan を代わりに行い、ヒットすれば拒否する。書き換えもできないので、同じく拒否する。
  2. `oleObject*.bin` など、ActiveX 以外のバイナリは scan しないことを docs に書く。
  3. 生の名前の場所（`!` なし）は表だけが頼りで、scan の安全網が効かないことを書く。コーパスに未知の
     場所は無かった（上記の測定）。
- **R2-m4 fold を ADR-0003 rev 4 §2.4 に揃える。** scan は「ASCII case-folded」と書かれているが、
  ADR-0003 rev 4 のシート名照合は広い Unicode fold（upper のあと lower）を使う。Phase1 が
  `'данные'!A1` を `Данные` に解決して書き換える一方で、未知の場所の同じ形を scan が見逃す。広い fold
  なら拒否が増えるだけなので、同じ fold を使う。なお §2.4（folding、未知シート → `Everything`）は
  `impl/xlsx-phase1`（PR #20）の revision 4 にしかなく、この worktree と main の ADR-0003 には存在しない。
  Depends on の行で revision を明記する。
- **R2-m5 「sheet-prefix token」の範囲を明記する。** Phase1 lexer は `Sheet2!LocalName` を
  `TokKind::Name { prefix }`、`Sheet2!#REF!` を `Area::RefError`、`Sheet1!Func(` を関数として返す
  （`lexer.rs` L87, L446–466）。rename では、これらの prefix も書き換える。delete では、`Sheet2!LocalName`
  と `Sheet2!#REF!` を裸の `#REF!` にする。この 2 点を §5/§6 に書き、テストする。
- **R2-m6 既に逆順の 3-D スパン。** ADR-0003 §2.4 は `Sheet3:Sheet1!A1` を正順と同じに扱うので、入力に
  逆順のスパンがありうる。§7 の「makes any 3-D reference's first endpoint come after its last」では、
  そのファイルの move がすべて拒否になる。「move が 2 端点の相対順序を **変える** とき拒否」と定義する。
- **R2-m7 `remove_parts` の `allowed_rel_types` を書く。** §5 の表は closed set に入る **外向き** の型の
  表で、PR #38 の `allowed_rel_types`（set の外から入ってくる関係の型）とは別物だ。delete では
  `worksheet`（chart sheet なら `chartsheet`）、calcChain では `calcChain`、VBA では `vbaProject` と
  署名の 3 型を渡す、と明記する。誤って広く渡すと、`StillReferenced` の安全網が効かない型が増える。

---

## PR #38 API との整合（まとめ）

| ADR-0009 の前提 | PR #38 の実際 | 判定 |
|---|---|---|
| `stage_part` → `remove_parts` が staged 内容で判定される | そのとおり（P3b/P3c） | 整合 |
| `Numbered` は max+1、除去済みの名前も避ける | `numbered()` は除去済みの名前（セッション内・plan 内）を含めて max を取る（P3d: `sheet3.xml`） | 整合 |
| 孤児 `.rels` があれば名前は使用中 | `check_free` | 整合 |
| rId の session high-water | `4b03e93` | 整合 |
| `apply` は `StalePlan` 以外で失敗しない | `generation` は `xml_part_mut`、remove、apply で増える。plan の後の Layer 2 編集はすべて stale になる | 整合。ただし Phase1 の合成は R2-M2 |
| `.rels` の `#…` Target の書き換え | API なし | **不整合（R2-M1）** |
| VBA を同名で置き換える | 同じ plan で同名を再追加できない | **不整合（R2-m1）** |
| closed set 内のパーツを stage | `ConflictsWithPlan` | 手順の明記が必要（R2-m2） |
| `incoming_relationships` で closed set を決める | `Package` の現状態に対してだけ定義される（plan の仮想ビュー版は無い） | 削除の plan では追加より前に計算するので足りる。順序を明記する |

## 出典

- PR #38 `crates/ooxml-core/src/package/add.rs`:
  - `RegistryPlan::step`/`check`（L495–513）
  - `add_part`/`check_free`/`numbered`（L666–750）
  - `add_relationship` の `#` 拒否（L791）
  - `remove_parts`/`removal`（L1265–1505、staged との衝突）
  - `stage_part`（L1506 付近）
- Phase1: `impl-xlsx-phase1/crates/ooxml-xlsx/src/edit.rs` `apply`（L282–360）、`formula/lexer.rs`。
- ADR-0003 rev 4（`impl-xlsx-phase1/docs/adr/0003-…md`）§2.4 L334–372、§5 L629–646。
- プローブ: `scratchpad/review-38-27r2/adr0009/cw_scan.py`、`cw_scan2.py`、`raw_probe.py`、
  `wt/crates/ooxml-core/tests/adr0009_probe.rs`（P1–P5 の出力は本文に引用）。
- POI test-data（`scratchpad/p2/poi`、366 ファイル）: `ConditionalFormattingSamples.xlsx`、`47504.xlsx`、
  `bug63189.xlsx`、`BrNotClosed.xlsx`、`47889.xlsx`、`57535.xlsx`。
