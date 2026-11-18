---
name: merge-pr
description: レビューが収束した ooxml-rs の PR を安全にマージする（head の固定、収束した SHA 以降の全コミットと衝突解消の確認、CI 全緑の確認、マージ後の積み上げ PR の付け替え）。「マージして」「収束したので入れて」と頼まれたときに使う。
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

シェルの変数は Bash の呼び出しをまたいで残らない。各コードブロックは 1 回の呼び出しで実行し、
先頭の `PR` `R` `SHA` には**リテラルの値**を書く（手順 3・4 の `SHA` は手順 1 で表示された値を書き写す）。

1. **マージする head を固定し、収束後のコミットをすべて確認する**。`R` はレビューファイルの対象 SHA:

   ```sh
   PR=NN; R=<reviewed sha>
   SHA=$(gh pr view "$PR" --json headRefOid --jq .headRefOid) || exit 1; echo "SHA=$SHA"
   git fetch origin || exit 1
   git merge-base --is-ancestor "$R" "$SHA" || { echo "レビューした SHA が head の祖先でない"; exit 1; }
   git log --no-merges --oneline "$R..$SHA" --not origin/main     # PR 側の全コミット（マージの第 2 親も含む）
   for m in $(git log --merges --format=%H "$R..$SHA" --not origin/main); do git show --remerge-diff --stat "$m"; done  # 手書きの衝突解消
   ```

   - 列挙されたコミットとして許されるのは、レビューファイルのコミットと、指摘 ID を挙げた Minor/nit の修正だけ。
   - マージコミットの `--remerge-diff` は空であること。ただし生成物の再生成だけなら CI の `codegen --check` で可。
   - それ以外（新機能、Major 級の変更、説明の無いコミット、手書きの衝突解消）があれば、マージせずに
     `$R..$SHA` の差分だけを短く再レビューする。
2. **main との関係を確認する**。CONFLICTING、または前回の取り込み以降に main で同じ crate や生成物が
   変わっているなら、Codex に main の取り込みを依頼する（`codex-task`）。取り込み後は手順 1 からやり直す。
3. **CI 全緑を確かめる**。`gh pr merge` は CI が赤でも通るので、必ずこのゲートを通す。
   CI は 10 分近くかかるので、Bash ツールの `timeout` を 600000 にするか、バックグラウンドで実行して完了通知を待つ:

   ```sh
   PR=NN; SHA=<手順 1 の SHA>; : "${PR:?}" "${SHA:?}"
   gh pr checks "$PR" --watch --interval 30 >/dev/null 2>&1   # 待つだけ。判定は下で行う
   n=$(gh pr checks "$PR" --json bucket --jq 'length') || { echo "checks を取得できない（0 件など）"; exit 1; }
   bad=$(gh pr checks "$PR" --json bucket --jq '[.[] | select(.bucket != "pass")] | length') || exit 1
   [ "$n" -ge 2 ] && [ "$bad" -eq 0 ] || { echo "CI が全緑でない（pass 以外 $bad / 全 $n）"; exit 1; }
   [ "$(gh pr view "$PR" --json mergeable --jq .mergeable)" = MERGEABLE ] || { echo "MERGEABLE でない"; exit 1; }
   ```

   - checks が 0 件（push 直後の "no checks reported"）、取得失敗、pending、`UNKNOWN` はすべて止まる側に倒れる。
     少し置いてからやり直す。
   - `n -ge 2` は現在の CI のジョブ数（Rust と python-pptx）。ジョブを増減したらここも直す。
4. **固定した head でマージする**（`--delete-branch` は付けない）:

   ```sh
   PR=NN; SHA=<手順 1 の SHA>; : "${PR:?}" "${SHA:?}"
   gh pr merge "$PR" --merge --match-head-commit "$SHA"
   ```

   `SHA` が空だと gh は head を固定しないので、`${SHA:?}` で必ず止める。
   手順 1 の後に head が動いていればマージは拒否されるので、手順 1 からやり直す。
5. **積み上げ PR を付け替えてから、ブランチを消す**。このブランチを base にしている PR を main に向ける:

   ```sh
   for n in $(gh pr list --base <branch> --json number --jq '.[].number'); do gh pr edit "$n" --base main; done
   git push origin --delete <branch>
   ```

   GitHub の自動付け替えに頼らず、明示的に付け替えてからブランチを消す（以前、付け替え前の削除で
   積み上げ PR が close されたことがある）。マージが拒否されたときは付け替えない。
6. CI が落ちた原因が PR と無関係（flaky など）なら、マージせずに原因を直す PR を別に作る。

## 報告

マージした PR、付け替えた PR、残った follow-up（Minor）を 1 行ずつ。
