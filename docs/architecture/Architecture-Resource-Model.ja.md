# リソースモデル

> This Japanese version is the authoritative source. The English version (`Architecture-Resource-Model.md`) has not been written yet.

収集した AWS のリソースをどう表現し、どの単位でまとめ、どう保存し(スナップショット)、ポリシーエンジンにどう渡すかを定めます。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## 方針

- リソースは「共通の項目」と「種別ごとの構成情報(`configuration`)」に分ける
- 共通の項目は Rust の型(`yata-core`)で定義する。構成情報は JSON の値(`serde_json::Value`)として持ち、Rust の共通の型には入れない
  - 種別ごとに Rust の型を `yata-core` に足すと、全機能が `yata-core` を書き換えることになり、並列開発で衝突するため
- 構成情報には、SDK が返した値をすべて入れる。秘密の値も伏せない(下の「コレクターの方針」「秘密の値の扱い」)
- 収集した結果は「アカウント × リージョン × リソース種別」を 1 単位(`ResourceSet`)としてまとめる

### コレクターの方針

コレクターは次の方針に従います。最初に適用した範囲とその詳細は [最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の「コレクターの方針」を参照してください。

- **コレクターは情報を集めるだけにする**。合否の判定と、判定に向けた絞り込み(CloudTrail の shadow trail の見分け、アタッチされた IAM ポリシーだけを対象にするなど)は Rego のルールで行う
- **API は既定のパラメーターで呼ぶ**。結果を絞り込む・広げるオプション(`Filters`、`Scope`、`includeShadowTrails` など)を付けない。次のものは「オプション」に含めない
  - ページングのためのパラメーター: 続きを取るトークン(`NextToken`、`Marker`、`ContinuationToken` など)と、ページングの形で呼ぶためのページの大きさ(`ListBuckets` の `MaxBuckets` など)。どちらも返るリソースの集合を変えない
  - 必須の識別子: リソースごとの操作で対象を指すもの(`GetBucketPolicy` の `Bucket`、`GetPolicyVersion` の `PolicyArn` と `VersionId`、`GetTrailStatus` の `Name` など)
- **返った結果は、絞り込まず・置き換えずにすべて入れる**
- **許す変換は、値の意味・件数を変えない形式の変換だけ**: SDK の型から JSON への変換、日時の RFC 3339 の文字列への変換、列挙の値の文字列への変換、バイナリ(Blob)の base64 の文字列への変換と、テキストの形式が定められた Blob の構造化、ポリシー文書の URL デコードと JSON のパース(下の「項目名と値の規則」)。ページングの項目と応答のメタデータを含めないことは、下の「格納の形」のとおり
- **秘密の値も伏せない**。ただし、秘密の値の取得そのものが目的の API(読み取り専用ガードの拒否リストの操作)は呼ばない(下の「秘密の値の扱い」)
- **あるリソースと 1 対 1 で対応する情報は、呼び出す API が分かれていても 1 つのリソースにまとめる**
  - リソースごとに呼ぶ取得の操作(`GetBucketPolicy`、`GetTrailStatus` など)の出力を、操作のキーで一覧の操作と同じリソースに入れる形(下の「格納の形」)は、この原則の一つである
  - アカウントに 1 つの操作の出力でも、その中の要素がリソースと 1 対 1 で対応するなら、要素ごとに対応するリソースに入れる。例: IAM の認証情報レポート(`GetCredentialReport`)の各行を、その行のユーザーの `aws.iam.user` に入れる(下の「格納の形」)。これは絞り込みではなく、この原則による対応づけで、行の内容は変えずに入れる
- **まとめる対象は、あるリソースに付随する情報に限る**。アカウント(`aws.account.account`)は、この原則でいう「あるリソース」に含めない。アカウントに 1 つだけの設定(IAM のアカウントの概要、パスワードポリシー、S3 のアカウントのパブリックアクセスブロックなど)は、`aws.account.account` にまとめず、それぞれ別の種別にする
  - 理由: サービスの異なる設定を 1 つの種別にまとめると、ある設定の収集の失敗がその種別を使うすべてのルールに波及し([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「状態の判定順」)、関係の無い判定まで `incomplete` になる。また、1 つのコレクターがサービスをまたぎ、1 リソース種別 1 ファイルの構成と、サービスごとの並行の作業が崩れる
  - リソースと 1 対多で対応する情報(EC2 の `DescribeInstances` の予約とインスタンスなど)は、この原則の対象ではない。扱いはその種別の機能設計書で決める
- **例外**: 「すべて入れる」に対する例外は、次のものだけとする
  - `aws.account.account` には `GetCallerIdentity` の `Account` だけを入れる(下の「アカウントのリソース」)
  - 認証情報レポートのルートユーザーの行(`<root_account>`)は、対応するリソースが無いため、入れ先が決まるまでどのリソースにも入れない(入れ先が決まるまでの暫定の不足。[最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の未決事項「認証情報レポートのルートユーザーの行」)

## 名前と区切りの文字

### アカウントキー

アカウントは、プロバイダとアカウント ID を `-` でつないだ文字列(アカウントキー)で表します。

- 形式: `<provider>-<account_id>`(例: `aws-123456789012`)
- `<provider>` は `[a-z][a-z0-9]*` に限る(`-` と `_` を含めない。リソース種別の最初の部分と同じ値)。アカウントキーは最初の `-` で分割すると、プロバイダとアカウント ID に戻せる
- `<account_id>` はプロバイダごとの ID をそのまま使う。AWS では 12 桁の数字。将来、GCP のプロジェクト ID や Azure のサブスクリプション ID(GUID)のように `-` を含む ID を扱っても、最初の `-` で分割するため曖昧にならない
- 区切りの文字の比較

| 候補 | 判断 |
| --- | --- |
| `-`(採用) | プロバイダの文字を `[a-z][a-z0-9]*` に限れば、最初の `-` で一意に分割できる。ファイル名・ディレクトリ名に使え、読みやすい |
| `_` | 分割の点は `-` と同じ。採る理由が特に無い |
| `.` | リソース種別(`aws.s3.bucket`)やルール ID と同じ文字になり、`aws.123456789012` がリソース種別のように見えて紛らわしい |
| `:` | Windows のファイル名・ディレクトリ名に使えず、スナップショットのディレクトリ名にできない |

### パーティション

- `provider` は `aws` に固定する。AWS のパーティション(`aws`、`aws-cn`、`aws-us-gov`)は provider とは別の概念として扱う
- 対応するパーティションは `aws` だけとし、パーティションの情報はモデルに持たない。`sts:GetCallerIdentity` で得た ARN のパーティションが `aws` でなければ、収集を始めずに終了する(終了コード 3)
- `aws` 以外のパーティションへの対応は範囲外とし、未決事項に置く。対応するときは、AWS に固有の情報として項目を追加する

### リソース種別

- 形式: `<provider>.<service>.<resource>` の 3 階層に固定する(例: `aws.s3.bucket`、`aws.iam.user`、`aws.ec2.security_group`)
- 各部分は `[a-z][a-z0-9_]*`(英小文字で始まる)とし、Rego の予約語(`as`、`contains`、`default`、`else`、`every`、`false`、`if`、`import`、`in`、`not`、`null`、`package`、`some`、`true`、`with`)を使わない。`input.aws.<service>.<resource>` をドット区切りの形で必ず書けるようにするため([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「名前の規則」と同じ規則)
  - ルールのメタデータで規則に合わない種別を書いた場合は、メタデータの誤り(ルール単位の失敗。`rules test` で 1、それ以外のコマンドで 2)
  - このリポジトリのコレクターが作る種別は、`yata-provider-aws` のテストで規則に合うことを確かめる
- 3 階層に固定するので、ある種別名が別の種別名の前の部分になることは無い(`aws.s3.bucket` と `aws.s3.bucket.policy` は作れない)。これにより、Rego で `input.aws.s3.bucket` のように入れ子でたどれる(下の「ポリシーエンジンへの渡し方」)
- `<service>` は AWS SDK のサービス名(`aws-sdk-<service>` の `<service>`)に合わせる
- 1 つのリソース種別は 1 つのコレクターだけが作る
- アカウント単位・リージョン単位の設定(例: パスワードポリシー、EBS のデフォルト暗号化)も 1 件のリソースとして表す(例: `aws.iam.password_policy`、`aws.ec2.ebs_encryption_default`)。設定が存在しない場合も、存在しないことを構成情報で表したリソースを 1 件作り、ルールが「未設定」を判定できるようにする
- 特別な種別 `aws.account.account`(下の「アカウントのリソース」)

## Resource の項目

| 項目 | 型 | 必須 | 内容 |
| --- | --- | --- | --- |
| `id` | 文字列 | 必須 | リソースを識別する値。`ResourceSet` の中(アカウント × リージョン × 種別)で一意。ARN があれば ARN。応答に ARN が無くても AWS が ARN の形式を定義している種別は、その形式で作った ARN。AWS が ARN を定義していない種別は、下の「ARN が無いリソースの id」の形式(下の「id の一意性」) |
| `type` | 文字列 | 必須 | リソース種別 |
| `account` | 文字列 | 必須 | アカウントキー(例: `aws-123456789012`) |
| `region` | 文字列 | 必須 | リージョン。グローバルなリソースは `global` |
| `name` | 文字列 | 任意 | 人が読む名前(バケット名、ユーザー名など) |
| `tags` | 文字列 → 文字列 | 任意 | リソースのタグ。取得できる種別のみ |
| `configuration` | JSON オブジェクト | 必須 | 種別ごとの構成情報。SDK が返した値を API の名前ごとに入れる(下の「構成情報」) |

`configuration` という名前は、AWS Config がリソースの構成情報を表すのに使う名前に合わせたものです。`resources` は `ResourceSet.resources` と重なり、`attributes` / `properties` は意味が広く、何が入るかが分かりにくいため採りませんでした。

### id の一意性

- リソースは、アカウント・リージョン・種別・`id` の組で識別する。ここでのリージョンはリソースの `region` の値(S3 のバケットのように、`ResourceSet` の `region` が `global` でリソースの `region` がバケットのリージョンになるものも、リソースの `region` を使う)
- `id` が一意であるのは `ResourceSet` の中(アカウント × リージョン × 種別)
- リソースの `region` と `id` の組も、アカウント × 種別の中で一意になる。これは次の 2 つの前提による
  - `regional` の種別では、リソースの `region` は、そのリソースを入れた `ResourceSet` の `region` と同じ
  - 1 つの種別は、`global` の `ResourceSet` か、リージョンごとの `ResourceSet` の、どちらか一方だけを持つ(スコープは種別ごとに 1 つ)
- 外から読み込む入力でも、この一意性を確かめる。スナップショットでは `ResourceSet` の中の `id` の重複を検証の誤りにし(下の「読み込みと検証」)、フィクスチャでは `input` の同じ種別の中の `id` と `region` の組の重複を形式の誤りにする([ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md))
- AWS が同じリソースを複数のリージョンに見せる場合に限り、同じ `id` が複数のリージョンの `ResourceSet` に現れてよい。例: CloudTrail のマルチリージョンの証跡は、ホームリージョンの証跡とほかのリージョンの shadow trail が同じ ARN で返るため、どれも `TrailARN` を `id` にする
  - コピーごとに別の `id`(リージョンを加えた形など)を作らない。ARN があるのに ARN を使わない例外になり、コレクターが shadow trail かを見分けて `id` を変えることになるため。AWS 自身も、どのリージョンのコピーにも同じ ARN を使う
- このため、ルールの `input` の配列には、同じ `id` で `region` だけが違うリソースが並びうる(下の「ポリシーエンジンへの渡し方」)。ルールの検出とフィクスチャの期待値は、必要に応じてリージョンも指定して 1 つのリソースを指せる([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「fail の規約」)

### ARN を形式から作るリソースの id

- 応答に ARN が含まれなくても、AWS が ARN の形式を定義している種別(EC2 の VPC、ネットワーク ACL、インスタンス、フローログなど)は、「ARN が無いリソース」に当たらない。AWS の文書の ARN の形式(Service Authorization Reference の各サービスの「Resource types」の表)に従って作った ARN を `id` にする
  - 例: `arn:aws:ec2:us-east-1:123456789012:vpc/vpc-0123456789abcdef0`
- 各種別の形式は、そのコレクターの機能設計書で表と照らして確かめる

### ARN が無いリソースの id

AWS が ARN を定義していない種別にだけ使います。

- 名前で区別するリソース: `<type>:<account_id>:<region>:<name>`(例: `aws.example.widget:123456789012:us-east-1:example-name`)
- アカウント・リージョンに 1 件だけのリソース(名前が無いもの): `<type>:<account_id>:<region>`(例: `aws.iam.password_policy:123456789012:global`)
- どちらも `arn:` で始めない(本物の ARN と区別するため)
- どの形式を使うかは、そのコレクターの機能設計書に書く

### アカウントのリソース

- 種別は `aws.account.account`(`<service>` は AWS のアカウント管理のサービス名 `account`)
- アカウントごとに 1 件だけ作る。`region` は `global`、`id` は `aws.account.account:<account_id>:global`、`name` は持たない
- コレクターではなく、`yata-provider-aws` が `sts:GetCallerIdentity` の結果から作る。このため、ルールとコレクターの対応の検査では例外とし、`yata-provider-aws` が公開する対応する種別の一覧には含める([ポリシー評価](Architecture-Policy-Evaluation.ja.md))
- `configuration` には `GetCallerIdentity` の `Account` だけを入れる。呼び出し元の ARN やユーザー ID は入れない(SDK の応答をすべて入れる方針の例外)
  - 理由: この種別はコレクターの収集で作るものではなく、`yata-provider-aws` がアカウントを識別するために作るものだから。また、呼び出し元の ARN やユーザー ID はルールの判定に使わず、実行した人の情報を残さないため

```json
{
  "id": "aws.account.account:123456789012:global",
  "type": "aws.account.account",
  "account": "aws-123456789012",
  "region": "global",
  "tags": {},
  "configuration": {
    "GetCallerIdentity": { "Account": "123456789012" }
  }
}
```

- 収集の対象にかかわらず常に作り、スナップショットにも含める
- Account サービスの API(`GetAlternateContact`、`GetContactInformation` など)が返す情報は、この種別に入れず、別の種別にする(例: `aws.account.alternate_contact`)。この種別はコレクターを持たない特別な種別のままにしておくため

## 構成情報(`configuration`)

### 格納の形

- キーは、値を取得した AWS API の操作名(PascalCase。例: `ListBuckets`、`GetPublicAccessBlock`)
- 値は、その操作の出力のうち、そのリソースに当たる部分
  - 一覧・説明の操作(`ListBuckets`、`DescribeInstances` など)では、出力の配列の中のそのリソースの要素
  - リソースごとの取得の操作(`GetPublicAccessBlock`、`GetBucketPolicy` など)では、出力全体
  - アカウントに 1 つの操作で、出力の中の要素がリソースと 1 対 1 で対応するもの(上の「コレクターの方針」)では、そのリソースに対応する要素。要素の取り出し方と値の形は、その種別の機能設計書で決める
  - どれも、ページングの項目(`NextToken`、`Marker` など)と応答のメタデータは含めない
- API は既定のパラメーターで呼び、返った結果を絞り込まずに入れる(上の「コレクターの方針」)
- 例: IAM の認証情報レポート(`GetCredentialReport`)
  - レポートはアカウントに 1 つで、`Content` は CSV のテキストの Blob。各行が 1 人の IAM ユーザー(またはルートユーザー)に対応する
  - 行の `arn` がユーザーの ARN と一致する行を、そのユーザーの `aws.iam.user` の `GetCredentialReport` のキーに、`{"GeneratedTime": "<RFC 3339>", "ReportFormat": "text/csv", "Content": {<列名>: <値>, ...}}` として入れる。出力の項目(`GeneratedTime`、`ReportFormat`、`Content`)はすべて入れ、`Content` だけをその行にする。列名は IAM のドキュメントの CSV の列名(`password_enabled` など。snake_case)、値は CSV の文字列のまま(`"TRUE"`、`"N/A"` などを変換しない)
  - `Content` を構造化して入れるのは、下の「項目名と値の規則」の、テキストの形式が定められた Blob の扱いによる。出力全体ではなく行を入れるのは、上の 1 対 1 の対応づけによる
  - 対応するリソースの無いルートユーザーの行は入れない(上の「コレクターの方針」の例外)
  - 詳細は [最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の「認証情報レポート」。同ページと [認証情報レポートの収集の流れ](../diagrams/Diagram-0005-Credential-Report-Collection-Flow.md) の値の形(`ReportFormat` を含まない)は、認証情報レポートのコレクターの機能設計書で、このページの形に合わせる
- API が「設定が存在しない」ことを示すエラー(例: パブリックアクセスブロックの設定が無い)を返した場合は、その操作のキーの値を `null` にする。どのエラーがこれに当たるかは、各コレクターの機能設計書で決める
- そのほかのエラーの扱いは、失敗した操作の種類で分ける(下の「操作ごとの取得の失敗」)

```json
{
  "ListBuckets": {
    "Name": "example-bucket",
    "CreationDate": "2026-01-01T00:00:00Z"
  },
  "GetPublicAccessBlock": {
    "PublicAccessBlockConfiguration": {
      "BlockPublicAcls": true,
      "IgnorePublicAcls": true,
      "BlockPublicPolicy": true,
      "RestrictPublicBuckets": true
    }
  },
  "GetBucketPolicy": null
}
```

### 操作ごとの取得の失敗

1 つのリソースの付随情報を取る操作(例: バケットごとの `GetBucketPolicy`)が、1 つのリソースでだけ拒否されることがあります。そのたびに組(アカウント × リージョン × 種別)全体を失敗にすると、ほかのリソースまで評価できなくなるため、次のように分けます。

| 失敗した操作 | 扱い |
| --- | --- |
| リソースを列挙する操作(`ListBuckets`、`DescribeSecurityGroups` など) | 組全体の失敗。`ResourceSet` を作らず `CollectionError` を記録する |
| リソースごとの付随情報を取る操作(`GetBucketPolicy` など)で、「設定が存在しない」以外のエラー | そのリソースのその操作のキーに、失敗の印を置く。リソースと組の収集は成功として扱う |

失敗の印の形です。

```json
{
  "ListBuckets": { "Name": "example-bucket", "CreationDate": "2026-01-01T00:00:00Z" },
  "GetBucketPolicy": {
    "yata_error": {
      "kind": "access_denied",
      "message": "access denied when calling GetBucketPolicy"
    }
  }
}
```

- 操作のキーの値を、`yata_error` だけを持つオブジェクトにする
  - `kind` は `CollectionError` の `kind` と同じ値(`access_denied` / `throttled` / `timeout` / `service` / `invalid_response` など)
  - `message` は英語で、認証情報を含めない
- 失敗の印は操作のキーの値の最上位に置く。AWS のどの操作の出力にも、最上位の項目 `yata_error` は無いため、出力と取り違えない(項目名には camelCase のものや、認証情報レポートの snake_case の列名も入るため、名前の書き方の違いには頼らない)
- Rego からは `object.get(r.configuration, ["GetBucketPolicy", "yata_error"], null) != null` で見分けられる
- 印の下の項目(例: `GetBucketPolicy.Policy`)は undefined になるため、それを参照するルールの本体は成り立たず、そのリソースは `fail` に入らない。このため、`yata-engine` は失敗の印を持つリソースがあればルールの状態を `incomplete` にし、そのリソースを `RuleOutcome.incomplete_resources` に記録する。黙って `pass` だけを出さない([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「状態の判定順」)
- 失敗の印はスナップショットにもそのまま保存する

### 項目名と値の規則

- 項目名は、サービスのモデルのメンバー名(AWS API リファレンスと `aws` CLI の JSON 出力と同じ名前)をそのまま使う。多くのサービスでは PascalCase だが、CloudWatch Logs(`filterName`、`logGroupName` など)のように camelCase のサービスもあり、どちらもそのまま使う
  - AWS のドキュメントと `aws` CLI の JSON 出力と同じ名前になり、ルールの作者が照らし合わせやすいため
  - Rust の SDK の名前(snake_case)は SDK の版によって変わることがあるが、API の名前は AWS が互換性を保つため、こちらの方が安定する
  - 構成情報のキー(操作名)は、サービスによらず PascalCase(上の「格納の形」)
- 値の変換の規則
  - 日時は RFC 3339(UTC)の文字列
  - 列挙型は API の値の文字列
  - 数値・真偽値はそのまま
  - バイナリ(Blob)は、標準の base64(RFC 4648 の標準の文字。パディングあり)の文字列にする。JSON にはバイト列をそのまま入れられないためで、値の意味・件数を変えない形式の変換として扱う。`aws` CLI のドキュメントの例でも、出力の Blob を base64 の文字列としてデコードしている
    - AWS のドキュメントでテキストの形式(CSV、JSON など)が定められた Blob は、その種別の機能設計書で決めて、構造化して入れてよい(例: 認証情報レポートの `Content`。上の「格納の形」)。構造化するときも、値を真偽値や `null` に変えるなど、意味を変える変換はしない
    - base64 にするか構造化するかは、その種別を公開する前に機能設計書で決める。公開した後に変えるのは、型の変更なので互換性の無い変更になる(下の「構成情報の互換性」)
  - ポリシー文書(IAM ポリシー、バケットポリシーなど、JSON を文字列で返す項目)は、JSON にパースしたオブジェクトにする。URL エンコードされて返る項目(IAM のポリシー文書など)はデコードしてからパースする。パースできない場合は元の文字列のままにする
  - API が値を返さなかった項目は `null` にする

### SDK の応答を格納する方式

AWS SDK for Rust の型は、通常 serde の `Serialize` を実装していないため、次の方式を比べ、**コレクターが SDK の型から JSON へ自前で変換する**方式にしました。

| 方式 | 判断 |
| --- | --- |
| 自前の変換(採用) | 項目名(API の名前)と値の変換を、コレクターが確実に管理できる。手間は掛かるが、変換を補助するマクロやヘルパーを `support/` に置いて減らす |
| SDK の unstable な機能(`--cfg aws_sdk_unstable` などで serde を有効にする) | 不安定な機能に依存し、ワークスペース全体のビルドの設定が変わる。項目名も Rust の名前になる |
| Smithy の生の応答(HTTP の本文) | サービスによって XML / JSON とプロトコルが違い、解析のための依存が増える。ページングの結合も自前になる |

- 「すべて入れる」は、変換する時点の SDK の出力型の全項目を対象にする(秘密の値・バイナリも含む)。SDK に新しい項目が増えた場合は、変換を更新するまで入らない(項目の追加なので互換性は壊れない)
- 全項目を変換したかは、コレクターの機能設計書の項目の一覧とコードレビューで確かめる

### 秘密の値の扱い

SDK の応答をすべて入れると、機微な値(SNS の購読の `Endpoint` のメールアドレスや URL、Lambda の環境変数など)も入りえます。次のように扱います。

- 秘密に当たる値の取得そのものを目的とする API は呼ばない(読み取り専用ガードの拒否リスト。[AWS アクセス](Architecture-AWS-Access.ja.md))
- それ以外の API の応答に含まれる値は、伏せずにそのまま入れる(`"<redacted>"` などへの置き換えはしない)。コレクターは情報を集めるだけで、どの値を秘密として扱うかの判断を収集に持ち込まないため(上の「コレクターの方針」)
- 応答に含まれる値は、スナップショット、Rego の `input`、ルールが `actual` に入れた場合はレポートに、そのまま入る
  - スナップショット: 外部に出さない。`yata collect` は書き込みの後に注意を出す(下の「書き込み(`yata collect`)」)
  - レポート: 外部に出すときは `--omit-values` で `actual` / `expected` を省く([出力形式](Architecture-Output-Formats.ja.md) の「値の扱い」)
- ルールの作者は、判定に要らない機微な値(SNS の購読の `Endpoint` など)を `actual` や `message` に入れない([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「fail の規約」)

### 構成情報の互換性

ルールは別のリポジトリで配布され、`yata` とは別にリリースされます。ルールは構成情報を項目名で参照するため、項目の名前が変わったり消えたりすると、ルールの参照は undefined になり、エラーにならずに黙って `pass` になります。これを防ぐため、次の規則を守ります。

- 公開した(リリースに含めた)リソース種別の `configuration` は、項目の追加だけを許す
- 名前の変更、削除、型や意味の変更は、互換性の無い変更とする
- 互換性の無い変更が必要な場合は、**新しいリソース種別名を作る**ことを原則とする(例: `aws.example.widget` を残したまま `aws.example.widget_v2` を足す)
  - 古い種別のコレクターは、移行の期間が終わるまで残し、ルールが新しい種別に移れるようにする
  - 影響がその種別を使うルールだけに限られるため。ポリシーの形式のバージョン(`format_version`)を上げると、すべてのポリシーのセットが新しい `yata` を必要とし、影響が大きい
- `format_version` を上げるのは、`input` 全体の形やリソースの共通の項目(`id`、`type`、`account`、`region` など)を変えるときだけとする([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「互換性の規則」)
- **SDK の更新時**: 項目名は API の名前なので、Rust の SDK の名前が変わっても出力は変わらない。SDK を更新する PR では、コレクターの出力を期待するファイルと比べるテストを実行し、差分が項目の追加以外を含む場合(API の項目の削除・型の変更など)は、互換性の無い変更として新しい種別名で対応する

## ResourceSet と Inventory

### ResourceSet

1 つのアカウント × 1 つのリージョン × 1 つのリソース種別の収集結果です。

```json
{
  "account": "aws-123456789012",
  "region": "us-east-1",
  "resource_type": "aws.ec2.security_group",
  "resources": [
    {
      "id": "arn:aws:ec2:us-east-1:123456789012:security-group/sg-0123456789abcdef0",
      "type": "aws.ec2.security_group",
      "account": "aws-123456789012",
      "region": "us-east-1",
      "name": "example-sg",
      "tags": {},
      "configuration": {
        "DescribeSecurityGroups": {
          "GroupId": "sg-0123456789abcdef0",
          "GroupName": "example-sg",
          "VpcId": "vpc-0123456789abcdef0",
          "IpPermissions": [
            {
              "IpProtocol": "tcp",
              "FromPort": 22,
              "ToPort": 22,
              "IpRanges": [ { "CidrIp": "0.0.0.0/0" } ]
            }
          ]
        }
      }
    }
  ]
}
```

(値はすべてダミーです。構成情報は一部の項目だけを示しています。)

- `ResourceSet` は収集に成功したときだけ作る。0 件でも作る(`resources` は空の配列)
- 収集に失敗した組には `ResourceSet` を作らず、`CollectionError` を記録する
- グローバルなリソース種別(IAM など)は `region` を `global` にした 1 つの `ResourceSet` にする
- 配列の順序は `id` の昇順にそろえ、出力を再現できるようにする
- S3 のバケット(`aws.s3.bucket`)は `global` の種別とし、`region` が `global` の `ResourceSet` 1 つに全バケットを入れる。各リソースの `region` にはバケットのリージョン(`ListBuckets` の `BucketRegion`)を入れる。理由は [最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の「S3 のバケット」
- 同じ `id` が複数のリージョンの `ResourceSet` に現れることがある(上の「id の一意性」)

### Inventory

1 回の収集の全体です。`scan` が直接収集した場合も、スナップショットから読み込んだ場合も、同じ型で表します。

| 項目 | 型 | 内容 |
| --- | --- | --- |
| `accounts` | `Vec<AccountInfo>` | 収集したアカウント。当面は 1 つだけ(下の「複数アカウントとの整合」) |
| `regions` | `Vec<String>` | 対象にしたリージョン |
| `resource_types` | `Vec<ResourceTypeInfo>` | 対象にしたリソース種別と、そのスコープ(`global` / `regional`) |
| `resource_sets` | `Vec<ResourceSet>` | 成功した収集の結果 |
| `collection_errors` | `Vec<CollectionError>` | 失敗した収集の記録 |
| `started_at` / `finished_at` | 日時 | 収集の開始・終了 |

収集の対象は「`accounts` × (`regional` の種別なら `regions`、`global` の種別なら `global`)× `resource_types`」の組です。各組は、`ResourceSet` があるか、`CollectionError` があるかの、どちらか一方になります。

### AccountInfo の項目

| 項目 | 型 | 必須 | 内容 |
| --- | --- | --- | --- |
| `account` | 文字列 | 必須 | アカウントキー(例: `aws-123456789012`) |
| `id` | 文字列 | 必須 | アカウント ID(例: `123456789012`) |
| `provider` | 文字列 | 必須 | プロバイダ(`aws`) |

### CollectionError の項目

`CollectionError` は収集の失敗の記録(`yata-core` の型)です。コレクターが返す Rust のエラー `CollectError`(`yata-provider-aws` の型)を、`yata-provider-aws` がこの記録に変換します。

| 項目 | 型 | 必須 | 内容 |
| --- | --- | --- | --- |
| `account` | 文字列 | 必須 | アカウントキー |
| `region` | 文字列 | 必須 | 失敗したリージョン。グローバルなリソース種別は `global` |
| `resource_type` | 文字列 | 必須 | 収集に失敗したリソース種別 |
| `kind` | 文字列 | 必須 | エラーの種類。`access_denied` / `throttled` / `region_unavailable` / `timeout` / `service` / `invalid_response` のいずれか([エラー処理とログ](Architecture-Error-Handling-and-Logging.ja.md)) |
| `message` | 文字列 | 必須 | 人が読む説明(英語)。認証情報を含めない |

### 「0 件」「未収集」「失敗」の区別

組(アカウント × リージョン × 種別)ごとに、次の 3 つの状態を区別します。

| 状態 | 条件 |
| --- | --- |
| 収集済み(0 件を含む) | その組の `ResourceSet` がある |
| 失敗 | その組の `CollectionError` がある |
| 未収集 | どちらも無い(例: 別のルールを選んで `yata collect` したスナップショットを、ほかの種別を使うルールで評価した) |

リソースごとの付随情報の取得の失敗(構成情報の失敗の印)は、組の状態には影響しません(組は「収集済み」のまま)。ルールの状態はこの区別と、失敗の印の有無から決めます([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「状態の判定順」)。

### 複数アカウントとの整合

- 型(`Inventory.accounts`、`ResourceSet.account`、`CollectionError.account`、アカウントキー)とスナップショットの構成は、複数のアカウントを持てる形にしておく
- 当面、`scan` と `collect` が収集するのは、認証情報で決まる 1 つのアカウントだけとする。複数のアカウントを含むスナップショットを `--snapshot` で読んだ場合は、未対応として終了コード 2 にする
- 複数のアカウントへの対応(AWS Organizations など)は未決事項とする。対応するときは、`input` をアカウントごとに作り、ルールの評価もアカウントごとに行う(下の「ポリシーエンジンへの渡し方」がアカウント単位なので、形を変えずに済む)

## スナップショット

`yata collect` が `Inventory` をディレクトリに保存したものです。`yata scan --snapshot <DIR>` で読み込み、AWS に接続せずに評価します。

### ディレクトリの構成

```
<snapshot>/
├── yata-snapshot.json                    # マニフェスト
└── aws-123456789012/                     # アカウントキー
    ├── global/
    │   ├── aws.account.account.json      # ResourceSet 1 つ
    │   └── aws.iam.user.json
    └── us-east-1/
        └── aws.ec2.security_group.json
```

- パスは `<snapshot>/<account>/<region>/<resource_type>.json`。1 ファイルに `ResourceSet` を 1 つ入れる
- 失敗した組のファイルは作らない(マニフェストの `collection_errors` に記録する)

### マニフェスト(`yata-snapshot.json`)

```json
{
  "schema_version": 1,
  "tool": { "name": "yata", "version": "0.1.0" },
  "started_at": "2026-01-01T00:00:00Z",
  "finished_at": "2026-01-01T00:05:00Z",
  "accounts": [
    { "account": "aws-123456789012", "id": "123456789012", "provider": "aws" }
  ],
  "regions": ["us-east-1"],
  "resource_types": [
    { "type": "aws.account.account", "scope": "global" },
    { "type": "aws.iam.user", "scope": "global" },
    { "type": "aws.ec2.security_group", "scope": "regional" },
    { "type": "aws.s3.bucket", "scope": "global" }
  ],
  "resource_sets": [
    { "account": "aws-123456789012", "region": "global", "resource_type": "aws.account.account", "file": "aws-123456789012/global/aws.account.account.json", "count": 1 },
    { "account": "aws-123456789012", "region": "global", "resource_type": "aws.iam.user", "file": "aws-123456789012/global/aws.iam.user.json", "count": 2 },
    { "account": "aws-123456789012", "region": "us-east-1", "resource_type": "aws.ec2.security_group", "file": "aws-123456789012/us-east-1/aws.ec2.security_group.json", "count": 3 }
  ],
  "collection_errors": [
    { "account": "aws-123456789012", "region": "global", "resource_type": "aws.s3.bucket", "kind": "access_denied", "message": "access denied when calling ListBuckets" }
  ]
}
```

| キー | 内容 |
| --- | --- |
| `schema_version` | スナップショットの形式のバージョン(整数)。互換性の無い変更で上げる |
| `tool` | 収集した `yata` の名前とバージョン |
| `started_at` / `finished_at` | 収集の開始・終了(RFC 3339、UTC) |
| `accounts` | 収集したアカウント |
| `regions` | 対象にしたリージョン |
| `resource_types` | 対象にしたリソース種別とスコープ |
| `resource_sets` | 成功した組と、そのファイルの相対パス(`/` 区切り)、リソースの件数 |
| `collection_errors` | 失敗した組 |

マニフェストの `resource_sets` は、`yata collect` が書き出したファイルの目録で、利用者が編集するものではありません(中央の一覧の問題は生じない)。

### 書き込み(`yata collect`)

- 出力先は `--output <DIR>` で指定する(必須。環境変数・既定値は無い)
- 出力先がすでにあれば終了コード 2。`--overwrite` を指定した場合だけ置き換える。置き換えてよいのは、ルートに `yata-snapshot.json` がある(スナップショットである)ディレクトリだけ。それ以外のディレクトリは `--overwrite` でも置き換えない(誤って別のディレクトリを消さないため)
- 出力先と同じ親ディレクトリに一時ディレクトリを作って書き込み、すべて書けたら名前を変えて出力先にする。`--overwrite` のときは、古いディレクトリを別の名前に移してから新しいものを出力先にし、最後に古いものを削除する
- 書き込みの途中で失敗した場合(終了コード 3)と中断した場合(130)は、一時ディレクトリを削除し、既存の出力先は変えない
- Unix では、ディレクトリを `0700`、ファイルを `0600` で作る。Windows では、親ディレクトリのアクセス権を引き継ぐ(未決事項を参照)
- 書き込みが終わったら、標準エラー出力に注意を出す: スナップショットには実環境の情報(アカウント ID、ARN、SDK の応答の構成情報。構成情報は伏せずに入っているため、機微な値を含みうる)が含まれること、リポジトリにコミットしたり外部に共有したりしないこと

### 読み込みと検証(`yata scan --snapshot`)

- 読み込みと検証の関数は `yata-core` に置き、`yata-cli` が呼ぶ
- 次のいずれかに当てはまれば `SnapshotError` とし、終了コード 2 にする
  - マニフェストが無い・JSON として不正・`schema_version` が一致しない
  - マニフェストの `resource_sets` のファイルが無い、またはファイルの中身(`account`、`region`、`resource_type`、件数)がマニフェストと一致しない
  - マニフェストに無い `.json` ファイルがある
  - `ResourceSet` の中で `id` が重複する。`regional` の種別で、リソースの `region` が `ResourceSet` の `region` と違う(上の「id の一意性」)
  - 収集の対象の組(`accounts` × リージョン × `resource_types`)のうち、`resource_sets` と `collection_errors` のどちらにも無いもの、または両方にあるものがある
  - 複数のアカウントを含む(当面は未対応)
  - ファイルの大きさ・ファイル数の上限を超える
  - 指定した最上位のディレクトリより下に、シンボリックリンク・reparse point がある(ポリシーのディレクトリと同じ扱い。[ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「外部のポリシーの安全性」)
- 上限の値は、ポリシーの資源の上限と同じく未決事項とする

## ポリシーエンジンへの渡し方

図: [リソースモデルの関係図](../diagrams/Diagram-0001-Resource-Model.md)

`yata-cli` が `Inventory` を `yata-engine` に渡し、`yata-engine` が **ルール × アカウント** ごとに Rego の `input` を作ります。

```json
{
  "account": { "account": "aws-123456789012", "id": "123456789012", "provider": "aws" },
  "regions": ["us-east-1", "ap-northeast-1"],
  "aws": {
    "s3": {
      "bucket": [
        { "id": "arn:aws:s3:::example-bucket", "type": "aws.s3.bucket", "region": "us-east-1", "...": "..." }
      ]
    },
    "cloudtrail": {
      "trail": [ { "...": "..." } ]
    }
  }
}
```

- リソース種別 `<provider>.<service>.<resource>` を入れ子にして置く。ルールは `input.aws.s3.bucket` のように参照する
- 1 つのアカウントの、全リージョンの `ResourceSet` の `resources` を、種別ごとに 1 つの配列にまとめて入れる。CIS の「全リージョンで CloudTrail が有効か」のように、リージョンをまたぐ判定ができるようにするため。各リソースは `region` を持つ
- まとめた配列は、`id` → `region` の昇順に並べる。評価とフィクスチャとの比較を再現できるようにするため
- 配列には、同じ `id` で `region` だけが違うリソースが並びうる(上の「id の一意性」。CloudTrail の shadow trail など)。ルールは、リソースを `id` だけでなく `id` と `region` の組で区別する必要があるかを、種別ごとに確かめる
- `regions` には対象にしたリージョンを入れる。リージョンごとの有無を判定するルールが使う
- 入れるのは、そのルールのメタデータの `target_type` と `related_types` の種別だけ。ルールが使うデータが明示され、フィクスチャも小さくなる
- 宣言した種別で、収集済みの組が 0 件だった場合は空の配列を入れる。評価されたルールの `input` では、宣言した種別の配列は必ず存在する
- 宣言した種別に、未収集・失敗の組がある場合の扱いはルールの状態で決める([ポリシー評価](Architecture-Policy-Evaluation.ja.md))。黙って空の配列を補って `pass` にすることはしない
- `collection_errors` は `input` に入れず、`yata-engine` がルールの状態を決めるのに使う
- `data` ドキュメントは使わない。ルールに渡すのは `input` だけとする

## 実装上の配置

- `Resource`、`ResourceSet`、`Inventory`、`AccountInfo`、`ResourceTypeInfo`、`CollectionError` の型、シリアライズ、スナップショットの読み込みと検証は `yata-core` に置く
- スナップショットの書き込み(一時ディレクトリ、名前の変更、アクセス権)は `yata-cli` に置く
- 種別の文字列定数と構成情報の組み立ては、各コレクターのファイル(`yata-provider-aws/src/collectors/<service>/<resource>.rs`)に置く
- 種別ごとの構成情報の形(呼ぶ API、「設定が存在しない」に当たるエラー、`id` の形式、Blob の項目を base64 にするか構造化するか)は、そのコレクターの機能設計書(`docs/features/`)に書く

## 出典

- ARN の形式と、サービスごとの形式の調べ方: <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference-arns.html>
- EC2 のリソースの ARN の形式(Service Authorization Reference): <https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazonec2.html>
- CloudTrail の `DescribeTrails`(shadow trail が同じ ARN で返る): <https://docs.aws.amazon.com/awscloudtrail/latest/APIReference/API_DescribeTrails.html>
- CloudWatch Logs の `DescribeMetricFilters`(項目名が camelCase): <https://docs.aws.amazon.com/AmazonCloudWatchLogs/latest/APIReference/API_DescribeMetricFilters.html>
- IAM の認証情報レポート(CSV の列): <https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_getting-report.html>、`GetCredentialReport`(`Content` が Blob): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetCredentialReport.html>
- base64(RFC 4648): <https://www.rfc-editor.org/rfc/rfc4648>
- `aws` CLI のパラメーターの型(Blob。出力の Blob を base64 としてデコードする例): <https://docs.aws.amazon.com/cli/latest/userguide/cli-usage-parameters-types.html>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| 構成情報の機械可読なスキーマ | 種別ごとの構成情報を JSON Schema などで定義し、フィクスチャやコレクターの出力を検証するか。ポリシーのリポジトリとの互換性の確認(ポリシーのリポジトリの CI で、ルールやフィクスチャが参照する項目がスキーマにあるかを確かめるなど)にも使う | 最初のコレクターを実装する Issue で必要性を判断する |
| 大規模アカウントでのメモリ使用量 | 全リソースをメモリに持つ設計のため、リソース数が非常に多い場合の上限 | 性能の問題が出た時点で別 Issue |
| `aws` 以外のパーティション | `aws-cn`、`aws-us-gov` への対応と、パーティションの情報の持ち方 | 需要が出た時点で別 Issue |
| 複数アカウントの収集・評価 | AWS Organizations の配下のアカウントの収集と、複数アカウントのスナップショットの評価 | 別 Issue |
| スナップショットの Windows のアクセス権 | Windows で、作成者だけが読めるアクセス権(ACL)を設定するか。設定する場合は依存クレートの追加が要る可能性がある | スナップショットを実装する Issue |
| スナップショットの上限の値 | ファイルの大きさ・ファイル数の上限 | スナップショットを実装する Issue |
| 変換の補助 | SDK の型から構成情報への変換を補助するマクロ・ヘルパーの形 | 最初のコレクターを実装する Issue |
| Blob の base64 の変換に使うクレート | [初期依存クレート](Architecture-Initial-Dependencies.ja.md) の未決事項を参照 | Blob の項目を base64 で入れる最初のコレクターを実装する Issue |
| スナップショットの注意の文言 | `yata collect` の注意に「構成情報は伏せずに入っているため、機微な値を含みうる」を加えたが、[CLI インターフェース](Architecture-CLI-Interface.ja.md) の「スナップショット」の注意の文にはまだ無い。CLI インターフェースのページを、このページの文言に合わせる | CLI インターフェースを更新する Issue(`yata permissions` の出力の示し方を決める Issue) |
