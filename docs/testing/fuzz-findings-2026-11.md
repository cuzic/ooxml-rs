# ファジング所見（2026-11）

対象: main @c4116e9（PR #16 depth-limit マージ後）の、信頼できない入力を受ける面。

- ooxml-core: `Package::from_bytes` / `to_bytes`、`XmlDocument::parse` / `serialize` / `force_rerender`、`LensMut` の書き込みプリミティブ
- ooxml-xlsx: `Workbook::from_bytes`、セルの読み取り、`set_number` / `set_string`、保存と再オープン
- ooxml-pptx: `Presentation::from_bytes`、全スライドの読み取り、テキスト編集（本文・段落・ラン・表セル）、保存と再オープン

ooxml-docx（#13 で main にマージ）は積み上げの PR（`test/fuzz-docx`）で `docx_edit` ターゲットを追加した（下の「ooxml-docx」の節）。
xlsx Phase1（#20）は main に入っており、`xlsx_edit` の oracle は Phase1 の契約に合わせてある（`phase1_oracle`）。
数式を書き換える編集のターゲットは、さらに積み上げの PR で追加する。`Package` の書き込み API（`set_part`、#14 の `remove_part`）の
「Err なら無変更」は、まだ検査していない。

ハーネスは `crates/ooxml-robustness`（publish しない dev 専用 crate）、cargo-fuzz のターゲットは `fuzz/`（ワークスペース外、nightly）。
各ターゲットの不変条件は `fuzz/README.md` の表にある。crate のソースは変更していない。修正は各 crate の担当者に任せる。

## 結論（先に）

**不変条件の違反が 7 件ある。High 2・Medium 3・Low 2、ほかに Info 2。** panic は見つからなかったが、**abort はある**:
F-6 の ZIP64 の形（459 バイトの zip）で `Package::from_bytes` がプロセスを abort させる（Opus レビュー round 1 の M1 で指摘、再現済み）。

| ID | 重大度 | 概要 | 再現テスト |
|---|---|---|---|
| F-1 | **High** | 先頭に UTF-8 BOM が 2 つある part は要素の span が 3 バイトずれる。編集すると壊れた XML を書く（xlsx: `set_number` が Ok で、保存したファイルが開けない） | `f1_double_bom_edit_writes_well_formed_xml`、`f1_double_bom_xlsx_write_saves_a_workbook_that_reopens` |
| F-2 | Medium | データがアーカイブ末尾を越える zip エントリを受け付け、無編集の保存でそのエントリを別のバイト列にして書く（Layer 1 違反、Err にならない） | `f2_entry_past_end_of_archive_is_refused_or_copied_unchanged` |
| F-3 | Medium | `LensMut::set_attr` / `remove_attr` が `xmlns:*` 宣言を拒否しない。使用中の宣言の削除で非 well-formed、再束縛で既存の名前の名前空間が黙って変わる | `f3_lens_writes_to_namespace_declarations_keep_output_meaning` |
| F-6 | **High** | part の読み取りが**申告された**非圧縮サイズで先にメモリを確保する。ZIP64 では申告サイズに上限がなく、459 バイトの zip で **open 時にプロセスが abort する**（`catch_unwind` でも止められない）。32 ビットの申告では約 4 GB を要求し、libFuzzer では OOM になる | `f6_zip64_declared_size_does_not_abort`（子プロセスで実行）、`f6_declared_size_does_not_drive_allocation` |
| F-7 | Medium | `Package::from_bytes` の重複名検査が O(n²)。空エントリ 65,535 個・6 MB の zip で open に 30 秒（release）。**PR #14 で修正済み**（マージ後に `#[ignore]` を外す） | `f7_open_is_not_quadratic_in_the_entry_count` |
| F-4 | Low | ルート要素の中の XML 宣言（`<?xml …?>`）を受け付け、木から落とす。再描画で消え、前後のテキストが 2 項目に分かれる（T1b 違反） | `f4_xml_declaration_inside_content_is_refused_or_kept` |
| F-5 | Low | 名前が空の要素（`< />`、`<\r/>`）を受け付ける。再描画で `</>` を書き、どのパーサーも読めない | `f5_empty_element_name_is_refused` |
| I-1 | Info | パーサーがほかにも XML 1.0 / Namespaces の非 well-formed 入力を受け付ける（下記） | — |
| I-2 | Info | 展開後サイズや圧縮率の上限がない（zip bomb） | — |

**状況（2026-11-16 時点）: F-1〜F-6 は PR #40、F-7 は PR #14 で修正され、どちらも main にマージ済み。**
再現テストは `crates/ooxml-robustness/tests/fuzz_findings.rs` にあり、PR #36 round 2 で `#[ignore]` を外した。
現在は 9 件すべてが通常の回帰テストとして CI で pass する。既知の所見を除外していた `known.rs` のフィルタも削除した。
以下は修正前の記録である。修正前は、どの再現テストも `#[ignore]` 付きだった。
`cargo test -p ooxml-robustness --test fuzz_findings -- --ignored` で 9 件すべてが失敗することを Rust 1.99.1 で確認した
（ZIP64 のテストは子プロセスが SIGABRT で落ちたことを報告する）。どのテストも、パッチ前の入力で同じ操作が成功する対照を先に検査するので、
無関係な理由の拒否で黙って pass することはない。PR #40（core の修正）の上では F-7 以外の 8 件が pass する。
当時は、既知の形の入力だけを `crates/ooxml-robustness/src/known.rs` のフィルタで飛ばしていた。ファザーが同じ所見を何度も報告しないようにするためで、`OOXML_FUZZ_ALL=1` で無効にできた。
修正後に、フィルタを無効にした状態（`OOXML_FUZZ_ALL=1`、`OOXML_SMOKE_ITERS=20`）でもスモークが通ることを確認してから削除した。

xlsx・pptx の format crate 固有の不変条件（拒否なら無変更、成功なら読み戻し一致、他 part の L1、再オープン）の違反は見つからなかった。
F-1 は xlsx 経由でも再現する（core の問題が format crate に波及する）。

## 実行内容

環境: 32 コア、rustc nightly 1.102.0（5f4effd57 2026-11-13）、cargo-fuzz 0.13.2、libFuzzer（ASan、debug assertion 付き）。
すべて `-fork=N -timeout=10 -rss_limit_mb=2048`。シードは `reference/fixtures/**` と `reference/round_trip_corpus` から
`examples/seed_corpus.rs` で実行時に作った（コミットしない）: パッケージ 74 個、XML part 1,202 個、xlsx 入力 340 個、pptx 入力 971 個、lens 48 個。

| ターゲット | 1 回目（フィルタなし） | 2 回目（`known.rs` のフィルタあり） | 2 回目の新規の失敗 |
|---|---|---|---|
| `xml_t1` | 約 10 分、3.3M 実行 | **40 分**（5 並列）、13.0M 実行、cov 2,103 | 0 |
| `xml_t1b` | 約 10 分、5.4M 実行 | **40 分**（5 並列）、6.5M 実行、cov 2,121 | 0 |
| `lens_ops` | 約 6 分、0.75M 実行 | **20 分**（4 並列）、2.8M 実行、cov 3,013 | 0 |
| `package_roundtrip` | 約 7 分、0.15M 実行 | **20 分**（5 並列）+ **15 分**（6 並列）、0.42M 実行、cov 5,872 | OOM 2 件（どちらも F-6） |
| `xlsx_edit` | 約 6 分、87k 実行 | **20 分**（5 並列）、231k 実行、cov 7,100 | 0 |
| `pptx_edit` | 約 6 分、18k 実行 | **20 分**（5 並列）+ **15 分**（10 並列）、133k 実行、cov 6,882 | 0 |

1 回目で見つかった失敗（lens 638 件、xml_t1b 316 件、package 30 件、xml_t1 6 件。`examples/replay.rs` で仕分け、`cargo fuzz tmin` で最小化）は
すべて F-1〜F-6 のどれかだった。1 回目の xlsx / pptx のログにある crash カウントは、実行中にバイナリを再ビルドしたため
fork したジョブが起動できなかった分で、成果物は 0 件（入力起因の失敗ではない）。
2 回目の開始直後にできた `crash-*` 2 件は 1 回目の残りプロセスが書いたもので、現在のハーネスでは再現しない（F-2 / F-4 の形）。
F-7 はファザーの入力サイズでは届かないので、F-2 の調査中に手で測った。

stable のスモーク（`tests/smoke.rs`、決定的な変異）: debug・既定値（`OOXML_SMOKE_ITERS=1`）でテスト実行約 4 秒（32 コア、ビルド時間を除く。ファザーと並行した初回はビルド込みで約 12 秒）、
release・`OOXML_SMOKE_ITERS=30` で 27 秒、どちらも失敗なし。

### Opus レビュー round 1 の後の変更

- 再現テスト: ZIP64 の F-6（子プロセスで実行）を追加した。F-1・F-2・F-6 にはパッチ前の入力で同じ操作が成功する対照を加えた。
  F-7 は 3 回測って最小値の比で判定する。
- フィルタを所見の形に絞った: F-4 はルート要素の開始タグより後の宣言だけを除外する。`check_package` は、BOM が 2 つある part があっても
  `[Content_Types].xml` / `_rels/.rels` でなければ L1 検査を続け、その part を parse と再描画の対象から外すだけにした。
- oracle を強めた: `assert_l1_except` は、Package が開けたのに raw で読めない入力を失敗として扱う（以前は検査せずに return していた）。
  pptx は保存・再オープンの前後で全 text body の `text()` が一致することを検査する（kind 1〜8 の編集にも oracle が付く）。
- seed: ZIP64・data descriptor・前置データ・壊れた CFB シグネチャ・part だけ Strict の変種を合成する（`package_roundtrip`）。
- ワークフロー: `timeout-minutes`、`permissions: contents: read`、dispatch 入力は `env:` 経由で数値か検査、nightly-2026-11-13 と
  cargo-fuzz 0.13.2 に固定、`fuzz/Cargo.lock` をコミット、seed を 1 ターゲット 64 件に間引く。
  `-max_total_time` は初期コーパスを一巡した後から数える点を README に書いた。
- 変更後の確認: 6 ターゲットを各 5 分（4 並列、合成 seed を追加したコーパス）回して新しい失敗はなかった（xml_t1 92 万回、xml_t1b 41 万回、lens 67 万回、
  xlsx 4.4 万回、package 8 千回。pptx は初期コーパスの一巡で時間を使い切った）。stable スモークも通る。

### Opus レビュー round 2 の後の変更

- origin/main をマージした（#14・#19・#20・#40 などを含む）。
  - `ShapePath` の API が変わったため、`pptx.rs` の比較を直した。
  - xlsx Phase1（#20、ADR-0003）では、値を編集すると他のシートにある依存セルのキャッシュも消える。
    - あわせて `workbook.xml` の `calcPr` が書き換わる。
    - 数式セルの集合が変わると、`calcChain.xml` も削除される。
    - 当初は、編集後の L1 検査の除外対象を広げて対応した。対象は全シート（content type で判定）、workbook、`[Content_Types].xml`、`.rels`、それに消えてもよい `calcChain.xml` だった。
      round 3 のレビュー（M2）で、これでは Phase1 の契約外の書き込みを見逃すと指摘された。7 つの変異のうち 5 つが生き残っていた。
      そこで、`src/phase1_oracle.rs` の厳しい oracle に置き換えた。書き込みのたびに直前の `Workbook::dependents` と「数式セルの集合が変わるか」を記録し、変わった part ごとに次だけを許す。
      - 依存セルを持つシート: 依存セルの `v`・`is`・`@t` だけ
      - workbook: `calcPr/@fullCalcOnLoad="1"` だけ（依存があったか集合が変わったときに限る）
      - `[Content_Types].xml` と workbook の `.rels`: calcChain の `Override`・`Relationship` がなくなることだけ（calcChain を消したときに限る）
      - calcChain（workbook の relationship から解決する）: 集合が変わったときに限って消える
      - それ以外: L1 一致
      calcPr に `iterate` を書く変異、calcChain を残す変異・常に消す変異、依存セルの `f` まで消す変異は、どれもこの oracle で検出されることを確認した。
- 9 件の `#[ignore]` を外し、`known.rs` を削除した。
- ZIP64 の子プロセスには `--include-ignored` を渡す。親は子の出力に `1 passed` があることを検査する。これで、テストが 0 件しか実行されなかった場合に空振りで pass することがなくなる（M1'）。
- ワークフロー:
  - `cargo metadata --locked` で `fuzz/Cargo.lock` が最新であることを強制する。
  - nightly を `nightly-2026-11-14` にした。所見の実行に使った `5f4effd57 2026-11-13` が入っているチャンネル。

## ooxml-docx（`docx_edit`、PR #36 に積む PR で追加）

対象: ooxml-docx Phase0（ADR-0004）。当初は main @8c54366 に対して実行し、現在は main @4f8e294 にマージして再確認した。`Document::from_bytes`、書き込みゲート（`write_gate` / `write_gates`）、
全段落・全ランの読み取り（`text`、`context`、`in_field`、`check_settable`）、`set_run_text`、`append_paragraph`、保存と再オープン。
入力は xlsx / pptx と同じ形（fixture 1 個の part 1 個を payload で置き換え、続くバイトで操作を選ぶ）。fixture は
`reference/fixtures/docx` の 26 ファイルで、XXE の検査用ファイル（`poi-ExternalEntityInText.docx`）も seed に入れた（DTD は open で拒否される）。

不変条件:

- **結果の予測**（PR #47 round 1 の M1）: 各編集の前に、ooxml-docx の公開 API とハーネス側の判定だけを使って、成功するか、どのエラーになるかを予測する。実際の結果は予測と一致しなければならない。
  - `set_run_text` の予測は、次の順で決まる。
    1. 書き込みゲート
    2. 段落とランの存在
    3. `check_settable`
    4. 書けない文字
  - `append_paragraph` の予測は、次の順で決まる。
    1. 書き込みゲート
    2. 書けない文字
    3. `w:body` の有無
    4. 本文の終わりで閉じていないフィールド（各ランの `w:fldChar` を数え直して判定する）
    5. A2
  - 予測できない結果は、lens が拒否する `mc:AlternateContent` の兄弟（`Core(UnresolvedSibling)`）だけである。
  - これにより「拒否すべきなのに Ok」と「過剰な拒否」の両方を検出できる。
- 拒否（Err）なら、パッケージのバイト列は変わらない。
- 成功した `set_run_text` は読み戻せる（`\r\n` / `\r` は `\n`）。`document.xml` は、そのランの元の span の中だけが変わる（T2）。
  さらに、ランの属性と `w:rPr` は変わらず、端や連続の空白を含む `w:t` には `xml:space="preserve"` が付く（m1）。
- 成功した `append_paragraph` は新しい最後の段落の番号を返し、読み戻せる。`document.xml` は 1 か所への挿入だけで変わる
  （`w:body` が自己終了タグだった場合は `w:body` の span の中）。挿入点は `w:body` の中で、本文直下に `w:sectPr` があればその前でなければならない（m2）。
- 編集が成功するたびに保存して開き直す。そのため、次の編集の T2 検査は現在のバイト列の span で行い、途中の出力もすべて再オープンを検査する。
- 最後に、main part 以外は L1 一致で、再オープンした文書でも全編集が読み戻せる。

`set_run_text` の対象は、半分の確率で `check_settable` が通るランから選ぶ（ゲートが掛かっていない fixture 19 個中 12 個に設定可能なランがある）。
テキストは半分の確率で、書ける文字（英字、空白、タブ、改行、非 ASCII）だけから作る（m3。以前は約 4 割が `InvalidXmlChar` で終わっていた）。
次の形はランダムな変異ではまず生じないので、`docx::targeted_inputs()` で作り、seed とスモークの両方に入れている。
- A2（最後の段落がセクション区切りを持つ）
- 本文の終わりで閉じていないフィールド
- `xml:space` が欠けた連続空白のラン
- 自己終了の `w:body`

レビューで生き残った ooxml-docx の変異 8 個（ゲートの削除 ×2、`check_settable` の削除、A2 とフィールド検査の無効化、`xml:space` を付けない、`w:rPr` を消す、no-op 判定の誤り）は、
いずれもスモーク（`OOXML_SMOKE_ITERS=30`）で検出されるようになった。

**round 1 対応後の再実行**（main マージ後、#40 / #14 / #20 入り）:

| 実行 | 時間 | 回数 | cov | 新規 |
|---|---|---|---|---|
| 1 回目（16 並列、seed 394 件。うち合成 7 件） | 15 分 | 13.5 万 | 7,260 | timeout 3 件（下記） |
| 2 回目（12 並列） | 6 分 | 3.7 万 | 7,283 | 0 |

- **timeout 3 件**: どれも 1.35 MB の fixture（`poi-bug59058.docx`）での、ハーネス側の負荷だった。
  - 原因: append の予測と位置の検査が main part を毎回 parse し、編集のたびに再オープンしていた。
  - release では 0.5 秒で終わる。ASan では単独で 5 秒前後、16 並列の負荷下では 10 秒を超えた。
  - 対策（PR #47 round 2 の m5 の後の値）: parse を 1 回にまとめた。main part が 512 KiB を超える文書では、編集を 1 回にし、読み取り後の 2 回目の保存と L1 比較も飛ばす。
    これで、この fixture の seed は ASan で 2.8〜3.6 秒になった（以前は負荷のない状態で 6〜12 秒）。
- **所見**: なし。

**分類そのものの検査（m6）**: 予測は ooxml-docx の分類用の公開 API（`check_settable`、`write_gate`、`in_field`）をそのまま使う。
そのため、「分類どおりに拒否・許可しているか」は検査できるが、分類そのものの誤りは検査できない。
このうちフィールドについては、`read_all` で各ランの `w:fldChar` を独立に数え直す。そして `in_field()` が「直前の深さ > 0、直後の深さ > 0、またはそのランが `w:fldChar` を持つ」と一致することを assert する。
変更履歴の wrapper とゲートの検出（settings.xml）が正しいかどうかは、ooxml-docx の単体テストの範囲とする。
ファザーが見るのは、分類と結果が一貫しているかどうかである。

**結果: 所見なし。** 7 分（12 並列、53k 回）と 25 分（16 並列、19.4 万回、cov 6,558）回し、stable スモーク（release、`OOXML_SMOKE_ITERS=100`）も通った。
1 回目に出た timeout 1 件はハーネスの負荷だった。最大の fixture（`poi-bug59058.docx`、document.xml 1.35 MB）で 12 回編集し、
毎回開き直すと、release で 1.2 秒、ASan では 10 秒を超える。ライブラリ側はどれも速い（open 34 ms、全ランの `check_settable` 0.6 ms、
`append_paragraph` 0.8 ms、保存 2 ms）。この fixture の扱いの最終的な値は、上の「対策」の項を参照。

## ooxml-xlsx Phase1（`xlsx_formula_edit`、PR #47 に積む PR で追加）

対象: main の ooxml-xlsx Phase1（#20、ADR-0003）。数式を含むブックで、任意のワークシートに対して次の 4 種類の編集を行う。

- `set_number`
- `set_string`
- `set_formula`
- `remove_formula`

対象セルは半分以上の確率で、数式セルか、どれかの数式が参照しているセルから選ぶ。共有数式のマスター、配列の結果、シート間の参照もここに含まれる。

**独立した依存モデル**: ハーネスは各シートの XML を直接読み、数式の参照を自前で抜き出して、依存関係を推移的に閉じる。

- 抜き出す参照:
  - 素の A1 参照と範囲
  - 行全体・列全体
  - シート名つき参照（引用符つき、大文字小文字を区別しない）
  - 共有数式のメンバー（マスターの式を、相対部分だけずらして使う）
  - 配列の結果範囲
- 抜き出さないもの（どれも辺を作らない）:
  - 名前、3-D 参照、構造化参照、外部参照
  - 関数の中にある文字列
- ここで見つかる辺はすべて本当の参照なので、モデルの依存先は、ライブラリが無効化すべきセルの**部分集合**になる。ライブラリは読めないものを過大に見積もる設計である。

**検査**:

1. `Workbook::dependents` は、モデルの依存先をすべて含む。
2. 編集が成功した後は、次のすべてが成り立つ。
   - モデルの依存先に `<v>` が残らない。
   - 依存先があるか数式セルの集合が変わった場合、`calcPr/@fullCalcOnLoad` が立つ（手動計算モードを除く）。
   - `calcChain.xml`（workbook の relationship で特定する）は、数式セルの集合が変わったときに限って、Override とともに消える。
   - 書いた値が読み戻せる。
   - workbook、registry、数式を持つシートと編集したシート以外は L1 一致。
   - 数式セルの集合がモデルの予想と一致する。
3. 拒否（`Err`）なら、パッケージは変わらない。`Poisoned` の後は、以後のすべての編集と保存が `Poisoned` になる。
4. 保存して開き直しても、拒否理由と数式セルの集合は変わらない。

**オラクルが本当に検出できるか**: ooxml-xlsx に変異を 3 つ入れて確認した（スモーク、`OOXML_SMOKE_ITERS=20`）。どれも検出された。

- 依存先のキャッシュを消さない
- `set_formula` で集合の変化を見ない
- `fullCalcOnLoad` を集合の変化でしか立てない

スモーク 200 回の内訳は、成功した編集 14,493、確認した依存先 9,405、calcChain の判定 6,726、拒否 5,873 だった。

**ハーネス側の誤り 2 件を、所見ではないと判定した**:

- part の名前が `calcChain.xml` でも、relationship から参照されていなければ計算チェーンではない。そこで、計算チェーンは relationship で特定するようにした。
- 壊れた `[Content_Types].xml` には、`calcChain` という文字列が別の PartName に残りうる。そこで、Override の有無は PartName で判定するようにした。

<!-- XF-RESULTS -->

## 所見の詳細

### F-1 [High] 先頭の BOM が 2 つあると span がずれ、編集で壊れた XML を書く

- **再現**: `XmlDocument::parse("\u{feff}\u{feff}<p xmlns=\"urn:t\"><item/>x</p>")` は Ok。`<item/>` の span は
  `start: 9`（正しい）だが `open_end`・`close` などは 3 小さい。`insert::<Ext>()` の後の `serialize()` は
  `"\u{feff}\u{feff}<p xmlns=\"urn:<itex<ext/>/>x</p>"` になる。
  xlsx: `poi/51850.xlsx` など 14 個の fixture（書き込みが拒否されなかったもの全部） で、worksheet の先頭に BOM を 2 つ足すと `set_number(7.0)` が Ok を返し、
  保存したファイルの再オープンが `XML error … attribute key must be directly followed by '=' or space` で失敗した。
- **影響**: 無編集なら T1 は成り立つ（prolog・ルートの span・epilog を連結すると入力全体になるため）ので、ゼロ編集の検査では見えない。
  dirty な要素があると、ずれた範囲を verbatim で書き出すので**黙って壊れたファイルを保存する**。ADR-0001 の「壊れたファイルを黙って作らない」に反する。
- **推定原因**: `xml/parse.rs:80` は最初の BOM だけを `base = 3` で読み飛ばし、`Reader::from_str(&src[base..])` に渡す。
  quick-xml は入力先頭の BOM を自分でも読み飛ばすが、その 3 バイトを `buffer_position()` に数えない。
  そのため `before` / `after`（= `base + buffer_position()`）が 3 小さい。`substr_range` から計算する `start` だけが正しく、
  両者の一致は `parse.rs:221` の `debug_assert_eq!(start, before)` でしか検査していない（release では無検査）。
  cargo-fuzz は debug assertion 付きでビルドするので、この assert で検出された。
- **修正の方向**: 2 つ目の BOM を拒否する（`UnsupportedEncoding` か `Xml`）か、quick-xml に BOM を見せないようにする。
  あわせて `debug_assert` を通常の検査（不一致なら `Err`）にするとよい。

### F-2 [Medium] アーカイブ末尾を越えるエントリを受け付け、無編集の保存で別のバイト列にする

- **再現**: stored エントリ 2 個の zip で、`a.bin` の中央ディレクトリの compressed size を `0x7FFF0000` に書き換える。
  `Package::from_bytes` も `to_bytes` も Ok。出力の `a.bin` は、ヘッダの compressed size が `0x7FFF0000` のまま、データ領域に
  入力の残り全部（データ 10 バイト + 中央ディレクトリ 116 バイト + EOCD 22 バイト = 148 バイト）がコピーされる。
  （raw で読む oracle では、入力側 148 バイト・出力側 286 バイトの「データ」に見える。どちらもファイル末尾まで読むため。）
  ファザーでは、ローカルヘッダの extra 長などでデータ開始位置が末尾を越える形も見つかった（入力側の生データ 0 バイト、出力側 715 バイト）。
  検出したのは 20 件で、すべてこの 2 つの形のどちらかだった。
- **影響**: README の「`Raw` は (name, method, CRC-32, compressed bytes) unchanged」が成り立たない。
  壊れた入力から、別の形で壊れた出力を Err なしに作る。`read_part` は Err（`Invalid checksum` など）になる場合と、
  Ok になる場合（compressed size だけが過大な stored / deflate）がある。
- **推定原因**: open 時に検査するのは名前だけ（`package.rs:121-125`）。`write_to` の `raw_copy_file`（`package.rs:301`）は
  zip crate が `take(compressed_size)` で読める分（アーカイブの残り全部）をコピーし、ヘッダは申告値のまま書く。
- **修正の方向**: open 時に各エントリの `data_start() + compressed_size() <= len` を検査し、満たさなければ `Error::Package`。

### F-3 [Medium] lens が名前空間宣言への書き込みを拒否しない

- **再現**: `<p xmlns="urn:t" xmlns:o="urn:o"><o:z/></p>` で
  `remove_attr(QName::new(XMLNS_NS, "o"))` は Ok で、出力は `unbound prefix "o"` で parse できない。
  `set_attr(QName::new(XMLNS_NS, "o"), "urn:other")` も Ok で、出力を parse すると `o:z` の名前空間が `urn:other` になる（木は `urn:o` のまま）。
- **影響**: どちらも呼び出し側が `XMLNS_NS` の `QName` を渡したときだけ起きるので、信頼できない入力から直接は届かない。
  ただし README の「no primitive can write a part that is not well-formed」（`InvalidXmlChar` の項）と、
  dirty フラグが表す「木 = 出力」の対応が崩れる。生成コードが `xmlns` を属性として持つことはないが、手書きの format crate コードは渡せる。
- **修正の方向**: `XMLNS_NS` の属性への `set_attr` / `remove_attr` を新しいエラー（例: `NamespaceDeclaration`）で拒否する。
  宣言の追加が必要になったら専用の API にする（README の「Phase0 never declares namespaces」と同じ考え方）。

### F-6 [High] 申告された非圧縮サイズでメモリを確保する（ZIP64 では abort）

- **再現（ZIP64、abort）**: `[Content_Types].xml` か `a.bin` を ZIP64 エントリとして書き、中央ディレクトリの zip64 extra の
  uncompressed size を 2^40 / 2^62 / 2^64−1 にした 459 バイトの zip。`Package::from_bytes`（`[Content_Types].xml` の場合は open だけで）または
  `read_part` が `memory allocation of 1099511627776 bytes failed` で **abort**（SIGABRT、exit 134）する。2^62 でも capacity overflow の
  panic ではなく abort になる。overcommit の有無に関係しない。ファザーが届かなかったのは seed に ZIP64 のエントリがなかったため
  （現在は `seed_corpus` が ZIP64 の変種を作る）。テスト `f6_zip64_declared_size_does_not_abort` は本体を子プロセスで動かし、
  abort してもテストバイナリの他の結果は失われない。
- **再現（32 ビットの申告）**: `a.bin`（10 バイト、stored）の中央ディレクトリの uncompressed size を `0xF0000000` にする。
  `read_part("a.bin")` は約 4 GB の確保を要求する（テストはグローバルアロケータで最大要求サイズを記録して確認）。
  ファザーでは deflate のエントリ（圧縮後 191 バイト、申告 3,087,008,061 バイト）で `-rss_limit_mb=2048` の OOM として検出し、2 回目でも `malloc(4278125026)` などの OOM が 2 件出た。スタックは `package.rs:181` ← `package.rs:138`（open 時の `[Content_Types].xml` 読み取り）だった。
- **影響**: 1 つの小さな入力でプロセスごと落ちる DoS。`handle_alloc_error` は unwind しないので、`catch_unwind` で隔離できない。
  `[Content_Types].xml` と `_rels/.rels` は open 時に読むので、**open だけで**到達する。ooxml-xlsx / -pptx / -docx の `from_bytes` も同じ経路を通る。
  32 ビットの申告（最大 4 GB）では、overcommit のある Linux なら触らないページは確保されないので表に出にくいが、
  overcommit を切った環境、32 ビット環境、メモリ上限のあるコンテナでは同じく abort する。
- **推定原因**: `package.rs:181` の `Vec::with_capacity(file.size() as usize)`。
- **修正の方向**: `size()` を信用しない。確保は `min(size, compressed_size × 定数, 上限)` にし、`take(上限 + 1)` で読んで超えたら `Err` にする
  （I-2 と同じ上限）。PR #40 はこの方針で直している（申告サイズは拒否の判断にだけ使い、バッファは 64 KiB から伸ばす。part と package の上限は設定可能）。

### F-7 [Medium] open が entry 数に対して二乗時間

- **再現**: 空の stored エントリ n 個の zip。release ビルド（ファザー実行中の負荷あり）で
  n = 10,000 → 0.99 秒、40,000 → 12.0 秒、65,535（6 MB）→ 30.2 秒。zip crate の `ZipArchive::new` だけなら 12〜36 ms。
  テストは 4,000 と 16,000 の時間比を見る（debug で 0.84 秒 → 12.2 秒、比 14.6。線形なら 4 前後）。
- **影響**: 数 MB のファイルで数十秒 CPU を占有できる（サービスで受け付けると DoS）。ZIP64 なら entry 数はさらに増やせる。
- **推定原因**: `package.rs:123` の `parts.iter().any(|p| p.name.eq_ignore_ascii_case(&name))`。
  `find()` も線形探索なので、format crate が part ごとに `contains` / `read_part` を呼ぶ処理も同様に二乗になりうる。
- **修正の方向**: ASCII 小文字化した名前の `HashSet` で重複検査し、`find` 用に `HashMap<String, usize>` を持つ。
- **状況**: PR #14（`core/remove-part`）の `from_bytes_with` が `HashSet` + `normalize_part_name` で重複検査を線形にしている（review N11）。
  二重に直さないこと。#14 のマージ後に `f7_*` の `#[ignore]` を外す。#14 の後も `find()` / `find_ci()` は線形探索なので、
  part ごとに `contains` / `read_part` を呼ぶ処理が二乗になる点は残る（別項として追跡する）。

### F-4 [Low] 内容中の XML 宣言を受け付けて木から落とす

- **再現**: `<a>x<?xml version="1.0"?>y</a>` は Ok。`force_rerender` 後の出力は `<a>xy</a>`。
  元の木は `Text("x")`, `Text("y")` の 2 項目、再 parse した木は `Text("xy")` の 1 項目なので `structurally_eq` が false（T1b 違反）。
  `<a><?xml encoding="latin1"?></a>` は `UnsupportedEncoding` になる。ファザーの最小再現は `<sst>r<?xml?>,</sst>`（20 バイト）。
- **影響**: XML 宣言は文書の先頭にしか置けないので、入力は非 well-formed。親要素が dirty になると宣言が出力から消える（黙った欠落）。
- **推定原因**: `parse.rs:99` の `Event::Decl` はエンコーディングを検査するだけで、ルートの中でも項目を作らない。
- **修正の方向**: 先頭（BOM の直後）以外の `Event::Decl` を `Error::Xml` で拒否する。

### F-5 [Low] 名前が空の要素を受け付ける

- **再現**: `< />`、`<\r/>`、`<a>< /></a>` が Ok で、ルートの `XName` の local name が空になる。
  `force_rerender` 後の出力 `</>` は parse できない。
- **影響**: 入力は非 well-formed。名前が空の要素はスキーマの要素と一致しないので、通常の編集で own_dirty になることはなく、
  実害は T1b 検査と `force_rerender` に限られる。
- **推定原因**: quick-xml は `<` の直後の空白を名前の区切りとして扱い、空の名前をエラーにしない。`parse.rs:258` も名前を検査していない。
- **修正の方向**: `open()` で local name・prefix が空なら `Error::Xml`。I-1 と合わせて Name 生成規則を検査してもよい。

### I-1 [Info] ほかの非 well-formed 入力

parse が Ok を返し、T1 / T1b は成り立つ（再描画しても同じ形で読める）ので不変条件の違反ではない。
「書き込みでは拒否する文字・構文を、読み込みでは受け付ける」非対称の記録として残す。

| 入力 | 備考 |
|---|---|
| `<a>\0</a>`（生の U+0000）、`<a>&#xFFFE;</a>` | `set_text` は同じ文字を `InvalidXmlChar` で拒否する |
| `<1a/>`、`<a:b:c xmlns:a="u"/>` | Name / QName の生成規則違反（local name が `b:c` になる） |
| `<a xmlns:p=""/>`、`<a xmlns:xml="urn:x"/>`、`<xmlns:a xmlns:xmlns="u"/>` | Namespaces in XML 1.0 で禁止 |
| `<a xmlns:p="u" xmlns:q="u" p:x="1" q:x="2"/>` | 展開名が重複する属性 |
| `<a/>trailing`、`&lt;<a/>` | ルートの外の文字データ・参照 |

### I-2 [Info] 展開後サイズの上限がない

`read_part` と open 時の `[Content_Types].xml` / `.rels` の読み取りは、申告サイズが正しければ展開後のサイズに上限を設けない。
deflate の圧縮率は最大で約 1:1000 なので、1 MB の part で約 1 GB を確保させられる。
ファザーの入力サイズでは到達しないため所見としては数えないが、F-6 の修正と合わせて上限（設定可能な値）を決めることを勧める。

## 再現と再実行

```sh
# 再現テスト（stable 1.99）
cargo test -p ooxml-robustness --test fuzz_findings
# stable のスモーク（CI の cargo test にも含まれる）
OOXML_SMOKE_ITERS=50 cargo test --release -p ooxml-robustness --test smoke
# ファザー（nightly）
cargo run -p ooxml-robustness --example seed_corpus -- fuzz/corpus
cd fuzz && cargo +nightly fuzz run xml_t1b corpus/xml_t1b -- -max_total_time=600
# 成果物の仕分け（stable、1 入力 1 行）
cargo run -p ooxml-robustness --example replay -- xml_t1b fuzz/artifacts/xml_t1b/*
```

`.github/workflows/fuzz-smoke.yml`（`workflow_dispatch` と毎週月曜の schedule）は、stable スモークを release で、
各ターゲットの cargo-fuzz を 60 秒ずつ実行する。`ci.yml` は変更していない。
