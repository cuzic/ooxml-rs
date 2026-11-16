# PR #50 レビュー（Opus, round 2）

対象: `impl/xlsx-phase2` @ `c404851`（round-1 修正 `5840411` と origin/main のマージ）。
検証はすべて `git clone` した複製で行った（worktree は読み取りのみ）。

- 場所: `/tmp/claude-1002/-home-cuzic/21ae987c-b761-4464-83f8-adb7f35041b8/scratchpad/review-pr50-r2/`（`repo/` は probe 用、`mutrepo/` は mutation 用）
- 保存物（`keep/`）:
  - `probe_r2.rs`、`probe_r2b.rs`: 再現テスト Q1–Q7、R1–R2。
  - `probe_perf_r2.rs`: append を伴う書き込みの計測。
  - `probe_corpus.rs`: round 1 の sweep に、`with_default_font_of` と `is_date`（活性化の前後）の検査を足したもの。
  - `mutate_r2.py`、`mut_r2.log`: mutant 40 個と、その結果。
  - `sweep_r2.log`、`probe_corpus_r2.log`: コーパスのログ。
- コーパス: POI `test-data/spreadsheet`（47dcff76、366 ファイル）。Python は `python3 -I` でだけ使った。

## 結論（先に）

**収束。Blocker 0、Major 0、Minor 5、Nit 2。**

round 1 の Major 5 件は、すべて意図どおりに直っていることを確認した。

- probe で直接確かめた。
- round 1 で生き残った mutant 11 個は、すべて CI で kill される。
- コーパスの結果は 326 ok / 0 fail で、round 1 から後退していない。

新しく見つけたのは次の 2 点で、どちらも Minor である。

- `ExcelDateTime::from_serial` が巨大な serial で整数オーバーフローを起こす。debug ビルドでは panic する。読み取り API の `datetime()` からも到達できる。
- テストの穴が残っている。特に M5(b) のキャッシュ無効化は、無効化をまるごと外しても既存テストが通る。

どちらもマージ前に直すことを勧める。ただし収束判定は妨げない。

## 実施した検証

| 項目 | 結果 |
| --- | --- |
| `cargo test -p ooxml-xlsx --release` | 全通過（`phase2_review1` 18、`phase2_perf` 2 を含む） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 警告なし |
| `cargo xtask codegen --check` | up to date（allowlist に `condense`、`extend`、`vertAlign`、`charset` を追加済み） |
| `RUSTDOCFLAGS=-D warnings cargo doc -p ooxml-xlsx` | 警告なし |
| コーパス sweep（`registry_sweep_over_a_local_corpus`） | 366 files、**ok 326 / fail 0**、refused 41（round 1、作者の主張と一致） |
| `probe_corpus.rs`（preserving setter、日付、Normal） | ok 98、merged range での拒否 17、`NoSuchStyle` 19、**不一致・panic 0**（round 1 と同一） |
| `default_font()` が `None` になるファイル | **43 件 → 2 件**（`47889.xlsx`、`style-alternate-content.xlsx`）。charset が原因のものは解消した |
| `with_default_font_of(..).set_bold()` を離れたセルに書き込む | 326 件で成功。失敗 4 件は font と無関係（iterative calc、`r` 欠落、AC sibling） |
| `is_date`（活性化前の直接読み）と活性化後の registry 経由 | styled cell 最大 300 個 × 330 ファイルで**全件一致** |
| round 1 の生存 mutant 11 個（R01、R07、R11–R15、R17、R18、R20、R22。パターンは現行コードに合わせて更新した） | **11/11 kill**。いずれも `phase2_review1.rs` の専用テストで kill され、CI 内で完結する |
| 私の新しい mutant 29 個（X01–X29。round 1 修正の箇所が対象） | 18 kill、**11 survive**（m3 を参照） |
| AC の 2 つの保守的拒否 | コーパスで `styleSheet` 直下の AC と sst 内の AC を探した。**該当は 0 件**だった（`style-alternate-content.xlsx` の AC は `fonts`、`cellXfs`、`dxfs` の中だけ） |

## 解消状況

| round 1 | 状況 | 確認内容 |
| --- | --- | --- |
| **M1** `ExcelDateTime` の pub フィールド | **解消** | フィールドは private で、getter がある。struct literal はコンパイルできない（P1 は書けなくなった）。<br>`serial()` は `is_finite` で拒否する。<br>9999-12-31 23:59:59.999999 は書けた（`2958465.99999999`）。<br>`from_serial(NaN/∞/負)` は `None`。<br>ただし巨大な有限値には別の問題がある（新規 m1） |
| **M2** `Builtin(≥164)` | **解消** | 164、165、999、200 は、`set_number_with_format` でも `set_format` でも `InvalidFormat`。<br>49、50、100、163 は通る。164 未満は custom の割り当てと衝突しないので、意味が後から付くことはない |
| **M3** `Font` の `charset` など | **解消** | 游ゴシック（theme color と scheme 付き）と、ＭＳ Ｐゴシック（color も scheme もない）の両方で `default_font()` が `Some` になる。<br>unchanged の既定フォントは font 0 にバイト単位で reuse され、italic の派生も `charset`、`family=3` を保つ（Q7）。<br>出力順は `b, i, strike, condense, extend, u, vertAlign, sz, color, name, family, charset, scheme` で、Excel の順と一致する。<br>manual-check 35 を確認した。<br>`outline`/`shadow` で Calibri に落ちることは Deviations と doc に記載されている（round 1 の「少なくとも明記」を満たす） |
| **M4** 存在しないセル | **解消** | 優先順位は「行 `customFormat="1"` の `s`」→「`col/@style`」→「0」。行書式が列書式に優先するのは Excel と同じである（Excel も LibreOffice も、空セルの表示は行書式が勝つ）。<br>行が `s="0" customFormat="1"`、列が bold のとき、結果は非 bold（Q4c、正しい）。<br>`customFormat` のない行 `s` は無視される（テストあり）。<br>日付列での `set_datetime` は列の date xf を `current` として受け取り、`EnsureDate` が既存の日付書式を保つ（Q4d）。<br>`set_named_style("Normal")` は Normal に戻す（Q4e）。<br>列の style が dangling（範囲外）なら `DanglingReference` になり、出力はバイト同一（Q4b、Q4f）。保守的な扱いで、妥当である。<br>例外は新規 n1 |
| **M5(a)** registry の clone | **解消（reuse の経路）** | Arc の copy-on-write になった。`plan` の clone は Arc の参照数を増やすだけで済み、`make_mut` は `next` 側でだけ起きる。そのため `self` は変わらない。<br>Q5 で確かめた。fontId が dangling になる計画が numFmt を `next` に足した後で失敗しても、`Err` 後の出力はバイト同一だった。次の `"0.000"` は新しく numFmt 164 を定義し、まっさらなセッションと同じバイトになった。漏れはない。<br>50 種の書式を 2 セッション書いても、2 回目は何も追加しない。<br>ただし append を伴う書き込みは、まだ O(styles) である（新規 m2） |
| **M5(b)** 数式の再走査 | **解消** | キャッシュは `Workbook` に持つ。数式の集合が変わるのは Phase1 の `edit_cell_ext` だけである（census は open 時に全シート分作られる。定義名を編集する API や、シートを追加する API はない）。無効化の条件は `CellOp::Formula` と、`target.is_some()`（数式セルの上書き、`remove_formula`、shared の hand-off を含む）である。<br>無効化は plan が成功した後、apply の前に行う。plan が `Err` なら何も変わらないので、キャッシュを残しても正しい。<br>R2 で 4 方向の遷移を観測した。<br>- false→true（`set_formula("CELL(...)")`。その後の style-only で B1 の `<v>` が strip される）<br>- true→false（`remove_formula`、`set_formula("A1*3")`、値による上書き）<br>どれも fresh session と同じ結果だった。<br>shared master の hand-off 後もキャッシュは true のまま正しい（Q6f） |
| m1 14、22 への写像 | 解消 | `"mm-dd-yy"` は custom 164 になる。`Builtin(14)` は明示すれば使える |
| m2 `General` の再定義 | 解消 | `BuiltinNumFormatRedefined { id: 0 }` |
| m3 `Format::date()` の fallback | 解消 | 新しい `NumFormat::DefaultDate` になった。14 が再定義されていれば `yyyy-mm-dd` に落ちる。`set_datetime` も同じ経路を通る。ADR §6 と一致するので、Deviation ではなくなった。公開 enum に variant が増えるが、0.x なので問題ない |
| m4 値の検証 | 解消 | 空 code、theme > 11、名前 > 31 文字、family > 14、indent > 250。コーパスでは、ファイル由来の既定フォントがこの検証で拒否される例は 0 件だった |
| m5 `is_date` の活性化 | 解消 | styles のないブックは `false`。活性化しないで読む。活性化後との一致はコーパスで確認した |
| m6 Normal が AC | 解消 | `default_unreadable` のとき、未設定の group を持つ `Format` は `NotExpressible` |
| m7 文字参照 | 解消 | `&#49;70` を decode して 171 を割り当てる。decode できない値は `Malformed` で拒否する（well-formed な XML では起きない） |
| m8 mutant | 解消 | 11/11 kill（上の表）。ただし今回の修正箇所に新しい穴がある（新規 m3） |
| m9 Deviations | 解消 | 2 つの AC 拒否、`outline`/`shadow`、#44 の再検討が載った |

**残した 2 つの保守的拒否について**: どちらも受け入れられる。

- 一つは `styleSheet` 直下の AC による registry 全体の拒否、もう一つは sst に AC があるときの reuse を含む拒否である。
- どちらも、黙って壊すのではなく `Err` にする方向に倒している。Err ⇒ バイト同一も保たれる。
- 366 ファイルのコーパスに該当例は 0 件で、Excel もこの形を出力しない。
- 緩めるかどうかは、実ファイルで報告が来てから決めればよい。
- part 2 か ADR-0006 の post-acceptance note に一行残しておくとよい（任意）。

## Findings

### Minor

**m1. `ExcelDateTime::from_serial` が巨大な serial で整数オーバーフローを起こす。debug ビルドで panic する**（`registry/dates.rs` 106–126 行、163 行）

- 根拠: R1 を実行した。
  - `from_serial(9.3e18 / 1e19 / 1e300 / f64::MAX, false)` は、debug で `attempt to add with overflow`（`civil_from_days` の `z + 719_468`、および `base + days`）を起こして panic する。
  - 読み取り API からも到達する。date 書式のセルに `<v>1E+19</v>` があると、`ws.datetime(0, 0)` が panic する。
  - release では wrapping によって偶然 `None` / `DateOutOfRange` になる。
- 原因: `serial.floor() as i64` が飽和したあとの i64 加算である。round 1 から存在したが、見落としていた。
- 修正案: `is_finite` の直後で、上限（1900 系 2,958,465、1904 系 2,957,003 程度。余裕を持って `>= 3.0e6`）を超える値を `None` にする。回帰テストを 1 本入れる。

**m2. append を伴う style 書き込みは、まだ O(styles.xml) per write**（`styles.rs` `Planner::staged`、`Arc::make_mut`）

- 根拠: `probe_perf_r2.rs`（release）で計測した。毎回新しい font と xf を足す書き込みを 400 回行った。

  | cellXfs | 時間 | 1 回あたり |
  | --- | --- | --- |
  | 1 | 0.44 s | 約 1.1 ms |
  | 5,001 | 4.1 s | 約 10 ms |
  | 20,001 | 19.3 s | 約 48 ms |

  同じ書式を reuse する書き込みは、どの場合も 400 回で約 15 ms だった（M5(a) の修正は効いている）。
- append は、styles.xml の `XmlDocument` の clone と、reuse map や `xfs` の `make_mut` コピーを伴う。
- 回数は「新しい書式の種類数」で抑えられる。上限は fonts 512、numFmts 200 などである。そのため通常の用途では問題にならず、Major にはしない。
- `phase2_perf.rs` は reuse の経路しか測っていない。
- 対応案: 次のいずれか。
  - part 2 以降で staged clone を追記ログに替える。plan は append の内容だけを持ち、apply で挿入する。
  - 最低限、doc の性能の節に「新しい書式 1 種ごとに O(スタイル表)」と明記する。

**m3. round-1 修正箇所のテストの穴（mutant 11 個が生存）**

- 次の 3 個が **最も重要**である。
  - X16: 無効化を常に off にする。
  - X17: `Formula` op だけで無効化する。
  - X18: target だけで無効化する。

  キャッシュの無効化を丸ごと外しても、全テストが通る。現在のコードは正しい（R2 で確認した）が、将来の変更で健全性が黙って壊れうる。R2 の false→true（`set_formula("CELL(\"format\",A1)")` の後で style-only にし、依存先の `<v>` が strip されることを見る）と、true→false（`remove_formula`）を、テストとして入れること。
- X06、X07、X09、X10: `condense`/`extend` の書き出し、`vertAlign` と `extend` の読み取りが未テスト。`set_font_script` と、`<condense/>`、`<vertAlign>` を持つ既定フォントの往復を 1 本ずつ入れる。
- X28: `is_date` の直接読みが custom code を無視してもテストが通る（m5 のテストは built-in 14 しか見ていない）。`numFmtId="164" formatCode="yyyy/m/d"` のケースを加える。
- X29: `cellStyleXfs` が index-stable でない（Fallback のない AC など）ときに cellXfs[0] へ落としても、テストが通る。m6 のテストは「entry 0 が stable な AC」の場合だけを扱っている。
- X20（`inherits` が protection を見ない）、X22（`&#x..;` を 10 進で読む）は小さい穴である。
- X23（`serial()` の `is_finite` を外す）は、private フィールドと検証付きコンストラクタの下では等価と判断した。

**m4. `missing_cell_style` が、`customFormat="1"` で `s` のない行を「行書式なし」と扱う**（`edit.rs` `missing_cell_style`）

- Q4a: 行が `<row r="1" customFormat="1">`、列が bold のとき、B1 は列の bold になる。
- `CT_Row/@s` の既定値は 0 なので、Excel はこの行を xf 0 で表示するはずである。
- Excel 自身はこの形を出力しないので、影響は小さい。
- 修正案: `customFormat` が真なら `s.unwrap_or(0)` を返す（1 行の変更）。

**m5. PR 本文の M5 の数値は reuse の経路のもの**

- m2 の append 経路の性質が、本文にも Deviations にも書かれていない。m2 の扱いを決めたら、本文に 1 行加えること。

### Nit

- **n1.** `InvalidFormula` のメッセージに `at byte 18446744073709551615`（`usize::MAX`）が出る。Q6g の `set_formula("CELL(\"format\",A1")`（閉じ括弧なし）で起きる。Phase1 の lexer 由来で、この PR の範囲外である。別 issue でよい。
- **n2.** `ExcelDateTime::month()` の doc は、時刻のみの値について「12」と書く。`day()` の値（31）には触れていない。時刻のみの値では `year()` が `None` であることを、month/day の doc にも書くとよい。

## 判定

**converged: yes**（Blocker 0、Major 0）。

マージ前に次を勧める。

- m1: overflow のガードとテスト 1 本。
- m3: キャッシュ無効化の回帰テスト（X16–X18）と、font フィールドの往復テスト。

どちらも小さな変更なので、もう 1 ラウンドのレビューは不要と考える。m2 と m4 は、part 2 で扱うか doc に記載すればよい。
