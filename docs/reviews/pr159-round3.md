# PR #159 レビュー（Opus, round 3）

対象: impl/xlsx-ej-j0a @ 921c884bc3916ec33f7e63b6ce9040b0ba1845fa

## 結論（先に）
converged: **yes**（Blocker 0 / Major 0 / Minor 1 / nit 1）。round 2 の M1 は推奨どおりの方式で解消した。
差分は `journal/mod.rs`、`tests/fixed.rs`、`tests/oracle1.rs` の 3 ファイル（+43/−9）だけなので、この範囲に絞って見た。

## 前ラウンドの解消状況
- M1: 解消。`snapshot_cell` が失敗した場合だけ、op の前に sheet part を直列化して保存し、
  `apply_seq` が成功した後の直列化と比べる（`journal/mod.rs:240-262`）。
  - 同じなら no-op として受け付け、log・key・seq は通常どおり進める。セルは追跡しない（`tracked` に入らない）。
  - 違えば、これまでどおり `broken` にして `Internal` を返す。
  - R2 のループは追跡していないセルを飛ばす（`journal/mod.rs:303-306`）。
  - dirty フラグではなく直列化で比べる理由（no-op でも印が付く、値を戻しても印が残る）がコメントに書いてあり、妥当。
  - fixed_23 は、両側 `Ok`、`broken` にならない、後続の op が両側で一致する、P が等しい、をすべて確かめている。
    実装者の mutant（常に `broken` にする）は fixed_23 で落ちる。
  - 生成器は out-of-order シートへの op を再び生成する（`oracle1.rs:208`）。報告では 5,000 ケースで green。

## 新しい指摘
- **m1** `journal/mod.rs:313-316` `sheet_part_text` は part を読めないと `None` を返す。前後とも `None` だと
  「変化なし」と判定され、op が受け付けられる。part が読めないのに `apply_seq` が成功する経路は今のところ無いはずだが、
  起きた場合は変化を見落とす。前後どちらかが `None` なら `broken` 側に倒すほうが安全。J0b で直せばよい。
- **n1** `journal/mod.rs:303-306` 追跡していないセルを飛ばす `continue` は、通常の op で snapshot を取り損ねた
  バグも黙って隠す。`debug_assert!(snapshot_error_path)` 相当の確認を足すとよい（J0b で可）。
