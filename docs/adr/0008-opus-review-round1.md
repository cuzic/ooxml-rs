# ADR-0008 レビュー（Opus, round 1）

対象: PR #28 `docs/adr/0008-pptx-phase1-deck-reading.md`（Proposed）、
`spikes/pptx-phase1/`（スクリプト、`allowlist_phase1.toml`、`results/`）、
`docs/roadmap/pptx.md` の差分。前提: ADR-0001/0002（post-acceptance notes 含む）、
ADR-0005 rev 3（main）と rev 4 §9（`impl-pptx-phase0-1-copy`）。
検証プローブ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr28/probe{1..5}.py`
（著者の `chain.py` の関数を import し、同じ 633 ファイルコーパス `pptx-p1/all.txt`
に対して `python3 -I` で実行）。

## 結論（先に）

方針はよい。provenance 付き `Resolved`、idx の同点を type と系列で決める規則、
Phase 1b への分離、ノート編集を Phase 0/0.1 の規則にそのまま載せる設計、
置換で「開始ランの書式を使う」規則、スパイクの量も十分にある。
ただし、**誤った値を「解決済み」として返す箇所が 4 つ**と、
**置換で触れていない文字の書式が変わる規則が 1 つ**ある（M1–M5）。

- 生成ビューは XSD 既定値を返す。このため「最初に値を設定した要素で決まる」が
  実装できない（`cap` 既定 `none`。コーパスで 37 ラン）。
- プレースホルダーの `p:style/a:fontRef` を無視している（32 シェイプ、45 ラン。
  PowerPoint 14 の実ファイル）。
- 東アジア/CS のテーマフォントがほぼ常に `""` に解決される（581 中 548 マスター）。
  ADR の「4,021/4,025 解決」はラテン字体だけを数えた値。
- 自動調整（`fontScale`/`lnSpcReduction`）とハイパーリンク色を考慮しない
  「指定値」が、実効値と区別されていない。
- 置換文字列に `\v` を含むと、一致の後ろの既存文字からリンクが外れる。
  不変条件 F が破れるが、スパイクはこの場合を試していない。

いずれも ADR の文言と規則の修正、それに合成フィクスチャと手動チェックの追加で直る。
設計を作り直す必要はない。

## Findings

### Major

**M1. 「設定されている」の定義がない。生成ビューは XSD 既定値を返すので、連鎖が既定値で止まる。**
§1.3「the first element that sets the property decides」、§5「API sketch over the ADR-0002 views」。
- 根拠: 生成ビューは既定値のある属性を `attr_parsed_or`/`attr_enum_or` で返し、
  属性がないことを隠す（`crates/ooxml-schema/src/generated/sml.rs` l. 268–286 `CellView::style_index` など）。
  連鎖上の型にある XSD 既定値（vendored Transitional XSD）は次のとおり。
  `CT_TextCharacterProperties@cap="none"`、`dirty`、`err`、`smtClean`、`smtId`。
  `CT_TextBodyProperties@upright="false"`。`CT_Transform2D@rot/flipH/flipV`。
  `CT_Placeholder@type="obj"`、`@idx="0"`（§1.2 step 2 は「`@idx` が明示されているか」を区別する必要がある）。
  `CT_Hyperlink@history/highlightClick/endSnd`。
- 影響: `a:rPr` が `cap` を持たず、レイアウト・マスター・`txStyles` のどこかが
  `cap="all"` を持つランが 37 ある（probe1、例 `60810.pptx`）。生成アクセサを
  そのまま使うと、これらは `none`/`Direct` として返る。
- 修正: §1.3 に次の 2 点を書く。「set = 生の要素に属性（子要素グループの場合はいずれかのメンバー）がある」。
  「リゾルバーは存在を確かめるアクセサ（`Lens::attr`）を使い、`*_or` 系は使わない。
  XSD 既定値は連鎖が尽きたときのアプリ既定値の扱いで、Phase 1 では返さない（`None`）」。
  O2 に合成ケースを足す（レイアウトの `lvl1pPr/defRPr cap="all"`、スライドは `<a:rPr lang="…"/>`）。

**M2. プレースホルダーの `p:style/a:fontRef` を無視している（§1.3 表 step 4 が「—」）。**
- 根拠: probe2 では、`p:style/a:fontRef` を持つテキスト付きプレースホルダーが 32 あり、
  そのうち 45 ランは fill を run・段落既定・シェイプのリストスタイルのどこからも
  得ていない。例 `customGeo.pptx`（`Microsoft Office PowerPoint` 14.0）
  slide1 の `ctrTitle`: `<a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef>`
  と `fillRef accent1`。これはプレースホルダーに図形スタイル（クイックスタイル）を
  適用したときに PowerPoint が書くものである。塗りが accent1 で文字色が lt1（白）、
  フォントが minor になることを PowerPoint 自身が保存している。
  ADR の規則では文字色は `titleStyle` の `tx1`、字体は `+mj-lt` になり、表示と一致しない。
- 修正: プレースホルダーにも step 4 を入れる。位置はシェイプの `a:lstStyle` の後、
  レイアウトプレースホルダーの前。非プレースホルダーと同じ考え方である。
  M1 の手動チェックに含め、O2 に S1 の変種を足す。
  確定できない場合でも、黙って飛ばさず `Source::ShapeStyle` として返す
  （provenance があるので監査できる）。

**M3. 東アジア/複合文字のテーマフォントが `""` として「解決」される。**
§1.4「4 … land on an empty theme `a:ea typeface=""` and are returned as such」。
- 根拠: probe4 では、`+mn-ea`/`+mj-ea` を参照するマスターが 581 あり、
  そのうち **548** でテーマの `a:majorFont/a:minorFont/a:ea@typeface=""` だった。
  Office 既定テーマの通常の形で、代わりに `a:font script="Jpan"` などが使われる
  （Jpan 付きは 972 件）。ADR の「4,025 中 4,021 解決」は `chain.py` が
  `latin` だけを数えた結果で、`ea`/`cs` は測っていない。
  日本語のデッキでは、`RunProps::east_asian` がほぼ常に `typeface: ""` を
  正しい値のように返すことになる。
- 修正: 空の typeface は値として返さない。
  `Font::ThemeScriptDependent { slot: MinorEa, script_fonts: &[(script, typeface)] }`
  のような明示的な「未解決（スクリプト依存）」として返す。
  §1.4 に `ea`/`cs` の実測値を書き、O2 S7 の期待値を「`""`」から変える（Q4 参照）。

**M4. 「指定値」と「実効値」の区別がない（自動調整とハイパーリンク色）。**
- 根拠（自動調整）: スライドのプレースホルダーのうち 64 が
  `a:normAutofit@fontScale/@lnSpcReduction` を持つ（probe1、例 `60810.pptx`
  `fontScale="92500" lnSpcReduction="10000"`）。PowerPoint の表示サイズは
  `sz`×`fontScale`、行間は `lnSpcReduction` を引いた値である。
  §5 の `RunProps.size` は `sz` そのものを返すが、§5 は「None = application default」
  としか書いておらず、返すのが表示値ではないことを書いていない。
  また ECMA-376 §21.1.2.1.2–3 には「`noAutofit`/`normAutofit` … If this element is
  omitted, then noAutofit or auto-fit off is implied」とある。
  ADR はこの自動調整グループをレイアウト/マスターから継承する
  （PowerPoint の実際の挙動と思われる）が、仕様と食い違っていることを書いていない。
  さらに、レイアウト/マスター側に `fontScale` を持つ連鎖が 10 ある（probe1）。
  継承した `fontScale` は、そのシェイプについて計算された値ではない。
- 根拠（リンク色）: `a:hlinkClick` を持つランが 83 あり、うち 33 は自分の
  `a:solidFill` を持ち、`ahyp:hlinkClr` を持つのは 12 だけだった（probe5）。
  PowerPoint はリンク付きのテキストをテーマの `hlink` 色で描き、ランの色を使うのは
  `a:hlinkClick/a:extLst/…/ahyp:hlinkClr val="tx"` があるときだけである
  （[MS-ODRAWXML] 5.33、Microsoft サポート「Change the color of hyperlink text」、
  LibreOffice bug 137367）。§1.3 の連鎖は、このラン色をそのまま `fill` として返す。
- 修正: §5 に「`RunProps`/`ParaProps` は連鎖で指定された値（自動調整・リンク色は未適用）」
  と明記する。`BodyProps` で `autofit: Resolved<Autofit{kind, font_scale, ln_spc_reduction}>`
  を返し、`fontScale` の継承を返すかどうかを決める（推奨: 継承した `fontScale` は
  `Unresolved` にする）。`RunRef::link()` の結果に `color_rule: Hlink | Text` を足す。
  自動調整がないときの扱い（ECMA と PowerPoint の差）を M 系の手動チェックに入れる。

**M5. 置換文字列に `\v` を含み、一致が開始ランの中で終わると、残す文字からリンクが外れる。**
§4 rule 2 と rule 5、Evidence の「both invariants held every time」。
- 根拠: rule 2 では、開始ランが「prefix + replacement (+ suffix)」になる。
  rule 5 では `\v` の後の部分が「リンクを除いた複製 `a:rPr`」の新しいランに入る。
  したがって suffix（一致の後ろにある既存の文字）は最後の新しいランに移り、
  開始ランに `a:hlinkClick` があった場合はリンクを失う。これは不変条件 F
  「every kept character keeps its old `a:rPr`」に反する。
  `findreplace.py` l. 205 の置換文字列は `'X' + pat[::-1]` だけで `\v` を含まないため、
  この経路は一度も実行されていない。
  また rule 5 の複製元は「開始ランの `a:rPr`」であり、Phase 0.1 の
  「段落の最初の `a:rPr`」とは違う。その違い自体は妥当だが、明記されていない
  （開始ランの `smtId≠0` などで `FormatNotCopyable` になる経路も同様）。
- 修正: suffix は元の要素（またはリンクを含む複製ラン）に残す。
  r:id を複製しない方針（ADR-0005 rev 4「no `r:id` is duplicated」）を保つなら、
  「開始ランにリンクがあり、`\v` を含み、suffix が空でない」組み合わせを
  `LinkSplitByBreak` として拒否するのが最小の修正である。
  O5 とスパイクに `\v` を含む置換文字列を加え、F が成り立つことを示す。

### Minor

**m1. Q1 の選択肢の立て方。ECMA の注記はテキストボックスと図形を区別している。**
§19.3.1.35 の注記は「otherStyle … text within a slide shape but **not within a text box**.
Text box styling is handled from within the bodyStyle」（`ecma/part1.txt` l. 110294）。
つまり ECMA の読み方は「図形 → `otherStyle`、`txBox="1"` → `bodyStyle`」である。
ところが `chain.py` の ECMA 変種（`q6.*.poi_vs_ecma_note_bodyStyle_differ`）は、
すべての非プレースホルダーに `bodyStyle` を当てている。
§1.3 の表も「`p:sp` text box or shape」を 1 列にまとめている。
M1 のデッキはテキストボックスと図形を別々に作り、`p:cNvSpPr@txBox` を規則の入力に
するかどうかを決めるべきである。

**m2. `clrMapOvr` の根拠の挙げ方。ECMA の本文は ADR と逆のことを言っている。**
§19.3.1.7 には「If the masterClrMapping element is present, the color scheme defined
by the master is used」、§20.1.6.6 にも同じ趣旨がある。
レイアウトの上書きを継承する 10 スライドは、すべてスライド側が
`a:masterClrMapping` で、そのうち 6 はレイアウトとマスターの対応が異なる（probe3）。
ADR の規則（POI と同じ、おそらく PowerPoint の実際の挙動）を支持するのは POI と、
LibreOffice のテスト `layout-clrmap-override.pptx` である。§1.4 の典拠を
「§19.3.1.6–7」から「ECMA と食い違う、POI・LibreOffice のテスト・手動チェックで決定」に直し、
S6 と M のチェックリストに含める。

**m3. step 2（`a:pPr/a:defRPr`）が PowerPoint で効くかは確認されていない。**
python-pptx の作者自身が「PowerPoint respects … `a:defRPr` か覚えていない」と書いており
（Stack Overflow 40059179）、効くとも効かないとも言い切れない。
この step で決まる値は 23（`chain-633.txt`）。M5 に「段落 `defRPr` だけでサイズ・色を指定した段落」を加える。

**m4. §1.3/§5 で使う要素のうち、許可リストにないものがある。**
`a:sym`（`RunProps::symbol`）、`a:buSzTx`、`a:buSzPts`、`a:buClrTx`、`a:buFontTx`、
`a:gradFill`/`a:blipFill`/`a:pattFill`/`a:grpFill`（`Fill::Other` の要素名と、
グループの存在判定に使う）、`a:custGeom`（m5）がない。
許可リストに足すか、§6 の「Read raw」の一覧に載せるかを決める。

**m5. 図形の幾何のグループ判定と、グループ内プレースホルダーの扱い。**
(a) `preset_geometry` は `a:prstGeom` だけを継承するので、スライドが `a:custGeom` を
持つと、レイアウトの `rect` を返してしまう。`prstGeom|custGeom` を XSD の choice
グループとして扱うべきである（コーパスでは 0 件）。
(b) §1.2 はレイアウトのグループ内プレースホルダーも候補に含めるが、python-pptx は
最上位だけを見る。グループの子座標系の `a:xfrm` をスライド座標として継承すると誤る
（グループの内外どちらの向きでも）。コーパスでは 0 件なので、
「グループ内のプレースホルダーは幾何を継承しない（`Err(TransformedGroup)` と同様）」
と書いておくのが安全である。

**m6. `txStyles` のカテゴリをどの型で決めるかが書かれていない。スパイクとも食い違う。**
`chain.py` は `txStyles` をスライドの型で、マスタープレースホルダーをレイアウトの型で
選んでいる。`IdxTypeMismatch`（0 件）や `IdxFamily` で両者が分かれたときの規則がない。
`hdr`/`sldImg` は、スパイクでは `otherStyle`、ADR の系列表では `sldImg`→body になっている。
LibreOffice は `pic`/`tbl`/`chart` などに `txStyles` を当てない（`pptshape.cxx` l. 123–137）。
§1.2 step 3 で同じ型が複数あるとき（2 コンテンツのレイアウトなど）の「最初」は推測であり、
provenance の `MatchKind::Type` で見えることを明記する。

**m7. ノートの連鎖の書き方があいまい。**
「steps 5–7 become」の後に要素が 2 つしかない（ノートマスターのプレースホルダーの
`lstStyle`、`notesStyle`）。`defaultTextStyle` を使うかどうかも、
ノートの `dt`/`ftr`/`sldNum`/`hdr` が `notesStyle` を使うかどうかも書かれていない。
LibreOffice は body 系だけに `notesStyle` を当てる。

**m8. `Unresolved` を返す場所が API にない。**
`Unresolved::TableStyle`（§1.3）と `phClr`（§1.4）、それに M3・M4 の未解決値は、
`Option<Resolved<…>>`（`None` は「どこも設定していない」）では表せない。
`enum Lookup<T> { Set(Resolved<T>), Unset, Unresolved(Reason) }` のような 3 値にする。

**m9. MCE の選択規則。**
`Requires` に並ぶ接頭辞は、要素のスコープにある名前空間宣言で URI に解決し、
URI で比べる必要がある。接頭辞の文字列で比べてはいけない。
また「理解する名前空間 = p14, p15」で morph（p159）を Fallback（多くは fade）として返すのは、
種別を生の QName で返す設計と噛み合わない（Q5 の推奨を参照）。

**m10. オラクルの独立性。**
(a) 「no reference library resolves the text chain independently」（§7 O2、roadmap）は
ADR 自身の表と矛盾する。POI はテキストのプロパティを解決している
（`CharacterPropertyFetcher.fetchProperty`: run → `pPr/defRPr` → shape → theme →
`getDefaultMasterStyle`）。POI の既知の差（レベルのフォールバック、
暗黙 idx の扱い）を差分表にして、Java の CI ジョブ（ゲートにしないか、
差分表に固定したゲート）で sz/b/latin/fill/algn を突き合わせることを勧める。
(b) O1 は、python-pptx とマッチング規則が一致する 1,266 件で `xfrm` の参照を
確かめるだけで、規則の争点（同点・暗黙 idx）は著者の手検証表に頼っている。
(c) O2 の期待値は規則の著者が書く。ビルドスクリプトの「1 編集 = 1 レベル」の構成から
機械的に作れる部分（source の位置）と、意味の判断（どの step が優先か）を分け、
後者は M のチェックか POI/LibreOffice との差分表に結び付ける。

**m11. `lvl` の値が不正なとき（`"9"`、`"x"`）の扱いが書かれていない。**
`AttrError` を返すなら、どのレベルとして解決するか（0 か、解決を打ち切るか）を書く。

**m12. `p:cTn@spd/accel/decel` の `set_type` は Phase 1 の API で使われていない。**
§3.5 の `Effect` にこれらのフィールドはない。ADR-0002 §1.3 の
「必要になったときに証拠付きで入れる」に従えば、14 件ではなく 11 件で足りる。
害はないので、残すなら理由（既定値の採用で `DefaultMismatch` を消す）を書く。

## 推奨（Q1–Q6）

- **Q1:** POI の順序（`defaultTextStyle` → `otherStyle`）を暫定とすることには同意する。
  ただし M1 のチェックデッキを、(i) `txBox="1"` のテキストボックス、(ii) 図形、
  (iii) `p:style` 付きのプレースホルダー（M2）の 3 種類にする。
  結果が出るまでは、`Source` が `MasterTextStyle{Other}`/`PresentationDefault` であることで
  呼び出し側が判断できる、という今の形を保つ。
- **Q2:** `otherStyle`（POI）を暫定とすることに同意する。
  食い違うのは 52 値だけで、サイズはほとんどマスターの dt/ftr/sldNum の `lstStyle` で決まる。
  M1 で確定させる。
- **Q3:** `a:defPPr` と `txStyles` のレベルの穴は参照しない、という提案に同意する。
  M5 に段落 `defRPr`（m3）を加える。
- **Q4:** 一覧を出すだけでは足りない（M3）。最低限、空の typeface を値として返さず、
  スクリプト依存の未解決として、候補（`script_fonts`）付きで返す。
  `lang`/`altLang` からスクリプトへの小さな表（ja→Jpan、ko→Hang、zh-CN→Hans、
  zh-TW/HK→Hant）による補助関数は、`Resolved` ではなく別のヘルパーとして出すのが妥当である。
  実際の選択はアプリの編集言語設定にも依存するため。
- **Q5:** 別々に返す（提案）ことに同意する。MCE の選択は接頭辞を URI に解決したうえで行う。
  種別は生の QName なので、p159 も「理解する」名前空間に入れ、現行の PowerPoint の表示
  （morph）に合わせることを勧める。`transition_branches()` はそのまま残す。
- **Q6:** はい。オーナーが PowerPoint で、テキストに付けた modern comment を 1 件作り、
  実ファイルのフィクスチャにすることを勧める。合成ファイルだけでは、`ac:txMk` の
  `cp`/`len` の数え方（UTF-16 か、段落区切りを含むか）を確かめられない。
- **Phase 1b の分離:** 同意する。表の構造は Phase 0 の `CellRef::grid_span`/`row_span`
  などで既に読めている。SmartArt は独自のパーツ構成を持つ。

## 判定

**Minor のみ直せば Accepted: no.**
M1–M5 を直したら round 2 に回す（いずれも文言・規則・合成フィクスチャ・手動チェックの
追加で直り、大きな設計変更は要らない）。
