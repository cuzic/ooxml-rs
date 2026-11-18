# PR #70 レビュー（Opus, round 2）

対象: docs/agent-guides @ 85590c4

## 結論（先に）
converged: **no**。Major 1 件。
round 1 の M1（CI ゲート）と M2（レビューファイルの受け渡し）は解消した。ゲートは `gh` 2.103.3 の実出力で、0 件・pending・UNKNOWN のいずれでも止まることを確かめた。
M3（収束後のコミット確認）は半分だけ直っている。merge-pr 手順 1 の `git log --first-parent <sha>..origin/<branch>` は、マージコミットの第 2 親から入った PR 側のコミットと、main を取り込んだときの衝突解消の手編集を表示しない。この PR が決めた「レビュアーが PR ブランチに push し、拒否されたら `git pull --no-rebase`」という手順を使うと、Codex が並行して push したコミットはこの経路で入る。また、確認する範囲が、マージ時に固定する `$SHA` に結び付いていない。Minor 2 件は Codex 側の pull 手順と、付け替えの順序。

## 前ラウンドの解消状況
- M1: 解消。merge-pr L32-40 を実際の PR で確かめた。`--json` 付きの `gh pr checks` は pending があっても終了コード 0 を返す（#70: 出力 `pending,pass`、終了コード 0。`--json` なしでは 8）。それでも `bad` が 1 になるので止まる。checks が 0 件のとき（#58: "no checks reported on the 'impl/xlsx-phase4' branch"）は `--json` 付きでも終了コード 1 で、`n=$(…) ||` の分岐で止まる。`mergeable` が `UNKNOWN` のとき（#67, #68）は `= MERGEABLE` が偽になり止まる。`--match-head-commit` は gh 2.103.3 にある。`$SHA` を取った後に head が動いた場合、checks は新しい head について評価されるが、マージは古い SHA に固定されているので拒否される。どちらに転んでも止まる側に倒れる。
- M2: 解消。review-pr の届け方（L68-73）、AGENTS.md L16・L61、process.md L52、codex-task L14-15 のどれもが、PR ブランチにコミットするという同じ書き方になった。今回この手順で push した（下の「確認して問題がなかった点」）。
- M3: 一部だけ解消。テンプレートに `対象:` 行が入り、merge-pr に収束後のコミット確認が入った。ただし確認のコマンドに抜け道がある → 下の M1。
- m1: 解消（AGENTS.md L50。`crates/ooxml-core/src/generated/parts.rs` が実在する）。
- m2: 解消（AGENTS.md L74。`crates/ooxml-{xlsx,docx,pptx}/examples/manual_check.rs` が実在する）。
- m3: 解消（AGENTS.md L61、CLAUDE.md L14-15、merge-pr L8、process.md L77 で主語が揃った）。
- m4: 解消（新規の作業は Issue、修正はコメント、と 3 文書で一致した。codex-task L17-19 に起動の 1 行がある）。
- m5: 解消（merge-pr L14）。
- m6: 解消（AGENTS.md L32-37 の 3 行は ci.yml の `pptx-readback` job と一致する。トレーラーの例も入った）。
- n1・n2・n3: 解消。

## Findings

### M1（Major）: 収束後のコミット確認は `--first-parent` を使うので、並行 push と衝突解消を見落とす。また、確認した範囲がマージする SHA に結び付いていない
- 根拠: `.claude/skills/merge-pr/SKILL.md` L20-27（手順 1）と L34（手順 3 の `SHA=`）。
  - **並行 push が隠れる。** レビュアーがレビューファイルのコミットを push しようとしたときに、Codex がすでに PR ブランチへ push していると、拒否されたレビュアーは `git pull --no-rebase` する（review-pr L72）。このときできるマージコミットの第 1 親はレビュアーのコミットになり、Codex のコミットは第 2 親の側に入る。スクラッチのリポジトリで再現した結果:
    ```
    $ git log --first-parent --oneline $R..HEAD
    769965f Merge branch 'pr' of origin into pr
    3916394 docs: PR review round 2
    $ git log --no-merges --oneline $R..HEAD
    3916394 docs: PR review round 2
    3fd5e29 refactor          ← 未レビューのコミット。first-parent では表示されない
    ```
    出てくるのはレビューファイルのコミットと「取り込み」のマージだけなので、手順 1 の許可リストを満たして見える。
  - **衝突解消が隠れる。** 手順 1 は「main の取り込み」を許すが、`--first-parent --oneline` ではマージコミットは 1 行の件名にしかならない。Codex が衝突解消のときに手で書いた内容は表示されない。再現では、`git show --remerge-diff <merge>` で初めて `+resolved-with-backdoor` が見えた（git 2.36 以上）。round 1 の M3 は、この経路を名指しで挙げていた。
  - **範囲が固定されていない。** 手順 1 は時刻 T1 の `origin/<branch>` を見る。一方、マージで固定する `$SHA` は手順 3（T3）で取る。T1 と T3 の間に Codex が push したコミットは、確認されないまま `--match-head-commit "$SHA"` を通る。
  - 許可の判断がコミットメッセージだけでできてしまう。「指摘 ID を挙げた Minor 修正」であることを、差分を見ずに件名だけで判定できる書き方になっている。
- 修正案: 手順 1 を、手順 3 と同じ `$SHA` を使う形にする:
  ```sh
  SHA=$(gh pr view "$PR" --json headRefOid --jq .headRefOid) || exit 1; git fetch origin
  git merge-base --is-ancestor <reviewed> "$SHA" || { echo "reviewed SHA が祖先でない"; exit 1; }
  git log --no-merges --oneline <reviewed>.."$SHA" --not origin/main     # PR 側の全コミット（並行 push 分も出る）
  git diff --stat <reviewed>..."$SHA" -- . ':!docs/reviews'               # 差分を読んで ID と照合する
  for m in $(git rev-list --merges <reviewed>.."$SHA" --not origin/main); do git show --remerge-diff --stat "$m"; done  # 衝突の手編集。生成物以外があれば再レビュー
  ```
  手順 3 では `SHA` を取り直さず、この値を使う。

### m1（Minor）: Codex 側に「作業前と push 拒否時に `git pull --no-rebase`」が書かれていない
- 根拠: レビュアーが PR ブランチに push するようになったので、Codex の既存の worktree は、レビューファイルが入る前の状態で止まっている。codex-task の 1 行の指示（L18「PR #NN の worktree で…」）とテンプレートのブランチ欄（L35「`<branch>` に追加コミット」）には、最初に pull する手順がない。この状態の Codex は、根拠の `docs/reviews/prNN-roundK.md` を見つけられないか、push が拒否される。拒否されたときに rebase や force-push で解決するのは AGENTS.md L59 に反するが、代わりにどうするかは書かれていない。
- 修正案: AGENTS.md の Git の約束に「既存 PR の作業前に `git pull --no-rebase origin <branch>`。push が拒否されたら同じく pull してから push」を加える。codex-task のブランチ欄にも同じ 1 行を入れる。

### m2（Minor）: 積み上げ PR を付け替える手順が、マージより前にある
- 根拠: merge-pr の手順 4（付け替え）は手順 5（マージ）より前にある。手順 5 で `--match-head-commit` が拒否されると、手順 1 からやり直すことになるが、上に積んだ PR はすでに main を向いている。その PR の差分には、まだマージされていない下の PR のコミットが含まれるので、owner がそちらを先にマージすると、下の PR の未収束のコミットが main に入る。
- 修正案: 順序を「マージ（`--delete-branch` なし）→ 付け替え → ブランチ削除」にする。付け替えた後にブランチを消す、という round 1 n3 の意図はこの順でも保たれる。

## 確認して問題がなかった点
- 本文の主張を検証した。ファイルの実在（`parts.rs`、各形式の `manual_check.rs`、`oracle_cases.rs`、`crates/ooxml-bench`、`xtask/`、`spikes/`。`fuzz/` は無く、#36 は open）と、ci.yml の 2 job 名・コマンドを確かめた。
- レビューファイルを誰がコミットするか、誰がマージするか、誰が main を取り込むかについて、AGENTS.md・CLAUDE.md・3 つの skill・process.md の間に矛盾はない。
- ゲートの `n -ge 2` は、現在の 2 job（`test, clippy, codegen --check (Rust 1.99)` と `pptx T3 read-back (python-pptx 1.0.2)`）と一致する。ci.yml は `pull_request` を base で絞っていないので、積み上げ PR にも checks が付く。
- 届け方の実地テスト: スクラッチの clone から、このファイルだけのコミットを `docs/agent-guides` に通常の push で送った。手順どおりに進められた。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 2 / nit 0）
