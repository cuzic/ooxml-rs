# PR #70 レビュー（Opus, round 4）

対象: docs/agent-guides @ 3c93a758085e5d6be370df157177652b60734a16

## 結論（先に）
converged: **yes**。Blocker 0、Major 0。Minor 1 件と nit 1 件は follow-up でよい。
round 3 の M1・m1・m2 はどれも解消した。スクラッチのリポジトリで merge-pr のコードを動かし、空の SHA、置き換え忘れのプレースホルダ、main 側の他の PR のマージがどれも、止まるか一覧から外れることを確かめた。
文書どおりに進めたときに、未レビューの変更がマージされる、作業が失われる、エージェントが先に進めなくなる、という経路は見つからなかった。

## 前ラウンドの解消状況
- M1: 解消。merge-pr L20-21 で「各コードブロックは 1 回の呼び出しで実行し、`PR` `R` `SHA` はリテラルで書く」と明記した。手順 3（L44）と手順 4（L58）の先頭に `: "${PR:?}" "${SHA:?}"` がある。L41 で CI 待ちは timeout 600000 かバックグラウンド実行にした。L28 で `git fetch origin` が `SHA=` の後に移った。プローブの結果:
  - `SHA=` が空のとき: `bash: SHA: parameter null or not set`（rc=127）で止まり、後続の `gh pr merge` は実行されない。
  - プレースホルダ `SHA=<手順 1 の SHA>` を書き換え忘れたとき: 行全体が構文エラー（rc=2）になり、何も実行されない。
  - `--watch` がタイムアウトで打ち切られても、やり直したシェルは変数をリテラルで持つので、固定は外れない。
- m1: 解消。L31 は `--not origin/main` 付きになった。スクラッチのリポジトリで、main 側に手書きで衝突を解いた他のブランチのマージ（`Merge other (resolved)`）を置き、PR 側に「きれいな main 取り込み」と「`f` を `BACKDOOR` で解いた main 取り込み」を作った。旧ループは 3 個を列挙したが、新ループは PR 側の 2 個だけを列挙した。手書きの解消は `f | 7 +------` として表示され、きれいな取り込みの `--remerge-diff` は空だった。`--no-merges` の一覧には、PR 側のコミット（review、m1、未レビューの追加、pr: f）がすべて出た。
- m2: 解消。AGENTS.md L62-63 と codex-task L34-35 が `git pull --no-rebase origin <branch>` と `git push -u origin <branch>` になった。ブランチ名が必要な理由も書いてある。

## Findings

### m1（Minor）: 収束後のコミットを `--oneline` だけで見ているので、指摘 ID 付きのメッセージの下に大きな変更があっても見分けられない
- 根拠: merge-pr L30 は `git log --no-merges --oneline "$R..$SHA" --not origin/main` を実行し、L34 は「指摘 ID を挙げた Minor/nit の修正だけ」なら許す。L36 は「新機能、Major 級の変更」を再レビューに回すとしているが、それを判断する材料（変更の量や、触ったファイル）はこの出力に無い。スクラッチの試行では、`m1: fix` と `unreviewed extra` はメッセージでしか区別できなかった。メッセージが `m1: …` なら、中身にかかわらず通ってしまう。
- 修正案: L30 を `--stat` 付きにする（`git log --no-merges --stat --format='%h %s' …`）。あわせて「指摘の範囲より明らかに大きいファイルや行数があれば再レビュー」と 1 行書き足す。

### n1（nit）: review-pr L73 だけ引数なしの `git pull --no-rebase` のまま
- 根拠: AGENTS.md L62 は「ブランチ名を必ず書く」としており、CLAUDE.md はこれを `@AGENTS.md` で取り込んでいる。レビュアーの clone は `gh pr checkout` で上流が `origin/<branch>` になるので、実害はない（round 3 で確認済み）。ただし、文書の間で書き方がそろっていない。
- 修正案: review-pr L73 を `git pull --no-rebase origin <branch>` にする。

## 確認して問題がなかった点
- CI は `pull_request` に path フィルタを持たない（`.github/workflows/ci.yml` L3-6）。そのため、head がレビューファイルだけのコミットでも checks は 2 件出る。手順 3 の `n -ge 2` が 0 件のまま止まり続けることはない（この PR の head 3c93a75 でも、2 件とも pass）。
- 手順 3 で見る checks は、その時点の head のもの。手順 1 の後に head が動いていれば、手順 4 の `--match-head-commit` で拒否され、手順 1 に戻る。
- `git fetch` を `SHA=` の後に置いたので、SHA が手元に無いために `--is-ancestor` が誤って止まる状況は、force-push 禁止のもとでは起きない。
- 手順 5（付け替え → 削除、拒否されたら付け替えない）、process.md L52-55・L79-81、CLAUDE.md L14-15 は、merge-pr の現行の手順と一致している。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 1 / nit 1）
