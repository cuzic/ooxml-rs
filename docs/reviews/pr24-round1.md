# PR #24 レビュー（Opus, round 1）

対象: `perf/bench-baseline` @ `10c2fc7`（`crates/ooxml-bench`、`docs/testing/performance-baseline-2026-11.md`）

## 結論（先に）

- 方法論はおおむね妥当です。
  - umya と ooxml の編集内容は同じです（B 列、同じ行、1 始まりへの変換も正しい）。umya は `read_reader(_, true)` でシートまで全部読み込みます。
  - calamine はオープン時に共有文字列表を読み込み、ooxml の `value()` も SST を解決するので、読み取りの仕事量は揃っています。
  - ooxml の `write_string` は `inlineStr` を書き、`constant_memory` もインライン文字列を書くので、Phase6 の比較相手として釣り合っています。
  - `cargo test --workspace --all-features` は ooxml-bench も比較用 3 ライブラリもビルドしません（実測で確認）。
  - CI の clippy コマンドは警告ゼロで通ります。
  - ライセンスは許容的なものだけです。
- **Major が 1 件あります。** breakdown の `best()` が戻り値の drop を計測区間に含めています。そのため、報告書の「quick-xml の約 6×」「drop は parse の 40%」が誤っています。baseline 側でも、drop を含むケースと含まないケースが混在しています。
- 予測値（外挿）にはラベルが付いています。ただし報告書自身の breakdown データと比べると過小見積りです。Phase6 に向けた優先順位も明示されていません（いずれも Minor）。

## Findings

### M1（Major）drop の計上が不統一で、hotspot 1 と Summary の数値が誤っている

- **breakdown 側。** `benches/breakdown.rs` L41-50 の `best()` は `std::hint::black_box(f());` を文として実行しています。戻り値の一時値は文の終わり、つまり `t.elapsed()` より前に drop されます。
  - そのため「`XmlDocument::parse`」列（L95）と「parse sheet1.xml」列（L144）は **parse + drop** の時間です。
  - 一方、drop は別の列（L99-101）でも計っています。drop が二重に計上されていることになります。
- **プローブで確認しました**（scratchpad、100k cells、負荷下、3 回とも同じ傾向）。
  - parse+drop は 155 ms、parse のみは 85 ms、drop は 70 ms、quick-xml のイベント処理は 18.7 ms でした。
  - parse のみと quick-xml の比は **約 4.5×** です。drop は parse のみの **約 80%** に当たります。
  - 報告書の 1M 行も同じように読み直せます。parse のみは 2.87 − 1.23 ≈ 1.64 s です。quick-xml との比は約 3.3×、drop は parse の約 75% になります。
  - 誤っている記述: Summary の「about 6× slower」「another 40%」（L26-27）、hotspot 1 の「6–7×」「40%」（L219-227）、hotspot 4 の「parsing takes 3.48 s of the 3.91 s」（このうち約 1.2 s は drop）。
- **baseline 側。** `measure()` は戻り値を `dt` の計測後に drop します（`lib.rs` の `drop(black_box(out))`）。
  - 木を返すケースでは drop が計測に入りません。該当は `core/xml-parse`、`core/xml-clone`、`xlsx/open/ooxml` です。
  - 結果だけを返すケースでは、クロージャの中で drop が起きて計測に入ります。該当は `xlsx/read-all/calamine`、`xlsx/read-all/ooxml-raw-walk`、edit-save 全般です。
  - 影響 1: Summary の「Open only 1.21 s vs calamine 0.33 s, 3.6×」は、片方だけが drop を含む比較です。
  - 影響 2: raw-walk（1.83 s）が、より多くの仕事をしている open（1.21 s）より遅いのも、主にこの差です。
- **修正案。**
  - `best()` は `let v = f(); let dt = t.elapsed(); drop(v);` の形にし、drop を別の列として出します。
  - baseline は「ユーザーが払う総コスト」に揃えて、drop も計測に含めるのが自然です。たとえば `measure` の中で `drop(out)` を計測区間に入れます。どちらを選んでも、Method に明記してください。
  - そのうえで Summary と hotspot 1・4 の倍率を直してください。結論の方向（allocation と drop が支配的）は変わらず、drop の比重はむしろ大きくなります。

### m1（Minor）外挿の予測値が、報告書自身の breakdown と比べて過小

- `quadratic()` は直前に計測できた 1 点から外挿します。
  - read-all の 1M は、10k の 1 点から 100× の外挿です。100k はスキップされたので `prev_read` が更新されていません。
- breakdown の 1 呼び出しあたりのコストは O(rows) より速く伸びています。
  - `cell(0,0)` は 1k 行で 56 µs、10k 行で 1.64 ms（29×）、100k 行で 18.5 ms です。
  - これを使うと、1M セルの全読み取りは 18.5〜44 ms × 1M = **約 5〜12 h** になります。Summary の「1.8–4 h」より長くなります。
  - write-new も同様です。「1 行追加」の測定（5000→10000 行で 4.8×）から見て、「35 min」は下限に近い値です。
- **修正案。** 「下限の目安（X の 1 点からの二次外挿）」と明記してください。外挿の起点（10k / 100k）も書いてください。

### m2（Minor）Summary の Write 行が run 1 と run 2 を混ぜている

- 「rust_xlsxwriter 1.0 s at 1M（run 1）。With `constant_memory`: 1.75 s（run 2）」と並べると、constant_memory の方が遅いように読めます。同じ run 2 では通常モードが 2.19 s、constant_memory が 1.75 s です。
- Phase6 の目標は constant_memory との比較です。にもかかわらず、比率「≈190×」は通常モード（run 1）に対するものです。
- **修正案。** 同じ run の constant_memory に対する比を出してください。100k、run 2 なら 45.23 / 0.123 ≈ 370× です。

### m3（Minor）constant_memory はファイル I/O を伴い、ヒープピークに一時ファイルが入らない

- `add_worksheet_with_constant_memory` は `tempfile_in` で一時ファイルに行データを書きます（rust_xlsxwriter 0.102 の `workbook.rs`、`performance.rs` の "Restrictions"）。
  - そのため Method の「no I/O is timed」はこのケースに当てはまりません。
  - 「3.9 MiB」には約 33 MiB の一時ファイルが含まれていません。tmpfs ならこれも RAM を使います（このホストの `/tmp` は ext4）。
- 文字列については、通常モードは SST、constant_memory と ooxml はインライン文字列です。比較が釣り合っているのは良い点なので、Method に書いておく価値があります。

### m4（Minor）Results 表に Runs 列がない

- ベンチは Runs を出力していますが、報告書の表では落ちています。
- 3 s を超える 1M のケースは runs = 1 で、ウォームアップなしの 1 回だけです（run 2 で Median = Min の行）。
- 負荷下のデータなので、どの行が 1 回だけの計測かが分かるようにしてください。
- breakdown は best-of-3 で、「one more row」は 1 回だけの計測です。これも Method に書いてください。

### m5（Minor）raw-walk の「下限」は、calamine より仕事が少ない比較

- `ooxml-raw-walk` は sharedStrings も styles も読み込みません。A 列の `v`（SST の添字）も `f64` として足し込むだけです。
- calamine は文字列を解決して `Data` を作ります。
- 「5.5×」は ooxml に有利な比較です。「lower bound」と書かれているので許容範囲ですが、Summary にも一言添えてください。

### m6（Minor）カウンティングアロケータの細部

- スレッド安全性は問題ありません（atomic と `fetch_max`）。
- `realloc` は `sub(old)` してから `add(new)` します。移動を伴う realloc では旧と新の領域が同時に存在する瞬間がありますが、この順序ではピークに入りません。
  - 保守的にするなら `add(new)` を先に、`sub(old)` を後にします。実害は小さい見込みです（大きな領域は mremap で移るため）。
- `count` は realloc も数えます（doc コメントに記載あり）。
  - そのため「5.38 allocations per element」には `Vec` の伸長（`grow_one`）が含まれます。
  - hotspot 1 の内訳（XName、attrs、children…）は、別々に確保される要素だけを列挙しています。
  - realloc を別に数えると、提案 3（attrs を一度で確保、inline capacity）の効果を見積もれます。

### m7（Minor）「dev-dependencies は cargo bench のときだけコンパイルされる」は不正確

- `cargo test --workspace --all-features --no-run` では、ooxml-bench、umya、rust_xlsxwriter のどれもビルドされないことを確認しました。calamine はもともと ooxml-xlsx の dev-dep です。
- 一方、CI の `cargo clippy --workspace --all-targets` はベンチターゲットもチェックします。
  - その結果、**55 crate** が追加でチェックされます。umya-spreadsheet、rust_xlsxwriter、pest、fancy-regex、chrono、aes、tempfile、psm/stacker（`cc` によるビルドスクリプト）などです。
  - 現時点では警告ゼロで通ります。ただし、これらの crate が将来のツールチェーンや lint で壊れると CI が落ちるリスクがあります。
- **修正案。** 報告書の記述を「`cargo test` ではビルドされない。`clippy --all-targets` ではチェックされる」に直してください。そのリスクを受け入れるかどうかも一言書いてください。

### m8（Minor）hotspot の優先順位が Phase6 に対して明示されていない

- 費用対効果が最も高いのは hotspot 2.1 と 2.2（行インデックスと追記の高速パス）です。二次の振る舞いを解消し、edit-save で 1,000 編集のときの 4.6× もここが原因です。次が 3.1（圧縮レベル。小さな変更で save を約 14× 速くできる）です。番号の順序（1 → 2 → 3）と逆なので、推奨する着手順を明記してください。
- hotspot 1 の 1.1〜1.3（名前のインターン化、span による遅延デコード、ノードの縮小）だけでは、Phase6 のメモリ目標（触った行に比例）を満たせません。1.44 GiB を数倍減らしても、calamine の 77 MiB より桁違いに大きいままです。
  - 目標に届くのは 1.4（遅延 `sheetData`）だけです。
  - 1.1〜1.3 は pptx や他の part には効きます。一方 worksheet については、1.4 を実装すれば効果の多くが不要になります。その旨を書いてください。
- 2.1 の行インデックスの置き場所について。
  - `Worksheet` は `Workbook::worksheet()` のたびに作られる短命の借用です。
  - したがって、インデックスは `SheetContext` か `Workbook` 側に持たせ、書き込み時に更新するか無効化する必要があります。
  - 「Worksheet が木を所有している」という記述（L259-261）は不正確です。

### m9（Minor、本 PR の範囲外）example 名 `manual_check` の重複

- `crates/ooxml-xlsx/examples/manual_check.rs`（d05dee0）と `crates/ooxml-pptx/examples/manual_check.rs`（ce11413）は main にもともとあり、本 PR が持ち込んだものではありません。
- `cargo test --workspace` で "output filename collision" の警告が出ます。cargo#6313 によれば、将来ハードエラーになる可能性があります。
- 実害は 2 点です。
  - `target/debug/examples/manual_check` が上書きされるので、バイナリを直接実行すると別のクレートのものが動くことがあります。
  - 将来、CI の `cargo test` が壊れるリスクがあります。
- 別 PR で `xlsx_manual_check` / `pptx_manual_check` などに改名することを推奨します。重大度は Minor です。

### nit

- ライセンス
  - 追加した 3 crate は calamine（MIT）、rust_xlsxwriter（MIT OR Apache-2.0）、umya-spreadsheet 2.3.3（MIT）です。
  - 推移的な依存も MIT、Apache、BSD、Zlib、Unicode-3.0 などの許容的なライセンス（または選択肢）だけです。
  - `publish = false` で配布しないので、問題ありません。
- `Workbook::from_bytes(bytes.clone())` では、`Vec` から `Arc<[u8]>` への変換でもう一度コピーが起きます（ooxml 側だけ 2 回コピー）。影響は無視できます。

## 判定

converged: **no**

- M1（drop の計上と、それに基づく倍率）を直せば、方法論として収束します。
- m1〜m8 は報告書の文言と表の修正で対応できます。
- m9 は別 PR で扱ってください。
