---
name: review-pr
description: ooxml-rs の PR または ADR を敵対的にレビューし、docs/reviews/prNN-roundK.md（ADR は docs/adr/NNNN-opus-review-roundK.md）を PR ブランチにコミットする。PR のレビュー、レビューラウンド、収束判定を頼まれたときに使う。
---

# PR / ADR の敵対的レビュー

目的は **Blocker と Major を見つけること**。体裁や好みの指摘を増やすことではない。

## 入力

- PR 番号（または ADR 番号）とラウンド番号 K。前ラウンドのレビューファイルがあれば必ず読む。
- 根拠になる ADR（`docs/adr/`、Accepted のもの）と、そのレビュー記録。

## 手順

1. **独立したコピーで作業する**: `git clone` でスクラッチ領域に複製し、`gh pr checkout NN` する。
   レビューする head の SHA（`git rev-parse HEAD`）を控える。
   作業中の worktree を `cp -a` しない（git のメタデータを共有して他の作業を壊す）。
   他のエージェントのプロセスを `pkill` で止めない。
2. **前ラウンドの解消確認**: 各指摘が直ったかをプローブで確かめる。説明文ではなく挙動で判定する。
3. **壊しにいく**: ADR の規則ごとに、規則を破る入力を作る。重点:
   - `Err` を返したのに出力が変わる（Err ⇒ byte-identical 違反）
   - `Ok` なのに参照がぶら下がる、id が重複する、別の part を壊す
   - 途中失敗で半端な状態が残る（poisoning・ロールバック）
   - 局所性（触っていない span が変わる）
   - 名前空間・`mc:AlternateContent`・`xml:space`・エスケープ（`&` `<` `"`、`_xHHHH_`）
4. **機械的検証は結果を読む**: 変異テスト・コーパス走査・fuzz は CI か既存スクリプトの出力を使う。
   足りないときだけ手書きの変異を数個入れ、テストで落ちるかを見る。長時間の実行で待たない。
   プローブや変異はコミットしない（レビューファイルだけをコミットする）。
5. **他の open PR との干渉**: 同じファイル・生成物・公開名に触る PR がないか `gh pr list` で確認する。

## 重大度

| 等級 | 基準 |
| --- | --- |
| Blocker | 正常系で壊れたファイルを書く、データを黙って失う、安全性の穴 |
| Major | 特定の入力で壊れたファイルや ぶら下がり参照、`Ok` での黙った誤り、拒否で出力が変わる、ADR の規則違反、重要な経路にテストが無い |
| Minor | 安全側の過剰拒否、文書の不一致、テストの穴（挙動は正しい） |
| nit | 表現・コメント |

## 出力

`docs/reviews/prNN-roundK.md`:

```markdown
# PR #NN レビュー（Opus, round K）

対象: <branch> @ <レビューした head の SHA>

## 結論（先に）
<2〜4 文。converged: yes/no と理由>

## 前ラウンドの解消状況
<指摘ごとに 1 行>

## Findings
### M1（Major）: <一文>
- 根拠: <再現手順・ファイル:行・プローブ>
- 修正案: <一文>

## 判定
converged: yes/no（Blocker n / Major n / Minor n / nit n）
```

**converged: yes** は Blocker と Major が 0 のとき。Minor は follow-up に回してよい。

## 届け方

作者（Codex）とマージ手順が読めるように、レビューファイルを **PR ブランチにコミットして push する**
（このファイルだけの docs コミット。メッセージは `docs: PR #NN review round K`）。
push が拒否されたら `git pull --no-rebase` してから push し直す（force-push しない）。
ADR のレビューも同じく、その ADR の PR ブランチに入れる。

最後の返答では、結論・判定・指摘を 1 行ずつと、push したコミットの SHA を返す。
