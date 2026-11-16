# ADR-0010 レビュー（Opus, round 3）

対象: `plan/core-performance`（PR #32）@ `b21050e`。`docs/adr/0010-core-performance.md`（Proposed, revision 3）と `spikes/core-performance/`。round 1、round 2 は同じディレクトリの `0010-opus-review-round1.md`、`0010-opus-review-round2.md`。

照合した相手: main（この worktree の `crates/`）、#18 `core/detach-insert` `cca838e`（`with_prefixes`）、foldhash 0.2.0 のソース（`seed.rs`、`fast.rs`、`lib.rs`、README）。

プローブ（すべて scratchpad の `review-pr32-r3/` 配下。リポジトリは変更していません）:

- `spikes/core-performance/src/bin/r3.rs`
  - `docx` / `docx2`: 最大の `word/document.xml` で、core、foldhash、std `RandomState`、ahash、**std + 256 スロットの前段キャッシュ**、foldhash + 前段キャッシュを比べます。best of 25 を 12〜15 round。round ごとに実行順を回しています。
  - `dos`: `hashdos.rs` と同じ FNV 衝突名とランダム名を使い、4,096〜262,144 個で比べます。
  - `distinct`: 異なるランダム名 262,144 個で比べます。
  - `eq`: key の `Eq` が呼ばれた回数を数えます（時間に依らない衝突の指標）。入力は、ランダム名、FNV 衝突名、foldhash の構造を突く 5 系統の名前です。
  - `seed`: 同じ key の hash を、インスタンス間とプロセス間で比べます。
- `src/compact_fc.rs`: spike の interner に、direct-mapped の前段キャッシュ（256 スロット）を足したものです。
  - スロットは、名前の長さ、先頭と末尾のバイト、prefix の長さ、URI のアドレスから決めます。
  - 当たりかどうかは、内容を比べて確かめます。外れたら、従来の `HashMap` を引きます。
- `src/bin/fallback.rs`: §2.3 の「展開していない `own_dirty` ノード」の描き方を、展開してから描いた結果と比べます。
- `size/s.rs`: `RawNode` の配置ごとの `size_of` を調べます。

ホストの負荷は 1 分平均で 25〜37 でした（32 コア）。時間は同じ session 内の相対値で、round 間の最小値（または第 1 四分位）で読んでいます。

## 結論（先に）

- **N1（Major）は解消しました。** interner の hasher は鍵付きになり、衝突入力でも線形に収まります。
  - `hashdos` を再実行しました。FNV では 65,536 個の衝突名に 1.31 s かかります。foldhash では、衝突名とランダム名がほぼ同じ時間です（35.1 対 34.7 ms）。
  - 名前が 4 倍になると、時間は約 4 倍になります（foldhash のランダム名で 56 → 220 ms）。
  - `Eq` の呼び出し回数（時間に依らない指標）は、どの入力でも、どの hasher でも 0.548〜0.551 回/操作で、ランダム名と区別がつきません。
    - 試した入力: FNV 衝突名、40 バイトで中央 6 文字だけが違う名前、24 バイトで 2 語目だけが違う名前（foldhash の読み込み範囲が重なる長さ）、5 文字の短い名前、前半と後半を入れ替えた 16 バイトの名前、同じ文字で長さだけが違う名前。
  - seed を知らずに foldhash の衝突を作ることは、できませんでした。
    - 短い名前（16 バイト以下）は、入力 2 語と長さで hash が決まり、seed に依らない衝突はありません。
    - 長い名前は、どの乗算にも秘密の値が XOR されます。
  - seed が外に出る経路もありません。interner の表は引くだけで、反復もしないし、hash 値も外に出しません。インスタンスごとに per-hasher seed が変わることも確かめました（同じプロセス内で 3 回とも違う値、プロセス間でも違う値）。
- **N2〜N7 と nit も、おおむね解消しました。** ただし、新しい Minor が 5 件あります。どれも文面の修正で済みます。
  - **R3-1:** foldhash についての記述が不正確で、強すぎます。
    - seed は「乱数」ではなく、ASLR と時刻から作られます。
    - foldhash 自身が「対話的な攻撃者への HashDoS 耐性は主張しない」と書いています。
    - license は Zlib だけです。
    - 「parse は後退しない」という受け入れ条件と、foldhash 自身の +1〜7% が矛盾しています。
  - **R3-2:** 展開していない `own_dirty` ノードの描き方は、§2.3 が言う「バイト一致」になりません。2 つの入力でずれることを、プローブで確かめました。
  - **R3-3:** #18 の `with_prefixes` を「属性だけの書き込み」に分類しているのは誤りです。
  - **R3-4:** 操作単位の mutator に、子への `&mut`（添字指定）がありません。これがないと、lens の降下が書けません。
  - **R3-5:** heap ≤ 80 MiB の上限が、peak なのか、保持量なのかが決まっていません。peak だとすると、source のコピーで上限を越えます。
- **Open question 5（hasher）への推奨: std の `RandomState` に、小さな前段キャッシュを足すことです。依存は増やしません。**
  - std 単体が core より約 19% 遅い、という主張は再現しました（17.3〜18.2 ms 対 14.7〜15.0 ms）。
  - しかし前段キャッシュを足すと、core と同等か、それより速くなりました（13.3〜13.9 ms 対 14.3〜14.9 ms）。foldhash 単体（15.3〜15.6 ms、core 比 +4〜7%）よりも速い結果です。
  - キャッシュの位置は攻撃者が選べますが、できるのは「外れ」を起こすことだけです。外れたら SipHash の表を引くので、最悪でも std 単体と同じで、線形に収まります。
  - CLAUDE.md の std 優先の方針にも合います。そのため、foldhash を足す根拠はなくなります。
- **判定: Accepted after Minor only = yes。**

## 解消状況（round 2 の指摘）

| ID | 状況 | 確認したこと |
|---|---|---|
| N1 FNV の HashDoS | **解消**（Minor が残る → R3-1） | §1 で鍵付きの hasher になった。spike は hasher を型パラメータに取り、既定は `foldhash::fast::RandomState`。`hashdos` の再実行、`Eq` の回数、seed の変化は「結論」のとおり。stage 1 の受け入れ条件に、衝突入力（≤ 1.25×、4 倍の名前で ≤ 5 倍の時間）と、異なる名前が多い入力（main の ≤ 3×）が入った。ただし、どちらの条件も混んだホストでの時間比だと不安定（R3-1）。 |
| N2 privacy と属性だけの編集 | **解消**（Minor が残る → R3-2, R3-3） | `RawNode` は葉の module `xml/node.rs` に移る。Rust の private field は、定義した module とその子孫からしか見えない。葉の module には子孫がないので、`write.rs`、`parse.rs`、`mod.rs` から見えなくなり、強制は本当に効く。parser は `pub(super)` の builder を使う。展開していない `own_dirty` ノードは正規の状態になり、自己閉じの場合も描き方が書かれた。`debug_assert!` は外れた。ただし、描き方が「展開してから描いたもの」と一致しない入力がある（R3-2）。`with_prefixes` の分類も誤り（R3-3）。 |
| N3 mutator と移行の一覧 | **解消**（Minor が残る → R3-4, nit） | 操作単位の mutator（`insert_child`、`remove_child`、`splice_children`、`retain_children`、`set_children`）になり、`&mut Vec` は渡さない。移行の一覧は main、#14、#18、#38、#26 を網羅した。ただし、添字で子への `&mut` を得る経路がない（R3-4）。interaction の表には、`children_mut` という古い名前が 2 行残っている（nit）。 |
| N4 Targets の数字 | **解消**（Minor が残る → R3-5） | heap の上限は part の種類ごとになった（sheets ≤ 1/4、docx/pptx ≤ 1/2.5）。edit + save は open を除く。open の内訳に、SST、行ノードの構築、census の二度目の展開が入った。合計は約 0.5〜0.55 s に `Vec<String>` を足したもので、0.6 s には余裕がない、と正直に書かれ、stage 2 で測ってから決める。SST は非キャッシュで読む要件になり、保持量は約 60 MiB。ただし、80 MiB が peak か保持量かが未定（R3-5）。 |
| N5 #29 の派生値 | **解消** | `alternate`、`cells_end`、`misordered` を含め、行の中身から作る値はすべて遅延になった。二つ目のデコーダは作らず、`release_children` か core の非構築モードを使う。 |
| N6 API と配置 | **解消**（nit） | builder（`with_defer`/`with_count`、#40 の上限とは別の field）、`xml_part_with` と `parse_part_with` での深さの下限、順序のテスト（open の後、worksheet の root が遅延の行を持つ）、入れ子の遅延（1 段ずつ）、depth は `u32`。`Option<Box<Deferred>>` に `size_of <= 72` の assert も付いた。72 B に収まるのは、dirty フラグを span の word に詰めた場合だけ（プローブ: フラグを別の `bool` にすると 80 B、詰めると 72 B、`children` を enum にすると bool のままでも 72 B）。§1 はフラグを詰めると書いているので、矛盾はない。§2.4 にも一言あるとよい（nit）。 |
| N7 #40 と #36 | **解消** | 表に入った。#40 の新しい検査を共有の経路に置くこと、`NamespaceDeclaration` が fallback の前提であること、#36 の corpus をエラー一致の property test に使うこと。 |
| nit（`SaveOptions`、`release_part`、`XName` の重複） | **解消** | `to_bytes_with`/`save_with` でも検証する。関係のない計画も stale になる点は、受け入れて文書化した。同じ URI に別アドレスがあっても無害、と書いた。 |

## 新しい指摘

### R3-1（Minor）foldhash の記述が不正確。「後退しない」と矛盾し、std を退けた根拠も弱い

§1（L172〜211）、Revision 3（L62〜69）、Open question 5（L703〜707）、Sources（L717）が対象です。

- **「keyed with a per-process random seed」は不正確です。**
  - foldhash 0.2 の global seed は、OS の乱数ではありません。次の値を混ぜて作ります（`seed.rs` の `generate_global_seed`）。
    - スタック、関数、static のアドレス（ASLR）
    - `UNIX_EPOCH.elapsed()`
    - `Box` のアドレス
  - per-hasher seed は、スタックのアドレスと thread-local の連鎖から作ります。
  - crate の説明は「minimally DoS-resistant」です。README には「**foldhash does not claim to provide HashDoS resistance against interactive attackers**」とあり、時間の観測から秘密の値を導けると明記しています。
  - 「Hash values are never exposed, so an attacker cannot learn the seed from output」は、出力については正しいです。ただし、時間による側路は否定していません。
  - `wasm32-unknown-unknown` では、時刻も ASLR もありません。そのため seed は事実上固定になります。
    - std の `RandomState` も、この target では固定の鍵になるので、ここは比較の論点にはなりません。
    - ただし、ADR が「乱数の seed」と言い切るのは誤りです。
  - **修正案:** 想定する脅威を、ADR に一文で書いてください。たとえば「信頼できないファイルの非対話的な処理。parse ごとに新しい per-hasher seed を使い、hash 値も表の反復順も外に出さない」。そのうえで、foldhash の免責を引用してください。spike のコメント（`compact.rs`）と README の「per-process random seed」も同じように直してください。
- **license は Zlib だけです**（foldhash 0.2.0 の `Cargo.toml` は `license = "Zlib"`）。「MIT/Zlib」を直してください。
  - また、foldhash は今の依存木にありません。hashbrown 0.17 は zip → indexmap 経由で入っていますが、`default-hasher` は使っていません。つまり、本当に新しい crate になります。
- **foldhash 自身が「後退しない」を満たしていません。**
  - ADR の表でも、foldhash は 14.1〜14.6 ms で、core（13.9〜14.1 ms）より 1〜4% 遅いです。
  - 今回の session では、round の最小値で 15.3〜15.6 ms でした。core は 14.3〜14.9 ms で、+4〜7% です。
  - 一方で、§1 の「parse time equal to core's」（L163）と Consequences の「parse no slower anywhere」（L675）は、FNV のときの数字のままです。
  - std を「19% 遅いので no regression に落ちる」と退けるなら、同じ基準で foldhash も落ちます。
  - **修正案:** 受け入れ条件に、許容幅を書いてください（例「main 比 +3% 以内」）。あるいは、下の前段キャッシュで後退そのものをなくしてください。
- **std を退けた根拠は、前段キャッシュで覆ります（Open question 5 への推奨）。**

  最大の `word/document.xml`、round の最小値（括弧内は中央値）。

  | 方式 | session A（15 round） | session B（15 round） | session C（12 round） | 依存 |
  |---|---|---|---|---|
  | core | 14.74 ms | 14.86 ms | 14.33 ms（q1 14.61） | — |
  | foldhash | 15.63 | 15.55 | 15.31（q1 15.80） | foldhash |
  | std `RandomState` | 17.33 | 18.20 | — | なし |
  | **std + 前段キャッシュ** | **13.26（13.91）** | **13.50（14.64）** | **13.94（q1 14.13）** | **なし** |
  | foldhash + 前段キャッシュ | — | — | 13.83（q1 14.10） | foldhash |

  - std 単体の「約 19% 遅い」は再現しました（+18〜22%）。
  - 前段キャッシュは、256 スロットの direct-mapped な配列です（`compact_fc.rs`、20 行程度）。std だけで、core より 3〜10% 速くなり、foldhash より速くなりました。
  - 実在の part は、数百種類の名前を何度も繰り返します。そのため、ほとんどの lookup が SipHash を通りません。
  - **DoS について:**
    - 攻撃者が名前を選んでも、できるのはキャッシュを外させることだけです。外れたら、鍵付きの SipHash の表を引きます。
    - 衝突名とランダム名の時間比は、1.00〜1.07× でした（4,096〜262,144 個）。
  - **代償:** 異なる名前が非常に多い入力では、std の速さになります（262,144 個で core の 3.0〜3.6×）。今の「main の ≤ 3×」の境界ぎりぎりです。
    - この条件は、二乗を見張るための目安です。線形であることは、もう一つの条件（4 倍の名前で ≤ 5 倍）が保証します。
    - そのため、「≤ 4×」に緩めるか、interner の項目数に上限を設けて、超えたら intern しない、のどちらかで足ります。
  - **推奨:** Open question 5 の提案を「std の `RandomState` + 前段キャッシュ（依存を増やさない）」に替えてください。owner が foldhash を選ぶ場合でも、前段キャッシュは足す価値があります（foldhash + 前段キャッシュが、両方の条件で最速でした）。
- **時間比の受け入れ条件は、混んだホストでは不安定です。**
  - 262,144 個での foldhash の衝突名とランダム名の比は、単発の run で 1.37×、1.47×、0.95× とばらつきました。1.25× の境界を単発で割ることがあります。
  - **修正案:** 衝突の判定を、時間に依らない指標にしてください。テスト用の key の `Eq` が呼ばれた回数を数え、衝突入力でもランダム入力と同じく 1 操作あたり 1 回未満、とします（今回の実測は 0.55）。そうすれば、CI で決定的に判定できます。時間比は、静かなホストでの参考値に回します。

### R3-2（Minor）展開していない `own_dirty` ノードの描き方は「バイト一致」しない

§2.3（L300〜311）と、stage 2 の受け入れ条件（L569〜570）が対象です。

- §2.3 の描き方は「描き直した open tag ＋ `source[open_end..close.0]` をそのまま ＋ **source どおりの close tag**」です。そして「展開して clean な子を描いたものとバイトが一致する」としています。
- しかし今の serializer（core の `write.rs` L32〜58、spike も同じ）は、`own_dirty` のノードを次のように描きます。
  - 子がなければ、自己閉じ（`/>`）にする。
  - close tag は、**名前から作り直す**（`</{name}>`）。
- そのため、次の 2 つでずれます（`fallback.rs` で確認）。

  | source | 展開して描いた結果 | ADR の描き方 |
  |---|---|---|
  | `<row r="1"></row>`（中身が空で、自己閉じではない） | `<row r="1"/>` | `<row r="1"></row>` |
  | `<row r="2" >x…</row >`（close tag に空白） | `…</row>` | `…</row >` |

- どちらも、XML としては正しい出力です。T2（編集した要素の span の外は変わらない）も守られます。
- しかし、stage 2 の property test「deferred と full の parse で、編集の有無にかかわらず同じバイトになる（属性だけの編集を含む）」は、この入力で落ちます。
  - 中身が空の `<row …></row>` は、Excel 以外の producer がよく書く形です。
- **修正案（どちらか）:**
  - (a) 描き方を serializer に合わせる。中身の範囲が空なら `/>` にし、close tag は名前から描く。これで「バイト一致」が文字どおり成り立ちます。
  - (b) source の close tag を残す今の描き方を採る。その場合は「バイト一致」の主張を取り下げ、property test の比較を「parse し直した木が構造的に等しく、T2 を満たす」に変えます。
  - (a) を推奨します。fallback と通常の経路が同じ出力になるので、テストが単純になります。

### R3-3（Minor）#18 の `with_prefixes` は「属性だけの書き込み」ではない

§2.3（L295〜299）が対象です。

- #18（`cca838e`）の `with_prefixes`（lens.rs L950〜968）は、次のことをします。
  - **要素自身の名前の prefix** と、すべての属性の prefix を、挿入先の宣言に合わせて付け替える。
  - そのあと `mem::take(&mut n.children)` で、子へ再帰する。
- 名前や prefix を変えたノードを、展開しないまま R3-2 の描き方で出力したとします。すると、中身をそのままコピーした部分の prefix は、**元の文書**の束縛を前提にしています。挿入先の文書では、その prefix は未束縛かもしれず、別の URI に束縛されているかもしれません。
  - 今の実装の流れなら、`set_children` の前に展開されるので、実害は出ません。
  - しかし ADR は、`with_prefixes` を「展開せず `own_dirty` のままにする」例として挙げています。これを読んだ実装者が、遅延ノードに特別な扱いを足すと壊れます。
- **修正案:**
  - `with_prefixes` を、属性だけの書き込みの一覧から外してください。
  - 「展開しなくてよいのは、自分の**属性の値**、または名前空間宣言以外の属性の追加と削除だけ。要素名や prefix を変える操作、別の文書へ移す操作（detach/insert）は、**深く**展開してから行う」と書いてください。入れ子の遅延の中も含めて展開します。
  - あわせて、不変条件を一つ足してください。「遅延ノードの `Deferred.source` は、そのノードが属する文書の source と同じ」。こうすれば、文書をまたいだ遅延ノードがありえないことを、assert で確かめられます。

### R3-4（Minor）操作単位の mutator に、添字で子への `&mut` を得る経路がない

§1（L147）と §2.3（L292〜294）が対象です。

- 一覧にあるのは `insert_child`、`remove_child`、`splice_children`、`retain_children`、`set_children` だけです。
- しかし main の `LensMut::node_mut` と `mark_path`（lens.rs L134、L234、L569）は、`&mut` で 1 段ずつ降ります。#38 の `apply`（`root.children.get_mut`）、#26 の `insert_at` の後の編集も同じで、**既存の子への `&mut`** が要ります。
  - 今の一覧の操作では、どれもこの用途を表せません。
- **修正案:** `child_mut(i) -> Option<&mut RawItem>`、または `children_slice_mut() -> &mut [RawItem]` を足してください。どちらも呼ぶ前に展開し、長さは変えられないものにします。
  - 長さを変えない `&mut [RawItem]` なら、stage 4 の arena（連続した範囲）とも両立します。
- この経路で降りる lens も、§2.3 の「降りるときに展開する」を満たします。属性だけの編集は、降りる先のノード自身を展開しない、という区別も書いてください（`row/@ht` の場合、`sheetData` の子への `&mut` は取りますが、`row` の子への `&mut` は取りません）。

### R3-5（Minor）heap ≤ 80 MiB は peak か保持量か。peak なら source のコピーで越える

Targets（L594、L614〜624）が対象です。

- baseline の heap は、`PeakAlloc` で測った **peak** です（`performance-baseline-2026-11.md`。calamine の 77 MiB も peak）。比べるなら、80 MiB も peak のはずです。
- しかし今の parser は `parse_with(bytes: &[u8])` で、展開したバイト列を `Box<str>` に**コピー**します。ADR の `Arc<str>` でも、同じ署名ならコピーになります。
  - そのため 1M セルの sheet の open では、展開した `Vec<u8>`（33 MiB）と `Arc<str>`（33 MiB）が、同時に生きる瞬間があります。そこに、遅延の行ノード（15〜25 MiB）と入力ファイル（3.4 MiB、`Package` が `Arc<[u8]>` で保持）が加わります。
  - peak は **85〜95 MiB** と見積もれます。
  - SST を `parse_part` で読む場合も、木（23 MiB）が一時的に生きます。
- **修正案:** 80 MiB が「open 後の保持量」なのか「peak」なのかを書いてください。
  - peak なら、part を展開した結果を `Arc` に直接書き込む経路を要件にしてください。たとえば `Arc::new_uninit_slice` に展開し、UTF-8 を検査してから `Arc<str>` として扱います。こうすれば、コピーが要らなくなります。
  - あわせて、memory の内訳に入力ファイル（`Package` が持つ `Arc<[u8]>`）を足してください。

### nit

- interaction の表で、#14 と #26 の行がまだ「through `children_mut`」です（L544、L547）。それぞれ `remove_child`、`insert_child`（＋ R3-4 の `child_mut`）に直してください。core/add-part の行の `root.children.get_mut` も、R3-4 の accessor に対応します。
- §2.4 の `u32` の depth について。`with_max_depth(usize)` が `u32::MAX` を超える値を受けたときにどうするか（飽和させるか、拒否するか）を、一言書いてください。
- §2.4 の `Option<Box<Deferred>>` で 72 B に収まるのは、§1 のとおりフラグを span の word に詰めた場合だけです（プローブでは、フラグを別にすると 80 B）。§2.4 にもそう書いておくと、assert が落ちたときの理由がすぐわかります。
- §2.1 の入れ子の遅延について。展開した中身の span は、part の source の**絶対**オフセットであると明記してください。T2 と、R3-2 の描き方の両方が、これを前提にしています。
- §2.4 の `release_children` の「中が dirty なら false」について。ノード自身の `own_dirty`（属性だけの編集）は対象外で、子が clean なら解放してよい、と明記してください。R3-2 の描き方があるので、それで安全です。
- 範囲外の観察: prefix の解決（core の `Scope::lookup`、spike の `Ctx::lookup`）は、束縛の列を後ろから線形に探します。1 つの要素に大量の `xmlns:pN` を宣言し、それを使う属性を並べると、二乗の時間になり得ます。ただし、これは今の core にもある挙動で、ADR-0010 が持ち込むものではありません。#36 の fuzz の候補です。

## 判定

- converged: **no**（Minor の R3-1〜R3-5 が残っています）
- Accepted after Minor only: **yes**
  - Blocker と Major はありません。N1 は、鍵付きの hasher で解消しました。線形であることは、時間と、時間に依らない `Eq` の回数の両方で確かめました。
  - R3-1〜R3-5 は、どれも ADR の文面を直せば済みます。R3-2 だけは、stage 2 の property test が実際に落ちる内容です。実装前に、(a) か (b) のどちらにするかを決めてください。
  - Open question 5 は、「std の `RandomState` ＋ 前段キャッシュ（依存なし）」を推奨します。foldhash を採る場合でも、R3-1 の記述（seed の出どころ、免責、license、後退の許容幅）は直す必要があります。
