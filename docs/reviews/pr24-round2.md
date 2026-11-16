# PR #24 レビュー（Opus, round 2）

対象: `perf/bench-baseline` @ `f600641`（round 1 の指摘への修正コミット）。round 1 は `docs/reviews/pr24-round1.md`。

## 結論（先に）

- **M1 と m1〜m8 はすべて解消しています。** 新しい Major も Minor もありません。nit が 4 件あります。
  - drop の計上が統一されました。baseline は全ケースが drop 込み（`measure` の計測区間内で `drop(black_box(f()))`）です。breakdown は drop を除いて計り、drop は `best_drop` で別に計ります。Method、表の見出し、Summary、hotspot 1・4 の記述もこの方針に揃っています。
  - Summary と hotspot の倍率は、Results 表と breakdown 表（run 3）から再計算すると一致します（下の表）。
- **スポット再現をしました。** scratchpad にコピーを作り、`breakdown` 全体と `baseline -- xlsx/`（`OOXML_BENCH_MAX=100000`）を実行しました。負荷は load average 11〜35 です。
  - アロケーション数、realloc 数、heap after parse、heap peak は報告書と完全に一致しました（決定的、という記述どおり）。
  - 時間の比率は報告書の幅か、その近くに収まりました。parse / events は 4.1〜4.6×、drop / parse は 40〜94%、open / calamine は 100k で 4.6× です。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` は PR の HEAD で exit 0 でした。`cargo test` が ooxml-bench をビルドしないこと、clippy がベンチターゲットまでチェックすること、という記述は正しいです。

## round 1 指摘の確認

| 指摘 | 状態 | 根拠 |
|---|---|---|
| M1 drop の計上 | 解消 | `lib.rs` `measure`：warm-up と計測は同じ `run` クロージャで、drop は計測区間の内側。`breakdown.rs` `best`：`let v = f(); let dt = t.elapsed(); drop(v);`。`best_drop` で drop を単独計測。表の見出しは「`XmlDocument::parse` (no drop)」「drop」「drop of the `Workbook`」。Summary の Open 行は「drop included … Of this, drop is 1.2 s」。 |
| m1 外挿は下限 | 解消 | skip 行は「lower bound ≈ … (quadratic extrapolation from 10k/100k cells …)」。Summary には、breakdown の 1 呼び出しコストに基づく 5.6〜12 h も併記。下限であることは数値でも確認できます。10k の全読み取りは 1 呼び出しあたり 132 µs（1.32 s / 10k）で、これを行数に比例させると 100k 行では 13 ms。実測は 20〜44 ms です。 |
| m2 run の混在 | 解消 | すべて run 3。Write 行は constant_memory との比（35.88 s / 0.115 s ≈ 312 → 「≈ 310×」）。 |
| m3 constant_memory の I/O | 解消 | Method「What the comparisons do」に tempfile のファイル I/O と、heap peak に一時ファイル（約 33 MiB）が入らないことを明記。文字列の書き方（inline と SST）も明記。 |
| m4 warm-up と Runs 列 | 解消 | warm-up を 1 回入れ、それだけで 3 s を超えるケースは cold run 1 回（Runs = 1）。表に Runs 列あり。breakdown は best of 3、「one more row」は単発と明記。 |
| m5 raw-walk の注記 | 解消 | Summary の行内に「Favourable to ooxml-rs: the walk resolves neither shared strings nor styles」。 |
| m6 アロケータ | 解消 | realloc は `add(new)` を先に、`sub(old)` を後に。`reallocs` を別に数え、表は「5.29 (+ 0.10)」。 |
| m7 CI と clippy の記述 | 解消 | Method の Harness 節。実測でも clippy は exit 0 でした。リスクを受け入れることと、逃げ道（`--exclude ooxml-bench`）も記載。 |
| m8 推奨順 | 解消 | 「Recommended order of work」（row index → 圧縮レベル → lazy `sheetData` → compact tree → census・pptx）。row index の置き場所（`SheetContext`／`Workbook`）、lazy `sheetData` だけがメモリ目標に届くことも明記。 |

Summary の数値を表から再計算した結果:

| 記述 | 再計算 | 判定 |
|---|---|---|
| raw-walk 5.2×, 19× memory | 3.33 / 0.6456 = 5.16、1.48 GiB / 77.2 MiB = 19.6 | OK |
| Open 5.4×, drop 1.2 s | 3.51 / 0.6456 = 5.44、breakdown の drop は 1.20 s | OK |
| Write ≈ 310× at 100k, 1M 下限 ≈ 1.0 h | 35.88 / 0.11488 = 312、3587.52 s | OK |
| edit 1,000: 4.4×, 100: 1.0× | 29.30 / 6.72 = 4.36、6.88 / 6.81 = 1.01 | OK |
| parse 4.0〜5.6×, drop 46〜80% | breakdown 表の列そのもの | OK（ただし nit 4） |
| passthrough 0.36〜2.6 GB/s | 359〜2633 MB/s | OK |
| spike: 2.6× fewer allocations, 5× less heap | 5.29 / 2.05 = 2.58、(45〜50) / (8.6〜9.7) ≈ 5.2 | OK |

## Findings

### n1（nit）Method の「29×」が run 1 の値のまま

- L95-96 に「`cell(0, 0)` costs 29× more at 10× the rows」とあります。
- run 3 の表（L251-253）では、1k→10k 行が 71.6 µs → 1.77 ms で 24.7×、10k→100k 行が 1.77 → 20.27 ms で 11.5× です。私の再現では 11.9× と 34.8× でした。
- 「Every number comes from one run (run 3)」と食い違います。また、10k→100k はほぼ線形なので、「faster than linearly」の根拠としても弱いです。
- **修正案。** 下限である根拠は、上の m1 の行で示した比較の方が確実です。「10k から外挿した 1 呼び出しコストは 100k 行で約 13 ms、実測は 20〜44 ms」と書き換えてください。

### n2（nit）`lib.rs` の crate doc が古い

- `crates/ooxml-bench/src/lib.rs` L6-7 に「plus the heap peak of the first run」とあります。
- 実装と `Sample::peak_bytes` の doc は「warm-up run（または単発の cold run）」に変わっています。合わせてください。

### n3（nit）圧縮レベル 1 の効果の倍率が 3 通りある

- Summary の推奨順 2 は「about 14× faster」（L53）、hotspot 3 は「15× faster」（L335）です。
- 一方、`to_bytes` 全体で見ると 1.06 s が約 0.03 + 0.07 ≈ 0.1 s になるので、約 10× です。
- 「deflate が 15×、save 全体で約 10×」のように、どちらの比かを書き分けてください。

### n4（nit）run によって大きく動く比率を固定値で書いている

- drop / parse は run 3 で 46〜80% でした。私の再現では 40〜94% です（100k で 94%）。
- calamine の 1M は Median 645.6 ms、Min 374.1 ms と 1.7 倍開いています。そのため Open 5.4× と raw-walk 5.2× は、Min 基準なら約 9× になります。比較相手の ooxml 側は cold run 1 回です。
- 結論（allocation と drop が支配的で、calamine との差は数倍）は変わりません。Summary で「drop は parse の約半分から同程度（run により変動）」「1M の calamine 比は 5〜9×（median／min）」のように幅を見せると、読み手が数値に過度に依存しなくなります。

## 判定

- converged: **yes**
- Accepted after Minor only: **yes**（残りは nit 4 件のみ。直さずにマージしても構いません。）
