# リソースモデル

> This Japanese version is the authoritative source. The English version (`Architecture-Resource-Model.md`) has not been written yet.

収集した AWS のリソースをどう表現し、どの単位でまとめ、どう保存し(スナップショット)、ポリシーエンジンにどう渡すかを定めます。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## 方針

- リソースは「共通の項目」と「種別ごとの構成情報(`configuration`)」に分ける
- 共通の項目は Rust の型(`yata-core`)で定義する。構成情報は JSON の値(`serde_json::Value`)として持ち、Rust の共通の型には入れない
  - 種別ごとに Rust の型を `yata-core` に足すと、全機能が `yata-core` を書き換えることになり、並列開発で衝突するため
- 構成情報には、SDK が返した値をすべて入れる。ただし秘密に当たる値は伏せる(下の「秘密の値の扱い」)
- 収集した結果は「アカウント × リージョン × リソース種別」を 1 単位(`ResourceSet`)としてまとめる

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
| `id` | 文字列 | 必須 | リソースを一意に識別する値。ARN があれば ARN。無い場合は下の「ARN が無いリソースの id」の形式 |
| `type` | 文字列 | 必須 | リソース種別 |
| `account` | 文字列 | 必須 | アカウントキー(例: `aws-123456789012`) |
| `region` | 文字列 | 必須 | リージョン。グローバルなリソースは `global` |
| `name` | 文字列 | 任意 | 人が読む名前(バケット名、ユーザー名など) |
| `tags` | 文字列 → 文字列 | 任意 | リソースのタグ。取得できる種別のみ |
| `configuration` | JSON オブジェクト | 必須 | 種別ごとの構成情報。SDK が返した値を API の名前ごとに入れる(下の「構成情報」) |

`configuration` という名前は、AWS Config がリソースの構成情報を表すのに使う名前に合わせたものです。`resources` は `ResourceSet.resources` と重なり、`attributes` / `properties` は意味が広く、何が入るかが分かりにくいため採りませんでした。

### ARN が無いリソースの id

- 名前で区別するリソース: `<type>:<account_id>:<region>:<name>`(例: `aws.example.widget:123456789012:us-east-1:example-name`)
- アカウント・リージョンに 1 件だけのリソース(名前が無いもの): `<type>:<account_id>:<region>`(例: `aws.iam.password_policy:123456789012:global`)
- どちらも `arn:` で始めない(本物の ARN と区別するため)
- どの形式を使うかは、そのコレクターの機能設計書に書く

### アカウントのリソース

- 種別は `aws.account.account`(`<service>` は AWS のアカウント管理のサービス名 `account`)
- アカウントごとに 1 件だけ作る。`region` は `global`、`id` は `aws.account.account:<account_id>:global`、`name` は持たない
- コレクターではなく、`yata-provider-aws` が `sts:GetCallerIdentity` の結果から作る。このため、ルールとコレクターの対応の検査では例外とし、`yata-provider-aws` が公開する対応する種別の一覧には含める([ポリシー評価](Architecture-Policy-Evaluation.ja.md))
- `configuration` には `GetCallerIdentity` の `Account` だけを入れる。呼び出し元の ARN やユーザー ID は、ルールの判定に使わず、実行した人の情報を残さないため入れない(SDK の応答をすべて入れる方針の例外)

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
  - どちらも、ページングの項目(`NextToken`、`Marker` など)と応答のメタデータは含めない
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
- キー名 `yata_error` は英小文字で始まるので、AWS API の項目名(PascalCase)とは重ならない
- Rego からは `object.get(r.configuration, ["GetBucketPolicy", "yata_error"], null) != null` で見分けられる
- 印の下の項目(例: `GetBucketPolicy.Policy`)は undefined になるため、それを参照するルールの本体は成り立たず、そのリソースは `fail` に入らない。このため、`yata-engine` は失敗の印を持つリソースがあればルールの状態を `incomplete` にし、そのリソースを `RuleOutcome.incomplete_resources` に記録する。黙って `pass` だけを出さない([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「状態の判定順」)
- 失敗の印はスナップショットにもそのまま保存する

### 項目名と値の規則

- 項目名は AWS API リファレンスの名前(PascalCase のメンバー名)をそのまま使う
  - AWS のドキュメントと `aws` CLI の JSON 出力と同じ名前になり、ルールの作者が照らし合わせやすいため
  - Rust の SDK の名前(snake_case)は SDK の版によって変わることがあるが、API の名前は AWS が互換性を保つため、こちらの方が安定する
- 値の変換の規則
  - 日時は RFC 3339(UTC)の文字列
  - 列挙型は API の値の文字列
  - 数値・真偽値はそのまま
  - バイナリ(Blob)は入れない(キーごと省く)
  - ポリシー文書(IAM ポリシー、バケットポリシーなど、JSON を文字列で返す項目)は、JSON にパースしたオブジェクトにする。URL エンコードされて返る項目(IAM のポリシー文書など)はデコードしてからパースする。パースできない場合は元の文字列のままにする
  - API が値を返さなかった項目は `null` にする

### SDK の応答を格納する方式

AWS SDK for Rust の型は、通常 serde の `Serialize` を実装していないため、次の方式を比べ、**コレクターが SDK の型から JSON へ自前で変換する**方式にしました。

| 方式 | 判断 |
| --- | --- |
| 自前の変換(採用) | 項目名(API の名前)、値の変換、秘密の値の伏せ方を、コレクターが確実に管理できる。手間は掛かるが、変換を補助するマクロやヘルパーを `support/` に置いて減らす |
| SDK の unstable な機能(`--cfg aws_sdk_unstable` などで serde を有効にする) | 不安定な機能に依存し、ワークスペース全体のビルドの設定が変わる。項目名も Rust の名前になる |
| Smithy の生の応答(HTTP の本文) | サービスによって XML / JSON とプロトコルが違い、解析のための依存が増える。ページングの結合も自前になる |

- 「すべて入れる」は、変換する時点の SDK の出力型の全項目を対象にする(秘密の値・バイナリを除く)。SDK に新しい項目が増えた場合は、変換を更新するまで入らない(項目の追加なので互換性は壊れない)
- 全項目を変換したかは、コレクターの機能設計書の項目の一覧とコードレビューで確かめる

### 秘密の値の扱い

SDK の応答をすべて入れると、秘密に近い値(Lambda の環境変数、EC2 の UserData、ECS のタスク定義の環境変数など)も入りえます。構成情報はスナップショット、Rego の `input`、レポートの `actual` に入るため、次の規則で伏せます。

- 秘密に当たる値の取得を目的とする API は呼ばない(読み取り専用ガードの拒否リスト。[AWS アクセス](Architecture-AWS-Access.ja.md))
- 応答に秘密に当たる値が含まれる項目は、コレクターが明示的に伏せる。キーは残し、値を固定の文字列 `"<redacted>"` に置き換える(例: Lambda の `Environment.Variables` の各値)。キーを残すのは、「秘密らしい名前の環境変数がある」などをルールが判定できるようにするため
- どの項目を伏せるかは、コレクターのファイル内の一覧に書き、機能設計書にも書く。伏せる項目の判断に迷う場合は伏せる
- 認証情報そのもの(アクセスキーのシークレット、パスワード、トークン)を返す項目は、どのコレクターでも必ず伏せる
- 伏せたことは、コレクターの出力を固定するテストで確かめる([テスト方針](Architecture-Testing-Strategy.ja.md))
- 公開した種別で、あとから伏せる項目を足す必要が分かった場合は、**安全上の修正として、例外的に同じ種別名のまま行う**(下の「構成情報の互換性」の例外)。秘密の値を出し続けないことを優先するため
  - リリースノートで、伏せた種別と項目を告知する
  - その項目の値を参照するルールは、値が `"<redacted>"` に変わるため、判定が変わる影響を受けうる

### 構成情報の互換性

ルールは別のリポジトリで配布され、`yata` とは別にリリースされます。ルールは構成情報を項目名で参照するため、項目の名前が変わったり消えたりすると、ルールの参照は undefined になり、エラーにならずに黙って `pass` になります。これを防ぐため、次の規則を守ります。

- 公開した(リリースに含めた)リソース種別の `configuration` は、項目の追加だけを許す
- 名前の変更、削除、型や意味の変更は、互換性の無い変更とする
- 伏せる項目の追加(値が `"<redacted>"` に変わる)も互換性の無い変更だが、安全上の修正として例外的に同じ種別名のまま行う(上の「秘密の値の扱い」)
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
- S3 のバケットは、バケットのリージョンにかかわらず 1 回の一覧で取れるが、`region` にはバケットのリージョンを入れる。どの `ResourceSet` に入れるか(グローバルとして扱うか、リージョンごとに分けるか)はコレクターの機能設計書で決める

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
- 書き込みが終わったら、標準エラー出力に注意を出す: スナップショットには実環境の情報(アカウント ID、ARN、SDK の応答の構成情報)が含まれること、リポジトリにコミットしたり外部に共有したりしないこと

### 読み込みと検証(`yata scan --snapshot`)

- 読み込みと検証の関数は `yata-core` に置き、`yata-cli` が呼ぶ
- 次のいずれかに当てはまれば `SnapshotError` とし、終了コード 2 にする
  - マニフェストが無い・JSON として不正・`schema_version` が一致しない
  - マニフェストの `resource_sets` のファイルが無い、またはファイルの中身(`account`、`region`、`resource_type`、件数)がマニフェストと一致しない
  - マニフェストに無い `.json` ファイルがある
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
- `regions` には対象にしたリージョンを入れる。リージョンごとの有無を判定するルールが使う
- 入れるのは、そのルールのメタデータの `target_type` と `related_types` の種別だけ。ルールが使うデータが明示され、フィクスチャも小さくなる
- 宣言した種別で、収集済みの組が 0 件だった場合は空の配列を入れる。評価されたルールの `input` では、宣言した種別の配列は必ず存在する
- 宣言した種別に、未収集・失敗の組がある場合の扱いはルールの状態で決める([ポリシー評価](Architecture-Policy-Evaluation.ja.md))。黙って空の配列を補って `pass` にすることはしない
- `collection_errors` は `input` に入れず、`yata-engine` がルールの状態を決めるのに使う
- `data` ドキュメントは使わない。ルールに渡すのは `input` だけとする

## 実装上の配置

- `Resource`、`ResourceSet`、`Inventory`、`AccountInfo`、`ResourceTypeInfo`、`CollectionError` の型、シリアライズ、スナップショットの読み込みと検証は `yata-core` に置く
- スナップショットの書き込み(一時ディレクトリ、名前の変更、アクセス権)は `yata-cli` に置く
- 種別の文字列定数、構成情報の組み立て、伏せる項目の一覧は、各コレクターのファイル(`yata-provider-aws/src/collectors/<service>/<resource>.rs`)に置く
- 種別ごとの構成情報の形(呼ぶ API と、伏せる項目)は、そのコレクターの機能設計書(`docs/features/`)に書く

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
