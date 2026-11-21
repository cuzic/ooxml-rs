# PR #130 レビュー（Opus, round 3）

対象: docs/cleanup-after-merge @ c19dbd9a21372249d23e516a586f933961a8b4f7

## 結論（先に）
round 2 の M1・m1・m2 はすべて解消した。worktree は「上流が同じ名前のリモートブランチ」のときだけ消す判定になり、
`origin/main` から切ったばかりの worktree は残る。owner の hook（リポジトリ外）も同じ判定に揃っており、dry-run で確かめた。
残りは hook 側の nit 2 件だけ。converged: yes。

## 前ラウンドの解消状況
- M1: 解消。手順 7 は `branch.<name>.merge` が `refs/heads/<name>` であることを条件にした。
  プローブ: `git worktree add -b <tmp> <path> origin/main` の直後は `branch.<tmp>.merge=refs/heads/main`、
  `refs/remotes/origin/<tmp>` は無い。hook はこの比較（`branch.$br.merge` != `refs/heads/$br` なら
  「never pushed under its own name」で残す）をリモートブランチ消滅の判定の後、マージ済み判定の前に行うので、この worktree は残る。
  dry-run でも、上流が main のままの既存 worktree が同じ理由で残されることを確認した（削除予定は 0 件）。
- m1: 解消。手順 7 と AGENTS.md の両方が `git log --all --not --remotes`（ブランチ、detached HEAD、stash）に変わった。
  hook も同じコマンドを使う。
- m2: 解消。手順 7 に「中のクローンすべてが条件を満たし、ディレクトリ全体も 24 時間無更新」が入り、
  hook は `find -maxdepth 3 -name .git` で見つけたクローンをすべて判定し、そのあとディレクトリ全体の無更新と使用中を見る。

## Findings

### n1（nit）: hook がディレクトリ内のクローンを探す深さが 3 まで
- 根拠: hook は一時ディレクトリ内の `.git` を `-maxdepth 3` で探す。それより深いクローンは個別判定されず、
  ディレクトリ全体の無更新（24 時間）だけで消える。古い未 push のコミットはこの条件では守られない。
  現状の一時ディレクトリはどれも深さ 1〜2 なので実害は見当たらない。hook はリポジトリ外なので PR の判定には含めない。
- 修正案: `-maxdepth` を外す（`target/` は除外済み）か、手順 7 に深さの前提を書く。

### n2（nit）: `-u` なしで push したブランチの worktree はいつまでも残る
- 根拠: `git push origin X`（`-u` なし）だと上流は main のままなので、マージ後も「never pushed under its own name」で残る。
  安全側で、残したものは一覧に出るので owner が判断できる。
- 修正案: 必要なら AGENTS.md の push 手順に `git push -u` を書く。対応しなくてもよい。

## 他の open PR との干渉
`AGENTS.md`、`CLAUDE.md`、`.claude/skills/merge-pr/`、`.claude/skills/review-pr/` に触る open PR は無い。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 2）
