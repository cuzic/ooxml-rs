# PR #62 レビュー（Opus, round 2）

対象: impl/docx-phase2 @ dd2a80f1377d316088cdad7cba2feb852e0b2d64

## 結論（先に）
converged: no。round 1 の M1〜M3・m1〜m3・n1 は挙動で確認してすべて直っていた（A2 での `After(最後)` は `sectPr` を移す、
フィールド内の空段落・表内段落は `InsideField`、区切り脚注は `UnsupportedContext`、区切り後の generation も変わる）。
main の取り込み（c45c4b6, dd2a80f）は競合の解消が無いクリーンなマージで、main との差分は追加だけ。#59/#69 の挙動は失われていない。
残りは 1 件。今回足した 3 つの事前検査（`check_insert_under`）にテストが無く、どれを消しても `Err` のまま段落が挿入される（M1）。

## 前ラウンドの解消状況
- M1（A2 で `After(最後の段落)` が既定の最終セクションを作る）: 解消。`Plain`/`LikeNeighbour`（`w:rPr` を含む `pPr` も schema 順で正しい）/`Style` で
  `sectPr` が新しい段落に移る。2 セクションの A2 文書、末尾に空 `w:sdt` がある文書も正しい。`sdt`・表の中は `SectionBreakAtEnd`、
  未定義 `r:id` は `RelIdReferenced`、どちらもバイト同一。
- M2（複数段落フィールド内の空段落）: 解消。`starts_in_field` で、本文の空段落、表のセルの空段落（フィールドが表をまたぐ場合）とも
  段落挿入・リンク・区切りが `InsideField`、バイト同一。フィールドが閉じた後の段落の前への挿入は `Ok`（過剰拒否なし）。
- M3（3 つの拒否にテストが無い）: 解消。表内 `sectPr`、`headerReference` 上だけの `xmlns:r`、append の未定義 `r:id` にテストがあり、
  変異（`tables_between`/`r` 名前空間/`RelIdReferenced`）は落ちる。
- m1（セクション最後の段落で区切る）: 解消。最終セクション・途中セクション（S が後ろの段落）とも `AlreadySectionEnd`。
  後半が表だけ・`w:sdt` だけになる区切りは `Ok`（段落が後ろにあるので妥当）。
- m2（区切り脚注）: 解消。`separator`/`continuationSeparator`/`continuationNotice` の脚注・文末脚注を拒否し、
  同じ part の通常の脚注には挿入できる。
- m3（generation）: 解消。S を持つ後ろの段落も `bump` される（変異は落ちる）。`gen_counter` で値は再利用されない。
- n1（`w:pPrChange` の報告）: 解消。`element: "w:pPrChange"` を返し、テストあり。

## Findings

### M1（Major）: 新しい事前検査 3 つにテストが無く、消すと `Err` で出力が変わる
- 根拠: `structure.rs:335`（`LikeNeighbour` のコピーの `check_insert_under`）、`structure.rs:717`（移す `sectPr` のコピー）、
  `structure.rs:719`（A2 移動での `LikeNeighbour` のコピー）を 1 つずつ消すと、`structure`・`adversarial` のテストは全部通る。
  一方、接頭辞を元の `w:p` で宣言した文書では、どの変異も `Err(NamespaceNotInScope("urn:x"))` を返しつつ段落を残す。
  - 335 を消す: `<w:p xmlns:x="urn:x"><w:pPr><w:jc w:val="left" x:a="1"/></w:pPr></w:p>…` に `insert_paragraph(After(p0), LikeNeighbour)`
    → 本文に `<w:p/>` が増える。
  - 717 を消す: `<w:p xmlns:x="urn:x"><w:pPr><w:sectPr x:a="1"/></w:pPr></w:p>` に `append_paragraph` または `insert_paragraph(After(p0))`
    → `<w:p><w:pPr/></w:p>` が増える。
  - 719 を消す: 同じ形で `x:a` を `w:jc` に付けた A2 文書に `insert_paragraph(After(p0), LikeNeighbour)` → `<w:p/>` が増える。
  元のコードではどれもバイト同一で拒否される（挙動は正しい）。round 1 M3 と同じく、Err ⇒ byte-identical を守る分岐にテストが無い
  （AGENTS.md「新しい拒否には必ず Err ⇒ byte-identical のテスト」）。
- 修正案: 上の 3 つの文書で `refused(...)` が `Core(NamespaceNotInScope)` を返すテストを足す。

### n1（nit）: `structure.rs:298` の `parent_path.len() != 1` 検査は今のテストでは冗長
- 根拠: 消しても `insert_after_a2_keeps_the_landscape_section` の表のケースは `append_moving_section` 側で `SectionBreakAtEnd` になり通る。
  先に弾く意味（表の中で `append_moving_section` まで進まない）はあるので、残すならコメントで理由を書く程度でよい。
- 修正案: そのままでよい。必要ならコメントを 1 行足す。

## main の取り込みの確認
- `git show --remerge-diff` は c45c4b6・dd2a80f とも空（競合の手での解消なし）。
- `git diff origin/main HEAD` の `document.rs`・`format.rs`・`styles.rs`・`fields.rs`・`tests/adversarial.rs` は追加か可視性の変更だけ。
  #59 の書式・スタイル・`enable_update_fields_on_open`（W2 解除は `write_gate_for` 経由で `Operation::Structure` にも効く）、
  #69 の follow-up の挙動は残っている。`resolve_style`・`wml_prefix_in_scope` は main の実装をそのまま使い、`prefix_in_scope` へ一般化しただけ。
- `adversarial.rs` の A2 テストは「本文直下なら移動、`sdt` 内は `SectionBreakAtEnd`」に書き換えられ、`m8`（末尾に段落の無いブロック）も移動後の位置を確認している。
- `ooxml-robustness/src/docx.rs` のオラクル（7e1fe1e）は A2 の移動を予測し、旧 `sectPr` と新段落の外側のバイト一致を確かめる。
  移動の lens 拒否（`NotDetachable`/`NamespaceNotInScope`/`TooDeep`）も、拒否時のバイト同一は従来どおり検査される。
- CI（test/clippy/codegen --check、python-pptx T3）は dd2a80f で緑。

## 他の PR との干渉
- #66（Word Phase2 part 2）は `impl/docx-phase2` に積まれているが、8cce62c 以降（round 1 の修正と dd2a80f）をまだ含まない。
  `structure.rs` の M1/M2 修正を #66 に取り込む必要がある。
- #74（docx_replace fuzz）は `ooxml-robustness/src/docx.rs` に触る。どちらが後にマージされても A2 の予測（拒否→移動）を合わせる必要がある。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 0 / nit 1）
