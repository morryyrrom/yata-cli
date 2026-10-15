# 最初のルール群の範囲(CIS AWS Foundations Benchmark)

> This Japanese version is the authoritative source. The English version (`Feature-0005-Initial-Rule-Set-Scope.md`) has not been written yet.

最初のリリースで扱うチェックの範囲を、CIS AWS Foundations Benchmark のコントロールから決めます。含める各コントロールについて、使うリソース種別、構成情報(`configuration`)に入れる API の操作、必要な IAM 権限を対応づけ、コレクターとルールの作業を並行して進められる単位に分けます。

全体設計は [アーキテクチャ概要](../architecture/Architecture-Overview.ja.md)、リソース種別と構成情報の規則は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、ルールのメタデータは [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md)、読み取り専用ガードは [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) を参照してください。認証情報レポートの扱いは [ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) に記録します。

ルールの書式と評価の単位は、この設計の後に [ADR: ルールをリソースごとに評価する](../adr/ADR-0030-Per-Resource-Rule-Evaluation.ja.md) で変わりました(ルールは `target_type` のリソースごとに評価され、評価の対象のリソースが `input`、宣言した種別の全リソースが `data` に入る。`fail` の要素の `resource_region` は廃止)。このページの判定の形の説明(「リージョンをまたぐ判定」「監視のコントロール」「CloudTrail の証跡」)は、その変更に合わせて直してあります。「全体設計への反映」と「後続の Issue」は、この設計の時点の計画の記録です。

## 目的と背景

- 最初のリリースで評価できるコントロールの範囲を決める
- 範囲が決まると、必要なリソース種別と AWS SDK のサービスのクレートが決まり、コレクター(このリポジトリ)とルール(ポリシーのリポジトリ)の作業を並行して始められる
- 全体設計で「最初のルール群を決める Issue で決める」としていた次の事項を、この設計で決める
  - 対象とする CIS のバージョン([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の未決事項)
  - 先に追加する AWS SDK のサービス([ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md)、[初期依存クレート](../architecture/Architecture-Initial-Dependencies.ja.md))
  - 認証情報レポートの扱い([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の未決事項)
  - S3 のバケットをどの `ResourceSet` に入れるか([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ResourceSet」)

## スコープ

- 対象とする CIS AWS Foundations Benchmark のバージョン
- 含めるコントロールと、除くコントロール(理由付き)
- 含める各コントロールの `target_type` / `related_types`
- 使うリソース種別ごとのスコープ、構成情報に入れる操作、必要な IAM 権限
- `[workspace.dependencies]` に追加する AWS SDK のサービスのクレート
- 認証情報レポート(`iam:GenerateCredentialReport`)の扱い
- グローバルなサービスと S3 のバケットのリージョンの扱い
- 後続の Issue の分け方と依存関係
- コレクターの方針(収集だけを行い、判定に向けた絞り込みはルールで行う。リソースと 1 対 1 で対応する情報のまとめ方)

## スコープ外

- コレクター・エンジン・CLI の実装
- ルール本体とそのフィクスチャ(ポリシーのリポジトリで書く)
- ポリシーのリポジトリの作成
- `Cargo.toml` への依存の追加(この設計のマージ後、別の PR で行う)
- `docs/architecture/` の変更(別の PR で行う。下の「全体設計への反映」)
- 各コレクターの構成情報の細部(全項目の一覧、「設定が存在しない」に当たるエラーの全一覧、ARN が無いリソースの `id` の最終形)。各コレクターの機能設計書で決める

## 対象とする CIS のバージョン

- **CIS Amazon Web Services Foundations Benchmark v7.1.0** を対象にする
  - CIS の公開ページ(<https://www.cisecurity.org/benchmark/amazon_web_services>)に、現在の版として「Amazon Web Services Foundations (7.1.0)」が掲載されている
  - v7.1.0 の公開日は未確認(未決事項「CIS v7.1.0 の本文の確認」)。CIS の更新のお知らせに載っているのは v7.0.0 の公開(出典を参照)
- ルールのメタデータの `references` は `{"standard": "cis-aws-foundations", "version": "7.1.0", "control": "<番号>"}` とする。ルールの公開は、v7.1.0 の本文で番号を確かめた後に行う(未決事項「CIS v7.1.0 の本文の確認」)

### コントロールの番号と題名の確かさ

- CIS のベンチマークの本文(PDF)は CIS WorkBench での登録が必要で、v7.1.0 の本文は未確認(未決事項)
- このページのコントロールの番号・題名・レベル(L1 / L2)は、**v7.0.0 のもの**を、CIS の認定を受けた第三者の監査ファイルの公開ページ(Tenable の「CIS Amazon Web Services Foundations v7.0.0 L1 / L2」)で確かめたもの。v7.1.0 で番号・題名が変わっている可能性がある
- 自動化できる(Automated)か手作業(Manual)かの区分も、v7.0.0・v7.1.0 のどちらについても公式の情報では確かめられなかった。下の「除くコントロール」で「人の判断が要る」とした理由は、題名の内容からの判断である
- ルールを公開する前に、v7.1.0 の本文で番号・題名・区分を確かめる(未決事項)。番号が違っていた場合は、この表を直す PR を先に出す

## コントロールを選んだ基準

次の基準をすべて満たすものを選び、件数の目安(15〜25 件)に収めました。

1. **読み取り専用の API で判定できる**: 構成情報だけで合否が決まり、人の判断・組織の手続きの確認を要しない
2. **影響が大きい**: アカウントの乗っ取り(ルートユーザー、認証情報)、データの公開(S3)、監査の記録の喪失(CloudTrail)、ネットワークの露出(セキュリティグループ、ネットワーク ACL)に直結する
3. **収集するサービスを抑える**: 新しく必要な SDK のサービスを、IAM・S3・CloudTrail・監視(CloudWatch Logs、CloudWatch、SNS)・EC2 に限る。同じリソース種別で複数のコントロールを判定できるものを優先する
4. **拒否リストの操作を要しない**: `s3:GetObject` などで中身を読む必要が無い
5. **1 つのアカウントで判定できる**: AWS Organizations をまたぐ判定を要しない(複数アカウントは全体設計で未決事項)
6. **L1 を優先する**: L2 は、影響が特に大きく、収集の追加が小さい(新しい SDK のサービスを要しない)ものに限って含める。含めた L2 の 5 件の理由は次のとおり
   - 4.2、4.5、6.5: L1 のために集める種別(`aws.cloudtrail.trail`、`aws.ec2.security_group`)だけで判定でき、収集の追加が無い
   - 3.1.1: 暗号化されない通信でのデータの取得を防ぐ設定で、データの保護への影響が大きい。追加はバケットごとの `GetBucketPolicy` の 1 つの操作だけで、種別は 3.1.4 と同じ `aws.s3.bucket`
   - 4.7: VPC のフローログは、ネットワークを通じた侵害の調査に欠かせない記録で、監査の記録の喪失(基準 2)に当たる。新しい種別(`aws.ec2.vpc`、`aws.ec2.flow_log`)を足すが、`aws-sdk-ec2` は追加済みで、リージョンごとの一覧の操作 2 つだけで済む

## 含めるコントロール(24 件)

図: [コントロールとリソース種別の対応](../diagrams/Diagram-0005-CIS-Controls-Resource-Mapping.md)

番号・題名は v7.0.0 のもの(上の「コントロールの番号と題名の確かさ」)。「判定に使う操作」は、構成情報のうち判定に主に使う操作のキーです。括弧の中は、その操作の値の中の項目の経路です。リソースを列挙する操作(`DescribeTrails`、`DescribeInstances` など)の値は出力の形を保ち、要素の配列をそのリソースの要素 1 つだけにしたものなので、経路は添字 `[0]` を含みます([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「列挙の操作の値」)。リソース種別ごとの操作と権限の全体は、下の「リソース種別ごとの操作と IAM 権限」の表にあります。コントロールに必要な IAM 権限は、`target_type` と `related_types` の種別の権限の和です。

| 番号 | 題名 | Lv | `target_type` | `related_types` | 判定に使う操作 |
| --- | --- | --- | --- | --- | --- |
| 2.4 | Ensure no 'root' user account access key exists | L1 | `aws.iam.account_summary` | なし | `GetAccountSummary`(`SummaryMap.AccountAccessKeysPresent`) |
| 2.5 | Ensure MFA is enabled for the 'root' user account | L1 | `aws.iam.account_summary` | なし | `GetAccountSummary`(`SummaryMap.AccountMFAEnabled`) |
| 2.8 | Ensure IAM password policy requires minimum length of 14 or greater | L1 | `aws.iam.password_policy` | なし | `GetAccountPasswordPolicy` |
| 2.9 | Ensure IAM password policy prevents password reuse | L1 | `aws.iam.password_policy` | なし | `GetAccountPasswordPolicy` |
| 2.10 | Ensure multi-factor authentication (MFA) is enabled for all IAM users that have a console password | L1 | `aws.iam.user` | なし | `GetCredentialReport`(`password_enabled`、`mfa_active`) |
| 2.11 | Ensure credentials unused for 45 days or more are disabled | L1 | `aws.iam.user` | なし | `GetCredentialReport`(`password_last_used`、`access_key_N_last_used_date` など) |
| 2.12 | Ensure access keys are rotated every 90 days or less | L1 | `aws.iam.user` | なし | `GetCredentialReport`(`access_key_N_active`、`access_key_N_last_rotated`) |
| 2.13 | Ensure IAM users receive permissions only through groups | L1 | `aws.iam.user` | なし | `ListAttachedUserPolicies`、`ListUserPolicies` |
| 2.14 | Ensure IAM policies that allow full "\*:\*" administrative privileges are not attached | L1 | `aws.iam.policy` | なし | `ListPolicies`、`GetPolicyVersion` |
| 3.1.1 | Ensure S3 Bucket Policy is set to deny HTTP requests | L2 | `aws.s3.bucket` | なし | `GetBucketPolicy` |
| 3.1.4 | Ensure that S3 is configured with 'Block Public Access' enabled | L1 | `aws.s3.bucket` | `aws.s3control.account_public_access_block` | `GetPublicAccessBlock`(バケット・アカウントの両方) |
| 4.1 | Ensure CloudTrail is enabled in all regions | L1 | `aws.account.account` | `aws.cloudtrail.trail` | `DescribeTrails`、`GetTrailStatus`、`GetEventSelectors` |
| 4.2 | Ensure CloudTrail log file validation is enabled | L2 | `aws.cloudtrail.trail` | なし | `DescribeTrails`(`trailList[0].LogFileValidationEnabled`) |
| 4.4 | Ensure that server access logging is enabled on the CloudTrail S3 bucket | L1 | `aws.cloudtrail.trail` | `aws.s3.bucket` | `DescribeTrails`(`trailList[0].S3BucketName`)、バケットの `GetBucketLogging` |
| 4.5 | Ensure CloudTrail logs are encrypted at rest using KMS CMKs | L2 | `aws.cloudtrail.trail` | なし | `DescribeTrails`(`trailList[0].KmsKeyId`) |
| 4.7 | Ensure VPC flow logging is enabled in all VPCs | L2 | `aws.ec2.vpc` | `aws.ec2.flow_log` | `DescribeVpcs`、`DescribeFlowLogs` |
| 5.2 | Ensure management console sign-in without MFA is monitored | L1 | `aws.account.account` | `aws.cloudtrail.trail`、`aws.cloudwatchlogs.metric_filter`、`aws.cloudwatch.metric_alarm`、`aws.sns.topic` | 下の「監視のコントロール」 |
| 5.3 | Ensure usage of the 'root' account is monitored | L1 | `aws.account.account` | 5.2 と同じ | 下の「監視のコントロール」 |
| 6.1.1 | Ensure EBS volume encryption is enabled in all regions | L1 | `aws.ec2.ebs_encryption_default` | なし | `GetEbsEncryptionByDefault` |
| 6.2 | Ensure no Network ACLs allow ingress from 0.0.0.0/0 to remote server administration ports | L1 | `aws.ec2.network_acl` | なし | `DescribeNetworkAcls` |
| 6.3 | Ensure no security groups allow ingress from 0.0.0.0/0 to remote server administration ports | L1 | `aws.ec2.security_group` | なし | `DescribeSecurityGroups` |
| 6.4 | Ensure no security groups allow ingress from ::/0 to remote server administration ports | L1 | `aws.ec2.security_group` | なし | `DescribeSecurityGroups` |
| 6.5 | Ensure the default security group of every VPC restricts all traffic | L2 | `aws.ec2.security_group` | なし | `DescribeSecurityGroups` |
| 6.7 | Ensure that the EC2 Metadata Service only allows IMDSv2 | L1 | `aws.ec2.instance` | なし | `DescribeInstances`(`Reservations[0].Instances[0].MetadataOptions.HttpTokens`) |

内訳: IAM 9 件、S3 2 件、CloudTrail・ログ 5 件、監視 2 件、EC2/VPC 6 件(L1 19 件、L2 5 件)。

### `target_type` の選び方

- 結果(`pass` / `fail`)はリソースごとに作られるため、検出の対象になるリソースの種別を `target_type` にする
- 「全リージョンで有効か」「アカウントに監視の設定が 1 組あるか」のように、特定のリソースに結びつかない判定は `aws.account.account` を `target_type` にし、判定に使う種別を `related_types` に置く(4.1、5.2、5.3)。`aws.account.account` は常に収集済みなので、関連する種別が 0 件でも結果が 1 件出る
- アカウント・リージョンに 1 件の設定は、その設定の種別を `target_type` にする(2.4、2.5、2.8、2.9、6.1.1)。設定が無い場合も、存在しないことを表すリソースが 1 件あるため、ルールは「未設定」を `fail` にできる
- 4.4 は、検出の対象を CloudTrail の証跡にする。バケットを `target_type` にすると、証跡と無関係なすべてのバケットに `pass` が付き、結果が読みにくくなるため

### リージョンをまたぐ判定

ルールは `target_type` のリソースごとに評価され、そのリソースが `input` に入ります。`data` はアカウント単位で、全リージョンのリソースを種別ごとに 1 つの配列にまとめ、`data.yata.context.regions` に対象にしたリージョンを入れます([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ポリシーエンジンへの渡し方」)。この形に合わせ、次のように判定します。

| コントロール | 判定の形 |
| --- | --- |
| 4.1 | `input` はアカウント(`aws.account.account`)。`data.aws.cloudtrail.trail` の中に、`IsMultiRegionTrail` が真で、`GetTrailStatus.IsLogging` が真で、管理イベントの読み書きをすべて記録する証跡が 1 つ以上あるかを見る。配列には、ホームリージョンの証跡と、ほかのリージョンに現れる shadow trail の両方が入る(下の「CloudTrail の証跡」)。存在を確かめる判定なので、どちらのコピーで条件を満たしてもよく、同じ証跡が複数あっても結果は 1 件(`aws.account.account`)のまま |
| 6.1.1 | `aws.ec2.ebs_encryption_default` は対象リージョンごとに 1 件あるので、各リソース(`input`)を判定する。`data.yata.context.regions` のリージョンのうちリソースが無いものは、収集の失敗・未収集としてエンジンがルールの状態で表す |
| 4.7 | VPC(`input`)ごとに、`data.aws.ec2.flow_log` に `ResourceId` がその VPC の ID のフローログがあるかを見る。VPC とフローログは同じリージョンにある |
| 5.2、5.3 | 証跡(ホームリージョンのものか shadow trail かを問わない)の `CloudWatchLogsLogGroupArn` のロググループのリージョンで、メトリクスフィルター・アラーム・SNS のトピックを探す。リージョンをまたいで配列からたどれる。証跡の設定はどのコピーでも同じなので、同じ証跡の複数のコピーからたどっても、たどり着く先は同じ |

- 「全リージョン」は、そのスキャンの対象リージョン(`data.yata.context.regions`)の範囲での判定になる。`--region` で対象を絞った場合、対象外のリージョンは判定に入らない(未決事項)

### 監視のコントロール

図: [監視のコントロールの連なり](../diagrams/Diagram-0005-Monitoring-Control-Chain.md)

5.2 と 5.3 は、次の連なりが 1 組以上あるときに `pass` とします。判定の細部(フィルターのパターンの比べ方など)はルールの Issue で決めます。

1. マルチリージョンで記録中(`IsLogging`)の証跡があり、管理イベントをすべて記録し、`CloudWatchLogsLogGroupArn` が設定されている(`aws.cloudtrail.trail`。shadow trail でもよい)
2. そのロググループに、コントロールが求めるパターンのメトリクスフィルターがある(`aws.cloudwatchlogs.metric_filter`)
3. そのフィルターのメトリクス(名前と名前空間)に対するアラームがある(`aws.cloudwatch.metric_alarm`)
4. アラームのアクションの SNS のトピックに、確認済みの購読が 1 つ以上ある(`aws.sns.topic`)

結果は `aws.account.account` の 1 件なので、shadow trail が重複して現れても結果は増えません。メンバーアカウントで組織の証跡(shadow trail)しか無い場合、ロググループは通常、組織の証跡を作ったアカウントにあり、2〜4 が `data` に無いため `fail` になります。この場合の判定と `message` の書き方(組織の証跡のロググループが別のアカウントにあることを示すなど)は未決事項「別のアカウントにある証跡の送り先」で決めます。

5.1、5.4〜5.15 も同じ 4 つの種別で判定できるため、今回の範囲に含めなかったものも、コレクターを足さずにルールだけで追加できます。

## 除くコントロール(46 件)

v7.0.0 の L1 / L2 の監査ファイルに載っていた 70 件のうち、上の 24 件以外を除きます。理由の区分は次のとおりです。

| 区分 | 内容 |
| --- | --- |
| A | AWS Organizations・複数アカウントが前提。複数アカウントは全体設計の未決事項 |
| B | 人の判断・組織の手続きの確認が要る(題名からの判断。上の「コントロールの番号と題名の確かさ」) |
| C | 今回の範囲に無いサービスの収集が要る。次の段階の候補 |
| D | 今回のサービスで追加できるが、件数の目安と影響の大きさから後回しにする。次の段階の候補 |

| 番号 | 題名(要約) | Lv | 区分 | 理由 |
| --- | --- | --- | --- | --- |
| 2.1.1〜2.1.6 | AWS Organizations の構成(ルートアクセスの一元管理、ガードレール、管理アカウント、OU、委任管理者) | L2 | A | Organizations の管理アカウントからの判定が要る |
| 2.2 | Maintain current AWS account contact details | L1 | B | 連絡先が「最新か」は API で判定できない |
| 2.3 | Ensure security contact information is registered | L1 | C | Account サービス(`account:GetAlternateContact`)の収集が要る。次の段階の候補 |
| 2.6 | Ensure hardware MFA is enabled for the 'root' user account | L2 | D | 仮想 MFA かどうかの判定に `iam:ListVirtualMFADevices` が要る |
| 2.7 | Eliminate use of the 'root' user for administrative and daily tasks | L1 | B | 「日常の作業に使っていないか」は判断を伴う。認証情報レポートのルートの行で一部を判定できるため、次の段階で再検討する |
| 2.15 | Ensure a support role has been created to manage incidents with AWS Support | L1 | D | ロール(`iam:ListEntitiesForPolicy` など)の収集が要る |
| 2.16 | Ensure IAM instance roles are used for AWS resource access from instances | L2 | B | インスタンスが AWS にアクセスするかどうかは構成情報から分からない |
| 2.17 | Ensure that all expired SSL/TLS certificates stored in AWS IAM are removed | L1 | D | `iam:ListServerCertificates` の収集が要る。影響が比較的小さい |
| 2.18 | Ensure that IAM External Access Analyzer is enabled for all regions | L1 | C | IAM Access Analyzer の収集が要る。次の段階の候補 |
| 2.19 | Ensure IAM users are managed centrally via identity federation or AWS Organizations | L2 | A | 複数アカウントの管理の方法の判断が要る |
| 2.20 | Ensure access to AWSCloudShellFullAccess is restricted | L1 | D | ポリシーのアタッチ先(`iam:ListEntitiesForPolicy`)の収集が要る |
| 2.21 | Ensure AWS resource policies do not allow unrestricted access using "Principal": "\*" | L1 | C | 多数のサービスのリソースポリシーの収集が要る |
| 3.1.2 | Ensure MFA Delete is enabled on S3 buckets | L2 | D | `s3:GetBucketVersioning` が要る。MFA Delete はルートユーザーしか有効にできず、検出が多くなりやすい |
| 3.1.3 | Ensure all data in Amazon S3 has been discovered, classified, and secured when necessary | L2 | B | データの中身の分類が要る。中身の読み取り(`s3:GetObject`)は拒否リストにあり行わない |
| 3.2.1〜3.2.3 | RDS の保存時の暗号化、マイナーバージョンの自動更新、パブリックアクセス | L1 | C | RDS の収集が要る。次の段階の候補 |
| 3.2.4 | Ensure Multi-AZ deployments are used for enhanced availability in Amazon RDS | L1 | C | RDS の収集が要る。可用性の項目で、設定ミスの影響が他より小さい |
| 3.3.1 | Ensure that encryption is enabled for EFS file systems | L1 | C | EFS の収集が要る。次の段階の候補 |
| 4.3 | Ensure AWS Config is enabled in all regions | L2 | C | AWS Config の収集が要る。次の段階の候補 |
| 4.6 | Ensure rotation for customer-created symmetric CMKs is enabled | L2 | C | KMS の収集が要る。次の段階の候補 |
| 4.8、4.9 | S3 のオブジェクトレベルのログ(書き込み・読み取り) | L2 | D | 証跡のイベントセレクターとバケットの組み合わせで判定でき、収集の追加は無い。件数の目安のため後回し |
| 4.10 | Ensure all AWS-managed web front-end services have access logging enabled | L1 | C | CloudFront、ELB、API Gateway など多数のサービスの収集が要る |
| 5.1、5.4〜5.15 | 監視(5.2、5.3 以外) | L1 / L2 | D | 5.2、5.3 と同じ種別で判定でき、収集の追加は無い。ルールだけで追加できる |
| 5.16 | Ensure AWS Security Hub is enabled | L2 | C | Security Hub の収集が要る |
| 6.1.2 | Ensure CIFS access is restricted to trusted networks | L1 | D | `aws.ec2.security_group` で判定でき、収集の追加は無い。対象が CIFS(ポート 445)だけで、リモート管理のポートを広く扱う 6.3・6.4 より露出の範囲が狭く、4.7(調査の記録)より優先度を下げた。件数の上限に近いため最初のリリースのルールの作業量を抑え、次の段階でルールだけで加える |
| 6.6 | Ensure routing tables for VPC peering are "least access" | L2 | B | 「最小のアクセス」かどうかは判断を伴う |
| 6.8 | Ensure VPC Endpoints are used for access to AWS Services | L2 | B | どのサービスにエンドポイントが要るかは判断を伴う |

- 拒否リストの操作が要るコントロールは、3.1.3 だけでした(中身の分類を自分で行う場合)。範囲から除きます
- v7.1.0 で追加・削除されたコントロールがあれば、v7.1.0 の本文の確認(未決事項)のときにこの表に加える

## コレクターの方針

この範囲のコレクターは、次の方針に従います。[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) に加えるまでは、この節を根拠とします(下の「全体設計への反映」)。

- **コレクターは情報を集めるだけにする**。合否の判定と、判定に向けた絞り込みは Rego のルールで行う
- **API は既定のパラメーターで呼ぶ**(結果を絞り込む・広げるオプションを付けない)。次のものは「オプション」に含めない
  - ページングのためのパラメーター: 続きを取るトークン(`NextToken`、`Marker`、`ContinuationToken` など)と、ページングの形で呼ぶためのページの大きさ(`ListBuckets` の `MaxBuckets`。下の「S3 のバケット」)。どちらも返るリソースの集合を変えない
  - 必須の識別子: リソースごとの操作で対象を指すもの(`GetBucketPolicy` の `Bucket`、`GetPolicyVersion` の `PolicyArn` と `VersionId`、`GetTrailStatus` の `Name` など)
- **返った結果は、絞り込まず・置き換えずにすべて入れる**
  - 「すべて入れる」の対象は、リソースの情報を取る操作(リソースを列挙する操作と、リソースごとの付随情報を取る操作)の出力である。別の操作の前提として呼ぶ手順の操作の出力は、リソースの情報ではないため対象外で、構成情報のキーにしない。これは「すべて入れる」の例外ではない。この範囲では、認証情報レポートの生成を待つために呼ぶ `GenerateCredentialReport` の出力(`Description`、`State`)がこれに当たる([IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「`GenerateCredentialReport` の出力を入れない理由」)
- **許す変換は、値の意味・件数を変えない形式の変換だけ**: SDK の型から JSON への変換、日時の RFC 3339 の文字列への変換、列挙の値の文字列への変換、バイナリ(Blob)の base64 の文字列への変換と、テキストの形式が文書化された Blob の構造化(その種別の機能設計書で決めた場合。[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「項目名と値の規則」)、ポリシー文書の URL デコードと JSON のパース。ページングの項目と応答のメタデータを含めないことと、列挙の操作の出力の要素の配列をリソースごとに分けることは、全体設計の「格納の形」「列挙の操作の値」のとおり
- **秘密の値も伏せない**(`"<redacted>"` への置き換えは行わない)。ただし、秘密の値の取得そのものが目的の API(読み取り専用ガードの拒否リストの操作。[AWS アクセス](../architecture/Architecture-AWS-Access.ja.md))は呼ばない
  - 応答に含まれる値は、スナップショット、Rego の `input` / `data`、ルールが `actual` に入れた場合はレポートに、そのまま入る。スナップショットを外部に出さないこと(`yata collect` の注意)と、レポートを外部に出すときの `--omit-values` で扱う
  - ルールの作者は、判定に要らない機微な値(SNS の購読の `Endpoint` など)を `actual` や `message` に入れない
- **あるリソースと 1 対 1 で対応する情報は、呼び出す API が分かれていても 1 つのリソースにまとめる**
  - リソースごとに呼ぶ取得の操作(`GetBucketPolicy`、`GetTrailStatus` など)の出力を、操作のキーで一覧の操作と同じリソースに入れる形([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「格納の形」)は、この原則の一つである
  - アカウントに 1 つの操作の出力でも、その中の要素がリソースと 1 対 1 で対応するなら、要素ごとに対応するリソースに入れる。この範囲では、認証情報レポート(`GetCredentialReport`)の各行がこれに当たり、IAM ユーザーとそのユーザーの行は 1 対 1 なので、行を `aws.iam.user` の各リソースに入れる(下の「認証情報レポート」)。これは「絞り込み」ではなく、この原則による対応づけで、行の内容は変えずに入れる。ただし、対応するリソースの無いルートユーザーの行は、下の「例外」のとおり、この範囲では入れない
- **まとめる対象は、あるリソースに付随する情報に限る**。アカウント(`aws.account.account`)は、この原則でいう「あるリソース」に含めない。アカウントに 1 つだけの設定(`aws.iam.account_summary`、`aws.iam.password_policy`、`aws.s3control.account_public_access_block` など)は、`aws.account.account` にはまとめず、それぞれ別の種別にする
  - 理由: サービスの異なる設定を 1 つの種別にまとめると、ある設定の収集の失敗がその種別を使うすべてのルールに波及し(「振る舞い」のエラー時)、関係の無い判定まで `incomplete` になる。また、1 つのコレクターがサービスをまたぎ、1 リソース種別 1 ファイルの構成と、サービスごとに分けた後続の Issue の並行の作業が崩れる
  - リソースと 1 対多で対応する情報(`DescribeInstances` の予約とインスタンスなど)は、この原則の対象ではない。ただし、列挙の操作の出力で、リソースの要素を包む上位の要素の項目(`DescribeInstances` の予約の `ReservationId`、`OwnerId` など)は、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「列挙の操作の値」の入れ子の配列の形で、その中の各リソースに入る([ADR: 列挙の操作の値に出力の形を保つ](../adr/ADR-0039-Keep-Output-Shape-for-Listing-Operation-Values.ja.md))
- **例外**: 「すべて入れる」に対する例外は、この範囲では次のものだけとする
  - `aws.account.account` には `GetCallerIdentity` の `Account` だけを入れ、`Arn`・`UserId` は入れない(全体設計の「SDK の応答をすべて入れる方針の例外」のとおり)。理由: `aws.account.account` はコレクターの収集で作るものではなく、`yata-provider-aws` がアカウントを識別するために作るものだから。全体設計の理由(ルールの判定に使わず、実行した人の情報を残さない)も引き継ぐ
  - 認証情報レポートのルートユーザーの行(`<root_account>`)は、入れ先を決めるまで、この範囲ではどのリソースにも入れない。入れ先が決まるまでの暫定の不足である(下の「認証情報レポート」、未決事項「認証情報レポートのルートユーザーの行」)
- **バイナリ(Blob)の項目も入れる**。Blob は標準の base64 の文字列で入れ、AWS のドキュメントでテキストの形式が定められた Blob は、その種別の機能設計書で決めて構造化して入れてよい([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「項目名と値の規則」)。認証情報レポートの `Content` は、この規則で構造化して入れるもので、「すべて入れる」の例外ではない(下の「認証情報レポート」)

## リソース種別ごとの操作と IAM 権限

- 種別名は 3 階層で、各部分は `[a-z][a-z0-9_]*`、Rego の予約語を含まない。`<service>` は `aws-sdk-<service>` の `<service>` に合わせる
- 「操作(構成情報のキー)」は、構成情報のキーになる API の操作名。1 行目がリソースを列挙する操作(失敗すると組全体の失敗)で、2 行目以降はリソースごとの付随情報を取る操作(失敗するとそのキーに失敗の印)
- 「null にするエラー」は、「設定が存在しない」としてキーの値を `null` にするエラーの候補。最終的な一覧は各コレクターの機能設計書で決める
- `id` の決め方
  - 「ARN」: 応答に ARN があればそれを使う
  - 「ARN(形式から作る)」: AWS が ARN を定義しているが、応答に含まれない種別。これらは「ARN が無いリソース」に当たらないと判断し、AWS の文書の ARN の形式(Service Authorization Reference の各サービスの「Resource types」の表)に従って作った ARN を `id` にする。EC2 の例: `arn:aws:ec2:<region>:<account_id>:vpc/<VpcId>`(IAM ユーザーガイドの ARN の例と同じ形)。ネットワーク ACL(`network-acl/<NetworkAclId>`)、インスタンス(`instance/<InstanceId>`)、フローログ(`vpc-flow-log/<FlowLogId>`)も同じ表で定義されている。各形式は、コレクターの機能設計書で表と照らして確かめる
  - 「`<type>:…`」: AWS が ARN を定義していないもの。[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ARN が無いリソースの id」の形式に従う
- ARN を形式から作ることを全体設計で明示する変更を、「全体設計への反映」に挙げる

| 種別 | スコープ | 操作(構成情報のキー) | IAM 権限 | null にするエラー | `id` |
| --- | --- | --- | --- | --- | --- |
| `aws.iam.account_summary` | global | `GetAccountSummary` | `iam:GetAccountSummary` | なし | `aws.iam.account_summary:<account_id>:global` |
| `aws.iam.password_policy` | global | `GetAccountPasswordPolicy` | `iam:GetAccountPasswordPolicy` | `NoSuchEntity` | `aws.iam.password_policy:<account_id>:global` |
| `aws.iam.user` | global | `ListUsers` / `ListAttachedUserPolicies` / `ListUserPolicies` / `GetCredentialReport` | `iam:ListUsers`、`iam:ListAttachedUserPolicies`、`iam:ListUserPolicies`、`iam:GenerateCredentialReport`、`iam:GetCredentialReport` | なし | ユーザーの ARN |
| `aws.iam.policy` | global | `ListPolicies` / `GetPolicyVersion` | `iam:ListPolicies`、`iam:GetPolicyVersion` | なし | ポリシーの ARN |
| `aws.s3.bucket` | global(下の「S3 のバケット」) | `ListBuckets` / `GetBucketPolicy` / `GetPublicAccessBlock` / `GetBucketLogging` | `s3:ListAllMyBuckets`、`s3:GetBucketPolicy`、`s3:GetBucketPublicAccessBlock`、`s3:GetBucketLogging` | `NoSuchBucketPolicy`、`NoSuchPublicAccessBlockConfiguration` | バケットの ARN |
| `aws.s3control.account_public_access_block` | global | `GetPublicAccessBlock` | `s3:GetAccountPublicAccessBlock` | `NoSuchPublicAccessBlockConfiguration` | `aws.s3control.account_public_access_block:<account_id>:global` |
| `aws.cloudtrail.trail` | regional | `DescribeTrails` / `GetTrailStatus` / `GetEventSelectors` | `cloudtrail:DescribeTrails`、`cloudtrail:GetTrailStatus`、`cloudtrail:GetEventSelectors` | なし | 証跡の ARN(同じ ARN が複数のリージョンに現れる。下の「CloudTrail の証跡」) |
| `aws.cloudwatchlogs.metric_filter` | regional | `DescribeMetricFilters` | `logs:DescribeMetricFilters` | なし | `aws.cloudwatchlogs.metric_filter:<account_id>:<region>:<name>`(`<name>` はロググループ名とフィルター名から作る。形式はコレクターの機能設計書で決める) |
| `aws.cloudwatch.metric_alarm` | regional | `DescribeAlarms` | `cloudwatch:DescribeAlarms` | なし | アラームの ARN |
| `aws.sns.topic` | regional | `ListTopics` / `ListSubscriptionsByTopic` | `sns:ListTopics`、`sns:ListSubscriptionsByTopic` | なし | トピックの ARN |
| `aws.ec2.security_group` | regional | `DescribeSecurityGroups` | `ec2:DescribeSecurityGroups` | なし | セキュリティグループの ARN |
| `aws.ec2.network_acl` | regional | `DescribeNetworkAcls` | `ec2:DescribeNetworkAcls` | なし | ARN(形式から作る) |
| `aws.ec2.instance` | regional | `DescribeInstances` | `ec2:DescribeInstances` | なし | ARN(形式から作る) |
| `aws.ec2.vpc` | regional | `DescribeVpcs` | `ec2:DescribeVpcs` | なし | ARN(形式から作る) |
| `aws.ec2.flow_log` | regional | `DescribeFlowLogs` | `ec2:DescribeFlowLogs` | なし | ARN(形式から作る) |
| `aws.ec2.ebs_encryption_default` | regional | `GetEbsEncryptionByDefault` | `ec2:GetEbsEncryptionByDefault` | なし | `aws.ec2.ebs_encryption_default:<account_id>:<region>` |

このほか、`aws.account.account`(`sts:GetCallerIdentity`)を 4.1、5.2、5.3 の `target_type` に使います。`aws.account.account` は `yata-provider-aws` が作り、コレクターは不要です。

### 種別ごとの補足

**アカウント・リージョンに 1 件の設定**(`aws.iam.account_summary`、`aws.iam.password_policy`、`aws.s3control.account_public_access_block`、`aws.ec2.ebs_encryption_default`)

- 取得の操作が 1 つだけで、その操作がリソースそのものを作るため、その操作の失敗(「設定が存在しない」以外)は組全体の失敗(`CollectionError`)にする
- 「設定が存在しない」エラーでは、キーの値を `null` にしたリソースを 1 件作る([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「リソース種別」)

**`aws.iam.user`**

- `ListUserPolicies` はインラインポリシーの名前だけを返す。2.13 はインラインポリシーの有無だけを見るので、ポリシー文書(`GetUserPolicy`)は取らない
- `GetCredentialReport` の扱いは下の「認証情報レポート」

**`aws.iam.policy`**

- `ListPolicies` は既定のパラメーターで呼ぶ(`Scope`、`OnlyAttached`、`PathPrefix`、`PolicyUsageFilter` を付けない)。API の既定では、カスタマー管理ポリシーと AWS 管理ポリシーのすべてが、アタッチの有無にかかわらず返る
- アタッチされているかの絞り込み(`AttachmentCount` が 1 以上か)と、AWS 管理ポリシーを対象にするか(`Arn` が `arn:aws:iam::aws:policy/` で始まるか)は、2.14 のルールで行う
- 影響: どのアカウントでも、使われていない AWS 管理ポリシー(1,000 件を超える)も入る
  - `GetPolicyVersion` の呼び出しが、ポリシーの件数だけ(1,000 回を超えて)増える。IAM の API のスロットリングに当たりやすくなり、収集の時間が延びる
  - スナップショットの `aws.iam.policy` のファイルと、2.14 の `data` が、AWS 管理ポリシーの文書の分だけ大きくなる
  - 呼び出しの並行数・スロットリングの際の再試行の扱いは、後続の Issue 4a で決める
- `GetPolicyVersion` は、`ListPolicies` の `DefaultVersionId` の版を取る(`PolicyArn` と `VersionId` は必須の識別子)。値は出力全体(`{"PolicyVersion": {...}}`)で、`Document` は URL デコードしてから JSON にパースする([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「項目名と値の規則」)
- インラインポリシーは 2.14 の対象に含めない(未決事項)

**`aws.cloudwatchlogs.metric_filter`**

- CloudWatch Logs の API の項目名は camelCase(`filterName`、`logGroupName`、`metricTransformations` など)。項目名は API の名前をそのまま使う規則なので、camelCase のまま入れる。構成情報のキー(操作名 `DescribeMetricFilters`)は PascalCase のまま。全体設計の「PascalCase のメンバー名」の書き方の修正は「全体設計への反映」に挙げる

**`aws.sns.topic`**

- `ListSubscriptionsByTopic` の値は出力全体(`{"Subscriptions": [...]}`。ページングの項目を除く)
- 購読の `Endpoint`(メールアドレス、HTTP(S) の URL、電話番号など)も、伏せずにそのまま入れる(上の「コレクターの方針」)
- 影響: 個人のメールアドレスや、URL に埋め込まれた認証情報(クエリ文字列のトークンなど)が、スナップショットと Rego の `input` / `data` に載る。ルールが `actual` に入れれば、レポートにも載る
- 5.2、5.3 の判定には、確認済みの購読(`SubscriptionArn` が `PendingConfirmation` でない)があるかだけを使い、`Endpoint` を `actual` や `message` に入れない

**`aws.ec2.instance`**

- `DescribeInstances` の値は、出力の形を保ち、経路にある配列(`Reservations` と、その予約の `Instances`)を、そのインスタンスとその予約の要素 1 つだけの配列にしたもの: `{"Reservations": [{"ReservationId": ..., "OwnerId": ..., "RequesterId": ..., "Groups": [...], "Instances": [<そのインスタンスの要素>]}]}`([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「列挙の操作の値」の入れ子の配列、[ADR: 列挙の操作の値に出力の形を保つ](../adr/ADR-0039-Keep-Output-Shape-for-Listing-Operation-Values.ja.md))。予約の項目は、その予約のどのインスタンスの値にも入る。ページングの項目(`NextToken`)は入れない。`UserData` は `DescribeInstances` の応答に含まれず、取得もしない
- 6.7 の判定には、`Reservations[0].Instances[0].MetadataOptions.HttpTokens` を使う

**`aws.cloudwatch.metric_alarm`**

- `DescribeAlarms` は既定のパラメーターで呼ぶ。API の既定では、メトリクスのアラーム(`MetricAlarms`)だけが返り、複合アラームは返らない。種別がメトリクスのアラームなので、既定のままでよい

**`aws.ec2.*`、`aws.cloudwatchlogs.metric_filter`、`aws.sns.topic` の一覧の操作**

- `DescribeSecurityGroups`、`DescribeNetworkAcls`、`DescribeInstances`、`DescribeVpcs`、`DescribeFlowLogs`、`DescribeMetricFilters`、`ListTopics` は、フィルター(`Filters`、`logGroupName` など)を付けずに呼ぶ

### 収集するサービスと SDK のクレート

| サービス | SDK のクレート | 状態 | 種別 |
| --- | --- | --- | --- |
| STS | `aws-sdk-sts` | 追加済み | `aws.account.account` |
| EC2 | `aws-sdk-ec2` | 追加済み | `aws.ec2.*`(6 種別) |
| IAM | `aws-sdk-iam` | 追加する | `aws.iam.*`(4 種別) |
| S3 | `aws-sdk-s3` | 追加する | `aws.s3.bucket` |
| S3 Control | `aws-sdk-s3control` | 追加する | `aws.s3control.account_public_access_block` |
| CloudTrail | `aws-sdk-cloudtrail` | 追加する | `aws.cloudtrail.trail` |
| CloudWatch Logs | `aws-sdk-cloudwatchlogs` | 追加する | `aws.cloudwatchlogs.metric_filter` |
| CloudWatch | `aws-sdk-cloudwatch` | 追加する | `aws.cloudwatch.metric_alarm` |
| SNS | `aws-sdk-sns` | 追加する | `aws.sns.topic` |

## 追加する SDK のクレート

`[workspace.dependencies]` に追加するのは次の 7 つです。

| クレート | 用途 | 選定理由 | ライセンス |
| --- | --- | --- | --- |
| `aws-sdk-iam` | IAM の 4 種別 | AWS 公式の SDK([ADR: AWS SDK for Rust と読み取り専用ガード](../adr/ADR-0001-AWS-SDK-and-Read-Only-Guard.ja.md)) | Apache-2.0 |
| `aws-sdk-s3` | S3 のバケット | 同上 | Apache-2.0 |
| `aws-sdk-s3control` | アカウントのブロックパブリックアクセス(3.1.4) | 同上。アカウント単位の設定は S3 Control の API にしか無い | Apache-2.0 |
| `aws-sdk-cloudtrail` | 証跡 | 同上 | Apache-2.0 |
| `aws-sdk-cloudwatchlogs` | メトリクスフィルター | 同上 | Apache-2.0 |
| `aws-sdk-cloudwatch` | アラーム | 同上 | Apache-2.0 |
| `aws-sdk-sns` | トピックと購読 | 同上 | Apache-2.0 |

- バージョンは追加する時点の最新の安定版とし、既にある `aws-config`・`aws-sdk-sts`・`aws-sdk-ec2` と同じ時期のリリースにそろえる
- `yata-provider-aws` の `[dev-dependencies]` で、各クレートの `test-util` 機能を有効にする(`aws-smithy-mocks` を使うため。[テスト方針](../architecture/Architecture-Testing-Strategy.ja.md))

### 依存の入れ方

ルートの `[workspace.dependencies]` に書いただけでは、どのメンバーも参照しないため `Cargo.lock` は変わりません。`Cargo.lock` が変わるのは、`crates/yata-provider-aws/Cargo.toml` が `aws-sdk-<service>.workspace = true` で参照したときです。サービスごとのコレクターの PR がそれぞれ `crates/yata-provider-aws/Cargo.toml` と `Cargo.lock` を書き換えると互いに衝突するため、次のように入れます。

- AWS プロバイダの基盤の Issue(前提の P1)の PR で、次をまとめて行い、`Cargo.lock` の更新を 1 回にする(後続の Issue 2 は P1 に含める)
  - ルートの `[workspace.dependencies]` に 7 つのクレートを追加する
  - `crates/yata-provider-aws/Cargo.toml` の `[dependencies]` に、この範囲で使う SDK のクレート 9 つ(追加する 7 つと、`aws-sdk-sts`・`aws-sdk-ec2`)を `.workspace = true` で入れる
  - 同じ `[dev-dependencies]` に、9 つの `test-util` 機能と `aws-smithy-mocks` を入れる
- コレクターの PR(後続の Issue 4a〜4f)は、`Cargo.toml` と `Cargo.lock` を変更しない。変更が必要になったら作業を止めて報告し、単独の PR にする
- まだコードから使わないクレートを先に入れることになるが、Rust の既定では使っていない依存は警告にならず、clippy の `-D warnings` にも影響しない(`unused_crate_dependencies` は既定で無効)
- P1 も `crates/yata-provider-aws/Cargo.toml` に `aws-config` などを入れるため、SDK のクレートの追加を P1 に含め、`crates/yata-provider-aws/Cargo.toml` の変更を 1 つの PR にまとめる
- 既定の機能(`default-features`)を外すかどうかは、依存を追加する PR でビルド時間と合わせて判断する

## グローバルなサービスとリージョン

### IAM などのグローバルなサービス

- IAM の 4 種別と `aws.s3control.account_public_access_block` は `global` とし、パーティションの既定リージョン(`us-east-1`)に 1 回だけ呼ぶ([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の「アカウントとリージョン」)
- `--region` を指定しても、`global` の種別は収集する(全体設計どおり)

### S3 のバケット

- `aws.s3.bucket` は `global` の種別とし、`region` が `global` の `ResourceSet` 1 つに全バケットを入れる。各リソースの `region` にはバケットのリージョン(`ListBuckets` の `BucketRegion`)を入れる
- 理由
  - `ListBuckets` は 1 回の呼び出しで全リージョンのバケットを返すため、リージョンごとに分けると同じ一覧を何度も取るか、1 回の結果を分ける処理が要る
  - 4.4 のように、別のリージョンの証跡からバケットを参照する判定がある。バケットの一覧がアカウントで 1 つの組にまとまっていれば、「一部のリージョンだけ失敗」の状態が生じず、ルールの状態の判定が単純になる
  - スナップショットのマニフェストの例(`aws.s3.bucket` が `global`)とも合う
- `--region` を指定した場合も、すべてのバケットを収集する(`global` の種別と同じ扱い)
- バケットごとの操作(`GetBucketPolicy` など)は、そのバケットのリージョンのエンドポイントに送る。そのリージョンに接続できない場合は、そのキーに失敗の印(`kind` は `region_unavailable`)を置く
- `ListBuckets` は呼び出し元のアカウントが所有するバケットだけを返す。ページングのある形(`MaxBuckets` と `ContinuationToken`)で全件を取り、絞り込みのパラメーター(`BucketRegion`、`Prefix`)は付けない
  - ページングの無い呼び出しは、汎用バケットのクォータが既定の 10,000 のアカウントでしか使えず、それを超えるアカウントでは拒否される(AWS は常にページングの形で呼ぶことを強く推奨している)。`MaxBuckets` はページングの形で呼ぶためのページの大きさで、返るバケットの集合を変えないため、「コレクターの方針」のオプションに含めない
  - API リファレンスの応答の例では、`BucketRegion` はページングの形の応答にだけ現れている。`BucketRegion` を使う点でも、ページングの形で呼ぶ

### CloudTrail の証跡

#### 収集

- `aws.cloudtrail.trail` は `regional` とし、各リージョンで `DescribeTrails` をパラメーターなしで呼ぶ
  - API の既定(`includeShadowTrails` が真、`trailNameList` が空)では、そのリージョンをホームとする証跡に加えて、ほかのリージョンをホームとするマルチリージョンの証跡のコピー(shadow trail)が返る。組織の証跡は、メンバーアカウントでは shadow trail として返る([`DescribeTrails`](https://docs.aws.amazon.com/awscloudtrail/latest/APIReference/API_DescribeTrails.html))
  - 返った証跡は、shadow trail を含めてすべて、そのリージョンの `ResourceSet` に入れる。各リソースの `region` は `DescribeTrails` を呼んだリージョン
- `GetTrailStatus` と `GetEventSelectors` も、shadow trail を含む各証跡について、`DescribeTrails` を呼んだのと同じリージョンで、証跡の名前ではなく `TrailARN` を指定して呼ぶ
  - `GetTrailStatus`: API リファレンスに、shadow trail の状態を取るには ARN を指定すること、メンバーアカウントから組織の証跡を指すには完全な ARN を指定することが書かれている。この呼び方で、shadow trail と組織の証跡の状態を取れる。CloudTrail のユーザーガイドにも、メンバーアカウントは組織の証跡を変更できないが、`get-trail-status` で状態を確かめられることが書かれている
  - `GetEventSelectors`: `TrailName` に ARN を指定できるが、shadow trail と、メンバーアカウントからの組織の証跡についての記述は、API リファレンスに無い(未確認。未決事項「shadow trail のイベントセレクターの取得」)
  - 取れない場合(`TrailNotFoundException`、`OperationNotPermittedException`、`UnsupportedOperationException`、権限不足など)は、「設定が存在しない」には当てず、そのキーに失敗の印を置く。そのリソースを使うルールは `incomplete` になり、黙って `pass` にはならない。各エラーの `kind` への対応は、後続の Issue 4d のコレクターの機能設計書で決める
- 影響: `GetTrailStatus` と `GetEventSelectors` の呼び出しが、「証跡の数 × 証跡が現れるリージョンの数」ずつになる。例えば、マルチリージョンの証跡が 1 つあるアカウントを 17 のリージョンで収集すると、それぞれ 17 回
- `--region` で対象を絞り、証跡のホームリージョンが対象に入っていない場合も、マルチリージョンの証跡は対象のリージョンに shadow trail として現れるため収集される。ホームリージョンが対象外のシングルリージョンの証跡は収集されない(未決事項「全リージョンの判定と `--region`」)

#### ホームリージョンの証跡と shadow trail の見分け

- コレクターは見分けず、どちらも同じ形で入れる
- ルールは、`configuration.DescribeTrails.trailList[0].HomeRegion` がリソースの `region` と同じならホームリージョンの証跡、違えば shadow trail と見分ける(`DescribeTrails` の値は、出力の形を保ち、`trailList` をその証跡の要素 1 つだけの配列にしたもの。[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「列挙の操作の値」)
- 組織の証跡は `configuration.DescribeTrails.trailList[0].IsOrganizationTrail` が真で、ARN のアカウント ID は組織の証跡を作ったアカウントのもの(メンバーアカウントでも同じ ARN。[組織の証跡](https://docs.aws.amazon.com/awscloudtrail/latest/userguide/creating-trail-organization.html))。ARN のアカウント ID と `data.yata.context.account.id` を比べれば、別のアカウントが管理する証跡かが分かる

#### 同じ ARN の証跡と `id`

マルチリージョンの証跡は、同じ ARN で複数のリージョンの `ResourceSet` に現れます。`data` では種別ごとに全リージョンの配列にまとめるため、同じ `id` で `region` だけが違うリソースが並びます。全体設計の「`id` はリソースを一意に識別する値」と食い違うため、次のようにします。

- `id` は、ホームリージョンの証跡も shadow trail も `TrailARN` のままにする
- リソースの識別を「アカウント・リージョン・種別・`id` の組」とし、`id` が一意であるのは `ResourceSet` の中(アカウント × リージョン × 種別)とする。AWS が同じリソースを複数のリージョンに見せる場合に限り、同じ `id` が複数のリージョンに現れてよい。この変更を「全体設計への反映」に挙げる
- 同じ `id` の複数のリソースのうち 1 つを検出の対象にできるよう、次のように扱う(この設計の時点の全体設計の変更は「全体設計への反映」)
  - (ADR-0030 の後)ルールはリソースごとに評価されるため([ADR: ルールをリソースごとに評価する](../adr/ADR-0030-Per-Resource-Rule-Evaluation.ja.md))、同じ `id` のコピーもそれぞれ別のリソースとして評価され、別の結果になる。ルールは `input.region` でどのリージョンのコピーかが分かる。この設計の時点では、`fail` の要素に任意のキー `resource_region` を加えて 1 つのリソースを指す形にしていたが、ADR-0030 で廃止した
  - フィクスチャの期待値 `expected.fail` の要素を、`id` の文字列か、`{"id": ..., "region": ...}` のオブジェクトで書けるようにする。オブジェクトはその `id` とリージョンのリソースだけが不合格になることを、文字列はその `id` のすべてのリソースが不合格になることを期待する。これにより、どのリージョンのコピーが `fail` か(ルールがコピーを正しく見分けたか)を `yata rules test` で確かめられ、案 B(ホームリージョンの証跡だけが `fail`)も期待値として書ける。同じ `id` を文字列とオブジェクトの両方で書く場合と、同じ `{id, region}` を重ねて書く場合は、フィクスチャの誤りとする(「重複なし」の読み方)
  - `incomplete_resources` の要素に `region` を加える
  - 結果の並び順を「重要度 → ルール ID → リソース ID → リージョン」にする
- 全体設計([リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md)、[ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md))に加えるまでは、この節を根拠とする
- 理由: コピーごとに別の `id`(リージョンを加えた形など)を作ると、ARN があるのに ARN を使わない例外になり、コレクターが shadow trail かを見分けて `id` を変えることになる。AWS 自身も、どのリージョンのコピーにも同じ ARN を使う

#### 結果の見せ方

shadow trail は、ホームリージョンの証跡の複製ですが、利用者がその存在を結果から分かるようにします。ルール本体はポリシーのリポジトリで書くため、ここでは判定の形と見せ方の方針を決めます。

| コントロール | 結果の単位 | shadow trail の扱い |
| --- | --- | --- |
| 4.1 | `aws.account.account` の 1 件 | 存在の判定なので、ホームリージョンの証跡か shadow trail かを問わず条件を満たせばよい。重複しても結果は 1 件 |
| 4.2、4.5 | 証跡のコピーごと(リージョンごと) | 下の案 A |
| 4.4 | 証跡のコピーごと | 下の案 A。`S3BucketName` のバケットは、どのコピーからも同じバケットになる |
| 5.2、5.3 | `aws.account.account` の 1 件 | 連なりの起点にどのコピーを使ってもよい。重複しても結果は 1 件 |

- **案 A(推奨)**: すべてのコピーを同じ条件で判定する。設定はコピーの間で同じなので、ホームリージョンの証跡が `fail` なら、shadow trail も各リージョンで `fail` になる
  - 各コピーは別のリソースとして評価され、`fail` の結果のリージョンがそのコピーのリージョンになる。shadow trail(`input.configuration.DescribeTrails.trailList[0].HomeRegion` が `input.region` と違う)の `message` には、shadow trail であることを書く(例: `"Shadow trail of <TrailARN> does not ..."`)。ホームリージョンは ARN に含まれるため、`message` に埋め込んでよいリソースの ID の範囲で利用者に分かる
  - 組織の証跡で、別のアカウントが管理するものは、`message` にそのことを書く(修正は組織の証跡を管理するアカウントで行う)
  - レポートでは、リソース ID(ARN。ホームリージョンを含む)とリージョンの列が違う行として現れ、table の既定の表示(`fail` だけ)でも shadow trail の存在が分かる
  - ホームリージョンが対象外(`--region`)でも、shadow trail で判定されるため、黙って見逃さない
  - 欠点: 1 つの証跡の設定の不備が、現れるリージョンの数だけの検出になり、集計の件数が増える
- **案 B**: ホームリージョンの証跡だけを判定し、shadow trail は、ホームリージョンの証跡が `data.aws.cloudtrail.trail` にあれば `pass`、無ければ(ホームリージョンが対象外の場合)同じ条件で判定する
  - 検出の重複は無いが、shadow trail は `pass` としてだけ現れる(JSON の `results[]` と、table の `--show-passed`)。ルールの書き方も案 A より複雑になる
- どちらにするかは、ルールの Issue 6c で決める(未決事項「shadow trail の検出の見せ方」)

## 認証情報レポート

図: [認証情報レポートの収集の流れ](../diagrams/Diagram-0005-Credential-Report-Collection-Flow.md)

2.10〜2.12 の判定に、IAM の認証情報レポートを使います。レポートの生成(`iam:GenerateCredentialReport`)は、読み取り専用ガードの例外として許可します。判断の詳細は [ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) を参照してください。

- **行とユーザーの対応づけ**: レポートはアカウントに 1 つだが、そのまま 1 つのリソースには入れず、各行をその行のユーザーの `aws.iam.user` のリソースに入れる。IAM ユーザーとそのユーザーの行は 1 対 1 で、「コレクターの方針」の「あるリソースと 1 対 1 で対応する情報は 1 つのリソースにまとめる」原則による対応づけである(絞り込みではない)。これにより、ルールがユーザー単位で判定でき、結果がユーザーごとに作られる。どのユーザーにも当たらない行(ルートユーザーの行 `<root_account>`)は、この範囲ではどのリソースにも入らない。これは「すべて入れる」に対する、入れ先が決まるまでの暫定の不足で、「コレクターの方針」の例外に挙げる(下のルートユーザーの行の項目)
- 生成を許可する理由: `GenerateCredentialReport` は、アカウントに 1 つだけあるレポートを作り直す操作で、IAM のユーザー・認証情報・設定を作成・変更・削除しない。CIS の監査の手順もこのレポートを使う
- 収集の流れ(`aws.iam.user` のコレクターの中で行う)
  1. `ListUsers` でユーザーを列挙する
  2. `GenerateCredentialReport` を呼ぶ。`State` が `COMPLETE` になるまで、間隔を空けて呼び直す(間隔と上限は [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「レポートの生成を待つ手順」)。IAM は 4 時間以内に作ったレポートがあればそれを使う
  3. `GetCredentialReport` でレポート(CSV)を取り、行ごとに解析する
  4. 行の `arn` がユーザーの ARN と一致する行を、そのユーザーの `GetCredentialReport` のキーに入れる
- 構成情報の形
  - `GetCredentialReport` の値は `{"GeneratedTime": "<RFC 3339>", "ReportFormat": "text/csv", "Content": { <列名>: <値>, ... }}` とする。出力の項目はすべて入れ、`Content` だけをそのユーザーの行にする
  - `Content` は API では CSV のテキストの Blob で、そのユーザーの行を列名をキーにしたオブジェクトにして入れる。これは [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「項目名と値の規則」の、テキストの形式が文書化された Blob を機能設計書で決めて構造化する扱いによるもので、「すべて入れる」の例外ではない。また、アカウントに 1 つの操作の出力を行ごとにリソースへ対応づけるため、「リソースごとの取得の操作は出力全体を入れる」の形とも異なる(リソースモデルの「格納の形」)。列名は IAM のドキュメントの CSV の列名(`password_enabled`、`mfa_active` など。snake_case)をそのまま使う。CSV の解析の規則は [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「CSV の解析」
  - 値は CSV の文字列のまま入れる(`"TRUE"`、`"N/A"`、`"no_information"` などを変換しない)。`N/A` などの特別な値を真偽値や `null` に変えると、意味の区別が失われるため
  - `GeneratedTime` を入れ、ルールがレポートの古さを判断できるようにする
- 生成・取得が失敗した場合(権限不足、時間切れ、`ReportNotPresent` など)は、すべてのユーザーの `GetCredentialReport` のキーに失敗の印を置く。`ListUsers` が成功していれば組の収集は成功として扱い、ルールの状態は `incomplete` になる
- レポートに行の無いユーザー(4 時間以内に作られたレポートが使い回され、その後に作られたユーザーなど)は、そのユーザーの `GetCredentialReport` のキーに失敗の印を置く(`null` にしない)。2.10〜2.12 は `incomplete` になる。失敗の印の種類とメッセージは [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「失敗の印」
- ルートユーザーの行(`<root_account>`)は、今回の範囲では使わず、入れ先も今回は決めない(2.4、2.5 は `GetAccountSummary` で判定する)。2.7 を加えるときに、「コレクターの方針」の 1 対 1 の情報をまとめる原則とその範囲に従って入れ先を決める(未決事項「認証情報レポートのルートユーザーの行」)
- `yata permissions` が出す権限に `iam:GenerateCredentialReport` が入る。AWS 管理ポリシー `SecurityAudit` はこの権限を含む

## 振る舞い

この設計は CLI のコマンド・オプションを変えません。この範囲のコレクターとルールがそろったときの振る舞いを示します。

- `yata scan`: 選んだルールの `target_type` / `related_types` から収集する種別が決まる。24 件すべてを選ぶと、上の 16 種別と `aws.account.account` を収集する
- `yata permissions`: 選んだルールの種別の `required_permissions` の和を出す。24 件すべてでは、上の表の権限に、`sts:GetCallerIdentity` と、`--region` を指定しない場合の `ec2:DescribeRegions` を加えたものになる。`iam:GenerateCredentialReport` は Describe / List / Get の形ではないが、例外リストにある操作として含める。この操作を使うことを出力でどう示すか(標準エラー出力の注意、ポリシーの `Sid` で分けるなど)は未決事項にする
- エラー時
  - リソースを列挙する操作の権限不足は、その組の `CollectionError`(`access_denied`)になる。その種別を使うルールは、全リージョンで失敗なら `skipped`(`collection_failed`)、一部なら `incomplete` になる
  - バケットごとの操作、証跡ごとの `GetTrailStatus` / `GetEventSelectors`(shadow trail を含む)、認証情報レポートなどの失敗は失敗の印になり、ルールは `incomplete` になる
  - ルールの状態は、ルールが宣言した種別のリソースに失敗の印があるかで決まり、ルールがどの操作のキーを使うかは見ない([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「状態の判定順」)。このため、失敗の影響は同じ種別を使うほかのルールにも及ぶ
    - 認証情報レポートの失敗で、2.10〜2.12 に加えて 2.13(`ListAttachedUserPolicies` などだけを使う)も `incomplete` になる。逆に `ListAttachedUserPolicies` の失敗で 2.10〜2.12 も `incomplete` になる
    - ある証跡(shadow trail を含む)の `GetEventSelectors` の失敗で、4.1・5.2・5.3 に加えて、`DescribeTrails` だけを使う 4.2・4.5 と 4.4 も `incomplete` になる
    - `iam:GenerateCredentialReport` を付けない権限で実行すると、`aws.iam.user` を使うルール(2.10〜2.13)がすべて `incomplete` になる
    - あるバケットの `GetBucketPolicy` の失敗で、3.1.1 に加えて 3.1.4・4.4(証跡と無関係なバケットでも)が `incomplete` になる
  - この波及は、結果を誤って `pass` にする方向には働かず、利用者は `incomplete_resources` で原因の操作を確かめられるため、最初のリリースでは許容する。操作の単位で状態を決めるか(ルールのメタデータに使う操作を宣言するなど)は、未決事項にする
  - いずれもスキャンは不完全で、終了コードは [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) に従う(`--fail-on` 以上の検出が無ければ 4)

## 構成

- コレクターは 1 リソース種別 1 ファイルで、次の場所に置く([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の「コレクター」)

```
crates/yata-provider-aws/src/collectors/
├── iam/{account_summary,password_policy,user,policy}.rs
├── s3/bucket.rs
├── s3control/account_public_access_block.rs
├── cloudtrail/trail.rs
├── cloudwatchlogs/metric_filter.rs
├── cloudwatch/metric_alarm.rs
├── sns/topic.rs
└── ec2/{security_group,network_acl,instance,vpc,flow_log,ebs_encryption_default}.rs
```

- 認証情報レポートの生成・取得と行の対応づけは `collectors/iam/user.rs` に、CSV の解析はサービスに依存しない関数として `support/csv.rs` に置く([IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「構成」)
- 読み取り専用ガードの例外リストへの `IAM` / `GenerateCredentialReport` の追加は、`yata-provider-aws/src/support/` の例外リストに理由とともに書く(単独の PR)
- 種別ごとの構成情報の詳細(全項目、`null` にするエラー、`id` の形式)は、サービスごとのコレクターの機能設計書(`docs/features/`)に書く
- ルール本体は、ポリシーのリポジトリに置く。ディレクトリはサービスの単位(`iam/`、`s3/`、`cloudtrail/`、`monitoring/`、`ec2/` など)を想定し、詳細はポリシーのリポジトリで決める

## テスト方針

- このリポジトリのテストは全体設計の [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) に従う。実 AWS には接続しない
- 各コレクター: `aws-smithy-mocks` で SDK の応答をモックし、構成情報の形(操作名のキー、項目名、値の変換、`null`、失敗の印)を確かめ、出力を期待するファイルと比べて固定する
- 種別名: 各コレクターの種別が名前の規則(3 階層、`[a-z][a-z0-9_]*`、予約語なし)に合うことを `yata-provider-aws` のテストで確かめる
- 読み取り専用: `required_permissions` の形の検査で、例外リストにある `iam:GenerateCredentialReport` だけを許すことを確かめる。ガードが `GenerateCredentialReport` を通し、ほかの `Generate*` の操作(例: `GenerateServiceLastAccessedDetails`)を拒否することを確かめる
- 認証情報レポート: モックで `State` が `STARTED` → `COMPLETE` になる応答、生成の失敗、`GetCredentialReport` の失敗、レポートに行の無いユーザーを用意し、構成情報と失敗の印を確かめる。CSV はダミー値だけで作る
- コレクターの方針: 伏せる処理が無いため、伏せた値を確かめるテストは置かない。代わりに次を確かめる
  - `aws-smithy-mocks` のリクエストの照合で、一覧の操作を既定のパラメーターで呼んでいること(`DescribeTrails` に `includeShadowTrails` を付けない、`ListPolicies` に `Scope` / `OnlyAttached` を付けない、など)
  - モックの応答の件数と、構成情報に入ったリソースの件数が同じこと(絞り込んでいない)。認証情報レポートは、ルートユーザーの行を除いた行の数と、`GetCredentialReport` のキーに行が入ったユーザーの数を比べる。`aws.sns.topic` の `Endpoint` などが、ダミーの値のまま入ること
- CloudTrail の証跡: 同じ ARN の証跡(ホームリージョンの証跡と shadow trail)を複数のリージョンのモックで返し、各リージョンの `ResourceSet` に入ること、`GetTrailStatus` / `GetEventSelectors` を `TrailARN` で呼ぶこと、shadow trail での呼び出しの失敗が失敗の印になることを確かめる
- ルール: ポリシーのリポジトリで、各ルールに `pass_` / `fail_` のフィクスチャを置き、`yata rules test` で確かめる。リージョンをまたぐルール(4.1、5.2、5.3、6.1.1)は、複数のリージョンのリソースを含むフィクスチャを用意する。証跡を使うルール(4.1、4.2、4.4、4.5、5.2、5.3)は、同じ ARN の証跡がホームリージョンと shadow trail として現れるフィクスチャ、shadow trail だけが現れるフィクスチャ(ホームリージョンが対象外、メンバーアカウントの組織の証跡)を用意する。失敗の印を含むフィクスチャ(`incomplete`)も用意する

## 全体設計への反映

次の点は `docs/architecture/` の変更が必要です。この設計の PR では変更せず、後続の Issue(下の「後続の Issue」の 1)で単独の PR にします。(この表のうち `fail` の要素の `resource_region` に関する変更は、その後 [ADR: ルールをリソースごとに評価する](../adr/ADR-0030-Per-Resource-Rule-Evaluation.ja.md) で廃止しました。[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の (3) と (9) の Blob の扱いと認証情報レポートの値の形は、現在は上の「コレクターの方針」と「認証情報レポート」のとおりです。)

| ページ | 変更 |
| --- | --- |
| [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) | 未決事項「Describe / List / Get 以外の読み取り操作」を解決済みにする。例外リストの初期の内容を「IAM / `GenerateCredentialReport` → `iam:GenerateCredentialReport`」とし、理由を書く。テストの「`required_permissions` が `<service>:(Describe|List|Get)...` の形」に「または例外リストにある操作の IAM アクション名」を加える。「コレクター」の「秘密に当たる項目を伏せる一覧は、コレクターのファイルに置く」を消し、コレクターは既定のパラメーターで呼ぶこと([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の方針への参照)を書く |
| [ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md) | 「中央の列挙を許すもの」の例外リストの「原則として空」を、例外リストの内容に合わせて直す。未決事項「先に追加する AWS SDK のサービス」を解決済みにする |
| [初期依存クレート](../architecture/Architecture-Initial-Dependencies.ja.md) | `aws-sdk-<service>` の行を、追加する 7 つのクレートに置き換える |
| [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) | (1) 「項目名は AWS API リファレンスの名前(PascalCase のメンバー名)」を、「サービスのモデルのメンバー名(AWS CLI の JSON 出力と同じ名前)。多くは PascalCase だが、CloudWatch Logs などは camelCase」に直す。(2) 失敗の印の「キー名 `yata_error` は英小文字で始まるので AWS API の項目名とは重ならない」という理由を、「失敗の印は操作のキーの値の最上位に置き、AWS のどの操作の出力にも最上位の項目 `yata_error` は無い」に書き直す(camelCase の項目と、認証情報レポートの snake_case の列名を入れるため)。(3) `GetCredentialReport` の扱いを加える: アカウントに 1 つのレポート(Blob の CSV)の各行を、(6) の 1 対 1 の情報をまとめる原則による対応づけとして、その行のユーザーのリソースに `{"GeneratedTime": ..., "Content": {<列名>: <CSV の文字列>}}` として入れる。現在の「バイナリは入れない」規則の例外になり(この規則の扱いは (9) で決める)、「リソースごとの取得の操作は出力全体」の形とも異なることを書く。(4) 「Resource の項目」の `id` に、応答に ARN が無くても AWS が ARN の形式を定義している種別は、その形式で作った ARN を使うことを明記する(「ARN が無いリソースの id」は、AWS が ARN を定義していない種別だけに使う)。(5) S3 のバケットの `ResourceSet` は、この設計で `global` に決めたことへの参照を加える。(6) 「方針」と「格納の形」に、上の「コレクターの方針」(既定のパラメーターで呼ぶ、絞り込まず・置き換えずにすべて入れる、許す変換は形式の変換だけ、あるリソースと 1 対 1 で対応する情報は API が分かれていても 1 つのリソースにまとめる(リソースごとの取得の操作を操作のキーで入れる既存の形と、認証情報レポートの行の対応づけを、この原則の例として示す)、まとめる対象はリソースに付随する情報に限り、アカウントに 1 つだけの設定は `aws.account.account` にまとめず別の種別にする(理由を含む)、例外(`aws.account.account` の `Account` だけ、認証情報レポートのルートユーザーの行の暫定の不足))を加える。`aws.account.account` を `GetCallerIdentity` の `Account` だけにする規則は「SDK の応答をすべて入れる方針の例外」の文言のまま残し、理由に「コレクターの収集ではなく、`yata-provider-aws` がアカウントを識別するために作るもの」を加える。(7) 「秘密の値の扱い」から伏せる規則(`"<redacted>"` への置き換え、伏せる項目の一覧、伏せたことのテスト、あとから伏せる項目を足す場合の例外)をなくし、拒否リストの操作を呼ばないことだけを残す。あわせて、「構成情報の互換性」の伏せる項目の追加の例外、「SDK の応答を格納する方式」の「秘密の値の伏せ方」「秘密の値・バイナリを除く」、「実装上の配置」の「伏せる項目の一覧」を直す。(8) 「Resource の項目」の `id` の一意性を「`ResourceSet` の中で一意。AWS が同じリソースを複数のリージョンに見せる場合(CloudTrail の shadow trail)は、同じ `id` が複数のリージョンに現れてよい。リソースはアカウント・リージョン・種別・`id` の組で識別する」に改め、「ポリシーエンジンへの渡し方」に、配列に同じ `id` が `region` 違いで並びうることを書く。(9) 方針と食い違う既存の規則(バイナリを入れない)の扱いを決めて反映する(未決事項「方針と食い違う既存の規則」) |
| [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) | 秘密の値が `"<redacted>"` に伏せられていることを確かめるテストをなくす。代わりに、既定のパラメーターで呼んでいること(リクエストの照合)と、応答の件数どおりに入っていること(絞り込んでいない)を確かめる項目を加える |
| [出力形式](../architecture/Architecture-Output-Formats.ja.md) | 「値の扱い」の「秘密に当たる値は、収集の時点で `"<redacted>"` に伏せてある」を、「構成情報は伏せずに入っているため、`actual` に機微な値が入りうる。外部に出すときは `--omit-values` を使う」に直す。結果の並び順を「重要度 → ルール ID → リソース ID → リージョン」にし、`incomplete_resources` の要素に `region` を加える(上の「同じ ARN の証跡と `id`」) |
| [ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) | 「結果」の「秘密の値を伏せる項目の追加は、安全上の修正として同じ種別名のまま行う」の例外に、伏せる規則をなくしたことへの参照を加える。フィクスチャの `expected.fail` の要素を、`id` の文字列か `{"id": ..., "region": ...}` のオブジェクトで書けるようにし、同じ `id` を両方の形で書くこと・同じ `{id, region}` を重ねることはフィクスチャの誤りとする(上の「同じ ARN の証跡と `id`」)。`fail` の要素のキーに `resource_region` を加える |
| [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) | `yata permissions` の出力で、Describe / List / Get 以外の操作(`iam:GenerateCredentialReport`)を使うことをどう示すか。この行だけは後続の Issue 1 ではなく、示し方を決める前提の P5 で反映する(未決事項) |
| [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) | 未決事項「対象とする CIS のバージョン」を解決済みにし、この設計へのリンクを置く。「fail の規約」に任意のキー `resource_region` を加える(指定すればその `id` とリージョンのリソースだけ、省略すればその `id` のすべてのリソースの検出。文字列でない場合と、その `id` とリージョンの組のリソースが `target_type` に無い場合は評価のエラー)。「フィクスチャの実行」の `expected.fail` の比べ方を、`id` の文字列と `{"id", "region"}` のオブジェクトの両方に合わせる。「リソース単位の結果」を、`id` とリージョンでリソースに対応づける形に直し、`IncompleteResource` に `region` を加える(上の「同じ ARN の証跡と `id`」) |
| [ADR: AWS SDK for Rust と読み取り専用ガード](../adr/ADR-0001-AWS-SDK-and-Read-Only-Guard.ja.md) | 「結果」の `GenerateCredentialReport` の記述に、ADR-0005 で許可したことへの参照を加える |

## 後続の Issue

図: [後続の Issue の依存関係](../diagrams/Diagram-0005-Follow-up-Issue-Dependencies.md)

| # | Issue | 内容 | 依存 | 並行 |
| --- | --- | --- | --- | --- |
| 1 | 全体設計の更新(コレクターの方針(1 対 1 の情報をまとめる原則とその範囲を含む)、伏せる規則の廃止、`id` の一意性と `resource_region`、フィクスチャの期待値の `{id, region}`、認証情報レポートの行の対応づけなど) | 上の「全体設計への反映」の変更。未決事項「方針と食い違う既存の規則」もここで決める。`docs/architecture/` と ADR(ADR-0001 のフィクスチャの期待値の形の変更、参照の追記)だけの単独の PR | この設計のマージ | 2 と並行できる |
| 2 | AWS SDK の依存の追加 | ルートの `[workspace.dependencies]` に 7 つのクレートを追加し、`crates/yata-provider-aws/Cargo.toml` の `[dependencies]` と `[dev-dependencies]`(`test-util`、`aws-smithy-mocks`)に 9 つの SDK のクレートを入れ、`Cargo.lock` を 1 回で更新する(上の「依存の入れ方」)。共有ファイルの変更を 1 回にまとめるため、独立した Issue にせず P1 に含める | この設計のマージ(P1 の一部として行う) | 1 と並行できる |
| 3 | 読み取り専用ガードの例外の追加 | `support/` の例外リストに `GenerateCredentialReport` を加え、テストを直す。ガードがまだ無ければ P1 に含める | 1、P1 | 4a〜4f と並行できる |
| 4a | IAM のコレクター | `aws.iam.account_summary`、`aws.iam.password_policy`、`aws.iam.policy`、`aws.iam.user`(認証情報レポートを除く) | 2、P1 | 4b〜4f と並行できる |
| 4b | 認証情報レポートの収集 | `aws.iam.user` に `GetCredentialReport` のキーを加える(項目の追加なので互換性を壊さない) | 3、4a | 4c〜4f と並行できる |
| 4c | S3 のコレクター | `aws.s3.bucket`、`aws.s3control.account_public_access_block` | 2、P1 | 並行できる |
| 4d | CloudTrail のコレクター | `aws.cloudtrail.trail`(同じ ARN の証跡を複数のリージョンに出すため、1 で改める `id` の一意性に従う) | 1、2、P1 | 並行できる |
| 4e | 監視のコレクター | `aws.cloudwatchlogs.metric_filter`、`aws.cloudwatch.metric_alarm`、`aws.sns.topic` | 2、P1 | 並行できる |
| 4f | EC2/VPC のコレクター | `aws.ec2.security_group`、`aws.ec2.network_acl`、`aws.ec2.instance`、`aws.ec2.vpc`、`aws.ec2.flow_log`、`aws.ec2.ebs_encryption_default`。大きければネットワーク(前の 4 つ+フローログ)とそれ以外に分ける | 2(`crates/yata-provider-aws/Cargo.toml` への `aws-sdk-ec2` の追加を 2 でまとめて行うため)、P1 | 並行できる |
| 5 | CIS v7.1.0 の本文の確認 | 番号・題名・自動化の区分を v7.1.0 の本文で確かめ、この設計書の表を直す | なし | すべてと並行できる |
| 6a〜6e | ポリシーのリポジトリでのルールの作成 | セクションごとに分ける: IAM(9 件)、S3(2 件)、CloudTrail・ログ(5 件)、監視(2 件)、EC2/VPC(6 件)。共通の関数(IAM のポリシー文書の解析、CIDR とポートの判定、監視の連なりの判定)は、`lib/` の単独の PR を先に出す | 着手: P2、P3、対応するコレクターの機能設計書のマージ(構成情報の形が決まるため)。6c・6d・6e は P4 も。6c は、1 で決めた `resource_region` を P2 が実装していることも(ADR-0030 の後は、リソースごとの評価が実装されていること。6a〜6e のすべての前提になる)。公開: 5 の完了と、対応するコレクターを含む `yata` のリリースの後(マニフェストの `min_yata_version` を上げる) | セクションの間で並行できる |
| 7 | 利用者ガイドへの記載 | 最小権限の案内(`yata permissions`、`SecurityAudit`)と、`iam:GenerateCredentialReport` を使うこと、その影響([ADR-0005](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md))を書く。P5 で決めた `permissions` の出力の示し方を反映する | 1、P5 | 4a〜6e と並行できる |

### 前提の Issue(この設計の範囲外)

| # | Issue | どの行の依存か |
| --- | --- | --- |
| P1 | AWS プロバイダの基盤(`Collector` トレイト、build.rs、クライアントの生成、読み取り専用ガード、`aws-config` などの依存) | 2、3、4a〜4f |
| P2 | ポリシーエンジンと `yata rules test` の実装(フィクスチャでルールを確かめるため)。1 で決めた `id` の一意性の変更、`fail` の `resource_region`(誤りの扱いを含む。ADR-0030 で廃止し、リソースごとの評価に置き換えた)、`expected.fail` の `{id, region}` の形、`IncompleteResource` の `region` を含む | 6a〜6e |
| P3 | ポリシーのリポジトリの作成 | 6a〜6e |
| P4 | 全体設計の Issue「全リージョンの判定と `--region`」(下の未決事項) | 6c、6d、6e |
| P5 | `yata permissions` を実装する Issue(Describe / List / Get 以外の操作の示し方を決める。下の未決事項) | 7 |

- 4a〜4f は `collectors/<service>/` の別々のファイルに書き、`Cargo.toml` と `Cargo.lock` は 2 でまとめて変更済みのため、互いに衝突しない。各 Issue は、まずコレクターの機能設計書を書き、マージしてから実装する
- 6a〜6e のフィクスチャは、コレクターの機能設計書で構成情報の形が決まれば、コレクターの実装を待たずに書き始められる

## 出典

- CIS Amazon Web Services Benchmarks(版の掲載): <https://www.cisecurity.org/benchmark/amazon_web_services>
- CIS Benchmarks April 2026 Update(v7.0.0 の公開のお知らせ): <https://www.cisecurity.org/insights/blog/cis-benchmarks-april-2026-update>
- v7.0.0 のコントロールの番号と題名(第三者の監査ファイル): <https://www.tenable.com/audits/CIS_Amazon_Web_Services_Foundations_v7.0.0_L1>、<https://www.tenable.com/audits/CIS_Amazon_Web_Services_Foundations_v7.0.0_L2>
- IAM の認証情報レポート(生成の間隔、列、必要な権限): <https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_getting-report.html>
- `GenerateCredentialReport`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GenerateCredentialReport.html>
- `GetCredentialReport`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetCredentialReport.html>
- `GetAccountSummary`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetAccountSummary.html>
- `GetAccountPasswordPolicy`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetAccountPasswordPolicy.html>
- `ListPolicies`(既定ではすべてのポリシーを返す、`AttachmentCount`): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_ListPolicies.html>
- `ListBuckets`(`BucketRegion`、ページング、ページングの無い呼び出しの制限、`s3:ListAllMyBuckets`): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBuckets.html>
- S3 Control の `GetPublicAccessBlock`(アカウント単位): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_control_GetPublicAccessBlock.html>
- `DescribeTrails`(`includeShadowTrails` の既定が真、shadow trail、組織の証跡): <https://docs.aws.amazon.com/awscloudtrail/latest/APIReference/API_DescribeTrails.html>
- `GetTrailStatus`(shadow trail とメンバーアカウントからの組織の証跡は ARN で指定する): <https://docs.aws.amazon.com/awscloudtrail/latest/APIReference/API_GetTrailStatus.html>
- `GetEventSelectors`(`TrailName` に ARN を指定できる): <https://docs.aws.amazon.com/awscloudtrail/latest/APIReference/API_GetEventSelectors.html>
- 組織の証跡(メンバーアカウントでの見え方、同じ ARN、メンバーアカウントは変更できない): <https://docs.aws.amazon.com/awscloudtrail/latest/userguide/creating-trail-organization.html>
- `DescribeAlarms`(`AlarmTypes` を省くとメトリクスのアラームだけを返す): <https://docs.aws.amazon.com/AmazonCloudWatch/latest/APIReference/API_DescribeAlarms.html>
- `DescribeMetricFilters`(項目名が camelCase): <https://docs.aws.amazon.com/AmazonCloudWatchLogs/latest/APIReference/API_DescribeMetricFilters.html>
- ARN の形式と、サービスごとの形式の調べ方(VPC の ARN の例、Service Authorization Reference の「Resource types」の表): <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference-arns.html>
- EC2 のリソースの ARN の形式(Service Authorization Reference): <https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazonec2.html>
- `GetEbsEncryptionByDefault`(リージョン単位の設定): <https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_GetEbsEncryptionByDefault.html>
- AWS 管理ポリシー `SecurityAudit`(上の権限をすべて含む): <https://docs.aws.amazon.com/aws-managed-policy/latest/reference/SecurityAudit.html>
- SDK のクレート(ライセンス): <https://crates.io/crates/aws-sdk-cloudwatchlogs>、<https://crates.io/crates/aws-sdk-s3control>、リポジトリ <https://github.com/awslabs/aws-sdk-rust>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| CIS v7.1.0 の本文の確認 | コントロールの番号・題名・自動化の区分・L1 / L2 を v7.1.0 の本文で確かめる。v7.1.0 の公開日も確かめる。v7.0.0 の 70 件以外のコントロールがあれば、含める・除くを決める。ルールはこの確認の後に `version` を `7.1.0` として公開する。本文を確認する手段が得られない場合に、v7.0.0 を対象として公開するかは、この Issue で決める。v7.1.0 で加わったコントロールを範囲に入れるために新しい SDK のサービスが要る場合は、Issue 2 とは別に、共有ファイル(ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock`)の単独の PR を出す | 後続の Issue 5(ルールの公開の前) |
| 操作の単位でのルールの状態 | 失敗の印の影響が、同じ種別を使うほかのルールにも及ぶ(「振る舞い」のエラー時)。ルールが使う操作をメタデータで宣言し、その操作の失敗だけで `incomplete` にするか。最初のリリースでは許容する | 最初のリリースの後、全体設計の Issue |
| `yata permissions` での例外の操作の示し方 | 出力は IAM ポリシーの JSON で、注記を書く場所が無い。標準エラー出力に注意を出す、例外の操作を別の文(`Sid` で区別)に分ける、などから選ぶ。CLI の出力を変える場合は [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) を更新する | `yata permissions` を実装する Issue(前提の P5) |
| 全リージョンの判定と `--region` | `--region` で対象を絞ると、4.1・6.1.1 などの「全リージョン」の判定は対象のリージョンの範囲になり、対象外のリージョンの不備を見逃す。マルチリージョンの証跡は、ホームリージョンが対象外でも shadow trail として収集されるが、ホームリージョンが対象外のシングルリージョンの証跡は収集されない。ロググループのリージョンが対象外だと、5.2・5.3 の連なりをたどれず `fail` になる。アカウントで有効なリージョンの一覧を `data.yata.context` に入れるか、レポートで注意を出すか | ルールの Issue(6c、6d、6e)の前に、全体設計の Issue(前提の P4)として決める |
| レポートに行の無いユーザー | 解決済み: 失敗の印にする(`null` にしない)。[IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「行とユーザーの対応づけ」で決めた | 解決済み(後続の Issue 4b) |
| 認証情報レポートの待ち方 | 解決済み: 呼び直す間隔と上限は [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「レポートの生成を待つ手順」で決めた(時間切れになった場合は、上の「認証情報レポート」のとおり失敗の印を置く) | 解決済み(後続の Issue 4b) |
| shadow trail のイベントセレクターの取得 | `GetEventSelectors` を shadow trail の ARN で(別のリージョンをホームとする証跡、メンバーアカウントからの組織の証跡)呼んで取れるかは、API リファレンスに記述が無い(`GetTrailStatus` は取れることを確かめた)。確かめるには実際の AWS アカウントでの確認が要る。常に特定のエラーになると分かった場合に、そのエラーを失敗の印のままにするか(その場合、ルールの状態は種別の単位で決まるため、shadow trail がある環境では `aws.cloudtrail.trail` を使うすべてのルール(4.1・4.2・4.4・4.5・5.2・5.3)が常に `incomplete` になる)、別の扱いにするか | 後続の Issue 4d |
| shadow trail の検出の見せ方 | 4.2・4.4・4.5 で、上の「結果の見せ方」の案 A(すべてのコピーを判定し、shadow trail であることを `message` に書く)と案 B(ホームリージョンの証跡だけを判定する)のどちらにするか。推奨は案 A | ルールの Issue(6c) |
| 別のアカウントにある証跡の送り先 | 4.4 で証跡のバケットが、5.2・5.3 で証跡のロググループが、別のアカウント(集約用のアカウント、組織の証跡を作ったアカウント)にあり、`data` に無い場合の判定と `message` | ルールの Issue(6c、6d) |
| 2.14 の対象 | AWS 管理ポリシー(`AdministratorAccess` など)のアタッチを `fail` にするか、カスタマー管理ポリシーだけにするか。アタッチされていないポリシーは、ルールで `AttachmentCount` を見て除く。インラインポリシーを対象にするか(対象にするなら `GetUserPolicy` などの収集を加える) | ルールの Issue(6a) |
| 方針と食い違う既存の規則 | 解決済み: Blob は base64 の文字列で入れ、テキストの形式が文書化された Blob は機能設計書で決めて構造化してよい([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「項目名と値の規則」、上の「コレクターの方針」)。認証情報レポートの `Content` は例外ではない。`aws.account.account` の構成情報を `GetCallerIdentity` の `Account` だけにする規則は、「コレクターの方針」の例外として残す | 解決済み(後続の Issue 1) |
| `DescribeInstances` の予約の項目 | 解決済み: `DescribeInstances` の値は出力の形を保ち、経路にある配列を要素 1 つだけにした `{"Reservations": [{"ReservationId": ..., "OwnerId": ..., "RequesterId": ..., "Groups": [...], "Instances": [<そのインスタンスの要素>]}]}` とし、予約の項目はその予約のどのインスタンスの値にも入る([ADR: 列挙の操作の値に出力の形を保つ](../adr/ADR-0039-Keep-Output-Shape-for-Listing-Operation-Values.ja.md)、上の「`aws.ec2.instance`」) | 解決済み(ADR-0039) |
| 認証情報レポートのルートユーザーの行 | レポートの `<root_account>` の行をどの種別のリソースに入れるか(この範囲ではどのリソースにも入れない)。「コレクターの方針」の 1 対 1 の情報をまとめる原則とその範囲(リソースに付随する情報に限り、アカウントの設定は `aws.account.account` にまとめない)に従って入れ先を決める | 2.7 を範囲に加えるとき |
| 監視のフィルターのパターンの比べ方 | 5.2、5.3 で、メトリクスフィルターのパターンを CIS の例と文字列で比べるか、構成要素で比べるか | ルールの Issue(6d) |
| 次の段階の範囲 | 区分 C・D のコントロール(AWS Config、KMS、IAM Access Analyzer、RDS、EFS、Account、Security Hub、残りの監視など)をどの順に加えるか | 最初のリリースの後 |
