# PR #132 レビュー（Opus, round 2）

対象: fix/pptx-p3e-followups @ b3d78ad3b4f066e15d0533072b35e392377efbeb

## 結論（先に）
round 1 の M1・m1・n1 は、すべて挙動で解消を確認した。`ThemeFonts` は `Option<&str>` になり、`None` は書体を保持し、`Some("")` だけが消す。
新しい `Option` の意味を崩す入力（空文字、要素の欠落、全部 `None`、`mc:AlternateContent` の位置の変化、参照接頭辞の変種）を試したが、`Ok` での誤りも、拒否時の出力変化も見つからなかった。
**converged: yes**。

## 前ラウンドの解消状況
- M1（`..Default::default()` で ea/cs が消える）: 解消。majorFont を ea「游ゴシック Light」、cs「Arial」にして `ThemeFonts { latin: Some("Meiryo"), ..Default::default() }` を書くと、latin だけが変わり、ea/cs は残る。
  `theme_font_update_preserves_omitted_typefaces_and_can_clear_one` は、出力全体の完全一致でこれを確かめている。
  `None` を `Some("")` として扱う変異で、このテストが落ちる。呼び出し元（`tests/resolve.rs`、`tests/styles_theme.rs`、`tests/oracle/styles_theme_case.rs`）は更新済み。
  公開 API の型変更は ADR-0016 §10 の note に記載された。0.1.0 で未リリースなので、互換性の問題は無い。
- m1（`a:clrScheme` や書体要素を包む AC）: 解消。`theme_child` が経路上のどの要素でも包まれていれば `UnsupportedContent("theme element <名前> is wrapped in mc:AlternateContent")` を返す。
  `themeElements`、`clrScheme`、`accent2`、`fontScheme`、`majorFont`、`latin` のどれを包んでも、その要素名を含むエラーになり、出力はバイト一致。
- n1（参照の大文字小文字と空白）: 解消。`+MJ-LT`、` +mj-lt`、`\t+Mn-Cs \n`、全角空白で始まる `+mj-lt`、`+mj-lt\0` は拒否され、出力はバイト一致。
  前の空白を除く処理を壊す変異で、`theme_typeface_references_are_refused_without_changes` が落ちる。

## 新しい意味を壊しにいった結果（プローブはリポジトリ外で、コミットしていない）
- `latin: Some("")`: `typeface=""` になり、ea/cs は残る。文書どおり。
- 全部 `None`: `Ok` で、出力はバイト一致。すでに同じ値を書く場合もバイト一致。
- `a:ea` が無い collection に `east_asian: Some(..)`: `UnsupportedContent`（incomplete theme）で、出力はバイト一致。
  ADR-0016 §10 は欠けた要素の扱いを書いていないが、同じ節の「No theme part is created」と最小編集の方針に沿っていて、要素を作らず拒否するのは妥当。
  `east_asian: None` の場合は latin だけを書いて `Ok` になり、欠けた ea には触れない（局所性 OK）。不正な入力をそのまま残すだけで、新たに壊すことはない。
- `a:latin` を AC で包み、`ea` だけを指定: `Ok` で、ea の属性以外はバイト一致（AC はそのまま残る）。
  ea を書いた後に latin で失敗する順序（`latin: Some, east_asian: Some`）も、clone を捨てるので出力はバイト一致。
- `+mjé`（4 バイト目が文字境界でない）: `get(..4)` が `None` になり、panic しないで通常の名前として書かれる。`＋mj-lt`（全角）、`Arial +mj-lt`、`+m` も通常の名前として書かれる。参照ではないので妥当。
- CI 5 ジョブはすべて pass。`styles_theme` は 12 件 pass。同じファイルに触る他の open PR は無い。

## Findings
なし。

## 判定
converged: yes（Blocker 0 / Major 0 / Minor 0 / nit 0）
