# PR #70 レビュー（Opus, round 3）

対象: docs/agent-guides @ 2ad28d9de833cc61f57ace47672b07e1d9e4d8ab

## 結論（先に）
converged: **no**。Major 1 件、Minor 2 件。
round 2 の M1・m1・m2 は、書かれた内容としては解消している。手順 1 のコマンドをスクラッチのリポジトリで実行し、並行 push・手書きの衝突解消・祖先でない head のどれも表示されるか止まることを確かめた。
新しい Major は手順をまたぐシェル変数の扱いにある。手順 3 と 4 は手順 1 の `$PR` と `$SHA` を使う前提で書かれているが、Claude Code の Bash ツールは呼び出しごとにシェル変数を引き継がない。しかも手順 3 の `--watch` は CI の所要時間（約 8 分）が既定のタイムアウト（2 分）を超える。この組み合わせで `$SHA` が空のまま手順 4 に進むと、`--match-head-commit ""` は gh の側で無視され、head を固定しないマージになる。

## 前ラウンドの解消状況
- M1: 解消（ただし下の M1 と m1 が残る）。merge-pr L20-34 は head を先に固定し、`--is-ancestor`、`--no-merges … --not origin/main`、`--remerge-diff` の順で確認する。スクラッチのリポジトリで次の 4 通りを再現した（git 2.43）:
  - (a) レビュアーの push が拒否され `git pull --no-rebase` したケース。第 2 親の側の `refactor (unreviewed)` は `--no-merges` の一覧に出る。pull でできたマージコミットの `--remerge-diff` は空だった。
  - (b) main を取り込み、衝突を手で解いたケース。`Merge main` が `g.txt | 6 +----` と表示され、本文の差分に `+BACKDOOR-RESOLUTION` が見える。
  - (c) main をきれいに取り込んだケース。`--remerge-diff` は空だった。
  - (d) head が R の子孫でないケース。`--is-ancestor` が終了コード 1 を返して止まる。fetch していない SHA の場合も `fatal: Not a valid commit name` の後に止まる（表示されるメッセージは「祖先でない」で、原因とずれるが、止まる側に倒れる）。
  - `--not origin/main` によって隠れるのは main から到達できるコミットだけ。PR 側の 3 コミット（review・refactor・m1 修正）はすべて表示された。隠れるのは、積み上げの下の PR がマージ済みの場合のように、すでに main でレビューされたものに限られる。
- m1: 一部だけ解消。AGENTS.md L61-62 と codex-task L35 に `git pull --no-rebase` が入った。ただし引数を付けない形なので、下の m2 の条件では PR ブランチを取ってこない。
- m2: 解消。merge-pr L50-60 は、`--delete-branch` を付けずにマージし、その後で付け替え、最後にブランチを消す順になった。マージが拒否されたときは付け替えない、とも書かれている。リポジトリの `delete_branch_on_merge` は false なので、マージした時点でブランチが勝手に消えることもない。

## Findings

### M1（Major）: 手順 3・4 が手順 1 のシェル変数を前提にしている。変数が消えると、head を固定しないマージになる
- 根拠: `.claude/skills/merge-pr/SKILL.md` L37「`$PR` `$SHA` は手順 1 のもの」、L40 の `gh pr checks "$PR" --watch`、L50 の `gh pr merge "$PR" --merge --match-head-commit "$SHA"`。
  - Claude Code の Bash ツールは呼び出しごとにシェル変数をリセットする。手順 1・3・4 は別々のコードブロックとして書かれており、手順 3 にも手順 1 にも `exit 1` があるので、エージェントは別々の呼び出しで実行するのが自然。
  - 手順 3 の `--watch` は、CI が終わるまで戻らない。この PR の直近の run は 06:36:21 から 06:44:43 まで約 8 分かかっており、Bash ツールの既定のタイムアウト 120 秒を超える。タイムアウトで打ち切られれば、エージェントは新しいシェルで手順 3 をやり直すことになり、そのシェルでは `$PR` も `$SHA` も空になる。
  - 空のときの gh 2.103.3 の挙動を、ソースで確かめた。
    - `pkg/cmd/pr/merge/http.go` L77 は `if payload.expectedHeadOid != ""` のときだけ `ExpectedHeadOid` を送る。したがって `--match-head-commit ""` は固定なしのマージになる。
    - `merge.go` L100 と `checks.go` L105 では、引数 `""` が `SelectorArg` に入り、その場合は現在のブランチの PR が選ばれる。マージする Claude は review-pr と同じく `gh pr checkout NN` したコピーで作業するので、空の `"$PR"` でもエラーにならず、そのまま同じ PR に対して動いてしまう。
  - 結果として、手順 1 で確認した SHA より後に Codex が push したコミットが、確認されないまま main に入る。round 2 の M1 で塞いだはずの経路が、ここから開く。
- 修正案:
  - 手順 3 と手順 4 のコードの先頭に `: "${PR:?}" "${SHA:?}"` を置く。または「別の呼び出しでは PR 番号と SHA をリテラルで書き直す」と明記する。
  - 手順 3 の `--watch` は `run_in_background` で実行するか、タイムアウトを 600000 にする、と書く。
  - ついでに、手順 1 の `git fetch origin` は `SHA=` の後に置く。そうすれば、取得した SHA が fetch 済みであることが保証される。

### m1（Minor）: 衝突解消を見るループが `--not origin/main` を付けていないので、main 側にある他の PR のマージまで列挙する
- 根拠: merge-pr L28 の `git log --merges --format=%H "$R..$SHA"`。PR が main を取り込むと、この範囲には、main にすでに入っている他のブランチの「Merge origin/main into …」も含まれる。そうしたマージの多くは、手で衝突を解いた跡があり、`--remerge-diff` が空でない。L32 は「`--remerge-diff` は空であること」を条件にしているので、PR 自身に問題がなくても再レビューに回ることになる。
  - 実際の履歴で確かめた。PR #50 で、レビュー時点を `R = c404851^1`、head を `a3cdb3e^2` とした場合、L28 のループは 17 個のマージを列挙した。そのうち `--remerge-diff` が空でないものは 6 個で、5 個は他の PR のマージだった（`7367101 … plan/xlsx-phase3`、`49c3928 … impl/docx-phase1`、`68a02c1 … core/add-part`、`0c9fa99 …`、`bdc133d …`）。`--not <マージ時点の main>` を付けると、残るのは PR 自身のマージ 2 個（`ef98737`、`c404851`）だけになった。
  - このノイズがあると、本物の手書き解消が他のマージに紛れて見落とされやすくなる。また、再レビューする `$R..$SHA` の差分には main の変更がすべて入るので、無駄に大きくなる。
- 修正案: L28 を `git log --merges --format=%H "$R..$SHA" --not origin/main` にする。L27 と同じ範囲にそろえる。

### m2（Minor）: 引数なしの `git pull --no-rebase` は、Codex の worktree では main を取ってくることがある
- 根拠: codex-task L34 では、新しいブランチを `git worktree add -b <branch> <path> origin/<base>` で作る。git の既定（`autoSetupMerge`）では、このブランチの上流は `origin/<base>`（通常は main）になる。push のときに `-u` を付けなければ、上流は main のままになる。AGENTS.md L61-62 と codex-task L35 は `git pull --no-rebase` を引数なしで書いている。スクラッチで試した結果:
  ```
  upstream=origin/main
  $ git pull --no-rebase        → Already up to date.
  $ git push origin newpr       → rejected（push は何度でも拒否される）
  レビューファイル: no review file
  $ git pull --no-rebase origin newpr; git push origin newpr  → pushed、docs/r2.md が入る
  ```
  main が進んでいる場合は、依頼されていない main の取り込みが黙って起きる。進んでいない場合は「最新」と表示されたまま push の拒否が続き、force-push は禁止されているので、エージェントは先に進めなくなる。round 2 の修正案は `git pull --no-rebase origin <branch>` と branch を指定する形だった。
- 修正案: AGENTS.md L62 と codex-task L35 を `git pull --no-rebase origin <branch>` にする。または、新しいブランチの最初の push を `git push -u origin <branch>` と書く。

## 確認して問題がなかった点
- 手順の順序は、固定（1）→ main との関係（2。取り込んだら 1 に戻る）→ CI ゲート（3）→ 固定した SHA でマージ（4）→ 付け替え → 削除（5）。拒否されたときにやり直す地点は、どれも手順 1 になっている。
- `git push origin --delete` と付け替えは矛盾しない。`delete_branch_on_merge` は false で、main にブランチ保護は無い。付け替えは削除より前に明示的に行うので、GitHub の自動付け替えに頼らない。
- AGENTS.md L63-64（衝突解消の内容をマージコミットのメッセージに書く）は、merge-pr L28 の `--remerge-diff` と対応している。process.md L53-55 も「第 2 親と remerge-diff を確認し、マージはその head に固定する」で同じことを言っている。CLAUDE.md L14-15 と merge-pr L8 も、main の取り込みは Codex がする、で一致している。
- review-pr L73 のレビュアー側の pull は、`gh pr checkout` が上流を `origin/<branch>` に設定するので、引数なしでも正しく動く（今回のコピーで `@{u}` = `origin/docs/agent-guides` を確認した）。m2 は Codex 側だけの問題。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 2 / nit 0）
