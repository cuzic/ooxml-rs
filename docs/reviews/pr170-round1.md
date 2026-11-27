# PR #170 レビュー（Opus, round 1）

対象: impl/xlsx-js-compat @ 468902af742c8bddbfa2d4fc4c39d60be62b00e2

## 結論（先に）
main マージで CellOp を union した部分（Formula 2 タプル、DynamicFormula 維持、Date/ArrayFormula/SharedFormula 追加）は、コードを読んだ結果とプローブ 20 件ほどの挙動で正しいことを確かめた。マージ文が「復元した」と書く DynamicFormula の処理 6 点（write_cell の t=array と ref、census 登録、dependents、検証、formula_set_changes、shared-master 拒否）はすべてある。Err ⇒ byte-identical 違反と、Ok なのに参照がぶら下がる例は見つからなかった。ただし新しい公開 API の shared formula 系 3 つ（set_shared_formula / set_shared_formula_master / set_shared_formula_member）には Rust 側のテストが 1 本もなく、hand-off と全置換の経路が守られていないため Major とする。converged: no。

## 前ラウンドの解消状況
初回レビューのため該当なし。

## Findings
### M1（Major）: 新しい公開 API の set_shared_formula / set_shared_formula_master / set_shared_formula_member に Rust 側のテストが無い
- 根拠: `grep -rn set_shared_formula crates/` で見つかる呼び出しは phase4.rs の set_sparse_shared_formula_row だけ。新しい計画経路が守られていない: `replaced_groups` による全置換（edit.rs の plan_cell_writes）、別グループの master に member を書いたときの hand-off、`next_shared_formula_index` の si 割り当て、Err ⇒ byte-identical。プローブでは正しく動く。P4 の全置換は si=1 で ref=B1:D1、P6 の hand-off では C2 が si=0 の master（ref=C2:D2、式 B1+1）になる。P3 の部分重複は SharedFormulaMaster で拒否され、出力はバイト一致。P14 の翻訳不能も拒否され、出力はバイト一致。ただし、どれも変異を入れたときに落ちるテストが無い（AGENTS.md の「新しいコードには変異を最低 1 つ」に反する）。
- 修正案: 上の 4 経路と Err ⇒ byte-identical を phase4/phase5d に追加する。replaced_groups の `&= covered` を `|=` に変える変異で落ちることを確かめる。

### m1（Minor）: 動的配列の spill 範囲に値セルがあっても、set_array_formula(dynamic=true) が Ok を返す
- 根拠: B6=7 を置いてから B5 に `_xlfn.SEQUENCE(3)`、範囲 B5:B7 を書くと Ok（P19）。plan_cell_writes の ArrayFormula の重複検査は census の数式セルしか見ていない。Excel は開いたときの再計算で #SPILL! を出す（ファイルは壊れない）。静的配列でも同じく、範囲内の値セルが残る（P8）。
- 修正案: 範囲内の stored cell を拒否するか、意図した挙動として ADR-0021/0022 か PR 本文に記録する。

### m2（Minor）: set_shared_formula_master の ref が、他の数式セル（別グループの member や配列）を含んでも Ok を返す
- 根拠: P5（C1:C3 の master が si0 のグループ範囲の中）と P13（B1:E1 の master の範囲に静的配列 C1:D1 がある）。読み戻しは正しく、Excel の読み込みでも問題は出ない見込み。ただし ADR の拒否表に記載が無い。
- 修正案: 許容するのであれば、そのことを post-acceptance note に書く。

### n1（nit）: content_style_same にある Date/ArrayFormula/SharedFormula の「never skipped」腕には Journal からは到達しない
- 根拠: journal/op.rs::plan_call はこれらの op を作らない。安全側の fail-safe なので問題は無いが、到達しないことをコメントにはっきり書くとよい。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 2 / nit 1）

補足（範囲）: JS/wasm 側（ADR-0021 の受入条件との対応、properties/distribution/mutation の主張）は今回のラウンドでは確かめきれていない。round 2 で扱う。干渉 PR #169/#144 との突き合わせも未実施。CI は全緑（gh pr checks 170）。
