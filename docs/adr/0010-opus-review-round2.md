# ADR-0010 レビュー（Opus, round 2）

対象: `plan/core-performance`（PR #32）@ `01acb83`。`docs/adr/0010-core-performance.md`（Proposed, revision 2）と `spikes/core-performance/`。round 1 は `docs/adr/0010-opus-review-round1.md`。

照合した相手（各 worktree の HEAD）:

- main `8c54366`
- #14 `core/remove-part` `96af1ff`、#38 `core/add-part` `31906a9`、#18 `core/detach-insert` `cca838e`、#26 `core/insert-at` `631fc90`、#40 `fix/core-fuzz-findings` `a05b771`、#29 `perf/xlsx-row-index` `cca9cc6`

プローブ（すべて scratchpad の `review-pr32-r2/` 配下。リポジトリは変更していません）:

- `spikes/core-performance/src/bin/probe2.rs`：FNV interner の衝突入力、SipHash 版・ahash 版との比較、docx の parse 時間
- `spikes/core-performance/src/bin/probe3.rs`：1M セルの sharedStrings の木の大きさ
- `pr40/` + `skipprobe/`：#40 の parser に「構築しないモード」を同じループの中に入れたもの。エラーの一致と所要時間を測りました。

ホストの負荷は 1 分平均で 12〜51 でした。時間は同じ session 内の相対値で読み、繰り返して確認しています。

## 結論（先に）

- **round 1 の M1〜M4 は、設計としてはすべて解消しました。** m1〜m6 もおおむね解消しています。
  - spike を再実行しました。1,589 個の fixture part で、T1 のバイト一致、core と同じ木、全 re-render の一致を確認しました。interner を変えた後も成り立っています。
  - M2 の「同じ parser ループの構築しないモードなら、同じエラーになる」も確かめました。#40 の parser にこのモードを入れて、49 種類の不正入力を通しました。full parse と deferred parse で、49 件ともエラーの種類と位置が一致しました。
    - 試した入力: 閉じていないタグ、終了タグの不一致、未宣言の実体、`&#0;`、サロゲート、範囲外の文字参照、DOCTYPE、途中の XML 宣言、重複属性、未束縛の prefix、`xmlns:p=""`、空の名前、深さ 253〜400、prefix のスコープの漏れ、U+0000、BOM など。
    - 所要時間（1M セル）: full parse は 836〜1,148 ms、同じループの skip は 393〜423 ms でした（row ノードは今日の表現で構築、負荷 40〜48）。
  - 正しく直っているもの: M4 の generation の進め方、M3 の遅延化の要件、m4 の `parse_part` の意味、m5 の適用範囲。
- **ただし Major が 1 件、新しく見つかりました。**
  - **N1:** rev 2 で interner の hasher を FNV-1a に変えました。FNV-1a は鍵がないので、信頼できない入力で HashDoS を起こせます。
    - 下位ビットが衝突する名前は、数ミリ秒で作れます。それを並べた 18.8 MiB の part の parse に、**111 秒**かかりました。同じ数のランダムな名前なら 0.37 秒、core なら 0.19 秒です。
    - 名前の数が 4 倍になると、時間は約 16〜20 倍になります（二乗）。
    - 修正は小さく済みます。鍵付きの hasher（ahash など）なら、docx での速さは core と同じでした。FNV より速いくらいです。
- 残りは Minor です（N2〜N7）。
  - 中心は 2 つあります。
    - M1 の「field の privacy で強制する」は、今の module 構成では serializer に効きません。
    - Targets の中に、互いに矛盾する数字や根拠のない数字があります。
  - どれも ADR の文面を直せば済みます。

## 解消状況（round 1 の指摘）

| ID | 状況 | 確認したこと |
|---|---|---|
| M1 遅延ノードが展開されずに dirty になる | **解消**（Minor が残る → N2, N3） | §2.3 で不変条件、accessor 化、serializer の fallback、受け入れ条件（privacy、属性だけの編集の property test）が入った。fallback の出力は、展開してから clean な子を描いた結果と同じバイトになる（write.rs では、clean な子は span、text の span、markup の raw がどれも verbatim で、`open_end..close.0` をすき間なく覆う）。ADR-0001 の規則とも整合する。ただし privacy による強制は、`xml/` の子 module には効かない。`attrs` とフラグも対象外（N2）。 |
| M2 検証付き skip のコストと目標値 | **解消**（Minor が残る → N4） | §2.2 は「同じループの構築しないモード」になった。プローブで、エラーの一致（49/49）と所要時間（full 比 2.1〜2.9 倍速い）を確認。Targets は main 比の相対値になり、source が残ることも明記された。ただし Targets の中に矛盾がある（N4）。 |
| M3 #29 の索引が全行を展開する | **解消**（Minor が残る → N5） | §2.5 に、行ごとの遅延化、`SheetErr` を行の属性だけで判定すること、bulk 読みで全行を展開したままにしないことが入った。`RowEntry` の `alternate`、`cells_end`、`misordered` も行の中身から作る値だが、要件に挙がっていない（N5）。 |
| M4 `release_part` と `RegistryPlan` | **解消** | §5 で `release_part` が generation を進める。#38 の `RegistryPlan::check` は `generation` の不一致を `StalePlan` にするので、`apply` が `debug_assert!(false, "planned as parsed")` を通ることはなくなる。逆向き（Raw → Parsed の `xml_part`/`xml_part_with`）は generation を進めないが、`apply` は計画時の木を install して置き換えるだけで、編集は失われない。 |
| m1 docx の parse が遅い | **一部解消** | spike は 1.6 倍遅い状態からは脱した。ただし今回の session では、FNV 版は core より 5〜10% 遅かった（10 round で 15.6〜16.3 ms 対 14.5〜15.0 ms）。ahash 版は 15.0〜15.2 ms で core と同等。stage 1 の受け入れ条件（docx/pptx で後退しない）があるので、実装時に検出される。N1 の修正（鍵付き hasher）で、この点も同時に片づく。 |
| m2 速度比が 1 回の run の値 | **解消** | README と ADR は 3 回の範囲になった。再実行（1M: core 829 ms、compact 518 ms、drop 847 対 157 ms）も範囲内。 |
| m3 `defer` の型と配管 | **解消**（nit → N6） | `&'static [QName]`、part ごとの opt-in、深さとスコープを持つこと。 |
| m4 `parse_part` の意味 | **解消** | すべての状態（Raw、Parsed clean/dirty、Replaced、Added）について定義され、metadata の深さの下限も共有する。 |
| m5 圧縮レベル | **解消**（nit） | 追加した part と Replaced の part も対象。0..=9 の検証と、実データの数字も入った。 |
| m6 interaction の表 | **解消**（Minor → N7） | #38、#14、#18、#26、#29、pptx の `Slide<'a>`、slide だけ非キャッシュ。#40 と #36 は表にない。 |
| nit（`XName` の Hash/Debug、frozen parser の置き場） | **解消** | — |

## 新しい指摘

### N1（Major）FNV-1a の interner は、信頼できない入力で二乗時間になる（HashDoS）

- §1 と spike の `compact.rs` は、interner の鍵 `(prefix, local, URI のアドレス)` を FNV-1a で hash します。spike のコメントには「keys are short and trusted to be few」とあります。
  - しかし、要素名と属性名は **part の中身そのもの**で、攻撃者が自由に決められます。
  - FNV-1a は鍵を持ちません。しかも `h = (h ^ b) * P` なので、hash の下位 k ビットは、状態の下位 k ビットとバイトだけで決まります。
  - そのため、下位 22 ビットが衝突する 4 文字のブロックの組を誕生日攻撃で作り、それをつなげれば、2^m 個の名前がすべて同じバケット列に落ちます。生成は 1 ブロックあたり 2^11 回程度の試行で済み、数ミリ秒です。
  - hashbrown の probe は同じ起点から伸びるので、挿入と検索が O(n) になり、parse 全体は O(n²) になります。
- 実測（`probe2 dos`。`<r>` の直下に、異なる空要素 n 個を並べたもの）:

  | 名前の数（part の大きさ） | FNV・ランダムな名前 | **FNV・衝突する名前** | SipHash・衝突する名前 | ahash・衝突する名前 | core |
  |---|---|---|---|---|---|
  | 4,096（0.2 MiB） | 1.7 ms | 5.4 ms | 1.4 ms | — | 0.5〜0.6 ms |
  | 16,384（0.9 MiB） | 7.9〜9.3 ms | 71 ms | 6.1 ms | — | 2.1 ms |
  | 65,536（4.2 MiB） | 52〜86 ms | **1.3〜3.1 s** | 47〜83 ms | 62〜66 ms | 10〜21 ms |
  | 262,144（18.8 MiB） | 372 ms | **110.8 s** | 402 ms | — | 0.14〜0.19 s |

  - #40 の part の上限（`max_part_bytes`）は 1 GiB です。33 MiB の worksheet 1 つで、数分は止められます。
  - core の今の parser には名前の hash 表がないので、これは ADR-0010 が**新しく持ち込む** DoS の経路です。
- `uris` の表（SipHash）と core の `Scope.interned`（BTreeMap）は問題ありません。
- **修正案。**
  - §1 で、interner の hasher を「プロセスごとに乱数の鍵を持つもの」と定めてください（ahash の `RandomState`、foldhash の seed 付き、または std の `RandomState`）。
    - docx での実測（同じ session、best of 15 を 10 round）: core 14.5〜16.0 ms、FNV 15.6〜16.3 ms、**ahash 15.0〜15.2 ms**、SipHash 17.0〜18.3 ms。
    - ahash は FNV より速いので、m1 の「後退しない」も同時に満たします（外れ値の 2 回を除く）。
    - 鍵付きにするのが難しければ、代わりに、interner の項目数に上限を設けて、超えたら intern をやめる方法でも、二乗は避けられます。
  - stage 1 の受け入れ条件に、次の 2 つを加えてください。
    - 「衝突する名前を並べた入力で、parse が線形に収まる」
    - 「異なる名前が非常に多い入力で、main 比の後退が一定以内」（ランダムな名前 65k 個で、compact は core の約 5 倍遅い。interning は純粋なオーバーヘッドになる）
  - spike のコメントも直してください。

### N2（Minor）privacy による強制は、`xml/` の子 module に効かない。`attrs` とフラグも対象外

- §2.3 は「`children` field は `RawNode` を定義する module に private で、それで強制する」としています。
- しかし `RawNode` は今 `xml/mod.rs` にあります。Rust の private field は、**定義した module とその子孫**から見えます。
  - そのため `xml/write.rs`（serializer。M1 で問題になった場所そのもの）、`xml/parse.rs`、`mod.rs` 自身からは、private にしても直接触れます。
    - `mod.rs` 自身の中には、`structurally_eq`、`force_rerender`、`node_at`/`node_at_mut`、`text()` と、#14 と #38 が足す `remove_root_child_with_leading_ws` があります。
  - 受け入れ条件の「enforced by the field's privacy」は、肝心の場所で成り立ちません。
  - **修正案:** `RawNode` を葉の module（例 `xml/node.rs`）に移す、と明記してください。parser の構築は、そこの `pub(super)` な builder 経由にします。
- 不変条件「遅延ノードへの `&mut` はすべて先に展開する」は、`children` だけでなく `attrs`、`own_dirty`、`subtree_dirty` を直接書く経路にも関わります。これらは `pub(crate)` のまま残ります。
  - lens の `set_attr`/`remove_attr`（`node.attrs[i].value = …`、`attrs.push`）
  - #38 の `apply`（`e.attrs.iter_mut()`、`e.own_dirty = true`）
  - #18 の `with_prefixes`（`for a in &mut n.attrs`）
  - 遅延ノード自身の属性だけを変えるこれらの経路では、展開が起きません。出力が正しいのは、serializer の fallback のおかげです。
  - その fallback は `debug_assert!` で「不変条件違反」とされています。そのため、正当な経路で debug ビルドのテストが panic します。
  - **修正案（どちらか）。**
    - (a) `attrs` とフラグも accessor の裏に隠す。
    - (b) 「展開していない遅延ノードの `own_dirty`」を正規の状態と定め、fallback を定義された描き方にする。`debug_assert!` は外す。
    - (b) を推奨します。属性だけの編集（`row/@ht`）で行を展開しなくて済み、安くなるからです。
  - fallback の健全性は確かめました。
    - 中身を verbatim でコピーしたものは、展開して clean な子を描いたものとバイトが一致します。
    - #40 は lens による `xmlns` 宣言の追加と削除を拒否します（`NamespaceDeclaration`）。そのため、open tag を描き直しても、中身の prefix の束縛は変わりません。
  - ADR に書き足してほしい 2 点:
    - 自己閉じの遅延ノード（`<row r="5"/>`、`close == None`）のときの描き方（`close.0` がない）。
    - コピー元は、そのノードが持つ source の `Arc` であること。

### N3（Minor）`children_mut()` の型が決まっていない。移行の一覧も不完全

- #18 の `detach_node`（`out.children = children`）と `with_prefixes`（`std::mem::take(&mut n.children)`）は、子の列を**丸ごと書き換えます**。#38 の `append_root_child`（`root.children.splice`）も同じです。
  - `children_mut() -> &mut Vec<RawItem>` なら機械的に移行できます。ただしそれでは、Stage 4（arena）が「lens の内部だけの変更」になりません。呼び出し側が全員 `Vec` に縛られるからです。
  - **修正案:** stage 1b の accessor を、操作の単位で定めてください（`insert_child`、`remove_child`、`splice_children`、`retain_children`、`set_children`）。`&mut Vec` を返すなら、§1 と Owner question 2 の「lens 内部だけ」を取り下げてください。
- §2.3 の一覧から漏れている、`children` を直接触る箇所（privacy が正しく効けば compile 時に全部見つかります。各 PR の担当者に伝わる作業量を正確にするためのものです）:

  | 場所 | 漏れている箇所 |
  |---|---|
  | main | `Lens::node`、`LensMut::node_mut`/`mark_path`（lens.rs L134、L234、L569：**`&mut` で降りる主経路**）、`first` の判定（L267）、`place`（L376）、`set_text`（L538-548）、`node_at`/`node_at_mut`（mod.rs L274、L280） |
  | #18 | `ancestor_context`、`element_at`、`has_text`、`declarations`、`insert_detached`（L746-749）、`with_prefixes` |
  | #38 | `element()`/`append_root_child`（add.rs、diff の L1251-1275）、`Id` 属性の書き換え（diff の L2484）、`self.root.children`（diff の L2582） |
  | #26 | `check_insert_at`（`&self.node().children`） |

### N4（Minor）Targets の数字が互いに合わない。根拠のない部分がある

- **Stage 1 の heap ≤ 1/4（all parts）:** 同じ行に、根拠として「largest `word/document.xml` で 1/2.7」とあり、これは 1/4 を満たしません。pptx の slide も 1/2.85 です（README: 16.8 → 5.9 B/byte）。
  - 「sheets ≤ 1/4、docx/pptx ≤ 1/2.5」のように分けてください。
- **Stage 2+2x:** 「open + edit 1,000 cells + save ≤ 0.5 s」は、同じ節の「`Workbook::from_bytes` ≤ 0.6 s」より小さく、成り立ちません。
  - level 3 の deflate だけで 217 ms かかります。round 1 の提案は「open を含まない edit + save ≤ 0.5 s」でした。そちらに直してください。
- **open ≤ 0.6 s の内訳:**
  - 「shared strings と workbook の part で 0.1〜0.2 s（推定）」は、baseline の残差と合いません。baseline の残差は 1.95 − 1.25 − 0.29 − 0.05 ≈ 0.36 s です。
  - この残差の中身（読んだ範囲）:
    - `formula_census` は `read_part` で sheet をもう一度展開している（約 48 ms）。
    - SST の parse（core で 88〜100 ms、compact で 50〜66 ms、`probe3`）。
    - `Vec<String>` の構築。
  - 遅延 row ノードを作るコスト（同じループの skip で、今日の表現の row 100k を作って 393〜423 ms。負荷 40〜48）も、0.31 s には入っていません。
  - 0.6 s は、静かなホストならおそらく届きますが、余裕はありません。breakdown に SST などの段階を足して実測してから決めてください。
- **heap ≤ 80 MiB:** 1M セルの sharedStrings.xml（2.4 MiB）の compact の木は **23.0 MiB** です（core は 131 MiB、`probe3`）。
  - ooxml-xlsx は SST を `xml_part` で parse するので、木は Package に残ります（`load_shared_strings`）。さらに `Vec<String>` も持ちます。
  - 合計は、source 33 + 行 15〜25 + SST の木 23 + `Vec<String>` 約 4 = **75〜85 MiB** で、展開した行を数える前に上限を越え得ます。
  - **修正案:** stage 2x で、SST を §5 の `parse_part` で読む（キャッシュしない）と書いてください。あわせて census を parse の中に入れれば、二度目の展開も消えます。これで約 60 MiB に収まります。

### N5（Minor）#29 の `RowEntry` には、行の中身から作る値が他にもある

- §2.5 の 2 は「cell の位置と `RowErr`」を遅延にするとしています。しかし `SheetIndex::build` は、行ごとに次の値も先に計算します。
  - `alternate: has_ac(n)`（行の中の `mc:AlternateContent`）
  - `cells_end` と `misordered`（`row_tail`）
- どれも `row.indexed_elements()` と `children()` を読むので、このままでは全行を展開します。要件 2 の対象を「`RowEntry` のうち、行の中身から作る値すべて」に広げてください。
- 要件 3 の「clean な行は `content_span()` を quick-xml で走査する」は、ooxml-xlsx に 2 つ目の値デコーダ（実体、CR/LF の正規化、実体をまたぐ text の結合）を作ることになります。tree から読んだ値と食い違う危険があります。
  - `release_children` 側を推奨し、走査を選ぶなら core の非構築モードを使う、と書き添えてください。

### N6（Minor）part ごとの defer の API と、遅延ノードの配置

- `ParseOptions` は `#[non_exhaustive]` です。ooxml-xlsx からは struct literal で作れないので、`with_defer(&'static [QName])` と `with_count(..)` の builder が要ります。§2.1 と §3 に書いてください。
- `xml_part_with` にも、metadata の深さの下限（`ensure_parsed` と同じもの）を当てると明記してください。
- `parse_part` は「package の options で parse する」としています。そのため worksheet の `parse_part` は全部を parse します。
  - `parse_part_with(name, &ParseOptions)` を足すか、part ごとに記録した options を使うか、決めてください。
- 「すでに parse 済みの part は、そのときの options のまま」は、順序に依存します。どこかの経路が先に `xml_part(sheet)` を呼ぶと、deferral が黙って失われます（性能だけの問題です）。
  - stage 2x の受け入れ条件に、「open の後、worksheet の root が遅延の行を持つ」というテストを加えてください。
- §2.4 の遅延ノードの payload（`OnceLock`、`Arc` 2 つ、options、depth）を、`RawNode` に**直接**持たせると、全ノードが 72 B から約 150 B に戻り、stage 1 の利得が消えます。
  - `Option<Box<Deferred>>`（または `children` を `enum { Built(Vec), Deferred(Box<..>) }` にする）と明記してください。`size_of::<RawNode>() <= 72` の assert も加えてください。
- depth を `u16` にすると、`with_max_depth` が 65,535 を超える値を受け付けた場合に切り捨てが起きます。`u32` にするか、上限を設けてください。
- 遅延要素の中にある遅延要素（defer に複数の名前がある場合）を展開したとき、もう一度遅延にするかどうかを決めてください。

### N7（Minor）interaction の表に #40 と #36 がない

- **#40**（`fix/core-fuzz-findings`）:
  - `ParseOptions` に `max_part_bytes` と `max_package_bytes` を足します。`defer` と `count` は、これと衝突しないように並べる必要があります。
  - parser に新しい検査（BOM、NUL、`PositionMismatch`、`EmptyName`、`EmptyPrefixBinding`、XML 宣言の位置）を入れます。どれも**共有の経路**に置くことが、§2.2 の「構造上同じエラー」の前提です（プローブではすべて一致）。
  - lens の `NamespaceDeclaration` の拒否は、N2 の fallback が健全であることの前提です。
- **#36**（fuzz の harness）: stage 2 の「エラーが一致する」property test の入力に、fuzz の corpus を使ってください。

### nit

- `SaveOptions { pub compression }` は public field です。`Compression::Level(42)` を直接入れれば、`SaveOptions::new` の検証を迂回できます。`to_bytes_with` でも検証してください（`#[non_exhaustive]` は外部での構築を禁じますが、field への代入は禁じません）。
- `release_part` が generation を進めると、関係のない計画まで stale になります（例: 計画の途中で pptx の `release_slide` を呼ぶ）。
  - 許容範囲です。代わりに、`apply` で `install: None` かつ Raw のときにその場で parse し直す手もあります（元のバイト列は一度 parse に成功しているので、失敗しません）。
- spike の `Name` は `Eq`/`Hash` を実装していません。ADR の「pointer で比べてから内容で比べる。Hash は内容で計算する」は、spike では確かめられていません。
  - 同じ URI が別のアドレスで現れる場合はあります。spike の `empty`/`xml`/`xmlns` と、`uris[""]` などのことです（`xmlns=""` や、`xmlns:x="http://www.w3.org/XML/1998/namespace"`）。
  - その場合でも、`Name` の値が URI の `Arc` を保持し続けるので、アドレスの再利用（ABA）は起きません。起きるのは内容が同じ `Name` の重複だけで、正しさには影響しません。
  - core の `Scope.interned` は URI を内容で一意にするので、そもそも重複しません。
- 範囲外の観察（今日の core と #40 の挙動で、ADR-0010 は「同じエラー」として引き継ぐもの）: 次の入力を core は受理します。
  - 属性値の中の `<`
  - 属性の間に空白がない（`a="1"b="2"`）
  - コメントの中の `--`
  - `<1c/>`
  - C0 制御文字（`&#x1;`、生の U+0001）
  - 展開名が重複する属性（`xmlns:p="u" xmlns:q="u" p:a q:a`）

  fuzz（#36、#40）で扱う候補です。

## 判定

- converged: **no**
- Accepted after Minor only: **no**
  - 残る Major は N1 だけです。ADR §1 の hasher を「鍵付き（または項目数の上限あり）」に改め、stage 1 の受け入れ条件に衝突入力のテストを足せば解消します。spike で `ahash::RandomState` に差し替えるのは 1 行で、今回の計測では速さも core と同等でした。
  - N2〜N7 は文面の修正だけで済みます。次の round で N1 の修正を確認できれば、Minor のみで Accepted にできる見込みです。
