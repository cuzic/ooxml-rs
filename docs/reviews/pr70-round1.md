# PR #70 レビュー（Opus, round 1）

対象: `docs/agent-guides` @ 6453197（AGENTS.md 新設、CLAUDE.md 書き換え、`.claude/skills/{review-pr,merge-pr,codex-task}`、docs/process.md）。
main @ a3cdb3e の実ツリー、`.github/workflows/ci.yml`、`xtask/src/main.rs`、`gh` 2.103.3 の実出力と突き合わせた。

## 結論（先に）
converged: **no**。Major 3 件。いずれも「Claude はレビューとマージ判断、Codex は実装」という分担そのものの継ぎ目にある:
(1) merge-pr の CI ゲートは checks が 0 件・取得失敗のときに素通りする、
(2) 未コミットのレビューファイルが Codex にも PR worktree にも届かない、
(3) 収束したレビューとマージする head の対応が記録されず、Minor 修正を CI だけで通す規則と組み合わさると未レビューのコードが入る。
事実誤り（生成物の一覧、manual_check の場所）と役割の矛盾は Minor。マシン固有情報の混入はない（diff を `/home`・`/tmp`・ユーザー名・uid で検索して該当なし）。

## 前ラウンドの解消状況
なし（round 1）。

## Findings

### M1（Major）: merge-pr の CI ゲートは checks が 0 件か取得に失敗すると「全 pass」と判定する
- 根拠: `.claude/skills/merge-pr/SKILL.md` L23-24。
  - `gh pr checks NN | cut -f2 | grep -qv '^pass$'` は、stdout が空だと `grep -v` が 1 行も出さずに終了コード 1 になり、`if` が偽で通過する。
  - stdout が空になるのは、手順 3 の push 直後でまだ新しい head SHA に check run が登録されていないとき（`gh` は "no checks reported on the '<branch>' branch" を stderr に出して終了コード 1）、および認証・ネットワークエラーのとき。L23 の `--watch` も `>/dev/null 2>&1` でエラーが捨てられ、待たずに戻りうる。
  - L25 の `mergeable` 検査は `UNKNOWN`（push 直後の典型値）を通すので、ここでも止まらない。
  - 列の位置自体は正しい（`gh pr checks 70 | cat -A` で `name<TAB>pass<TAB>41s<TAB>url<TAB>` を確認。2 列目が状態）。問題は空出力と終了コードを無視していること。
  - 手順 4 の本文が言うとおり `gh pr merge` は CI を見ないので、この素通りがそのまま「CI 未実行の head をマージ」になる。
- 修正案: 終了コードと件数を両方見る。例:
  ```sh
  sha=$(gh pr view NN --json headRefOid --jq .headRefOid)
  gh pr checks NN --watch --interval 30 || { echo "checks not green"; exit 1; }   # 0=all pass, 1=fail, 8=pending
  n=$(gh pr checks NN --json bucket --jq 'length') || exit 1; [ "$n" -ge 2 ] || { echo "expected CI jobs missing"; exit 1; }
  [ -z "$(gh pr checks NN --json bucket --jq '.[] | select(.bucket != "pass") | .bucket')" ] || exit 1
  m=$(gh pr view NN --json mergeable --jq .mergeable); [ "$m" = MERGEABLE ] || exit 1
  ```
  さらに手順 6 を `gh pr merge NN --merge --match-head-commit "$sha"` にして、確認した head と違うものはマージできないようにする。期待する job 名（`test, clippy, codegen --check (Rust 1.99)` と `pptx T3 read-back (python-pptx 1.0.2)`）を明記するとなおよい。

### M2（Major）: レビューファイルが「未コミットでスクラッチの clone に置かれる」ため、Codex にもマージ手順にも届かない
- 根拠:
  - `.claude/skills/review-pr/SKILL.md` L17（スクラッチ領域に clone して作業）と L42（`docs/reviews/prNN-roundK.md` は未コミットで置く。コミットは作者かマージ手順が行う）。
  - `.claude/skills/codex-task/SKILL.md` L14・L24 は、修正タスクの根拠として `docs/reviews/prNN-roundK.md の M1, m2` を Codex に読ませる。CLAUDE.md L19 も「引き継ぎは PR 本文と `docs/reviews/` で行う」。
  - Codex は別プロセス（または別マシン・クラウド）で PR ブランチを checkout するので、Claude のスクラッチ clone にある未コミットのファイルは見えない。Codex は根拠なしの状態で止まるか、推測で直す。
  - `.claude/skills/merge-pr/SKILL.md` L15 は「PR の worktree で、レビューファイル（未コミットなら）をコミットする」とするが、ファイルは PR の worktree ではなくレビュアーのスクラッチ clone にある。以前は Claude が作者だったので成り立っていたが、作者が Codex になると成り立たない。
- 修正案: review-pr の出力手順を「レビューファイルを PR ブランチに docs だけのコミットとして push する（merge で追従。force-push しない）、または全文を PR コメントに貼る」に変える。codex-task はコミット済みのパスかコメントの URL を根拠として渡す。merge-pr L15 は「push 済みであることを確認する」に直す。

### M3（Major）: 収束したレビューの対象 SHA が記録されず、収束後に入ったコミットが無審査でマージされうる
- 根拠:
  - review-pr のテンプレート（`.claude/skills/review-pr/SKILL.md` L44-60）には、レビューした head SHA を書く欄がない。
  - merge-pr の前提（L10-11）は「最新ラウンドが converged: yes」だけを見る。
  - 一方で CLAUDE.md L20、docs/process.md の追加段落、codex-task L47 は「Minor/nit だけの修正は再レビュー不要、CI 通過で可」とする。
  - この組み合わせだと、Codex が「Minor の修正」として push したコミットが実際に何を変えたかを誰も見ないまま、merge-pr の前提を満たす。マージ手順 2 の `git merge origin/main` で入る衝突解消も同じ。Codex への作業移管でこの経路は増えるので、Major とした。
- 修正案: テンプレートの見出し直下に `対象: <branch> @ <sha>` を必須にする。merge-pr に「`git log --first-parent <reviewed-sha>..HEAD` で収束後に入った PR 側のコミットを列挙し、各コミットが指摘 ID ごとの修正・レビューファイル・main の取り込みだけであることを確かめる。範囲外の変更があれば短い再レビュー」を加える。

### m1（Minor）: 生成物の一覧に `crates/ooxml-core/src/generated/parts.rs` が無い
- 根拠: AGENTS.md L39 は生成物を `crates/ooxml-schema/src/generated/` と `codegen/report.txt` だけとする。`xtask/src/main.rs` L7-8・L39 の出力は `crates/ooxml-core/src/generated/parts.rs` も含む。Codex がこのファイルを手で直す余地がある（CI の `codegen --check` で落ちるので Minor）。
- 修正案: L39 に `crates/ooxml-core/src/generated/parts.rs` を加える。

### m2（Minor）: リポジトリの地図の manual_check の場所が誤り
- 根拠: AGENTS.md L59 は `examples/<format>_manual_check.rs` とする。実際は `crates/ooxml-{xlsx,docx,pptx}/examples/manual_check.rs` で、一式は `cargo xtask manual-check <outdir>` が書く（`xtask/src/main.rs` L12-13）。
- 修正案: 「`crates/ooxml-<format>/examples/manual_check.rs`（一式は `cargo xtask manual-check <outdir>`）」に直す。

### m3（Minor）: 「誰がマージするか」「誰が main を取り込むか」が文書ごとに食い違う
- 根拠:
  - AGENTS.md L49 は「エージェントは…マージはしない（owner か Claude の merge 手順が行う）」。Claude もエージェントなので、Claude 自身が読むと自己矛盾になる。docs/process.md L74 の "Agents do not merge" も同様。L18 は "Claude … decides merges"（判断）とだけ書き、実行者が明記されていない。
  - AGENTS.md L12 と process.md L19-20 は main の取り込みと衝突解消を Codex の仕事とするが、merge-pr L16-19 では Claude が `git merge origin/main`、生成物の再生成、ローカルでの完了の定義（test/clippy/codegen）まで行う。トークン節約という PR の目的とも合わない。
- 修正案: 「Codex はマージしない。マージは Claude が merge-pr で行う（owner が止めた PR を除く）」と主語を書き分ける。merge-pr の手順 2-3 は「main に遅れていれば Codex に取り込みタスクを出す。Claude はローカルでビルドせず CI の結果を見る」にする。

### m4（Minor）: タスク仕様の置き場所と Codex への渡し方が不一致・未定義
- 根拠: AGENTS.md L15 は「Issue か PR **本文**」、process.md L20 と codex-task L14 は「PR **コメント**」。Codex が PR 本文だけを読むと修正タスクを見落とす。また codex-task は Issue（`[codex] …`）を作るところで終わり、Codex をどう起動するか（GitHub での `@codex` 指定、`codex exec` への受け渡し、owner が手で渡す等）が書かれていない。
- 修正案: 置き場所を 1 つに揃え（例: 新規は Issue、修正は PR コメント、と AGENTS.md 側も合わせる）、codex-task に起動手段を 1 行で書く。

### m5（Minor）: merge-pr が「base が main であること」を確認しない
- 根拠: 現在 open の積み上げ PR は #47、#55、#62、#65、#66（base が main 以外）。merge-pr は base を確認せずに `git merge origin/main` と `gh pr merge` を行うので、積み上げの上側を先に扱うと、main ではなく中間ブランチにマージする。
- 修正案: 前提に「`gh pr view NN --json baseRefName` が `main`。そうでなければ下の PR から順に」を加える。

### m6（Minor）: Codex がそのまま実行できない記述が 2 か所
- 根拠:
  - AGENTS.md L26「pptx に触れたら python-pptx オラクルも手元で回す」には手順が無い。CI の手順は `pip install -r crates/ooxml-pptx/tests/oracle/requirements.txt` → `cargo run --release -p ooxml-pptx --example oracle_cases -- <dir>` → `python -I crates/ooxml-pptx/tests/oracle/check.py <dir> reference/fixtures/pptx`。pip はネットワークが要るので、Codex の実行環境で失敗しうる。`ooxml-core` の変更も pptx の出力に影響するが、条件が「pptx に触れたら」だけになっている。
  - AGENTS.md L51「`Co-Authored-By:` トレーラーを付ける」は値を書いていない。Claude には既定値があるが、Codex は推測することになる。
- 修正案: 3 行のコマンドを載せ、「ネットワークが無ければ CI の `pptx T3 read-back` に任せ、その旨を PR 本文に書く」とする。条件は「`ooxml-pptx` か `ooxml-core` に触れたら」にする。トレーラーは「各ツールの既定値。無ければ `Co-Authored-By: <tool name> <noreply address>`」のように例を置く。

### n1（nit）: ブランチ接頭辞から `ci/…` が抜けている
- 根拠: AGENTS.md L46 にない。docs/process.md L12 にはある。
- 修正案: `ci/…` を加えるか、process.md への参照にする。

### n2（nit）: 地図に `crates/ooxml-bench`、`xtask/`、`spikes/` が無い。`fuzz/` と `crates/ooxml-robustness` は main に無い（#36 は open）
- 修正案: 存在するものを加え、未マージのものは「#36 がマージされるまで存在しない」とはっきり書く。

### n3（nit）: merge-pr L34 の「付け替える前に削除すると自動で close される」は条件付き
- 根拠: GitHub は、PR のマージ後に head ブランチを削除したとき、それを base にする open PR を自動で付け替える（GitHub Docs "Changing the base branch of a pull request"）。close されるのは、マージせずに base ブランチを消した場合などに限られる。先に付け替える手順は安全側なので変更は不要だが、理由の書き方が不正確。また L35 の squash の例外は「merge で追従」の方針と合わせて理由を一言添えるとよい。
- 修正案: 「自動付け替えに頼らず、明示的に付け替えてから削除する」と書き直す。

## 確認して問題がなかった点
- `cargo xtask codegen --check`（`.cargo/config.toml` の alias）、完了の定義の 3 コマンドが ci.yml の `check` job と一致。clippy に `--all-features` が付いた（旧 CLAUDE.md より正しい）。
- MSRV 1.99 / edition 2024 は `Cargo.toml` の `[workspace.package]` と一致。ADR-0001 の T1/T1b/T2、`Err ⇒ byte-identical` の記述も ADR と一致。
- `codegen/allowlist/*.toml`、`codegen/overrides.toml`、`reference/fixtures/<format>/`、`docs/reviews/prNN-roundK.md` は実在。
- skill の frontmatter（`name` はディレクトリ名と一致、`description` に用途と起動の言い回しがある）は Claude Code の skill として有効。CLAUDE.md の単独行の `@AGENTS.md` は、CLAUDE.md からの相対パスとして import される。
- `gh pr checks` の 2 列目が状態（`pass`/`fail`/`pending`/`skipping`）であることは実出力で確認。退行は M1 の空出力のみ。
- 積み上げ PR の付け替えループ（`gh pr list --base <branch>` → `gh pr edit --base main`）は 1 段の付け替えとして正しい。open PR が 30 件を超えるとき用に `--limit 100` を付けるとよい。

## 判定
converged: no（Blocker 0 / Major 3 / Minor 6 / nit 3）
