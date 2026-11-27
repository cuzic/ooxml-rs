# ADR-0023 レビュー（Opus, round 5）

対象: `docs/adr/0023-xlsx-range-io-and-python-binding.md` revision 5。ブランチ plan/xlsx-rpa-foundation @ `0a743e80aa91581f20fb1dbc0ab3ef0a36715a86`（revision 5 の改稿は `0a743e8`、round 4 の記録は `469782d`）。

照合したもの:

- 改稿の差分（`7b5e2fa..0a743e8`）。
- §5.3（effective formula text の定義）、§6.2、§11.4、§12、oracle 3。
- ADR-0024 §7.1（R1）と R3（`CalcPolicy::Deferred`）。
- `edit.rs` の `content_style_same`（R1 側の比較が plain な数式であること）。

## 結論（先に）

round 4 の M1 と m1 は、どちらも正しく解消した。identical-`Formula` の drop は、§5.3 が定める effective formula（shared のメンバーは master の式を翻訳したテキスト、anchor と通常の数式は自分のテキスト）とのテキストの等しさになった。kind と保存属性は比べず、R1 とは共有しないと明記している。検証できないメンバー（§5.3 で `Unavailable` になるもの）は drop しない。これは R1 が意図して apply すること（ADR-0024 §7.1、round 8 の M1）とも、sequential twin との一致とも矛盾しない。oracle 3 には、shared グループの一部と全体、配列の anchor の formula モード往復と、stale cache のケースが入った。

新しい Blocker/Major は無い。Minor 1 件（stale cache を書き戻すと数式が置き換わることを、利用者向けに明記する）と nit 1 件だけである。**converged: yes**（Status は Proposed のまま。Accept 判定はしない）。

## 前ラウンドの解消状況

- **M1（formula モードの同一判定）:** 解消。
  - 比較の対象は、§5.3 の `FormulaText::Text` と同じ定義である。読み取り（`read_range(formulas="formula")`）が返す `Formula.text` と、drop の比較が同じ関数に基づくので、shared 列の一部と全体、配列・data-table・dynamic の anchor の往復は、すべて drop される（バイト一致）。
  - 一部だけを覆う場合も、ガードの前に drop されるので、`SharedFormulaMaster` の「同じグループの 2 セル」の規則に当たらない。
  - 配列のメンバーは §5.3 で `None` になり、Python では `None` = 書かないので、ガードに届かない。
  - 検証できないメンバー（あいまいなグループ、master が無い、翻訳できない、census が blocked）は §5.3 で `Unavailable` になり、formula モードでは読み取りが `FormulaUnavailable` で失敗する。drop しないという規則とも整合する。
- **m1（セッション中に読む状態）:** 解消。述語は normalized View（保留中の strip はキャッシュ無し）を読む。sequential twin はその場で strip するので、どちらの経路でも同じ判定になる。oracle 3 にもこのケースが入った。

## Findings

### m1（Minor）: 前の呼び出しで入力を変えた後に value モードの古い値を書き戻すと、数式が定数に置き換わる。これは意図した挙動だが、利用者向けの説明が無い

- **根拠:** §6.2 と oracle 3 は「Writing back a stale cache therefore applies in both paths」と定め、両経路の一致としては正しい。ただし RPA でよくある 2 段階の流れでは、B1 の数式が黙って定数 2 になる:
  1. `read_range`（value モード）。
  2. `write_cells(A1)` で入力を変える（B1 = `A1*2` のキャッシュが strip される）。
  3. 1 で読んだ領域を `write_range` で書き戻す。

  同じ呼び出しの中で A1 と領域全体を書き戻す場合は、呼び出し前の状態で判定するので数式は残る。この違いは §11.4 から読み取れない。
- **修正案:** §11.4 に、この挙動と回避策（往復には `formulas="formula"` を使う、または読み直してから書き戻す）を 1 文書く。value モードのキャッシュ規則の doc にも同じ注記を付ける。

### n1（nit）: revision 4 の履歴と round 3 の応答表に、撤回した記述が残っている

- **根拠:** l.84（Revision 4）と l.1336（round 3 の応答、M2）に「identical formula writes drop via the shared report」が残っている。履歴としては正しいが、revision 5 で置き換えたことが、その行からは分からない。
- **修正案:** 「（revision 5 で effective-formula equality に置き換え）」と添える。

## 判定

converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 1）。Status は Proposed のまま（Accept 判定はしない）。
