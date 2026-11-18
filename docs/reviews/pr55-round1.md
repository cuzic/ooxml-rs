# PR #55 レビュー（Opus, round 1）

対象: `impl/xlsx-phase3-appxml` @ `6f47555`（#53 `impl/xlsx-phase3` @ `33b4771` の上に積んである）。差分は `33b4771..6f47555` の 11 ファイル。
照合したもの: ADR-0009 §7、§8、§9、§11、corpus fact 5、XL 9(g)、`sheet_ops/app_xml.rs`、`sheet_ops/mod.rs` の配線（`listed`、`stage_app_xml` と 5 つの操作）、`escape.rs`、`names.rs` の `rewrite_location` と `mentions_prefix`、`codegen/allowlist/props.toml`、生成された `props.rs`、`xtask/src/config.rs`、`tests/phase3_appxml.rs`、PR 本文。

リポジトリは変えていない（この文書だけ untracked で置いた）。作業はすべて `scratchpad/review-53r2-55/` で、`git clone` したコピーを使って行った。

- `pr55/crates/ooxml-xlsx/tests/review55.rs`: プローブ Q1〜Q9 と、corpus 用の独立チェッカー `corpus_full_blocks`。作者のチェッカーは**先頭ブロック**しか見ないので、こちらは**全ブロック**を見る。見る内容は、ラベル不変、ブロック数不変、`size` と count の整合、種類ブロック（chart、macro、dialog）＝出力の該当シート一覧、その他ブロック（Named Ranges）の rename 追従と delete での削除、その他の操作では不変、の 6 点である。
- `scripts/mut55.py` と `mut55.txt`: 新しい手書き mutant 20 個（V1〜V20）。`scripts/author_mut.py` と `author_mut.txt`: 作者の A1〜A12 を再実行したもの。
- `corpus_author.txt`、`corpus_full.txt`: POI corpus（`p2/poi`、366 ファイル、`47dcff76`）の結果。
- `py/xfind.py`、`py/zero.py`: corpus の app.xml の事実確認。`python3 -I` で実行した。

## 結論（先に）

**収束（Blocker 0、Major 0）。** Minor が 4 件ある。merge の前に直すことを勧めるが、merge を止める理由にはしない。

ADR-0009 §11 のとおりにできている。

- 認識した形だけを触り、それ以外は 1 バイトも変えない（L1）。
- ラベルは変えない。ブロックを作り出さない。
- `vt:lpstr`、`vt:i4`、`size` の値だけを書き換える。
- 5 つの操作すべてで、同じ `RegistryPlan` の clone が 1 つ増えるだけである。

作者の corpus の数は再現した（2,733 consistent、untouched は 322。PR 本文は 323）。独立チェッカーで全ブロックを見ても、**誤りは 0 件**（ok 2,496、untouched 1,363）だった。作者の mutant も 11/12 を再現し、生き残った A2 が同値であるという主張も確かめた。

残る問題は、どれも app.xml の中だけで閉じている（Excel は保存時に作り直す）。

- **m1**: ブロックが空になると count 0 の組が残る。corpus では 11 回起きた。以後、そのブロックはもう識別できない。
- **m2**: タイトルを ST_Xstring として decode/encode しているが、Excel の app.xml は生の文字列であり、crate のシート名も decode していない。名前に `_xHHHH_` を含むと、次の操作から app.xml が「未認識」になって止まる。
- **m3**: `size` と `baseType` が認識条件に入っていない。ADR の文面とテスト側の oracle は `size` を見ているので、実装だけが違う。
- **m4**: 生き残った mutant 5 個（V4、V9、V10、V16、V17）が、テストの抜けを示している。

## Blocker

なし。

## Major

なし。

## Minor

### m1 [Minor] ブロックが空になると count 0 の組を残す。そのブロックは以後識別できず、unhide しても戻らない

`app_xml.rs:87` は、種類ブロックを「`!l.is_empty() && l == *titles`」で識別している。操作の後にブロックが空になっても、組は消さずに `<vt:i4>0</vt:i4>` を書く（ADR「ブロックは削除しない」に従っている）。

**再現**（Q4）: worksheet `A` と chart sheet `C`、app.xml は `Worksheets[A]`、`Charts[C]` とする。

1. `set_sheet_state("C", VeryHidden)` を呼ぶ。結果は `Charts` の count が `0` になり、titles は `[A]` になる。
2. `set_sheet_state("C", Visible)` を呼ぶ。count 0 のブロックは中身が空なので、もう「Charts」とは識別されない。そのため `app_xml.rs:115` の「作り出さない」ガードに当たり、**app.xml は untouched** になる。結果として、C が見えているのに `Charts` は 0 のまま残る。以後のどの操作でもこの状態は直らない。

Named Ranges でも同じことが起きる。delete で唯一の名前が消えると、count 0 の `Named Ranges` が残る（Q5）。

**corpus**（`corpus_full.txt` の `ZERO` 行）: 11 回起きた。

| 操作 | ブロック | ファイル |
|---|---|---|
| veryHidden | Charts | `47813`、`56557`、`60255_extra_drawingparts`、`chart_sheet`、`SimpleScatterChart`、`WithChartSheet` |
| veryHidden | Excel 4.0 Macros | `64420.xlsm`、`xlmmacro.xlsm` |
| delete | Named Ranges | `47089.xlsm`、`56644`、`mv-calculator-final-2-20-2013.xlsm` |

一方、corpus の app.xml で `HeadingPairs` を持つ 299 件には、count 0 の組は **1 件もない**（`py/zero.py`）。Excel が書かない形を、こちらが作っていることになる。

壊れはしない（`size` と count は整合している）。ただし、§11 の「一貫させる」が往復で成り立たない。

**修正案**（どちらか）:

- (a) 空になった組は消す。Excel が保存するときと同じ形になる。ADR §11 に「空になったブロックは削除する（作り出しはしない）」と書き足す。unhide はその後「ブロックがない → untouched」になるので、今と同じ扱いになる。
- (b) ブロックが空になる操作では、app.xml を untouched にする。

どちらにするかを ADR に書き、テストで固定すること（V4 も参照）。

### m2 [Minor] タイトルの ST_Xstring 扱いが、Excel にも crate のシート名にも合っていない

- `app_xml.rs:54` は、読むときにタイトルを `escape::decode` する。`:147` と `:150` は、書くときに `escape::encode` してから比べる。PR 本文も「Titles are ST_Xstring-encoded」と書いている。
- しかし `vt:lpstr` は `xsd:string` である。corpus の `xssf-enum.xltx.xlsx` では、`sheet/@name` が `…"Школа"_x000a_`（ST_Xstring）なのに、app.xml のタイトルは生の改行になっている。Excel は app.xml を ST_Xstring で書いていない。
- 比べる相手の `Listed.name` は、ローダーが `sheet/@name` を **decode せずに**持っている値である（`workbook.rs` の `raw`）。

**再現**（Q9）: `rename_sheet("A", "x_x0041_y")` を呼ぶ。

- `workbook.xml` には `x_x005F_x0041_y` と書かれ、`sheet_names()` も `x_x005F_x0041_y` を返す。
- app.xml にも `x_x005F_x0041_y` と書かれるが、decode すると `x_x0041_y` になる。
- そのため、次の `add_sheet("C", 2)` では先頭ブロックが一致せず、app.xml は**未認識として止まる**。同じセッションの中でも、reopen した後でも、`C` が足されない。

**修正案**: app.xml のタイトルは生の文字列として読み書きする（decode も encode もしない。XML のエスケープは `set_text` がする）。比べる相手は、ローダーが持つシート名と同じ表現にそろえる。

（範囲外の関連事項。#53 側の問題なので、そちらに回す: `rename_sheet` が `sheet/@name` を `escape::encode` する（`mod.rs:813`）のに、ローダーは decode しない。そのため、上の rename の後は `sheet_names()` が渡した名前と違う値を返し、`rename_sheet("x_x0041_y", …)` も見つからない。）

### m3 [Minor] `size` と `baseType` が認識条件に入っていない

ADR §11 は「`TitlesOfParts` は、その size が count の合計に等しい `vt:lpstr` vector」としている。テスト側の `app_blocks` も、`@size` と子の数の一致を認識条件にしている。ところが `blocks()`（`app_xml.rs:42-70`）は、子の数と count しか見ない。

**再現**（Q3）: 次の入力はどれも「認識」され、`add_sheet` で書き換えられる。

- `TitlesOfParts` の `size="5"`（子は 2）。出力では `size="3"` に直る。
- `HeadingPairs` の `size="6"`（子は 2）。出力では `size="2"` に直る。
- `size` がない。出力では `size` が足される（属性の順序も変わる）。
- `TitlesOfParts` の `baseType="variant"`。そのまま残り、`size` だけが更新される。

害は小さい（直す方向の変更である）。ただし、「すでに食い違っている app.xml は触らない」という L1 の線引きが、ADR、実装、oracle の 3 か所で違っている。作者の corpus sweep がこれで落ちないのは、corpus にこの形がないからにすぎない。

**修正案**: `@size` が子の数と一致し、`baseType` が `variant` と `lpstr` であることを `blocks()` の条件に入れる。または ADR の文面を実装に合わせて、oracle も合わせる。

### m4 [Minor・テスト] 新しい mutant 20 個のうち 7 個が生き残る。意味のあるものは 5 個

`phase3_appxml`、`phase3`、`phase3_review1`、`phase3_smoke` で回した（`mut55.txt`）。13 個が死んだ（V1、V2、V5〜V8、V11〜V15、V18、V20）。

生き残ったもののうち、意味のあるもの:

- **V4**: 空の種類一覧も識別に使う（`!l.is_empty()` を外す）。count 0 のブロック（m1）を固定するテストがない。
- **V9**: `HeadingPairs/@size` を書かない。`size` が食い違った入力のテストがない（m3）。
- **V10**: タイトルを encode しない。`_x` や制御文字を含む名前のテストがない（m2）。
- **V16**: vector の子の種類を検査しない。`vt:lpwstr` が混じったものや、他の型が混じったもののテストがない。
- **V17**: variant の子が 1 つであることを検査しない。

同値と判断したもの:

- **V3**（`covered` を無視する）: 同じタイトル列のブロックが 2 つないと差が出ない。
- **V19**（no-op の判定を外す）: `write` は違う値しか書き換えないので、バイト列は同じになる。

作者の A1〜A12 を再実行した結果（`author_mut.txt`）も、A2 だけが生き残った。A2 が同値であるという主張は確かめた。操作が変えるのはシート 1 枚で、ブロックのない種類に新しくシートが載るのは、veryHidden の chart、macro、dialog を unhide するときだけである。そのとき、ほかのブロックは変わらず、`new_blocks == old` で untouched になる。

**必要なこと**: V4、V9、V10、V16、V17 を落とすテストを足す。m1〜m3 を直せば、その回帰テストで大半が死ぬ。

## Nit

- 整形された app.xml（Q2[2]）で、足した `vt:lpstr` にインデントが付かない（`</vt:lpstr>\n    <vt:lpstr>C</vt:lpstr></vt:vector>`）。消したときも、空白のテキストノードが残る。意味は変わらない。
- 生成された型名が `VTLPSTR`、`VTInt32`、`VTVector` で、ほかのグループの命名（`Text`、`CellValue`）と揃っていない。公開 API に出るので、`ELEMENT_SUFFIXED` と同じように、今のうちに決めておくとよい。
- ADR §11 の最終行は、allowlist の prefix を `ep:` としているが、実装は `ap:` である。
- PR 本文の corpus の数「323 untouched」は、こちらの再実行では 322 だった（2,733 は一致した）。
- 作者の corpus test は先頭ブロックしか確かめていない。種類ブロックと Named Ranges の検証は、合成 fixture の 3 本だけである。こちらの `corpus_full_blocks` 相当を入れておくとよい（結果は 0 誤り）。

## 確認した点（問題なし）

- **認識と L1**（Q8、Q3、Q7）:
  - 次のものは untouched になる: `vt:lpwstr` のタイトル、`vt:i2` の count、先頭が Worksheets でないブロック、`HeadingPairs` のない app.xml、app.xml がない場合（corpus にある）。壊れた app.xml は、`xml_part` が失敗すると `Ok(())` で飛ばすことをコードで確かめた（`mod.rs:466` 以降）。
  - コメント、CDATA、文字参照（`&#65;`）を含む vector は認識され、正しく更新される。
  - rename が `UnrewritableReference` で拒否されたときは、すべての zip エントリが同一のままだった。その後の操作も正しく app.xml を更新する。
- **名前空間**（Q2）: prefix が `ep:` と `v:` のもの、`vt` を vector の上で局所的に宣言したもの、整形されたもの。どれも正しい名前空間で、追加、削除、count、`size` が更新される。
- **エスケープ**（Q1）: `&`、`<`、`>`、`"`、`'`、タブ、LF は、`set_text` で正しく XML エスケープされ、reopen でも同じ名前に戻る（ST_Xstring については m2 を参照）。
- **順序と hidden**（Q6）:
  - veryHidden のシートが途中にあっても、add、move、rename、delete、Hidden、Visible、VeryHidden のどれでも、ワークブック順で正しく一覧に出る。
  - Hidden のシートは一覧に残る。veryHidden のシートの rename と delete では、app.xml を触らない。
  - chart sheet を worksheet の前に動かしても、Charts ブロックは chart の順序だけで並ぶ（Q4）。
- **Named Ranges**（Q5）:
  - `b!Print_Area` は、`B` の rename で大文字小文字を畳んで `'B 2'!Print_Area` になり、delete では消える。
  - シートと同じ名前のグローバル名 `Data` は、rename でも残る。`'Data'!Foo` は `'My Data'!Foo` になる。
  - ローカライズされたラベル（`Arbeitsblätter`、`Benannte Bereiche`）は変わらない。
- **macro sheet**（Q6、corpus の `64420.xlsm`、`xlmmacro.xlsm`）: `Excel 4.0 Macros` ブロックは rename に追従する。add や move で worksheet の順序が変わっても、macro ブロックは崩れない。dialog sheet は corpus になく、合成 fixture も作っていない（コードの経路は macro と同じ）。
- **corpus**: 作者のテストで 2,733 / 322、0 failures だった。独立チェッカーでは ok 2,496、untouched 1,363、**誤り 0** だった。種類ブロックが付いたファイルとして、`57181.xlsm`（`Diagramme` と `Benannte Bereiche`）、`WithChartSheet.xlsx`、`64420.xlsm` を含む。
- **codegen と feature**: `cargo xtask codegen --check` は「8 generated files up to date」だった。`cargo check -p ooxml-schema --no-default-features --features props` は通る（`props` 単独で成り立つ）。`props` の生成物は `cfg_select!` で gate されている。allowlist の 7 項目は report.txt の 7 行と一致し、名前空間（`NS_AP` は extended-properties、`NS_VT` は docPropsVTypes）も正しい。
- `cargo clippy --workspace --all-targets --all-features -D warnings` と `cargo test --workspace`（test result 54 件、すべて ok）は、どちらも通った。

## 判定

**converged: yes**

Blocker と Major はない。merge の前に次を直すことを勧める。

- m1: count 0 のブロックの扱いを決め、ADR に書き、テストで固定する。
- m2: タイトルを生の文字列として扱う。
- m3: 認識条件の `size` と `baseType` について、ADR、実装、oracle をそろえる。
- m4: V4、V9、V10、V16、V17 を落とすテストを足す。

どれも app.xml の中だけの問題で、ブックのデータや参照を壊すものではない。後続 PR に回してもよい。
