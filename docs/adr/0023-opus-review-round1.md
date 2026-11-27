# ADR-0023 レビュー（Opus, round 1）

対象: `docs/adr/0023-xlsx-range-io-and-python-binding.md` revision 1（未コミットのワークツリー、base `f88d02c`）。

照合したもの:

- `main`（`f88d02c`）: `crates/ooxml-xlsx/src/{workbook.rs, worksheet.rs, setters.rs, edit.rs, error.rs, graph.rs, registry/dates.rs, registry/format.rs, sheet_ops/mod.rs, sheet_ops/grid.rs}`、`crates/ooxml-core/src/package.rs`、`crates/ooxml-docx/src/replace.rs`、`crates/ooxml-pptx/src/{presentation.rs, slides.rs}`、`.github/workflows/ci.yml`、`Cargo.toml`。
- 未コミットの js-compat 作業（読み取りのみ。ビルドしていない）: ADR-0021、ADR-0022（revision 3 と round 1/2 の記録）、`crates/ooxml-xlsx/src/{worksheet.rs, workbook.rs, setters.rs, edit.rs, error.rs, sheet_ops/mod.rs, formula/precedents.rs}`、`crates/ooxml-xlsx-wasm/src/lib.rs`、`packages/`（エラーコードの文字列）。行番号はこのワークツリーの現時点のもので、今後ずれる。
- ADR-0001、ADR-0003 §3.1、ADR-0021 "Cell and range projection" / "Mutation and ownership" / "Rust changes and transaction choice"、`AGENTS.md`、`docs/process.md`。

以下、「main」は `f88d02c`、「js」は js-compat ワークツリーを指す。

## 結論（先に）

方向は妥当である。主な判断は次の 4 つで、どれも正しい:

- Workbook を主語にし、シートを引数で渡す。
- 所有型で返す。
- 数式キャッシュを作らない。
- `Mutex` を GIL を外した状態でだけロックする。

Context の API 名は、ほぼすべて実在を確かめた。`Send`/`Sync` の主張もコンパイルで確かめた。

ただし Major が 9 件ある。分類すると次のとおり:

- **ロールバックの分離:** M1
- **エラーの分類:** M2
- **書き込みが典型的なシートで拒否される:** M3、M4
- **分類器が fail open になる:** M5
- **読み取りモデルの欠け:** M6、M8
- **名前解決の仕様と流用元の食い違い:** M7
- **ワークスペースと CI への影響の見積もり:** M9

**converged: no**。

## 前ラウンドの解消状況

round 1 のため、なし。

## 検証したこと

- **`Send`/`Sync`（コンパイルで確認）。** リポジトリ外のスクラッチ crate で、main の各 crate に対して次を assert し、すべて通った:
  - `Send`: `Workbook`、`Mutex<Workbook>`、`Document`、`Presentation`、各 crate の `Error`
  - `Sync`: `Mutex<Workbook>`、`Document`、`Presentation`、`ooxml_xlsx::Error`

  `Workbook: Sync` は ``OnceCell<ooxml_xlsx::graph::Dependents>` cannot be shared between threads safely`` で失敗した。ADR の表のとおりである。
  - js 側は指示どおりビルドしていない。追加フィールドは `Mutex<Option<SharedStringIndex>>`（js `workbook.rs:198`）だけで、`Send` を壊す型は見当たらない。
  - PyO3 の `allow_threads` は、クロージャが `&Mutex<Workbook>` と所有型の結果（`Result<RangeRead, Error>`）を運べればよい。これは上の結果で成り立つ。§11.1 の推論は正しい。
- **Context 表の API。** 次のものが実在する。
  - js（`worksheet.rs`）:
    - `declared_range`/`declared_bound`/`stored_extent`（1043/1058/1070）
    - `has_complex_value`（1343）、`has_non_scalar_content`（1330）、`ordinary_formula`（124）、`shared_string_index`（111）
    - `non_scalar_ranges`（1236）、`formula_result_ranges`（1269）、`array_formulas`（1075）、`shared_formula_at`（1207）、`translate_shared_formula`（1220）
    - `row_numbers`（702）、`cells_before_row`（717）
  - js（その他）:
    - `setters.rs`: `is_date_style`（609）、`set_iso_date`（480）
    - `sheet_ops/mod.rs`: `defined_name_nodes`（195）、`global_defined_names`（204）、`global_defined_name_formulas`（212）、`set_(global_)defined_name(s)`（221–236）
    - `workbook.rs`: `worksheet_names`/`worksheet_identities`（1049/1054）、`Workbook::fork`（230）
    - `ooxml-core/src/package.rs`: `Package::fork`/`generation`（195/529）
  - main:
    - `CellWrite` は `Number`/`String`/`Formula`/`RemoveFormula`（`worksheet.rs:633-643`）
    - `set_datetime`（`setters.rs:230`）、`date1904`（`setters.rs:86`、`&mut self`、`Result<bool>`）、`Worksheet::is_date`（`setters.rs:342`、`&mut self`、レジストリを有効にしない）
    - `ExcelDateTime::from_serial(f64, bool)`（`registry/dates.rs:106`）
    - `edit_refusal() -> Option<Error>`（`workbook.rs:446`）
    - `worksheet_by_name`（完全一致の後、fold して一意なら一致、`workbook.rs:366-382`）、`worksheet_by_index`（`<sheets>` の位置、`workbook.rs:387`）
    - private の `sheet_ops::defined_names`（`sheet_ops/mod.rs:162`）
  - js の Wasm 側の private ヘルパー: `classify_sheet` 5118、`classify_cell` 5125、`check_excel_edit` 5377、`classified_value` 5396、`classify_scalar_value` 5407、`scalar_cell_write` 5698、`unsupported` 5826、`js_error` 5833。Wasm の `worksheet(name)` は完全一致である（5108-5115）。
  - 実在しないもの: `Workbook::shared_strings`（m6）、`paragraph_texts`/`replace_many`/`slide_texts`。ただし最後の 3 つはバインディング側の合成として書かれているので、問題ない。
- **非公開情報の漏れ。** ADR 本文に非公開プロジェクト名、マシン固有のパス、ユーザー名は無い。動機は「PAD/UiPath migration」だけである。

## Findings

### M1（Major）: `transaction()` はスレッド間で分離されていない。ロールバックで、別スレッドの成功した編集が黙って消える

- **該当箇所:** §11.1、§11.2「Transactions」、Alternatives の「Per-call fork」。
- **根拠。** §11.1 では、ロックはメソッド呼び出しごとに取って放す。§11.2 は、`with wb.transaction():` の入口で `fork()` を取り、ブロックが例外で抜けたら fork を mutex に戻す、としている。トランザクションの間、ハンドルは他のスレッドに開いたままである。§11.1 は「RPA hosts with worker threads」を想定している。
- **壊れる例。**
  1. スレッド A がトランザクションに入る。
  2. スレッド B が `write_cells` に成功する（B から見ると確定した編集）。
  3. A のブロックが `RefusedError` を出す。
  4. fork が戻り、**B の編集が消える。** B には何も通知されない。

  逆に、A が正常に抜けた場合は、A のブロックに B の編集が混ざって確定する。「Transactions do not nest (`RuntimeError`)」も、別スレッドからの `transaction()` を入れ子と見なすのか待たせるのかが決まっていない。
- **なぜ重要か。** これは「成功した編集は残る」という、呼び出し側が頼る唯一の保証を破る。fork の復元は ADR-0021 の staging と同じく「入口の状態」に戻すが、ADR-0021 のハンドルは単一スレッド（Wasm）なので、この問題が起きなかった。
- **修正案。** トランザクションに所有者を持たせる。
  - 開いている間、他スレッドの呼び出しは GIL を外して待つ（`Condvar`）か、`RuntimeError` で拒否する。どちらかを §11.2 に書く。
  - 所有者スレッドの判定は、`std::thread::ThreadId` を mutex の中に置いて行う。
  - 別スレッドからの `transaction()` は、入れ子ではなく待機（または拒否）にする。
  - oracle 10 に「トランザクション中の別スレッドの書き込みは、待たされるか拒否され、ロールバックで消えない」を足す。

### M2（Major）: 例外カテゴリの表を `kind` 文字列だけで引くと、crate 間で同名の variant が衝突し、入力エラーが「壊れたファイル」に分類される

- **該当箇所:** §8、§11.3、oracle 9。
- **根拠。**
  - **同名の variant。** js の 4 crate の `Error` の variant 名（core 39、xlsx 106、docx 80、pptx 80）のうち、5 つが複数の crate にある: `InvalidValue`、`InvalidXmlChar`、`MergedCell`、`NoStylesPart`、`NoSuchTable`。このうち `InvalidValue` は意味が逆である:
    - xlsx: 「A stored value could not be decoded」（js `error.rs:186-191`）。§11.3 はこれを `MalformedFileError` に入れている。
    - docx: 「A value outside what Phase1 writes (an empty font name, …)」（`ooxml-docx/src/error.rs:255-257`）。これは呼び出し側の入力エラーである。

    `Core(e)` は `e.kind()` を返す（§8）ので、`kind` からは crate も分からない。
  - **入力エラーなのに `Malformed`。** 呼び出し側の誤りで `Malformed` を返す箇所がある:
    - `write_cells_with_styles` の長さ不一致（js `worksheet.rs:881-883`）
    - `set_shared_formula` の範囲と結果数の不一致（js `worksheet.rs:1094-1100`）

    これらは Python では `MalformedFileError` として出てしまう。
  - **`Io` の分類が無い。** core の `Io` は `open` でファイルが無い場合などに出る。表に無いので、Python 利用者が期待する `FileNotFoundError`/`OSError` にならない。
- **修正案。**
  - バインディングの表のキーを `(crate, kind)` にする。ハンドルの型で crate は決まる。`Core` 由来のものは `("core", kind)` で引く。
  - core の `Io` は `OoxmlIOError(OoxmlError, OSError)` に写し、`errno` を引き継ぐ。`NotFound` は `FileNotFoundError` の subclass にする。
  - U1 の受け入れ条件に、次の監査を入れる: 呼び出し側の入力から `Malformed`/`InvalidValue` を作っている箇所を列挙し、入力用の variant（例: `InvalidArgument`）に移すか、表で例外として扱うかを決める。
  - oracle 9 の網羅テストは、`(crate, kind)` の組で数える。

### M3（Major）: `clear_range`（と Python の `write_range`/`clear`）は、shared formula グループのセルを 2 つ以上含む範囲を必ず拒否する

- **該当箇所:** §6.2 `clear_range`、§12 `clear`/`write_range`、oracle 2・6。
- **根拠。** main `edit.rs:450-457` と js `edit.rs:597-603` は、同じ shared グループのセルを 1 つのバッチで 2 つ書くと、`SharedFormulaMaster { reason: "another cell of shared group {si} is written in the same batch" }` を返す。js の `write_cells` の doc もそう書いている（`worksheet.rs:869-870`）。
- **なぜ重要か。** Excel は、数式を下へ fill すると shared formula で保存する。§6.2 は `clear_range` を「1 つの `write_cells` batch」と決めているので、次の典型操作がすべて `RefusedError` になる:
  - 数式列を含むデータ領域の「範囲をクリア」
  - 前回出力の上への `write_range`

  js には、グループ単位でクリアする仕組みが既にある。ただし `clear_shared_formula_row` は 1 行に収まるグループ限定である（js `edit.rs:417-440`、`ClearingFormulaGroup::Shared`）。ADR はこの制約に触れておらず、oracle も shared グループを含む範囲を試していない。
- **修正案。** どちらかを決めて §6.2 に書く。
  - (a) `clear_range` で、範囲がグループの全セルを含む場合は `ClearingFormulaGroup::Shared` をグループ全体（複数行）に広げて 1 プランで消す。範囲がグループの一部だけを含む場合は、今どおり拒否する。
  - (b) 拒否を仕様として明記し、Q を立てる。

  `write_range` で shared グループを値で上書きする場合も、(a) と同じ「グループ丸ごとなら de-share」規則にするか拒否するかを決める。どちらの場合も、oracle 2/6 に「fill-down 数式の列を含む範囲」を足す。

### M4（Major）: Python の `None → CellWrite::Clear` のため、`read_range` の結果をそのまま `write_range` で書き戻すと、結合セルで拒否される。空いた座標には空セルが作られる

- **該当箇所:** §11.4 Write、§12 `write_cells`/`write_range`/`append_rows`。
- **根拠。**
  - `CellWrite::Clear` は「An absent target becomes a blank cell」（js `worksheet.rs:831-833`）。§6.2 自身も、大きな範囲では誤りだと書いている。
  - 結合範囲の非 anchor セルへの書き込みは、`Clear` であっても `MergedCell` で拒否される（js `edit.rs:521-523`。`style_only` 以外のすべての op に適用される）。テーブルのヘッダセルも同様である（同 525-527）。
  - §5.4 により、`read_range` は結合メンバーを `None` で返す。
- **なぜ重要か。** 「読んで、一部を直して、書き戻す」は RPA の基本パターンである。しかし結合を含む領域では、これが必ず `RefusedError` になる。また、長さの揃わない行を `None` で埋めると、空の `<c>` 要素が座標の数だけ増える。
- **修正案。**
  - `write_range`/`append_rows`/`write_cells` では、`None` を「その座標は変えない（スキップ）」にする。
  - 消したいときは、明示の番兵（`ooxml_rs.CLEAR`）か `clear()` を使う。
  - あるいは、`None` は「内容があるセルだけ `Clear`」（`clear_range` と同じ規則）にする。
  - どちらにするかを Q に立てる。
  - oracle に「結合を含む範囲の read → write 往復が `Ok` で、バイト差分が値の変わったセルだけ」を足す。

### M5（Major）: §9 で `ReadCell` 上に持ち上げる分類器は fail open になる

- **該当箇所:** §5.1 `ReadCell`、§5.6、§9。
- **根拠。**
  - **(a) 情報が足りない。** Wasm の分類器は `has_complex_value` を使う。これは `f`、`vm`、`cm` のいずれかがあれば「複合」とする（js `worksheet.rs:1360-1366`）。しかし `ReadCell` には次の情報が無い:
    - 数式の有無。`formula` は `FormulaRead::Text` のときしか入らない（§5.1、§5.3）。
    - `cm` 属性。
    - セルが保存されているかどうか。

    たとえば `FormulaRead::Omit` で読んだ数式セルは、`ReadCell::scalar` から見ると普通の数値である。§9 の「欠けていれば足す」は、この 3 つが今すでに欠けていることを書いていない。
  - **(b) 不完全な分類を見ない。** census が blocked のとき、§5.6 は role を `Plain` にし、`RangeRead.roles_complete` を `Err` にする。しかし `ReadCell::scalar(&self, opts)` は `roles_complete` を見ないので、array や spill のメンバーをスカラーとして返す。
    - 今の `classify_sheet` は `non_scalar_ranges` を通るので、census が blocked なら `Err` になる（js `worksheet.rs:1237-1239`）。つまり fail closed である。
    - ADR-0021 は、分類が不完全なら影響するシートのアクセスを拒否すると決めている（"Incomplete classification" 行、"Rust changes" の「fail closed」）。持ち上げで、これが後退する。
- **修正案。**
  - `ReadCell` に `has_formula: bool`（`Omit` でも必ず埋める）、`cell_metadata: bool`（`cm`）、`stored: bool` を常に持たせる。
  - 分類は `ReadCell` 単体ではなく `RangeRead` の上で行う（例: `RangeRead::scalar(i, opts) -> Result<Option<CellValue>>`）。`roles_complete` が `Err` なら、その `Err` を返す。または `ReadCell::scalar` を `pub(crate)` にとどめる。
  - U5 の受け入れ条件に、「census blocked のとき、`read_scalar` が Wasm 現行と同じ `Err` を返す」テストを足す。
  - 補足（Minor 相当）:
    - `read_scalar(at)` を呼ぶたびに範囲を作り直すと、Wasm が generation ごとにキャッシュしている `scalar_read_ranges`（js `lib.rs:5091`）より遅くなり、ADR-0022 の latency ゲートを割りうる。
    - `classify_cell` の `ranges.remove(index)`（js `lib.rs:5131-5136`）は、`non_scalar_ranges` の先頭が `merged_ranges` と同じ順で並ぶことに依存している（js `worksheet.rs:1240`）。持ち上げのときは、範囲の一致で除く形に直す。

### M6（Major）: `CellRole` は値を 1 つしか持てないが、実際の役割は重なる

- **該当箇所:** §5.1 `CellRole`、§5.4、§9。
- **根拠。** 結合と数式グループは独立した仕組みである。`non_scalar_ranges` も、`merged_ranges` に `formula_result_ranges` を足して作っている（js `worksheet.rs:1240, 1262`）。たとえば次の重なりがある:
  - 縦に結合した行に数式を fill すると、結合 anchor が同時に shared の master または member になる。
  - 結合範囲の中に array の anchor がある。
  - 結合メンバーが spill の範囲に入る。

  enum が 1 値だと、片方の情報が黙って落ちる。
- **なぜ重要か。** 次の 2 つが、落ちた側の情報を必要とする:
  - 呼び出し側の「fill merged」は、結合の情報を使う。
  - M5 の分類は、結合メンバーかどうかと数式グループかどうかの両方を使う。

  どちらを優先するかも ADR に書かれていない。
- **修正案。** `CellRole` を `#[non_exhaustive] struct CellRole { merge: Option<MergeRole>, formula: Option<FormulaRole> }` にする。
  - `MergeRole` は `Anchor(CellRange)` または `Member { anchor }`。
  - `FormulaRole` は Shared/Array の Master/Anchor/Member。

  oracle 6 に「結合 anchor かつ shared master」の fixture を足す。

### M7（Major）: `resolve_defined_name` の仕様（§7）と、流用するとしている `Env::resolve` の挙動が食い違う

- **該当箇所:** §7。
- **根拠。** js `formula/precedents.rs:110-136` の挙動は次のとおり:
  - `own: None`（名前定義の内側）で、同じ文字列のローカル名が 1 つでもあれば `None` を返す（132 行）。§7 は「`None` なら global 名だけが一致する」と書いている。この差により、`Rate` が global とどこかのシートのローカルの両方にあるブックで、`resolve_defined_name("Rate", None)` が `NoSuchName` になる。
  - 同じ fold の名前に `Scope::Invalid`（範囲外の `localSheetId`、js `workbook.rs:828-829`）が 1 つでもあれば、すべて解決不能になる（113 行）。
  - 曖昧な場合も不在の場合も同じ `None` を返す。

  `Env::resolve` は依存解析のための保守的な規則である。解決できない場合は依存先を「全部」にする前提で作られていて、API の名前解決とは目的が違う。
- **修正案。** `fold` と `NameDef.expr`（パース済みの式）だけを共有し、§7 の規則で新しく小さな解決関数を書く。§7 の「not a second implementation」は撤回する。§7 の規則とは次の 3 点である:
  - `scope` で指定したシートのローカル名が global 名より優先する。
  - `None` なら global 名だけを見る。
  - 同じ scope に fold して同じ名前が 2 つあれば曖昧とする。

  あわせて次の 2 点も決める:
  - 曖昧な場合を `NoSuchName` とは別の `reason` にするか（例: `AmbiguousName`）。
  - `Scope::Invalid` の名前の扱い。

  oracle 8 に「global とローカルに同名があり、scope=None」と「範囲外の `localSheetId`」を足す。

### M8（Major）: 日付の型付けが、経過時間の書式と時刻だけの値を誤って扱う

- **該当箇所:** §5.2、§6.1、§11.4、Q4、Q5。
- **根拠（読み取り）。** `is_date_format` は、`[h]`/`[mm]`/`[ss]` を含む経過時間の書式も日付と判定する（main `registry/format.rs:733-738`。テスト 755 行で `"[h]:mm:ss"` が日付）。§5.2 により、こうした値は `DateTime` になる。
  - 例: 勤務時間 25 時間（serial 1.041…）は `datetime(1900, 1, 1, 1, 0)` として返る。
  - 24 時間未満の値は Q5 で `datetime.time` になる。このため同じ列に `time` と `datetime` が混ざる。
  - いずれも値の意味が変わる誤りで、拒否ではない。
- **根拠（書き込み）。** `datetime.time` は `DateTime` で書かれ、書式は `EnsureDate` により組み込み 14（日付）になる（main `setters.rs:226-233`）。そのため、時刻だけのセルが Excel では `1/0/1900` と表示される。Q4 の代案（22）でも、`1/0/1900 12:00` になるだけである。
- **修正案。** Rust 側で書式を次の 4 種に分類する（`DateKind`）: 日付、時刻、日時、経過時間。
  - 経過時間の書式は `ReadValue::Number`（または新しい `Duration`）とし、Python では `timedelta` にする。
  - Q5 は serial の範囲ではなく、書式の種類で決める。
  - 書き込みでは、値の種類で `EnsureDate` の既定書式を選ぶ。時刻だけなら組み込み 21（`h:mm:ss`）か 20、日付だけなら 14、時刻つきなら Q4 の決定に従う。
  - oracle 4 に `[h]:mm` と時刻だけの書き込みを足す。

### M9（Major）: PyO3 crate をワークスペースに入れたときの CI と開発環境への影響が、事実と違う前提で見積もられている

- **該当箇所:** §10「Workspace consequence」、Q3、Consequences。
- **根拠。**
  - §10 は「CI already has one (the python-pptx oracle)」と書いている。しかし `cargo test --workspace --all-features` を走らせる job（`ci.yml:13-23`）は `setup-python` を使っていない。Python があるのは別の job（`ci.yml:37-47`）だけである。
  - `extension-module` を付けずに `ooxml-py` の lib テストや doctest のバイナリを作ると、libpython の共有ライブラリへのリンクが要る。`--all-features` で走る `cargo test --workspace` は、これを作る。
  - 影響を受けるのは CI だけではない。Codex のサンドボックスや貢献者の手元も、AGENTS.md の完了条件（3 コマンド）を満たせなくなりうる。ADR はこれを Q3 に回しているが、検証の根拠が無い。
- **修正案。** ADR で決めてから Accepted にする。
  - (a) `crates/ooxml-py` をワークスペースから外す（`exclude`）。独立の `Cargo.toml` と CI job を持たせ、Rust 側の変更でも必ず走るように path filter なしで回す。
  - (b) ワークスペースに残すなら、`[lib] test = false, doctest = false` にし、Rust 側のテストを置かない。そのうえで、clippy と check がインタプリタなしで通る設定（abi3 の設定や `PYO3_NO_PYTHON` 等）を、U7 の spike で CI 上で実証する。

  どちらでも、AGENTS.md の完了条件に pytest job を足す。U7 の受け入れ条件に「Linux と Windows の素の runner で `cargo test --workspace --all-features` が通る（ログを PR に添付）」を入れる。

### m1（Minor）: §3 の `Stored` の定義は、`stored_extent` の実装と一致しない

- **根拠。** js `worksheet.rs:259-275` の実装は次のとおりで、§3 の「None when … no stored row or cell」と食い違う:
  - セルが 1 つも無ければ `None` を返す。セルの無い `row` 要素しか無くても `None` である。
  - 列の範囲はセルだけから取る。行の範囲は全 `row` から取る。
- **修正案。** §3 と oracle 7 を実装に合わせるか、`used_range(Stored)` を `stored_extent` とは別に定義する。

### m2（Minor）: `first_free_row(None)` は結合を数えないので、追記で結合メンバーの行を提案しうる

- **根拠。** §3 では、結合は `Values` の外接矩形を広げない。§4 で結合を数えるのは `Some(c)` のときだけである。
- **壊れる例。** `B10:B12` が結合で、B10 に値がある。`append_rows(key_column=None)` は 11 行目を提案し、B11 への書き込みが `MergedCell` になる。
- **修正案。** `None` の場合も、内容のある anchor の結合は全行を使用済みとして数える。

### m3（Minor）: §5.3 の数式テキストに、決まっていない場合がある

- **根拠。**
  - **`shared_formula_at` が `None` を返す member。** `ambiguous_group` や master の無い member がこれに当たる（js `worksheet.rs:1207-1217`）。この場合の扱いが書かれていないので、実装は member 自身の空の `<f>` を返しかねない。その結果、`formulas="formula"` で `"="` が返る。
  - **1 セルの失敗で全体が失敗する。** 翻訳できない member が 1 つあると、`read_range` 全体が `Err` になる。
  - **census が blocked の場合（§5.6）。** 数式テキストの扱いが決まっていない。
- **修正案。** セル単位で表す（例: `formula: FormulaText::{None, Text(String), Unavailable(&'static str)}`）。範囲全体は失敗させない。

### m4（Minor）: 1 座標あたりのメモリが大きく、`max_cells` の既定 10,000,000 では RPA ホストで OOM になりうる

- **根拠。** `ReadCell` は `ReadValue`（約 32B）、`Option<String>`（24B）、`CellRole`（約 24B）、`u32` と bool 3 つを持ち、およそ 90B になる。文字列の heap は別にかかる。1 千万座標なら Rust 側だけで約 1GB、Python オブジェクトにすると数 GB である。
- **修正案。** Q9 にこの見積もりを書き、既定を 1,000,000 にするか、doc に明記する。または、role と数式を疎な別表にする（Blank の座標は値だけを持つ）。

### m5（Minor）: §8 の 2 つの記述に誤りがある

- **ADR-0021 のコードとの関係。** 「same strings are the ADR-0021 documented codes」は事実ではない。js の `packages/` が使うコードは `INVALID_VALUE`、`CELL_EDIT_REFUSED`、`WORKSHEET_READ_REFUSED` などの別の語彙である。ADR-0022 も `INVALID_INPUT` を使う。
- **Debug 接頭辞のテスト。** 「`format!("{e:?}")` が `e.kind()` で始まる」というテストは、`Core(e)` では成り立たない。Debug は `Core(Io(..))` になるのに、`kind()` は `"Io"` を返す。
- **修正案。** 前者は「対応表を adapter が持つ」に改める。後者は `Core` の分岐を除外するか、内側の Debug と比べるテストにする。

### m6（Minor）: Rust API の名前と、コストについての記述に誤りがある

- **根拠。**
  - `Workbook::shared_strings` という公開メソッドは無い。あるのは `pub(crate) shared_strings: Option<Vec<String>>`（js `workbook.rs:193`）と `shared_string_items`（js `workbook.rs:638`、`pub(crate)`）である。
  - §3 は「s index は文字列をコピーせずに検査する」と書いているが、`CellRef::value()` は文字列を `clone` する（js `worksheet.rs:160-161`）。`value()` で「内容あり」を定義すると、このコスト主張は成り立たない。
- **修正案。** 名前を直す。index だけを見る `pub(crate)` の判定を使うと書く。

### m7（Minor）: `Workbook::worksheet(SheetRef)` が公開されていない

- **根拠。** ADR-0018 の行列操作は `Worksheet` のメソッドである（main `sheet_ops/grid.rs:49-80`）。シート操作は `&str` か位置を取る。バインディングがこれらを §2 の規則で呼ぶには、`SheetRef` から `Worksheet` を得る公開の入口が要る。
- **修正案。** U2 に `Workbook::worksheet(SheetRef) -> Result<Worksheet<'_>>` を足す。

### m8（Minor）: パッケージングで、検証されていない前提が 3 つある

- **モジュール名。** cdylib の lib 名 `ooxml_rs` と、純 Python のパッケージ `ooxml_rs` を両立させるには、maturin の `module-name = "ooxml_rs._native"` 等を明記する必要がある。
- **abi3 と datetime。** abi3（limited API）で `datetime` の変換が使えるかは、PyO3 の版による。U7 で確かめる。
- **fault-injection。** oracle 10 の注入には、`ooxml-py` に `ooxml-xlsx/fault-injection` を転送する feature と、Python から呼べるフックが要る。また、この feature を付けた wheel を別にビルドする必要がある。

### m9（Minor）: `save` を「`to_bytes` の後に一時ファイル＋rename」とすると、既存の実装と重複し、ピークメモリが倍になる

- **根拠。** `Workbook::save` は poison を検査する（main `workbook.rs:494-497`）。その先の `Package::save` は、ストリームで一時ファイルに書いて rename し、失敗時は一時ファイルを消す（`package.rs:600-622`）。
- **修正案。** `Workbook::save` をそのまま呼ぶ。

### m10（Minor）: 型の一貫性と semver

- **根拠。**
  - 型の不一致: `DefinedName.local_sheet: Option<usize>` に対し、setter は `Option<u32>`（js `sheet_ops/mod.rs:226`）である。
  - semver: `CellWrite` は `#[non_exhaustive]` でない（main `worksheet.rs:633`）。js と本 ADR が variant を足し続けるので、外部の網羅的な `match` が毎回壊れる。
  - 公開フィールドの型: `roles_complete: Result<(), &'static str>` を公開フィールドに置くのは不自然である。
- **修正案。** 型を揃え、U2 で `CellWrite` に `#[non_exhaustive]` を付け、`roles_complete` は `Option<&'static str>` にする。

### m11（Minor）: テストしにくい受け入れ条件がある

- **根拠。** oracle 5 の「LibreOffice's expansion」は CI に無い。
- **修正案。** spike で一度だけ期待値を生成し、LibreOffice の版を固定して commit する。

### m12（Minor）: 公開 ADR の文言

- **リポジトリ外の資料。** 「(gap analysis)」（l.787）は、リポジトリに無い資料を指している。
- **エージェント運用の語。** 「the js-compat author」「that session」（l.125, 397, 461, 648, 655, 657, 729, 816）はエージェント運用の語である。「the ADR-0021 implementer」に統一する。Q7 の実装者の話は、PR 本文の方がよい。
- **非公開情報。** 漏れは無い。

### m13（Minor）: Python 側の規約

- **根拠。**
  - シートの `int` は 0 始まり、行と列は 1 始まりで、規約が混ざる。
  - `resolve_name(scope=None)` は global 名しか見ない。PAD の「名前付きセル」がローカル名に当たる場合の扱いが書かれていない。
- **修正案。** 両方を §11.4 に明記し、後者は Q にする。

## 所有者への質問について

| Q | 判断 |
|---|---|
| Q1（数式の既定） | 推奨どおり `"value"` でよい。 |
| Q2（版・配布） | 推奨どおりでよい。abi3 と datetime の可否は U7 で確かめる（m8）。 |
| Q3（ワークスペース内か） | **Q のままにしない。** ADR で決め、CI 上の実証を受け入れ条件にする（M9）。 |
| Q4（日時の書式） | **広げる。** 時刻だけの値と経過時間を含めて決める（M8）。 |
| Q5（`[0,1)` の serial） | **変更する。** serial の範囲ではなく、書式の種類で決める（M8）。 |
| Q6（used range の既定） | 推奨どおりでよい。`first_free_row(None)` の結合の扱いを直す（m2）。 |
| Q7（実装者） | owner の判断。ADR ではなく PR 本文に書く（m12）。 |
| Q8（`=` で始まる文字列） | 推奨どおりでよい。 |
| Q9（`max_cells`） | メモリの見積もりを書いて判断を仰ぐ（m4）。 |
| Q10（`defined_name_nodes`） | 推奨どおりでよい。 |
| 追加 | `None` を書くときの意味（M4）。`clear_range` と shared グループ（M3）。トランザクション中の他スレッド（M1）。 |

## 判定

**changes required.** converged: no（Blocker 0 / Major 9 / Minor 13）。
