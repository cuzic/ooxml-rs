# ADR-0002 レビュー（Opus, round 1）

対象: `docs/adr/0002-schema-driven-code-generation.md`（Status: Proposed draft, 2026-11-16, branch `docs/adr-0002-schema-codegen`）
方針: 読み取り専用の敵対的レビュー。ADR-0001（Accepted, revision 3）との整合、Open XML SDK のスキーマデータ（`dotnet/Open-XML-SDK@431ab05c`。ローカルの zip のコメントでコミット SHA が一致することを確認済み）、`ooxmlsdk` 0.15.0 のソースと round-trip ハーネスで、ADR の主張を裏付けを取って検証した。数値の確認は `python3 -I` で JSON を直接集計した。重大度は **[Blocker]**（generator/lens のコードを書く前に ADR の修正が必要）、**[Major]**（設計の一部が誤っているか、Phase0 の範囲の記述に影響する）、**[Minor]** の3段階。判断の分かれる指摘には「判断」と明記した。

## 結論（先に）

方向性は妥当。具体的には、生ツリーの上に定数表と薄いアクセサを置く構成、生成物のコミットと `--check`、`build.rs` と serde の不採用、`CONTENT` で子を QName 参照にして feature や allowlist から順序を独立させる点、enum の `Unknown` の扱い、の5点。ADR が出している数値（要素宣言数、particle の種別ごとの件数、2,122 / 754、`worksheet` が 38 要素の flat sequence であること、Phase0 の型がすべて flat sequence か leaf であること、parts が 128 件）もデータで再現できた。

ただし、このままでは実装できない、または Phase0 で誤った出力になる点が3つある。
1. 入力データには属性の既定値が1件も無い。ADR §3 は既定値を表に持ち、const で検証すると書いているが、入力（§1）からそれを作れない（B1）。
2. §3 の生成コードの形（`ooxml-xlsx` から `impl Lens<'a, Cell> { … }` を書く）は、Rust の inherent impl の規則（E0116）に反するのでコンパイルできない（B2）。
3. 定義されている挿入プリミティブは「スキーマ順位で位置を決める `insert`」だけで、Phase0 が実際に必要とする書き込み（同じ順位の兄弟の中で行番号・列番号の順に入れる、`maxOccurs=1` の重複を防ぐ、子の削除）を表せない。ADR は、ADR-0001 T2 が求める「行・列の順序」を「スキーマ順序」と混同している（B3）。

このほか、未掲載の兄弟（とくに `mc:AlternateContent`）に対する挿入規則に反例がある（M1）。マーカーの単位とベースクラス継承が未定義で、`CT_Cell` の属性と `xml:space` が表から落ちる（M2）。`ooxmlsdk` を退ける根拠に挙げた例が事実と合わない（M3）。Phase0 の allowlist では「どれが sheet1 か」を決められない（M4）。overrides の表現力が未定義のまま、Phase0 の時点で必要になる（M5）。

---

## B1 [Blocker] 入力の SDK データには属性の既定値が無いのに、§3 は既定値を前提にしている

### 根拠
- ADR L94: `const ATTRS: &'static [AttrSpec];   // name, simple type, default`。L117-118 の例: `attr!("s", U32, default = "0")`, `attr!("t", Enum(&CELL_VALUES), default = "n")`。L127 `attr_or_default("s")`。L150-151 では「every default parses as its simple type」を const で検証するとしている。
- SDK データ（`data/schemas/*.json`）の全属性 8,510 件を集計した結果、属性オブジェクトが持つキーは `QName`, `Type`, `PropertyComments`, `PropertyName`, `Validators`, `Version` だけで、既定値を表すキーは無い。`Validators` の引数名（`MinLength`, `MaxInclusive`, `Pattern`, `IsToken` …）にも既定値は無い。`typed/` と `schematrons.json` にも無い。
- 一方、ECMA-376 Transitional XSD（`ooxmlsdk-main/schemas/OfficeOpenXML-XMLSchema-Transitional/`）には `default=` が `sml.xsd` だけで 775 件、全体で 1,236 件ある。`CT_Cell` については `s` default="0"、`t` default="n"、`cm`/`vm` default="0"、`ph` default="false"（sml.xsd L2282-2286）。

### なぜ問題か
- Phase0 の読み取りは既定値に依存する。`<c r="B2"><v>90</v></c>`（fixture の B2）は `t` が無いので数値（`n`）として読む。`s` が無ければスタイル 0 になる。§1 で選んだ入力からは、この情報を生成できない。
- そのため L149-153 の「every default parses」は検証する対象そのものが無い。
- 手書きで補うと、生成の範囲が広がるたびに数百件の既定値を overrides に写すことになる。L274-277 が想定する「間違いの修正」としての overrides とは規模も性質も違う。
- Alternative 3（L241-246）は「XSD を読むには本物の XSD プロセッサが要る」という理由で退けている。しかし既定値だけなら `complexType/@name` → `attribute/@name` → `@default` の単純な参照で取れ、group や extension の解決はほぼ要らない（例外は `attributeGroup` 経由の属性くらい）。さらに `ooxmlsdk` は Transitional XSD を同梱しており、XSD から `data/schemas` 相当の JSON を再生成する生成器と、その差分オラクル（`crates/ooxmlsdk-build/src/sdk_data/xsd_schema_gen.rs`, `oracle.rs`）まで持っている。「XSD は重い」という前提は、少なくとも既定値については成り立たない。

### 提案
- §1 を「構造は SDK JSON、属性の既定値は ECMA-376 Transitional XSD から取る（2系統の入力）」に改めるか、「既定値は生成しない。必要な既定値は overrides に、出典つきで手で書く」と明記するか、どちらかを決める。前者なら XSD も vendoring と pin の対象にし、ライセンスと出典を `PROVENANCE.md` に加える。拡張名前空間（x14 など）の XSD は ECMA に無いので、その既定値は MS-XLSX などの仕様を出典にした overrides で扱う、と範囲を書く。
- 上が決まるまでは、L117-118 の例と L150-151 の const 検証を「既定値が手に入る場合のみ」に弱める。

---

## B2 [Blocker] §3 の生成コードの形はコンパイルできない（他クレートの型に inherent impl を書いている）

### 根拠
- `Lens` は `ooxml-core` で定義されている（L89-103「Hand-written (`ooxml-core`)」）。
- 生成物の出力先は `crates/ooxml-<format>/src/generated/<namespace>.rs`（L70-71）で、そこに `impl<'a> Lens<'a, Cell> { pub fn style_index(&self) … }` を置く設計になっている（L126-128）。

### なぜ問題か
- Rust の inherent impl は、型を定義したクレートの中にしか書けない（E0116 "cannot define inherent `impl` for a type outside of the crate where the type is defined"）。型引数 `Cell` がローカルでも例外にならない。§3 の中心にある「1行アクセサ」は、書かれた形のままでは一行もコンパイルできない。
- `impl ElementSpec for Cell` と `impl ChildOf<Row> for Cell` は問題ない。どちらも Self がローカル型なので orphan rule を満たす。問題は accessor だけ。
- 同じ問題は、名前空間を複数のフォーマットで共有する場合（DrawingML の `a:`、`mc:`、`r:` など）にも出る。§2 は出力先をフォーマットごとのクレートとしているが、§5 には `dml` feature があり、共有名前空間をどこに生成し、そのアクセサをどこに置くかが書かれていない。

### 提案
次のいずれかを選んで ADR に書く（どれが良いかは判断。API の使い勝手と、どのクレートに何を置くかに直結する）。
1. 要素ごとの拡張トレイトとして生成する（`pub trait CellLensExt { fn style_index(&self) … } impl CellLensExt for Lens<'_, Cell>`）。利用者が import しなくて済むように、フォーマットのクレートに `prelude` を用意する。
2. 生成コードをすべて `ooxml-core` の中に feature ごとに置く。フォーマットのクレートは domain API だけを持つ。共有名前空間の置き場所の問題もこれで同時に解ける。
3. `Lens` をフォーマットのクレートごとに定義する。たとえば core が `macro_rules!` で `Lens`/`LensMut` の定義を出し、各クレートがそれを展開する。

どれを選ぶにしても、L109-129 のサンプルを、実際にコンパイルが通る形に直す。

---

## B3 [Blocker] 挿入プリミティブが Phase0 の書き込みを表せない（スキーマ順序と、行・列の順序の混同）

### 根拠
- ADR L20-23 は「ADR-0001's T2 locality oracle already requires 'the insertion point respects ordering'」としている。しかし ADR-0001 L263-273 の T2 が求めているのは「the insertion point respects **row/column** ordering」で、スキーマ順序のことではない。
- 定義されている操作は `LensMut::insert<C: ChildOf<E>>(&mut self) -> Result<LensMut<'_, C>, Error>`（L104-106）だけで、位置はスキーマ順位から決まる。規則（L167-170）は「新しい子は、スキーマ位置が自分より大きい最初の既存兄弟の直前に入る。無ければ最後の既存兄弟の後に入る」。
- SDK データでは `sheetData` = `row*`（`Occurs:[{}]`、つまり上限なし）、`row` = `c*`, `extLst?`（L183-186 とデータで一致）。

### なぜ問題か
反例を3つ挙げる。

1. **同じ順位の中の順序。** fixture の `row r="2"` に対して `cell_mut(2, 1)`、つまり A2 より前の列への書き込み（たとえば A2 を一度消してから書き戻す、あるいは from-scratch で `write_number(0, 3)` の後に `write_number(0, 1)` を呼ぶ）を考える。規則では、新しい `c` は「位置が大きい兄弟」（`extLst`）が無いので、最後の `c` の後に入る。結果は `<c r="D1"/><c r="B1"/>` になり、Excel の修復ダイアログが出る。`row` についても同じことが起きる（`write_number(5, …)` の後に `write_number(2, …)` を呼ぶと、row 3 が row 6 の後に入る）。ADR-0001 L447-449 の from-scratch API は書き込み順を制限していないので、これは普通の使い方で起きる。`insert` には同じ順位の中の位置を指定する手段が無い。
2. **`maxOccurs=1` の重複。** `c` に既に `<v>` があるときに `insert::<CellValue>()` を呼ぶと、規則上は2つ目の `<v>` を `is`/`extLst` の前に入れる。Err にするのか、既存の要素を返すのかが書かれていない。
3. **削除が無い。** Phase0 の成功条件（ADR-0001 L486-498）は、全セルに対して `set_string`/`set_number` を呼ぶことを求めている。B2 の数値セルに `set_string` を呼ぶと、`<v>` を消して `<is><t>…</t></is>` を入れることになる。A1 の `t="s"` を `set_number` で上書きするときは、`t` 属性を外す（C2 の `t="b"` と同じ）。最後の子を失った要素は `own_dirty` にする（ADR-0001 L214-217）。ADR-0002 の `LensMut` には、子の削除、属性の設定や削除、それに伴う dirty フラグの義務のどれも書かれていない。

`insert` 自身にも dirty の義務がある。self-closing の親（`<sheetData/>` や `<c r="D2" s="1"/>`）が最初の子を得たら、親を `own_dirty` にし、ルートまで `subtree_dirty` を立てる（ADR-0001 L214-228）。この義務を持つのは生成コードではなく `ooxml-core` の汎用プリミティブなので、ADR-0002 の中で規定すべき。また ADR-0001 L285-290 は「値を書く呼び出しが来るまでノードを作らない」と決めている。`insert` がその時点ですぐ作るなら、`CellMut` が `insert` を呼ぶのは値を書く時だけ、という関係を明記する必要がある。

### 提案
- §4 の挿入を2段に分けて定義する。(a) スキーマが「どの帯（スロット）に入るか」を決める。(b) 帯の中での位置は呼び出し側が決める。例: `insert_by<C>(&mut self, key: impl Fn(&Lens<C>) -> Ordering)`、または `insert_before(&sibling)` / `insert_after(&sibling)` と、それが帯の境界をはみ出さないことの検証。`Worksheet` の lens は `r` の行番号・列番号で比較関数を渡す。
- `maxOccurs=1` の子が既にあるときの挙動（`Err(AlreadyPresent)`、または `get_or_insert`）、`remove`、`set_attr`/`remove_attr`、そしてそれぞれの dirty の義務（ADR-0001 L209-228 の規則に従うこと）を §3/§4 に書く。
- 改行やインデントなど空白だけの text 兄弟に対して、新しい要素をどちら側に入れるかを決める。T2 の差分が「挿入点に閉じる」ことの定義に関わる。
- 判断（副次的な提案）: (a) の「スロット」を「QName の集合とスキーマ順位」として一般化すると、`Choice` の要素群（繰り返しを含む）や、`maxOccurs≤1` の `Group`/入れ子の `Sequence` も同じ仕組みで扱える。ヒューリスティックな集計（Choice や Group の葉が要素か Any だけで、多要素の Sequence が繰り返されないもの）では、754 件の非 flat 型のうち約 693 件がこの形に入る。L270-273 で「後続 ADR に先送りする」としている範囲のかなりの部分が、追加の設計なしで扱える可能性がある。この数字は検証済みではなく、目安。

---

## M1 [Major] 未掲載の兄弟の扱いに反例がある（`mc:AlternateContent`、および SDK が MCE 専用の子を直接の子として並べていること）

### 根拠
- L170-175: 未掲載の兄弟（ベンダー拡張、`mc:AlternateContent`、コメント、PI）は位置の判定に使わない。新しい子との相対位置は「周囲の既知の兄弟で決まり、T2 がそれを検査する」。L200-202 は MCE を対象外とし、未掲載の兄弟として扱うとしている。
- SDK の `x:CT_Worksheet/x:worksheet` は 38 項目の flat sequence で、`drawing`(28), `legacyDrawing`(29), `legacyDrawingHF`(30), `drawingHF`(31), `picture`(32), `oleObjects`(33), `controls`(34), …, `tableParts`(36), `extLst`(37) の順。
- `ooxmlsdk` の overrides（`sdk_data/schema_extensions/schemas_openxmlformats_org_spreadsheetml_2006_main.json`）は、Worksheet について `"AlternateContent": [{"AfterField": "drawing", "Children": ["legacy_drawing"]}, {"BeforeField": "controls", "Children": ["controls"]}]` を追加している。これは、実ファイルでは `legacyDrawing` や `controls` が `mc:AlternateContent/mc:Choice` に包まれて現れるという実例。Workbook についても `AlternateContent` の後ろに `absolute_path` を置き、さらに SDK データに無い `xr:revisionPtr` を子として追加している。
- SDK データの `x:CT_Workbook/x:workbook` は、`x15ac:CT_AbsolutePath/x15ac:absPath` を位置 3 の**直接の子**として並べている（`InitialVersion: Office2013`）。

### なぜ問題か
- **反例:** 既存の子が `[pageMargins(20), drawing(28), legacyDrawing(29), mc:AlternateContent{controls(34)}, extLst(37)]` のシートに `picture`(32) を挿入する。規則上は「位置 > 32 の最初の既知兄弟」= `extLst` の直前に入るので、`…, AC{controls}, picture, extLst` になる。MCE の前処理後の順序は `controls(34), picture(32)` となり、スキーマ違反になる。既知の兄弟がまったく無い区間（AC が末尾）でも、「最後の兄弟の後」に入るので同じ結果になる。
- AC を「位置なし」とみなすと、AC の中身が新しい子より前の位置か後ろの位置かを区別できない。これは原理的な欠陥で、規則の文言を直すだけでは解決しない。
- 「T2 がそれを検査する」は成り立たない。ADR-0001 L540-542 のとおり fixture は `mc:Ignorable` を含まないし、`worksheet` 直下に未掲載の兄弟も無い。T2 はこのケースに一度も触れない。
- SDK が `absPath` を直接の子として並べていることには副作用がある。そのデータで生成した `CONTENT` は、実ファイル（AC に包まれている）と食い違う。直接の子としての読み取り（`workbook.abs_path()`）は実ファイルでは見つからない。将来 `insert::<AbsPath>()` を許すと、Excel が書かない形になる。

### 提案
- 位置を決める規則を拡張する: `mc:AlternateContent` の位置は、各 `mc:Choice`/`mc:Fallback` の最初の既知の子のスキーマ位置とする（枝の間で食い違う場合は最小値をとり、診断を出す）。それでも位置を決められない未知の兄弟は、直前の既知兄弟に付いているものとして扱う（今の規則と同じ）。この挙動を明文化する。
- SDK が「拡張名前空間の子を直接の子として並べている」項目（`InitialVersion` が付いた非 ECMA 名前空間の項目）について、実ファイルでは AC に包まれるのかを overrides で表せるようにする（M5）。
- MCE を「Phase0 の対象外」とするのは構わない。ただし、挿入規則の記述は Phase0 に限定されない一般規則として書かれているので、「AC が兄弟にいる親への挿入は、AC を解釈するまで `Err` を返す」という安全側の暫定措置でもよい。

---

## M2 [Major] マーカーの単位が「complex type」ではなく SDK の `(型, 要素)` の組であることと、ベースクラス継承の解決が書かれていない

### 根拠
- L137-139: 「Markers are keyed by *complex type in context* … matching how the SDK data itself is keyed (`"x:CT_Cell/x:c"`)」。L109 は「Generated (per complex type actually in the allowlist)」、L190-191 は「explicit allowlist of complex types」。
- SDK のキーは `(complex type)/(element QName)` の組。1つの complex type が複数の要素に使われている例は 286 件ある。たとえば `x:CT_Xstring` は `t`, `v`, `author`, `formula`, `oddHeader` など 15 要素に使われている。`x:CT_Cell` は `x:c`（Cell）と `x:oc`（OldCell）、`x:CT_LegacyDrawing` は `legacyDrawing` と `legacyDrawingHF`、`x:CT_PageBreak` は `rowBreaks` と `colBreaks`。逆に、同じ要素 QName が複数の型に対応する例も 191 件ある（`x:v` には `CT_Xstring/x:v` と `ST_Xstring/x:v` の2つがある）。
- 属性はベースクラス側に置かれている。
  - `x:CT_Cell/x:c` には `Attributes` が無く（`BaseClass: "CellType"`, `IsDerived: true`）、`r`/`s`/`t`/`cm`/`vm`/`ph` は抽象エントリ `x:CT_Cell/`（ClassName `CellType`, `IsAbstract`）にある。
  - `x:CT_Xstring/x:t` と `x:CT_Xstring/x:v` も属性を持たず、`xml:space` は抽象エントリ `x:CT_Xstring/`（`XstringType`）にある。
  - `x:CT_Rst/x:is` の `AdditionalElements` も `x:CT_Rst/` 側にある。
- `BaseClass` は SDK の Name ではなく .NET の ClassName で書かれている。ClassName は 374 件が複数のスキーマファイルにまたがって重複している（例: `Text` は dml / math / chartex / sml に存在する）。

### なぜ問題か
- `ElementSpec` は `QNAME` を1つしか持たない（L93）。したがってマーカーは必ず `(型, 要素)` の組ごとに必要になる。allowlist を complex type 単位で書くと、`CT_Xstring` を許しただけで 15 個のマーカーが一度に入り、「allowlist の差分 = 生成物の差分」というレビュー単位（L191-193）が崩れる。
- ベースクラスの属性をマージしないまま生成すると、`Cell::ATTRS` は空になり、`Text::ATTRS` から `xml:space` が落ちる。ADR-0001 L230-238（R2-M5）は、先頭や末尾に空白を含む値を書くときに `xml:space="preserve"` を付けることを求めている。Phase0 の必須要件が表から表現できなくなる。
- ClassName は名前空間をまたいで衝突するので、`BaseClass` は同じスキーマファイル（同じ .NET 名前空間）の中で解決するという規則が要る。

### 提案
- L137-139 を「マーカーと allowlist の単位は SDK の `Name`（`x:CT_Cell/x:c` のような `(型, 要素)` の組）」と書き直す。
- 生成器の規則として、「`BaseClass` を同じスキーマファイル内で ClassName から抽象エントリ（`Name` が `/` で終わるもの）に解決し、`Attributes`/`AdditionalElements` を継承として展開する」ことを §3 か §4 に追加する。

---

## M3 [Major] `ooxmlsdk` を退ける根拠の例が事実と合わない

### 根拠
- L219-224: 「Children that match no generated type are dropped on read … ADR-0001 requires such siblings (e.g. the fixture's foreign `<extLst>`) to survive.」
- `ooxmlsdk-derive/src/sdk_type.rs` L5726-5730: `unmatched_child_tokens_borrowed` は `if pure_any_dispatch || has_any_choice_dispatch { quote!{} } else { build_unmatched_child_tokens() }`。つまり **`Any` を含む型では skip されない**。Any にあたる子は `read_raw_element_xml_bytes` で生バイトとして保持され（L5465-5476, L3584-3616）、`XmlAny(Box<[u8]>)` に格納される。
- fixture の `<extLst><ext uri="…"><x14:customXmlExtension …/></ext></extLst>` は `x:CT_WorksheetExtension/x:ext` に該当する。その particle は `Choice[x14:conditionalFormattings, …, x15:timelineRefs, Any]` で、`ooxmlsdk` の `WorksheetExtension` も `choice(…, any)` になっている（`schemas_openxmlformats_org_spreadsheetml_2006_main.rs` L9922-9938）。

### なぜ問題か
- ADR が挙げた例そのもの、つまり fixture の外部名前空間の `extLst` は、`ooxmlsdk` でもおそらく残る（`ext` の中は Any として生バイトで保持される）。正確に言えば「Any のスロットが無い親の直下にある未知の子は捨てられる」。たとえば `worksheet` 直下や `row` 直下の未知要素、`ooxmlsdk` が overrides で `xr:revisionPtr` を足す前の `workbook` 直下の `xr:revisionPtr` がこれにあたる。
- 退ける結論自体は、L225-232 の「部品を丸ごと再シリアライズするので、触っていない兄弟のバイト一致が保証されない」で十分に成り立つ。しかし誤った例を残すと、後で「extLst は残るではないか」と反論されたときに、ADR-0001 alternative 5（L609-622）が約束した「公平な評価」が崩れる。
- ほかの記述も確認した。`raw_copy_file` による未ロード部品のパススルー（`common/package.rs` L341-360）、ハーネスの eager open（`rt.rs` L26-28）、正規化関数の数（`fn normalize*` が 47 個）は正しい。「~136k generated lines」（L267-268）はローカルのソースでは再現しなかった（`crates/ooxmlsdk/src/schemas/*.rs` が約 112k 行、`parts` を足して約 115k 行）。

### 提案
- L219-224 を「Any ワイルドカードの無い親の直下の未知要素は skip される（Any のスロット、たとえば `ext` の中身は生バイトとして保持される）」に直し、例を `worksheet` 直下や `row` 直下の未知要素に差し替える。主な理由を L225-232（バイト一致ではなく意味の同値）に置き直す。
- 「~136k」は計測した条件（どのディレクトリか）を書くか、削除する。
- ADR-0001 alternative 5 は「専用の評価を待つ」としていた。その評価がこの ADR で行われたことを、ADR-0001 側の追記か ADR-0002 の Context で明記する。

---

## M4 [Major] Phase0 の allowlist では「どれが sheet1 か」を決められず、既存の inlineStr の読み取りも抜けている

### 根拠
- L190-192: 「Phase0's allowlist is exactly the types in §4's last paragraph」（`worksheet`, `sheetData`, `row`, `c`, `f`, `v`, `is`, `t`, `dimension`）。
- L206-210: Phase0 では parts 表を「identifying a worksheet part by relationship type」に使う。
- `data/parts/WorksheetPart.json` は relationship type を持つが、ブック内には worksheet の関係が通常いくつもあり、シートの順序と名前は `workbook.xml` の `<sheets><sheet r:id>` で決まる。`WorkbookPart.json` には `ContentType` が無い（.xlsx/.xlsm/.xltx で異なるため）。
- `x:CT_Rst/x:is` は `t?, r*, rPh*, phoneticPr?` で、`t` は省略できる。

### なぜ問題か
- ADR-0001 の Phase0 は「`sheet1.xml` だけ」を対象にしているが、それがどの部品かは、relationship type だけでは複数シートのブックで決まらない。正しく決めるには `workbook.xml` → `sheets/sheet` → `r:id` → `workbook.xml.rels` の順に辿る必要がある。そのためには `CT_Workbook`, `CT_Sheets`, `CT_Sheet` の読み取りが要る（`ooxml-xlsx` が `cell()` で文字列を返すなら、共有文字列表 `sst`/`si`/`r` も要る）。
- 他のツールが書いた inlineStr セルは、`<is><r><t>…</t></r></is>` のようにリッチテキストの run だけで構成されていることがある。`r`（`CT_RElt`）が allowlist に無いので、`cell()` でその値を読めない。fixture にはこのケースが無いので、テストでも検出されない。
- 「allowlist はちょうどこれだけ」という書き方のせいで、こうした不足が設計判断のように見えてしまう。

### 提案
- Phase0 の allowlist に、読み取り専用として `x:CT_Workbook/x:workbook`, `x:CT_Sheets/x:sheets`, `x:CT_Sheet/x:sheet`, `x:CT_RElt/x:r` を加える。共有文字列を解決するなら sst 系も加える。または「これらは Phase0 では手書きで読む」と明記する。
- §6 の「relationship type で特定」は「`officeDocument` 関係 → `workbook.xml` の `<sheets>` の先頭」に直す。

---

## M5 [Major] overrides の表現力が定義されておらず、Phase0 の時点で必要になる

### 根拠
- L61-65 は overrides の存在と動機だけを書いていて、形式や操作の種類が無い。
- `ooxmlsdk` の SML 用 overrides（上記ファイル）が実際に行っている操作は次のとおり。属性の追加（Worksheet や Chartsheet などへの `xr:uid`）、位置を指定した子の追加（Workbook の `xr:revisionPtr`、`AfterPropertyName: "AbsolutePath"`）、enum の値の追加（`ConditionalFormattingOperatorValues` への `expression`、`SheetStateValues` への `show`）、MCE の位置（M1）、`xml:space` 属性の追加（Workbook）。
- B1 により、既定値も overrides で扱う候補になる。

### なぜ問題か
- 「SDK のデータと実際の Office の出力が食い違う」ケースは、すでに具体例が揃っている。overrides の形式が決まっていないと、生成器の入力の形が決まらない。`--check` の対象に overrides が入るのか、`PROVENANCE.md` に overrides のチェックサムも載せるのか、も決まらない。
- enum の値の不足は、`Unknown(&str)`（L143-148）で読み取り時の損失は防げる。しかし未知の値を書き込む API が無いと、`show` のような正当な値を typed API から書けない。

### 提案
- §1 に overrides の最小限の操作を列挙する: `add_attribute`, `add_child { after | before }`, `add_enum_value`, `set_default`, `mce_wrapped { children }`。各エントリに `reason` と `evidence`（fixture か仕様の節番号）を必須にする。overrides も `--check` の入力に含め、`PROVENANCE.md` にチェックサムを記録する。
- `ooxmlsdk` の overrides を参考にする際、内容を転記するなら、そのライセンス（MIT OR Apache-2.0）の表示義務も負うことを m3 と合わせて書く。

---

## m1 [Minor] コンパイル時検査の書き方が stable の const 評価の性質と合っていない

### 根拠
- L149-153: 「inline `const { … }` assertions (no duplicate QNames within a sequence, every default parses as its simple type)」。L264-266: 「schema violations in *insertion* become compile errors」。

### なぜ問題か / 正確な範囲
- **評価されることが保証されない:** 関数本体の中にある inline `const { }` は、その式が実行時に到達する場合しか評価が保証されない。呼ばれない関数や `if false` の中のブロックは評価されないことがある（Reference の const block の項）。ジェネリック関数の中では、単相化されたときに初めて評価される。検査を確実に走らせるには、型ごとに `const _: () = { … };` というモジュールレベルの const 項目を生成する必要がある（ジェネリックでない文脈では常に評価される）。
- **文字列の比較:** const 文脈で `&str == &str` は使えない（`PartialEq` は const trait ではなく、`const_trait_impl` は 1.99 時点でも unstable。CLAUDE.md L36）。`as_bytes()` と `while` ループで書いた比較関数を使えば stable で書ける。
- **既定値のパース:** 整数は `u32::from_str_radix` などが const として使える（1.82 で安定化）。真偽値と enum は自前のループで書ける。`xsd:double` の既定値（sml.xsd に 10 件）は `f64::from_str` が const でないので、検査できるのは字句形式（lexical space）が正しいかどうかまで。ADR の「parses as」は「字句形式に適合する」に弱める必要がある。B1 が片付くまでは、この検査自体に対象が無い。
- **「コンパイルエラーになる」の範囲:** コンパイル時に弾けるのは「どの型を子にできるか」だけ。順序、個数（`maxOccurs`）、`Content::Particle` の親に対する `UnsupportedContentModel` は実行時のエラーになる。L264-266 は言い過ぎ。

### 提案
- 検査の手段を `const _: () = assert!(…)` 項目に変える。`ChildOf` の impl のそれぞれに `const _: () = assert!(content_contains(Row::CONTENT, Cell::QNAME));` を併記すると、生成器のバグで impl と表が食い違った場合も検出できる。L264-266 は「子の型の正しさはコンパイル時、順序と個数は実行時」に直す。

## m2 [Minor] QName の同一性（名前空間 URI か prefix か）が規定されていない

ADR-0001 の `RawNode` は `attrs: Vec<(String, String)>`（ADR-0001 L165）。一方、ADR-0002 の `QNAME`/`ATTRS` は名前空間つきで表されている（`qn!(SML, "c")`、`x14ac:dyDescent`、`xml:space`）。prefix は文書ごとに自由に選べるので、照合は (名前空間 URI, ローカル名) で行う必要がある。`RawNode` 側でスコープ内の名前空間宣言を解決して保持するのか、照合のたびに解決するのかを、ADR-0002 で決める。

## m3 [Minor] ライセンス表示が vendoring したディレクトリにしか置かれていない

L56-57 と L278-279 は、`reference/` に upstream の MIT LICENSE を置くとしている。しかし `reference/` は crates.io に公開するパッケージには入らない。生成コードは SDK データの名前や `Summary` の文言を含み（L140-142）、MIT の言う「substantial portions」にあたり得る。このワークスペースのライセンスは `MIT OR Apache-2.0`（`Cargo.toml`）。生成コードを含む各クレートのパッケージに .NET Foundation の著作権表示を含めることを明記する（`THIRD-PARTY-NOTICES` を各クレートに置き、`include` に入れる）。これは判断ではなく MIT 条項の要件。

## m4 [Minor] `--check` の決定性

`--check`（L75-77）は、生成器の出力がバイト単位で再現できることを前提にしている。JSON を読む順序、`HashMap` の反復順、rustfmt のバージョン（rustfmt で整形する場合）が揺れると、CI が環境によって落ちる。生成器は `BTreeMap` などで出力順を固定し、整形は生成器の中で行う（`prettyplease` など、バージョンを pin したもの）か、`rust-toolchain.toml` で rustfmt を固定する、と1行書いておく。

## m5 [Minor] 数値と記述の細部

- 「474 across 45 Microsoft extension namespaces」（L27）はスキーマファイル単位では再現した。ただし 45 件のうち 10 件は要素宣言が 0 件（`Types` を持たない。例: `2015_revision2`, `2016_revision3`）。「要素宣言を持つのは 35 名前空間」と書く方が正確。
- `Content::Text(SimpleType)`（L181）について: leaf text の型は 344 件中 187 件でしか `Validators` から取れない。残りは文字列として扱う、と規定する。
- 版によって `Occurs` が変わる particle が 3 件ある（`a:CT_ColorMRU/p:clrMru`, `w:CT_TblGridChange`, `w:CT_DocParts`。いずれも Phase0 の範囲外）。L198-199 の「`InitialVersion` は挙動に影響しない」と同じ扱いで、どちらの `Occurs` を採用するかを決めておく。
- L13-17 の文脈依存の例（`x:dimension` には `CT_SheetDimension` と `CT_PivotDimension`、`x:row` と `x:sheetData` には external-link 用の型がある）はデータで確認できた。

---

## 推奨する修正の優先順位

1. **B2**（生成コードの形をコンパイル可能にする）: 生成器の出力の形が決まらないと何も書けない。共有名前空間を置くクレートも同時に決める。
2. **B3**（挿入 API を「帯（スロット）+ 呼び出し側の順序」に直し、削除・属性操作・重複・dirty の義務を規定する）: Phase0 の `cell_mut` の正しさに直結する。T2 との対応を書き直す。
3. **B1**（既定値の出所を決める）と **M5**（overrides の形式）: 入力仕様として一緒に決める。
4. **M2**（マーカーと allowlist の単位、ベースクラス継承）: 生成器の最初のパスに必要。`xml:space` の要件がこれに依存する。
5. **M4**（Phase0 の allowlist を補う）、**M1**（AC の位置を解決するか、AC がある親への挿入を `Err` にする）。
6. **M3**（`ooxmlsdk` の記述を正確にする）、その後に m1-m5。
