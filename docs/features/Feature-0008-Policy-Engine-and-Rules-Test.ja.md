# ポリシーエンジンと `yata rules test`

> This Japanese version is the authoritative source. The English version (`Feature-0008-Policy-Engine-and-Rules-Test.md`) has not been written yet.

`yata-engine` のポリシーエンジン(ポリシーのディレクトリの読み込み・検証、Rego の評価、結果の作成)と、ポリシーのリポジトリの CI で使う `yata rules test` を設計します。

ルールの書式、読み込みの順序、失敗の種類と終了コード、結果の型、状態の判定順は [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) で決まっています。この設計書はそれを繰り返さず、実装の具体(モジュールの構成、公開 API、エラーの型、処理の細部、資源の上限の値、regorus の設定、出力の形、テスト用のポリシーのセット)を決めます。入力の形は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、フィクスチャの形式は [ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md)、エンジンの選定は [ADR: ポリシーエンジン](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md)、コマンドとオプションは [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) に従います。

ルールの書式と評価の単位は、この設計の実装の後に [ADR: ルールをリソースごとに評価する](../adr/ADR-0030-Per-Resource-Rule-Evaluation.ja.md)(Issue #30)で変わりました。`fail` を部分集合ルールから真偽値に変え、`target_type` のリソースごとに評価し、`input` を評価の対象のリソース、`data` をアカウントの全リソースにしました。`fail` の要素の `resource_id` / `resource_region` とその対応づけは廃止です。フィクスチャのキーを `input` から `data` に変え(中身は Rego の `data` と同じ形)、`rules test` の JSON の `unexpected[].messages` を `message` にしました。次の節は、その変更に合わせて直してあります(実装は #30 の実装の Issue で行う): 「ルール」の手順 6、「失敗の種類」、「資源の上限」の評価時間、「regorus の設定」、「評価時間の上限の実現方法」、「`input` と `data` の組み立て」、「リソースごとの評価と結果」、「リソースごとの評価の性能」、「フィクスチャの検証」、「フィクスチャの実行と比較」、「JSON の形式」、「エラーの型」、「テスト方針」、「実装時の確認事項」。「実装の段階」と「全体設計の変更」は、この設計の時点の計画の記録です。

## 目的と背景

- ルールは別のリポジトリ(ポリシーのリポジトリ)で配布し、`yata` が実行時に読み込んで regorus で評価する。ポリシーのリポジトリでルールを書き始めるには、フィクスチャでルールを確かめる `yata rules test` が先に要る
- この設計で、`yata-engine` の読み込み・評価と `yata rules test` を実装できるところまで決める
- 全体設計で「ポリシーエンジンを実装する Issue」「ルールのテストを実装する Issue」で決めるとしていた次の事項を、この設計で決める
  - regorus の有効にする cargo feature の組み合わせ
  - `--policy-timeout` の既定値と、評価時間の上限の実現方法
  - 資源の上限の値と、オプションで変えられるようにするか
  - 評価時のメモリの上限
  - Windows の junction などの reparse point の判定方法
  - ダミーとして認めるアカウント ID の一覧
- 同じ `id` のリソースが複数のリージョンに現れる場合の扱い(リソースをアカウント・リージョン・種別・`id` で識別する、`fail` の `resource_region`(ADR-0030 で廃止)、`expected.fail` の `{id, region}`、`IncompleteResource` の `region`)を実装に含める

## スコープ

- ポリシーのディレクトリの読み込み: 存在の確認、マニフェスト、互換性の規則、走査、資源の上限、ライブラリとルールの読み込み、メタデータの検証、失敗の集め方
- Rego の評価: regorus のエンジンの設定、評価時間の上限、`input` と `data` の組み立て、リソースごとの評価と `fail` / `message` / `reason` の解釈、`RuleOutcome` の作成(状態の判定順、`incomplete_resources`)
- `yata rules test`: マニフェスト、読み込みの検証、フィクスチャのそろい方と形式、ダミー値、フィクスチャの実行、プロバイダの対応の検査、text / JSON の出力、終了コード
- `yata-core` の型: `Severity`、`RuleMetadata`、`Reference`、`PolicySetInfo`、`RuleOutcome`、`RuleStatus`、`SkipReason`、`RuleResult`、`ResultStatus`、`ResultResource`、`IncompleteResource`
- テスト用の最小のポリシーのセット(`crates/yata-engine/tests/fixtures/policies/`、`crates/yata-cli/tests/fixtures/policies/`)
- `yata rules test` に必要な範囲の `yata-cli`(引数の解析、ポリシーのディレクトリの決定、ログの初期化、終了コード、中断)
- 上の決定に合わせた `docs/architecture/` の更新(下の「全体設計の変更」)

## スコープ外

- `yata scan`、`yata collect`、`yata rules list`、`yata permissions` とレポートの出力(`ScanReport`、`yata-report`)
- ルールの絞り込み(`--rule`、`--exclude-rule`、`--min-severity`)の関数。`yata rules test` は絞り込みを使わないため、`scan` などを実装する Issue で `yata-engine` に足す
- ルール本体(ポリシーのリポジトリで作る)と、ポリシーのリポジトリの構成・CI の定義
- 全体設計の「未決事項(安全性)」のうち、ポリシーの署名・改ざんの確認
- ルールの評価の並列化([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「評価」のとおり、必要になったら行う)

## 実装の段階

実装は 4 つの PR に分けます。各段階は、前の段階が main に入ってから始めます。

| 段階 | 範囲 | 完了条件 | 依存 |
| --- | --- | --- | --- |
| P1: 読み込み・検証 | 依存の変更(下の「依存の追加」。ルートの `Cargo.toml` の変更はこの段階の 1 回だけ。`Cargo.lock` はこの段階と P4 で変わる)、`yata-core` の `Severity`・`RuleMetadata`・`Reference`・`PolicySetInfo`、`yata-engine` の `limits`・`names`・`version`・`manifest`・`files`・`walk`・`library`・`rule`・`metadata`・`engine_config`・`loader`・`error`、読み込みのテストと、そのためのテスト用のポリシーのセット | 正しいセットが読み込め、ディレクトリ全体の失敗とルール単位の失敗のうち読み込み(走査を含む)の段階で見つけるもののすべての種類が、決めた種類として報告されること。走査で見つける `orphan_fixtures` と、フィクスチャのディレクトリの `symlink`(`LoadMode::RulesWithFixtures` のとき)を含む。fmt・clippy・test が通ること | この設計のマージと、#7 の実装の PR のマージ(下の「共有ファイルの順序」) |
| P2: 評価・結果の作成 | `yata-core` の `RuleOutcome` などの結果の型、`yata-engine` の `input`・`findings`・`evaluate`、評価のテスト | 状態の判定順の 6 つの場合、`resource_region` の有無による対応づけ、`fail` の要素の誤り(`resource_region` が文字列でない、その `id` とリージョンのリソースが無い、など)、時間切れ、`incomplete_resources` の `region` がテストで確かめられていること | P1。#7 の実装の PR(`Inventory`、`Resource`、`ResourceSet` などの型) |
| P3: ルールのテストの機能 | `yata-engine` の `testing`(フィクスチャの読み込み・検証、ダミー値、期待値の比べ方、実行、`TestReport`)、テスト用のポリシーのセットのフィクスチャの失敗のケース | フィクスチャに関するすべての失敗の種類と、`expected.fail` の文字列とオブジェクトの両方の形がテストで確かめられていること | P2 |
| P4: `yata rules test` のコマンド | `yata-cli` の引数の解析(`rules test` のサブコマンド)、ポリシーのディレクトリの決定、ログの初期化、終了コード、中断、プロバイダの対応の検査、text / JSON の出力、CLI の統合テスト | `yata rules test` が正しいテスト用のセットで終了コード 0 になり、各失敗の種類で決めた終了コード(1 / 2)になること。JSON の出力が期待するファイルと一致すること | P3。#7 の実装の PR(`supported_resource_types()`) |

- P2・P3 の完了条件のうち、`resource_region` と `fail` の要素に関するものは ADR-0030 で廃止した。リソースごとの評価で確かめることは、下の「テスト方針」の「確かめること」
- P1 と P2 を分けるのは、P1 のコードは #7 の型を必要とせず、P2 は必要とするため。P1 のコードは #7 と並行して書いてよいが、PR は下の順序で作る

### 共有ファイルの順序

#7 の実装の PR も、ルートの `[workspace.dependencies]`、`Cargo.lock`、`crates/yata-core/Cargo.toml`(`serde`・`serde_json`)、`crates/yata-core/src/lib.rs` を変更します。次の順にします。

1. #7 の実装の PR を main に入れる(`crates/yata-core/Cargo.toml` の `serde`・`serde_json` もここで入る)
2. P1 の PR は、それを取り込んだ main から作り、ルートの `Cargo.toml` の `regorus` の行と `Cargo.lock` を 1 回で変える。`crates/yata-core/Cargo.toml` は変えない(#7 で足りる)

- 選んだ理由: `regorus` の行の変更だけを先に単独の PR にする案(b)は、共有ファイルの PR が 1 つ増え、`Cargo.lock` の変更も #7 と合わせて 2 回になる。#7 の後に P1 を作れば、`Cargo.lock` の変更は #7 と P1 の 1 回ずつで、どちらの PR も衝突しない。P1 を待たせる時間は、P1 のコードを並行して書いておくことで減らせる
- #7 の実装が大きく遅れる場合は、(b) に切り替える(その時点でメンテナーの了承を得る)
- P4 を P3 と分けるのは、P4 が中央の列挙(`yata-cli/src/cli.rs` のサブコマンド)を作り、#7 の公開 API を使うため。CLI の骨組み(`cli.rs`、`exit_code.rs`、`logging.rs`)を作る別の Issue が先に main に入っていれば、P4 はそれに `rules test` を足す。入っていなければ P4 が [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) と [ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md) の配置で作る(`rules test` に要る部分だけ)
- Issue の受け入れ条件(テスト用のセットで `yata rules test` が通り、各失敗の種類を決めた終了コードで報告する)は P4 で満たす

図: [段階と依存関係](../diagrams/Diagram-0008-Milestones-and-Dependencies.md)

## 読み込み

処理の順序は [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「処理の順序」のとおりです。ここでは `yata-engine` の中の処理を決めます。

### 処理の流れ

`load_policies(dirs, options)` は、ディレクトリごとに次を行います。

1. **ルートの確認**: `std::fs::metadata`(シンボリックリンクをたどる)で、存在し、ディレクトリであることを確かめる。無ければ `not_found`、ディレクトリでなければ `not_a_directory`、読めなければ `unreadable`(ディレクトリ全体の失敗)。最上位はシンボリックリンクでもたどる(全体設計の「(d) パスの扱い」)
2. **マニフェスト**: ルートの `yata-policies.json` を `symlink_metadata` で調べ、下の「マニフェスト」の検証をする
3. **走査**: 下の「走査」の規則でディレクトリを走査し、ライブラリ、ルール、フィクスチャのディレクトリ(`yata rules test` のときだけ)、失敗を集める
4. **ライブラリ**: ライブラリを読み込んだ基本のエンジンを作る(下の「ライブラリ」)
5. **ルール**: ルールごとに基本のエンジンを複製してルールを読み込み、検証する(下の「ルール」)
6. **件数**: ルールのファイル(`lib/` の外の `.rego`。読み込みに失敗したものを含む)が 0 件なら `no_rules`(ディレクトリ全体の失敗)

すべてのディレクトリを処理した後、マニフェストの `name` の重複を調べ、重複した 2 つ目以降のディレクトリを `duplicate_name` にします。

- ディレクトリ全体の失敗が見つかったら、そのディレクトリの処理をそこで止め、ほかのディレクトリの処理は続ける(見つかった誤りをまとめて報告するため)
- ルール単位の失敗は、そのルールを除いて処理を続ける
- 1 つでもディレクトリ全体の失敗があれば `Err(PolicyLoadError)` を返す(ディレクトリ全体の失敗とルール単位の失敗の両方を持つ)。無ければ `Ok(LoadedPolicies)` を返し、ルール単位の失敗は `LoadedPolicies.rule_failures` に入れる。終了コードへの対応づけ(`rules test` では 1、それ以外では 2)は `yata-cli` が行う
- ルールは、ディレクトリを指定した順 → ルール ID の順に並べる

図: [読み込みの処理の順序](../diagrams/Diagram-0008-Load-Processing-Order.md)

### 走査

- 深さ優先で走査する。各ディレクトリの項目は名前(バイト列)の昇順に処理し、同じ入力から同じ順で失敗を報告する
- 項目の種類は `std::fs::symlink_metadata` で調べる(シンボリックリンクをたどらない)。判定は下の「シンボリックリンクと reparse point」
- 名前が `.` で始まるディレクトリは走査せず、数にも入れない
- ルートの `yata-policies.json` は走査の対象にしない(上の手順 2 で扱う)
- ルートの `lib/` の下はライブラリの置き場所で、`.rego` をライブラリとして集める。`lib/` の下のディレクトリ名とファイル名には名前の文字の規則を当てない(ルール ID に入らないため)
- `lib/` の外の `<名前>.rego` はルールとして集める。ルール ID はマニフェストの `name`、ルートからのディレクトリ名、拡張子を除いたファイル名を `.` でつないで作る。ディレクトリ名かファイル名が名前の文字の規則に合わなければ、そのルールを `invalid_name`(ルール単位の失敗)にする。規則に合わない名前のディレクトリの中にも入り、その中の `.rego` をそれぞれ `invalid_name` にする
- 名前が `.fixtures` で終わるディレクトリはフィクスチャのディレクトリとして扱う
  - `yata rules test` のとき(`LoadMode::RulesWithFixtures`): 中に入り、項目を集める(下の「`yata rules test`」)。同じディレクトリに `<名前>.rego` が無ければ `orphan_fixtures`(ルール単位の失敗)
  - それ以外のとき(`LoadMode::Rules`): 中に入らない。ディレクトリ自身は数に入れる
- `.rego` 以外のファイルは無視する(数には入れる)
- 名前が UTF-8 でない項目: ディレクトリならその中の `.rego` を `invalid_name` にし、ファイルなら `.rego` かどうかを判定できないので無視する
- 走査中の `read_dir` の失敗は `unreadable`(ディレクトリ全体の失敗)

図: [ディレクトリの走査と項目の分類](../diagrams/Diagram-0008-Directory-Walk-Classification.md)

#### 資源の上限の当て方

| 上限 | 数え方 | 超えたとき |
| --- | --- | --- |
| ファイル数 | 走査で見つけたファイル(`.rego` 以外を含む。シンボリックリンク・reparse point は、指す先の種類を問わずファイル数に数え、ディレクトリ数には数えない(中をたどらないため))。`.` で始まるディレクトリの中と、`LoadMode::Rules` のときのフィクスチャのディレクトリの中は数えない | `too_many_files`(ディレクトリ全体の失敗)。走査をそこで止める |
| ディレクトリ数 | 走査で見つけたディレクトリ(ルートを除く)。数え方の例外はファイル数と同じ | `too_many_directories`(同上) |
| 深さ | ルートからのパスの部分の数。ルート直下の項目が 1。深さが上限と同じディレクトリまでは入らずに項目として数え、その中に何か(`.` で始まる名前を含む)があれば失敗にする | `depth_exceeded`(ディレクトリ全体の失敗)。空のディレクトリは失敗にしない |

- 深さの上限は、ルールのファイルにもフィクスチャのファイルにも同じに当てる。ルールのファイルが深さ `d` にあると、そのフィクスチャのファイルは深さ `d + 1` になる
- `LoadMode::Rules` のときは、フィクスチャのディレクトリの中に入らないため、その中身に深さの判定を当てない。このため、深さ 10 のルールのフィクスチャのディレクトリ(深さ 10。中身は深さ 11)は、`yata rules test` では `depth_exceeded`(ディレクトリ全体の失敗)になり、`scan` などでは失敗にならない。モードで結果が変わるのは、`scan` などがフィクスチャを読まない(全体設計の「フィクスチャに関する検査は `rules test` だけで行う」)ためで、ポリシーのリポジトリの CI の `yata rules test` で見つかる
- 上限の値は下の「資源の上限」

#### シンボリックリンクと reparse point

項目がシンボリックリンク・reparse point かは `FileType::is_symlink()` で判定します(理由は下の「Windows の reparse point の判定」)。見つけたものは読まず、場所によって次の失敗にします([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「(d) パスの扱い」の具体)。

| 場所 | 失敗 |
| --- | --- |
| `lib/` の外の `<名前>.rego` | そのルールの `symlink`(ルール単位) |
| フィクスチャのディレクトリそのもの、またはその中の項目(`yata rules test` のときだけ見る) | そのルールの `symlink`(ルール単位)。対応するルールが無ければ `orphan_fixtures` |
| マニフェスト | `manifest_missing`(ディレクトリ全体) |
| それ以外(ディレクトリ、`lib/` の下の項目、`.rego` 以外のファイル) | `symlink`(ディレクトリ全体) |

- `LoadMode::Rules` のとき、フィクスチャのディレクトリがシンボリックリンクでも、中を読まないので失敗にしない
- 確かめた後にファイルが差し替えられる競合(TOCTOU)は防がない。ポリシーのディレクトリに書き込める利用者は、ポリシーそのものを書き換えられるため(全体設計の「(a) 信頼の前提」)

### ファイルの読み込み

- 開く前に `symlink_metadata` の大きさを上限と比べ、超えていれば読まない
- 開いた後も `Read::take(上限 + 1)` で読み、上限を超えて読めた場合は大きさの上限を超えたものとして扱う(確かめた後に大きくなった場合に備える)
- `.rego` は UTF-8 として読む。UTF-8 でなければ、ルールなら `rego_error`、ライブラリなら `library_invalid`
- 大きさの上限を超えたもの: ルールのファイルとフィクスチャは `file_too_large`(ルール単位)、ライブラリは `library_invalid`、マニフェストは `manifest_too_large`(どちらもディレクトリ全体)
- 読み込みの I/O エラー: ルールのファイルとフィクスチャは `unreadable`(ルール単位)、それ以外は `unreadable`(ディレクトリ全体)

### マニフェスト

[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「マニフェスト」の検証を、次の細部で行います。

- 無い、またはシンボリックリンク・reparse point、またはファイルでない: `manifest_missing`
- JSON のオブジェクトでない、必須のキーが無い、型が違う: `manifest_invalid`
- `format_version`: 0 以上の整数。`SUPPORTED_FORMAT_VERSIONS`(`&[1]`)に無ければ `unsupported_format_version`
- `name`: 名前の文字の規則に合わなければ `manifest_invalid`
- `version`・`min_yata_version`: `x.y.z` の形(各部分は ASCII の数字 1 文字以上で、`u64` に収まる)。違えば `manifest_invalid`
- `min_yata_version` が、`LoadOptions.yata_version` より新しければ `yata_too_old`。比べ方は全体設計のとおり(`x`、`y`、`z` の順に数値で比べる)
- 知らないキーは無視する
- `LoadOptions.yata_version` は `yata-cli` が自分のバージョン(`CARGO_PKG_VERSION`)から作って渡す。`yata-engine` は自分のバージョンを使わない(テストで「新しすぎる」を確かめられるようにするため)。プレリリースの部分(`-` から後)とビルドメタデータ(`+` から後)を除いて `x.y.z` を取り出す(`YataVersion::from_package_version`)

### ライブラリ

1. 基本のエンジンを作る(下の「regorus の設定」)
2. `lib/` の下の `.rego` を、パスの昇順に `Engine::add_policy(相対パス, 内容)` で読み込む。戻り値のパッケージ(`data.` で始まる)が `data.yata.lib.<name>` と同じか、`data.yata.lib.<name>.` で始まらなければ `library_invalid`
3. ライブラリごとに `data.<パッケージ>.metadata` と `data.<パッケージ>.fail` を入力なしで評価し、どちらかが定義されていれば(未定義でなければ)`library_invalid`
4. ライブラリが 1 つ以上あれば、`eval_query("data")` を入力なしで 1 回呼び、モジュール全体の解析(未定義の関数の参照、安全でない変数など)のエラーを、ルールより先にライブラリの誤りとして見つける。regorus はルールの本体を評価するときに解析するため、`eval_query("true")` ではこれらのエラーが出ない(P1 で確認)
5. 構文・解析のエラーは `library_invalid`(ディレクトリ全体の失敗)

- 手順 3 は、`lib/` に置き間違えたルール(定数の `metadata` や、`default fail := false` により入力が無くても定義される `fail` を持つもの)を見つけるための検査で、入力に依存する本体の `metadata` までは見つけられない
- `message` と `reason` は、ライブラリにあっても失敗にしない。どちらも入力に依存するため入力なしの評価では見つけにくく、置き間違えたルールは `metadata` と `fail` で見つかる。また、エンジンはライブラリの `message` / `reason` を読まないため、ライブラリの関数・規則の名前を不必要に制限しない
- 失敗の報告には、regorus のエラーのメッセージ(ファイルのパスと行を含む)を入れる

### ルール

ルールごとに、次を順に行います。どこかで失敗したら、そのルールの失敗として記録し、次のルールに進みます。

1. ファイルを読む(上の「ファイルの読み込み」)
2. 基本のエンジンを `clone()` し、`add_policy(相対パス, 内容)` で読み込む。構文エラーは `rego_error`
3. 戻り値のパッケージが `data.yata.rules.<ルール ID>` でなければ `package_mismatch`
4. `data.yata.rules.<ルール ID>.metadata` を入力なしで `eval_rule` で評価する。解析のエラーは `rego_error`、時間切れは `rego_error`(メッセージで時間切れと示す)、未定義は `metadata_invalid`
5. メタデータを検証して `RuleMetadata` にする(下の「メタデータの検証」)。誤りは `metadata_invalid`
6. `data.yata.rules.<ルール ID>.fail` を入力なし(`set_input` も `add_data` もしない状態)で評価し、真偽値でなければ `fail_missing`(ルールに `fail` が無い、未定義、または真偽値でない値)。メッセージで原因を示す: ルールに `fail` が無い(`eval_rule` が `not a valid rule path` のエラー)なら `rule has no "fail"; add "default fail := false" and "fail if { ... }"`、未定義なら `fail is undefined without input; add "default fail := false"`、集合なら `fail must be a boolean rule; the partial set form "fail contains ..." is not supported`、それ以外の型なら `fail must be a boolean, but was <型>`(ADR-0030 で変更。変更前は集合であることを確かめていた)
7. 読み込んだエンジンをルールとともに持つ(評価とフィクスチャの実行で複製して使う)

#### メタデータの検証

[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「メタデータ」の表に、次の細部を加えます。

| キー | 検証 |
| --- | --- |
| `title` | 空でない文字列。改行(`\n`、`\r`)を含まない |
| `severity` | `critical` / `high` / `medium` / `low` / `info` のいずれかの文字列 |
| `description`、`remediation` | 空でない文字列 |
| `target_type` | リソース種別の形式(3 階層、各部分が名前の文字の規則に合う) |
| `related_types` | 無い、または文字列の配列。各要素がリソース種別の形式で、`target_type` と同じプロバイダ。`target_type` を含まない。重複が無い |
| `references` | 配列(空でもよい)。各要素はオブジェクトで、`standard`・`version`・`control` が空でない文字列。`url` は無いか文字列 |
| `tags` | 無い、または文字列の配列 |

- メタデータがオブジェクトでなければ誤り。知らないキーは無視する
- 1 つのルールに誤りが複数あれば、まとめて 1 つの失敗のメッセージにする

### 失敗の種類

`yata-engine` が返す失敗の種類の文字列です。JSON の出力の `kind` にもこの値を使います。

**ディレクトリ全体の失敗(`DirectoryFailureKind`)**

| 種類 | 内容 |
| --- | --- |
| `not_found` | ポリシーのディレクトリが無い |
| `not_a_directory` | ディレクトリでない |
| `unreadable` | ディレクトリ・マニフェスト・ライブラリなどが読めない |
| `manifest_missing` | マニフェストが無い、シンボリックリンク・reparse point、ファイルでない |
| `manifest_too_large` | マニフェストが大きさの上限を超える |
| `manifest_invalid` | マニフェストの形式の誤り(JSON、必須のキー、型、`name`、`version`・`min_yata_version` の形) |
| `unsupported_format_version` | `format_version` が対応外 |
| `yata_too_old` | `min_yata_version` が実行中の `yata` より新しい |
| `duplicate_name` | マニフェストの `name` がほかのディレクトリと重複する |
| `library_invalid` | ライブラリの誤り(構文・解析、パッケージ名、`metadata` / `fail` を持つ、大きさの上限、UTF-8 でない) |
| `symlink` | ルールのファイル・フィクスチャのディレクトリ以外にあるシンボリックリンク・reparse point |
| `too_many_files` / `too_many_directories` | ファイル数・ディレクトリ数の上限を超えた |
| `depth_exceeded` | 深さの上限より深いところに中身がある |
| `no_rules` | ルールのファイルが 0 件 |

**ルール単位の失敗(`RuleFailureKind`)**

| 種類 | 見つける段階 | 内容 |
| --- | --- | --- |
| `invalid_name` | 読み込み | ルールの名前、またはその上のディレクトリ名が名前の文字の規則に合わない |
| `symlink` | 読み込み | ルールのファイル、またはそのフィクスチャのディレクトリ(とその中)がシンボリックリンク・reparse point |
| `file_too_large` | 読み込み(ルールのファイル)、`rules test`(フィクスチャ。`run_tests`) | ルールのファイル、またはフィクスチャが大きさの上限を超える |
| `unreadable` | 読み込み(ルールのファイル)、`rules test`(フィクスチャ。`run_tests`) | ルールのファイル、またはフィクスチャが読めない |
| `rego_error` | 読み込み | 構文・解析のエラー(regorus の行の長さ・行数の上限を含む)、UTF-8 でない、メタデータ・`fail` の確認中の時間切れ |
| `package_mismatch` | 読み込み | パッケージ名とパスの不一致 |
| `metadata_invalid` | 読み込み | メタデータの誤り |
| `fail_missing` | 読み込み | `fail` が無い、または入力なしで真偽値にならない(`default fail := false` が無い、部分集合ルールの書式など) |
| `unsupported_resource_type` | `rules test`(`yata-cli`) | プロバイダが対応していない種別を宣言している |
| `orphan_fixtures` | 読み込み(走査。`LoadMode::RulesWithFixtures` のときだけ) | 対応するルールの無い `.fixtures` ディレクトリ |
| `fixtures_missing` | `rules test` | `pass_` か `fail_` のフィクスチャが無い(フィクスチャのディレクトリが無い場合を含む) |
| `fixture_invalid` | `rules test` | フィクスチャの形式の誤り(下の「フィクスチャの検証」) |
| `non_dummy_account_id` | `rules test` | ダミーとして認めていない 12 桁の数字がある |
| `fixture_mismatch` | `rules test` | 評価の結果が `expected.fail` と一致しない |
| `fixture_eval_error` | `rules test` | フィクスチャの評価のエラー(ルールの規約の誤り: `fail` が undefined・真偽値でない、`message` / `reason` の形の誤り、conflict を含む) |
| `fixture_timeout` | `rules test` | フィクスチャの評価の時間切れ |

## 資源の上限

### 既存のツールの例

| ツール | ファイルの大きさ | ファイル数・深さ | 評価時間 |
| --- | --- | --- | --- |
| OPA | バンドルの中の各ファイルの上限 `size_limit_bytes`(設定ファイル。既定 1 GiB)。`opa eval` / `opa test` にはオプションが無い | 無い(`--ignore` で除外だけできる) | `opa eval --timeout`(既定は無制限)、`opa test --timeout`(既定 5 秒) |
| Conftest | 無い | 無い(`--ignore` の正規表現で除外だけできる) | 無い |
| Checkov | 環境変数 `CHECKOV_MAX_IAC_FILE_SIZE`(CloudFormation。既定 50 MB)、`CHECKOV_MAX_FILE_SIZE`(秘密の検査。既定 5 MB) | 無い(`--skip-path` で除外だけできる) | 無い |
| Trivy(`trivy config`) | 無い | 無い(`--skip-dirs` / `--skip-files` で除外だけできる) | `--timeout`(全体。既定 5 分) |
| regorus | `PolicyLengthConfig`(ポリシーのファイル 1 MiB、20,000 行、1 行 1,024 文字。いずれも既定値) | - | `ExecutionTimerConfig`(既定なし) |

- 評価時間は、OPA と Trivy がオプションで変えられるようにしている
- ファイルの大きさを変えられるのは Checkov の環境変数だけで、ファイル数・深さを変えられるツールは無い。どのツールも、読む対象は除外の指定で減らす形をとる

### 決定

| 上限 | 値 | 変えられるか |
| --- | --- | --- |
| マニフェストの大きさ | 64 KiB | 変えられない |
| ルール・ライブラリのファイル(`.rego`)の大きさ | 1 MiB(regorus の `PolicyLengthConfig` の既定と同じ) | 変えられない |
| フィクスチャのファイルの大きさ | 4 MiB | 変えられない |
| ファイル数(ポリシーのディレクトリごと) | 20,000 | 変えられない |
| ディレクトリ数(ポリシーのディレクトリごと) | 5,000 | 変えられない |
| 深さ | 10 | 変えられない |
| 評価時間 | 30 秒(ルール × アカウントの評価(リソースごとの評価の合計)、フィクスチャ 1 つの評価(同じく合計)、読み込み時のメタデータ・`fail` の確認の、それぞれ 1 回ごと。ADR-0030 で、リソースごとの評価の合計に掛けることを明記した) | `--policy-timeout <SECONDS>`(1〜3600 の整数) |

- **評価時間だけをオプションにする**: 評価時間は、アカウントの大きさ(リソースの数)とルールの書き方で必要な長さが変わり、利用者の環境ごとに調整が要りうる。OPA・Trivy と同じく、オプションで変えられるようにする
- **ほかの上限はオプションにしない**: ポリシーのディレクトリの大きさは、利用者のアカウントではなくポリシーのリポジトリで決まる。値は実際のポリシーのリポジトリより十分大きくとり(下の「値の根拠」)、上限は誤ったディレクトリ(ホームディレクトリなど)を指定したときに走査が終わらないことと、異常に大きなファイルを読むことを防ぐためのものにする。比べたツールにも、ファイル数・深さを変えるオプションは無い。オプションは後から足しても互換性を壊さないため、必要になった時点で足す
- 上限は `yata-engine` の `ResourceLimits` の既定値(`Default`)として持つ。項目は `pub` にし、テストでは小さい値を渡す
- regorus の `PolicyLengthConfig` は既定のまま使う(1 行 1,024 文字、20,000 行の上限は、`add_policy` のエラーとして `rego_error` / `library_invalid` になる)

### 値の根拠

- **ファイル数・ディレクトリ数**: 1 ルールはルールのファイル 1 つ、フィクスチャのディレクトリ 1 つ、フィクスチャ 2〜10 程度。1,000 ルールでもファイルは 1 万前後、ディレクトリは 1,000 余り(フィクスチャのディレクトリと分類のディレクトリ)に収まる。その 2 倍以上を上限にした
- **深さ**: ルール ID は `<name>.<ディレクトリ>….<ルール名>` で、分類の階層は通常 2〜3(例: `aws/s3/`)。10 は十分な余裕があり、ルール ID が極端に長くなるのも防ぐ
- **ファイルの大きさ**: ルールは数 KiB、フィクスチャは手で書くダミーの JSON で通常数十 KiB。フィクスチャには IAM のポリシー文書のような大きな構成情報が入りうるため、ルールより大きくした。マニフェストは 4 つのキーだけなので 64 KiB で足りる
- **評価時間**: 通常のルールの評価はミリ秒の単位。大きなアカウント(数万のリソース)でリソースを突き合わせるルール(証跡とバケットなど)でも、数秒に収まる見込みで、30 秒は誤りの無いルールを止めない余裕をとった値。時間切れはスキャンを止めず、そのルールを `error` にするだけ(終了コード 4)なので、合わなければ利用者が上げる
- `--policy-timeout` の上限 3600 は、誤った値(ミリ秒のつもりの値など)を早く見つけるため。0 は「無制限」の意味にせず、引数の誤り(終了コード 2)にする

## 評価

### regorus の設定

基本のエンジン(`engine_config.rs`)は次の設定で作り、ライブラリとルールの読み込み・評価のすべてで使います。

| 設定 | 値 | 理由 |
| --- | --- | --- |
| Rego のバージョン | v1(regorus の既定。`set_rego_v0` を呼ばない) | 全体設計のとおり |
| 実行時間の制限 | `set_execution_timer_config(ExecutionTimerConfig { limit: policy_timeout, check_interval: 64 })`。リソースごとの評価では、`eval_rule` の呼び出しの前に `limit` を残りの時間に設定し直す | 下の「評価時間の上限の実現方法」 |
| `print` の出力 | `set_gather_prints(true)`。評価のたびに `take_prints()` で取り出し、`debug` のログに出す(ルール ID を付ける) | 全体設計の「(e) regorus の組み込み関数」 |
| 組み込み関数のエラー | `set_strict_builtin_errors(false)`(既定のまま) | OPA の既定と同じく、組み込み関数のエラー(例: `time.parse_rfc3339_ns` に `"N/A"` を渡した)を undefined として扱う。厳格にすると、認証情報レポートの `"N/A"` のような値で、ルール全体が `error` になる |
| ポリシーの長さ | `PolicyLengthConfig` の既定 | 上の「資源の上限」 |
| `data` | ルール × アカウント(フィクスチャでは 1 つのフィクスチャ)ごとに、`add_data` で 1 回入れる(ADR-0030 で変更。変更前は使わなかった) | 下の「`input` と `data` の組み立て」、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ポリシーエンジンへの渡し方」 |

- 組み込み関数のエラーを undefined として扱うと、その本体は成り立たず、リソースは黙って `pass` になりうる。これはルールの書き方の注意([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「書き方の注意」)と同じ性質で、フィクスチャで確かめる

### regorus の cargo feature

`regorus` は `default-features = false` とし、次を有効にします。

| 有効にする | 内容 | 追加の依存 |
| --- | --- | --- |
| `std` | 標準ライブラリを使う(`HashMap` など)。無いと `no_std` の構成になる | `parking_lot` など |
| `arc` | 参照カウントを `Arc` にする(エンジンをスレッド間で渡せるようにし、将来の評価の並列化に備える) | 無い |
| `base64`、`base64url`、`hex` | `base64.*`、`base64url.*`、`hex.*` | `data-encoding` |
| `cache` | `regex.*`・`glob.*` のコンパイル済みのパターンのキャッシュ | `lru` |
| `glob` | `glob.match` など(IAM のアクションのワイルドカードの照合など) | `globset` |
| `graph` | `graph.reachable` など | 無い |
| `jsonpatch` | `json.patch` | 無い |
| `net` | `net.cidr_contains`、`net.cidr_is_valid`、`net.cidr_expand`(セキュリティグループの CIDR の判定など)。DNS の問い合わせなど、ネットワークにアクセスする関数は含まない | `ipnet` |
| `regex` | `regex.*` | `regex` |
| `semver` | `semver.*` | `semver` |
| `time` | `time.*`(認証情報レポートの日時の比較など) | `chrono`、`chrono-tz` |
| `urlquery` | `urlquery.*` | `url` |

| 有効にしない | 理由 |
| --- | --- |
| `http` | `http.send` を登録するが、regorus の実装は常に undefined を返す(ネットワークにはアクセスしない)。ルールの作者が使えると誤解しないよう、登録しない |
| `opa-runtime` | `opa.runtime` はビルドの情報を返すだけで、ルールの判定に使わない |
| `uuid` | `uuid.*` は乱数で値を作り、同じ入力で結果が変わる |
| `jsonschema`、`yaml` | ルールの判定に使わない。依存が大きい |
| `coverage` | ルールのテストでカバレッジを出す機能はこの範囲に無い(未決事項) |
| `rvm` | regorus の仮想マシン。`Engine` のインタープリタでの評価には使わない |
| `allocator-memory-limits`、`mimalloc` | 下の「評価時のメモリの上限」 |
| `azure_policy`、`azure-rbac`、`ast`、`no_std`、`opa-no-std`、`opa-testutil`、`rego-extensions`、`dashmap`、`rand` | Azure 用、AST の出力、`no_std`、テスト用などで、使わない |

- 方針: ファイル・ネットワークにアクセスせず、結果が入力だけで決まる組み込み関数の機能は、OPA の `full-opa` に含まれるものをすべて有効にする。ルールが使う組み込み関数を後から有効にすると、それを使うルールは `min_yata_version` を上げる必要があり、古い `yata` の利用者がルールを使えなくなるため、最初に広く入れる
- regorus のどの機能にも、ファイルを読む組み込み関数は無い(`Engine::add_policy_from_file` などのファイルを読む API は Rust の API で、Rego からは呼べない)
- 機能を問わず常に登録される組み込み関数のうち、`rand.intn` は乱数を使い、`time.now_ns`(`time` の機能)は現在の時刻を返す。どちらも結果が実行のたびに変わりうる。`time.now_ns` とフィクスチャの扱いは未決事項

### 評価時間の上限の実現方法

regorus 0.12.0 のソースで次を確かめ、`set_execution_timer_config` で評価を打ち切る方式に決めます。別スレッドで評価して時間切れで結果を捨てる方式は採りません。

- `Engine::eval_rule` は、評価の前に実行時間の制限の設定を適用し(`apply_effective_execution_timer_config`)、内部の状態を初期化するときにタイマーを開始する(`clean_internal_evaluation_state` → `reset_execution_timer_state`)。評価ごとに時間を測り直す
- インタープリタは文(statement)を評価するたびにタイマーを進め(`eval_stmt_impl` → `check_execution_time`)、`check_interval` の回数ごとに時計を読み、上限を超えていれば `LimitError::TimeLimitExceeded` を返す。`every`、`some ... in`、内包表記の中の文もこの経路を通る
- ポリシーの解析(`prepare_for_eval`)は計測に入らない。解析の時間はファイルの大きさの上限で抑える
- `check_interval` は 64 とする。時計を読む回数を減らしつつ、打ち切りの遅れを文 64 個分に抑える。性能を測って変えてよい(実装の細部)
- 時間切れのエラーは `anyhow::Error` で返るため、`downcast_ref::<regorus::LimitError>()` で見分け、`EvaluationError::Timeout` にする(見分けられない場合の扱いは下の「実装時の確認事項」)

**リソースごとの評価での上限の掛け方**(ADR-0030 で追加): 上限は、ルール × アカウント(フィクスチャでは 1 つのフィクスチャ)の、リソースごとの評価の合計に掛けます。`eval_rule` は呼び出しのたびにタイマーを開始し直すため、そのままではリソースごと(`eval_rule` の呼び出しごと)の上限になります。次のようにします。

- ルール × アカウントの評価を始めるときに、開始の時刻を記録する(`std::time::Instant`)。開始からの経過時間には、`add_data` と、最初の `eval_rule` での解析(regorus のタイマーの計測の外)の時間も含める
- `eval_rule` を呼ぶたびに、その前に「残りの時間 = 上限 − 開始からの経過時間」を求め、`set_execution_timer_config(ExecutionTimerConfig { limit: 残りの時間, check_interval: 64 })` で設定し直す。残りが 0 以下なら呼ばずに `Timeout` にする
- `set_execution_timer_config` は設定を置き換えるだけで、エンジンの再準備(解析のやり直し)を起こさない(regorus 0.12.0 のソースで確認。`prepared` を変えない)
- 時間切れの `Timeout { limit }` の `limit` は、残りの時間ではなく `--policy-timeout` の値にする(利用者に見せる上限)

**打ち切れないもの**: 確認は文の単位なので、1 回の組み込み関数の呼び出し(例: `net.cidr_expand("0.0.0.0/0")` で約 43 億件のアドレスを作る、`numbers.range` で巨大な配列を作る)の途中では止まりません。この場合は時間の上限が効かず、メモリを使い尽くすことがあります。

- 別スレッドで評価しても、Rust ではスレッドを外から止められず、計算とメモリの消費は続く。結果を捨てるだけで資源の問題は解決しないため、採らない
- ポリシーは信頼できる出所から取得したものを使う前提(全体設計の「(a) 信頼の前提」)で、時間の上限は悪意のあるポリシーではなく、誤りのあるルール(無限に近いループ、入力の大きさに対して計算量の大きい突き合わせ)を止めるためのもの
- ポリシーのリポジトリには、入力の値を `net.cidr_expand` や `numbers.range` に渡さない(`net.cidr_contains` などで判定する)ことを、ルールの書き方のガイドとして載せる(ポリシーのリポジトリの Issue)

### 評価時のメモリの上限

regorus の `allocator-memory-limits` の機能では制限しません。ファイルの大きさの上限と評価時間の上限で代えます。

- この機能を有効にすると、regorus が `mimalloc` をプロセス全体のグローバルアロケーターにする(`mimalloc::assign_global!()`)。AWS SDK を含む `yata` 全体のメモリの割り当てが C のアロケーターに変わり、ビルドに C コンパイラが要る
- 上限(`set_global_memory_limit`)はプロセス全体で 1 つで、プロセスのすべての割り当て(収集した `Inventory` を含む)を数える。ルールの評価ごとの上限にならない
- 確認は時間の上限と同じく文の単位で、上の「打ち切れないもの」と同じく 1 回の組み込み関数の呼び出しの途中では止まらない
- 大規模アカウントでのメモリの使い方は、全体設計の未決事項「大規模アカウントでのメモリ使用量」で扱う

### `input` と `data` の組み立て

(ADR-0030 で変更。変更前はルール × アカウントごとに 1 つの `input` を作り、`data` は使わなかった。)

`yata-engine` は、ルール × アカウントごとに、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ポリシーエンジンへの渡し方」の形の `data` を作り、`add_data` で 1 回入れます。`target_type` のリソースごとに、そのリソースを `set_input` で `input` にします。

- `data.yata.context.account`: そのアカウントの `AccountInfo`
- `data.yata.context.regions`: `Inventory.regions`
- `data.<provider>.<service>.<resource>`: 宣言した種別(`target_type` と `related_types`)ごとに、そのアカウントの収集済みの `ResourceSet` の `resources` を 1 つの配列にまとめ、`(id, region)` の昇順に並べて置く
- 収集済みの組が 0 件の種別は空の配列を置く。未収集・失敗の組の扱いは下の「状態の判定」
- `input`: `data` の `target_type` の配列の要素を、そのまま(`regorus::Value` を複製して)使う。`Value` は参照カウントで中身を共有するため、複製は配列の要素の参照を増やすだけで済む
- `serde_json::Value` から `regorus::Value` への変換は、(アカウント, 種別)ごとに 1 回にし、その種別を宣言したルールの間で共有してよい(性能の細部。実装時に決めてよい)

### 状態の判定

[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「状態の判定順」を、次のように実装します。

- 種別ごとの「対象の組」: `Inventory.resource_types` にその種別が無ければ、すべての組を未収集とする。あれば、`global` の種別は(アカウント, `global`)、`regional` の種別は(アカウント, `Inventory.regions` の各リージョン)を対象の組とする
- 組ごとの状態: `Inventory.resource_sets` に組があれば収集済み、`Inventory.collection_errors` にあれば失敗、どちらにも無ければ未収集
- 判定順の 1〜2(`skipped`)に当たれば評価しない。3 以降は評価してから決める
- `incomplete_resources`: `data` に入れた、宣言したすべての種別のリソースについて、`configuration` の各キーの値が、キーが `yata_error` だけのオブジェクトであれば、`IncompleteResource { resource_id, resource_type, region, operation: キー, kind: yata_error.kind }` を作る。`kind` が文字列でなければ `"unknown"` にする。(種別, `id`, `region`, 操作名)の昇順に並べる
- 状態が `skipped` / `error` のときは、`results` と `incomplete_resources` を空にする

### リソースごとの評価と結果

(ADR-0030 で変更。変更前は `fail` の集合の要素を解釈し、`resource_id` / `resource_region` で `target_type` のリソースに対応づけていた。)

図: [リソースごとの評価の流れ](../diagrams/Diagram-0030-Per-Resource-Evaluation.md)

ルール × アカウントごとに、ルールのエンジンを `clone()` し、`add_data` で `data` を入れた後、`target_type` のリソースを `(id, region)` の昇順に回して、リソースごとに次を行います。誤りはすべて `EvaluationError`(ルールの状態 `error`)にし、最初に見つけた誤りで、そのルール × アカウントの評価を止めます。エラーには、エラーが起きたリソースの `id` とリージョンを持たせます。

1. `set_input` で `input` をそのリソースにする
2. `data.yata.rules.<ルール ID>.fail` を `eval_rule` で評価する
   - 評価のエラー: 時間切れは `Timeout`、それ以外(conflict を含む)は `Engine`(regorus のメッセージ)
   - 未定義(`Value::Undefined`。ルールに `fail` が無いときの `not a valid rule path` のエラーを含むが、読み込み時に確かめているので通常は起きない): `FailUndefined`
   - 真偽値でない: `FailNotBoolean`(型の名前を持つ)
3. `false` なら `pass` の `RuleResult` を作り、次のリソースへ進む
4. `true` なら、`data.yata.rules.<ルール ID>.message` を `eval_rule` で評価する
   - ルールに `message` が無い(`not a valid rule path` のエラー)、または未定義: メタデータの `title` を使う
   - 評価のエラー: 2 と同じ
   - 空でない文字列でない: `InvalidMessage`(型の名前、または空であること)
5. `data.yata.rules.<ルール ID>.reason` を `eval_rule` で評価する
   - ルールに `reason` が無い、または未定義: `path`・`actual`・`expected` は `None`
   - 評価のエラー: 2 と同じ
   - オブジェクトでない: `InvalidReason`
   - `path`: 無いか、JSON Pointer(RFC 6901)の形の文字列。空の文字列か `/` で始まり、`~` の後が `0` か `1` であること。満たさなければ `InvalidReason`
   - `actual`・`expected`: 無いか、任意の値(`null` を含む)
   - 知らないキーは無視する
6. `fail` の `RuleResult` を作る。`message` は 4 の値、`path`・`actual`・`expected` は 5 の値(`actual` / `expected` の無いものは `None`、`null` のものは `Some(Value::Null)`)

- `path` に `null` を書いた場合は、文字列でないので誤りとする(`actual` / `expected` の `null` は「値が `null`」という意味を持つため区別する)
- `eval_rule` の呼び出しの前には、残りの時間を制限に設定する(上の「リソースごとの評価での上限の掛け方」)
- `take_prints()` は `eval_rule` の呼び出しごとに取り出し、ルール ID とリソースの `id` を付けて `debug` のログに出す
- `target_type` の配列に `(id, region)` の組が同じリソースが 2 つ以上あれば、評価の前に `DuplicateResource`(`error`。`Inventory` の検証で起きないはずの状態を、念のため確かめる)
- `target_type` のリソースが 0 件なら、`add_data` も評価もしない
- `EvaluationError` の `Display`(`RuleOutcome.error` に入れる 1 行の説明)のうち `yata-engine` が作る部分には、ルールが返した値を入れず、型の名前だけを入れる。例は [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「評価のエラー」
- `Engine` の `message`(regorus のメッセージ)は値を含みうる(例: 関数の conflict の `function produced multiple outputs ...`)。加工せずに持ち、`EvaluationError` に種類を返す `kind()` と、regorus のメッセージを除いた説明(`yata-engine` が作る説明とリソースの `id`・リージョン。値を含まない)を返す関数を用意する
- 種類(`kind()`)と variant の対応:
  - `Timeout` → `timeout`
  - `Engine` で、regorus のメッセージが完全ルール・関数の conflict のもの(`rules should not produce multiple outputs`、`complete rules should not produce multiple outputs`、`function produced multiple outputs` など。文言で見分ける)→ `conflict`
  - `Engine` のそれ以外 → `engine`
  - `FailUndefined`、`FailNotBoolean`、`InvalidMessage`、`InvalidReason`(`path` の形の誤りを含む)→ `type`(ルールの規約の誤り)
  - `DuplicateResource` → `internal`(エンジンに渡した入力の誤り。`Inventory` の検証で起きないはずの状態)
- ログ([エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) の「ルールの評価のエラーのログ」と同じ):

| レベル | 内容 |
| --- | --- |
| `warn` | ルール ID と種類(`kind()`)だけ |
| `debug` | 加えて、リソースの `id` とリージョン、`yata-engine` が作る説明、regorus のメッセージの全文 |

- `--omit-values` のときの `RuleOutcome.error` は、regorus のメッセージだけを省き、`yata-engine` が作る説明(リソースの `id`・リージョンを含む)は残す。反映はレポートを組み立てる `yata-cli` / `yata-report` の側で行う

### リソースごとの評価の性能

(ADR-0030 で追加。)

regorus 0.12.0 のソースで次を確かめました。

- `set_input` は入力を置き換えるだけで、エンジンの再準備(モジュールの解析、規則の収集、ループの巻き上げの表の作成)を起こさない。`add_data` は再準備を起こす(`prepared = false`)。このため、`data` をルール × アカウントごとに 1 回入れ、リソースごとに `input` だけを替える
- `eval_rule` は呼び出しのたびに評価の状態を初期化する(`clean_internal_evaluation_state`: `data` の作業用の複製、評価済みの規則の記録 `processed`、規則の値 `rule_values`、組み込み関数のキャッシュ `builtins_cache` を消す)。評価をまたいだ規則の値のキャッシュは無く、`data` だけに依存する補助のルールもリソースごとに計算し直される
- `data` の作業用の複製と `input` の差し替えは、`Value` が参照カウントで中身を共有するため、データの大きさによらず軽い

見積もり(実装時に測って確かめる):

- 1 回の評価の固定の費用(状態の初期化と `eval_rule` の準備)は小さく、単純なルール(`input` の数項目を見るもの)の 1 回の評価は数十マイクロ秒程度と見込む。100 ルール × 1 万リソースで 100 万回の評価なら、1 つのスレッドで数十秒程度になりうる。これまでの書式でもルールの本体はリソースの数だけ回っていたため、増えるのは主に評価ごとの固定の費用
- `fail` が `true` のリソースでは、`eval_rule` を最大 3 回(`fail`、`message`、`reason`)呼ぶ。評価のたびにキャッシュが消えるため、`message` / `reason` が参照する補助のルール(`fail` と共有するものを含む)は、その評価で計算し直される。不合格のリソースの評価の費用は、合格のリソースの最大 3 倍程度になる
- `data` の配列を走査するルールは、計算量が「対象のリソースの数 × 走査するリソースの数」になる(例: ユーザー 1,000 × ポリシー 1,000 で 100 万回の比較)。最初のルール群では、`target_type` が `aws.account.account`(1 件)や証跡(少数)のルールが `data` を走査し、件数の多い種別(ユーザー、セキュリティグループなど)のルールはほぼ `input` だけで判定するため、上限(ルール × アカウントで 30 秒)に収まる見込み
- 遅い場合の手段: ルールの評価の並列化(全体設計の「評価」。未決事項)、`data` だけに依存する計算の共有([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の未決事項)

### 公開する評価の関数

```rust
/// Evaluates `rules` against every account in `inventory`.
/// Returns one outcome per rule x account, ordered by rule ID then account key.
pub fn evaluate(rules: &[&Rule], inventory: &Inventory) -> Vec<RuleOutcome>;
```

- ルール × アカウントごとに、ルールが持つエンジンを `clone()` して使う(`&Rule` を変更しないため。リソースごとには `clone()` せず、`set_input` で `input` だけを替える)
- ルールの状態のエラーは `RuleOutcome.error` に、英語の 1 行の説明(`EvaluationError` の `Display`)として入れる。あわせて、エラーの種類と、regorus のメッセージを除いた説明も `RuleOutcome` に持たせる(`--omit-values` とログのため。項目名は実装時に決めてよい)

## `yata rules test`

### コマンド

```
yata rules test [--policy-dir <DIR>]... [--format <text|json>] [--policy-timeout <SECONDS>]
                [-v|-vv|-vvv | -q] [--color <auto|always|never>]
```

| オプション | 既定値 | 内容 |
| --- | --- | --- |
| `--policy-dir <DIR>`(複数指定可) | `YATA_POLICY_DIR`。どちらも無ければ終了コード 2 | 全体設計のとおり |
| `--format <text\|json>` | `text` | 標準出力の形式 |
| `--policy-timeout <SECONDS>` | 30 | フィクスチャ 1 つの評価(リソースごとの評価の合計)の時間の上限(1〜3600) |

- 出力はいつも標準出力に書く。ファイルに書くときはシェルのリダイレクトを使う(`--output` は設けない)
- `--format` の値は、`scan` の `table` / `json` / `sarif` とは別のものにする。テストの結果は表ではなく、項目ごとの失敗の一覧なので、`table` ではなく `text` とする

### 処理の流れ

1. `yata-cli` がポリシーのディレクトリを決める(全体設計のとおり。`YATA_POLICY_DIR` は `std::env::split_paths` で分ける)
2. `yata-engine` の `run_tests(dirs, options)` が、`LoadMode::RulesWithFixtures` で読み込み、下の検査を行って `TestReport` を返す。ディレクトリ全体の失敗があれば `Err(PolicyLoadError)`
3. `yata-cli` が、読み込めたすべてのルールの `target_type` / `related_types` を、`yata-provider-aws` の `supported_resource_types()` の種別名と照らし合わせ、対応していない種別があるルールに `unsupported_resource_type` の失敗を足す(`TestReport::add_failure`)。`target_type` の最初の部分が `aws` でないルールは、すべての種別を対応していないものとする
4. `yata-cli` が `--format` の形式で標準出力に書き、終了コードを決める

### 検査

ルールごとの検査の順です。読み込みに失敗したルールは、フィクスチャを検査せず、フィクスチャの結果を `not_run` にします(フィクスチャのディレクトリの `orphan_fixtures`・`symlink` の検査は読み込みの段階で行う)。

1. フィクスチャのディレクトリの中身(下の「フィクスチャのディレクトリ」)
2. `pass_` と `fail_` のフィクスチャが 1 つ以上ずつあるか(`fixtures_missing`)
3. フィクスチャごとに: 読み込み(大きさ、読めるか)→ 形式の検証(`fixture_invalid`)→ ダミー値(`non_dummy_account_id`)→ 評価と比較(`fixture_mismatch` / `fixture_eval_error` / `fixture_timeout`)

- 1 と 2 の誤り(フィクスチャのディレクトリの余計な項目、`pass_` / `fail_` の不足)があっても、そのルールの検査を止めず、3 をあるフィクスチャすべてに行う(1 回の実行で多くの問題を見せるため)
- 形式の誤りのあるフィクスチャは評価しない。ダミー値の誤りは、形式が正しければ評価もする(1 回の実行で多くの問題を見せるため)
- 1 つのフィクスチャで複数の誤りが見つかれば、種類ごとに 1 つの失敗にする

### フィクスチャのディレクトリ

- 中の項目は、ファイル名が `pass_<case>.json` か `fail_<case>.json`(`<case>` は 1 文字以上)の通常のファイルだけとする
- 名前が `.` で始まる項目(`.DS_Store` など)は無視する
- それ以外のファイルとディレクトリは `fixture_invalid`。名前を間違えたフィクスチャ(`pass_x.jsn` など)が黙って使われなくなるのを防ぐため
- フィクスチャは名前の昇順に実行する

### フィクスチャの検証

[ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) の「フィクスチャの形式」と [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「期待値の比べ方」の誤りに、次の細部を加えます。どれも `fixture_invalid` です。

- ファイルが JSON のオブジェクトでない。`description` があって文字列でない
(ADR-0030 で、フィクスチャのキーを `input` から `data` に変え、中身を Rego の `data` と同じ形にした。)

- `data` が無い、またはオブジェクトでない。最上位にキー `input` がある場合は、変更前の形式であることをメッセージで示す(`fixture uses the old "input" key; rename it to "data" and move account and regions under data.yata.context`)
- `data` の最上位に、`yata` とルールのプロバイダ(`target_type` の最初の部分)以外のキーがある
- `data.yata` がオブジェクトでない、または `context` 以外のキーがある。`data.yata.context` がオブジェクトでない、または `account` と `regions` 以外のキーがある(`data.yata` の下はエンジンが入れる名前のため)
- `data.yata.context.account` が `AccountInfo` の形(`account`・`id`・`provider` の文字列)でない。`data.yata.context.regions` が文字列の配列でない
- 宣言した種別(`target_type` と `related_types`)の配列が無い、または配列でない
- **宣言していない種別の配列がある**(例: `related_types` に書いていない `data.aws.s3.bucket`)。実際の評価ではエンジンは宣言した種別しか `data` に入れないため、宣言を忘れたルールがフィクスチャでだけ通るのを防ぐ
- 配列の要素が `Resource` の形(`id`・`type`・`account`・`region` が文字列、`configuration` がオブジェクト、`name` が無いか文字列、`tags` が無いか文字列の値のオブジェクト)でない。`type` がその配列の種別と違う。`account` が `data.yata.context.account.account` と違う
- 同じ種別の配列に `(id, region)` の組が同じリソースが複数ある
- `expected` が無い、またはオブジェクトでない。`expected.fail` が無い、または配列でない
- `expected.fail` の要素の誤り(全体設計の「期待値の比べ方」に挙げたもの)
- `pass_` のファイルで `expected.fail` が空でない。`fail_` のファイルで `expected.fail` が空
- 最上位と `expected` の知らないキーは無視する(前方互換)

### ダミー値

フィクスチャの JSON の、すべてのオブジェクトのキーと文字列の値(パースした後の値)から、ASCII の数字がちょうど 12 文字続く部分(前後が数字でない部分)を取り出し、下の一覧に無いものがあれば `non_dummy_account_id` にします。整数の値で、桁数がちょうど 12 のものも同じに調べます。

- ARN(`arn:aws:iam::123456789012:user/example`)、アカウントキー(`aws-123456789012`)、名前に埋め込んだ番号(`example-123456789012-logs`)も対象になる
- 失敗の報告には、見つけた場所の JSON Pointer だけを出し、値そのものは出さない(実際のアカウント ID であった場合に、CI のログに広げないため)
- アカウント ID でない 12 桁の数字(バイト数など)も対象になる。フィクスチャでは別の桁数の値を使う

#### ダミーとして認めるアカウント ID

| 値 | 出所 |
| --- | --- |
| `123456789012` | AWS のドキュメントで最も多く使われる例の値 |
| `111122223333`、`222233334444`、`333344445555`、`444455556666`、`777788889999`、`000011112222` | AWS のドキュメントで例に使われている値(複数アカウントの例) |
| `000000000000`、`111111111111`、`222222222222`、`333333333333`、`444444444444`、`555555555555`、`666666666666`、`777777777777`、`888888888888`、`999999999999` | 同じ数字を 12 個並べた値。一部は AWS のドキュメントでも例に使われている |

- 複数のアカウントを使うフィクスチャ(別のアカウントを許すバケットポリシーなど)のために、複数の値を認める
- AWS はこれらの値を例として予約したと明言していない。目的は実際のアカウント ID をフィクスチャにコミットする誤りを防ぐことで、見た目で例と分かる値に限る
- 一覧は `yata-engine/src/testing/dummy.rs` の定数に置く。一覧の変更は、ポリシーのリポジトリのフィクスチャの合否を変えるため、機能の PR に混ぜず単独の PR で行う

### フィクスチャの実行と比較

1. フィクスチャの `data` の宣言した種別の配列を `(id, region)` の昇順に並べ替える(実際の評価と同じ順にするため)
2. (ADR-0030 で変更)ルールのエンジンを `clone()` し、フィクスチャの `data` をそのまま `add_data` で入れて([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「フィクスチャの評価」)、上の「リソースごとの評価と結果」と同じ関数で `target_type` のリソースごとに評価し、`fail` になったリソースの `(id, region)` の集合を作る。評価時間の上限はフィクスチャ 1 つの評価の合計に掛ける。時間切れは `fixture_timeout`、それ以外の `EvaluationError` は `fixture_eval_error`(メッセージにリソースの `id` とリージョンを含む)
3. `expected.fail` から期待する集合を作り(全体設計の「期待値の比べ方」)、比べる。違えば `fixture_mismatch`。詳細に、期待したが不合格にならなかったもの(`missing`)と、期待しなかったが不合格になったもの(`unexpected`。結果の `message` を付ける)を入れる

- フィクスチャでは、状態(`incomplete` など)は比べない(ADR のとおり)
- 評価の `print` の出力は、ほかの評価と同じく `debug` のログに出す

### 終了コード

[CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) の `yata rules test` の行のとおりです。

| コード | 条件 | 標準出力 |
| --- | --- | --- |
| 0 | ルール単位の失敗が無い | 結果 |
| 1 | ルール単位の失敗が 1 つ以上ある | 結果 |
| 2 | 引数の誤り(`--format`・`--policy-timeout` の値を含む)、`YATA_LOG` の誤り(UTF-8 でない・`EnvFilter` の書式の誤り。`-v` / `-q` があれば読まない)、ポリシーのディレクトリの指定が無い(`YATA_POLICY_DIR` が空の要素だけのときを含む)、ディレクトリ全体の失敗 | 出さない。失敗の一覧を標準エラー出力に出す(ディレクトリ全体の失敗と、それまでに見つかったルール単位の失敗) |
| 3 | 標準出力への書き込みの失敗 | 途中まで出ていることがある |
| 3 | 内部のエラー(`spawn_blocking` のタスクの `JoinError`、パッケージのバージョン(`CARGO_PKG_VERSION`)の解析の失敗、tokio のランタイムの作成の失敗、ログの初期化の失敗)。実行時の致命的なエラーとして扱う | 出さない。理由を標準エラー出力に出す |
| 130 | Ctrl-C で中断した | 出さない |

- 中断: `yata-cli` は tokio のランタイムの中で `run_tests` を `spawn_blocking` で実行し、`tokio::signal::ctrl_c()` と同時に待つ。Ctrl-C が先なら、標準出力に何も書かず、標準エラー出力に中断した旨を書いて flush し、`std::process::exit(130)` で終わる
  - ランタイムを drop すると `spawn_blocking` のタスクの終了を待つため、`#[tokio::main]` から戻る形にはしない。`std::process::exit` は実行中のスレッドを待たずにプロセスを終える。この時点で一時ファイルなど後始末の要るものは無い(`rules test` はファイルを書かない)
  - テスト: 「処理と中断の通知を同時に待ち、中断が先なら `Interrupted` を返す」関数を、通知の future を引数で受け取る形にして単体テストで確かめる(終わらない処理と、すぐ完了する通知で、すぐ `Interrupted` が返ること)。実際のシグナルを送る統合テストは、プラットフォームごとの差が大きいため置かず、手元で確かめる

図: [yata rules test の流れと終了コード](../diagrams/Diagram-0008-Rules-Test-Flow.md)

### text の形式

人が読むための形式で、書式の互換性は保証しません(機械処理には JSON を使う)。

```
example_policies 0.1.0 (format 1): 3 rules

FAILED  example_policies.cloudtrail.multi_region_enabled (not run)
ok      example_policies.iam.password_policy_min_length (3 fixtures)
FAILED  example_policies.s3.bucket_public_access_block (2 fixtures)

failures:

example_policies.cloudtrail.multi_region_enabled
  [metadata_invalid] cloudtrail/multi_region_enabled.rego
    metadata.severity must be one of critical, high, medium, low, info

example_policies.s3.bucket_public_access_block
  [fixture_mismatch] s3/bucket_public_access_block.fixtures/fail_block_public_acls_disabled.json
    fixture result does not match expected.fail
    missing:    arn:aws:s3:::example-bucket (us-east-1)
    unexpected: arn:aws:s3:::other-bucket (us-east-1): Bucket arn:aws:s3:::other-bucket does not block public ACLs

(no rule)
  [orphan_fixtures] s3/old_rule.fixtures
    no rule file s3/old_rule.rego for this fixture directory

test result: FAILED. 1 passed; 2 failed; 3 failures (fixtures: 4 passed, 1 failed, 2 not run)
```

- ルールごとに 1 行(`ok` / `FAILED`)を出し、その後に失敗の詳細、最後に集計を出す。ポリシーのディレクトリが複数あれば、ディレクトリごとに見出しの行とルールの行を出す
- ルールの行と失敗の詳細は、JSON の `rules[]` と `failures[]` と同じ順に並べる(下の「JSON の形式」の表)。どのルールにも属さない失敗は、ポリシーのディレクトリごとに、そのディレクトリのルールの失敗の後に `(no rule)` の見出しでまとめる
- パスはポリシーのディレクトリからの相対パス(`/` 区切り)で、ポリシーのディレクトリ自体のパスは出さない(失敗は見出しのマニフェストの `name` で区別できる)
- `ok` / `FAILED` の色付けは `--color` と `NO_COLOR` に従う
- ルールの `message`、regorus のエラーのメッセージ、フィクスチャの値など、ポリシーのディレクトリから来た文字列は、書く前に次の文字を `\u{1b}` の形にエスケープする。改行もエスケープし、1 つの値が 1 行に収まるようにする(全体設計の「(c) 出力への注入」)
  - 制御文字: C0・C1 の制御文字と DEL(ANSI のエスケープシーケンスの ESC、改行、復帰を含む)
  - 行・段落の区切り: U+2028、U+2029
  - 双方向テキストの制御文字: U+200E、U+200F、U+202A〜U+202E、U+2066〜U+2069、U+061C と、非推奨の書式制御文字 U+206A〜U+206F
  - 理由: 端末や CI のログの表示を偽装されないようにするため。制御文字は色や行の書き換え、区切り文字は行の分割、双方向の制御文字は表示される文字の順序の入れ替え(`ok` に見せかけるなど)に使える
  - ほかの文字(日本語、絵文字、ゼロ幅の結合子 U+200D など)はそのまま出す
- `debug` 以上のログでは、評価の所要時間もルールごとに出す

### JSON の形式

機械処理のための形式です。`schema_version` は互換性の無い変更(項目の削除・意味の変更)のときに上げ、項目の追加では上げません。ADR-0030 で `unexpected[].messages`(配列)を `message`(文字列)に変えましたが、`yata rules test` の JSON はまだリリースしていないため `1` のままにしています。

```json
{
  "schema_version": 1,
  "tool": { "name": "yata", "version": "0.1.0" },
  "result": "failed",
  "summary": {
    "rules": { "total": 3, "passed": 1, "failed": 2 },
    "fixtures": { "total": 7, "passed": 4, "failed": 1, "not_run": 2 },
    "failures": 3
  },
  "policies": [
    { "name": "example_policies", "version": "0.1.0", "format_version": 1 }
  ],
  "rules": [
    {
      "id": "example_policies.cloudtrail.multi_region_enabled",
      "policy_set": "example_policies",
      "path": "cloudtrail/multi_region_enabled.rego",
      "target_type": null,
      "related_types": [],
      "status": "failed",
      "fixtures": [
        { "file": "fail_no_trail.json", "status": "not_run" },
        { "file": "pass_multi_region.json", "status": "not_run" }
      ]
    },
    {
      "id": "example_policies.s3.bucket_public_access_block",
      "policy_set": "example_policies",
      "path": "s3/bucket_public_access_block.rego",
      "target_type": "aws.s3.bucket",
      "related_types": [],
      "status": "failed",
      "fixtures": [
        { "file": "fail_block_public_acls_disabled.json", "status": "failed" },
        { "file": "pass_all_blocked.json", "status": "passed" }
      ]
    }
  ],
  "failures": [
    {
      "kind": "metadata_invalid",
      "policy_set": "example_policies",
      "rule_id": "example_policies.cloudtrail.multi_region_enabled",
      "path": "cloudtrail/multi_region_enabled.rego",
      "message": "metadata.severity must be one of critical, high, medium, low, info",
      "details": null
    },
    {
      "kind": "fixture_mismatch",
      "policy_set": "example_policies",
      "rule_id": "example_policies.s3.bucket_public_access_block",
      "path": "s3/bucket_public_access_block.fixtures/fail_block_public_acls_disabled.json",
      "message": "fixture result does not match expected.fail",
      "details": {
        "missing": [
          { "id": "arn:aws:s3:::example-bucket", "region": "us-east-1" }
        ],
        "unexpected": [
          {
            "id": "arn:aws:s3:::other-bucket",
            "region": "us-east-1",
            "message": "Bucket arn:aws:s3:::other-bucket does not block public ACLs"
          }
        ]
      }
    },
    {
      "kind": "orphan_fixtures",
      "policy_set": "example_policies",
      "rule_id": null,
      "path": "s3/old_rule.fixtures",
      "message": "no rule file s3/old_rule.rego for this fixture directory",
      "details": null
    }
  ]
}
```

(例では、3 つのルールのうち `iam.password_policy_min_length` を `rules` から省いています。値はすべてダミーです。)

| 項目 | 内容 |
| --- | --- |
| `schema_version` | この形式のバージョン(`1`)。`scan` の JSON の `schema_version` とは別の値 |
| `tool` | `yata` の名前とバージョン |
| `result` | `passed`(ルール単位の失敗が無い。終了コード 0)/ `failed`(終了コード 1) |
| `summary.rules` | ルールのファイルの数と、失敗の有無で分けた数 |
| `summary.fixtures` | フィクスチャの数と、状態で分けた数 |
| `summary.failures` | `failures` の要素の数 |
| `policies[]` | ポリシーのディレクトリごとのマニフェストの値(指定した順)。ディレクトリのパスは含めない |
| `rules[]` | ルールのファイルごとの結果。ポリシーのディレクトリの順 → `path` の順 |
| `rules[].id` | ルール ID。名前の規則に合わず ID を作れない場合は `null` |
| `rules[].policy_set` | マニフェストの `name` |
| `rules[].path` | ポリシーのディレクトリからの相対パス(`/` 区切り) |
| `rules[].target_type` / `related_types` | メタデータの値。メタデータを読めなかった場合は `null` / `[]` |
| `rules[].status` | `passed` / `failed`(そのルールに `failures` の要素が 1 つ以上ある) |
| `rules[].fixtures[]` | フィクスチャのファイルごとの結果(名前の順)。`status` は `passed` / `failed`(形式の誤り・ダミー値・不一致・評価のエラー・時間切れのどれか)/ `not_run`(ルールの読み込みに失敗した) |
| `failures[]` | ルール単位の失敗。ポリシーのディレクトリの順に、まずそのディレクトリのルールに属する失敗(`rules[]` の順 → 見つけた順)、次にどのルールにも属さない失敗(`orphan_fixtures`。`path` の順)を並べる |
| `failures[].kind` | 上の「失敗の種類」の `RuleFailureKind` の値 |
| `failures[].rule_id` | ルール ID。ルールに属さない、または ID を作れない場合は `null` |
| `failures[].path` | 失敗の場所(ルールのファイル、フィクスチャのファイル、フィクスチャのディレクトリなど)の相対パス |
| `failures[].message` | 英語の説明 |
| `failures[].details` | 種類ごとの詳細。無ければ `null`(下の表) |

| `kind` | `details` |
| --- | --- |
| `fixture_mismatch` | `missing`: `{id, region}` の配列。`unexpected`: `{id, region, message}` の配列(`message` は結果の `message` の文字列)。どちらも `(id, region)` の昇順。ADR-0030 で、1 つのリソースに結果が 1 件になったため、`messages`(配列)を `message`(文字列)に変えた |
| `non_dummy_account_id` | `locations`: JSON Pointer の文字列の配列(値は出さない) |
| `unsupported_resource_type` | `types`: 対応していない種別の文字列の配列 |
| `fixture_timeout` | `timeout_seconds`: 上限の秒数 |
| そのほか | `null` |

- JSON ではシリアライズの際に制御文字がエスケープされるため、text の形式のエスケープは行わない
- `failures[].message` と `details` は、評価の結果を除き、ポリシーのディレクトリの内容をそのまま埋め込まない(regorus のエラーのメッセージは埋め込む)

## Windows の reparse point の判定

標準ライブラリの `std::fs::symlink_metadata(path)?.file_type().is_symlink()` で判定します。依存クレートは追加しません。

- Rust の標準ライブラリ(Windows)の `FileType` は、属性に `FILE_ATTRIBUTE_REPARSE_POINT` があり、かつ reparse tag に「名前の代理(name surrogate)」のビット(`0x20000000`)がある項目を `is_symlink()` とする。`symlink_metadata` は `FILE_FLAG_OPEN_REPARSE_POINT` で開いて reparse tag を読む
- シンボリックリンク(`IO_REPARSE_TAG_SYMLINK`)と junction(`IO_REPARSE_TAG_MOUNT_POINT`)は、どちらも名前の代理の reparse point なので、`is_symlink()` が真になる。`is_dir()` / `is_file()` は偽になる
- 名前の代理でない reparse point(OneDrive などのクラウドのファイルのプレースホルダー、重複除去のファイルなど)は、ほかの場所を指すものではなく、そのファイル自身の中身を表すもので、通常のファイル・ディレクトリとして扱う。全体設計の「(d) パスの扱い」の目的(ディレクトリの外のファイルを読まない、走査を循環させない)は、別の場所を指す名前の代理の reparse point を止めれば満たせる。すべての reparse point を止めると、OneDrive の同期フォルダーに置いたポリシーのディレクトリが使えなくなる
- Unix ではシンボリックリンクだけが `is_symlink()` になる

## 構成

### `yata-engine`

```
crates/yata-engine/
├── Cargo.toml
├── src/
│   ├── lib.rs               # モジュールの宣言と公開 API の再エクスポート
│   ├── error.rs             # PolicyLoadError、DirectoryFailure(Kind)、RuleFailure(Kind)、FailureDetails、EvaluationError
│   ├── limits.rs            # ResourceLimits と既定値
│   ├── names.rs             # 名前の文字の規則、Rego の予約語、リソース種別の形式
│   ├── version.rs           # YataVersion(x.y.z の解析と比較)
│   ├── manifest.rs          # Manifest の読み込みと検証、SUPPORTED_FORMAT_VERSIONS
│   ├── files.rs             # 上限付きの読み込み、シンボリックリンク・reparse point の判定
│   ├── walk.rs              # 走査(深さ・数の上限、項目の分類)
│   ├── engine_config.rs     # 基本のエンジンの作成(タイマー、print、v1)、時間切れの見分け
│   ├── library.rs           # ライブラリの読み込みと検証
│   ├── rule.rs              # Rule とルールの読み込み・検証
│   ├── metadata.rs          # メタデータの取り出しと検証(RuleMetadata への変換)
│   ├── loader.rs            # load_policies、LoadOptions、LoadMode、LoadedPolicies、PolicySet
│   ├── input.rs             # Inventory からの data の組み立て(P2。ADR-0030 で input から data に変更)
│   ├── findings.rs          # リソースごとの評価と fail / message / reason の解釈(P2。testing と共有。ADR-0030 で fail の要素の解釈から変更。名前は実装時に変えてよい)
│   ├── evaluate.rs          # evaluate、状態の判定、incomplete_resources(P2)
│   ├── testing.rs           # testing のモジュールの宣言(P3)
│   └── testing/
│       ├── fixture.rs       # フィクスチャのディレクトリの検査、フィクスチャの読み込みと検証
│       ├── expected.rs      # expected.fail の解釈と比較
│       ├── dummy.rs         # ダミーのアカウント ID の一覧と検査
│       ├── runner.rs        # run_tests
│       └── report.rs        # TestReport、RuleTestResult、FixtureResult、TestSummary(Serialize)
└── tests/
    ├── load.rs              # 読み込みのテスト(P1)
    ├── evaluate.rs          # 評価のテスト(P2)
    ├── rules_test.rs        # ルールのテストの機能のテスト(P3)
    └── fixtures/policies/   # テスト用のポリシーのセット(下の「テスト方針」)
```

### 公開 API(`yata-engine`)

名前の細部は実装時に変えてよいものとします。公開する項目には doc コメントを書きます。

```rust
/// Resource limits applied while loading a policy directory.
pub struct ResourceLimits {
    pub max_manifest_bytes: u64,   // 64 KiB
    pub max_rego_bytes: u64,       // 1 MiB
    pub max_fixture_bytes: u64,    // 4 MiB
    pub max_files: usize,          // 20_000
    pub max_directories: usize,    // 5_000
    pub max_depth: usize,          // 10
}
impl Default for ResourceLimits { /* the values above */ }

/// Default for `--policy-timeout`.
pub const DEFAULT_POLICY_TIMEOUT: Duration = Duration::from_secs(30);

/// `x.y.z` version of the running `yata`, without pre-release or build metadata.
pub struct YataVersion { /* major, minor, patch */ }
impl YataVersion {
    pub fn from_package_version(version: &str) -> Result<Self, VersionError>;
}

pub enum LoadMode {
    /// Rules and libraries only (scan, collect, rules list, permissions).
    Rules,
    /// Rules, libraries and fixture directories (rules test).
    RulesWithFixtures,
}

pub struct LoadOptions {
    pub yata_version: YataVersion,
    pub limits: ResourceLimits,
    pub policy_timeout: Duration,
    pub mode: LoadMode,
}

pub fn load_policies(dirs: &[PathBuf], options: &LoadOptions)
    -> Result<LoadedPolicies, PolicyLoadError>;

pub struct LoadedPolicies {
    pub policy_sets: Vec<PolicySet>,
    /// Rule-level failures. Exit code 2 outside `rules test`.
    pub rule_failures: Vec<RuleFailure>,
}
impl LoadedPolicies {
    /// Successfully loaded rules, in directory order then rule ID order.
    pub fn rules(&self) -> impl Iterator<Item = &Rule>;
}

pub struct PolicySet { /* info: PolicySetInfo, rules, fixture directories (private) */ }
impl PolicySet {
    pub fn info(&self) -> &PolicySetInfo;
    pub fn min_yata_version(&self) -> &str;
}

pub struct Rule { /* id, policy_set, path, metadata, regorus engine (private) */ }
impl Rule {
    pub fn id(&self) -> &str;
    pub fn policy_set(&self) -> &str;
    /// Path relative to the policy directory, `/`-separated.
    pub fn path(&self) -> &str;
    pub fn metadata(&self) -> &RuleMetadata;
}

pub fn evaluate(rules: &[&Rule], inventory: &Inventory) -> Vec<RuleOutcome>;   // P2

pub mod testing {                                                               // P3
    /// Loads with `LoadMode::RulesWithFixtures` and runs every check owned by the engine.
    pub fn run_tests(dirs: &[PathBuf], options: &LoadOptions)
        -> Result<TestReport, PolicyLoadError>;

    pub struct TestReport { /* policies, rules, failures */ }
    impl TestReport {
        pub fn rules(&self) -> &[RuleTestResult];
        /// Adds a failure found outside the engine (e.g. provider coverage).
        pub fn add_failure(&mut self, failure: RuleFailure);
        pub fn passed(&self) -> bool;
        pub fn summary(&self) -> TestSummary;
    }
}
```

- `regorus` の型は公開 API に出さない(エンジンを差し替える可能性に備えるため。[ADR: ポリシーエンジン](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md))
- `LoadedPolicies`・`TestReport` などは、`Rule` が regorus のエンジンを持つため `Clone` を実装しない

### エラーの型(`error.rs`)

```rust
/// Policy loading failed with at least one directory-level failure (exit code 2).
#[derive(Debug, thiserror::Error)]
#[error("failed to load policies ({} directory-level failure(s))", .directory_failures.len())]
#[non_exhaustive]
pub struct PolicyLoadError {
    pub directory_failures: Vec<DirectoryFailure>,
    pub rule_failures: Vec<RuleFailure>,
}

pub struct DirectoryFailure {
    /// Policy directory as given by the caller.
    pub policy_dir: PathBuf,
    /// Location relative to the policy directory, if any.
    pub path: Option<String>,
    pub kind: DirectoryFailureKind,
    pub message: String,
}

pub struct RuleFailure {
    pub policy_set: String,
    pub rule_id: Option<String>,
    pub path: String,
    pub kind: RuleFailureKind,
    pub message: String,
    pub details: Option<FailureDetails>,
}

#[non_exhaustive] pub enum DirectoryFailureKind { NotFound, NotADirectory, Unreadable, ManifestMissing, /* ... */ }
#[non_exhaustive] pub enum RuleFailureKind { InvalidName, Symlink, FileTooLarge, /* ... */ }
#[non_exhaustive] pub enum FailureDetails { Mismatch { .. }, Locations { .. }, Types { .. }, Timeout { .. } }

/// Error while evaluating one rule (rule status `error`).
/// Variants changed by ADR-0030 (per-resource evaluation).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EvaluationError {
    /// `resource` is the resource being evaluated when the limit was hit, if any.
    Timeout { limit: Duration, resource: Option<ResourceKey> },
    Engine { message: String, resource: ResourceKey },
    FailUndefined { resource: ResourceKey },
    FailNotBoolean { found: &'static str, resource: ResourceKey },
    InvalidMessage { reason: String, resource: ResourceKey },
    InvalidReason { reason: String, resource: ResourceKey },
    DuplicateResource { resource_id: String, region: String },
}

/// `(id, region)` of the target resource an evaluation error belongs to.
pub struct ResourceKey { pub id: String, pub region: String }
```

- `FailNotSet`、`InvalidFailElement`、`UnknownResource` は ADR-0030 で廃止した
- `found` と `reason` には型の名前と規則の説明だけを入れ、ルールが返した値は入れない

- 種類の enum は `serde` で snake_case の文字列にする(JSON の `kind`)
- `PolicyLoadError` は、全体設計の [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) の `PolicyLoadError` に当たる。ルール単位の失敗は、`rules test` で 1 にするため、エラーの型ではなく `RuleFailure` の一覧で返す
- 元のエラー(I/O、regorus)は、メッセージの文字列にして持つ。regorus の `anyhow::Error` を公開 API に出さないため
- regorus のエラーのメッセージには、誤りのある行の Rego のソースの抜粋が含まれる。これはそのまま出す。抜粋されるのはポリシーのリポジトリのルール・ライブラリの本文で秘密の値ではなく、ルールを書く人が誤りの場所を見つけるのに要るため。端末に出すときの制御文字のエスケープは `yata-cli` の出力側(`sanitize.rs`)で行う

### `yata-core` に足す型

[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「結果の型」と [出力形式](../architecture/Architecture-Output-Formats.ja.md) の型を、1 つの型(と関連する小さな型)ごとに 1 ファイルで置きます。どれも `serde` の `Serialize` / `Deserialize` を付け、列挙の値は snake_case にします。

| ファイル | 型 | 段階 |
| --- | --- | --- |
| `severity.rs` | `Severity`(`Critical` / `High` / `Medium` / `Low` / `Info`。`Ord` は重要度の順) | P1 |
| `rule_metadata.rs` | `RuleMetadata`(`title`、`severity`、`description`、`remediation`、`target_type`、`related_types`、`references`、`tags`)、`Reference`(`standard`、`version`、`control`、`url: Option<String>`) | P1 |
| `policy_set_info.rs` | `PolicySetInfo`(`name`、`version`、`format_version`) | P1 |
| `rule_outcome.rs` | `RuleOutcome`、`RuleStatus`、`SkipReason`、`IncompleteResource`(`resource_id`、`resource_type`(JSON では `resource_type`)、`region`、`operation`、`kind`) | P2 |
| `rule_result.rs` | `RuleResult`、`ResultStatus`、`ResultResource`(`id`、`type`、`account`、`region`) | P2 |

- `RuleResult` の `message` / `path` / `actual` / `expected` は `Option` で持つ。JSON の出し方(`pass` で置かない、`fail` で `null` を置く、`--omit-values` で省く)はレポートの Issue で決める(シリアライズの細部は `ScanReport` 側で行う)
- `yata-core/src/lib.rs` にモジュールの宣言と再エクスポートを足す

### `yata-cli`(P4)

```
crates/yata-cli/src/
├── main.rs                  # ランタイムの作成、コマンドの実行、終了コード
├── cli.rs                   # clap の定義(rules test のサブコマンドと共通のオプション)
├── exit_code.rs             # 終了コードの定数と、エラーからの対応づけ
├── logging.rs               # -v / -q / YATA_LOG によるログの初期化
├── policy_dir.rs            # --policy-dir / YATA_POLICY_DIR の決定
├── sanitize.rs              # 端末に書く文字列の制御文字のエスケープ
└── commands/
    ├── rules_test.rs        # rules test の処理(読み込み、プロバイダの対応の検査、出力、終了コード)
    └── rules_test/
        ├── text.rs          # text の形式
        └── json.rs          # JSON の形式(schema_version、tool を付けて TestReport を書く)
```

- 既に CLI の骨組みがあれば、`main.rs`・`cli.rs`・`exit_code.rs`・`logging.rs`・`policy_dir.rs` はそれを使い、`rules test` に要るものだけを足す
- `sanitize.rs` は、レポートの Issue で出力への注入の扱い(全体設計の「(c)」)を決めるときに、`yata-report` などの共通の場所へ移してよい

## 依存の追加

P1 で、ルートの `Cargo.toml` の `regorus` の行を変え、`Cargo.lock` を更新します(#7 の実装の PR の後。上の「共有ファイルの順序」)。`Cargo.lock` は P4 の `yata-cli` の依存の追加でも変わります。新しいクレートはワークスペースに追加しません(使うクレートは、いずれも既にルートの `[workspace.dependencies]` にある)。ルートの `Cargo.toml` と `Cargo.lock` は共有ファイルのため、P1 の実装の前にメンテナーの了承を得ます。

### ルートの `Cargo.toml`(変更が必要)

```toml
regorus = { version = "0.12.0", default-features = false, features = [
    "arc", "std", "base64", "base64url", "cache", "glob", "graph", "hex",
    "jsonpatch", "net", "regex", "semver", "time", "urlquery",
] }
```

- ワークスペースの依存で既定の機能が有効だと、メンバーのクレートでは外せないため、ルートで `default-features = false` にする
- バージョンは今の `0.12.0` のまま変えない(無関係な依存の更新を混ぜないため)
- 既定の機能(`coverage`、`http`、`jsonschema`、`opa-runtime`、`rvm`、`uuid`、`yaml` など)を外すため、`jsonschema`、`yaml_serde`、`uuid`、`postcard` などのクレートは入らない。P1 の前は `regorus` を使うクレートが無く `Cargo.lock` に入っていなかったため、P1 では `regorus` とその依存(`verus_*` / `vstd` を含む。いずれも MIT か MIT OR Apache-2.0)が増えるだけで、外れるクレートは無い

### 各クレートの `Cargo.toml`

| クレート | `[dependencies]` に足す | `[dev-dependencies]` に足す | 段階 |
| --- | --- | --- | --- |
| `yata-engine` | `regorus`、`serde`、`serde_json`、`thiserror`、`tracing` | `tempfile` | P1 |
| `yata-core` | なし(`serde`、`serde_json` は #7 の実装の PR で入る) | - | - |
| `yata-cli` | `clap`、`tokio`、`tracing`、`tracing-subscriber`、`serde`、`serde_json`(CLI の骨組みが先にあれば、足りないものだけ) | `assert_cmd`、`tempfile` | P4 |

- どれもワークスペースに既にあるクレートで、`xxx.workspace = true` で参照する。ライセンスは [初期依存クレート](../architecture/Architecture-Initial-Dependencies.ja.md) のとおり
- メンバーのクレートの依存の追加でも `Cargo.lock` のそのクレートの依存の一覧が変わる。P4 で `yata-cli` の依存を足す場合は、P4 の実装の前にメンテナーの了承を得る(P1 で `yata-cli` の依存を先に足すと、CLI の骨組みの Issue と衝突しやすいため、P4 で足す)
- 依存の向きは全体設計のとおり(`yata-engine` → `yata-core`、`yata-cli` → 各クレート)。`yata-engine` は `yata-provider-aws` に依存しない

## 使う AWS API と IAM 権限

ありません。`yata-engine` と `yata rules test` は AWS に接続しません。プロバイダの対応の検査は、`yata-provider-aws` の `supported_resource_types()`(AWS に接続しない関数)だけを使います。

## テスト方針

[テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) に従い、実 AWS とネットワークには接続しません。フィクスチャの値はダミーだけを使います。ファイルを書くテストは `tempfile` の一時ディレクトリを使います。

### テスト用のポリシーのセット(`crates/yata-engine/tests/fixtures/policies/`)

ルールの `target_type` には、実際のコレクターに依存しない架空の種別(`aws.example.widget`、`aws.example.gadget`、`aws.example.trail`)と `aws.account.account` を使います(`yata-engine` はプロバイダの対応を検査しないため)。マニフェストの `name` は `test_policies` とします。

```
policies/
├── valid/                                    # 正しいセット(rules test が合格する)
│   ├── yata-policies.json
│   ├── README.md                             # 無視されること
│   ├── .hidden/broken.rego                   # 走査されないこと(構文エラーのファイル)
│   ├── lib/
│   │   └── helpers.rego                      # package yata.lib.test_policies.helpers(失敗の印の判定など)
│   ├── account_rule.rego                     # ルートの直下のルール(aws.account.account)
│   ├── account_rule.fixtures/{pass_*.json, fail_*.json}
│   ├── example/
│   │   ├── widget_tagged.rego                # related_types に aws.example.gadget、lib を使う
│   │   ├── widget_tagged.fixtures/           # 失敗の印を含むフィクスチャ、0 件の配列のフィクスチャを含む
│   │   └── nested/deep/
│   │       ├── deep_rule.rego                # 複数の階層
│   │       └── deep_rule.fixtures/...
│   └── regional/
│       ├── trail_region.rego                 # 同じ id が複数のリージョンに現れる種別。input.region で一部のリージョンのコピーだけを fail にする(ADR-0030 で変更)
│       └── trail_region.fixtures/            # expected.fail の文字列の形とオブジェクトの形の両方
├── dir_failures/<case>/                      # ディレクトリ全体の失敗。1 ケース 1 ディレクトリ
│   ├── manifest_missing/
│   ├── manifest_invalid_json/
│   ├── manifest_missing_key/
│   ├── manifest_name_reserved/
│   ├── manifest_name_digit/
│   ├── manifest_version_format/              # min_yata_version が x.y.z でない
│   ├── manifest_format_version/              # format_version が対応外
│   ├── manifest_min_version_too_new/
│   ├── duplicate_name_a/、duplicate_name_b/  # 2 つを同時に指定して duplicate_name
│   ├── library_syntax/
│   ├── library_package/
│   ├── library_has_metadata/
│   ├── library_has_fail/
│   └── no_rules/
└── rule_failures/                            # ルール単位の失敗。1 つのセットにまとめ、全部が 1 回で報告されることを確かめる
    ├── yata-policies.json
    ├── ok/passing_rule.rego (+ fixtures)     # ほかが失敗しても合格すること。フィクスチャにダミーの値 123456789012 を含める
    ├── invalid_name/1digit.rego、invalid_name/as.rego、Bad-Dir/rule.rego
    ├── rego_error/syntax.rego
    ├── rego_error/slow_metadata.rego         # 入力に依存しない重い計算で metadata を作る。読み込み時の時間切れが rego_error になること
    ├── package_mismatch/wrong_package.rego
    ├── metadata/{missing_key, bad_severity, bad_type, related_other_provider, related_has_target, input_dependent}.rego
    ├── fail_missing/{no_fail, no_default, partial_set, not_boolean}.rego   # ADR-0030 で変更: default の無い fail、fail contains の書式、入力なしで真偽値でない fail
    ├── rule_contract/{undefined_at_runtime, fail_not_boolean, fail_conflict, message_not_string, message_empty, message_conflict, reason_not_object, reason_bad_path, reason_path_null}.rego (+ fixtures)   # ADR-0030 で fail_element から置き換え。fixture_eval_error になること
    ├── expected/{dup_string, dup_object, string_and_object, object_missing_region, region_not_string, unknown_id, unknown_id_region}.rego (+ fixtures)
    ├── fixture_format/{data_dup_id_region, missing_declared_type, undeclared_type, bad_resource, pass_with_fail, fail_without_fail, unknown_file, old_input_key, bad_context, unknown_yata_key}.rego (+ fixtures)   # ADR-0030 で、フィクスチャの data の形の誤り(変更前の input のキー、data.yata.context の誤り、data.yata の知らないキー)を追加
    ├── fixtures_missing/{no_dir, no_fail_fixture}.rego
    ├── orphan/gone.fixtures/
    ├── mismatch/wrong_expectation.rego (+ fixtures)
    └── timeout/slow.rego (+ fixtures)        # 入力があるときだけ遅くなるルール(下の注)。テストでは短い policy_timeout を渡す
```

- 時間切れのケースのルール(`timeout/slow.rego` と、P2 の評価のテストで使う「終わらないルール」)は、`data` の配列(フィクスチャの数百件の要素)を三重の `some ... in` で回すなど、`data` があるときだけ遅くなる書き方にする。読み込み時の `fail` の確認は入力なしで評価するため、入力が無くても遅いルールは読み込みの `rego_error` になり、`fixture_timeout` / 評価の `error` のケースにならない。読み込み時の時間切れは `rego_error/slow_metadata.rego` で別に確かめる
- `rule_contract/undefined_at_runtime.rego` は、`default` が無く、入力なしでは本体が成り立つ(`fail if not input.x` など)ため読み込みを通り、`pass_` のフィクスチャで `fail` が undefined になるルールにする
- 評価時間の上限がリソースごとの評価の合計に掛かることは、P2 の評価のテストで、1 つのリソースの評価は上限より短いが、全リソースの合計は上限を超えるルールと入力で確かめる
- 1 回の組み込み関数の呼び出しの途中では止まらないため、遅さは `numbers.range` などの 1 回の呼び出しではなく、文の繰り返しで作る
- テストで渡す短い `policy_timeout` は 1 秒とし、遅いルールは上限が無ければ数十秒以上かかる量にする。`ok` などのほかのルールは数ミリ秒で終わる小ささにし、遅い CI でも上限との間に十分な差を取る

- 1 ケースを 1 ディレクトリ(ルール単位の失敗では 1 ルールのファイル)に分ける。ルール単位の失敗を 1 つのセットに置くのは、1 回の実行で全ルールの失敗が集まることを確かめるため
- 次のケースはリポジトリに置かず、テストの中で一時ディレクトリに作る: シンボリックリンク(ルールのファイル、フィクスチャのディレクトリの中、ディレクトリ、ライブラリ、マニフェスト)、Windows の junction、大きさの上限を超えるファイル、ファイル数・ディレクトリ数の上限、深さの上限、読めないファイル(Unix のみ)
  - 上限のテストは `ResourceLimits` に小さい値を渡して行う
  - Unix のシンボリックリンクは `std::os::unix::fs::symlink` で作る。Windows のシンボリックリンクは権限が無いと作れないため、作れなければそのテストを飛ばして標準エラー出力にその旨を出す。junction は `cmd /C mklink /J` で作る(`#[cfg(windows)]`。CI は Linux のため、Windows の確認は手元で行う)
- 「ダミーでない 12 桁の数字」(`non_dummy_account_id`)のケースは、リポジトリにファイルを置かない。どの 12 桁の数字も実在のアカウント ID でありうるため、一覧に無い 12 桁の値をコミットしない
  - テストの中で、`rule_failures/ok` のルールとフィクスチャを一時ディレクトリに写し、フィクスチャの中のダミーの値(`123456789012`)を、一覧の値から機械的に作った一覧に無い値(例: 最後の 1 桁を変えたもの。一覧に無いことをテストの中で確かめる)に置き換えて実行する
  - 作り方をテストのコードのコメントに書く

### 確かめること

| 段階 | テスト |
| --- | --- |
| P1 | `valid` が読み込め、ルールが ID の順に並ぶ。`.hidden` と `README.md` が無視される。`lib` のパッケージを使うルールが読める。`dir_failures` の各ケースで決めた `DirectoryFailureKind` になる。複数のディレクトリを指定し、1 つの失敗でほかのディレクトリの処理が続く。`rule_failures` で、読み込みの段階のルール単位の失敗がすべて集まり、`ok` のルールは読み込める。`LoadMode::Rules` でフィクスチャのディレクトリの中のシンボリックリンクが失敗にならず、`LoadMode::RulesWithFixtures` で `symlink` になる。上限・シンボリックリンク・junction の各ケース。`YataVersion::from_package_version` がプレリリースとビルドメタデータを除く。名前の規則と予約語、リソース種別の形式の単体テスト |
| P2 | 偽の `Inventory`(ダミー値)で、状態の判定順の 6 つの場合(未収集、すべて失敗、評価のエラー、一部のリージョンの失敗、失敗の印、正常)と、`aws.account.account` が常に収集済みであること。(ADR-0030 で変更)`target_type` のリソースごとに 1 件の結果ができ、`(id, region)` の昇順に並ぶ。`input` が評価の対象のリソースで、`data` に宣言した種別だけが `(id, region)` の昇順で入り、`data.yata.context` に `account` と `regions` が入る。同じ `id` の複数のリージョンのリソースが別々に評価される。`message` が無いときに `title` が使われ、`reason` の `path` / `actual` / `expected`(`null` を含む)が結果に写る。ルールの規約の各誤り(`fail` が undefined・真偽値でない、`message` が文字列でない・空、`reason` がオブジェクトでない、`path` が JSON Pointer でない・`null`、`fail` と `message` の conflict)で `error` になり、説明にリソースの `id` とリージョンが入り、値が入らない。`fail` が `false` のリソースでは `message` / `reason` が評価されない(`message` に誤りのあるルールでも `pass` のリソースだけなら `error` にならない)。時間切れで `error`(短い上限と終わらないルール、および 1 つずつは短いが合計で上限を超えるルール)。`target_type` のリソースが 0 件なら評価しない。`incomplete_resources` に `region` と `operation` が入り、昇順に並ぶ。`print` の出力が標準出力に出ない |
| P3 | `valid` で `run_tests` が合格する(`expected.fail` の文字列とオブジェクトの両方、一部のリージョンのコピーだけを `fail` にするルール)。`rule_failures` で、フィクスチャに関するすべての失敗の種類(`non_dummy_account_id` は上の一時ディレクトリのケースで)が、決めた `RuleFailureKind` と `details` で報告され、`ok` のルールは合格する。読み込みに失敗したルールのフィクスチャが `not_run` になる。`TestReport` の JSON(`Serialize`)の形 |
| P4 | `assert_cmd` で `yata rules test`: 正しいセットで 0、ルール単位の失敗で 1、ディレクトリ全体の失敗・指定なし・`--format` の誤り・`--policy-timeout 0` で 2 と標準出力が空であること。対応していない種別(`aws.example.unsupported`)のルールで 1 と `unsupported_resource_type`。`--format json` の出力を `crates/yata-cli/tests/golden/` の期待するファイルと比べる(更新は `YATA_UPDATE_GOLDEN=1` のときだけ)。text の出力に制御文字がエスケープされて出る。`--policy-dir` と `YATA_POLICY_DIR` の優先度。子プロセスの環境変数は全体設計の「実 AWS に接続しないための対策」のとおりに設定する。中断を待つ関数の単体テスト(上の「終了コード」の中断) |

### CLI の統合テスト用のセット(`crates/yata-cli/tests/fixtures/policies/`)

| セット | 内容 |
| --- | --- |
| `rules_test_ok/` | `target_type` が `aws.account.account` のルールとフィクスチャ。合格する |
| `rules_test_fail/` | 期待と違うフィクスチャを持つルールと、制御文字を含む `message` を返すルール。終了コード 1 |
| `unsupported_type/` | `target_type` が `aws.example.unsupported` のルール(フィクスチャは合格する)。終了コード 1 |
| `manifest_missing/` | マニフェストの無いディレクトリ(空のディレクトリは Git に残らないため、ルールのファイルだけを置く)。終了コード 2 |

## 実装時の確認事項

ドキュメントとソースの読み取りで確かめきれなかった点です。実装の最初に確かめ、結果が違えば設計書を直してから進めます。

| 項目 | 確かめること | 違った場合 |
| --- | --- | --- |
| 時間切れのエラーの見分け | `eval_rule` の時間切れのエラーが `anyhow::Error::downcast_ref::<regorus::LimitError>()` で取り出せるか | メッセージの文字列で見分ける。それも不安定なら、評価の前後の経過時間が上限以上かで見分ける |
| `clone()` とタイマー | `Engine::clone()` で、タイマーの設定と `print` を集める設定が複製されるか | 複製の後に設定し直す |
| 未定義のルールの評価 | 存在しないルールのパス(`fail` の無いルール、`metadata` の無いライブラリ)を `eval_rule` したときに、未定義の値が返るか、エラーになるか。部分集合ルールで要素が無いときに空の集合が返るか | エラーの種類で「未定義」を見分ける。空の集合が返らなければ、`fail` の確認の方法を変える(`get_packages` とパッケージのルールの一覧など) |
| 解析のエラーの出方 | ライブラリの解析のエラー(未定義の関数の参照など)が `eval_query("true")` で出るか | ライブラリだけを読み込んだエンジンで、パッケージを `eval_rule` して確かめる |
| `rvm` を外した構成 | `default-features = false` と上の機能の組み合わせで、`Engine` の `add_policy` / `eval_rule` / `eval_query` が使えるか、ビルドが通るか | 必要な機能を足す(足すものと理由を PR に書く) |
| 時間の上限の効き方 | 内包表記・`every`・再帰的でない大きなループ・`walk` を含むルールが、上限で止まるか(テストで確かめる) | 止まらない書き方を、ポリシーのリポジトリのガイドに書く |

P1 の実装で確かめた結果です(regorus 0.12.0)。

- 時間切れのエラーの見分け: `downcast_ref::<regorus::LimitError>()` で取り出せる。代わりの方法は使わない
- `clone()` とタイマー: タイマーの設定と `print` を集める設定は複製される
- 未定義のルールの評価: 存在しないルールのパスを `eval_rule` すると、未定義の値ではなくエラー(`not a valid rule path`)になる。このエラーを「未定義」として扱う(ほかの用途に使われていないことを regorus のソースで確かめた)。入力に依存して定義されないルールは未定義の値になる。部分集合ルールで要素が無いときは空の集合が返る
- 解析のエラーの出方: `eval_query("true")` では出ない。上の「ライブラリ」の手順 4 のとおり `eval_query("data")` で見つける
- `rvm` を外した構成: `add_policy` / `eval_rule` / `eval_query` が使え、ビルドも通る。機能は足していない
- 時間の上限の効き方: 内包表記、`every`、三重の `some ... in`、`walk` を含むルールが、いずれも上限で止まる

リソースごとの評価(ADR-0030)の実装の最初に確かめることです。結果が違えば設計書を直してから進めます。

| 項目 | 確かめること | 違った場合 |
| --- | --- | --- |
| `data.yata` とルールのパッケージ | `add_data` で `{"yata": {"context": ...}}` を入れたエンジンで、パッケージ `yata.rules.<ID>` のルールと `yata.lib.<name>` のライブラリが評価でき、ルールから `data.yata.context.regions` を参照できるか。`eval_rule("data.yata.rules.<ID>.fail")` が `data` の `yata` の下の基本の文書と衝突しないか | `data.yata.context` の代わりに、プロバイダ名と重ならない最上位の名前(例: `data.yata_context`)にする。全体設計と ADR-0030 を直す |
| `set_input` の後の再評価 | 準備済みのエンジンで `set_input` だけを替えて `eval_rule` を繰り返したとき、前のリソースの規則の値が残らないか(`clean_internal_evaluation_state` で消えるはずのもの)。`with_document` の `input` も替わるか | 評価ごとにエンジンを `clone()` する(準備済みのエンジンの `clone()` は再準備を起こさない) |
| `default` と `eval_rule` | `default fail := false` だけが成り立つとき、`eval_rule` が `false` を返すか。ルールに `message` が無いときの `eval_rule` が `not a valid rule path` のエラーになるか | エラーの種類で「定義が無い」を見分ける方法を変える |
| conflict のエラー | `message` の 2 つの本体が別の値で成り立つとき、`eval_rule` がエラー(`rules should not produce multiple outputs` など)を返し、時間切れと見分けられるか | エラーのメッセージで見分ける |
| タイマーの設定し直し | `eval_rule` の前に `set_execution_timer_config` で `limit` を変えたとき、その値で打ち切られ、再準備が起きないか | 残りの時間で打ち切れない場合は、リソースごとの評価の前後で経過時間を測り、合計が上限を超えた時点で `Timeout` にする(1 回の `eval_rule` の上限は `--policy-timeout` のまま) |
| 性能 | 1 万件の `target_type` のリソースと単純なルールで、1 回の評価の固定の費用と、ルール × アカウントの評価の時間を測る。すべてのリソースが合格の場合と、すべてが不合格の場合(`message` / `reason` の評価を含む)の両方を測る(上の「リソースごとの評価の性能」の見積もりと比べる) | 見積もりより大きく遅ければ、PR に測定の結果を書き、全体設計の未決事項「`data` だけに依存する計算の共有」と評価の並列化の検討を早める |

## 全体設計の変更

この PR で、次のとおり `docs/architecture/` を更新します。

| ページ | 変更 |
| --- | --- |
| [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) | 一覧に `--format <text\|json>`(`rules test`、既定 `text`)の行を足し、`--format <table\|json\|sarif>` の行の対象が `scan` だけであることはそのまま。`--policy-timeout` の既定値を 30 秒(1〜3600 の整数)にし、「既定値が未決の項目」と未決事項から `--policy-timeout` を外す。「基本」の「1 サブコマンド 1 ファイル」を「1 サブコマンド 1 モジュール(必要ならサブモジュールに分ける)」に直す |
| [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) | 「(b) 資源の上限」に値と、評価時間だけをオプションにすることを書く。「(d) パスの扱い」に reparse point の判定方法を書く。「(e)」と「評価」に、regorus の機能と時間の上限の実現方法を書く(この設計書へのリンク)。「ルールのテスト」の「フィクスチャのそろい方」に、宣言していない種別の配列が無いことを加え、「ダミー値」にこの設計書の一覧へのリンクを書く。未決事項(安全性)の「資源の上限の値」「評価時のメモリの上限」「junction の判定方法」と、未決事項の「regorus の有効にする機能」「評価時間の上限の既定値」「評価時間の上限の実現方法」「ダミーとして認める値の一覧」を、決めた内容とこの設計書へのリンクに置き換えて外す。未決事項に「`time.now_ns` とフィクスチャ」を足す |
| [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) | 未決事項「ダミーとして認める値の一覧」を外し、原則にこの設計書の一覧へのリンクを書く |
| [初期依存クレート](../architecture/Architecture-Initial-Dependencies.ja.md) | `regorus` の行に、`default-features = false` と有効にする機能を書く |
| [ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md) | `yata-engine` の主な依存に `serde`、`thiserror` を加える。`commands/` の「1 サブコマンド 1 ファイル」を「1 サブコマンド 1 モジュール(必要ならサブモジュールに分ける)」に直す |
| [ADR: ポリシーエンジン](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md) | 本文は変えず、末尾に「追記」を置き、評価時間の上限の確認の結果と cargo feature をこの設計書で決めたことを書く(本文が参照していたポリシー評価の未決事項が無くなるため) |
| [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) | 「主なエラー」の `PolicyLoadError` の行を、ディレクトリ全体の失敗だけを表す形に直し、ルール単位の失敗は `LoadedPolicies.rule_failures` で返す(`yata-cli` が `rules test` 以外で 2、`rules test` で 1 にする)行を足す |
| [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md)(失敗の表) | 「Rego の構文エラー」の行に、読み込み時のメタデータ・`fail` の確認での時間切れを含める。評価時間の上限の注記を、ルール × アカウントの評価での時間切れに限る |
| [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) | 「読み込みと検証」の「上限の値は、ポリシーの資源の上限と同じく未決事項」を、ポリシーの上限は決まり、スナップショットの上限は別に決める、と直す |

## 出典

- regorus 0.12.0 の cargo feature: <https://docs.rs/crate/regorus/0.12.0/features>
- regorus の `Engine`(`eval_rule`、`set_execution_timer_config`、`set_gather_prints`、`take_prints`、`set_strict_builtin_errors`、`set_policy_length_config`): <https://docs.rs/regorus/0.12.0/regorus/struct.Engine.html>
- regorus の制限(`ExecutionTimerConfig`、`PolicyLengthConfig`、`LimitError`、メモリの上限の関数): <https://docs.rs/regorus/0.12.0/regorus/utils/limits/index.html>、<https://docs.rs/regorus/0.12.0/regorus/utils/limits/struct.ExecutionTimerConfig.html>、<https://docs.rs/regorus/0.12.0/regorus/utils/limits/struct.PolicyLengthConfig.html>、<https://docs.rs/regorus/0.12.0/regorus/utils/limits/enum.LimitError.html>
- regorus 0.12.0 のソース(`set_input` と `add_data` の再準備の有無、`eval_rule` ごとの状態の初期化 `clean_internal_evaluation_state`、`set_execution_timer_config`、完全ルールの conflict のエラー): <https://docs.rs/crate/regorus/0.12.0/source/src/engine.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/interpreter.rs>
- Rego の言語仕様(`default`、完全ルールの conflict、`else`): <https://www.openpolicyagent.org/docs/policy-language>
- regorus のソース(タイマーの適用と確認の場所、メモリの上限、組み込み関数の登録): <https://docs.rs/crate/regorus/0.12.0/source/src/engine.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/interpreter.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/utils/limits/time.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/utils/limits/memory.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/lib.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/builtins/mod.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/builtins/http.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/builtins/opa.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/builtins/net.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/builtins/time.rs>
- OPA の CLI(`opa eval` / `opa test` の `--timeout`、`--ignore`): <https://www.openpolicyagent.org/docs/cli>
- OPA の設定(バンドルの `size_limit_bytes`): <https://www.openpolicyagent.org/docs/configuration>
- Conftest のオプション(`--ignore`): <https://www.conftest.dev/options/>
- Checkov の CLI と環境変数(`CHECKOV_MAX_IAC_FILE_SIZE`、`CHECKOV_MAX_FILE_SIZE`、`--skip-path`): <https://www.checkov.io/2.Basics/CLI%20Command%20Reference.html>
- Trivy の `trivy config`(`--timeout`、`--skip-dirs`、`--skip-files`): <https://trivy.dev/latest/docs/references/configuration/cli/trivy_config/>
- Rust の `std::fs::FileType`、`std::fs::symlink_metadata`: <https://doc.rust-lang.org/std/fs/struct.FileType.html>、<https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html>
- Rust の標準ライブラリの Windows の実装(`FileType::new` の name surrogate の判定、`lstat`): <https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/windows.rs>
- reparse tag(name surrogate のビット、`IO_REPARSE_TAG_MOUNT_POINT`、`IO_REPARSE_TAG_SYMLINK`): <https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-tags>
- AWS のドキュメントで例に使われているアカウント ID の調査: <https://dev.classmethod.jp/articles/aws-account-example-id-by-documents/>
- JSON Pointer(RFC 6901): <https://www.rfc-editor.org/rfc/rfc6901>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| `time.now_ns` とフィクスチャ | 認証情報の最終使用からの日数など、現在の時刻を使うルールは、フィクスチャの結果が実行する日によって変わる。選択肢: (1) フィクスチャの日時を、判定の境界から十分離れた過去・未来の値にする(形式の変更なし)、(2) フィクスチャに現在の時刻を指定するキー(例: `now`)を足し、regorus の拡張関数で `time.now_ns` の代わりの関数を用意する(フィクスチャの形式とルールの書き方の変更)、(3) エンジンが `data.yata.context` に評価の時刻を入れる(`data` の形の変更。ADR-0030 の前は `input` に入れる案だった)。推奨は (1) で始め、境界のテストが必要になったら (3) を検討する。regorus が `with time.now_ns as ...` に対応しているかも確かめる | 時刻を使うルール(IAM の 2.11〜2.13 など)を作るポリシーのリポジトリの Issue の前 |
| ルールのテストのカバレッジ | regorus の `coverage` の機能で、フィクスチャが通らなかったルールの行を報告するか | 需要が出た時点で別 Issue |
| 出力への注入のエスケープの共通化 | `yata rules test` の text の形式のエスケープ(`yata-cli/src/sanitize.rs`)を、`scan` の table の形式とログと共通にするか、置き場所をどこにするか | レポートの出力を実装する Issue |
| 資源の上限のオプション | ファイルの大きさ・ファイル数・ディレクトリ数・深さを、オプション・環境変数で変えられるようにするか。この設計では変えられない | 上限に当たる実際のポリシーのリポジトリが現れた時点 |
| ルールの評価の並列化 | ルール × アカウントの評価を複数のスレッドで行うか(`arc` の機能でエンジンをスレッド間で渡せる) | `scan` の性能が問題になった時点 |
