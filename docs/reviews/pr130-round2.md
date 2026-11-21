# PR #130 レビュー（Opus, round 2）

対象: docs/cleanup-after-merge @ ddf659681743de48d57037447b795c0692c13d0a

## 結論（先に）
round 1 の 6 件は文書の上ではすべて直っている。ただし、手順 7 の「作ったばかりの worktree は消さない」は
`origin/main` から切った新しいブランチを守れていない。Git の既定（`branch.autoSetupMerge=true`）で upstream が
`origin/main` になるため、「一度も push していない」の判定（upstream が設定されているか）をすり抜ける。
owner の hook（リポジトリ外）はこの判定をそのまま使っているので、作業を始めたばかりの worktree を消す。
converged: no（Major 1）。

## 前ラウンドの解消状況
- M1: 一部未解消。対象をマージしたブランチの worktree に絞り、untracked も変更として数えるようになった。
  ただし「push したことがあるか」の判定に穴がある（下の M1）。
- M2: 解消。`/tmp/ooxml-*` も git の状態（未 push・未コミット・untracked）と 24 時間の無更新で判定するようになった。
- m1: 解消。AGENTS.md に push 済みの確認（`git status`、`git log --branches --not --remotes`）が入り、worktree は対象外になった。
- m2: 解消。「自分で作った」target だけを消し、共有の target は消さないと書かれた。
- m3: 解消。手順 5 の後に行うことと、hook が `gh pr merge` と `git push origin --delete` の直後に動くことが書かれた。
- n1: 解消。`-d` が拒否したら `-D` を使わず報告する、と書かれた。

## Findings

### M1（Major）: `origin/main` から切ったばかりの worktree が「push 済みで、リモートのブランチが消えた」と判定される
- 根拠: `.claude/skills/merge-pr/SKILL.md` 77–81 行。条件は「マージしたブランチが checkout されていて、リモートのブランチは消えて
  おり、HEAD が `origin/main` に含まれ、clean」と「一度も push していないブランチは消さない」。
  プローブ: `git worktree add -b impl/probe-x <path> origin/main` の直後は、
  `branch.impl/probe-x.remote=origin`、`branch.impl/probe-x.merge=refs/heads/main`（autoSetupMerge の既定）、
  `refs/remotes/origin/impl/probe-x` は無い、HEAD は `origin/main` に含まれる、`git status --porcelain` は空。
  owner の hook は「`branch.<br>.remote` が無ければ未 push」「`refs/remotes/origin/<br>` が無ければリモートは消えた」と判定するので、
  この worktree は全条件を満たして `git worktree remove` され、ブランチも `-d` で消える。
  中身の損失は無いが、作業を始めたエージェントの作業ディレクトリが、別のセッションのマージを契機に消える。
  これは手順 7 が「消さない」と明記している場合そのもの。
  hook の `in_use` は、絶対パスで操作するエージェントのコマンドが呼び出しの合間に動いていないので守りにならない（手順 7 自身がそう書いている）。
- 修正案: 「push したことがある」を「upstream が同名のリモートブランチ（`branch.<br>.merge` が `refs/heads/<br>`）」と書き、
  hook も同じ判定にする。あわせて、worktree にもクローンと同じ 24 時間無更新の条件を入れると、判定の穴に対して二重に守れる。

### m1（Minor）: クローンの判定が stash と detached HEAD のコミットを見ていない
- 根拠: 同 82–84 行と AGENTS.md の新しい規則。`git log --branches --not --remotes` は `refs/stash` と、どのブランチにも
  乗っていない detached HEAD のコミットを数えない。クローンは `rm -rf` されるので、これらは戻らない（24 時間の無更新が
  条件なので起きにくい）。
- 修正案: `git log --all --not --remotes`（HEAD と stash を含む）が空であることを条件にする。

### m2（Minor）: hook は `/tmp/ooxml-*` の中で最初に見つけたクローンだけを調べ、ディレクトリ全体を消す
- 根拠: hook は深さ 2 までで最初の `.git` を 1 つ見つけ、そのクローンの判定が通れば `/tmp/ooxml-*` 全体を `rm -rf` する。
  同じディレクトリに 2 つ目のクローンや git でないファイルがあると、それらの状態と更新時刻は見ていない。
  手順 7 の文面は「ディレクトリごと」の判定方法を書いていない。
- 修正案: 中のクローンをすべて判定し、無更新の判定はディレクトリ全体に対して行う、と手順に書き、hook も合わせる。

## 他の open PR との干渉
`AGENTS.md` と `.claude/skills/merge-pr/` に触る open PR は無い。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 2 / nit 0）
