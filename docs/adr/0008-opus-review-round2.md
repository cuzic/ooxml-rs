# ADR-0008 レビュー（Opus, round 2）

対象: PR #28 `docs/adr/0008-pptx-phase1-deck-reading.md` revision 2（`e114f06`）、
`spikes/pptx-phase1/`（`chain.py`、`findreplace.py`、`allowlist_phase1.toml`、`results/`）。
前提: [round 1](0008-opus-review-round1.md)。
検証プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr28-r2/`
（`xsd_defaults.py`、`probes/p1.py`〜`p3.py`、`merge.py` と `trial/`。
コーパスはスパイクと同じ 633 ファイル `pptx-p1/all.txt`。実行はすべて `python3 -I`）。
POI は `apache/poi` の **`47dcff769bb395034dddfd59c4609659b6dbc6a4`**（trunk、2026-11-13）から
11 ファイルを取得した。スパイクが使ったローカルの写しとバイト単位で一致した。

## 結論（先に）

round 1 の Major 5 件（M1–M5）はすべて解消された。Minor 12 件も、文言・規則・測定のいずれかで
対応されている。再実行の結果は次のとおり。

- find/replace スパイク: コミット済みの `findreplace-633.txt` と同じ結果になった
  （`\v` を含む置換 111,064 件を適用、158 件を `LinkSplitByBreak` で拒否、T と F は全件で成立）。
- 許可リストの試行 codegen: 124 行の `Read` が増え、診断は 57 のまま。
  `pml`/`dml`/両方のビルド、`clippy -D warnings`、`ooxml-pptx` のテストがすべて通る。
- M1 の属性一覧: 連鎖上の型について、vendored XSD の既定値付き属性と完全に一致する。

新たに見つかった問題は Minor 10 件で、どれも文言・スパイク・テスト計画の修正で直る。
誤った値を `Set` として返す経路は、コーパス上では見つからなかった。
ただし r2-m1（グループの要素が持つ属性を、M1 の存在判定で連鎖の上まで探してしまう点）は、
規則として誤りなので直す必要がある。

## 解消状況（round 1）

| ID | 状況 | 確認内容 |
|---|---|---|
| M1 | 解消 | §1.3 に「set = 生の要素に属性がある」と書かれた。`*_or` を使わないこと、XSD 既定値を返さないこと（`Unset` と `xsd_default()`）も明記された。一覧は `xsd_defaults.py` で XSD と照合し、`CT_TextCharacterProperties`（cap/dirty/err/smtClean/smtId）、`CT_TextBodyProperties@upright`、`CT_(Group)Transform2D`、`CT_TextNormalAutofit`、`CT_TextAutonumberBullet@startAt`、`CT_TextFont`、`CT_Placeholder`、`CT_Hyperlink` と一致した。`CT_TextParagraphProperties` には既定値付きの属性がない。グループ要素の属性の扱いだけ r2-m1 で直す |
| M2 | 解消 | プレースホルダーにも step 4 を適用。対象は自分の `p:style` だけで、スクリプトごとの typeface と色の子要素から決める。S11 と M1 も追加済み |
| M3 | 解消 | 空の typeface は `Unresolved(ScriptDependent{slot, script_fonts})` になる。548/581 の実測、latin の未解決 4 件（`bug64693.pptx`）は `chain-633.txt` と一致した。`cs` は測っていないが、規則は同じなので問題ない |
| M4 | 解消 | §1.6 に「値は指定値」と明記された。autofit は独立した `Lookup` になり、継承した scale は `Unresolved`。`Link::color_rule()` も追加された。`ahyp:hlinkClr` のスキーマ（`val` = `hlink`/`tx`）は [MS-ODRAWXML 5.33](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-odrawxml/014fbc20-3705-4812-b8cd-93f5af05b504) と [SDK の `HyperlinkColorEnum`](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.office2019.drawing.hyperlinkcolor.hyperlinkcolorenum?view=openxml-3.0.1) で確認した。`tx` の意味は仕様の本文にないので、M7 の手動チェックで決める今の形が正しい。「10 chains」は r2-m5 を参照 |
| M5 | 解消（スパイクに漏れ 1 件） | §4 rule 5 が変更され、`LinkSplitByBreak` で拒否するようになった。再実行で 111,064 適用 / 158 拒否 / T・F 全件成立と、コミット済みの結果に一致した。ただしスパイクは `a:hlinkMouseOver` を見ていない（r2-m3） |
| m1 | 解消 | 測定を `txBox` で分けた（1,139 / 2,862。sz 417 / 564）。M1 のデッキは 3 種類 |
| m2 | 解消 | ECMA と食い違うことを明記し、根拠を POI と LO のテストに置いた。M6 も追加 |
| m3 | 解消 | 未検証であることを明記し、M5 に追加 |
| m4 | ほぼ解消 | 6 件を追加。`p:cNvSpPr` が漏れている（r2-m4） |
| m5 | 解消 | `prstGeom｜custGeom` を choice として扱う。グループ境界をまたぐ幾何は `Unresolved(GroupCoordinates)`。コーパスではグループ内のプレースホルダーは 0 件（p1） |
| m6 | 解消（POI の引用に誤り） | `txStyles` はスライドの型、マスターのプレースホルダーはレイアウトの型で選ぶ。ただし、これを「POI と同じ」とする記述が誤っている（r2-m2） |
| m7 | 解消 | ノートの連鎖が明記された。色・テーマ・幾何は書かれていない（r2-m6） |
| m8 | 解消 | `Lookup = Set｜Unset｜Unresolved(Reason)` |
| m9 | 解消 | `Requires` を URI で比べる。コーパスでは、接頭辞が `mc:Choice` 自身で宣言されているもの 236、祖先で宣言されているもの 146 があり、スコープでの解決が必要なことを裏づける。p159 の URI `http://schemas.microsoft.com/office/powerpoint/2015/09/main` もコーパスで確認した |
| m10 | 解消（差分表に漏れ） | POI を任意のオラクルとして追加し、期待値を機械的なものと意味的なものに分けた。POI との差分表に漏れがある（r2-m2b） |
| m11 | 解消（細部があいまい） | `Unresolved(InvalidLevel)` を返す。step 4 の扱いがあいまい（r2-m9） |
| m12 | 解消 | 上書きは 11 件で、`p:cTn` は外した。試行 codegen でも `DefaultMismatch` は 39 件で、既存の行のまま |
| 0.1 % 訂正 | 正しい | 39/59,716 = 0.065 %、35/46,093 = 0.076 %。どちらも 0.1 % になる |

## Findings

### Major

なし。

### Minor

**r2-m1. グループに属する要素の属性を、M1 の存在判定で連鎖の上まで探してはいけない。**
§1.3 の M1 の一覧には、`a:CT_TextAutonumberBullet@startAt`、`a:CT_TextFont@pitchFamily/charset`、
`a:CT_TextNormalAutofit@fontScale/lnSpcReduction`、`a:CT_Transform2D@rot/flipH/flipV` が入っている。
しかしこれらは、要素グループ（箇条書きの種類、`a:latin`、autofit、`a:xfrm` 単位）で
**選ばれた要素自身**の属性である。たとえばスライドの `<a:buAutoNum type="arabicPeriod"/>` の
`startAt` は 1 であり、レイアウトの `buAutoNum@startAt="3"` を継いではいけない。
§1.5 は `xfrm` を「one unit」と書いているので、一覧の記述と食い違っている。
p2 プローブの結果は次のとおり。
- スライドが `<a:normAutofit/>` を持ち、`fontScale`（2 件）や `lnSpcReduction`（4 件）を持たないのに、
  上位の要素が持っている例がある（`bullet-indent.pptx`、`slide-section-test.pptx`）。
  属性ごとに連鎖をたどると、本来の 100 %/0 % ではなく `Unresolved(InheritedAutofitScale)` になる。
- `startAt` と `rot`/`flip` が漏れる例は 0 件。

修正: §1.3 に次を書く。「グループのメンバー要素の属性は、グループを決めた要素からだけ読む。
その要素に属性がなければ、その要素の XSD 既定値を `Set`（source はその要素、`defaulted: true` などで区別）として返し、連鎖はたどらない」。
M1 の一覧は「独立に継承される属性」と「グループ要素の属性」に分ける。
S10 または S12 に「スライドは `<a:normAutofit/>`、レイアウトは `fontScale="90000"`」を足す。

**r2-m2. POI についての記述に 3 つの誤り（`47dcff76` で確認）。**
(a) §1.2「Layout → master … as python-pptx's and POI's mapping; the slide's own type is not used here」は、
POI については誤り。`XSLFShape.fetchShapeProperty`（l. 341–362）は、マスターを
`getPlaceholderType(ph)` で探す。この `ph` は**スライド**のプレースホルダーである。
レイアウトの型を使うのは python-pptx だけ。規則自体は変えなくてよい
（家族が分かれる `IdxTypeMismatch` は 0 件）が、根拠を直し、O2b の差分表にも入れる。
(b) POI は、プレースホルダーには `p:defaultTextStyle` を使わない。
`ParagraphPropertyFetcher.getThemeProps` は `shape.isPlaceholder()` のとき `null` を返す。
したがって step 8（プレースホルダーの `PresentationDefault`）は ECMA §19.2.1.8 だけが根拠になる。
`chain-633.txt` では、この step が決める値は `marL` 327、`b` 5、`sz` 4、`fill` 4、`latin` 3、`algn` 3 である。
O2b の「known differences」にこの項目がないので、O2b は表の外の差分で必ず失敗する。表に足すこと。
(c) Sources の「Apache POI XSLF (trunk, 2026-11-16)」はコミットを固定していない。
`47dcff769bb3` に固定する（スパイクが読んだ写しと一致することは確認済み）。

**r2-m3. スパイクの `LinkSplitByBreak` 判定が `a:hlinkClick` しか見ていない。**
§4 rule 5 は `a:hlinkClick or a:hlinkMouseOver` を拒否の条件にしている。
しかし `findreplace.py` の `link()` は `a:hlinkClick` だけを見る。
p3 で合成のランを作った結果は次のとおり。
- `hlinkMouseOver` だけを持つ開始ランで、一致がラン内で終わり、置換文字列に `\v` を含む場合:
  拒否されずに適用され、**F が失敗する**（suffix `def` がリンクのないランに移る）。
- `hlinkClick` の場合は正しく拒否される。

コーパスで `hlinkMouseOver` を持つランは 1 件（`keyframes.pptx`）なので、
「both invariants held」はこの経路を試していない。ADR の規則は正しいので、スパイクの判定を直し、
O5 に `hlinkMouseOver` だけを持つランのケースを入れる。

**r2-m4. 許可リストの漏れと件数の表記。**
`ShapeRef::is_text_box` は `p:cNvSpPr@txBox` を読む。しかし `a:CT_NonVisualDrawingShapeProps/p:cNvSpPr` は、
既存の `pml.toml` にも追加分にもなく、§6 の「Read raw」の一覧にもない。どちらかに入れる。
また §6 の表の見出しは「`pml.toml` (48) / `dml.toml` (76)」だが、
`allowlist_phase1.toml` と `allowlist_p1.txt` の実数は 47 / 77 である（合計 124 は正しい）。
`a:masterClrMapping` は dml の列に入っているので、pml の数え間違いと思われる。

**r2-m5. 古いままの数値。**
§1.4「5,567 scheme colours … 4,870 through `bg/tx`」は revision 1 の値である。
revision 2 の `chain-633.txt` では `color.resolved 5616`、`mapped(bg/tx) 4848`、`direct 768`。
§1.6「reached from a layout or master (10 chains)」も、p2 では、上位から `fontScale` を得る連鎖は
4 件（すべて `tdf128550.pptx`、マスターから）しか見つからない。
定義（何を数えたか。r2-m1 の 2/4 件を含めるか）を書いて数え直すか、スパイクの結果を引用する。

**r2-m6. ノートと、レイアウト/マスター上のシェイプの文脈が書かれていない。**
§1.4 の色の対応表（slide → layout → master）、テーマ、§1.5 の幾何（slide → layout ph → master ph）は、
ノートでは次のように読み替える必要がある。
- 色: ノートスライドの `p:clrMapOvr` → ノートマスターの `p:clrMap`
- テーマ: ノートマスターのテーマ
- 幾何: ノートスライド → ノートマスターのプレースホルダー

これを明記する。また §3.1 は「layout/master shapes」を同じ API で読むとしている。
`LayoutRef`/`MasterRef` のシェイプにも `props()` を出すなら、その連鎖を定義する。
定義例: レイアウトのプレースホルダー = steps 1–3 → マスターのプレースホルダー → `txStyles`。
POI はマスター上では steps 2–5 を飛ばす。出さないなら「出さない」と書く。

**r2-m7. スライドの `themeOverride` に触れていない。**
1 ファイル（`60810.pptx` slide18 → `ppt/theme/themeOverride1.xml`、`a:clrScheme` のみ）がある。
この例では内容がマスターのテーマと同じなので、値は変わらない。
§1.1/§1.4 に「Phase 1 は `themeOverride` を適用しない（または、ある場合は色を `Unresolved(ThemeOverride)` にする）」と書いておく。

**r2-m8. 明示 idx がレイアウトにない孤立プレースホルダーを手動チェックに入れる。**
p1 では、スライドのプレースホルダーのうち 49 件が、レイアウトにない明示 `@idx` を持つ。
ほとんどが `4294967295` で、内訳は title 23、obj 10、body 7、sldNum 5、ctrTitle/ftr/pic 各 1。
§1.2 はこれらを step 3（Type/Family）か Orphan で解決する。
これは意味の判断（m10c）だが、M2 は「idx-0 tie と implicit-idx」しか見ていない。
M2 に「`idx="4294967295"` のタイトルで、レイアウトにタイトルがある場合とない場合」を足す。

**r2-m9. 不正な `lvl` のとき step 4 を見るかどうかがあいまい。**
§1.3 は「steps 1–2 still apply and every level-dependent step yields `Unresolved(InvalidLevel)`」としている。
しかし step 3 は level に依存し、step 4（`fontRef`）は依存しない。
「最初にレベル依存の step に達した時点で `Unresolved` を返して止まる」と書く。
step 4 で値が決まると、より優先される step 3 の値を確かめないまま返すことになるからである。

**r2-m10. ハイパーリンク色の規則の適用範囲。**
`color_rule()` が対象にするのは `a:hlinkClick` だけ（`hlinkMouseOver` は色を変えない）であることを明記する。
また、リンク付きランのうち URL 以外の action を持つものがある（`hlinksldjump` 18、`macro` 11、
`hlinkshowjump` 1、`hlinkfile` 1。p1）。これらも `hlink` 色で描かれるかどうかを M7 に足す。

## 推奨（Q1–Q6）

- **Q1:** 暫定案（POI の順序）に同意する。`47dcff76` での POI の順序は次のとおりで、ADR の記述と一致する。
  `CharacterPropertyFetcher.fetchProperty`: run → `pPr/defRPr` → `fetchShapeProperty`
  （シェイプの `lstStyle`。プレースホルダーはさらにレイアウト → マスター）→
  `defaultTextStyle`（**非プレースホルダーのみ**）→ `getDefaultMasterStyle`
  （非プレースホルダーと dt/ftr/sldNum は `otherStyle`、レベルは下位にフォールバック。ノートはすべての型で `notesStyle`）。
  ただし r2-m2 の (a)(b) を直す。
- **Q2:** 同意する（POI の `getDefaultMasterStyle` で確認した）。
- **Q3:** 同意する。
- **Q4:** 同意する。
- **Q5:** 同意する。URI で比べること、p159 を理解する名前空間に入れることは、コーパスでも妥当と確認した。
- **Q6:** 同意する（PowerPoint で作った実ファイルを使う）。
- 試行 codegen は本レビューでも再現した（`merge.py` で 47 + 77 件を統合）。
  `report.txt` の差分は `Read` 124 行だけ。Diagnostics は 57（`DefaultMismatch` 39）のまま。
  ビルド、clippy、`ooxml-pptx` のテストもすべて通る。

## 判定

**Minor のみ直せば Accepted: yes.**
r2-m1 と r2-m3 は、実装に入る前に ADR とスパイクで直すこと。
それ以外は、文言・数値・テスト計画の修正で足りる。
