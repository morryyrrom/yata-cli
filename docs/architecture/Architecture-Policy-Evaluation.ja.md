# ポリシー評価

> This Japanese version is the authoritative source. The English version (`Architecture-Policy-Evaluation.md`) has not been written yet.

ルールの書式、実行時の読み込み、評価、結果の作り方を定めます。ルールは `yata` のバイナリにもこのリポジトリにも含めず、別のリポジトリで配布されたポリシーのディレクトリから実行時に読み込みます。エンジンの選定理由は [ADR: ポリシーエンジン](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md)、ルールとフィクスチャの配置・自動検出は [ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) を参照してください。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## ルールの構成

ポリシーのディレクトリの中の階層は自由です。1 ルールは 1 つの `.rego` ファイル(`<ルール名>.rego`)で、フィクスチャはその隣の `<ルール名>.fixtures/` に置きます。ルートにはマニフェスト `yata-policies.json` を置きます。

```
<policy-dir>/
├── yata-policies.json                       # マニフェスト(name: "example_policies")
├── lib/                                     # 共通の関数のライブラリ(ルールではない)
│   └── helpers.rego
├── s3/
│   ├── bucket_public_access_block.rego      # ルール
│   └── bucket_public_access_block.fixtures/
│       ├── pass_all_blocked.json
│       └── fail_block_public_acls_disabled.json
└── iam/
    └── password_policy_min_length.rego
```

### 名前の規則

- ディレクトリ名、`.rego` ファイルの拡張子を除いた名前(ルール名)、マニフェストの `name` は、**名前の文字の規則**に従う
  - 文字は `[a-z][a-z0-9_]*`(英小文字で始まり、英小文字・数字・`_` が続く)
  - Rego の予約語は使えない: `as`、`contains`、`default`、`else`、`every`、`false`、`if`、`import`、`in`、`not`、`null`、`package`、`some`、`true`、`with`
  - 理由: パッケージ名(`yata.rules.<ID>`)と、`data.aws.<service>.<resource>` のような参照を、ドット区切りの形で必ず書けるようにするため(数字で始まる名前や予約語は、ドット区切りでは書けない)
  - リソース種別の各部分にも同じ規則を当てはめる([リソースモデル](Architecture-Resource-Model.ja.md))
  - 例外: フィクスチャのディレクトリ `<ルール名>.fixtures` と、`.` で始まるディレクトリ(`.git` など)
  - ルートの `lib/` は、ルールではなくライブラリの置き場所(下の「ライブラリ」)
- ルールのファイルは、`lib/` の下を除き、ルートの直下も含めて、任意の場所に置ける

### ルール ID

- 形式: `<マニフェストの name>.<ディレクトリ名>.….<ルール名>`。区切りは `.`
  - 例: `example_policies.s3.bucket_public_access_block`、ルートの直下なら `example_policies.<ルール名>`
- 各部分は名前の文字の規則に従い `.` を含まないので、`.` で区切っても曖昧にならない。区切りに `.` を使うのは、Rego のパッケージ名(`.` で区切る)と対応させるため
- ポリシーのセットのバージョンは ID に含めない。バージョンを含めると、ポリシーのリリースのたびに全ルールの ID が変わり、`--rule` / `--exclude-rule` の指定、将来の抑制の機能、SARIF の `ruleId` による GitHub code scanning のアラートの追跡、結果の比較が壊れるため。バージョンは `ScanReport.policies` に出す
- ディレクトリを ID に含めるため、ファイルを移動・改名すると ID が変わる。ID の変更は利用者の設定や過去の結果との比較に影響するので、ポリシーのリポジトリでは移動・改名を避ける。移動する場合は、互換性の無い変更として扱う(リリースノートなどで告知する)
- ID はメタデータに書かない。パスとマニフェストだけから決まる
- マニフェストの `name` はポリシーのディレクトリの間で一意でなければならない(重複すると ID が重複しうるため、ディレクトリ全体の失敗として終了コード 2)。1 つのポリシーのディレクトリの中では、パスが一意なので ID は重複しない

### パッケージ名

- ルールのパッケージ名は `yata.rules.<ルール ID>` とする(例: `package yata.rules.example_policies.s3.bucket_public_access_block`)
- パッケージ名がパスから決まる値と一致しなければ、読み込みの失敗にする
- ルールは、ルールごとに別の regorus のエンジンで評価する(下の「評価」)。それでもパッケージ名を ID から決めるのは、ライブラリのパッケージと衝突させないため、ルールを読む人が ID を一目で分かるようにするため

### ライブラリ

- ポリシーのディレクトリのルートの `lib/` の下(階層は自由)の `.rego` は、ルールではなく、共通の関数のライブラリとして扱う
- ライブラリのパッケージ名は `yata.lib.<マニフェストの name>` で始める(例: `package yata.lib.example_policies.helpers`)。違えばディレクトリ全体の失敗
- ライブラリは、同じポリシーのディレクトリのルールを評価するエンジンにだけ読み込む。ほかのポリシーのディレクトリのライブラリは使えない
- ライブラリには `metadata` と `fail` を置かない(置いてあれば読み込みの失敗)
- ライブラリの誤りは、同じポリシーのディレクトリの全ルールに影響するため、ディレクトリ全体の失敗(終了コード 2)になる

ポリシーのリポジトリでの `lib/` の使い方のガイドです。

- 1 ファイル 1 パッケージにし、話題ごとにファイルを分ける(例: `lib/iam_policy.rego` に IAM のポリシー文書の解析、`lib/network.rego` に CIDR の判定)
- 既存の関数の意味(引数・戻り値・判定の内容)を変えない。直すときは新しい関数を足し、ルールを順に移す。ライブラリはすべてのルールから使われうるため、意味を変えると多くのルールの判定が黙って変わる
- `lib/` の変更は、ルールの追加・変更とは別の単独の PR にする(並列に追加されるルールの PR と衝突させず、影響の範囲をレビューしやすくするため)

### フィクスチャの置き場所

- ルール `<dir>/<ルール名>.rego` のフィクスチャは `<dir>/<ルール名>.fixtures/` に置く
- 対応するルールの無い `.fixtures` ディレクトリは、`rules test` でルール単位の失敗として報告する(ほかのコマンドではフィクスチャを読まないので無視する)
- フィクスチャの書式は [ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) を参照

## ルールの書式

Rego v1 の構文で書きます。1 ファイルに、メタデータ `metadata`、そのリソースが不合格かを表す `fail`、任意の `message` と `reason` を置きます。ルールは `target_type` のリソースごとに評価され、評価の対象のリソースが `input` に入ります(下の「評価」)。この書式にした理由は [ADR: ルールをリソースごとに評価する](../adr/ADR-0030-Per-Resource-Rule-Evaluation.ja.md) を参照してください。

```rego
package yata.rules.example_policies.s3.bucket_public_access_block

metadata := {
	"title": "S3 buckets should block public ACLs",
	"severity": "high",
	"description": "S3 Block Public Access should block public ACLs on each bucket.",
	"remediation": "Enable BlockPublicAcls in the bucket's Block Public Access settings.",
	"target_type": "aws.s3.bucket",
	"references": [
		{"standard": "cis-aws-foundations", "version": "x.y.z", "control": "x.y.z"},
	],
}

block_public_acls := object.get(input.configuration, ["GetPublicAccessBlock", "PublicAccessBlockConfiguration", "BlockPublicAcls"], null)

default fail := false

fail if block_public_acls != true

message := sprintf("Bucket %s does not block public ACLs", [input.id])

reason := {
	"path": "/configuration/GetPublicAccessBlock/PublicAccessBlockConfiguration/BlockPublicAcls",
	"actual": block_public_acls,
	"expected": true,
}
```

上の例は書式を示すためのもので、4 つの設定のうち `BlockPublicAcls` だけを判定しています。設定が無い場合(`GetPublicAccessBlock` が `null`)も `object.get` で `null` として取り出し、不合格にしています。`block_public_acls` は、エンジンが読まない補助のルールです。CIS のバージョンとコントロール番号はダミーです。

ほかのリソースを参照するルールは、`data` から読みます(例: CloudTrail の証跡を `target_type` にし、保存先のバケットを `data.aws.s3.bucket` から探す)。`input` と `data` の中身は [リソースモデル](Architecture-Resource-Model.ja.md) の「ポリシーエンジンへの渡し方」で定めます。

### メタデータ

| キー | 必須 | 内容 |
| --- | --- | --- |
| `title` | 必須 | 1 行の題名(英語) |
| `severity` | 必須 | `critical` / `high` / `medium` / `low` / `info` のいずれか |
| `description` | 必須 | 何を確かめるか、なぜ問題か(英語) |
| `remediation` | 必須 | 修正手順(英語) |
| `target_type` | 必須 | 評価の対象にするリソース種別(文字列 1 つ)。ルールはこの種別のリソースごとに 1 回評価され、そのリソースが `input` に入る。結果(`pass` / `fail`)はこの種別のリソースごとに 1 件作る。この種別の全リソースは `data` にも入る |
| `related_types` | 任意 | 判定のために参照するだけのリソース種別の配列(例: CloudTrail の証跡を判定するために参照する S3 のバケット)。`data` に入る。`target_type` を含めない。当面は `target_type` と同じプロバイダの種別だけを書ける(ほかのプロバイダの種別があればメタデータの誤り) |
| `references` | 必須 | 対応する基準の配列(空でもよい)。各要素は `standard`、`version`、`control` を持つ。URL を足す場合は `url`(下の「基準の参照」) |
| `tags` | 任意 | 文字列の配列。絞り込みに使う |

- メタデータは文字列・配列・オブジェクトの定数だけで書き、入力に依存させません(入力なしで評価して取り出すため)
- 表に無いキーは、エラーにせず無視します(下の「互換性の規則」)
- ルールのプロバイダは `target_type` の最初の部分(`aws` など)で決まります。パスからは決めません

### 基準の参照

- 最初のルール群が対象とする基準は **CIS Amazon Web Services Foundations Benchmark v7.1.0** とする。対象のコントロールと選んだ理由は [最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) を参照
- CIS のコントロールに対応するルールは、`references` に `{"standard": "cis-aws-foundations", "version": "7.1.0", "control": "<番号>"}` を書く
- コントロールの番号・題名は、v7.1.0 の本文で確かめてからルールを公開する([最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の未決事項「CIS v7.1.0 の本文の確認」)

### ルールの規約

エンジンがルールのパッケージから読む規則は、`metadata`、`fail`、`message`、`reason` の 4 つです。それ以外の名前の規則(補助のルール、関数)は自由に置けます。

| 規則 | 必須 | 形 | 内容 |
| --- | --- | --- | --- |
| `fail` | 必須 | 真偽値の完全ルール。`default fail := false` を必ず書く | 評価の対象のリソース(`input`)が不合格なら `true` |
| `message` | 任意 | 空でない文字列の完全ルール | 人が読む説明(英語)。どこが、なぜ不合格かを書く。省略した(undefined の)場合は、メタデータの `title` を使う |
| `reason` | 任意 | オブジェクトの完全ルール | 不合格の箇所と値。キーは下の表 |

`reason` のキーです。

| キー | 必須 | 内容 |
| --- | --- | --- |
| `path` | 任意 | 不合格の箇所。リソースのオブジェクト(`input`)を起点とする JSON Pointer(RFC 6901)の文字列(例: `/configuration/GetPublicAccessBlock/PublicAccessBlockConfiguration/BlockPublicAcls`)。特定の項目に結びつく不合格では書く |
| `actual` | 任意 | その箇所の実際の値(任意の JSON の値。`null` を含む)。`path` を書いたときは書く |
| `expected` | 任意 | 期待する値、または期待の説明(任意の JSON の値。`null` を含む) |

- 結果は 1 つのリソースにつき 1 件(`pass` か `fail`)。1 つのリソースに不合格の箇所が複数あっても、結果は 1 件にまとめる(下の「書き方の注意」)
- `message` と `reason` は、`fail` が `true` のリソースでだけ評価して使う。`fail` が `false` のリソースでは評価しない(そのため、`pass` のリソースでの `message` / `reason` の誤りは見つからない)
- `message` には構成情報の値を埋め込まず、値は `reason` の `actual` に入れる(リソースの ID や名前は埋め込んでよい)。`--omit-values` で値を省けるようにするため([出力形式](Architecture-Output-Formats.ja.md) の「値の扱い」)
- 構成情報は伏せずに入っているため([リソースモデル](Architecture-Resource-Model.ja.md) の「秘密の値の扱い」)、判定に要らない機微な値(SNS の購読の `Endpoint` など)を `actual` や `message` に入れない
- 「全リージョンで有効でない」のように、特定の項目に結びつかない不合格では、`reason` または `path` を省いてよい
- `reason` の知らないキーは無視する。`path` がリソースの中に実在するかは確かめない

#### 評価のエラー

次の誤りは、そのルールの評価のエラー(状態 `error`)にします。

| 誤り | エラーの説明の例 |
| --- | --- |
| `fail` が undefined(`default fail := false` が無く、本体が成り立たなかった) | `fail is undefined; add "default fail := false"` |
| `fail` が真偽値でない | `fail must be a boolean, but was a string` |
| `message` が文字列でない、または空の文字列 | `message must be a non-empty string, but was a number` |
| `reason` がオブジェクトでない | `reason must be an object, but was an array` |
| `reason.path` が文字列でない(`null` を含む)、または JSON Pointer の形でない(空の文字列か `/` で始まり、`~` の後が `0` か `1`) | `reason.path must be a JSON Pointer string` |
| 完全ルール(`fail`、`message`、`reason`)が、1 つのリソースで 2 通りの値になる(Rego の conflict。regorus の評価のエラーになる) | regorus のエラーのメッセージ(例: `rules should not produce multiple outputs`) |
| そのほかの regorus の評価のエラー、評価時間の上限を超えた | regorus のエラーのメッセージ、または時間切れ |

- あるリソースでエラーが起きたら、そのルールの評価をそこで止め、状態を `error` にして結果を作らない(下の「状態の判定順」)
- エラーの説明(`RuleOutcome.error`)は英語の 1 行で、上の説明に、エラーが起きたリソースの `id` とリージョンを付ける(例: `fail must be a boolean, but was a string (resource arn:aws:s3:::example-bucket in us-east-1)`)
- `yata-engine` が作る説明(上の表の regorus のメッセージ以外のもの)には値を入れず、型の名前だけを入れる。ルールの返した値には構成情報の値が入りうるため([エラー処理とログ](Architecture-Error-Handling-and-Logging.ja.md) の「ログに書くもの・書かないもの」と同じ扱い)
- regorus 由来のメッセージ(conflict、そのほかの評価のエラー)は、値を含みうる(例: regorus 0.12.0 の関数の conflict のエラー `function produced multiple outputs ...` は、関数の出力の値を埋め込む)。`yata-engine` はこのメッセージを加工せず、エラーの種類(`conflict` / `engine` など)と分けて持つ
  - レポートの `RuleOutcome.error` には、`yata-engine` が作る説明(エラーの種類、リソースの `id` とリージョンを含む)に加えて regorus のメッセージを入れる。`--omit-values` のときは regorus のメッセージだけを省き、`yata-engine` が作る説明は残す(値を含まないため)
  - ログでの扱いは [エラー処理とログ](Architecture-Error-Handling-and-Logging.ja.md) の「ルールの評価のエラーのログ」
- `message` / `reason` が undefined なのは誤りではない(省略と同じ)

### 書き方の注意

- `default fail := false` を必ず書く
  - Rego では、存在しない項目を参照した式は undefined になり、その本体は成り立たない。`default` があれば、本体が成り立たないリソースの `fail` は `false`(`pass`)になる
  - `default` が無いと、本体が成り立たないリソースで `fail` が undefined になり、ルール全体が `error` になる。読み込み時の確認(下の「検証」)では見つからない書き方もあるため、`pass_` のフィクスチャで確かめる
- `fail` の本体で、任意の項目(`name`、`tags`、`null` になりうる構成情報など)を参照すると、その項目が無いリソースは黙って `pass` になる
  - 値が無いこと自体を不合格にしたい場合は、`object.get` や `not` で「無い」場合を明示的に判定する
  - `message` などには必須の項目(`id` など)を使う
  - 項目が無い・`null` のケースのフィクスチャを用意して確かめる
- `fail` の本体は複数書いてよい(どれかが成り立てば `true`)。`fail := false if { ... }` は書かない。`true` になる本体と同時に成り立つと conflict になる
- `message` / `reason` を場合ごとに書き分けるときは、`else` でつなぐ。`if` の付いた定義を並べると、2 つの本体が同時に成り立ったリソースで conflict になり、ルールが `error` になる

```rego
# block_public_acls と block_public_policy は、上の例と同じ形の補助のルール
# 誤り: バケットが ACL とポリシーの両方を許していると、message が 2 通りの値になり conflict になる
message := sprintf("Bucket %s does not block public ACLs", [input.id]) if block_public_acls != true
message := sprintf("Bucket %s does not block public policies", [input.id]) if block_public_policy != true

# 正しい: else でつなぐと、最初に成り立ったものだけが使われる
message := sprintf("Bucket %s does not block public ACLs", [input.id]) if {
	block_public_acls != true
} else := sprintf("Bucket %s does not block public policies", [input.id]) if {
	block_public_policy != true
}
```

- 1 つのリソースに不合格の箇所が複数ありうる場合は、次のどちらかにする
  - 1 つのルールのまま、`message` でまとめて説明する(`else` で優先する箇所を決める、または `concat` で並べる)。`reason` は 1 つの箇所しか示せないため、代表の箇所を示すか、`path` を省く
  - 箇所ごとに結果を分けたい場合は、ルールを分ける
- `input` は評価の対象の 1 つのリソース。同じ種別のほかのリソースと比べる場合は、`data` の `target_type` の配列を使う(`input` のリソースも含まれる)。同じ `id` が複数のリージョンに現れる種別(CloudTrail の証跡など。[リソースモデル](Architecture-Resource-Model.ja.md) の「id の一意性」)では、`id` と `region` の組で区別する
- `data` だけに依存する補助のルール(例: 証跡が使うバケットの名前の集合)も、リソースごとに計算し直される(下の「評価」)。`target_type` のリソースが多く、`data` の大きな配列を走査するルールは、計算量が「対象のリソースの数 × 走査するリソースの数」になる。時間の上限はルール × アカウントの合計に掛かるため、対象のリソースの数が少ない種別を `target_type` にできるなら、そうする
- 構成情報の操作のキーの値が、取得の失敗の印(`{"yata_error": {...}}`。[リソースモデル](Architecture-Resource-Model.ja.md) の「操作ごとの取得の失敗」)になっていることがある
  - この場合、その下の項目(例: `input.configuration.GetBucketPolicy.Policy`)は undefined になり、その項目を参照する本体は成り立たない。`object.get(..., null)` で取り出すと `null` になり、「設定が無い」場合と同じに扱われる
  - `yata-engine` は、失敗の印を持つリソースがあればルールの状態を `incomplete` にし、そのリソースを `RuleOutcome.incomplete_resources` に記録する(下の「状態の判定順」)。黙って `pass` だけが出ることは無い
  - 失敗の印を見分けたいルールは、`object.get(input.configuration, ["<操作名>", "yata_error"], null) != null` のように判定する。共通の関数として `lib/` に置くとよい
  - 失敗の印を含むフィクスチャ(状態が `incomplete` になるケース)を用意するとよい

## 読み込み

図: [ポリシーの読み込みの流れ](../diagrams/Diagram-0001-Policy-Loading.md)

### 責務の分担

| 処理 | 担当 |
| --- | --- |
| ポリシーのディレクトリの決定(オプション・環境変数)。指定が無ければ終了コード 2 | `yata-cli` |
| ディレクトリの存在・読み取りの確認(見つからない・読めなければ `PolicyLoadError`)、走査、ファイルの読み込み、資源の上限の確認、マニフェストの検証、ライブラリとルールの読み込み、メタデータの検証 | `yata-engine`(`loader` モジュール) |
| ルールの絞り込み(`--rule`、`--exclude-rule`、`--min-severity`) | `yata-engine` が絞り込みの関数を提供し、`yata-cli` がオプションを渡して呼ぶ |
| ルールの `target_type` / `related_types` とプロバイダの対応の検査 | `yata-cli`(`yata-engine` は AWS を知らないため) |

ファイルシステムからの読み込みは `yata-engine` に置きます。ポリシーの形式(ディレクトリの構成、マニフェスト、ルール、ライブラリ、フィクスチャ)を知っているのは `yata-engine` だけにし、`yata rules test` とテストからも同じ処理を使うためです。`yata-engine` は受け取ったパスだけを読み、環境変数や既定のパスは見ません。

### 処理の順序

1. `yata-cli` がポリシーのディレクトリを決める(指定が無ければ終了コード 2)
2. `yata-engine` が、ディレクトリごとに存在と読み取りを確かめ、マニフェストを検証する。マニフェストの `name` がディレクトリの間で重複していないかも確かめる
3. `yata-engine` が、すべてのディレクトリを走査し、ライブラリとすべてのルールを読み込んで検証する(絞り込みの前に、対象外のルールも含めて検証する)
4. `yata-cli` が、`yata-engine` の絞り込みの関数でルールを絞り込む。絞り込んだ結果が 0 件なら終了コード 2
5. `yata-cli` が、絞り込んだ後のルールだけを対象に、プロバイダの対応を検査する。対象外にしたルールは、対応していない種別を宣言していても検査しない(利用者が対象外にしたルールのためにスキャンを止めないため)

`yata rules test` は絞り込みを行わず、すべてのルールを対象にします。

### ポリシーのディレクトリの決定

`--policy-dir <DIR>`(複数指定可)→ 環境変数 `YATA_POLICY_DIR`(OS のパスの区切り文字で複数指定可)→ 既定の場所、の順に決めます。オプションと環境変数の両方があれば、オプションだけを使います(混ぜない)。既定の場所は未決事項で、決まるまでは「既定なし」として扱い、どちらも無ければ終了コード 2 にします。優先度の規則は [CLI インターフェース](Architecture-CLI-Interface.ja.md) の「オプションの優先度」で定めます。

`yata` はネットワークからポリシーを取得しません。ポリシーのリポジトリの取得・更新(git clone、ダウンロードなど)は利用者が行います。

### マニフェスト

ポリシーのディレクトリのルートに `yata-policies.json` を必ず置きます。ルールの一覧は書きません(中央の一覧ではない)。ルールは走査で見つけます。

```json
{
  "format_version": 1,
  "name": "example_policies",
  "version": "0.1.0",
  "min_yata_version": "0.1.0"
}
```

| キー | 必須 | 内容 |
| --- | --- | --- |
| `format_version` | 必須 | ポリシーの形式のバージョン(整数)。ディレクトリの構成、ルールの書式(`metadata` / `fail` / `message` / `reason`)、フィクスチャの形式、`input` / `data` の形のいずれかに互換性の無い変更をしたときに上げる |
| `name` | 必須 | ポリシーのセットの名前。名前の文字の規則(`[a-z][a-z0-9_]*`、予約語を除く)に従う。ルール ID とパッケージ名の先頭になる。ログとレポートに出す |
| `version` | 必須 | ポリシーのセットのバージョン(`x.y.z`)。ルール ID には含めない。ログとレポートに出す |
| `min_yata_version` | 必須 | このポリシーのセットに必要な `yata` の最低バージョン(`x.y.z`)。新しいリソース種別を使うルールを足したときなどに上げる |

ポリシーのディレクトリを複数指定した場合は、ディレクトリごとにマニフェストが必要で、ディレクトリごとに 1 つずつ検証します。1 つでも合わなければ終了コード 2 です。読み込んだマニフェストの `name` / `version` / `format_version` は、ディレクトリごとに 1 要素として `ScanReport.policies` に入れます([出力形式](Architecture-Output-Formats.ja.md))。

`yata-engine` は読み込みの最初に、各マニフェストについて次を確かめ、合わなければ終了コード 2 にします。

- マニフェストがある。JSON として正しく、必須のキーがそろっている
- `name` が名前の文字の規則(`[a-z][a-z0-9_]*`、予約語を除く)に従い、ほかのポリシーのディレクトリの `name` と重複しない
- `format_version` が、その `yata` が対応する形式のバージョンである(対応するバージョンは `yata-engine` の定数で持つ。当面は `1` だけ)
- `min_yata_version` が `x.y.z`(0 以上の整数 3 つを `.` でつないだもの)の形である。違えば形式の誤り
- `min_yata_version` が、実行中の `yata` のバージョン以下である。`x.y.z` の各数値を順に比べる。実行中の `yata` のバージョンにプレリリースの部分(`-alpha.1` など)やビルドメタデータがあれば、それを無視して `x.y.z` だけで比べる。比較のためのクレートは追加しない

上限(この `yata` では読めない新しい形式)は `format_version` で表すため、マニフェストに `yata` のバージョンの上限は書きません。

### 互換性の規則

ポリシーのリポジトリと `yata` は別々にリリースされるため、次の規則で互換性を保ちます。

**知らないキー(前方互換)**

- マニフェスト、ルールのメタデータ、ルールの `reason` に、その `yata` が知らないキーがあっても、エラーにせず無視する
- エンジンがルールのパッケージから読む規則の名前(`metadata`、`fail`、`message`、`reason`)を足すときは、`format_version` を上げる。既存のルールが同じ名前の補助のルールを持っていると、その意味が変わるため
- 判定に関わらないキー(表示用の項目など、古い `yata` が無視しても判定が変わらないもの)を足すときは、`min_yata_version` を上げなくてよい。そのキーの効果を必須にしたい場合(古い `yata` で使われるのを防ぎたい場合)だけ、そのキーを解釈する `yata` のバージョンに `min_yata_version` を上げる
- 正しく評価するために必ず解釈しなければならないキー(無視すると判定が変わるもの)を足すときは、`format_version` を上げる

**構成情報(`configuration`)の互換性**

- 外部のルールは、リソースの構成情報を項目名で参照する。項目の名前が変わったり消えたりすると、ルールの参照は undefined になり、黙って `pass` になる(上の「書き方の注意」で述べた性質)
- このため、公開した(リリースに含めた)リソース種別の `configuration` は、項目の追加だけを許す。名前の変更、削除、型や意味の変更は、互換性の無い変更とする
- 互換性の無い変更は、**新しいリソース種別名を作る**ことを原則とする。`format_version` を上げるのは、`input` / `data` 全体の形やリソースの共通の項目を変えるときだけとする
- 詳細は [リソースモデル](Architecture-Resource-Model.ja.md) の「構成情報の互換性」を参照

### 走査

- 各ポリシーのディレクトリの下を、深さの上限(資源の上限の 1 つ)まで再帰的に走査し、`.rego` ファイルを見つける
- 階層は自由なので、「浅すぎる・深すぎる」の検査は行わない。深さの上限より深いディレクトリが中身を持っていた場合は、そこに置かれたルールが黙って使われなくなるのを防ぐため、ディレクトリ全体の失敗(どのコマンドでも終了コード 2)にする
- `.` で始まるディレクトリは走査しない
- `lib/` の下の `.rego` はライブラリ、それ以外の `.rego` はルールとして扱う
- `.rego` 以外のファイル(README など)は無視する。フィクスチャは `rules test` のときだけ読む
- 名前の文字の規則(`[a-z][a-z0-9_]*`、予約語を除く)に合わないディレクトリの中の `.rego`、または規則に合わない名前の `.rego` は、ルール単位の失敗にする(フィクスチャのディレクトリと `.` で始まるディレクトリを除く)
- シンボリックリンクなどの扱いは、下の「外部のポリシーの安全性」の (d) に従う
- 見つけたルールは ID の順に並べ、毎回同じ順で扱う

### 検証

読み込み時に次を検証します。

- マニフェスト(上の「マニフェスト」)
- ライブラリ: Rego として構文が正しく、パッケージ名が `yata.lib.<name>` で始まり、`metadata` / `fail` を持たない
- ルール: Rego として構文が正しい(ライブラリと一緒に読み込む)、パッケージ名がパスから決まる値と一致する、`metadata` が入力なしで評価でき必須のキーと値の形がそろっている、`target_type` / `related_types` がリソース種別の形式(`<provider>.<service>.<resource>`。各部分は名前の文字の規則に従う)で、`related_types` が `target_type` と同じプロバイダである、`fail` を入力なし(`input` も `data` も無い状態)で評価して真偽値になる
  - `fail` の確認は、`default fail := false` の書き忘れと、これまでの部分集合ルールの書式(`fail contains ...`。入力なしでは空の集合になる)を見つけるためのもの。入力なしでは、`input` を参照する本体は成り立たず、`default` があれば `false` になる。`default` が無くても入力なしで本体が成り立つ書き方(`not input.x` など)は見つけられないため、実行時の undefined も評価のエラーにする(上の「評価のエラー」)
  - `message` と `reason` は入力に依存するため、読み込み時には確かめない
- ルールが 1 つ以上ある(ディレクトリごと)

### 失敗の種類と終了コード

失敗は「ディレクトリ全体に関わるもの」と「ルール単位のもの」に分けます。`yata rules test` 以外のコマンドでは、どちらも終了コード 2(最初に見つかったところで止める必要はなく、見つかった誤りをまとめて標準エラー出力に出してよい)です。`yata rules test` では、ルール単位の失敗は全ルールを検査してから集めて報告し、終了コード 1 にします。ディレクトリ全体に関わる失敗は、`yata rules test` でも 2 です。

| 失敗 | 単位 | `rules test` | それ以外のコマンド(`scan`、`collect`、`rules list`、`permissions`) |
| --- | --- | --- | --- |
| ポリシーのディレクトリの指定が無い(`yata-cli`) | ディレクトリ全体 | 2 | 2 |
| ポリシーのディレクトリが見つからない・読めない(`yata-engine`) | ディレクトリ全体 | 2 | 2 |
| マニフェストが無い(シンボリックリンク・reparse point の場合を含む)・形式の誤り(`name` が名前の文字の規則に合わない(予約語を含む)、`min_yata_version` が `x.y.z` でないを含む)・`format_version` が対応外・`min_yata_version` が新しすぎる | ディレクトリ全体 | 2 | 2 |
| マニフェストの `name` がディレクトリの間で重複する(ID の重複) | ディレクトリ全体 | 2 | 2 |
| ライブラリの誤り(構文、パッケージ名、`metadata` / `fail` を持つ、大きさの上限、シンボリックリンク) | ディレクトリ全体 | 2 | 2 |
| ルールのファイル・フィクスチャのディレクトリ以外にあるシンボリックリンク・reparse point(ディレクトリ、ルート直下のファイルなど) | ディレクトリ全体 | 2 | 2 |
| マニフェストのファイルが大きさの上限を超える | ディレクトリ全体 | 2 | 2 |
| 走査するファイル数・ディレクトリ数が上限を超える | ディレクトリ全体 | 2 | 2 |
| 深さの上限より深いディレクトリに中身がある | ディレクトリ全体 | 2 | 2 |
| ルールが 0 件 | ディレクトリ全体 | 2 | 2 |
| 絞り込みの結果が 0 件 | 絞り込み | 対象外(絞り込みをしない) | 2 |
| ルールの名前・その上のディレクトリ名が名前の文字の規則に合わない(文字、数字で始まる、予約語) | ルール単位 | 1 | 2 |
| ルールのファイル、またはそのフィクスチャのディレクトリの中にシンボリックリンク・reparse point がある | ルール単位 | 1 | 2(フィクスチャは読まないので、ルールのファイルのみ) |
| ルールのファイルまたはフィクスチャが大きさの上限を超える | ルール単位 | 1 | 2(ルールのファイルのみ) |
| Rego の構文エラー(解析のエラーと、読み込み時のメタデータ・`fail` の確認での時間切れを含む) | ルール単位 | 1 | 2 |
| パッケージ名とパスの不一致 | ルール単位 | 1 | 2 |
| メタデータの誤り(必須のキーの不足、値の形の誤り、種別の形式の誤り(各部分が名前の文字の規則に合わない、予約語を含む)、`related_types` にほかのプロバイダの種別がある)、`fail` が無い・入力なしで真偽値にならない | ルール単位 | 1 | 2 |
| プロバイダが対応していない種別 | ルール単位 | 1 | 2(絞り込んだ後のルールだけを検査する) |
| 対応するルールの無い `.fixtures` ディレクトリ | ルール単位 | 1 | 検査しない(フィクスチャを読まない) |
| フィクスチャの不足・形式の誤り | ルール単位 | 1 | 検査しない |
| ダミー以外のアカウント ID | ルール単位 | 1 | 検査しない |
| フィクスチャの期待との不一致、フィクスチャの評価のエラー・時間切れ | ルール単位 | 1 | 検査しない |

- **ID の重複はディレクトリ全体の失敗**とします。1 つのポリシーのディレクトリの中ではパスが一意なので ID は重複せず、重複はマニフェストの `name` が同じディレクトリを複数指定したときだけ起きるためです
- **ルールが 0 件はディレクトリ全体の失敗**とします。指定したディレクトリが誤っている(ポリシーのリポジトリのルートでない、取得に失敗している)可能性が高いためです。複数のディレクトリを指定した場合は、ディレクトリごとに確かめます
- **深すぎる位置はディレクトリ全体の失敗**とします。どのルールに属するかが分からず、黙って無視するとルールが使われなくなるためです
- フィクスチャに関する検査は `rules test` だけで行います。ほかのコマンドはフィクスチャを読みません
- ルール × アカウントの評価で評価時間の上限を超えたもの(`scan` のとき)は読み込みの失敗ではなく、評価の失敗(ルールの状態 `error`、スキャンは不完全で終了コード 4)として扱います。読み込み時のメタデータ・`fail` の確認での時間切れは、上の表の「Rego の構文エラー」の行のとおり読み込みの失敗です

## 外部のポリシーの安全性

ポリシーは外部のリポジトリから取得したファイルで、`yata` の評価の結果を左右します。次の方針で扱います。

### 決定事項

**(a) 信頼の前提**

- ポリシーは評価の結果を左右する。改ざんされたポリシーは、検出を隠したり、誤った結果を出したりできる
- そのため、利用者が信頼できる出所から取得したポリシーを使う前提とする
- `yata` は、ポリシーの署名や出所を検証しない。署名・改ざんの確認は、未決事項の「ポリシーの取得・更新」に含める

**(b) 資源の上限**

次の 4 つに上限を設けます。上限を超えた場合の終了コードは「失敗の種類と終了コード」の表のとおりです。

- 1 ファイルの大きさ(ルール、ライブラリ、フィクスチャ、マニフェスト)。読み込む前にファイルの大きさを確かめる
- 走査するファイル数とディレクトリ数(ポリシーのディレクトリごと)
- 走査の深さ
- 評価時間(`--policy-timeout`。下の「評価」)

値は次のとおりです。決めた根拠と、既存のツール(OPA、Conftest、Checkov、Trivy)との比較は [ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「資源の上限」を参照してください。

| 上限 | 値 | 変えられるか |
| --- | --- | --- |
| マニフェストの大きさ | 64 KiB | 変えられない |
| ルール・ライブラリのファイルの大きさ | 1 MiB | 変えられない |
| フィクスチャのファイルの大きさ | 4 MiB | 変えられない |
| ファイル数(ポリシーのディレクトリごと) | 20,000 | 変えられない |
| ディレクトリ数(ポリシーのディレクトリごと) | 5,000 | 変えられない |
| 深さ(ルートの直下の項目を 1 とする) | 10 | 変えられない |
| 評価時間(ルール × アカウントの評価ごと。リソースごとの評価の合計) | 30 秒 | `--policy-timeout`(1〜3600 秒) |

- 評価時間だけをオプションにする。必要な長さが利用者のアカウントの大きさで変わるため。ほかの上限はポリシーのリポジトリの大きさで決まり、値に十分な余裕をとったため、オプションにしない
- 評価時のメモリの上限は設けない(regorus の `allocator-memory-limits` は、プロセス全体のアロケーターを置き換え、上限もプロセス全体で 1 つになるため)。ファイルの大きさの上限と評価時間の上限で代える

**(c) 出力への注入**

- ルールが返す `message` と `reason` の `actual` / `expected` と、メタデータの文字列(`title`、`description`、`remediation` など)は、table 形式で端末に書く前に、表示を偽装できる文字を取り除くか、エスケープする。ログに出すときも同じ
  - 対象: 制御文字(C0・C1 の制御文字と DEL。ANSI のエスケープシーケンスを含む)、行・段落の区切り(U+2028、U+2029)、双方向テキストの制御文字(U+200E、U+200F、U+202A〜U+202E、U+2066〜U+206F、U+061C)
  - 理由: 端末や CI のログで、色や行の書き換え、行の分割、表示される文字の順序の入れ替えによって、結果の表示を偽装されないようにするため
  - `yata rules test` の text の形式の扱いは [ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「text の形式」で決めた
- JSON と SARIF では、シリアライズの際にエスケープされる
- 取り除く・エスケープする方法の詳細は、レポートの機能の Issue で決める

**(d) パスの扱い**

- `--policy-dir` / `YATA_POLICY_DIR` に指定した最上位のディレクトリは、シンボリックリンクであってもたどってよい(利用者が明示的に指定したため)
- その下の走査では、シンボリックリンクも、Windows の junction などの reparse point もたどらない。ディレクトリにもファイルにも当てはめる
  - 判定は標準ライブラリの `std::fs::symlink_metadata` と `FileType::is_symlink()` で行う。Windows では、シンボリックリンクと junction を含む、ほかの場所を指す reparse point(name surrogate)が `is_symlink()` になる。ほかの場所を指さない reparse point(OneDrive などのクラウドのファイルのプレースホルダーなど)は通常のファイル・ディレクトリとして扱う(理由は [ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「Windows の reparse point の判定」)
  - ポリシーのディレクトリの外のファイルを読んだり、走査が循環したりするのを防ぐため
  - 見つけたシンボリックリンク・reparse point は読まず、警告で飛ばさずに失敗として扱う。飛ばすだけにすると、ルールのファイルがリンクだったときにそのルールが警告だけで消え、終了コード 0 になってしまうため
    - ルールのファイル、またはそのフィクスチャのディレクトリの中: ルール単位の失敗。`rules test` では 1、それ以外のコマンドでは 2
    - それ以外(ディレクトリ、ライブラリ、ルート直下のファイル): ディレクトリ全体の失敗として、どのコマンドでも 2。特定のルールに結びつけられず、その下の多くのルールが読まれなくなるおそれがあるため
    - マニフェストがリンクの場合: 「マニフェストが無い」と同じ扱いで、どのコマンドでも 2
    - `.` で始まるディレクトリは走査しないため、その中のリンクは対象外
- 最上位だけたどる方針は、利用者が自分で置いたリンク(例: 取得したリポジトリへのリンク)は使え、リポジトリの中身に含まれたリンクでほかの場所を読まれることは防げるため、妥当と判断した
- スナップショットのディレクトリにも同じ方針を当てはめる([リソースモデル](Architecture-Resource-Model.ja.md))

**(e) regorus の組み込み関数**

- ファイルやネットワークにアクセスする機能は有効にしない
- `print` などの出力は標準出力に出さない。regorus の出力を集める機能(`set_gather_prints`)で受け取り、ログ(標準エラー出力、`debug`)に回す。標準出力はレポート専用のため
- regorus は `default-features = false` とし、ファイル・ネットワークにアクセスせず結果が入力だけで決まる組み込み関数の機能(`base64`、`base64url`、`hex`、`glob`、`graph`、`jsonpatch`、`net`、`regex`、`semver`、`time`、`urlquery`)と、`std`、`arc`、`cache` を有効にする。`http`(`http.send`)、`opa-runtime`、`uuid`、`jsonschema`、`yaml`、`coverage`、`rvm`、`allocator-memory-limits` などは有効にしない。一覧と理由は [ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「regorus の cargo feature」

### 未決事項(安全性)

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| ポリシーの署名・改ざんの確認 | 「ポリシーの取得・更新」の未決事項に含める | ポリシーのリポジトリの配布方法を決める Issue |

## 評価

図: [リソースごとの評価の流れ](../diagrams/Diagram-0030-Per-Resource-Evaluation.md)

- ルール × アカウントごとに、`target_type` のリソース 1 つずつについてルールを評価する([ADR: ルールをリソースごとに評価する](../adr/ADR-0030-Per-Resource-Rule-Evaluation.ja.md))
- `data` は、そのアカウントの全リージョンの、`target_type` と `related_types` の種別のリソースと、アカウントの情報・対象のリージョンから作る。`input` は、評価の対象の 1 つのリソース。形は [リソースモデル](Architecture-Resource-Model.ja.md) の「ポリシーエンジンへの渡し方」。この組み立ては `yata-engine` が行う
- ルールごとに regorus の `Engine` を 1 つ使う。共通の設定をした `Engine` にポリシーのディレクトリごとのライブラリを読み込んでおき、ルールごとに `clone()` してそのルールのファイルだけを読み込む
  - ルール同士がパッケージや補助ルールで干渉しないようにするため
- ルール × アカウントごとに、ルールのエンジンを `clone()` し、`add_data` で `data` を 1 回入れる。`target_type` のリソースを `(id, region)` の昇順に回し、リソースごとに `set_input` で `input` を差し替えて、次を評価する
  1. `data.yata.rules.<ID>.fail` を `eval_rule` で評価する
  2. `fail` が `true` なら、`data.yata.rules.<ID>.message` と `data.yata.rules.<ID>.reason` を `eval_rule` で評価する。ルールに定義が無い場合は undefined として扱う
  3. 値を「ルールの規約」で確かめ、`RuleResult` を作る(下の「リソース単位の結果」)。誤りがあれば、そこで止めてルールの状態を `error` にする
- `target_type` のリソースが 0 件なら、ルールを評価しない(結果は 0 件。状態は下の「状態の判定順」で決める)
- regorus は評価(`eval_rule`)のたびにルールの値のキャッシュを消す。`data` だけに依存する補助のルールも、リソースごとに計算し直される(上の「書き方の注意」)
- 評価には時間の上限を設ける。上限は**ルール × アカウントの評価の合計**に掛ける(リソースごとではない。リソースの数によらず、1 つのルールの評価が終わるまでの時間に上限を置くため)。regorus の `Engine` の実行時間の制限機能(`set_execution_timer_config`)で打ち切る。`eval_rule` は呼び出しごとにタイマーを開始し直すため、`yata-engine` は呼び出しの前に、残りの時間(上限 − そのルール × アカウントの評価を始めてからの経過時間)を制限に設定する。残りが無ければ時間切れにする。インタープリタは一定の数の文(`check_interval`。既定の設定では 64)を評価するごとに経過時間を確かめ、1 回の組み込み関数の呼び出しの途中では止まらない。上限は `--policy-timeout` で変えられる(既定 30 秒)。詳細は [ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「評価時間の上限の実現方法」
- regorus はネットワークアクセスを伴う機能を有効にしない(`default-features = false` で必要な機能だけを有効にする。上の「(e) regorus の組み込み関数」)
- ルールの評価は互いに独立なので、並列に実行してよい(並列化は実装時に必要なら行う)

## 結果の作り方

### 結果の型

`yata-engine` は、ルール × アカウントごとに 1 つの `RuleOutcome` を返します。型は `yata-core` に置きます(項目名の細部は実装時に決めてよい)。

| 型 | 項目 | 内容 |
| --- | --- | --- |
| `RuleOutcome` | `rule_id: String` | ルール ID |
| | `metadata: RuleMetadata` | ルールのメタデータ |
| | `account: String` | 評価したアカウントのアカウントキー |
| | `status: RuleStatus` | ルール全体の状態 |
| | `skip_reason: Option<SkipReason>` | `status` が `Skipped` のときだけ `Some` |
| | `error: Option<String>` | `status` が `Error` のときだけ `Some`(エラーの説明。regorus のメッセージを含みうる)。エラーの種類と、regorus のメッセージを除いた説明も別に持つ(`--omit-values` とログのため。上の「評価のエラー」) |
| | `results: Vec<RuleResult>` | リソース単位の結果。`Skipped` / `Error` のときは空 |
| | `incomplete_resources: Vec<IncompleteResource>` | 構成情報に取得の失敗の印を持っていたリソース(`target_type` / `related_types` のもの)。各要素は `resource_id`、`resource_type`、`region`(リソースの `region`)、`operation`(失敗した操作名)、`kind`。無ければ空 |
| `RuleStatus` | `Evaluated` / `Incomplete` / `Skipped` / `Error` | JSON では `evaluated` / `incomplete` / `skipped` / `error` |
| `SkipReason` | `NotCollected` / `CollectionFailed` | JSON では `not_collected` / `collection_failed` |
| `RuleResult` | `rule_id`、`status: ResultStatus`(`Pass` / `Fail`)、`severity`、`resource`(`id`、`type`、`account`、`region`)、`message`、`path`、`actual`、`expected` | リソース単位の結果。埋め方は下の「リソース単位の結果」。ルール単位の状態は持たない |

`yata-cli` は、全ルールの `RuleOutcome` と収集エラー、メタ情報から `ScanReport` を組み立てます。`ScanReport` の型は [出力形式](Architecture-Output-Formats.ja.md) で定めます。

### リソース単位の結果

図: [リソースごとの評価の流れ](../diagrams/Diagram-0030-Per-Resource-Evaluation.md)

ルールごとに、`target_type` のリソースそれぞれについて、評価の結果から `RuleResult` を 1 件作ります。リソースは `id` とリージョン(リソースの `region`)の組で区別します([リソースモデル](Architecture-Resource-Model.ja.md) の「id の一意性」)。同じ `id` が複数のリージョンに現れる種別では、それぞれが別のリソースとして評価され、別の結果になります。

| 項目 | `fail` が `false`(`pass`) | `fail` が `true`(`fail`) |
| --- | --- | --- |
| `status` | `Pass` | `Fail` |
| `resource` | 評価の対象のリソースの `id`、`type`、`account`、`region` | 同じ |
| `severity` | メタデータの `severity` | 同じ |
| `message` | 持たない(`None`) | ルールの `message`。undefined ならメタデータの `title` |
| `path` | 持たない | `reason.path`。`reason` か `path` が無ければ `None` |
| `actual` | 持たない | `reason.actual`。キーが無ければ `None`、値が `null` なら `null`(`Some(null)`) |
| `expected` | 持たない | `reason.expected`。扱いは `actual` と同じ |

- 結果は `target_type` のリソースの `(id, region)` の昇順に並べる
- `incomplete_resources` の要素と `RuleResult` も、`resource_id` とリージョンの組で対応づける(レポートで `incomplete_resources` に挙がったリソースの `pass` を区別するため)

### ルールの状態(RuleStatus)

| 状態 | 条件 |
| --- | --- |
| `evaluated` | 評価できた |
| `incomplete` | 評価できたが、`target_type` / `related_types` のいずれかの種別で、一部のリージョンの収集に失敗している、または構成情報に取得の失敗の印を持つリソースがある |
| `skipped` | 評価しなかった。理由(`skip_reason`)は `not_collected`(未収集)か `collection_failed`(収集がすべて失敗) |
| `error` | いずれかのリソースの評価中にエラーが起きた(時間切れ、`fail` が undefined・真偽値でない、`message` / `reason` の形が不正、conflict など。上の「評価のエラー」) |

### 状態の判定順

`yata-engine` は、ルールが宣言した種別ごとに、対象の組(そのアカウント × 対象のリージョン。グローバルな種別では `global` だけ)が「収集済み」「失敗」「未収集」のどれかを `Inventory` から調べ([リソースモデル](Architecture-Resource-Model.ja.md) の「0 件・未収集・失敗の区別」)、次の順で状態を決めます。先に当てはまったものを状態とします。

1. いずれかの種別で、未収集の組が 1 つでもある → `skipped`(`not_collected`。評価しない)
2. いずれかの種別で、すべての組が失敗している → `skipped`(`collection_failed`。評価しない)
3. `target_type` のリソースごとに評価を行い、いずれかのリソースで評価のエラーが起きた → `error`(そこで評価を止め、結果を作らない)
4. いずれかの種別で、失敗した組が 1 つでもある(一部のリージョンだけ失敗) → `incomplete`(収集済みの組のリソースだけで評価する)
5. `data` に入れたリソース(`target_type` / `related_types` のもの)のいずれかが、構成情報に取得の失敗の印(`{"yata_error": {...}}`)を持つ → `incomplete`(そのリソースも含めて評価し、`incomplete_resources` に記録する)
6. それ以外 → `evaluated`

- 未収集の組が 1 つでもあれば評価しないのは、評価に使うデータが欠けていることを、黙って `pass` にしないため。`scan` が直接収集する場合は、必要な種別をすべてのリージョンで収集するので、未収集は起きない(スナップショットを評価する場合に起きる)
- 取得の失敗の印を持つ `target_type` のリソースは、ルールの判定によっては `pass` の結果になりうる(印の下の項目が undefined になり、`fail` が `default` の `false` になるため)。状態を `incomplete` にし、`incomplete_resources` に挙げることで、その `pass` が確かなものでないことを利用者に示す。レポートでは、`incomplete_resources` に挙がったリソースの `pass` を区別して表示する([出力形式](Architecture-Output-Formats.ja.md))
- `aws.account.account` は常に収集済み

図: [ルール状態と終了コードの決まり方](../diagrams/Diagram-0001-Rule-Status-and-Exit-Code.md)

`skipped` と `error` のルールは結果を作りません。状態が `incomplete`、`error`、`skipped`(理由を問わない)のルールが 1 つでもあれば、スキャンは「不完全」です。終了コードへの反映は [CLI インターフェース](Architecture-CLI-Interface.ja.md) を参照してください。

## ルールとコレクターの対応

- `yata-engine` は AWS を知らないため、ルールとコレクターの対応の検査は `yata-cli` が実行時に行う。`yata-cli` は、ルールの `target_type` の最初の部分でプロバイダを決め、そのプロバイダ(`yata-provider-aws`)が公開する「対応する種別の一覧」([AWS アクセス](Architecture-AWS-Access.ja.md))と、`target_type` / `related_types` を照らし合わせる
- 絞り込んだ後のルールが、対応していない種別を宣言していたら、終了コード 2 にする。`--rule` などで対象から外したルールは検査しない(上の「処理の順序」)。`yata rules test` では全ルールを検査し、ルール単位の失敗(終了コード 1)として報告する
- ポリシーのリポジトリでは、この検査を `yata rules test` で行う(下の「ルールのテスト」)。新しい種別を使うルールは、その種別に対応した `yata` のリリースの後に、マニフェストの `min_yata_version` を上げて公開する
- `aws.account.account` はコレクターではなく `sts:GetCallerIdentity` の結果から作るため、コレクターの有無の検査の例外とする。`yata-provider-aws` の対応する種別の一覧には含める([リソースモデル](Architecture-Resource-Model.ja.md))

## ルールのテスト

`yata-engine` にフィクスチャでルールをテストする機能(`testing` モジュール)を置き、`yata rules test` から使います。ポリシーのリポジトリの CI は `yata rules test --policy-dir .` を実行して、ルールを確かめます。

`yata rules test` は、ポリシーのディレクトリのすべてのルールについて、次を行います。

| 検査 | 担当 | 内容 |
| --- | --- | --- |
| マニフェスト | `yata-engine` | 「マニフェスト」の検証と同じ。合わなければ終了コード 2 |
| 読み込みの検証 | `yata-engine` | 「検証」と同じ(ライブラリ、構文、パッケージ名とパスの一致、メタデータ、名前の規則) |
| フィクスチャのそろい方 | `yata-engine` | 各ルールに `pass_` と `fail_` のフィクスチャが 1 つ以上ある。フィクスチャの形式が正しく、宣言した種別の配列がそろっていて、宣言していない種別の配列が無い(エンジンは宣言した種別しか `data` に入れないため)。対応するルールの無い `.fixtures` ディレクトリが無い。細部は [ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「フィクスチャの検証」 |
| ダミー値 | `yata-engine` | フィクスチャに含まれる 12 桁の数字が、ダミーとして認めた値の一覧([ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「ダミーとして認めるアカウント ID」)にある |
| フィクスチャの実行 | `yata-engine` | 各フィクスチャを評価し、`expected.fail` と一致するかを確かめる(下の「フィクスチャの評価」「期待値の比べ方」) |
| プロバイダの対応 | `yata-cli` | ルールの `target_type` / `related_types` が、実行中の `yata` のプロバイダが対応する種別である |

- ディレクトリ全体に関わる失敗と引数の誤りは終了コード 2 にする
- ルール単位の失敗(上の「失敗の種類と終了コード」の表で「ルール単位」のもの)は、1 つのルールの失敗で止めずに全ルールを検査してから、失敗をまとめて報告する。失敗が 1 つでもあれば終了コード 1 にする(CI で全部の問題を一度に見られるようにするため)
- 失敗の報告には、ルール ID とファイルのパスを含める
- ポリシーのリポジトリの CI では、そのマニフェストの `min_yata_version` 以上の `yata` を使う

### フィクスチャの評価

フィクスチャは、キー `data` の下に、Rego の `data` と同じ形の値(`yata.context` の `account` と `regions`、宣言した種別の配列)を置きます([ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) の「フィクスチャの形式」、[リソースモデル](Architecture-Resource-Model.ja.md) の「ポリシーエンジンへの渡し方」)。

- `yata-engine` は、フィクスチャの `data` をそのまま Rego の `data` に入れる(種別の配列は、実際の評価と同じく `(id, region)` の昇順に並べ替える)
- `target_type` の配列のリソースを 1 つずつ、Rego の `input` にして評価する(上の「評価」と同じ処理。評価時間の上限は、フィクスチャ 1 つの評価の合計に掛ける)
- 評価のエラー(`default` の無い `fail` が undefined になった、など)は、フィクスチャの評価のエラーとしてルール単位の失敗にする

### 期待値の比べ方

図: [フィクスチャの期待値と結果の対応](../diagrams/Diagram-0011-Fixture-Expectation-Comparison.md)

フィクスチャの `expected.fail` の要素は、`id` の文字列か、`{"id": ..., "region": ...}` のオブジェクトで書きます([ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) の「フィクスチャの形式」)。

- 期待する不合格のリソースの集合を、`id` とリージョンの組の集合として作る
  - 文字列: フィクスチャの `data` の `target_type` のリソースのうち、その `id` のすべてのリソース(リージョンを問わない)
  - オブジェクト: その `id` とリージョンのリソース
- 実際の不合格のリソースの集合(上の「リソース単位の結果」で `fail` になったリソースの、`id` とリージョンの組の集合)と比べ、一致すれば合格とする
- これにより、同じ `id` のリソースのうちどのリージョンのものが不合格か(ルールが `input.region` などでコピーを正しく見分けたか)を確かめられる
- 次はフィクスチャの形式の誤り(ルール単位の失敗)とする
  - 同じ `id` の文字列を重ねて書く、同じ `{id, region}` を重ねて書く、同じ `id` を文字列とオブジェクトの両方で書く
  - オブジェクトに `id` と `region` のどちらかが無い、または文字列でない
  - 要素がフィクスチャの `data` の `target_type` のリソースを 1 つも指さない(文字列の `id` のリソースが無い、オブジェクトの `id` とリージョンのリソースが無い)。`id` を書き間違えた `fail_` のフィクスチャが、何も検出しないルールで黙って通るのを防ぐため
  - フィクスチャの `data` の同じ種別の配列に、`id` と `region` の組が同じリソースが複数ある([リソースモデル](Architecture-Resource-Model.ja.md) の「id の一意性」)

フィクスチャの書式は [ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) を参照してください。

## 出典

- regorus の `Engine` の API(`eval_rule`、`clone`、`set_input`、`add_data`、`set_execution_timer_config`、`set_gather_prints` など): <https://docs.rs/regorus/0.12.0/regorus/struct.Engine.html>
- regorus 0.12.0 のソース(`add_data` と `set_input` の違い、評価ごとの状態の初期化): <https://docs.rs/crate/regorus/0.12.0/source/src/engine.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/interpreter.rs>
- Rego の言語仕様(undefined の扱い、`default`、完全ルールの conflict、`else`、`object.get` など): <https://www.openpolicyagent.org/docs/policy-language>
- JSON Pointer(RFC 6901): <https://www.rfc-editor.org/rfc/rfc6901>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| ルールの除外・抑制 | 特定のリソースやルールを結果から除外する仕組み(設定ファイル、タグなど)。ルール ID にバージョンを含めないので、ID を基準にできる | 別 Issue |
| ポリシーのディレクトリの既定の場所 | プラットフォームのデータディレクトリ(例: Linux の `~/.local/share/yata/policies`)を既定にするか。既定にする場合はパスを決めるクレート(`directories` など)の追加が必要([初期依存クレート](Architecture-Initial-Dependencies.ja.md))。決まるまでは既定なし(指定が無ければ終了コード 2) | ポリシーのリポジトリの配布方法を決める Issue |
| ポリシーの取得・更新 | ポリシーのリポジトリの取得・更新(git clone、リリースのアーカイブのダウンロードなど)を `yata` が行うか、利用者に任せるか。行う場合はネットワークへのアクセスと署名・改ざんの確認が必要になる。決まるまでは `yata` は取得しない | ポリシーのリポジトリの配布方法を決める Issue(この Issue の範囲外) |
| ポリシーのリポジトリの構成 | リポジトリの名前、CI、リリースの方法、`yata` の取得方法(CI で使う `yata` のバージョンの固定など) | ポリシーのリポジトリを作る Issue(この Issue の範囲外) |
| `data` だけに依存する計算の共有 | regorus は評価のたびにルールの値のキャッシュを消すため、`data` だけに依存する補助のルールがリソースごとに計算し直される。大きなアカウントで遅いルールが現れたら、ルール × アカウントごとに 1 回だけ計算して共有する仕組み(例: ルールが宣言した補助のルールをエンジンが先に評価して `data` に入れる)を検討する。仕組みはルールの書式に関わるため、入れるなら `format_version` との関係も決める | リソースごとの評価を実装する Issue で性能を測り、問題があればその時点で別 Issue |
| `time.now_ns` とフィクスチャ | 現在の時刻を使うルールのフィクスチャの結果が、実行する日で変わらないようにする方法([ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の未決事項) | 時刻を使うルールを作るポリシーのリポジトリの Issue の前 |
