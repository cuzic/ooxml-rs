---
name: merge-pr
description: レビューが収束した ooxml-rs の PR を安全にマージする（レビューファイルのコミット、main の取り込み、CI 全緑の確認、積み上げ PR の付け替え）。「マージして」「収束したので入れて」と頼まれたときに使う。
---

# 収束した PR のマージ

## 前提

- 最新ラウンドのレビューが converged: yes（Blocker/Major 0）であること。
- レビュアーが「マージ前に直して」と書いた小さな項目があれば、先に作者（Codex）が直していること。

## 手順

1. **PR の worktree で**レビューファイル（未コミットなら）をコミットする。
2. `git fetch && git merge origin/main`（rebase しない）。衝突したら:
   - 生成物は `cargo xtask codegen` で再生成する。
   - コードの衝突が大きいときは自分で解かず、作者に戻す（解消後に短い再レビュー）。
3. ローカルで完了の定義（AGENTS.md）を確認してから push する。
4. **CI を待って全部 pass を確認する**。`gh pr merge` は CI が落ちていても通ってしまう:

   ```sh
   gh pr checks NN --watch --interval 30 >/dev/null 2>&1
   if gh pr checks NN | cut -f2 | grep -qv '^pass$'; then echo "NOT ALL PASS"; exit 1; fi
   [ "$(gh pr view NN --json mergeable --jq .mergeable)" = CONFLICTING ] && exit 1
   ```

5. **積み上げ PR を先に付け替える**。このブランチを base にしている PR を main に向ける:

   ```sh
   for n in $(gh pr list --base <branch> --json number --jq '.[].number'); do gh pr edit $n --base main; done
   ```

   付け替える前に `--delete-branch` すると、上に積んだ PR が自動で close される。
6. `gh pr merge NN --merge`（squash は履歴に revert などが混じったときだけ）。
   ブランチ削除は、付け替えが済んでから行う。
7. CI が落ちた原因が PR と無関係（flaky など）なら、マージせずに原因を直す PR を別に作る。

## 報告

マージした PR、付け替えた PR、残った follow-up（Minor）を 1 行ずつ。
