---
name: merge-pr
description: レビューが収束した ooxml-rs の PR を安全にマージする（収束した SHA 以降のコミット確認、CI 全緑の確認、積み上げ PR の付け替え、head を固定したマージ）。「マージして」「収束したので入れて」と頼まれたときに使う。
---

# 収束した PR のマージ

Claude（または owner）が行う。main の取り込みや衝突解消は自分でせず、Codex に戻す。

## 前提（1 つでも欠けたら止める）

- 最新ラウンドのレビュー（PR ブランチにコミット済みの `docs/reviews/prNN-roundK.md`）が
  converged: yes で、`対象: <branch> @ <sha>` が書かれている。
- PR の base が main: `gh pr view NN --json baseRefName --jq .baseRefName` が `main`。
  積み上げ PR は、下の PR がマージされて付け替わってから扱う。
- レビュアーが「マージ前に直して」と書いた項目があれば、作者（Codex）が直している。

## 手順

1. **収束後のコミットを確認する**。`<sha>` はレビューファイルの対象 SHA:

   ```sh
   git fetch origin && git log --first-parent --oneline <sha>..origin/<branch>
   ```

   許されるのは、レビューファイルのコミット、指摘 ID を挙げた Minor/nit の修正、main の取り込みだけ。
   それ以外（新機能、Major 級の変更、説明の無いコミット）があれば、マージせず差分だけの短い再レビューを行う。
2. **main との関係を確認する**。CONFLICTING、または前回の取り込み以降に main で同じ crate や生成物が
   変わっているなら、Codex に main の取り込みを依頼する（`codex-task`）。取り込み後は手順 1 からやり直す。
3. **CI 全緑と head を確かめてからマージする**。`gh pr merge` は CI が赤でも通るので、必ずこのゲートを通す:

   ```sh
   PR=NN
   SHA=$(gh pr view "$PR" --json headRefOid --jq .headRefOid) || exit 1
   gh pr checks "$PR" --watch --interval 30 >/dev/null 2>&1   # 待つだけ。判定は下で行う
   n=$(gh pr checks "$PR" --json bucket --jq 'length') || { echo "checks を取得できない/未完了/失敗"; exit 1; }
   bad=$(gh pr checks "$PR" --json bucket --jq '[.[] | select(.bucket != "pass")] | length') || exit 1
   [ "$n" -ge 2 ] && [ "$bad" -eq 0 ] || { echo "CI が全緑でない（pass 以外 $bad / 全 $n）"; exit 1; }
   [ "$(gh pr view "$PR" --json mergeable --jq .mergeable)" = MERGEABLE ] || { echo "MERGEABLE でない"; exit 1; }
   ```

   - checks が 0 件（push 直後の "no checks reported"）、取得失敗、`UNKNOWN` はすべて止まる側に倒れる。
     少し置いてからやり直す。
   - `n -ge 2` は現在の CI のジョブ数（Rust と python-pptx）。ジョブを増減したらここも直す。
4. **積み上げ PR を先に付け替える**。このブランチを base にしている PR を main に向ける:

   ```sh
   for n in $(gh pr list --base <branch> --json number --jq '.[].number'); do gh pr edit "$n" --base main; done
   ```

   GitHub の自動付け替えに頼らず、明示的に付け替えてからブランチを消す（以前、付け替え前の削除で
   積み上げ PR が close されたことがある）。
5. `gh pr merge "$PR" --merge --match-head-commit "$SHA"`。
   ゲートの後に head が動いていればマージは拒否されるので、手順 1 からやり直す。
   ブランチ削除は、付け替えが済んでから行う。
6. CI が落ちた原因が PR と無関係（flaky など）なら、マージせずに原因を直す PR を別に作る。

## 報告

マージした PR、付け替えた PR、残った follow-up（Minor）を 1 行ずつ。
