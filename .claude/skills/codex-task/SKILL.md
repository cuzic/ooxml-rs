---
name: codex-task
description: ooxml-rs の実装・修正・main の取り込みと衝突解消を Codex に渡すためのタスク仕様（GitHub Issue または PR コメント）を書く。実装を Codex に任せたい、レビュー指摘の修正を依頼したい、と言われたときに使う。
---

# Codex へのタスク仕様

Codex は AGENTS.md を読むが、この会話の文脈は知らない。仕様だけで作業が閉じるように書く。
判断が要る点は先に決めて書くか、「決めずに Open questions に書いて止める」と明示する。

## 書く場所

- 新しい作業: `gh issue create` で Issue にする（タイトルは `[codex] <短い要約>`）。
- 既存 PR への修正・main の取り込み: その PR に**コメント**として書く（見出しを `## [codex] タスク仕様` にする）。
  根拠のレビューファイルは PR ブランチにコミット済みのもの（`docs/reviews/prNN-roundK.md`）を指す。

書いたら、owner に Codex への 1 行の指示を返す。例:
「PR #NN の worktree で: PR #NN の最新の `[codex] タスク仕様` コメントをやって」
「新しい worktree で: Issue #NN をやって」

## テンプレート

```markdown
## [codex] タスク仕様

### 目的
<1〜2 文。何ができるようになるか>

### 根拠
- ADR: docs/adr/NNNN-....md の §x（Accepted）
- レビュー: docs/reviews/prNN-roundK.md の M1, m2（修正タスクの場合）

### ブランチ
- 新規: `git worktree add -b <branch> <path> origin/<base>`（積み上げなら base を明記し、PR タイトルに "stacked on #NN"）
- 既存 PR: `<branch>` に追加コミット（merge で追従、force-push しない）

### スコープ
- やること: <箇条書き。指摘 ID ごとに 1 行>
- やらないこと: <範囲外を明記>
- 触ってよいファイル: <crate / ディレクトリ>

### 受け入れ条件
- AGENTS.md の完了の定義（test / clippy / codegen --check）
- 追加するテスト: <指摘ごとの再現テスト、Err ⇒ byte-identical のテスト>
- 変異: <落ちるべき変異の例>
- 計測（該当時）: <コーパス件数・性能の数値と、比較対象>

### 報告
PR 本文に「指摘 → 修正」の表と Status / next steps を書く。コミットメッセージに指摘 ID を入れる。マージはしない。
```

## 粒度の目安

- 1 タスク = 1 PR = レビュー 1 回で見られる量（数百行〜千行台）。
- レビュー指摘の修正は、Minor/nit だけなら CI 通過で完了とし、再レビューを求めない。
  ただし Minor 修正のコミットには指摘 ID を書かせる（merge-pr が収束後のコミットを照合するため）。
