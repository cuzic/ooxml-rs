# PR #165 レビュー（Opus, round 1）

対象: impl/xlsx-ej-p2w @ 805fa44
converged: yes

## 結論（先に）
Blocker・Major なし。P2 の変換は ADR §4.1 part B の条件（Base の `cellXfs` 数 ≤ i < 現在の数）でだけ起き、
live と replay は同じ `StyleXf` を `accept` → `apply_seq` → `set_cell_xf` で適用する（fixed 33 の
`rebuild()` 後のバイト一致と、`replay.rs` の `input_of` が `call` をそのまま運ぶことで確認）。
`Part::Base(i)` は Base 数で検査されるようになった（#163 m3 解消）。指摘は、ぶら下がった Base 参照の
非対称な拒否が文書とテストに固定されていないこと、POI 側にカバレッジの下限がないことの Minor 2 点と nit 3 点。

## 前ラウンドの解消状況
初回。関連する前 PR の指摘:
- #163 m3（`Part::Base(i)` をテーブル長で検査）: 解消。`base_ref` は `limit.min(t.count)` で比べ、
  `xf_content_tests` に session font 追加後の `Base(base.fonts)` → `DanglingReference`・バイト不変のテストがある。
- #164 m4（P2 の配線）: 解消。

## 検証したこと（プローブ。コミットしていない）
- `cargo test -p ooxml-xlsx --all-features journal`: 97 pass / 4 ignored。
- 変換条件: `append_inner`（`journal/mod.rs:250-278`）は `i >= base_counts.cell_xfs` かつ `i < cell_format_count()`
  のときだけ `xf_content` → `StyleXf`。`i >= now` は index のまま順次呼び出しが `StyleIndexOutOfRange` で拒否。
  `cellXfs` が index-stable でない Base では session `xf` が作られず `now == base` なので index のまま両側同じ拒否。
- キー: `StyleXf` は `ALL_STYLE`（6 グループ）、fixed 33 が 6 グループ全部と `Content` 無しを確認。
  `CellCall` に対するワイルドカード一致で `StyleXf` を取りこぼす箇所はない（`fields` と `apply_seq` だけ）。
- `JournalError::Unsupported` の削除: crate-private で公開契約なし。J0 の op 集合に残る part B op はない
  （P1 は #164 で受理、P3–P6 は `OpKind` に op 自体がない、P7/P8 は計画外）。
- POI oracle（`OOXML_J0B_POI_CASES=6` の短い実行）で変換に一時的な `eprintln!` を入れると、
  1,939 op 中 41 回 P2 が受理された（base 1〜201 の各種 fixture、`i = base + 0..2`）。session index は実際に通る。
- ぶら下がり: Base `xf` 2 の `borderId` を 9（範囲外）に書き換えた oracle fixture で
  `set_style_index(A1, 2)` → `set_font(A1, italic)`（session xf 4）→ `set_style_index(A2, 4)`:
  順次 `Ok(())`、Journal `Err(Op(DanglingReference { table: "borders", index: 9 }))`、Journal は不変（m1）。

## Findings

### m1（Minor）: ぶら下がった Base 参照から派生した session `xf` の非対称な拒否が、コードとテストに固定されていない
- 根拠: `crates/ooxml-xlsx/src/journal/mod.rs:259-266`。上のプローブのとおり Journal は `DanglingReference` で拒否し、
  順次 API は受理する。拒否は安全側で（順次側はぶら下がった `borderId` を持つセルを書く）、part B は oracle 1 (i)
  の対象外（F8）なので ADR 違反ではない。ただしこの事実は PR 本文の Known にしかなく、`append` の doc にも
  テストにもない。将来 `xf_content` が緩む・逆に厳しくなる変更を誰も検出できない。
- 修正案: `append` の doc（`mod.rs:196-199`）に「P2 の `set_style_index(i)` は `xf_content` が拒否する
  `xf`（ぶら下がった Base 参照、拡張内容）を `Op(..)` で拒否し、順次 API は受理しうる」と書き、上のプローブを
  fixed case（Journal の状態不変を含む）として足す。

### m2（Minor）: POI oracle に新しい op 群の受理数の下限がない
- 根拠: `crates/ooxml-xlsx/src/journal/tests/poi.rs:584-604` は fixture ごとの `ok`/`refused` 合計だけを数える。
  session index・`ClearValue`・dynamic・row/col が全部拒否されるか生成されなくなっても通る
  （oracle 1 側には `session_ok > 10` などの下限がある）。今は P2 が実際に通ることをプローブで確認したが、回帰は見えない。
- 修正案: `run_fixture` で session index（`i >= info.formats` で受理）、`ClearValue`、`DynamicFormula`、row/col の
  受理数を数え、`sequential_equals_shadow_on_the_poi_fixtures` の合計に緩い下限（例: それぞれ > 0、session > 10）を置く。

### n1（nit）: `revert` の doc が実際より強い
- 根拠: `poi.rs:407-423`。raw な値（`t="d"` など）の Base セルは `ClearValue` で値が消えるだけで、Base の `t`/`v`
  には戻らない。`PVal::Cache`（式なしの配列結果セル）では `ClearValue` は常に `ArrayResultCell` で両側拒否される。
  Base に無いセル（`None`）は今も `RemoveFormula` で、先に書いた値を消さない（snapshot が `None` なので復元も起きない）。
- 修正案: doc を「空の `c` は `ClearValue` で正確に戻る。raw 値は値を消すだけ、配列結果は拒否、欠けたセルは近似」に直す。

### n2（nit）: `append` の doc に削除した `Err(Unsupported)` が残っている
- 根拠: `crates/ooxml-xlsx/src/journal/mod.rs:197`。
- 修正案: `Err(Op)` だけにする。ADR のエラー表（`Unsupported { op }`）は P3–P6 が op 集合に入るとき用なので、
  その時に variant を戻す旨を `JournalError` の doc に一行書いてもよい。

### n3（nit）: `StyleXf` を呼び出し側が直接渡せる
- 根拠: `crates/ooxml-xlsx/src/journal/op.rs:124-128` は「caller は渡さない」と書くが、`append` は `StyleXf` を
  任意の `BaseStyleCounts` ごと受け取り、そのまま適用・記録する。crate-private なので実害はない。
  また `xf_content` が `accept` の sheet 検査より先に走るので、不正な sheet 番号と拒否される `xf` が重なると
  順次側（`NoSuchSheet`）と違う種類を返す。
- 修正案: `append_inner` で caller 由来の `StyleXf` を `debug_assert!`（または counts が `self.base_counts` と
  等しいことを確認）し、変換を `worksheet_by_index` の検査の後に置く。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 2 / nit 3）
