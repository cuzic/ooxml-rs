# PR #47 レビュー（Opus, round 1）

対象: `test/fuzz-docx` @c6ff9a1（#36 @66e1e07 に積む。PR 固有の変更は c6ff9a1 の 13 ファイル）。
検証: 独立した `git clone` 3 つ（head、head＋origin/main 48aa8f0 のマージ、fuzz 用）。Rust 1.99.1、nightly 1.102.0、cargo-fuzz 0.13.2。

## 結論（先に）

**ターゲットの oracle は「書いたことは正しく書かれる」側については健全だが、「書いてはいけないときに書かない」側の oracle がない。**
自己比較のような空振りはなかった（`before_main` は再オープン直後の元バイト、`after` は dirty なツリーの直列化で、別物を比べている）。
しかし ooxml-docx に手で入れた 11 個の変異のうち **8 個が生き残った**。特に ADR-0004 の安全側の契約
—— 文書レベルの書き込みゲート（W1–W3）、ランのゲート（R1–R4: フィールド内・変更履歴内など）、`append_paragraph` の A2 / フィールド内拒否 ——
を外す変異は**どれも検出されない**。スモークの入力の 17%（147/869）はゲート付き文書で、拒否経路は十分に通っているのに、
oracle が「拒否されるべきだった」を見ていないためである。fuzz 対象の中心機能が ADR-0004 の拒否規則である以上、これは Major。

- Major 1 件（M1）、Minor 4 件（m1–m4）、Nit 2 件
- 判定: **converged: no**

## 実行結果

| 項目 | 結果 |
|---|---|
| `cargo +nightly fuzz run docx_edit`（seed 387 件、8 workers × 約 250 秒、ASan） | 約 1 万回、cov 5,176、**失敗なし**。1 worker あたり 4–5 exec/s（各編集ごとの保存・再オープンが支配的） |
| スモーク `OOXML_SMOKE_ITERS=30`（release） | pass、5 s |
| 編集結果の分布（スモーク 30 iters、計測用の一時パッチ） | open 1,525 件中 Ok 869（うちゲート付き 147）。`set_run_text` Ok 372、`append_paragraph` Ok 211。拒否の内訳: 無効文字 約 40%、範囲外 index、W1/W2/W3、R1/R4、`cannot position` |
| head＋origin/main | #36 と同じ `pptx.rs:163` の E0308（#36 の m10）。直すと `OOXML_FUZZ_ALL=1` スモーク 6/6 pass、clippy `-D warnings` 警告なし |
| `fuzz/Cargo.lock` | `cargo metadata --locked` が head・マージ後とも通る（ooxml-docx の追加は正しく反映） |

### 手で入れた変異（ooxml-docx、スモーク `OOXML_SMOKE_ITERS=30`）

| # | 変異 | 結果 |
|---|---|---|
| 1 | `set_run_text` の `self.write_gate()?` を削除 | **生存** |
| 2 | `set_run_text` の `r.check_settable()?` を削除（フィールド内・変更履歴内・mc 内のランに書く） | **生存** |
| 3 | `append_paragraph` の `self.write_gate()?` を削除 | **生存** |
| 4 | `append_paragraph` の A2（`ends_with_section_break`）を無効化 | **生存** |
| 5 | `set_t` が `xml:space="preserve"` を付けない | **生存** |
| 6 | `write_pieces` が子を消した後に Tab で `Err` を返す（部分変更のまま拒否） | 検出（refused edit changed the package） |
| 7 | `write_pieces` が `w:rPr` も消す | **生存** |
| 8 | `append_paragraph` が誤った index を返す | 検出 |
| 9 | `Br` を `w:tab` として書く | 検出（reads back） |
| 10 | `append_paragraph` のフィールド内（`open_fields_at_end`）検査を無効化 | **生存** |
| 11 | no-op 判定が `lacks_needed_preserve` を見ない | **生存** |

検出できた 3 つは「拒否なら無変更」「読み戻し」「戻り値」の oracle によるもので、それらは期待どおり働いている。
生き残った 8 つはどれも、出力が well-formed で読み戻せ、変更がランの span の中（または 1 か所の挿入）に収まる。

## oracle の健全性（問われた 4 点）

1. **拒否 ⇒ パッケージ不変**: 健全。`before` / `after` とも `doc.to_bytes()` で、dirty なツリーは再直列化されるので、
   部分変更して `Err` を返す実装は検出される（変異 6）。
2. **`set_run_text` はランのバイト範囲の中だけを変える**: 健全。span は再オープン直後の文書から取るので `before_main`（= 元バイト）と座標が一致し、
   `read_part` は dirty なら直列化結果を返す（`Package::read_part`）ので自己比較にはならない。共通接頭辞 ≥ start かつ共通接尾辞 ≥ len − end の判定も正しい。
   ただし「範囲の中なら何でもよい」ので、ランの中の保持すべきもの（`w:rPr`、`xml:space`）は見ていない（m1）。
3. **`append_paragraph` は 1 か所の挿入**: 健全（接頭辞＋接尾辞 ≥ 元の長さ、かつ長さが増える）。挿入位置（body 直下の `w:sectPr` より前、`w:body` の中）は見ていない（m2）。
4. **他の part はバイト一致**: 健全。最後に zero-edit 保存（`base`）と最終出力を main 以外 L1 比較し、途中の再オープンを経ても推移的に成り立つ。

## 所見

### M1 [Major] 「拒否すべきなのに Ok」を検出する oracle がない（ゲートの迂回が素通り）

**場所**: `crates/ooxml-robustness/src/docx.rs` 112–158 行（編集ループ）

ハーネスは `write_gate()` と `check_settable()` を読むが、編集結果との整合を assert しない。変異 1–4・10 が生き残った。
ADR-0004 の Phase0 の安全性はこれらの拒否規則で成り立っており、壊れた入力（payload で settings.xml・document.xml を置き換える）に対して
ゲート判定とランの分類が一貫しているかは、まさにファザーに見てほしい性質である。

**修正案**（各編集の前に予測を立て、結果と突き合わせる）:

- `let gate = doc.write_gate();` が `Err` なら、`set_run_text` / `append_paragraph` は**同じエラー**で `Err`（`Ok` なら panic）。
- `set_run_text(p, r, text)`: 段落・ランが存在し、ゲートなしなら、`check_settable()` の `Err` ⇔ 同じ `Err`、`map_text` が通らない文字（`< ' '` で `\t\n\r` 以外、U+FFFE/FFFF）⇒ `InvalidXmlChar`、
  それ以外は `Ok` でなければならない（過剰な拒否も検出できる）。予測は ooxml-docx の公開 API（`write_gate`、`check_settable`、`paragraph(p)?.run(r)`）と
  ハーネス側の文字判定だけで作れる。
- `append_paragraph`: ゲートなしで `Ok` のとき、再オープン後の最後の段落の直前の段落が `w:pPr/w:sectPr` を持ち、かつ body 直下に `w:sectPr` がない形でないこと（A2）、
  元の最後の段落の最後のランが `in_field()` でないこと（フィールド内）を確認する。逆方向（`SectionBreakAtEnd` / `InsideField` で拒否されたなら、その形であること）も同様。

これで変異 1–4・10 は検出されるはず（ゲート付きの open が 147 件、フィールド内ランへの `set_run_text` 拒否が 60 件以上あるので頻度は十分）。

### m1 [Minor] ランの中で保持すべきもの（`w:rPr`、`xml:space`）を見ていない

**場所**: `docx.rs` 123–133 行

変異 5・7・11 が生存。ADR-0004 §3 は「`w:rPr` と全属性を保持」「端や連続の空白には `xml:space="preserve"`」と定めるが、
`Run::text()` は `xml:space` を見ずに読むので読み戻しでは分からず、局所性の検査も span の中なので通る。
**修正案**: 成功した `set_run_text` の後、編集前後のランで (a) `w:rPr` 子要素の生バイト（span で切り出し）が一致、(b) ランの属性が一致、
(c) テキストに先頭・末尾・連続の空白を含む各 `w:t` に `xml:space="preserve"` がある、を assert する。

### m2 [Minor] `append_paragraph` の挿入位置を見ていない

**場所**: `docx.rs` 136–149 行

「1 か所の挿入」は満たすが、body 直下の `w:sectPr` の後や `w:body` の外への挿入でも通る（新しい段落はどこにあっても index 上は最後になりうる）。
**修正案**: 挿入点（共通接頭辞の長さ）が `w:body` の span の中で、body 直下の `w:sectPr` があればその開始より前であることを assert する。

### m3 [Minor] 生成テキストの約 4 割が無効文字で拒否される

`ops.string()` が制御文字を高頻度で出すため、`set_run_text` / `append_paragraph` の約 40% が `InvalidXmlChar` で終わる（上の分布）。
拒否経路の検査としては有益だが、成功経路（T2 の局所性・読み戻し）の試行が減る。ASan 下で 4–5 exec/s と遅いターゲットなので、
半分の確率で印字可能文字＋`\t\n\r`＋空白（端・連続を含む）だけから作ると、m1 の preserve 検査にも効く。

### m4 [Minor] 検証対象が古い main（8c54366）で、#36 と同じ main マージ後の build 修正が要る

文書の「対象: main @8c54366」以降、#40（core のサイズ上限・BOM 等）と #14（`from_bytes_with`）が ooxml-core に入った。
#36 で main をマージして直した後（#36 の m10）、この PR にも取り込み、docx_edit を数分回し直して文書の数値を更新する。
現状この PR の差分（main 比）には main 由来の無関係な 50 ファイルが見えるが、#36 が main をマージすれば消える。

### Nit

1. `max_steps` を main part 512 KiB 超で 4 回に落とした判断は妥当（fixture `poi-bug59058.docx` の再オープンが支配的）。ただし文書の「12 回で ASan 10 秒超」は `-timeout=10` の CI 設定ちょうどなので、512 KiB 未満でも数百 KiB の part に payload で置き換わると近づく。`-timeout` との関係を README に一言。
2. `read_all` の `let _ = (doc.write_gate(), ...)` は読みの panic 検査としては十分だが、M1 の予測に使うなら戻り値を捨てない形にまとめるとよい。

## 判定

**converged: no** — M1（ゲート・ランの拒否規則と結果の整合 oracle）が必要。m1・m2 は同じ箇所への小さな追加なので同時に推奨。
直したら変異 1–5・7・10・11 で再確認すること（スモーク `OOXML_SMOKE_ITERS=30` で 1 変異 20 秒程度）。
