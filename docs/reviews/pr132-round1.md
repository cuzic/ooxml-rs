# PR #132 レビュー（Opus, round 1）

対象: fix/pptx-p3e-followups @ 0783cd0a3468688bf952e7fdb7cee045c0890845

## 結論（先に）
m1〜m3 と n2 は挙動で解消を確認した。共有判定、`mc:AlternateContent` の拒否、`+mj-`/`+mn-` の拒否はどれもバイト一致で、変異でテストが落ちる。
ただし n1 は文書化だけで、`ThemeFonts { latin: …, ..Default::default() }` という Rust で最も自然な書き方が、既存の東アジア・複合文字の書体を `Ok` のまま消す。
この書き方はリポジトリ自身の呼び出し 3 箇所でも使われている。誤用しやすく、気づけないため Major とする。**converged: no**。

## 前ラウンド（PR #124 round 1）の解消状況
- m1（配布資料マスターとの共有）: 解消。slide+handout、slide+notes+handout は `SharedTheme`、バイト一致。notes と handout だけが共有するテーマや、handout 専用のテーマでは、スライドマスター 0 の編集は `Ok`（正しい）。handout 判定を外す変異で `theme_shared_with_handout_master_requires_opt_in` が落ちる。
- m2（スライドマスター同士）: 解消。`bug68703.pptx` でテストがある。プローブでは `master_theme_mut(1)` 側も `SharedTheme` でバイト一致。
- m3（`mc:AlternateContent`）: 解消。`tblStyleLst` 直下（空の `mc:Fallback` だけの場合も含む）、スロットを包む場合、色値を包む場合はすべて、説明的な `UnsupportedContent` を返しバイト一致。Choice にだけスロットがある場合も拒否する。accent2 が AC に包まれた状態で accent1 を編集すると `Ok` になり、accent1 の色以外はバイト一致（局所性 OK）。
- n1（`ThemeFonts`）: 一部のみ。参照の拒否は入った（`+mj-`、`+mn-cs` はバイト一致で拒否）。空文字は文書化だけ → M1。
- n2（XML 宣言）: 解消。新規 `ppt/tableStyles.xml` は `<?xml … standalone="yes"?>\r\n` で始まり、テストで固定されている。

## 確認したこと（プローブはリポジトリ外で、コミットしていない）
- CI 5 ジョブはすべて pass。`styles_theme` 10 件 pass。
- 手書きの変異 2 件（handout の CT 除外、書体参照の検査を無効化）で、それぞれ対応するテストが落ちた。
- 同じファイルに触る他の open PR は無い。

## Findings
### M1（Major）: `ThemeFonts` の `..Default::default()` が、既存の ea/cs 書体を `Ok` のまま黙って消す
- 根拠: `poi/sample.pptx` の majorFont を `<a:ea typeface="游ゴシック Light"/><a:cs typeface="Arial"/>` にして `set_major_font(ThemeFonts { latin: "Meiryo", ..Default::default() })` を呼ぶと、`Ok(())` が返り、`<a:ea typeface=""/><a:cs typeface=""/>` になる。
  `ThemeFonts` は `Default` を derive しているので、構造体更新構文を勧めているのと同じである。
  リポジトリ内でも `tests/resolve.rs:841`、`tests/styles_theme.rs:256,440` がこの書き方をしている（今の fixture の ea/cs が元から空なので、問題が表に出ない）。
  結果として、日本語の deck では全スライドの東アジア文字の見出し・本文フォントが変わる。ファイルは壊れないが、利用者が指定していない値が `Ok` で失われる。
  警告は構造体の doc comment にしかない。重大度表の「`Ok` での黙った誤り」に当たる。
  Issue #127 は「または文書化」も認めていたが、文書化だけでは、この誤用を防げない。
- 修正案: フィールドを `Option<&str>`（`None` = 変更しない、`Some("")` = 消す）にする。0.1.0 で未リリースなので、今変えるのが最も安い。
  最低限の代替案は `Default` の derive を外して 3 つすべての指定を強制することだが、こちらでは「1 つだけ変える」ができない。
  どちらでも、既存の ea/cs が残ること（または消えること）を確かめるテストを 1 件付ける。ADR-0016 の note も更新する。

### m1（Minor）: `a:clrScheme` や `a:latin` そのものを包む `mc:AlternateContent` が「required element missing」と報告される
- 根拠: `a:clrScheme` 全体を AC で包むと、`set_color` は `UnsupportedContent("theme is missing a required colour or font element")` を返す。
  majorFont の `a:latin` を AC で包んだ場合も、`set_major_font` は同じ文言になる。どちらもバイト一致で安全側だが、要素は存在しているので、文言が原因を誤って伝える。
- 修正案: `incomplete_theme` を返す前に、親に AC の子があれば `alternate_color()` 相当の文言にする。直さない場合は note に記載する。

### n1（nit）: 書体参照の検査は大文字小文字と前後の空白に厳密
- 根拠: `"+MJ-LT"` と `" +mj-lt"` は `Ok` で書き込まれる。PowerPoint が参照として解釈するかは未確認で、害は小さい。
- 修正案: `trim_start()` と ASCII の大文字小文字を無視した比較にするか、現状のままとし、その旨を文書に書く。

## 判定
converged: no（Blocker 0 / Major 1 / Minor 1 / nit 1）
