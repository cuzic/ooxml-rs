# PR #48 レビュー（Opus, round 2）

対象: PR #48 `impl/pptx-phase1-replace`（HEAD `555bbdd`。round-1 修正 `efcf5e9` と origin/main のマージ）。
照合したもの: round-1 レビュー（`docs/reviews/pr48-round1.md`）、ADR-0008 §4・§7 O5、
`tests/oracle/check.py`、`tests/oracle/cases.rs`、`.github/workflows/ci.yml`。
作業場所: スクラッチの `git clone` コピー（`review-pr48-r2/pr48`、mutant 用に `review-pr48-r2/mut`）。
プローブ（`zz_r2.rs`、`presentation.rs` 末尾の `zz_r2_probe`、`zz_gen.rs`）と mutant はスクラッチ内だけにある。

## 結論（先に）

round-1 の M1 と m1–m6 はすべて対処済みで、実測でも確かめた。Blocker と Major はない。

- `check.py` の find/replace 部分は、**編集の仕組み（規則 2–4 と、5 のうち `\v` で分割する部分）については
  本当に独立している**。マッチを自分で探し、python-pptx の lxml 木に対して右から左へ編集し、
  ラン単位の (text, rPr) モデルで比べる。CI の `pptx-readback` ジョブで実行される（`oracle_cases` が `replace_cases()` を追加している）。
  手元の結果は 112 ケース、置換 1,302 件、失敗 0 件。編集の形を壊す Rust 側の mutant は 4 個とも `check.py` だけで検出できた。
- ただし**拒否の判定**（規則 1 の `FieldInMatch`、規則 5 の `LinkSplitByBreak`、規則 6）は Rust の報告をそのまま信じている。
  拒否を増やす mutant も減らす mutant も 7 個すべて `check.py` を通過した（r2-m1）。
  これらは `cargo test` 側ではすべて検出されるので、実害の経路はない。M1 の残り半分として Minor にとどめる。
- コーパス一巡（614 ファイル）と python-pptx による読み戻し（942 出力）は、round 1 と同じく不一致 0 件だった。

**判定: converged = yes**（Minor 2 件、Nit 数件。マージを妨げない）。

---

## 解消状況

| round 1 | 状況 | 確認方法 |
|---|---|---|
| M1 O5 の python-pptx 側オラクル | **解消（一部 r2-m1 に残る）** | `check.py` に `apply_replace`、`replace_one`、`finish_paragraph` を追加。CI の `pptx-readback` で `oracle_cases`（`replace_cases()` を含む）に続けて実行される。手元の実行結果: 112 cases, 1,302 replaced, 0 failures。独立性は下の mutant 表のとおり |
| m1 無関係なランへの `<a:t/>` 挿入 | 解消 | 該当の節を削除。回帰テスト `runs_outside_matches_are_untouched` が round-1 の M4 を検出する |
| m2 規則 6 の正規形 | 解消。過剰に受け入れることもない | 下の「m2 の検証」 |
| m3 試験漏れ | 解消（M12 は残る。Nit） | round-1 の mutant を再実行: M4、M9、M13、M14 は検出。M12 と M15 は生存 |
| m4 apply の途中失敗 | 解消 | 下の「m4 の検証」 |
| m5 `#[non_exhaustive]` と ADR との差 | 解消 | `FindOptions`、`ReplaceOptions`、`Match`、`TextLocation`、`ReplaceReport` に付いた。setter を追加。ADR との差はモジュールの rustdoc と PR 本文に書かれている |
| m6 古い記述 | 解消 | `grep "under review"` で 0 件。PR 本文は rev 3 と項目 14–16 に合わせて書き直されている |
| Nit（rels の残り、正規化なし） | 解消 | モジュールの rustdoc に記載された |

### mutant（手作業で 20 個。各 mutant で `cargo test -p ooxml-pptx --release` と `oracle_cases` + `check.py` を実行）

| mutant | 内容 | cargo test | check.py |
|---|---|---|---|
| R1-M4 | m1 の `a:t` 欠落の節を戻す | 検出 | 通過（モデルは「`a:t` なし」と「空の `a:t`」を区別しない。想定どおり） |
| R1-M9 | 規則 6 を両端のランだけで比べる | 検出 | 通過 |
| R1-M12 | `\v` コピーの挿入先検査を `Ok` にする | **生存** | 通過 |
| R1-M13 | 中身が空になった最後のランを残す | 検出 | **検出**（2 件） |
| R1-M14 | 規則 4 の「1 つは残す」を無効にする | 検出 | 通過 |
| R1-M15 | 校正属性の除外を入れ子の要素にも適用する | 生存（ほぼ等価） | 通過 |
| X1 | `Allow` でも常に `Refuse` として扱う | 検出 | **通過**（1,302 → 1,299 件） |
| X2 | `LinkSplitByBreak` をリンク付きの開始ラン全体に広げる | 検出 | **通過**（1,301 件） |
| X3 | `\v` の後の新しいランに書式を付けない | 検出 | **検出**（12 件） |
| X4 | 接尾辞を落とす | 検出 | **検出**（12 件） |
| X5 | `FieldInMatch` の検査をなくす | 検出 | 通過（O5 のケースにフィールドにかかる一致がない） |
| X6 | `LinkSplitByBreak` をなくす | 検出 | **通過**（1,308 件。リンク外へ移される文字を `check.py` も同じように作る） |
| X7 | rPr 内の空白テキストを内容とみなす | 検出 | 通過 |
| X8 | 規則 6 で最上位の属性をすべて無視する | 検出 | 通過 |
| X9 | 段落の先頭から始まる一致をすべて拒否する | 検出 | **通過**（1,299 件） |
| X10 | 失敗時の復元をなくす | 検出 | 通過 |
| X11 | `ascii_case_insensitive` を無視する | 検出 | 通過（O5 にこのオプションのケースがない） |
| X12 | `Allow` でもランをまたぐ一致をすべて拒否する | 検出 | **通過**（1,292 件） |
| X13 | 規則 4 で空になったランを削除しない | 検出 | **検出**（7 件） |
| X14 | 規則 4 で残すランを最初ではなく最後の空ランにする | **生存** | 通過 |

`cargo test` 側は 20 個中 17 個を検出した。生存したのは M12、M15、X14 である。
`check.py` は、編集の形を壊す mutant（M13、X3、X4、X13）を独立に検出した。拒否の判定を変える mutant（X1、X2、X5、X6、X8、X9、X12）は 1 個も検出しなかった。

### m2 の検証（`zz_r2.rs`、`MixedFormat::Refuse`、"ab"|"cd" の上で `bc` → `X`）

拒否されたもの（期待どおり）:

- `a:rPr` なし と `b="1"`
- `lang` だけ と `lang` + `sz`
- `latin` の typeface が A と B
- `a:rPr` なし と `hlinkClick` / `hlinkMouseOver` だけ
- `b="1"` と `b="true"`
- 入れ子の要素にある `lang`
- 子要素の順序が違うもの

受け入れられたもの（期待どおり）:

- `<a:rPr/>` と `a:rPr` なし
- `ja-JP`/`altLang` と `en-US`
- `<a:rPr b="1"> </a:rPr>` と `<a:rPr b="1"/>`
- XML コメントの有無

見える差を受け入れてしまう例は見つからなかった。`trim()` で捨てるテキストは `a:rPr` の子孫にある空白だけのテキストである。スキーマ上そこに意味のある文字データはない（`extLst` の拡張の中を除く）。

### m4 の検証（`zz_r2_probe`、`#[cfg(test)]` の fault injection をすべてのスライド k に対して実行）

- 対象: `slide-sections`（7 枚）、`bug62513`（19 枚）、`sample`（2 枚）、`45545_Comment`（11 枚）。
- 一致があるスライドで失敗させると、すべての k で `Err` になった。
- その後の `to_bytes()` は、何も変えずに保存したもののバイト列と完全に一致した。復元後にもう一度呼ぶと、正常に置換された。
- 入力ファイルそのもののバイト列とは一致しない。ただし何も変えずに保存した場合も一致しないので、これはパッケージを書き直すことによるものである。
  repo の「byte-identical」の定義（`to_bytes` 同士を比べる）には合っている。
- mutant X10（復元をなくす）は単体テストが検出する。

### コーパス一巡と読み戻し（round 1 と同じ入力）

- 不変条件 T/F と、空のランについての新しい不変条件は、すべて成立した。
  - 614 ファイル（LibreOffice 420 + POI 94 + ShapeCrawler 100）、24,679 呼び出し、34,227 置換。
  - 拒否: `FieldInMatch` 896、`MixedFormatInMatch` 69、`LinkSplitByBreak` 32、`MergedCell` 4。
  - round 1 より数が増えたのは、Refuse、大文字小文字を区別しない、Abort の組み合わせが加わったため。
  - PR 本文の数値（633 ファイル）とはコーパスの範囲が違うが、拒否の内訳はほぼ一致する。
- python-pptx 1.0.2 による読み戻し: 942 出力、1,404 段落が `str.replace` と一致した。読み込みエラー、段落集合の変化、不一致はいずれも 0 件だった。

---

## Minor

### r2-m1. `check.py` は拒否の判定を Rust から受け取ったまま使い、自分では判定しない（M1 の残り）

`apply_replace`（`check.py`）は、python 側の一致集合から Rust が報告した `refused` を引き、残りが Rust の `replaced` と等しいかを確かめる。
そのため、次の 3 つは誰も比べていない。

- Rust が拒否した一致が、本当に拒否されるべきだったか。
- 拒否された一致が、python の一致集合に含まれているか。
- 拒否の理由が正しいか。

上の表のとおり、拒否を増やす mutant（X1、X9、X12）も減らす mutant（X2、X6、X8）も `check.py` を通過した。
特に X6 では、リンクのランから外へ移った文字が F を破る。それでも `check.py` も同じ編集をするので一致してしまう。

round-1 M1 の推奨 (b) は「規則 1–5 を独立に適用」だった。ADR §7 O5 も「from the rules of §4」としている。
規則 1 と規則 5 の拒否は §4 の規則の一部である。さらに、O5 のケースには `FieldInMatch` が 1 件もない。
`cargo test` はこれらの mutant をすべて検出するので、今の実害はない。

**推奨。**

- `check.py` で、段落の中だけで決まる拒否を独立に判定する。対象は規則 1（一致が `a:fld` にかかる）、
  規則 5（開始ランの rPr に `hlinkClick`/`hlinkMouseOver` があり、一致が開始ランの中で終わり、置換文字列に `\v` を含む）、
  規則 6（`Refuse` のとき、隣り合うランの canonical rPr を比べる）。その結果を Rust の `refused` と照合する（種類も含めて）。
- 本体単位の拒否（`AnimatedCharacters`、`CommentAnchoredText`、`InsideAlternateContent`、`MergedCell`）は Rust の報告を受け入れてよい。
  ただし、それらの拒否が python の一致集合に含まれていることは確かめる。
- O5 のケースに、フィールドにかかる一致を 1 つ以上入れる（例: スライド番号フィールドの前後の文字をまたぐ句）。

### r2-m2. 規則 4 の「残すのは開始ラン」が未テスト（X14 が生存）

段落のランがすべて空になるとき、ADR §4 の規則 4 は「the start run stays」としている。実装は最初の空ランを残す（`position`）。
これを最後の空ラン（`rposition`）に変えても、`cargo test` と `check.py` の両方を通る。
F モデルは空のランの書式を見ないので、残ったランの書式（次に入力される文字の書式）が変わっても検出できない。

**推奨。** 書式の違う 2 つのランが、同じ呼び出しで両方とも空になるテストを足す。
例: `[b=1]"ab"` と `[plain]"ab"` で `ab` → `""`。残ったランが `b="1"` を持つことを確かめる。

---

## Nit

- R1-M12（`\v` コピーの挿入先検査）は依然として生存している。実際には失敗しない防御コードなので、そのことを rustdoc かコメントに一言書くとよい。
- `MixedFormat::Refuse` では、一致の途中にある**テキストのない**ランの書式の差でも `MixedFormatInMatch` になる（`zz_r2` の EMPTYMID）。
  見えない差による拒否で、安全側に倒れている。
- 復元のループ `*self.pkg.xml_part_mut(&part)? = doc;` は、途中で `?` が返ると残りのスライドを戻さない。
  構成上は起こらないが、`for` の中で最初のエラーを記録して最後まで戻すほうが、主張（「every changed slide is restored」）に合う。
- `MixedFormat` と `OnRefusal`（公開 enum）には `#[non_exhaustive]` がない。将来、variant を足す余地を残すなら付ける。
- ADR O5 の「python-pptx's paragraph text equals Python's `str.replace`」は、`check.py` では明示的に確かめていない。
  ラン単位の全体モデルで比べるので、これより強い検査になっている。拒否がない場合は `str.replace` と比べる 1 行を足すと、ADR の文言とそのまま対応する。

## 判定

- converged: **yes**（Blocker 0、Major 0、Minor 2: r2-m1、r2-m2）。
- r2-m1 と r2-m2 は試験の追加だけで閉じ、実装の変更は要らない。この PR の中で直しても、後続の PR に回してもよい。
