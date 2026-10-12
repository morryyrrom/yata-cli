# AWS プロバイダの基盤

> This Japanese version is the authoritative source. The English version (`Feature-0007-AWS-Provider-Foundation.md`) has not been written yet.

`yata-provider-aws` の基盤(コレクターの枠組み、ビルド時の自動検出、クライアントの生成、読み取り専用ガード、認証情報とリージョンの解決、並行実行、公開 API)と、後続のコレクターが使う依存クレートの追加を設計します。

全体設計は [アーキテクチャ概要](../architecture/Architecture-Overview.ja.md)、AWS への接続とコレクターは [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、収集結果の型は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、エラーの扱いは [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) を参照してください。コレクターの自動検出は [ADR: コレクターのビルド時自動検出](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md)、読み取り専用ガードは [ADR: AWS SDK for Rust と読み取り専用ガード](../adr/ADR-0001-AWS-SDK-and-Read-Only-Guard.ja.md) と [ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) に従います。最初のルール群で使うリソース種別と SDK のクレートは [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) で決めています。

## 目的と背景

- サービスごとのコレクター(#12〜#17)を、共有ファイルに触れずに「コレクターのファイル 1 つの追加」で並行して実装できるようにする
- すべての SDK のクライアントに読み取り専用ガードを付け、書き込み系の操作と、秘密の値の取得・認証情報の発行を行う操作を、送信前に止める
- 全体設計で「AWS プロバイダを実装する Issue で決める」としていた次の事項を、この設計で決める
  - ガードの実装方法と、ガードが比べるサービス名の文字列([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の未決事項)
  - `--profile` のときに、プロファイルの認証情報だけを使う方法([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) の未決事項)
  - `--concurrency` の既定値と再試行の回数(同上)
  - SDK のエラーからエラーの種類(`access_denied` など)を判定する方法([エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) の未決事項)
- [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の後続の Issue のうち、次の 2 つをこの Issue に含める
  - 2(AWS SDK の依存の追加): ルートの `Cargo.toml` と `Cargo.lock` の変更を、この Issue の実装の PR で 1 回だけ行う
  - 3(読み取り専用ガードの例外の追加): 例外リストを IAM の `GenerateCredentialReport` から始める

## スコープ

- `Collector` トレイト、`Scope`、`CollectContext`、`CollectFuture`
- build.rs によるコレクターの検出と、モジュール宣言・コレクターの一覧の生成
- `src/support/` のクライアント生成のヘルパー
- SDK のインターセプターとしての読み取り専用ガードと、拒否リスト・例外リスト
- `aws-config` による認証情報とリージョンの解決(`--profile` の扱いを含む)
- `sts:GetCallerIdentity` による `aws.account.account` の作成と、`ec2:DescribeRegions` による対象リージョンの決定
- アカウント × リージョン × 種別の並行実行、同時実行数の制限、中断(Ctrl-C)の扱い
- `ResourceSet`、`CollectionError`、`Inventory` など、収集結果の型(`yata-core`)
- SDK のエラーからエラーの種類への対応づけと、構成情報の失敗の印を作るヘルパー
- 公開 API(対応する種別の一覧、収集の関数)
- 依存の追加(ルートの `[workspace.dependencies]`、`crates/yata-provider-aws/Cargo.toml`、`crates/yata-core/Cargo.toml`)
- 上記のテスト(`aws-smithy-mocks` によるモック。実 AWS には接続しない)

## スコープ外

- 各サービスのコレクター(#12〜#17)。この Issue では `src/collectors/` にコレクターを置かない
- ポリシーエンジンと CLI のコマンド(`--profile`、`--region`、`--concurrency` の引数の解析と終了コードへの対応づけは CLI の Issue で行う。この設計では、CLI が使う公開 API と、エラーの種類ごとの終了コードの対応を決める)
- スナップショットの書き込み・読み込みと、日時の RFC 3339 の文字列への整形(日時の整形に使うクレートは [初期依存クレート](../architecture/Architecture-Initial-Dependencies.ja.md) の未決事項のまま)
- SDK の型から構成情報への変換を補助するマクロ・ヘルパー([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の未決事項。最初のコレクターの Issue で決める)
- `docs/architecture/` の変更。この設計と全体設計が食い違う点は、下の「アーキテクチャとの差異」にまとめる

## 振る舞い

### 公開 API

`yata-provider-aws` が公開するのは次の項目だけです。`Collector` トレイト、`CollectContext`、ガード、クライアントの生成は公開しません(名前は実装時に変えてよい)。

| 項目 | 内容 |
| --- | --- |
| `supported_resource_types() -> Vec<ResourceTypeSpec>` | 作れるリソース種別の一覧。各要素は種別名、スコープ(`Scope::Global` / `Scope::Regional`)、`required_permissions`(IAM アクション名)。`aws.account.account`(`global`、`["sts:GetCallerIdentity"]`)を必ず含む。種別名の昇順 |
| `setup_permissions() -> SetupPermissions` | コレクターと関係なく収集の準備で使う権限。常に要る `sts:GetCallerIdentity` と、`--region` を指定しないときだけ要る `ec2:DescribeRegions` を分けて返す。`yata permissions` が使う |
| `collect(request: CollectRequest, cancel: impl Future<Output = ()> + Send) -> Result<Inventory, ProviderError>` | 収集を行い `Inventory` を返す。`cancel` が完了したら中断する(下の「中断」) |
| `CollectRequest` | `resource_types`(収集する種別の名前)、`profile`(`--profile` の値。`Option<String>`)、`regions`(`--region` の値。`Option<Vec<String>>`)、`concurrency`(`NonZeroUsize`) |
| `DEFAULT_CONCURRENCY` | `--concurrency` の既定値(16。下の「並行実行」) |
| `ProviderError` | 収集を始められない・続けられないエラー(下の「エラー時」) |

- `CollectRequest.resource_types` に `aws.account.account` を含めても含めなくてもよい。`aws.account.account` は常に作る
- `collect` は非同期関数で、tokio のランタイムの中で呼ぶ(ランタイムは `yata-cli` が作る)

### 収集の流れ

全体の流れは [収集の流れの図](../diagrams/Diagram-0007-Collection-Sequence.md) を参照してください。

1. **要求の検査**: `resource_types` に対応していない種別があれば `ProviderError::UnsupportedResourceType`。重複した種別は 1 つにまとめる。`regions` に形式の誤った値があれば `ProviderError::InvalidRegion`(下の「リージョン名の検査」)。重複したリージョンは 1 つにまとめる。`regions` が `Some` で空の配列の場合も `ProviderError::InvalidRegion` にする(`None` と同じに扱うと、利用者が対象を絞ったつもりで全リージョンを収集することになるため。CLI の引数の解析では空にならないが、ライブラリの API として検査する)
2. **設定の読み込み**: `aws-config` で認証情報とリージョンを解決し、`SdkConfig` を作る(下の「認証情報とリージョンの解決」)
3. **アカウントの確認**: 最初の API のリージョンに向けて、ガード付きの STS のクライアントで `GetCallerIdentity` を呼ぶ
   - `Account` が 12 桁の数字でなければ `SetupError`
   - `Arn` の第 2 要素(パーティション)が `aws` でなければ `SetupError`
   - アカウントキー `aws-<Account>` と、`aws.account.account` のリソース 1 件を作る。`configuration` は `{"GetCallerIdentity": {"Account": "<Account>"}}` だけとし、`Arn`・`UserId` は入れない([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「アカウントのリソース」)
4. **対象リージョンの決定**
   - `regions` があれば、それを昇順に並べたものを対象にする。`DescribeRegions` は呼ばない
   - 無ければ、最初の API のリージョンに向けて `ec2:DescribeRegions` を既定のパラメーターで呼ぶ(`AllRegions`、`Filters`、`RegionNames` を付けない)。API の既定では、アカウントで有効なリージョンだけが返る。`RegionName` を昇順に並べたものを対象にする
   - `DescribeRegions` の失敗(権限不足を含む)は `SetupError`。メッセージで `--region` の指定を案内する
5. **組の展開**: 収集する種別ごとに、`Scope::Global` なら 1 組(リージョン `global`)、`Scope::Regional` なら対象リージョンごとに 1 組を作る。実行の順は、`global` の組(種別名の昇順)→ リージョンの昇順 × 種別名の昇順。時間の掛かる IAM などのグローバルな組を先に始めるため
6. **並行実行**: 組ごとにコレクターを実行する(下の「並行実行」)
7. **組み立て**: `Inventory` を作って返す(下の「収集結果の型」)。`aws.account.account` は、要求に含まれていなくても次のように常に入れる
   - `accounts` に、そのアカウントの `AccountInfo` を 1 件
   - `resource_types` に、`aws.account.account`(`global`)
   - `resource_sets` に、`region` が `global` で、手順 3 で作ったリソース 1 件だけを持つ `aws.account.account` の `ResourceSet`。手順 3 が失敗した場合は収集全体が `SetupError` になるため、この種別の `CollectionError` は生じない

### `collect` に渡す `region`

- `Scope::Regional` のコレクターには、その組のリージョン(例: `ap-northeast-1`)を渡す
- `Scope::Global` のコレクターには、パーティションの既定リージョン(`aws` パーティションでは `us-east-1`)を渡す。API はこのリージョンに向けて呼ぶ([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の「アカウントとリージョン」)
- リソースの `region` の値はコレクターが決める。グローバルな種別は通常 `global`、`aws.s3.bucket` はバケットのリージョン([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「S3 のバケット」)。`ResourceSet.region` は組のリージョン(`global` またはリージョン名)

### リージョン名の検査

- `--region` の値は、英小文字・数字・`-` だけからなり、英小文字で始まり数字で終わる、32 文字以下の文字列に限る(例: `us-east-1`、`ap-southeast-7`)。リージョン名は SDK がエンドポイントのホスト名に使うため、想定外の文字を渡さないための形式の検査で、存在するリージョンかどうかは確かめない
- 存在しない・有効でないリージョンを指定した場合は、そのリージョンの組がそれぞれ接続の失敗(`region_unavailable` など)として `CollectionError` になる

### 中断

- `collect` は、引数の `cancel` が完了した時点で、実行中のすべてのタスクを中止し、`ProviderError::Interrupted` を返す。準備(`GetCallerIdentity`、`DescribeRegions`)の途中でも同じ
- タスクは `tokio::task::JoinSet` で持ち、`collect` の future が破棄されたときにも、すべてのタスクが中止される(`JoinSet` の破棄で中止される)
- `yata-cli` は `cancel` に Ctrl-C の待ち受け(`tokio::signal::ctrl_c()`)を渡し、`Interrupted` を受けたら結果を出力せずに終了コード 130 で終了する。ライブラリの中ではシグナルを待ち受けない(テストで中断を確かめられるようにするため)
- 中止された呼び出しはすべて読み取りの操作なので、途中で止めても AWS 側に影響は残らない

### エラー時

| エラー | 内容 | `yata-cli` での終了コード |
| --- | --- | --- |
| `ProviderError::UnsupportedResourceType` | 対応していない種別の要求(通常は CLI の対応の検査で先に止まる) | 2 |
| `ProviderError::InvalidRegion` | `--region` の形式の誤り、または `regions` が空の配列 | 2 |
| `ProviderError::Setup(SetupError)` | 認証情報が無い・解決できない、指定したプロファイルが無い、`GetCallerIdentity` の失敗、`Account` の形式の誤り、`aws` 以外のパーティション、`DescribeRegions` の失敗 | 3 |
| `ProviderError::Interrupted` | `cancel` の完了(Ctrl-C) | 130 |
| (`Inventory.collection_errors`) | 組ごとの収集の失敗。`collect` は `Ok` を返す | 4(`--fail-on` 以上の検出が無い場合) |

- エラーのメッセージは英語で、認証情報の値を含めない。`SetupError` のメッセージには、SDK のエラーのコード(例: `ExpiredToken`)を含め、サービスが返したメッセージの本文は含めない(本文には実行者の ARN が含まれることがあるため)。本文を含む詳細は `debug` のログに出す
- エラーの enum には `#[non_exhaustive]` を付け、元のエラーは `#[source]` で保持する([エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md))

### ログ

- `info`: 対象リージョンの数と一覧、組の数、収集の所要時間、成功・失敗の組の数
- `warn`: `CollectionError` ごとに 1 行(アカウントキー、リージョン、種別、種類、メッセージ)
- `debug`: 組ごとの開始と終了、リソースの件数、SDK のエラーの詳細
- `warn`(ガード): 読み取り専用ガードが操作を止めたとき。メッセージを `read-only guard blocked <サービス名>/<操作名>` の形にし、`CollectionError` ごとの `warn` と区別できるようにする。サービス名と操作名だけを出す。収集は続くため、`error`(処理を続けられないエラー)にはしない
- 認証情報の値はどのレベルでも出さない

## 認証情報とリージョンの解決

全体の流れは [認証情報とリージョンの解決の図](../diagrams/Diagram-0007-Credential-and-Region-Resolution.md) を参照してください。

### 認証情報

| 指定 | 認証情報プロバイダ |
| --- | --- |
| `--profile` なし | `aws_config::defaults(BehaviorVersion::latest())` の既定の認証情報プロバイダチェーン(環境変数 → 共有の `config` / `credentials` ファイル(`AWS_PROFILE`)→ Web ID → ECS → IMDSv2) |
| `--profile <NAME>` | `aws_config::profile::ProfileFileCredentialsProvider::builder().profile_name(NAME).build()` を作り、`ConfigLoader::credentials_provider` に渡す。あわせて `ConfigLoader::profile_name(NAME)` も設定する |

- `ConfigLoader::credentials_provider` で認証情報プロバイダを与えると、`aws-config` はその部分の既定の解決のチェーンを使わない(`aws-config` のドキュメント: 「Overriding a component will skip the standard resolution chain from for that component」)。このため、`--profile` のときは環境変数の認証情報(`AWS_ACCESS_KEY_ID` など)を見ない
- `ConfigLoader::profile_name` だけでは足りない。既定のチェーンでは環境変数の認証情報がプロファイルより先に見られるため、`profile_name` を指定しても、環境変数の認証情報があればそちらが使われる
- `ProfileFileCredentialsProvider` は、プロファイルの静的な認証情報、`role_arn` と `source_profile` / `credential_source` による AssumeRole、`credential_process`、SSO を扱う。プロファイルが `credential_source = Environment` を指定している場合は、利用者がプロファイルで選んだ方法として、その AssumeRole の元の認証情報に環境変数を使う
- `ConfigLoader::profile_name` は、`AWS_PROFILE` より優先される(`aws-config` のドキュメントの順: この指定 → `AWS_PROFILE` → `default`)。認証情報以外のプロファイルの設定(`max_attempts`、`retry_mode` など)も、指定したプロファイルから読む
- `ProfileFileCredentialsProvider` は `ConfigLoader` の外で作るため、`configure(&ProviderConfig)` で、下で解決した最初の API のリージョンを渡す(AssumeRole の STS の呼び出し先を決めるため)。`ProviderConfig` の作り方の細部は実装時に `aws-config` の API に合わせる
- 指定したプロファイルが無い・認証情報を解決できない場合、エラーは最初の API(`GetCallerIdentity`)の呼び出しで出る。これを `SetupError` にする

### 最初の API のリージョン

[CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) の「AWS の環境変数との関係」の順を、`ConfigLoader::region` に渡すリージョンのプロバイダで実現します。

| 指定 | リージョンのプロバイダ |
| --- | --- |
| `--profile` なし | `RegionProviderChain::default_provider().or_else(Region::from_static("us-east-1"))`(SDK の既定の解決 → `us-east-1`) |
| `--profile <NAME>` | `RegionProviderChain::first_try(EnvironmentVariableRegionProvider::new()).or_else(ProfileFileRegionProvider::builder().profile_name(NAME).build()).or_else(Region::from_static("us-east-1"))`(`AWS_REGION` → `AWS_DEFAULT_REGION` → そのプロファイルの `region` → `us-east-1`) |

- `EnvironmentVariableRegionProvider` は `AWS_REGION` を先に見て、無ければ `AWS_DEFAULT_REGION` を見る(`aws-config` のドキュメント)。SDK の既定の解決(`--profile` なし)も同じプロバイダを使う。AWS CLI も `AWS_DEFAULT_REGION` を読み、`AWS_REGION` を優先するため、AWS CLI と同じ扱いになる。[CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) の順には `AWS_DEFAULT_REGION` が無いため、「アーキテクチャとの差異」に挙げる
- このリージョンは `GetCallerIdentity`、`DescribeRegions`、AssumeRole にだけ使い、スキャンの対象リージョンには使わない
- `yata` は AWS の環境変数を自分で読まず、`aws-config` のプロバイダに任せる

### 再試行とタイムアウト

- 再試行は SDK の既定の解決に任せ、コードで `retry_config` を設定しない(下の「再試行の回数」)
- タイムアウトは SDK の既定のまま(`BehaviorVersion::latest()` の既定)とし、操作ごとのタイムアウトは設けない(未決事項)

## 読み取り専用ガード

### 実装方法

インターセプターから操作名とサービス名を取得できることを、`aws-smithy-runtime-api` の公開 API と SDK のソースで確かめました。クライアントをラップする型で操作を限定する代わりの方法は使いません。

- 生成された SDK のコードは、各操作の実行時の設定(`RuntimePlugin::config`)で、`ConfigBag` に操作のメタデータを入れる。例(`aws-sdk-sts` の `GetCallerIdentity`): `cfg.store_put(::aws_smithy_runtime_api::client::orchestrator::Metadata::new("GetCallerIdentity", "STS"));`
- `Metadata` は公開の型で、`name()`(操作名)と `service()`(サービス名)を持ち、`Storable` を実装している(`StoreReplace`)。インターセプターは `cfg.load::<Metadata>()` で読める
- ガードは `Intercept` トレイトを実装し、`read_before_serialization(&self, context, runtime_components, cfg: &mut ConfigBag)` で判定する
  - このフックは、入力をリクエストにシリアライズする直前に、実行ごとに 1 回呼ばれる。エラーを返すと `modify_before_completion` に進み、エラーが実行の結果になるため、シリアライズ・署名・送信は行われない
  - `read_before_execution`(実行の始めのフック)は使わない。`aws-smithy-runtime` のオーケストレーター(1.16.0 の `orchestrator.rs`)では、クライアントに付けたインターセプターの `read_before_execution` が呼ばれた後に操作の設定(操作のメタデータを含む)が適用されるため、このフックでは `Metadata` を読めない
  - `read_before_serialization` の時点では操作の設定が適用済みで、`Metadata` を読める
  - 再試行は送信の段階の繰り返しで、シリアライズの前のフックは実行に 1 回だけ呼ばれる。操作は変わらないため、実行に 1 回の判定で足りる
- `Metadata` が `ConfigBag` に無い場合は拒否する(判定できないときは止める側に倒す)
- ガードの実装に使う型は、各 SDK のクレートの `config` モジュールが再エクスポートしている(`Intercept`、`ConfigBag`、`RuntimeComponents`、`interceptors` モジュール)。`Metadata` と `BoxError` は `aws-smithy-runtime-api`(`client` 機能)から使う

### サービス名の文字列

ガードが比べるサービス名は `Metadata::service()` の値で、SDK のソースで確かめた値は次のとおりです(Smithy のサービスのモデルの `sdkId` と同じ表記。`aws-sdk-<service>` の `<service>` とは違う)。

| SDK のクレート | `Metadata::service()` | 確かめた操作 |
| --- | --- | --- |
| `aws-sdk-sts` | `STS` | `GetCallerIdentity` |
| `aws-sdk-ec2` | `EC2` | `DescribeRegions` |
| `aws-sdk-iam` | `IAM` | `GenerateCredentialReport` |
| `aws-sdk-s3` | `S3` | `GetObject` |
| `aws-sdk-s3control` | `S3 Control` | `GetPublicAccessBlock` |
| `aws-sdk-cloudtrail` | `CloudTrail` | `DescribeTrails` |
| `aws-sdk-cloudwatchlogs` | `CloudWatch Logs` | `DescribeMetricFilters` |
| `aws-sdk-cloudwatch` | `CloudWatch` | `DescribeAlarms` |
| `aws-sdk-sns` | `SNS` | `ListTopics` |
| `aws-sdk-secretsmanager`(拒否リストのみ) | `Secrets Manager` | `GetSecretValue` |
| `aws-sdk-ssm`(拒否リストのみ) | `SSM` | `GetParameter`、`GetParametersByPath` |
| `aws-sdk-ecr`(拒否リストのみ) | `ECR` | `GetAuthorizationToken` |

- 比較は大文字・小文字を区別する完全一致とする
- SDK の更新でこの値が変わると、拒否リスト・例外リストの項目が当たらなくなる。下の「テスト方針」のガードのテストで、各項目が実際のクライアントで当たることを確かめ、変わったことを検出する(この表の SDK のクレートのうち、依存に入っている 9 つが対象)

### 判定

判定の流れは [判定の図](../diagrams/Diagram-0007-Read-Only-Guard-Decision.md) を参照してください。

判定は「拒否リスト → 例外リスト → 接頭辞」の順に行います([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md))。

1. (サービス名, 操作名)が拒否リストにあれば拒否する
2. 例外リストにあれば許可する
3. 操作名が `Describe`、`List`、`Get` のいずれかで始まり、その直後が英大文字なら許可する(`Get` + `CallerIdentity` など。接頭辞だけの一致で、単語の途中で切れる名前を許さないため)
4. それ以外は拒否する

拒否したときは、`BoxError` としてガードのエラー(`ReadOnlyViolation`。サービス名・操作名・理由(拒否リスト / 接頭辞に当たらない / メタデータが無い)を持つ)を返します。SDK はこれを `SdkError` の原因として呼び出し元に返し、再試行しません。

### 拒否リストと例外リスト

- `src/support/guard_lists.rs` に、2 つの静的な配列として書く。各項目は、サービス名、操作名、対応する IAM アクション名、理由
- 初期の拒否リストは [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の 9 項目(Secrets Manager / `GetSecretValue`、SSM / `GetParameter`・`GetParameters`・`GetParametersByPath`・`GetParameterHistory`、STS / `GetSessionToken`・`GetFederationToken`、ECR / `GetAuthorizationToken`、S3 / `GetObject`)。サービス名は上の表の文字列
- 初期の例外リストは次の 1 項目([ADR-0005](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md))

| サービス名 | 操作名 | IAM アクション名 | 理由 |
| --- | --- | --- | --- |
| `IAM` | `GenerateCredentialReport` | `iam:GenerateCredentialReport` | アカウントに 1 つの認証情報レポートを作り直すだけで、IAM のリソース・設定を作成・変更・削除しない。CIS の 2.10〜2.12 の判定に要る |

- 例外は(サービス名, 操作名)の組で当てる。ほかのサービスの同名の操作や、IAM のほかの `Generate*` の操作(`GenerateServiceLastAccessedDetails` など)は許可しない
- 2 つのリストの変更は、機能の PR に混ぜず、理由とともに単独の PR で行う([ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md) の「中央の列挙を許すもの」)

### 認証情報の解決で SDK が内部で呼ぶ操作

`aws-config` が AssumeRole や SSO のために内部で作るクライアントにはガードが付きません。これは全体設計([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の「認証情報の解決で SDK が内部で呼ぶ操作」)のとおり、対象外として許容します。

## クライアントの生成

- コレクターと基盤は、SDK のクライアントを `CollectContext::client::<C>(region)` でだけ得る。`C` は SDK のクライアントの型(例: `aws_sdk_s3::Client`)
- `src/support/clients.rs` に、トレイト `GuardedClient`(`fn build(factory: &ClientFactory, region: Region) -> Self`)と、それを SDK のクレートごとに実装するマクロを置き、依存に入れた 9 つのクレートについてマクロを呼ぶ
  - 実装の中身: `<sdk>::config::Builder::from(&sdk_config)` → `.region(region)` → `.interceptor(ReadOnlyGuard)` → (テストのときだけ)追加のインターセプター → `<sdk>::Client::from_conf(...)`
  - `Client::new` / `Client::from_conf` を書いてよいのは、このファイルだけとする(ソースの検査のテストで確かめる)
  - 対応していないサービスのクライアントは `client::<C>()` で作れない(コンパイルエラーになる)。サービスを足すときは、依存の追加(共有ファイルの単独の PR)と同じ PR で、このファイルのマクロの呼び出しに 1 行足す
- `ClientFactory` は `SdkConfig` を持つ。`SdkConfig` の HTTP クライアント(接続のプール)と認証情報のキャッシュは、同じ `SdkConfig` から作ったクライアントの間で共有されるため、組ごとにクライアントを作ってもよい
- `CollectContext` は `SdkConfig` をコレクターに公開しない(コレクターが自分でクライアントや設定を作れないようにするため)

## コレクターの枠組み

### `Collector` トレイト

[AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の「コレクター」の概略のとおりとします。

```rust
/// Boxed future returned by [`Collector::collect`].
pub(crate) type CollectFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Resource>, CollectError>> + Send + 'a>>;

pub(crate) trait Collector: Send + Sync {
    /// Resource type produced by this collector, e.g. "aws.s3.bucket".
    fn resource_type(&self) -> &'static str;
    /// Whether the resource is global or regional.
    fn scope(&self) -> Scope;
    /// IAM actions required by this collector, e.g. ["s3:ListAllMyBuckets"].
    fn required_permissions(&self) -> &'static [&'static str];
    /// Collects resources by calling APIs in `region`.
    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a>;
}
```

- `Scope` は `yata-core` の型を使う(`ResourceTypeInfo` のスコープと同じ型。`global` / `regional` にシリアライズする)
- 各コレクターのファイルは `pub(crate) fn collector() -> Box<dyn Collector>` を定義する([ADR](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md))
- コレクターの中の API の呼び出しは順に行う(コレクターの中で並行に呼ばない)。並行に呼ぶ必要が出た場合の扱いは未決事項
- 基盤は、API のパラメーターを加えたり、結果を絞り込んだりしない。コレクターは、ページング・必須の識別子以外は既定のパラメーターで API を呼ぶ([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「コレクターの方針」)

### `CollectContext`

| 項目 | 内容 |
| --- | --- |
| `account_id() -> &str` | アカウント ID(12 桁) |
| `account_key() -> &str` | アカウントキー(`aws-<account_id>`) |
| `client::<C: GuardedClient>(region: &str) -> C` | ガード付きのクライアントを、指定したリージョンに向けて作る。`aws.s3.bucket` のように、`collect` に渡された `region` 以外のリージョン(バケットのリージョン)に送る場合も、これで作る |

- 1 回の収集で 1 つの `CollectContext` を作り、すべての組で共有する(`Arc`)

### `CollectError` と失敗の印

`src/support/errors.rs` に、コレクターが使う次のものを置きます。

- `CollectError`: 組全体の失敗(リソースを列挙する操作の失敗)を表すエラー。`CollectError::from_sdk(operation, err)` で、SDK のエラーから種類(`CollectionErrorKind`)とメッセージを作る
- `failure_mark(operation, &err) -> serde_json::Value`: リソースごとの付随情報を取る操作の失敗を、構成情報の失敗の印 `{"yata_error": {"kind": ..., "message": ...}}` にする([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「操作ごとの取得の失敗」)
- 「設定が存在しない」に当たるエラーの判定は、コレクターごとに決める(各コレクターの機能設計書)。判定にはエラーのコード(`ProvideErrorMetadata::code()`)を使う

### エラーの種類の判定

SDK のエラー(`SdkError`)から `CollectionErrorKind` を次のように決めます。上の行から順に当てはめます。

| 条件 | 種類 |
| --- | --- |
| 原因をたどって読み取り専用ガードのエラー(`ReadOnlyViolation`)がある | `service`(メッセージで、ガードが止めたことと操作名を示す) |
| `TimeoutError`、または `DispatchFailure` で `is_timeout()` が真 | `timeout` |
| `DispatchFailure` で `is_io()` が真(DNS の解決の失敗、接続の失敗など) | `region_unavailable` |
| `ResponseError`(応答を解釈できない) | `invalid_response` |
| `ServiceError` で、コードが `AccessDenied`、`AccessDeniedException`、`UnauthorizedOperation`(EC2)、`AuthorizationError`(SNS) | `access_denied` |
| `ServiceError` で、コードが SDK の再試行で「スロットリング」に分類されるもの(`Throttling`、`ThrottlingException`、`ThrottledException`、`RequestThrottledException`、`TooManyRequestsException`、`RequestLimitExceeded`、`SlowDown` など。AWS の SDK の文書の一覧) | `throttled`(再試行を尽くした後) |
| `ServiceError` で、コードが `OptInRequired` | `region_unavailable` |
| それ以外(上に当たらない `ServiceError`、`ConstructionFailure` など) | `service` |

- 判定は、エラーの事実(変種、コード、タイムアウト・I/O か)を受け取る純粋な関数にして、単体テストで表を確かめる
- コード一覧のサービスごとの過不足は、各コレクターの機能設計書で確かめて足す(この表への追加は `support/errors.rs` の変更で、機能の PR で行ってよい)
- メッセージは `"<種類の説明> when calling <操作名> (<コード>)"` の形の英語とする(例: `"access denied when calling ListBuckets (AccessDenied)"`)。サービスが返したメッセージの本文は入れない(本文には実行者の ARN が含まれることがあり、`CollectionError` と失敗の印はレポートとスナップショットに残るため)。本文を含む詳細は `debug` のログに出す

## ビルド時の自動検出

[ADR: コレクターのビルド時自動検出](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md) に従い、次のように実装します。

- 検出と生成の処理は、標準ライブラリだけを使う `crates/yata-provider-aws/build/discover.rs` に書き、`build.rs` は `#[path]` でこれを読み込んで呼ぶだけにする。テスト(`tests/discover.rs`)も同じファイルを `#[path]` で読み込み、一時ディレクトリで検出の規則を確かめる
- 走査の対象は `src/collectors/<service>/<resource>.rs` だけ
  - `<service>` は `[a-z][a-z0-9]*`(`_` を含めない)。`aws-sdk-<service>` の `<service>` に合わせる
  - `<resource>` は `[a-z][a-z0-9_]*`
  - どちらも Rego の予約語([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「リソース種別」)を使わない
  - `<service>` に `_` を含めないので、モジュール名 `<service>_<resource>` は重ならず、Rust の予約語にもならない
- 名前の規則と場所の規則を当てるのは、ディレクトリと `.rs` のファイルだけとする。`.rs` 以外のファイル(`.gitkeep`、`README.md` など)は、場所と名前にかかわらず無視する
- 次のものはビルドのエラーにする
  - `src/collectors/` の直下の `.rs`
  - `src/collectors/<service>/` の下のディレクトリ(3 階層目以降。中身にかかわらず)
  - `mod.rs`
  - `<service>` の位置にある、名前の規則に合わないディレクトリ(中身が空でも)
  - `<resource>` の位置にある、名前の規則に合わない `.rs`
  - シンボリックリンク(ファイル・ディレクトリとも)
- `src/collectors/` が無い・空の場合は、空の一覧を生成する。この Issue ではコレクターを置かないため、ディレクトリを Git で保つ空のファイル(`.gitkeep`)だけを置く。`.gitkeep` は検出の規則で無視されるため、コレクターを足す PR でも消さずに残す(並列の PR で削除が衝突しないようにするため)
- 生成するもの(`OUT_DIR/collectors_generated.rs`。ファイルのパスの昇順)
  - `#[path = "<絶対パス>"] mod <service>_<resource>;`(パスは `{:?}` で文字列リテラルにし、Windows の `\` と空白を正しく扱う)
  - `pub(crate) fn all_collectors() -> Vec<Box<dyn Collector>>`
  - `pub(crate) const COLLECTOR_PATH_TYPES: &[&str]`: 各ファイルのパスから作った種別名 `aws.<service>.<resource>`(`all_collectors()` と同じ順)
- `cargo:rerun-if-changed=src/collectors`、`cargo:rerun-if-changed=build.rs`、`cargo:rerun-if-changed=build/discover.rs` を出す
- `src/collectors.rs` は `include!(concat!(env!("OUT_DIR"), "/collectors_generated.rs"));` だけを書く

### 種別名とファイルのパスの対応

- コレクターの種別名は、ファイルのパスから決まる `aws.<service>.<resource>` と一致しなければならない(例: `collectors/s3/bucket.rs` は `aws.s3.bucket`)。テストで `resource_type()` と `COLLECTOR_PATH_TYPES` を比べる
- これにより、1 つの種別を 2 つのコレクターが作ることはファイルの配置の上で起こらない(重複のテストも置く)
- `collectors/account/account.rs` は置かない(`aws.account.account` は基盤が作る)。テストで確かめる
- [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の 16 種別のファイルの配置は、すべてこの規則に合う

## 並行実行

組の結果の振り分けは [組の結果の振り分けの図](../diagrams/Diagram-0007-Group-Result-Routing.md) を参照してください。

- 許可の数が `concurrency` の `tokio::sync::Semaphore` を使う。上の「収集の流れ」の手順 5 の順に組を回すループの中で、先に許可(`acquire_owned`)を得てから、その組のタスクを `JoinSet` に入れ、許可をタスクに渡す。タスクは終わったら許可を返す(破棄する)
  - タスクの中で許可を待つ形にすると、許可を得る順がタスクが最初にポーリングされる順になり、生成の順にならない。生成のループで許可を得ることで、実行を始める順を手順 5 の順に保つ
  - 許可の待ちは `cancel` と `select` し、待っている間の中断にも応じる
- タスクの結果
  - 成功: リソースを `id` の昇順に並べ、`ResourceSet` にする(0 件でも作る)。並べ替えは基盤が行い、コレクターは順序を気にしなくてよい
  - コレクターが返したリソースの `type` がコレクターの種別と違う、`account` がアカウントキーと違う、または同じ `id` のリソースが組の中に 2 件以上ある場合は、コレクターの誤りとして、その組を `CollectionError`(`invalid_response`)にする(`id` は `ResourceSet` の中で一意。[最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「同じ ARN の証跡と `id`」で改める形でも、`ResourceSet` の中では一意)
  - `CollectError`: `CollectionError` に変換して記録し、ほかの組は続ける
  - タスクのパニック(`JoinError::is_panic`): その組を `CollectionError`(`service`、メッセージ `"collector panicked"`)にして続ける
- 中断は上の「中断」のとおり

### `--concurrency` の既定値: 16

同時に実行する組(タスク)の数の上限です。

- **AWS の制限の単位**: スロットリングの制限は、アカウント × リージョン × API(操作)の単位で掛かる。EC2 は API ごと・リージョンごとのトークンバケット(Describe などの読み取りは容量 100・毎秒 20 の補充。フィルター・ページングなしの `DescribeInstances`・`DescribeSecurityGroups` などは容量 50・毎秒 10)、CloudTrail は `DescribeTrails`・`GetTrailStatus`・`GetEventSelectors` がリージョンごとに毎秒 10、STS はアカウント × リージョンで毎秒 600(出典を参照)
- **同時実行数と制限の関係**: 1 つの組は 1 つのリージョンの 1 つの種別で、その中の API の呼び出しは順に行う。同時に動く組は、別の種別(別の API)か別のリージョンなので、同時実行数を増やしても、1 つの API の 1 つのリージョンへの呼び出しの速さはほとんど増えない。1 つの組が順に呼ぶ速さ(1 回の呼び出しに数十〜数百ミリ秒)は、上の補充の速さに収まる
- **グローバルな IAM**: IAM のリクエストの制限は公開されていない。最初のルール群の IAM の組は 4 つで、同時実行数が 4 以上なら、いずれも同時に動く。同時実行数でこれを抑える効果は小さく、IAM の負荷は各コレクターの呼び出しの回数で決まる
- **所要時間**: 最初のルール群では、グローバルな組が 6、リージョンの組が 10 種別 × 有効なリージョン数(標準で有効なリージョンでおよそ 17)で、合わせて 180 前後の組になる。待ち時間の大半はネットワークの往復なので、同時実行数を増やすほど短くなるが、全体の時間は最も長い組(`aws.iam.policy` の `GetPolicyVersion` の 1,000 回を超える呼び出しなど)で下限が決まる。16 で、グローバルな組とリージョン 1 つ分の組をおおむね同時に動かせる
- **手元の資源**: 同時の接続数とメモリを、開発者の端末や CI のランナーで問題にならない程度に抑える
- 既定の 16 が合わない場合(低速な回線、プロキシ、制限の厳しい環境)は、利用者が `--concurrency` で下げる。値の上限は設けない(組の数を超える値は効果が無いだけ)。0 は CLI の引数の検査で終了コード 2 にする(`NonZeroUsize` で受ける)
- コレクターへの補足: EC2 の `DescribeInstances`・`DescribeSecurityGroups` などは、フィルターもページングも無い呼び出しだと小さいバケット(容量 50・毎秒 10)を使う。ページングのパラメーター(`MaxResults`)は「コレクターの方針」で付けてよいパラメーターなので、EC2 のコレクターの Issue でページングの形で呼ぶかを決める

### 再試行の回数: SDK の既定(standard、最大 3 回の試行)

- **方式**: SDK の standard の再試行を使う。AWS はすべての用途の既定として standard を推奨し、adaptive は単一のリソースに大量に呼ぶ用途向けで、汎用の既定には推奨していない。adaptive のクライアント側の速さの制限はクライアントのインスタンスごとで、この設計では組ごとにクライアントを作るため、効果も小さい
- **回数**: 最大 3 回の試行(初回 + 再試行 2 回)。スロットリングでは、指数的な待ち(基準 1 秒、ジッター付き)で再試行する。上の「同時実行数と制限の関係」のとおり、1 つの API への速さは制限に収まる見込みで、スロットリングは一時的な集中に限られるため、2 回の再試行で吸収できる
- **再試行の上限の仕組み**: standard には再試行の割当て(トークンバケット。クライアントのインスタンスごと)があり、失敗が続くと再試行を止めて早く失敗する。再試行の嵐にならない
- **コードで設定しない理由**: SDK の設定の優先度は「コードの設定 > 環境変数 > 共有の `config` ファイル > SDK の既定」で、コードで `retry_config` を設定すると、利用者の `AWS_MAX_ATTEMPTS`・`AWS_RETRY_MODE`・プロファイルの `max_attempts`・`retry_mode` が効かなくなる。コードで設定せず、利用者がこれらの AWS の標準の設定で回数・方式を変えられるようにする。`yata` に再試行のオプションは設けない
- **尽きた場合**: 再試行を尽くしたスロットリングは、組の `CollectionError`(`throttled`)か、リソースの失敗の印になり、ルールは `skipped` / `incomplete` になる。黙って `pass` にはならない
- **見直しの条件**: 呼び出しの回数の多いコレクター(`aws.iam.policy` など)の Issue で、スロットリングによる失敗が問題になると分かった場合は、その時点で回数・方式を見直す(コードで既定を変える場合も、利用者の環境変数・プロファイルの設定を優先する方法を合わせて決める)
- 補足: AWS の SDK の文書の再試行の数値(待ちの基準、割当ての費用)は、新しい再試行の動作(`AWS_NEW_RETRIES_2026=true` で有効にする)のもので、既定の動作では待ちの時間などが一部違う。どちらでも上の判断は変わらない

## 収集結果の型(`yata-core`)

[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の型を `yata-core` に置きます。1 つの型(と関連する小さな型)を 1 ファイルにします。

| ファイル | 型 | serde |
| --- | --- | --- |
| `scope.rs` | `Scope`(`Global` / `Regional`。`global` / `regional`) | あり |
| `resource.rs` | `Resource`(`id`、`resource_type`(JSON では `type`)、`account`、`region`、`name: Option<String>`、`tags: BTreeMap<String, String>`、`configuration: serde_json::Map<String, Value>`) | あり。`name` は `None` のときキーを省く(`skip_serializing_if`)。`tags` は空でも `{}` として常に出す |
| `resource_set.rs` | `ResourceSet`(`account`、`region`、`resource_type`、`resources`) | あり |
| `account.rs` | `AccountInfo`(`account`、`id`、`provider`) | あり |
| `resource_type_info.rs` | `ResourceTypeInfo`(`resource_type`(JSON では `type`)、`scope`) | あり |
| `collection_error.rs` | `CollectionError`(`account`、`region`、`resource_type`、`kind`、`message`)、`CollectionErrorKind`(`access_denied` / `throttled` / `region_unavailable` / `timeout` / `service` / `invalid_response`。`#[non_exhaustive]`) | あり |
| `inventory.rs` | `Inventory`(`accounts`、`regions`、`resource_types`、`resource_sets`、`collection_errors`、`started_at` / `finished_at: std::time::SystemTime`) | なし |

- serde の形は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の JSON の例に合わせる(`aws.account.account` の例に `name` が無く、`tags` が `{}` であること)。Rust の予約語 `type` に当たる項目は、フィールド名を `resource_type` にし、`#[serde(rename = "type")]` で JSON のキーを `type` にする
- `Inventory` は、スナップショットのマニフェストとは形が違い、そのままシリアライズしないため、serde を付けない。日時は `SystemTime` で持ち、RFC 3339 の文字列への整形はスナップショット・出力の Issue で行う(日時のクレートの選定を待たずに済むため)。スナップショットの読み込み(`yata-core`)では RFC 3339 の文字列を `SystemTime` に戻す処理も要るため、日時のクレートを決める Issue で、整形と解析の両方を扱う
- `Inventory` には、上の「収集の流れ」の手順 7 のとおり、`aws.account.account` の `AccountInfo`・`ResourceTypeInfo`・`ResourceSet` を常に入れる
- `Inventory` の各配列の順序は、出力を再現できるように決める: `regions` は昇順、`resource_types` は種別名の昇順、`resource_sets` と `collection_errors` は(アカウント、リージョン、種別)の昇順
- 収集の対象の組の各組は、`resource_sets` と `collection_errors` のどちらか一方にだけ現れる([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「Inventory」)。テストで確かめる
- `yata-core/src/lib.rs` には、これらのモジュールの宣言と再エクスポートを足す
- これらの型と、`yata-core/Cargo.toml` への `serde`・`serde_json` の追加は、この Issue の実装の PR に含める。共通部分の変更は機能の PR と分けて先に main へ入れるのが原則だが、分けると `Cargo.toml`・`Cargo.lock` の変更が 2 回になり、並列のブランチとの衝突の機会が増えるため、変更を 1 回にすることを優先する。型は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) で項目まで決まっており、この PR で形を新たに決める部分は少ない

## 構成

モジュールと型の関係は [モジュールの構成図](../diagrams/Diagram-0007-Provider-Module-Structure.md) を参照してください。

```
crates/yata-provider-aws/
├── Cargo.toml
├── build.rs                       # build/discover.rs を呼ぶだけ
├── build/
│   └── discover.rs                # 検出と生成(標準ライブラリのみ。テストからも読む)
├── src/
│   ├── lib.rs                     # 公開 API(supported_resource_types、setup_permissions、collect、型)
│   ├── collector.rs               # Collector、CollectFuture
│   ├── collectors.rs              # 生成物を include! するだけ
│   ├── collectors/                # <service>/<resource>.rs(この Issue では .gitkeep のみ)
│   ├── context.rs                 # CollectContext
│   ├── error.rs                   # ProviderError、SetupError
│   ├── setup.rs                   # 設定の読み込み、GetCallerIdentity、DescribeRegions、aws.account.account
│   ├── runner.rs                  # 組の展開、セマフォ、JoinSet、中断、Inventory の組み立て
│   ├── support.rs                 # support のモジュールの宣言
│   └── support/
│       ├── clients.rs             # ClientFactory、GuardedClient とその実装(9 クレート)
│       ├── guard.rs               # ReadOnlyGuard(Intercept)、判定の関数、ReadOnlyViolation
│       ├── guard_lists.rs         # 拒否リスト・例外リスト(理由付き)
│       ├── errors.rs              # CollectError、エラーの種類の判定、failure_mark
│       └── partition.rs           # パーティションの既定リージョン(us-east-1)、ARN からのパーティションの取り出し
└── tests/
    ├── source_scan.rs             # クライアントの直接生成のソースの検査
    └── discover.rs                # build/discover.rs の規則の検査

crates/yata-core/src/
├── lib.rs                         # モジュールの宣言と再エクスポート
├── scope.rs
├── resource.rs
├── resource_set.rs
├── account.rs
├── resource_type_info.rs
├── collection_error.rs
└── inventory.rs
```

- 並列のコレクターの PR が触るのは `src/collectors/<service>/<resource>.rs` と各自の機能設計書だけで、一覧・`mod` 宣言・`Cargo.toml`・`Cargo.lock` に追記しない
- 複数のコレクターで共有する処理を `support/` に足す場合は、`support.rs` にモジュールの宣言を 1 行足す。並列の PR で隣り合う行の衝突が起こりうるが、頻度は低い。1 つのコレクターだけが使う処理は、そのコレクターのファイルに置く([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「構成」と同じ)
- `yata-core` は他の Issue(ポリシーエンジンなど)も型を足すため、`yata-core/src/lib.rs` は共有されやすい。収集結果の型はこの Issue の実装の PR で入れる(上の「収集結果の型」)。他の Issue は、その PR が main に入った後に取り込んでから型を足す

## 依存の追加

[最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「依存の入れ方」のとおり、この Issue の実装の PR で `Cargo.toml` と `Cargo.lock` を 1 回で変更し、コレクターの PR(#12〜#17)は変更しません。

### ルートの `[workspace.dependencies]` に追加する

| クレート | バージョン | ライセンス | 用途 |
| --- | --- | --- | --- |
| `aws-sdk-iam` | `1.128.0` | Apache-2.0 | IAM の 4 種別 |
| `aws-sdk-s3` | `1.152.0` | Apache-2.0 | S3 のバケット |
| `aws-sdk-s3control` | `1.133.0` | Apache-2.0 | アカウントのブロックパブリックアクセス |
| `aws-sdk-cloudtrail` | `1.125.0` | Apache-2.0 | 証跡 |
| `aws-sdk-cloudwatchlogs` | `1.156.0` | Apache-2.0 | メトリクスフィルター |
| `aws-sdk-cloudwatch` | `1.134.0` | Apache-2.0 | アラーム |
| `aws-sdk-sns` | `1.116.0` | Apache-2.0 | トピックと購読 |

- 選定理由は [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「追加する SDK のクレート」のとおり(AWS 公式の SDK)
- **バージョンの決め方**: 既にある `aws-sdk-sts = "1.119.0"`・`aws-sdk-ec2 = "1.267.0"` と同じ AWS SDK for Rust のリリース(crates.io への公開が同じ日のもの)の版にそろえる。上の表は、crates.io で `aws-sdk-sts 1.119.0` と同じ日に公開された各クレートの版で、いずれも `rust-version` が `1.94.1`(ワークスペースの `rust-version` と同じ)
  - 同じリリースの版は、共通の `aws-smithy-*`・`aws-types` などの版の要求がそろうため、`Cargo.lock` で同じ版にまとまる
  - 実装の時点で新しいリリースが出ていても、この PR では上の版を使う。既存の `aws-config`・`aws-sdk-sts`・`aws-sdk-ec2` を上げない(無関係な依存の更新を機能の PR に混ぜないため)。SDK をまとめて上げるのは別の PR とする
  - 実装の時点でこれらの版が取り下げ(yank)られていた場合は、同じリリースの系列の次の版を選び、PR に理由を書く
- 既定の機能(`default-features`)は変えない([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「依存を追加する PR でビルド時間と合わせて判断する」について、この PR では既定のままとし、ビルド時間が問題になった時点で別の PR で見直す)

### `crates/yata-provider-aws/Cargo.toml`

```toml
[dependencies]
yata-core.workspace = true
aws-config.workspace = true
aws-sdk-sts.workspace = true
aws-sdk-ec2.workspace = true
aws-sdk-iam.workspace = true
aws-sdk-s3.workspace = true
aws-sdk-s3control.workspace = true
aws-sdk-cloudtrail.workspace = true
aws-sdk-cloudwatchlogs.workspace = true
aws-sdk-cloudwatch.workspace = true
aws-sdk-sns.workspace = true
aws-smithy-runtime-api = { workspace = true, features = ["client"] }
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true

[dev-dependencies]
aws-sdk-sts = { workspace = true, features = ["test-util"] }
aws-sdk-ec2 = { workspace = true, features = ["test-util"] }
aws-sdk-iam = { workspace = true, features = ["test-util"] }
aws-sdk-s3 = { workspace = true, features = ["test-util"] }
aws-sdk-s3control = { workspace = true, features = ["test-util"] }
aws-sdk-cloudtrail = { workspace = true, features = ["test-util"] }
aws-sdk-cloudwatchlogs = { workspace = true, features = ["test-util"] }
aws-sdk-cloudwatch = { workspace = true, features = ["test-util"] }
aws-sdk-sns = { workspace = true, features = ["test-util"] }
aws-smithy-mocks.workspace = true
tempfile.workspace = true
tokio = { workspace = true, features = ["time", "test-util"] }
```

- テストの偽のコレクターの遅延(`tokio::time::sleep`)のため、`[dev-dependencies]` で `tokio` の `time` を明示する。ワークスペースの `tokio` の機能(`rt-multi-thread`、`macros`、`sync`、`signal`)に `time` は無く、SDK の依存から機能が統合されることに頼らない。`test-util` は、遅延のテストで時間を進める(`tokio::time::pause`)ため。どちらの機能も新しいクレートを引き込まないため、`Cargo.lock` は変わらない

- `aws-smithy-runtime-api` はワークスペースに既にあり(`1.19.0`)、`client` 機能は既定で無効なため、このクレートで有効にする(`Metadata` と `Intercept` が `client` 機能の下にある)。ルートの `Cargo.toml` の変更は要らない
- `ConfigBag` などは SDK のクレートの再エクスポートを使うため、`aws-smithy-types` は直接の依存に加えない
- まだコードから使わない SDK のクレートを先に入れるが、使っていない依存は既定では警告にならない([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「依存の入れ方」)。この Issue では 9 つすべてを `support/clients.rs` の `GuardedClient` の実装とガードのテストで使う

### `crates/yata-core/Cargo.toml`

```toml
[dependencies]
serde.workspace = true
serde_json.workspace = true
```

- `Resource.configuration`(`serde_json::Map`)と、型の serde の実装のため。どちらもワークスペースに既にある

### `Cargo.lock`

- 上の変更で、AWS SDK のクレートとその依存、`serde`・`serde_json`・`thiserror`・`tokio`・`tracing`・`tempfile` などが加わる。この PR で 1 回だけ更新する

## 使う AWS API と IAM 権限

基盤が呼ぶ API は次の 2 つだけです。どちらも読み取りの操作で、ガードの接頭辞の規則で許可されます。

| API | 呼ぶとき | IAM 権限 |
| --- | --- | --- |
| `sts:GetCallerIdentity` | 毎回(最初の API のリージョンに 1 回) | 不要(`GetCallerIdentity` は、明示的に拒否しても呼べる操作。権限の一覧では `sts:GetCallerIdentity` として示す) |
| `ec2:DescribeRegions` | `--region` を指定しないとき(1 回) | `ec2:DescribeRegions` |

- `setup_permissions()` はこの 2 つを返し、`supported_resource_types()` の `aws.account.account` の `required_permissions` は `["sts:GetCallerIdentity"]` とする([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の「必要な IAM 権限」)
- 例外リストの `iam:GenerateCredentialReport` は、認証情報レポートのコレクターの Issue で使う。この Issue では呼ばない

## テスト方針

[テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) に従い、実 AWS には接続しません。SDK の呼び出しは `aws-smithy-mocks`(`MockResponseInterceptor` とモックの HTTP クライアント)でモックし、認証情報はダミーの静的な値、アカウント ID は `123456789012` などのダミーだけを使います。

テストでクライアントを作るときも本番と同じ `ClientFactory` と `GuardedClient` を通します。`ClientFactory` に、テストのときだけ追加のインターセプター(モック)と、モックの HTTP クライアント・ダミーの認証情報・再試行なし(`RetryConfig::disabled()`)の `SdkConfig` を渡せる作り方(`#[cfg(test)]`)を用意します。このため、ガードと基盤のテストは主にクレート内の単体テスト(`#[cfg(test)] mod tests`)に置きます。

### 読み取り専用ガード

- **判定の関数**(表のテスト): `Describe` / `List` / `Get` で始まり直後が英大文字の操作は許可、`Create` / `Put` / `Delete` / `Update` / `Start` などは拒否、`Getaway` のように直後が英大文字でないものは拒否、拒否リストの 9 項目はすべて拒否(`Get` で始まっても)、`IAM` / `GenerateCredentialReport` は許可、`IAM` / `GenerateServiceLastAccessedDetails` と、`IAM` 以外のサービス名の `GenerateCredentialReport` は拒否、メタデータが無い場合は拒否
- **実際のクライアントでの確認**: 9 つの SDK のクライアントそれぞれを `GuardedClient` で作り、モックを付けて次を確かめる
  - 読み取りの操作 1 つ(例: STS `GetCallerIdentity`、EC2 `DescribeRegions`、IAM `ListUsers`、S3 `ListBuckets`、S3 Control `GetPublicAccessBlock`、CloudTrail `DescribeTrails`、CloudWatch Logs `DescribeMetricFilters`、CloudWatch `DescribeAlarms`、SNS `ListTopics`)がモックの応答で成功する
  - 書き込みの操作 1 つ(例: STS `GetSessionToken`(拒否リスト)、EC2 `CreateTags`、IAM `CreateUser`、S3 `PutBucketPolicy`、S3 Control `PutPublicAccessBlock`、CloudTrail `StopLogging`、CloudWatch Logs `DeleteLogGroup`、CloudWatch `PutMetricAlarm`、SNS `Publish`)が失敗し、エラーの原因に `ReadOnlyViolation` があり、その操作が送信の段階に進んでいないこと。送信されないことは、`read_before_transmit` で操作名を記録するテスト用のインターセプターを付け、拒否した操作が記録に無いことで確かめる(`aws-smithy-mocks` のルールの `num_calls()` は、ガードより前のフックで数えられるため、この確認には使えない)。許可した操作は記録にあることも確かめる
  - S3 `GetObject`(拒否リスト)が同様に拒否される
  - IAM `GenerateCredentialReport` が許可され、`GenerateServiceLastAccessedDetails` が拒否される
  - これにより、上の「サービス名の文字列」の表の値が SDK の実際の値と合っていることも確かめる(SDK の更新で値が変わればこのテストが失敗する)
- **リストの整合**: 拒否リスト・例外リストの各項目の IAM アクション名が `<小文字のサービスの接頭辞>:<操作名>` の形で、操作名の部分が SDK の操作名と同じであること。2 つのリストに同じ項目が無いこと。各項目に理由が書かれていること

### コレクターの一覧と権限

コレクターがまだ無いため、一覧が空でも成り立ち、コレクターが足されたら自動的に対象になるテストにします。テストの中では、テスト用の偽のコレクター(テストのコードの中で定義する)でも同じ検査の関数を確かめます。

- **`required_permissions` の形**: `supported_resource_types()` のすべての要素(`aws.account.account` を含む)について、各権限が `<service>:(Describe|List|Get)<英大文字>...` の形か、例外リストの IAM アクション名であること。拒否リストの IAM アクション名を含まないこと。空でないこと
- **種別名の規則**: 3 階層、最初が `aws`、各部分が `[a-z][a-z0-9_]*` で Rego の予約語でないこと。重複が無いこと
- **パスとの一致**: `all_collectors()` の各コレクターの `resource_type()` が `COLLECTOR_PATH_TYPES` の同じ位置の値と一致すること。`aws.account.account` を作るコレクターが無いこと
- **ソースの検査**(`tests/source_scan.rs`): `src/` の下の `.rs` ファイルを文字列で調べ、`support/clients.rs` 以外に `Client::new(` と `Client::from_conf(` が無いこと。`src/collectors/` の下に `aws_config::` が無いこと。文字列の検査は、型の別名(`use aws_sdk_s3::Client as S3;` からの `S3::from_conf(` など)では回避できるため、コードレビューで補う
- **検出の規則**(`tests/discover.rs`): 一時ディレクトリに配置を作り、`build/discover.rs` の関数で、正しい配置の生成結果(順序、パスの書き出し、`COLLECTOR_PATH_TYPES`)と、誤った配置(直下の `.rs`、3 階層目のディレクトリ、`mod.rs`、大文字・`-` を含む名前、`_` を含むサービス名、予約語)のエラーと、ディレクトリが無い場合に空の一覧になること、`.rs` 以外のファイル(場所・名前を問わない)が無視されることを確かめる

### 準備(アカウントとリージョン)

- `GetCallerIdentity` のモックの応答にダミーの `Account`・`Arn`・`UserId` を入れ、`aws.account.account` の `configuration` が `{"GetCallerIdentity": {"Account": "123456789012"}}` だけで、`Arn`・`UserId` を含まないこと。`id`・`region`・`account`・`tags` が [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の例どおりであること
- `Arn` のパーティションが `aws-cn` のとき、`Account` が 12 桁でないとき、`GetCallerIdentity` が `AccessDenied` などで失敗したときに `SetupError` になること
- `DescribeRegions` を既定のパラメーターで呼ぶこと(モックのリクエストの照合で `AllRegions`・`Filters`・`RegionNames` が無いこと)。順不同の応答から昇順の対象リージョンになること。失敗で `SetupError` になり、メッセージに `--region` があること
- `regions` を指定したときに `DescribeRegions` が呼ばれないこと。重複が 1 つにまとまり、昇順になること。形式の誤ったリージョン名で `InvalidRegion` になること
- 対応していない種別で `UnsupportedResourceType` になること

### 並行実行と中断

テスト用の偽のコレクター(遅延、失敗、件数、パニック、終わらないものを設定できる)を使います。

- 同時実行数の上限: `concurrency` が 2 のとき、同時に動くコレクターの数の最大が 2 を超えないこと(原子的なカウンターで測る)
- 組の展開: `global` の種別は 1 組、`regional` の種別は対象リージョンごとに 1 組になり、`regions`・`resource_types` が昇順になること
- 1 つの組の失敗が `CollectionError` になり、ほかの組は `ResourceSet` になること。0 件でも `ResourceSet` ができること。各組が `resource_sets` と `collection_errors` のどちらか一方にだけあること(`aws.account.account` の組も対象にする)
- `aws.account.account` を要求に含めない場合も、`accounts` に `AccountInfo` が 1 件、`resource_types` に `aws.account.account`(`global`)、`resource_sets` にリソース 1 件の `ResourceSet` が入ること
- 種別の重複が 1 つにまとまること。`regions` が空の配列のときに `InvalidRegion` になること
- 実行を始める順が手順 5 の順になること(同時実行数 1 で、偽のコレクターが記録する開始の順で確かめる)
- リソースが `id` の昇順に並ぶこと。種別・アカウントの違うリソース、または同じ `id` のリソースを返したコレクターの組が `invalid_response` になること。パニックしたコレクターの組が `service` になり、ほかの組は続くこと
- 中断: 終わらないコレクターを実行中に `cancel` を完了させると、すぐに `Interrupted` が返り、実行中のタスクが中止されること(偽のコレクターが持つ値の `Drop` で確かめる)

### エラーの種類の判定

- 判定の関数の表のテスト(上の「エラーの種類の判定」の各行)
- モックで `AccessDenied`・`ThrottlingException`・`OptInRequired` などのコードのエラーを返し、`CollectError::from_sdk` と `failure_mark` の種類・メッセージの形(サービスのメッセージの本文を含まないこと)を確かめる

### 認証情報とリージョンの解決

- `--profile` の有無で、`ConfigLoader` に渡す認証情報プロバイダとリージョンのプロバイダの組み立てが上の表のとおりになることを、組み立てを表す値を返す関数の単体テストで確かめる
- 一時ディレクトリの `config` ファイル(ダミーの静的な認証情報を持つプロファイル)を `ProfileFiles` で指定し、`ProfileFileCredentialsProvider` がそのプロファイルの認証情報を返すことを確かめる(既定の場所の `config` / `credentials` ファイルは読まない設定にする)
- 環境変数を書き換えるテストは置かない(edition 2024 では `std::env::set_var` が `unsafe` で、`unsafe` を禁止しているため。プロセス全体の状態を共有して並列のテストに影響するためでもある)。「`--profile` のときに環境変数の認証情報を使わない」ことは、既定のチェーンを使わずに認証情報プロバイダを明示する組み立てのテストで確かめる

## アーキテクチャとの差異

次の点は全体設計と食い違う、または全体設計の未決事項をこの設計で決めたものです。`docs/architecture/` の更新は別の Issue で行います。

| ページ | 全体設計の記述 | この設計 | 根拠 |
| --- | --- | --- | --- |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md) | 初期状態の例外リストは空(原則として空) | 例外リストを IAM / `GenerateCredentialReport` の 1 項目から始める | [ADR-0005](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) | `required_permissions` の形は `<service>:(Describe\|List\|Get)...` | 例外リストの IAM アクション名も許す | ADR-0005 |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md)、[ADR-0005](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) | 例外リスト・拒否リストの変更は、機能の PR に混ぜず単独の PR で行う(ADR-0005 の例外の追加も単独の PR) | 例外リストへの `GenerateCredentialReport` の追加を、ガードを作るこの Issue の PR で行う。ガード自体がこの PR で初めて作られ、最初の内容として入れるため([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の後続の Issue 3 の「ガードがまだ無ければ P1 に含める」) | 上の「目的と背景」 |
| [ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md) | `support/` に認証・リージョン・読み取り専用ガードなどを置く | 認証情報・リージョンの解決とアカウント・リージョンの決定は `src/setup.rs` に置き、`support/` には複数のコレクターが使うもの(クライアントの生成、ガード、エラー、パーティション)だけを置く | コレクターから使わない準備の処理を、コレクター向けの `support/` と分けるため |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、[テスト方針](../architecture/Architecture-Testing-Strategy.ja.md)、[出力形式](../architecture/Architecture-Output-Formats.ja.md) | 秘密に当たる項目を `"<redacted>"` に伏せ、伏せる一覧をコレクターのファイルに置き、テストで確かめる。出力の `actual` にも伏せた値が出る | 基盤は伏せる仕組み・ヘルパーを用意しない。コレクターは値を伏せないため、`actual` に機微な値が入りうる(外部に出すときは `--omit-values`) | プロジェクトの規約(コレクターは収集のみ、秘密の値も伏せない)、[最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「コレクターの方針」 |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) | ガードの実装方法とサービス名の文字列は未決 | `read_before_serialization` で `Metadata` を読む方式に決定。サービス名は `sdkId` の表記(上の表) | 上の「読み取り専用ガード」 |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) | `--profile` の認証情報の限定方法は未決 | `ProfileFileCredentialsProvider` を `ConfigLoader::credentials_provider` に渡す | 上の「認証情報とリージョンの解決」 |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) | `--concurrency` の既定値と再試行の回数は未決 | 既定値 16、再試行は SDK の既定(standard、最大 3 回の試行)を使い、コードで設定しない | 上の「並行実行」 |
| [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) | エラーの種類の判定方法は未決 | 上の「エラーの種類の判定」の表。ガードが止めた操作は `service` | 上の表 |
| [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) の「主なエラー」 | `yata-provider-aws` のエラーは `SetupError`(3)と `CollectError`(記録して続行) | 公開のエラーを `ProviderError` とし、`Setup(SetupError)`(3)に加えて、`UnsupportedResourceType`・`InvalidRegion`(2)と `Interrupted`(130)を持つ。`CollectError` はクレート内の型のまま | 上の「エラー時」。ライブラリの API として要求の誤りと中断を返すため |
| [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) の「AWS の環境変数との関係」 | 最初の API のリージョンは `AWS_REGION` → プロファイルの `region` → `us-east-1` | `AWS_REGION` の次に `AWS_DEFAULT_REGION` も見る | `aws-config` の `EnvironmentVariableRegionProvider` の動作。AWS CLI と同じ扱い |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) | `CollectContext` は SDK の設定を持つ | `SdkConfig` は `ClientFactory` の中に閉じ、コレクターには公開しない(ガード付きのクライアントの生成だけを公開) | クライアントの直接生成を防ぐため |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の「公開 API」 | 公開する関数は「対応する種別の一覧」と「収集」の 2 つ | 加えて `setup_permissions()`(`sts:GetCallerIdentity`、`ec2:DescribeRegions`)を公開する。収集の関数は中断のための `cancel` を受け取る | `yata permissions` が準備の権限を知るため。Ctrl-C をテストで確かめるため |
| [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「リソース種別」 | `<service>` は SDK のサービス名に合わせる | 加えて、コレクターの種別名はファイルのパスから決まる `aws.<service>.<resource>` と一致させ、`<service>` に `_` を含めない | モジュール名の重なりを防ぎ、種別の重複をファイルの配置で防ぐため |
| [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) | 読み取り専用のテストは `yata-provider-aws/tests/` に置く | ソースの検査は `tests/` に、ガードと権限の検査はクレート内の単体テストに置く | ガード・クライアントの生成を公開しないため |
| [ADR: コレクターのビルド時自動検出](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md) | 生成物はモジュール宣言と `all_collectors()` | 加えて `COLLECTOR_PATH_TYPES` を生成し、検出の処理を `build/discover.rs` に分ける | 上の「種別名とファイルのパスの対応」、検出の規則のテストのため |

## 出典

- 操作のメタデータ(`Metadata`。`name()`、`service()`、`Storable`): <https://docs.rs/aws-smithy-runtime-api/latest/aws_smithy_runtime_api/client/orchestrator/struct.Metadata.html>
- インターセプター(`Intercept`。`read_before_serialization` の呼ばれる時点とエラー時の動き): <https://docs.rs/aws-smithy-runtime-api/latest/aws_smithy_runtime_api/client/interceptors/trait.Intercept.html>
- オーケストレーターのフックと操作の設定の適用の順(`aws-smithy-runtime` 1.16.0 の `src/client/orchestrator.rs`): <https://github.com/smithy-lang/smithy-rs/blob/main/rust-runtime/aws-smithy-runtime/src/client/orchestrator.rs>
- `aws-smithy-runtime-api` の機能(`client` は既定で無効): <https://docs.rs/crate/aws-smithy-runtime-api/latest/features>
- SDK のクレートの `config` モジュールの再エクスポート(`Intercept`、`ConfigBag` など): <https://docs.rs/aws-sdk-sts/latest/aws_sdk_sts/config/index.html>
- 生成された SDK のコードの `Metadata::new` の値(サービス名の文字列。リポジトリ `awslabs/aws-sdk-rust` の `sdk/<service>/src/operation/<operation>.rs`): <https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/sts/src/operation/get_caller_identity.rs>、<https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/iam/src/operation/generate_credential_report.rs>、<https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/s3control/src/operation/get_public_access_block.rs>、<https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/cloudwatchlogs/src/operation/describe_metric_filters.rs>、<https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/secretsmanager/src/operation/get_secret_value.rs>(ほかの表の値も同じ場所で確かめた)
- `ConfigLoader`(`profile_name`、`credentials_provider`、上書きで既定のチェーンを使わないこと): <https://docs.rs/aws-config/latest/aws_config/struct.ConfigLoader.html>
- `ProfileFileCredentialsProvider` とその `Builder`(`profile_name`、扱えるプロファイルの設定): <https://docs.rs/aws-config/latest/aws_config/profile/credentials/struct.ProfileFileCredentialsProvider.html>、<https://docs.rs/aws-config/latest/aws_config/profile/credentials/struct.Builder.html>
- 既定の認証情報プロバイダチェーンの順: <https://docs.rs/aws-config/latest/aws_config/default_provider/credentials/struct.DefaultCredentialsChain.html>
- `ProviderConfig`: <https://docs.rs/aws-config/latest/aws_config/provider_config/struct.ProviderConfig.html>
- `EnvironmentVariableRegionProvider`(`AWS_REGION` → `AWS_DEFAULT_REGION`): <https://docs.rs/aws-config/latest/aws_config/environment/region/struct.EnvironmentVariableRegionProvider.html>
- AWS CLI の環境変数(`AWS_DEFAULT_REGION` と `AWS_REGION` の優先): <https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-envvars.html>
- SDK の再試行の動作(standard / adaptive、最大の試行の回数、待ち、再試行の割当て、スロットリングのコード、設定の優先度): <https://docs.aws.amazon.com/sdkref/latest/guide/feature-retry-behavior.html>
- `RetryConfig`(`standard()` は最大 3 回の試行): <https://docs.rs/aws-smithy-types/latest/aws_smithy_types/retry/struct.RetryConfig.html>
- EC2 の API のスロットリング(トークンバケット、読み取りの容量と補充、フィルター・ページングなしの呼び出しのバケット): <https://docs.aws.amazon.com/ec2/latest/devguide/ec2-api-throttling.html>
- EC2 のエラーコード(`UnauthorizedOperation`、`OptInRequired`、`RequestLimitExceeded`): <https://docs.aws.amazon.com/AWSEC2/latest/APIReference/errors-overview.html>
- CloudTrail の TPS の制限: <https://docs.aws.amazon.com/general/latest/gr/ct.html>
- STS のリクエストの制限(アカウント × リージョンで毎秒 600): <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html>
- `ec2:DescribeRegions`(既定では有効なリージョンを返す): <https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_DescribeRegions.html>
- `sts:GetCallerIdentity`: <https://docs.aws.amazon.com/STS/latest/APIReference/API_GetCallerIdentity.html>
- `aws-smithy-mocks`(`mock!`、`mock_client!`、`MockResponseInterceptor`、`RuleMode`): <https://docs.rs/aws-smithy-mocks/latest/aws_smithy_mocks/>、<https://docs.aws.amazon.com/sdk-for-rust/latest/dg/testing-smithy-mocks.html>
- SDK のクレートの版・公開日・`rust-version`・ライセンス(crates.io): <https://crates.io/crates/aws-sdk-iam>、<https://crates.io/crates/aws-sdk-s3>、<https://crates.io/crates/aws-sdk-s3control>、<https://crates.io/crates/aws-sdk-cloudtrail>、<https://crates.io/crates/aws-sdk-cloudwatchlogs>、<https://crates.io/crates/aws-sdk-cloudwatch>、<https://crates.io/crates/aws-sdk-sns>、<https://crates.io/crates/aws-sdk-sts>、<https://crates.io/crates/aws-smithy-mocks>
- Cargo のビルドスクリプト(`rerun-if-changed` にディレクトリを指定した場合): <https://doc.rust-lang.org/cargo/reference/build-scripts.html>
- `tokio::task::JoinSet`(破棄でタスクを中止): <https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| ガードが止めた操作のエラーの種類 | 現在の種類の一覧に当てはまるものが無いため `service` にした。専用の種類(例: `blocked`)を設けるか | 全体設計の更新の Issue |
| コレクターの中の並行の呼び出し | `aws.iam.policy` の `GetPolicyVersion`(1,000 回を超える)など、呼び出しの多いコレクターの中で並行に呼ぶか。呼ぶなら、同時実行数の上限との関係(タスクの許可を持ったまま追加の許可を待つと行き詰まるため、別の上限にするなど)を `CollectContext` に加える | 呼び出しの多いコレクターの Issue(IAM のコレクター) |
| 操作のタイムアウト | 応答の無い接続で組が終わらない場合に備え、操作ごとのタイムアウト(`timeout_config` の `operation_timeout`)を設けるか。設ける場合は値と、利用者の設定との優先度 | CLI の `scan` を実装する Issue、または問題が出た時点 |
| `access_denied` などのコードの過不足 | 上の「エラーの種類の判定」のコードの一覧が、各サービスのエラーのコードを網羅しているか。とくに、有効でないリージョンを指定した場合に返るコード(`InvalidClientTokenId`、`UnrecognizedClientException` など)を `region_unavailable` にするか | 各コレクターの Issue(#12〜#17) |
| EC2 の一覧の操作のページング | フィルター・ページングなしの `DescribeInstances`・`DescribeSecurityGroups` などは小さいバケットを使う。ページングの形(`MaxResults`)で呼ぶか | EC2 のコレクターの Issue(#17) |
