# AWS アクセス

> This Japanese version is the authoritative source. The English version (`Architecture-AWS-Access.md`) has not been written yet.

AWS への接続方法、認証情報の扱い、リージョン、コレクターの構成、読み取り専用の強制、必要な IAM 権限を定めます。SDK の選定と読み取り専用ガードの判断は [ADR: AWS SDK for Rust と読み取り専用ガード](../adr/ADR-0001-AWS-SDK-and-Read-Only-Guard.ja.md) を参照してください。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## SDK

- 公式の AWS SDK for Rust(`aws-config` と `aws-sdk-<service>`)を使う
- 非同期ランタイムは tokio(SDK の既定)
- `aws_config::defaults(BehaviorVersion::latest())` で設定を読み込む
- 再試行は SDK の標準の再試行(スロットリングと一時的なエラーを再試行する)を使う
- ページングのある API は SDK のページネーター(`into_paginator()`)で全件を取得する

## 認証情報

- 認証情報は SDK の既定の認証情報プロバイダチェーンに任せる。Yata Cloud は認証情報のファイルや環境変数を自分で読まない
  - チェーンの順序: 環境変数 → 共有の `config` / `credentials` ファイル(`AWS_PROFILE`)→ Web ID → ECS / EKS のコンテナ認証情報 → EC2 インスタンスメタデータ(IMDSv2)
- `--profile <NAME>` を指定したときだけ、そのプロファイルを使うよう SDK に渡す。このときは、そのプロファイルの認証情報だけを使い、環境変数の認証情報と `AWS_PROFILE` は使わない(オプション > 環境変数の規則。[CLI インターフェース](Architecture-CLI-Interface.ja.md) の「AWS の環境変数との関係」)
- 最初の API を呼ぶリージョンは、`--profile` を指定した場合は `AWS_REGION` → プロファイルの `region` → `us-east-1` の順(AWS CLI と同じ)、指定しない場合は SDK の既定の解決 → `us-east-1` で決める。スキャンの対象リージョンには使わない([CLI インターフェース](Architecture-CLI-Interface.ja.md) の「AWS の環境変数との関係」)
- ロールの引き受け(AssumeRole)は、共有 `config` ファイルのプロファイル設定(`role_arn`、`source_profile`)で行う。CLI に独自の AssumeRole のオプションは設けない
- 認証情報の値をログ・出力・エラーメッセージに含めない
- 認証情報が見つからない、または `sts:GetCallerIdentity` が失敗した場合は、収集を始めずに終了する(終了コード 3)

### 認証情報の解決で SDK が内部で呼ぶ操作

- 認証情報プロバイダチェーンは、設定に応じて SDK の内部で `sts:AssumeRole`、`sts:AssumeRoleWithWebIdentity`、`sso:GetRoleCredentials` などを呼ぶことがある
- これらは `aws-config` が内部で作るクライアントから呼ばれ、Yata Cloud が作るサービスクライアントを通らないため、読み取り専用ガードの対象外となる
- これらは一時的な認証情報を得るための操作で、利用者の AWS リソースを作成・変更・削除しない。利用者が自分の設定(プロファイル、環境変数)で選んだ認証方法を実行するものであるため、読み取り専用の規約(「リソースを作成・変更・削除する API を呼ばない」)に反しないものとして許容する
- Yata Cloud のコードから、これらの操作を直接呼ぶことはしない

## アカウントとリージョン

1. `sts:GetCallerIdentity` でアカウント ID とパーティション(ARN の第 2 要素)を得て、アカウントキー(`aws-<account_id>`)と `aws.account.account` リソースを作る。パーティションが `aws` でなければ `SetupError` として終了する(終了コード 3。[リソースモデル](Architecture-Resource-Model.ja.md) の「パーティション」)
2. 対象リージョンを決める
   - `--region <REGION>` が指定されていれば、そのリージョンだけ(複数指定可)
   - 指定が無ければ、`ec2:DescribeRegions` で得た、アカウントで有効なリージョンすべて
   - `ec2:DescribeRegions` が失敗した場合(権限不足を含む)は、`SetupError` として収集を始めずに終了する(終了コード 3)。エラーメッセージで `--region` の指定を案内する
3. グローバルなサービス(IAM など)の API は、パーティションの既定リージョン(`aws` パーティションでは `us-east-1`)に対して 1 回だけ呼ぶ

対応するパーティションは `aws` のみです(未決事項を参照)。

## コレクター

1 つのリソース種別を収集する処理を「コレクター」と呼び、1 リソース種別 1 ファイルで `crates/yata-provider-aws/src/collectors/<service>/<resource>.rs` に置きます。build.rs がファイルを見つけてモジュール宣言と一覧を生成するので、一覧ファイルへの追記は不要です([ADR: コレクターのビルド時自動検出](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md))。

各ファイルは、次の関数を 1 つ公開します(crate 内公開)。

```rust
/// Returns the collector defined in this file.
pub(crate) fn collector() -> Box<dyn Collector>;
```

`Collector` トレイトの概略です(名前・シグネチャの細部は実装時に決めてよい)。

```rust
pub(crate) trait Collector: Send + Sync {
    /// Resource type produced by this collector, e.g. "aws.s3.bucket".
    fn resource_type(&self) -> &'static str;
    /// Whether the resource is global or regional.
    fn scope(&self) -> Scope;
    /// IAM actions required by this collector, e.g. ["s3:ListAllMyBuckets"].
    fn required_permissions(&self) -> &'static [&'static str];
    /// Collects resources in the given region.
    fn collect<'a>(&'a self, ctx: &'a CollectContext, region: &'a str) -> CollectFuture<'a>;
}
```

- `CollectFuture<'a>` は `Pin<Box<dyn Future<Output = Result<Vec<Resource>, CollectError>> + Send + 'a>>` の別名とする(`async-trait` クレートを使わない)
- `CollectContext` は SDK の設定、アカウント情報、読み取り専用ガード付きのクライアントを作る手段を持つ
- `Scope::Global` のコレクターは 1 回、`Scope::Regional` は対象リージョンごとに実行する
- 1 つのリソースを作るのに複数の API が必要な場合(例: バケット一覧 → バケットごとの設定)、それらは同じコレクターの中で呼ぶ。結果は構成情報(`configuration`)に API の操作名ごとに入れる([リソースモデル](Architecture-Resource-Model.ja.md) の「構成情報」)
- SDK の出力型から構成情報への変換と、秘密に当たる項目を伏せる一覧は、コレクターのファイルに置く([リソースモデル](Architecture-Resource-Model.ja.md) の「秘密の値の扱い」)
- 同じサービスの複数のコレクターが共通の処理を使う場合は、`crates/yata-provider-aws/src/support/` に置く(build.rs の検出対象外)

### 実行

- `yata-cli` が必要なリソース種別の一覧を渡し、`yata-provider-aws` が対応するコレクターを選んで実行する
- アカウント × リージョン × 種別の組を tokio のタスクとして並行に実行し、同時実行数をセマフォで制限する(`--concurrency`)
- 成功した組は `ResourceSet` にする(0 件でも作る)。1 つの組が失敗しても、ほかの組は続行し、失敗は `CollectionError` として記録する([エラー処理とログ](Architecture-Error-Handling-and-Logging.ja.md))
- Ctrl-C を受けたら実行中のタスクを中止し、結果を出力せずに終了する(終了コード 130)
- `yata-provider-aws` は収集結果を `Inventory`(`ResourceSet` の集まりと `CollectionError` の記録)として `yata-cli` に返す。評価には関与しない

処理順の全体は [yata scan の処理順](../diagrams/Diagram-0001-Scan-Sequence.md) を参照してください。

### 公開 API

`yata-provider-aws` はコレクターそのもの(`Collector` トレイト)を公開せず、次の `pub` 関数を公開します(名前は実装時に決めてよい)。

| 関数 | 内容 |
| --- | --- |
| 対応する種別の一覧 | `yata-provider-aws` が作れるリソース種別と、それぞれのスコープ(`global` / `regional`)と `required_permissions` を返す。`aws.account.account`(`sts:GetCallerIdentity`)も含める |
| 収集 | 必要なリソース種別の一覧と、プロファイル・リージョン・同時実行数の設定を受け取り、`Inventory` を返す |

- ルールとコレクターの対応の検査は、この一覧を使って `yata-cli` が行う(`yata-engine` は AWS を知らないため)。詳細は [ポリシー評価](Architecture-Policy-Evaluation.ja.md) を参照
- `yata permissions` も、この一覧から必要な権限を集める

## 読み取り専用の強制

次の 3 段で、Describe / List / Get 系以外の API と、秘密の値の取得・認証情報の発行を行う API を呼ばないようにします。

1. **実行時のガード**: すべてのサービスクライアントに、SDK のインターセプターとして読み取り専用ガードを付ける。ガードはリクエストの送信前に、呼び出す操作名(SDK が設定に入れる操作のメタデータ)とサービス名を調べ、次の操作をエラーにして送信させない
   - `Describe`、`List`、`Get` のいずれでも始まらない操作(例外リストにあるものを除く)
   - 拒否リストにある操作。`Get` などで始まっても、秘密の値を取得するもの・認証情報を発行するもの・データの中身を読むものを載せる
   - 拒否リストは SDK の操作の名前(サービス + 操作名。ガードが操作のメタデータから得る値)で書く。テストで IAM アクション名と比べるため、各項目には対応する IAM アクション名も併記する。初期の拒否リストは次のとおり(サービス / 操作名 → IAM アクション名)
     - Secrets Manager / `GetSecretValue` → `secretsmanager:GetSecretValue`
     - SSM / `GetParameter` → `ssm:GetParameter`
     - SSM / `GetParameters` → `ssm:GetParameters`
     - SSM / `GetParametersByPath` → `ssm:GetParametersByPath`
     - SSM / `GetParameterHistory` → `ssm:GetParameterHistory`(SSM の 4 つは SecureString の値を取得しうる)
     - STS / `GetSessionToken` → `sts:GetSessionToken`
     - STS / `GetFederationToken` → `sts:GetFederationToken`
     - ECR / `GetAuthorizationToken` → `ecr:GetAuthorizationToken`
     - S3 / `GetObject` → `s3:GetObject`(バケット内のデータの読み取り)
   - ガードが比べるサービス名の正確な文字列(操作のメタデータの `service` の値)は、ガードの実装方法の確認(未決事項)とあわせて実装時に確かめる
   - 判定は「拒否リスト → 例外リスト → 接頭辞(Describe / List / Get)」の順に行う。拒否リストにある操作は例外リストより優先して拒否する
   - クライアントは `yata-provider-aws/src/support/` の生成用マクロ(またはヘルパー)でだけ作る。コレクターで `Client::new` / `Client::from_conf` を直接呼ばない
   - 例外リスト・拒否リストはどちらも `support/` 内に理由とともに書く。初期状態の例外リストは空
   - この 2 つのリストは中央の列挙だが、追加の頻度が低い共有箇所として扱う([ワークスペース構成](Architecture-Workspace-Layout.ja.md))。変更は機能の PR に混ぜず、メンテナーの判断を経た単独の PR で行う
2. **テスト**:
   - すべてのコレクターの `required_permissions` が `<service>:(Describe|List|Get)...` の形で、拒否リストの操作を含まないことを確かめる。`required_permissions` は IAM アクション名なので、拒否リストの各項目に併記した IAM アクション名と比べる(SDK の操作名とは直接比べない)
   - コレクターのソースに `Client::new` / `Client::from_conf` が無いことを確かめる(ソースを文字列で検査するテスト)
   - ガードが書き込み系の操作名と、拒否リストの操作を拒否することを確かめる
3. **コードレビュー**: SDK の呼び出しが読み取り専用で、拒否リストに相当する操作(秘密の値の取得、認証情報の発行、データの中身の読み取り)を新たに使っていないかを確認する。該当する操作を見つけたら拒否リストへの追加を検討する

認証情報の解決で SDK が内部で呼ぶ操作は、このガードの対象外です(上の「認証情報の解決で SDK が内部で呼ぶ操作」を参照)。

## 必要な IAM 権限

- 常に必要: `sts:GetCallerIdentity`、`ec2:DescribeRegions`(`--region` を指定しない場合)
- そのほかは、実行するコレクターの `required_permissions` の和集合
- `yata permissions` コマンドが、選んだルールに必要な権限を IAM ポリシーの JSON として出力する。権限の一覧を手で保守する文書は作らない
- 利用者向けの案内では、最小権限として `yata permissions` の出力を使う方法と、AWS 管理ポリシー `SecurityAudit` を付ける方法を示す(利用者ガイドを作る Issue で書く)
- 権限不足(`AccessDenied` など)の扱いは [エラー処理とログ](Architecture-Error-Handling-and-Logging.ja.md) を参照

## 出典

- 認証情報プロバイダチェーン: <https://docs.aws.amazon.com/sdk-for-rust/latest/dg/credproviders.html>
- `aws-smithy-mocks` によるテスト: <https://docs.aws.amazon.com/sdk-for-rust/latest/dg/testing-smithy-mocks.html>
- 操作のメタデータ(`Metadata`): <https://docs.rs/aws-smithy-runtime-api/latest/aws_smithy_runtime_api/client/orchestrator/struct.Metadata.html>
- `sts:GetCallerIdentity`: <https://docs.aws.amazon.com/STS/latest/APIReference/API_GetCallerIdentity.html>
- `ec2:DescribeRegions`: <https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_DescribeRegions.html>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| Describe / List / Get 以外の読み取り操作 | CIS の IAM 関連のチェックで使われる認証情報レポートは、`iam:GetCredentialReport` の前に `iam:GenerateCredentialReport` でレポートを生成する必要がある。`Generate` はリソースを変更しないが、名前の規則(Describe / List / Get)からは外れる。例外として許すか、別の API で代替するか | 認証情報レポートを使うルールの Issue(メンテナーの判断が必要) |
| `aws` 以外のパーティション | GovCloud、中国リージョンへの対応。当面はパーティションが `aws` でなければ終了コード 3 | 需要が出た時点で別 Issue |
| `--profile` の認証情報の限定 | `--profile` のとき、環境変数の認証情報を使わずにプロファイルの認証情報だけを使う方法の確認([CLI インターフェース](Architecture-CLI-Interface.ja.md) の未決事項) | AWS プロバイダを実装する Issue |
| 複数アカウントのスキャン | AWS Organizations の配下のアカウントを順に引き受けてスキャンするか | 別 Issue |
| `--concurrency` の既定値と再試行の回数 | スロットリングを避けつつ速く収集できる値 | AWS プロバイダを実装する Issue |
| ガードの実装方法の確認 | インターセプターから操作名を取得する方法が SDK の公開 API で実現できるかの確認。実現できない場合は、クライアントの生成をラップする型で操作を限定する方法に切り替える | AWS プロバイダを実装する Issue |
