# xlsx Phase0 実装レビュー（Opus, round 3: PR #17）

## 対象/方針

- 対象: PR #17、ブランチ `fix/xlsx-phase0-review1`（`23c8676`）。round 2 レビュー（`9a7ad29`）以降の 5 コミット `d1cb9bc..23c8676`。
  - `crates/ooxml-xlsx/src/worksheet.rs`（`plan()`/`apply()` への分割、`check_insertable`、`debug_assert!`、単体テスト `dry_run_matches_core`）
  - `README.md`、`docs/roadmap/xlsx.md` §7
  - `tests/refusals.rs`（`n1_*`、`n3_*`、`n5_*`、`f7_dimension_ref` の拡張）、`tests/edit.rs`（`e16_insert_in_the_middle`）
- 照合した文書: round 1/2 レビュー、ADR-0002 §4.1（`place`）、`crates/ooxml-core/src/lens.rs`（`place`、`insert_at_slot`、`choose_element_prefix`、`set_attr`、`set_text`、`remove`）。
- 方法:
  1. `apply()` の各ステップについて「最初の変更より後に失敗しうるか」を core のコードまで追った。
  2. scratch 上のコピーに探索テスト `tests/adversarial3.rs` を足し、**debug と release の両方**で実行した（`catch_unwind` で `debug_assert!` の panic も記録）。対象: namespace prefix の変種 5 種、構造の変種 11 種（重複 `sheetData`/`dimension`、未知要素、`extLst` の位置と重複など）、v/is 掃除で何が消え何が残るか。
  3. core の `place()` に「もっともらしい将来の厳格化」を入れた模擬ドリフトで、mirror と CI の追従性を確かめた。
  4. fixer の `mutate3.py`（M1–M46）を現 HEAD で再実行した。fixer の `result3.txt` は `23c8676`（`debug_assert!` と単体テストの追加）より**前**の実行なので、生存判定が変わりうる。追加の変異 M47–M52 も足した。
  5. `examples/manual_check` を round 2 時点（`e538dce`）と現 HEAD で実行し、出力を比較した。全 26 フィクスチャで N1/N3 の形を走査した。
- リポジトリ本体は変更していない。このファイルだけを追加した（untracked）。フィクスチャは信頼しない入力として扱い、パースするだけにした。
- 重大度: Blocker/Major は次のいずれか。それ以外は Minor。
  - Err なのに文書が変わる
  - 黙ったデータ消失
  - release ビルドでの健全性の欠如
  - コア保証が未テスト

## 結論（先に）

- round 2 の **N1–N6 はすべて解消**した（N7 は推奨のみで対象外）。
- `apply()` は、**現在の core に対しては release ビルドでも失敗しない**。
  - debug と release の探索 120 ケースで、「plan が Ok なのに apply が Err」は 0 件だった。
  - 構造的な理由: `apply()` の中で**最初の変更より後に**来るステップは、いずれも core の現行規則では失敗できない（下記 §2）。
  - 失敗しうる挿入はどれも、その経路で最初の変更になっている。だから mirror（`check_insertable`）が仮に漏らしても、release では「無変更の Err」になる。
  - 唯一の例外は**既存セルへの `v`/`is` の挿入**だ。`v`/`is` の削除（変更）の後に来る。
- mirror と core の結び付きは**部分的**だ。
  - 単体テスト `dry_run_matches_core` は `Cell`×`v` の 7 形状しか比べない。
  - 模擬ドリフト（core が「他の slot の数超過」も拒否するようになる）では、ooxml-xlsx の全スイートが green のまま通った。そのうえ release では **Err と同時にセルの値が消えた**（R3-1、Minor）。
  - 現時点の欠陥ではないので Minor とした。release での防御（poison など）を推奨する。
- v/is の掃除で、利用者が残したいデータが黙って消えることはない。
  - 消えるのは、上書きされる旧値の `v`/`is`（rich run、`rPh`、`phoneticPr` を含む）だけ。
  - `extLst`（中身ごと）、未知の名前空間の子、コメント、PI、`s`/`ph`/`cm`/外部属性は残る。
  - 未知の子とコメント/PI は、新しい値より前に移る（R3-3、Minor、表示のみ）。
- 回帰はない。
  - `manual_check` の全 11 出力が round 2 時点とバイト単位で同一。CHECKLIST.md も同一。
  - コーパス 26 件（40 sheet）に N1/N3 の形を持つセルは 0 件。
  - debug と release の全スイートが green。
- mutation（M1–M52、52 件）: 45 件が KILLED、8 件が生存した（§4）。
  - fixer の `result3.txt` で生き残っていた M41、M43、M45、M46 は、`23c8676` の後では KILLED になる。
  - 生存したもののうち M16、M40、M42、M44、M47–M49 は、現在の core に対しては等価と判断した（ただし M40 は R3-1 のドリフトの下では等価でない）。
  - 新しい M51（既存セルが範囲外にあるときに `dimension` を広げない）は**等価でない**のに生き残る（R3-2、Minor）。

**判定: converged = yes**（Blocker 0、Major 0。Minor 3 件: R3-1–R3-3。いずれも収束の条件にしない）

## round-2 解消状況

| ID | 状態 | 根拠 |
|---|---|---|
| N1（重複 `v`/`is` と `extLst`: Err で値が消える） | **解消** | 「削除してから挿入」の構造は残るが、`plan()` が全拒否と挿入点を先に決める。既存セルの経路では、`f`/`vm` を除けば残る子は `extLst` と未掲載の子だけになる。だから、旧 `v`/`is` を全部削除した後の `v`/`is` 挿入は、現在の core では構造上失敗しない（§2）。round 2 の再現 2 件は `n1_duplicate_values_cleaned_up` で Ok になり、値も正しい（6 形状 × 2 書き込み。calamine で読めるものは CR）。Err 側は `n1_refusals_change_nothing_same_path` で、Err ⇒ dirty でない、same-path save で L1 同一、を確かめている。私の round 2 の probe p1 と p4 も、現 HEAD で Ok か無変更 Err になった。M38/M39 は KILLED |
| N2（行の途中への新しいセル） | **解消** | `e16_insert_in_the_middle`: 行頭（A2 を B2 の前へ）、行の間（行 3 を 2 と 4 の間へ）、行末（E1）。T2 と CR を確かめている。M29、M30 は KILLED |
| N3（`c`/`is` 内の AC で読み取りが Empty） | **解消** | `locate` がセルと、セル直下の `is` の AC を拒否する（`worksheet.rs:288-295`）。`n3_alternate_content_inside_cell_on_read` で M21、M37 は KILLED。コーパスへの影響は 0 件 |
| N4（roadmap §7 の文言） | **解消** | `docs/roadmap/xlsx.md:522-526` が README と一致した。継承された `xml:space` については「Excel では未確認」と明記されている。manual check の項目は足されていないが、未確認と書いてあるので許容できる |
| N5（M28、M35、M7b が生き残る） | **解消** | `n5_unicode_edges`（NBSP/U+3000 を calamine と照合、U+FFFE/U+FFFF の書き込みと読み戻し）、`f7_dimension_ref` の「内側への書き込みでは綴りがバイト単位で不変」。M28、M35、M7b は KILLED。M16 は round 1/2 と同じく等価 |
| N6（`dimension/@ref` の表記） | **解消** | README に受け入れる綴り（大小文字、`$`、角の順序、単一セル）と拒否する綴りが列挙された。`f7_dimension_ref` が拒否 11 種と受け入れ 6 種を確かめる |
| N7（`cargo fmt` gate） | 対象外 | 推奨のみ。変更なし |

## 2. plan/apply 分割の検証

### `apply()` のステップと失敗可能性

| 経路 | 最初の変更 | その後に来るステップ | 最初の変更より後に失敗しうるか |
|---|---|---|---|
| `sheetData` なし | `get_or_insert_sheet_data`（`Worksheet` への挿入。dry-run 済み） | 空の `sheetData` への `row` 挿入、`set_row_index`、空の `row` への `c` 挿入、`set_cell_reference`、空の `c` への `v`/`is`(`t`) 挿入、`set_text`、`dimension` | **いいえ**。親はいずれも新規で空。prefix は親自身の prefix が同じ名前空間に束縛されているので必ず選べる（`choose_element_prefix` の最初の分岐）。属性 spec はすべて complete（`ATTRS_INCOMPLETE` は既定の false） |
| 新しい行 | `insert_row_by`（dry-run 済み） | 同上（`row` 以降） | **いいえ** |
| 既存の行、新しいセル | `insert_cell_by`（dry-run 済み） | 空の `c` への挿入、`set_*`、`dimension` | **いいえ** |
| 既存のセル | `v`/`is` の全削除 | `t` の削除/設定、`v` か `is` の挿入（dry-run 済み）、`set_text`、`dimension` | **現在の core ではいいえ**。残る子は `extLst` と未掲載の子だけ（`f` → `FormulaPresent`、AC → `locate` で拒否）。`extLst` の slot は `v`/`is` より後ろなので、順序違反も `AlreadyPresent` も起こりえない。**ただし core の `place()` の規則が変われば、ここだけが「変更の後の失敗」になる** |
| `dimension` | — | `set_reference`（`set_attr`。正規形の ASCII なので `InvalidXmlChar` にならない） | いいえ |

- テキストの XML 文字は `plan()` で先に検査される。`escape::encode` の後では到達しない（M44 は等価）。
- `expect("planned with a dimension")` などの `expect` は、`plan` と `apply` が同じ `&mut self` の下で連続して走るので、到達しない。

### 探索（`adversarial3.rs`、debug/release）

- **namespace prefix（q1、5 種 × 3 セル × 2 型）**:
  - 既定名前空間が別の URI に束縛された `x:worksheet`（`sheetData` なし）
  - prefix を行にだけ宣言したもの
  - `sheetData xmlns=""`
  - 行で `x` を再宣言したもの
  - セルだけ別 prefix のもの
  
  → すべて Ok。新しい要素は親の prefix を使い、読み戻しも一致した。
- **構造（q2、11 種 × 4 セル × 2 型）**:
  - 重複 `sheetData`、重複 `dimension`（2 つ目が不正）、`worksheet` 直下の未知要素
  - `extLst` が `dimension` より前（→ `UnresolvedSibling`、無変更）
  - 行の `extLst` がセルの間にある（→ 後ろへの挿入は `UnresolvedSibling`、無変更）
  - 行の `extLst` 2 つ、`sheetData` 内の未知要素
  - セル内の `extLst` 2 つ、`<extLst/><is/><extLst/><v/>`、main 名前空間の未知の子
  
  → Ok か「無変更の Err」のどちらかで、**panic も「Err と変更」も 0 件**。debug と release で結果は同一だった。
- `cargo test --release -p ooxml-xlsx`: 全スイートが green。

### R3-1 (Minor) — mirror と core の結び付きが部分的。ドリフトすると release で「Err と変更」になる

**根拠**

- `check_insertable`（`worksheet.rs:155-192`）は core の `place()`（`lens.rs:329-371`）の写しだ。両者を結ぶのは次の 2 つしかない。
  - (a) 単体テスト `dry_run_matches_core`: `Cell` に `v` を挿入する場合だけを 7 形状で比べる。`Row`×`c`、`SheetData`×`row`、`Worksheet`×`sheetData`、`Cell`×`is` は比べない。
  - (b) `write()` の `debug_assert!`: テストが実際に通した形状でしか働かない。
- **模擬ドリフト**（`drepo/`）: core の `place()` に、「対象以外の slot が `max_total` を超えていたら `UnresolvedSibling`」というもっともらしい厳格化を入れた。
  - ooxml-xlsx の全スイート（`--lib`、`edit`、`package`、`refusals`、`oracle`）は **green のまま**だった。
  - 入力 `<c r="A1"><v>1</v><extLst/><extLst/></c>` に `set_string("n")` を実行した結果:
    - debug: `plan() let through a write that apply() failed` で panic
    - **release: `Err(UnresolvedSibling(c))`、part は dirty、A1 は `<c r="A1" t="inlineStr"><extLst/><extLst/></c>`（値が消える）**
- `debug_assert!` は release では何もしない。ドリフトが起きると、release の利用者は「Err なら無変更」が破れた状態を、通常の Err として受け取る。

**なぜ Minor か**

- 現在の core では、この経路に到達する入力は見つからなかった（§2 の表と探索）。
- 「変更の後に失敗しうる」挿入は、既存セルへの `v`/`is` の 1 か所だけだ。そこは (a) の形状群で部分的に守られている。
- 起こるのは、将来 core を変えたときだけだ。

**提案**（どれか一つで十分）

1. **core に dry-run を持たせ、写しをなくす**: `LensMut::check_insert::<C>(&self, children_filter)` か、`place()` を `Lens`（読み取り）側から呼べる形で公開する。core のテストがそのまま保証になる。
2. **release でも防御する**: `apply()` が Err を返したら、`debug_assert!` に加えて part（か `Workbook`）を poison する。たとえば `Error::Internal` を返し、以後の `save`/`to_bytes` を拒否する。最低でも、Err と変更が黙って保存されることは防げる。既存セルの経路なら、`c` の `RawNode` を `apply` の前に clone しておき、Err なら差し戻すのも安い（core に置換 API が要る）。
3. **最低限**: `dry_run_matches_core` を 4 つの親子組み合わせに広げる。形状には、変更の後の挿入に効くもの（`extLst` の重複、未掲載の子、wildcard の子）を足す。

**「debug だけ panic」の評価**: テストのオラクルとしては妥当だ。panic するので CI で確実に見える。ただし、release で何もしないのは、「Err なら無変更」を README が保証していることと釣り合わない。上の 2 のように、release では poison して Err にするのを推奨する。

## 3. v/is の掃除で消えるもの・残るもの

`q3_cleanup_preserves`（rich `is`、`rPh`、`phoneticPr`、コメント、PI、未知の名前空間の子、`extLst/ext` の中身、`s`/`ph`/`cm`/外部属性を持つセル）:

| 対象 | 結果 |
|---|---|
| 旧 `is` の rich run、`rPh`（ルビ）、`phoneticPr` | 消える。上書きされる旧値の一部なので妥当。Excel で値を上書きしても同じく消える |
| 重複した 2 つ目の `is`/`v`（`extLst` の後ろを含む） | 消える。スキーマ違反の旧値。round 2 で、calamine がこちらを読む場合があると確かめた。残すと読み手ごとに値が食い違うので、消すのが正しい |
| `extLst`（`ext` の中身ごと）、未知の名前空間の子、コメント、PI | **残る** |
| `s`、`ph`、`cm`、外部名前空間の属性 | 残る（`t` だけが変わる） |

### R3-3 (Minor) — 未掲載の子とコメント/PI が新しい値より前に移る

- 例:
  - `<!--note--><is>…</is><?pi x?><u:keep/><extLst>…` → `<!--note--><?pi x?><u:keep/><v>1</v><extLst>…`
  - `<bogus/><v>1</v><bogus2/>` → `<bogus/><bogus2/><v>2</v>`
- 原因: core の `place()` の規則で、未掲載の子は「直前の掲載済み兄弟に付く」と扱われ、新しい要素は次の slot の先頭に入る。旧 `v` を消すと、その後ろにあった未掲載の子が前の兄弟に付き直す。
- データは失われない。T2 の「変化は編集したセルの span の中」も守られている。ただし、ADR-0002 §4.1 の「未掲載の子は直前の掲載済み兄弟に付く」という意図からすると、旧 `v` に付いていた子が新しい `v` の前に来るのは、わずかにずれている。記録だけ残す。

## 4. mutation の再実行

再実行には現 HEAD のコピー（`mrepo/`）を使った。テスト対象は fixer と同じく `--lib`、`edit`、`package`、`refusals`、`oracle`（debug ビルド）。

| 変異 | fixer（`result3.txt`、`23c8676` より前） | 今回 | 判断 |
|---|---|---|---|
| M1–M39（M16 を除く） | KILLED | KILLED | — |
| M16 セル単位の formula ガードを削除 | SURVIVED | SURVIVED | **等価**。workbook 全体のガードと重複（round 1/2 と同じ） |
| M40 セル内容の dry-run を削除 | SURVIVED | SURVIVED | **現 core では等価**。残る子は `extLst` と未掲載の子だけで、`v`/`is` の挿入は失敗しえない（§2）。ただし R3-1 の模擬ドリフトの下では等価でない。この dry-run は、唯一の「変更の後の挿入」を守る保険として残す価値がある |
| M41 行の dry-run を削除 | SURVIVED | **KILLED**（`n1_refusals_change_nothing_same_path`） | 等価ではなかった。`extLst` がセルより前にある行で到達する。今は `debug_assert!` 経由で検出される |
| M42 `sheetData` の dry-run を削除 | SURVIVED | SURVIVED | **等価**。`sheetData` の掲載済みの子は `row`（上限なし）だけで、AC は `locate` が先に拒否する |
| M43 `worksheet` の dry-run を削除 | SURVIVED | **KILLED**（同上） | 等価ではなかった。`sheetData` がなく子の順序が崩れた worksheet で到達する |
| M44 XML 文字の事前検査を削除 | SURVIVED | SURVIVED | **等価**。`escape::encode` の後と `format_number` の出力に不正な文字は残らない（M35 は KILLED）。多重防御として残すのは妥当 |
| M45 dry-run が slot 順序を無視 | SURVIVED | **KILLED**（`dry_run_matches_core` ほか） | — |
| M46 dry-run が `AlreadyPresent` を無視 | SURVIVED | **KILLED**（`dry_run_matches_core`） | — |
| M47 `debug_assert!` を削除（新規） | — | SURVIVED | 現 core では等価（到達する入力がない） |
| M48 行の dry-run と `debug_assert!` を両方削除（新規） | — | SURVIVED | **観測上は等価**。行/worksheet/`sheetData` への挿入は、その経路で最初の変更なので、失敗しても無変更の Err になる。この 3 つの dry-run は「Err ⇒ 無変更」には寄与せず、`debug_assert!` と組んで mirror の正しさを検査する役割だけを持つ |
| M49 `worksheet` の dry-run と `debug_assert!` を両方削除（新規） | — | SURVIVED | 同上 |
| M50 dry-run の対象に旧 `v`/`is` を含める（新規） | — | KILLED（15 テスト） | — |
| M51 既存セルが `dimension` の外にあるとき広げない（新規） | — | **SURVIVED** | **等価でない**（R3-2） |
| M52 dry-run が AC 兄弟を無視（新規） | — | KILLED（`dry_run_matches_core`） | — |

fixer が「等価」とした生存 5 件（M40、M42、M44 と、`23c8676` より前の M41/M43、または M45/M46）に対する判断:

- **M40、M42、M44 は等価**という主張を支持する。
- **M41、M43 は等価ではなかった**。ただし今は KILLED なので問題はない。
- M45、M46 は単体テストで KILLED。

### R3-2 (Minor) — 範囲外にある既存セルへの書き込みで `dimension` を広げる経路が未テスト

- 変異 M51（`(!range.contains(at) && loc.cell.is_none())`）が全スイートを生き残る。
- 既存の `dimension` テストは、次の 2 種類しかない。
  - 新しいセルを範囲外に書くもの
  - 範囲内に書くもの（綴りが不変であることの確認）
- 既存セルが（古くなった）`dimension` の外にある入力はない。実装は正しい（`plan()` はセルの有無に関わらず `contains` を見る）。
- README の「Grown to include the written cell」は、この場合も保証している。
- 提案: `f7_dimension_ref` に、`dimension ref="A1:B2"` と既存セル D2（範囲外）を持つ入力を足し、D2 への書き込みで `A1:D2` になることを確かめる。

## 5. 回帰の確認

- **`manual_check`**: `e538dce`（round 2 時点）と `23c8676` の出力 11 ファイルの全 zip エントリがバイト単位で同一だった。`CHECKLIST.md` も同一。Excel での確認項目（roadmap §7.6）の期待値に変化はない。
- **コーパス**: 26 ファイル（40 sheet）を走査した。セル内 AC、`v` の重複、`is` の重複を持つセルは 0 件。N1 の掃除と N3 の読み取り拒否が実ファイルに及ぼす影響はない。
- **テスト**: debug の全スイート（unit 7、edit 19、oracle 5、package 8、refusals 21、doc 2）と、`--release` の全スイートが green。
- Phase0 の保証との関係:
  - 「Err ⇒ 無変更」: 現 core では成り立つ（§2）。将来のドリフトについては R3-1。
  - T2: 掃除の差分もセルの span の中に収まる。
  - 「新しい行/セルは順序どおり」: e16 で担保された。
  - 「`dimension` は内側への書き込みでは触れない」: f7 で担保された。

## 再現物

scratch ディレクトリ: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-xlsx-p0-r3/`

| ファイル | 内容 |
|---|---|
| `repo/crates/ooxml-xlsx/tests/adversarial3.rs` | 探索テスト q1–q4（q5 は `drepo` 側） |
| `adv3_debug.out`、`adv3_release.out` | その出力（debug / release） |
| `drepo/` | core `place()` の模擬ドリフトと `q5_drift` |
| `mutate3.py`、`mut3_result.txt` | M1–M52 の再実行結果 |
| `mc_old/`、`mc_new/` | `manual_check` の出力比較 |

## 判定

- Blocker: 0
- Major: 0
- Minor: 3
  - R3-1: mirror と core の結び付きが部分的で、release では防御がない
  - R3-2: 範囲外の既存セルでの `dimension` 拡張が未テスト
  - R3-3: 掃除の後、未掲載の子が新しい値より前に移る
- round 2 からの持ち越し: なし（N1–N6 は解消、N7 は推奨のみ）

**converged = yes**。

- R3-1 は、現在の core に対しては欠陥ではない。core に dry-run API を足すか、release で poison する対応を、Phase1 か core の次の変更で行うことを推奨する。
- R3-2 はテストを 1 件足せば閉じる。この PR で足すのが望ましいが、収束の条件にはしない。
