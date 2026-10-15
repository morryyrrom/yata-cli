# S3 のコレクター

> This Japanese version is the authoritative source. The English version (`Feature-0014-S3-Collectors.md`) has not been written yet.

S3 の 2 つのリソース種別(`aws.s3.bucket`、`aws.s3control.account_public_access_block`)のコレクターを設計します。

最初のルール群と、使う種別・操作・権限は [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) で決まっています(とくに「コレクターの方針」「S3 のバケット」)。コレクターの枠組み(`Collector`、`CollectContext`、`CollectError`、失敗の印、エラーの種類の判定、読み取り専用ガード)は [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md)、組の中の並行の呼び出し(`support::fan_out`)・変換(`support::convert`)・期待するファイルとの比較(`support::golden`)は [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) に従います。構成情報の形の規則は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、エラーの扱いは [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md)、テストは [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) に従います。

## 目的と背景

- 最初のルール群の CIS 3.1.1(バケットポリシーで HTTP の要求を拒否する)、3.1.4(ブロックパブリックアクセス)、4.4(CloudTrail のバケットのサーバーアクセスログ)は、S3 のバケットの設定とアカウント単位のブロックパブリックアクセスの設定を使う。ルール(ポリシーのリポジトリ)のフィクスチャを書き始めるには、構成情報の形(全項目、`null` にするエラー、`id` の形式)が先に決まっている必要がある
- `aws.s3.bucket` は `global` の種別で、1 回の `ListBuckets` で全リージョンのバケットを取り、バケットごとの操作はそのバケットのリージョンに送る([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「S3 のバケット」)。バケットの数だけ 3 つの操作を呼ぶため、[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の組の中の並行の呼び出しを使う
- 次の未決事項のうち S3 の分を、この設計で決める
  - [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「`access_denied` などのコードの過不足」
  - [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「各コレクターの構成情報の細部(全項目の一覧、「設定が存在しない」に当たるエラーの全一覧)」

## スコープ

- `src/collectors/s3/bucket.rs`(`aws.s3.bucket`)と `src/collectors/s3control/account_public_access_block.rs`(`aws.s3control.account_public_access_block`)
- 各種別の構成情報の全項目、`id`・`name`・`region`・`tags` の決め方、`null` にするエラー、組全体の失敗と失敗の印に分ける操作、`required_permissions`
- `ListBuckets` のページングと、バケットごとの操作のリージョンの扱い(`BucketRegion`)
- バケットごとの 3 つの操作の並行の呼び出し(既存の `support::fan_out` を使う)と、S3 での上限の妥当性
- S3 のエラーのコードの、エラーの種類への対応
- 上記のテスト(`aws-smithy-mocks`。実 AWS には接続しない)

## スコープ外

- オブジェクトの中身の読み取り。`s3:GetObject` は読み取り専用ガードの拒否リストにあり、呼ばない
- ルール本体(ポリシーのリポジトリ)
- 後回しにしたコントロールの操作(3.1.2 の `GetBucketVersioning` など)。バケットごとに呼ぶ操作は `GetBucketPolicy`、`GetPublicAccessBlock`、`GetBucketLogging` の 3 つだけとし、ほかのコントロールを範囲に加えるときに、その操作をキーとして足す(項目の追加なので構成情報の互換性を壊さない)
- バケットのタグ(`GetBucketTagging`)。`aws.s3.bucket` はタグを取得しない種別とする(下の「`aws.s3.bucket`」)
- S3 Express One Zone のディレクトリバケット(`ListDirectoryBuckets`)。理由は次のとおり
  - `ListBuckets` はディレクトリバケットを返さない(API リファレンスの注記「This operation is not supported for directory buckets」)。ディレクトリバケットの一覧は別の操作 `ListDirectoryBuckets` で、リージョンのエンドポイント(`s3express-control.<region>.amazonaws.com`)に送る。権限も別(`s3express:ListAllMyDirectoryBuckets`)
  - このため、ディレクトリバケットを集めるなら、`global` の `aws.s3.bucket` に混ぜず、リージョンの別の種別(例: `aws.s3.directory_bucket`)にする必要がある。新しい種別・権限・リージョンごとの組を足すことになり、この Issue の範囲を超える
  - 最初のルール群の 3 コントロールは、ディレクトリバケットでは判定の意味が薄い。ディレクトリバケットは、バケット単位のブロックパブリックアクセスの設定がすべて有効で変更できず(ユーザーガイド「Access management」)、`GetPublicAccessBlock` と `GetBucketLogging` はディレクトリバケットに対応していない(各 API リファレンスの注記)
  - 扱いは未決事項「ディレクトリバケット」とする
- ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock` の変更。必要な依存(`aws-sdk-s3`、`aws-sdk-s3control` と各 `test-util`)は基盤の Issue で入っている
- `support/` など共有のファイル、`docs/architecture/` の変更。この設計と全体設計・既存の設計書の差は、下の「既存の設計との差異」にまとめる
- CLI のオプションの追加

## 共通の方針

2 つのコレクターは、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「コレクターの方針」と、[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「共通の方針」に従います。S3 で具体的に決めることは次のとおりです。

- **既定のパラメーターで呼ぶ**: 付けるのは、ページングのパラメーター(`ListBuckets` の `MaxBuckets` と `ContinuationToken`)と、必須の識別子(`GetBucketPolicy`・`GetPublicAccessBlock`・`GetBucketLogging` の `Bucket`、S3 Control の `GetPublicAccessBlock` の `AccountId`)だけ
  - `ListBuckets` の `Prefix` と `BucketRegion`(結果を絞り込むパラメーター)は付けない
  - バケットごとの操作の `ExpectedBucketOwner`(所有者が違えば 403 にする任意のヘッダー)は付けない。`ListBuckets` は呼び出し元のアカウントが所有するバケットだけを返すため、付けても結果は変わらず、既定のパラメーターで呼ぶ方針に合わせる
- **絞り込まない・置き換えない**: `ListBuckets` が返したバケットは、リージョン・名前・作成日にかかわらずすべてリソースにする
- **許す変換**: SDK の型から JSON への変換、日時の RFC 3339 の文字列への変換、列挙の値の文字列への変換、バケットポリシーの JSON のパース(下の「`GetBucketPolicy` の `Policy` の変換」)だけ
- **秘密の値も伏せない**: この 2 種別の応答に秘密の値は無いが、バケットポリシーの文書(プリンシパルのアカウント ID、条件の値など)やログの付与先の `EmailAddress` などは、そのまま入れる
- **途中のページの失敗**: `ListBuckets` のどのページの失敗も、それまでのページの結果を使わず組全体の失敗にする
- **グローバルの扱い**: 2 種別とも `Scope::Global`。`collect` に渡されるリージョン(`us-east-1`)に向けて、`ListBuckets` と S3 Control の `GetPublicAccessBlock` を呼ぶ。バケットごとの操作は、そのバケットのリージョンに送る(下の「バケットのリージョンの扱い」)

### 値の変換の規則

[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「値の変換の規則」と同じです。日時は `support::convert` の関数で RFC 3339 の文字列(UTC、`Z`)にし、`Option` の `None` は `null`、列挙(`BucketLogsPermission`、`Type`、`PartitionDateSource`)は `as_str()` の文字列(SDK が知らない値も API が返した文字列のまま)にします。項目名は API リファレンスのメンバー名(PascalCase)をそのまま使います。

- SDK が `Option` でなく値を必ず持つ型にしている項目(`LoggingEnabled` の `TargetBucket`・`TargetPrefix`、`Grantee` の `Type`)は、`null` にならない。API が値を返さなかった場合、`aws-sdk-s3`(1.152.0)の生成コード(`src/serde_util.rs` の `logging_enabled_correct_errors`・`grantee_correct_errors`。応答の不足を補う client error correction)は、応答の解釈の失敗にせず、`TargetBucket`・`TargetPrefix` に空文字列 `""`、`Type` に `"no value was set"`(SDK が知らない値 `Type::Unknown`)を入れる
  - SDK の型からは、API が返した値か SDK が補った値かを見分けられない。このため、SDK の制約として受け入れ、変換では手を加えない(`""` や `"no value was set"` を `null` に置き換えると、API が実際に空文字列を返した場合と区別できなくなり、「置き換えない」方針にも反する)。IAM のコレクターの、SDK が `false` にする真偽値と同じ扱い
  - ルールは、これらの項目の空文字列・`"no value was set"` を「値が無い」とみなしてよいが、`null` にはならないことを前提にする
- ブロックパブリックアクセスの 4 つの真偽値は、SDK では `Option<bool>` なので、API が返さなければ `null` になる

### 値を変換できない場合

応答が SDK の型としては受け取れても、構成情報にできない場合は、`invalid_response` として次のように扱います。

| 場合 | 扱い |
| --- | --- |
| `ListBuckets` の要素に `Name` が無い | 組全体の失敗(`CollectError`、`invalid_response`)。`id` を決められず、要素を落とすと絞り込みになるため |
| `ListBuckets` の要素に `BucketRegion` が無い、またはリージョン名の形式でない | 組全体の失敗(`invalid_response`)。リソースの `region` を決められず、バケットごとの操作の送り先も決まらないため(下の「バケットのリージョンの扱い」) |
| `ListBuckets` の要素の `CreationDate` を整形できない | 組全体の失敗(`invalid_response`) |

- メッセージは `"invalid response when calling ListBuckets (<理由のコード>)"` の形にし、理由のコードは `MissingName`、`MissingBucketRegion`、`InvalidBucketRegion`、`InvalidDateTime` とする。バケット名などの識別子は含めず、`debug` のログに出す
- リージョン名の形式の検査には、`--region` の検査と同じ関数(`crate::is_valid_region_name`)を使う
- バケットごとの操作の出力には日時が無いため、バケットごとの操作で値を変換できない場合は生じない(SDK が応答を解釈できない場合は、SDK のエラー `ResponseError` として失敗の印になる)

## 種別ごとの設計

### `aws.s3.bucket`

| 項目 | 内容 |
| --- | --- |
| ファイル | `collectors/s3/bucket.rs` |
| スコープ | global(`region` が `global` の `ResourceSet` 1 つに全バケットを入れる) |
| 操作 | `ListBuckets`(列挙)、`GetBucketPolicy`・`GetPublicAccessBlock`・`GetBucketLogging`(バケットごと。`Bucket` は必須の識別子) |
| `required_permissions` | `["s3:ListAllMyBuckets", "s3:GetBucketPolicy", "s3:GetBucketPublicAccessBlock", "s3:GetBucketLogging"]` |
| リソースの件数 | `ListBuckets` が返したバケットの数(全ページ) |
| `id` | バケットの ARN を形式から作る: `arn:<partition>:s3:::<Name>`(例: `arn:aws:s3:::example-bucket`) |
| `name` | `Name` |
| `region` | `BucketRegion`(例: `eu-west-1`) |
| `tags` | 常に `{}`(タグを取得しない種別) |
| `null` にするエラー | `GetBucketPolicy` の `NoSuchBucketPolicy`、`GetPublicAccessBlock` の `NoSuchPublicAccessBlockConfiguration` |
| 組全体の失敗 | `ListBuckets` の失敗(どのページでも)。`Name`・`BucketRegion` の無い要素、`BucketRegion` の形式の誤り、整形できない `CreationDate`(上の「値を変換できない場合」) |
| 失敗の印 | バケットごとの 3 つの操作の、上の `null` にするエラー以外の失敗(そのバケットのそのキーだけ) |

図: [aws.s3.bucket の構成情報のキー](../diagrams/Diagram-0014-S3-Bucket-Config-Keys.md)

構成情報の形(バケットポリシー・ブロックパブリックアクセス・ログの設定があるバケット):

```json
{
  "ListBuckets": {
    "BucketArn": null,
    "BucketRegion": "eu-west-1",
    "CreationDate": "2026-01-01T00:00:00Z",
    "Name": "example-bucket",
    "Owner": {
      "DisplayName": null,
      "ID": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  },
  "GetBucketPolicy": {
    "Policy": {
      "Version": "2012-10-17",
      "Statement": [
        {
          "Sid": "DenyInsecureTransport",
          "Effect": "Deny",
          "Principal": "*",
          "Action": "s3:*",
          "Resource": ["arn:aws:s3:::example-bucket", "arn:aws:s3:::example-bucket/*"],
          "Condition": { "Bool": { "aws:SecureTransport": "false" } }
        }
      ]
    }
  },
  "GetPublicAccessBlock": {
    "PublicAccessBlockConfiguration": {
      "BlockPublicAcls": true,
      "BlockPublicPolicy": true,
      "IgnorePublicAcls": true,
      "RestrictPublicBuckets": true
    }
  },
  "GetBucketLogging": {
    "LoggingEnabled": {
      "TargetBucket": "example-logs-bucket",
      "TargetGrants": null,
      "TargetObjectKeyFormat": {
        "PartitionedPrefix": { "PartitionDateSource": "EventTime" },
        "SimplePrefix": null
      },
      "TargetPrefix": "example-bucket/"
    }
  }
}
```

どれも設定されていないバケット:

```json
{
  "ListBuckets": { "...": "..." },
  "GetBucketPolicy": null,
  "GetPublicAccessBlock": null,
  "GetBucketLogging": { "LoggingEnabled": null }
}
```

(値はすべてダミーです。)

**`ListBuckets`**

- 値は、出力の `Buckets` の中のそのバケットの要素の 4 項目(`BucketArn`、`BucketRegion`、`CreationDate`、`Name`)に、そのページの出力の `Owner`(`{"DisplayName": ..., "ID": ...}`)を加えたもの
  - `Owner` は出力の最上位の項目で、API リファレンスでは「The owner of the buckets listed」(一覧のバケットの所有者)。どのバケットにも同じ値が入る。要素ごとに 1 対 1 で付随する情報として、要素と同じ値に入れる。出力の最上位の項目を要素の値に加えるのは、全体設計の「格納の形」(一覧の操作は要素だけ)を広げる扱いになる。この扱いは、別の Issue(#39)で全体設計([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「格納の形」)を先に更新して main へ入れ、この設計のマージはそのあとにする(「既存の設計との差異」)
  - `Owner` を返さなかったページの要素では `"Owner": null`。`DisplayName` は返らなければ `null`(API リファレンスの応答の例も `ID` だけを返している)
  - `ContinuationToken` はページングの項目、`Prefix` は要求の `Prefix` を返すだけの項目(要求に付けた場合だけ返る)なので、入れない
- `BucketArn` は、API リファレンスでディレクトリバケットにだけ対応する項目とされ、汎用バケットでは通常 `null`。返った場合もそのまま入れる
- `CreationDate` は、API リファレンスの注記のとおり、バケットポリシーの変更などで変わることがある。値は API が返したまま入れる
- どのページも `Buckets` を返さなかった場合は、バケットが 0 件として扱う(要素がリソースそのものなので `null` を入れる場所が無く、バケットが 0 件のアカウントは正しく存在するため。IAM の `ListPolicies` のように常に要素があるとは言えない)

**`GetBucketPolicy`**

- 値は出力全体(`{"Policy": ...}`)。`NoSuchBucketPolicy`(バケットポリシーが無い)では `null`
- 呼び出しは成功したが `Policy` が返らなかった場合は `{"Policy": null}` とし、`NoSuchBucketPolicy` の `null` と区別する
- 3.1.1 は、`Policy.Statement` に `aws:SecureTransport` が `false` の要求を拒否する文があるかを見る。`GetBucketPolicy` が `null`(ポリシーが無い)なら、拒否する文が無いことを表す

**`GetBucketPolicy` の `Policy` の変換**

- API はバケットポリシーを JSON の文書のまま(HTTP の本文として)返し、SDK はそれを文字列(`Option<String>`)として渡す。IAM のポリシー文書と違い URL エンコードされていないため、URL デコードはせず、JSON としてパースするだけにする([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「項目名と値の規則」の、ポリシー文書の扱い)
- パースできない場合は、API が返した文字列をそのまま `Policy` に入れる。失敗の印にはしない(値は API が返した値のままで、ルールは型で見分けられるため)。失敗したことは `debug` のログに出す
- パースは `serde_json::from_str` で行い、関数は `collectors/s3/bucket.rs` に置く(IAM の `Document` の URL デコードの関数とは処理が違い、ほかに使うコレクターが無いため)

**`GetPublicAccessBlock`(バケット)**

- 値は出力全体(`{"PublicAccessBlockConfiguration": {...}}`)。4 項目(`BlockPublicAcls`、`IgnorePublicAcls`、`BlockPublicPolicy`、`RestrictPublicBuckets`)をすべて入れ、返らない項目は `null`
- `NoSuchPublicAccessBlockConfiguration`(バケット単位の設定が無い)では `null`。成功したが `PublicAccessBlockConfiguration` が返らなかった場合は `{"PublicAccessBlockConfiguration": null}`
- この操作はバケット単位の設定だけを返す。S3 が実際に使うのは、バケットとアカウント(組織の設定を引き継ぐことがある)の設定のうち最も制限の強い組み合わせで、それを合わせて判定するのは 3.1.4 のルール(`aws.s3control.account_public_access_block` を `related_types` に置く)

**`GetBucketLogging`**

- 値は出力全体(`{"LoggingEnabled": ...}`)。サーバーアクセスログが無効なバケットでは、API はエラーではなく `LoggingEnabled` の無い応答を返すため、`{"LoggingEnabled": null}` になる。この操作に `null` にするエラーは無い
- `LoggingEnabled` は 4 項目(`TargetBucket`、`TargetGrants`、`TargetPrefix`、`TargetObjectKeyFormat`)
  - `TargetGrants` は、返れば要素ごとに `{"Grantee": {...} | null, "Permission": <文字列> | null}` の配列、返らなければ `null`。`Grantee` は 5 項目(`DisplayName`、`EmailAddress`、`ID`、`Type`、`URI`)。`Type` は API の XML では属性(`xsi:type`)だが、API リファレンスと AWS CLI の JSON 出力のメンバー名 `Type` を使う
  - `TargetObjectKeyFormat` は、返れば `{"PartitionedPrefix": {"PartitionDateSource": ...} | null, "SimplePrefix": {} | null}`、返らなければ `null`。`SimplePrefix` は項目を持たない型なので、返れば空のオブジェクト `{}` になる
- 4.4 は、証跡の `S3BucketName` と同じ `name` のバケットの `GetBucketLogging.LoggingEnabled` が `null` でないかを見る。照合はバケットの `name`(または `ListBuckets.Name`)で行い、`id` の形には頼らない

**`id`・`name`・`region`・`tags`**

- `id` は、AWS の文書の ARN の形式(Service Authorization Reference の S3 の `bucket`: `arn:${Partition}:s3:::${BucketName}`)で作る。応答に ARN が無くても AWS が ARN の形式を定義している種別なので、「ARN が無いリソースの id」には当たらない([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ARN を形式から作るリソースの id」)
  - パーティションは `support::partition::SUPPORTED_PARTITION`(`aws`)を使う。ARN を作る関数はパーティションを引数に取る形にし、`aws` 以外のパーティションに対応するときに直せるようにする
  - `BucketArn` が返った場合も、`id` には使わず形式から作る。`BucketArn` はディレクトリバケットにだけ対応すると文書化されており、汎用バケットの `id` を応答の有無で変えないため。`BucketArn` が返り、作った ARN と違う場合は `debug` のログに出す
  - バケット名はすべてのアカウント・リージョンで一意なので、`ResourceSet` の中で `id` は重ならない
- `name` は `Name`
- `region` は `BucketRegion`。`ResourceSet` の `region` は `global` のまま([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ResourceSet」)
- `tags` は常に `{}`。`aws.s3.bucket` は、この範囲ではタグを取得しない種別とし、`tags` は「タグが無い」ことを表さない。ルールはこの種別の `tags` を判定に使わない(未決事項「バケットのタグ」)

**処理の順**

1. `ListBuckets` の全ページを取る(失敗したら組全体の失敗)。各要素から `Name`・`BucketRegion` を取り出し、`ListBuckets` の値を作る(上の「値を変換できない場合」に当たれば組全体の失敗)
2. 要素に現れた `BucketRegion` ごとに、`ctx.client::<aws_sdk_s3::Client>(<BucketRegion>)` でクライアントを 1 つずつ作る
3. バケットごとに、`GetBucketPolicy` → `GetPublicAccessBlock` → `GetBucketLogging` の順に呼ぶ 1 つの仕事を作り、`support::fan_out` で上限付きで並行に実行する(下の「組の中の並行の呼び出し」)。仕事には、バケット名(所有する文字列)と、そのバケットのリージョンのクライアントの複製を渡す。3 つの操作は、それぞれ独立に成功・`null`・失敗の印を記録する(1 つの失敗で、残りを呼ばないことはしない)
4. バケットごとに `Resource` を組み立てる

図: [aws.s3.bucket の収集の流れ](../diagrams/Diagram-0014-S3-Bucket-Collection-Sequence.md)

#### バケットのリージョンの扱い

- **送り先**: バケットごとの 3 つの操作は、`ListBuckets` の `BucketRegion` のリージョンに向けたクライアントで呼ぶ。S3 は、バケットのリージョンと違うリージョンのエンドポイントへの要求を受け付けない(`PermanentRedirect` などのエラーを返す)。AWS SDK for Rust の `aws-sdk-s3`(1.152.0)には、そのようなエラーを受けて別のリージョンへ送り直す処理が無い(応答のヘッダー `x-amz-bucket-region` を読むのは `HeadBucket` の出力の解釈だけ)ため、送り先を最初から正しく決める
- **`BucketRegion` を使う理由**: API リファレンスでは、`BucketRegion` は要求にパラメーターが 1 つ以上ある場合に返る。ページングの形(`MaxBuckets` 付き)で呼ぶため、常に返る想定とする。値はリージョンコード(API リファレンスの応答の例では `us-east-1`、`us-east-2`)で、`GetBucketLocation` の `LocationConstraint` のような旧来の表記(`EU`、`us-east-1` を表す空の値)ではないため、リージョン名の形式の検査に通る。`GetBucketLocation` や `HeadBucket` でリージョンを調べ直すことはしない(操作と権限が増える。`HeadBucket` は `Describe` / `List` / `Get` で始まらず、読み取り専用ガードが拒否する)
- **`BucketRegion` が無い・形式が誤っている場合**: 組全体の失敗(`invalid_response`)にする(上の「値を変換できない場合」)。リソースの `region` は必須の文字列で、`global` などを仮に入れるとルールの判定(4.4 のリージョンの扱いなど)を誤らせうるため。通常は起こらない応答とし、起こった場合の補い方は未決事項「`BucketRegion` が欠けた場合の補い方」とする
- **接続できないリージョン**: DNS の解決の失敗・接続の失敗(`DispatchFailure` で `is_io()` が真)は、既存の判定の表で `region_unavailable` になり、そのバケットのそのキーに失敗の印が置かれる([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「S3 のバケット」)。`OptInRequired` のコードも `region_unavailable` になる
- **オプトインのリージョン**: `ListBuckets` は `us-east-1` に送り、全リージョンのバケットを返す。アカウントでオプトインのリージョンが無効になっている場合に、そのリージョンのバケットへの要求がどのエラー(コード)になるかは、公式の文書で確かめられなかった。この設計では、既存の判定の表に当たらないコードは `service` の失敗の印とし(メッセージにコードが残る)、`region_unavailable` にするコードを足すかは未決事項「接続できないリージョンのエラーのコード」とする。いずれの場合も失敗の印になり、ルールは `incomplete` になるため、黙って `pass` にはならない
- **別リージョンへの転送のエラー**: 一覧を取った後にバケットが削除され、同じ名前で別のリージョンに作り直された場合などは、`PermanentRedirect`(301)、`AuthorizationHeaderMalformed`(400)、`IllegalLocationConstraintException`(400)などが返りうる。これらは送り直さず、`service` の失敗の印にする(メッセージにコードが残る)。送り直すには応答のヘッダーを読む処理を足す必要があり、まれな競合のために処理を増やさない
- **`--region` との関係**: `--region` を指定しても、すべてのバケットを収集し、そのバケットのリージョンに送る([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「S3 のバケット」)。`--region` で対象外にしたリージョンのバケットにも要求を送る

### `aws.s3control.account_public_access_block`

| 項目 | 内容 |
| --- | --- |
| ファイル | `collectors/s3control/account_public_access_block.rs` |
| スコープ | global |
| 操作 | S3 Control の `GetPublicAccessBlock`(`AccountId` は必須の識別子) |
| `required_permissions` | `["s3:GetAccountPublicAccessBlock"]` |
| リソースの件数 | 常に 1 件(設定が無い場合も 1 件) |
| `id` | `aws.s3control.account_public_access_block:<account_id>:global`(例: `aws.s3control.account_public_access_block:123456789012:global`) |
| `name` | なし(キーを出さない) |
| `region` | `global` |
| `tags` | `{}` |
| `null` にするエラー | `NoSuchPublicAccessBlockConfiguration`(HTTP 404。アカウントに設定が無い) |
| 組全体の失敗 | `NoSuchPublicAccessBlockConfiguration` 以外の失敗 |
| 失敗の印 | なし |

構成情報の形:

```json
{
  "GetPublicAccessBlock": {
    "PublicAccessBlockConfiguration": {
      "BlockPublicAcls": true,
      "BlockPublicPolicy": true,
      "IgnorePublicAcls": true,
      "RestrictPublicBuckets": true
    }
  }
}
```

設定が無い場合:

```json
{ "GetPublicAccessBlock": null }
```

- 値は出力全体。4 項目をすべて入れ、返らない項目は `null`。成功したが `PublicAccessBlockConfiguration` が返らなかった場合は `{"GetPublicAccessBlock": {"PublicAccessBlockConfiguration": null}}` とし、`NoSuchPublicAccessBlockConfiguration` の `null` と区別する
- API リファレンスでは、この操作はアカウント単位の実効の設定を返し、それは組織の単位のポリシーを引き継ぐことがある。コレクターは返った値をそのまま入れ、組織の設定から来たものかを見分けない
- 構成情報のキーは操作名 `GetPublicAccessBlock` で、`aws.s3.bucket` のバケット単位の `GetPublicAccessBlock` と同じ名前になる。種別が違うため衝突しない(キーは種別ごとの構成情報の中の名前)
- **`AccountId` の渡し方**: `ctx.account_id()`(`GetCallerIdentity` で確かめた 12 桁のアカウント ID)を渡す。収集の対象のアカウントそのものを指す必須の識別子で、結果を絞り込むパラメーターではない
- **送り先**: `collect` に渡されるリージョン(`us-east-1`)に向けたクライアント(`ctx.client::<aws_sdk_s3control::Client>(region)`)で呼ぶ。SDK のエンドポイントの規則(この操作は `RequiresAccountId` が真)で、要求はアカウント ID を先頭に付けたホスト(`<account-id>.s3-control.us-east-1.amazonaws.com`)に送られる。コレクターはエンドポイントを指定しない
- `NoSuchPublicAccessBlockConfiguration` の判定は、`ProvideErrorMetadata::code()` で行う(SDK のエラーの変種には頼らない。[AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「`CollectError` と失敗の印」)
- 3.1.4 は、`input` のバケットの `GetPublicAccessBlock` と、`data.aws.s3control.account_public_access_block` のこのリソースを合わせて判定する

## 組の中の並行の呼び出し

### 方式

[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「組の中の並行の呼び出し」を、そのまま使います。

- `fan_out(ctx.fan_out_limit(), jobs, task)` で、バケットごとの仕事を上限 8(`CollectContext::fan_out_limit()`、`context.rs` の `FAN_OUT_LIMIT`)で並行に実行する。上限の値をコレクターに書かない
- 1 つの仕事は 1 つのバケットの 3 つの操作を順に呼ぶ。このため、`aws.s3.bucket` の組が同時に送る S3 の要求は最大 8
- 仕事は SDK のエラーを `null` または失敗の印に変え、失敗しない値(3 つのキーの JSON の値)を返す。1 つのバケットの失敗は、ほかのバケットの仕事を止めない
- 結果はバケットの一覧の順に並ぶため、構成情報は実行のたびに同じになる(`Resource` の並べ替えは基盤が `id` の昇順で行う)
- `support/`・`context.rs` は変更しない

### S3 での上限 8 の妥当性

- **S3 の制限**: S3 が公表している要求の速さの目安は、バケットのプレフィックスごとに毎秒 3,500 の書き込み・5,500 の読み取り(オブジェクトの操作)。バケットの設定の取得(`GET ?policy` など)の制限は公表されていない。制限を超えると `503 SlowDown` を返し、SDK はこれをスロットリングとして再試行する
- **集中しない**: 1 つのバケットに送るのは 3 つの要求を順にだけで、同時に送る 8 つの要求は別々のバケット(多くは複数のリージョン)に向く。往復を 100〜300 ms とすると、組全体で毎秒 30〜80 程度の要求で、1 つのバケットへの速さは問題にならない
- **アカウント単位の制限**: バケットの設定の API にアカウント単位の制限があるかは公表されていない。IAM と同じく、ほかの利用者(デプロイのパイプラインなど)と枠を分け合う可能性があるため、控えめな 8 を使う
- **再試行の割当て**: `aws-sdk-s3`(1.152.0)の `src/config.rs` の既定の再試行の分割の名前は `s3-<リージョン>`。再試行の割当て(トークンバケット)は、バケットのリージョンごとに分かれる。あるリージョンでスロットリングが続いても、ほかのリージョンのバケットの再試行は妨げられない。`ListBuckets`(`us-east-1`)は `us-east-1` のバケットの操作と割当てを共有するが、`ListBuckets` は並行の仕事を始める前に終わる。S3 Control は別の分割(`s3control-us-east-1`)
- **IAM の組との関係**: S3 と IAM は別のサービスで、再試行の割当ても制限も分かれる。組の中で並行に呼ぶ組(`aws.iam.user` 8、`aws.iam.policy` 8、`aws.s3.bucket` 8)と、単発の呼び出しの組(`aws.iam.account_summary`、`aws.iam.password_policy`、`aws.s3control.account_public_access_block` の各 1)が同時に動いても、同時の呼び出しは最大 8 + 8 + 8 + 1 + 1 + 1 = 27 で(IAM の最大 18 に S3 の 9 が加わる)、手元の資源の点でも問題にならない
- **見直し**: 実際のアカウントでスロットリングによる失敗の印が多い場合、またはバケットが多く時間が問題になる場合は、値を見直す(未決事項「組の中の並行数の実測による見直し(S3)」)。上限はサービスによらず共通の値のため、S3 だけ変えるには `CollectContext` の変更(共有のファイル)が要る

### 件数の見込みと所要時間

| 種別 | 呼び出しの回数 | 所要時間の見込み(往復 100〜300 ms) |
| --- | --- | --- |
| `aws.s3control.account_public_access_block` | 1 | 1 秒未満 |
| `aws.s3.bucket`(バケット B 個) | `ListBuckets` が B / 10,000 ページ(順に)+ バケットごとに 3 回(8 並行) | B = 100 で 4〜12 秒、B = 1,000 で 40〜115 秒、B = 10,000 で 7〜19 分 |

- 往復の時間は、端末とバケットのリージョンの距離で変わる。端末から遠いリージョンのバケットが多いと、見込みの上側になる
- 汎用バケットのクォータの既定値は 10,000(引き上げ可能)。大きなアカウントでは `aws.s3.bucket` の組が収集の全体の時間の下限を決めうる
- メモリ: バケットポリシーは 1 つ 20 KB 以下(S3 のクォータ)。バケットが 10,000 個で全バケットに最大の大きさのポリシーがあっても 200 MB 程度で、通常はずっと小さい

## 再試行とスロットリング

- 再試行は [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の決定のまま、SDK の既定(standard、最大 3 回の試行)とし、コードで設定しない
- 再試行を尽くしたスロットリング(種類 `throttled`)の扱いは、操作の種類で決まる

| 種別 | 操作 | 再試行を尽くしたスロットリング | ルールへの影響 |
| --- | --- | --- | --- |
| `aws.s3.bucket` | `ListBuckets`(どのページでも) | 組全体の失敗(`CollectionError`、`throttled`) | 3.1.1、3.1.4、4.4 が `skipped`(`collection_failed`)。`aws.s3.bucket` は `global` の組が 1 つだけなので、その失敗は「すべての組が失敗」に当たる(4.4 では `related_types` の種別として) |
| `aws.s3.bucket` | `GetBucketPolicy`、`GetPublicAccessBlock`、`GetBucketLogging` | そのバケットのそのキーに失敗の印(`throttled`) | `aws.s3.bucket` を使うルール(3.1.1、3.1.4、4.4)が `incomplete` |
| `aws.s3control.account_public_access_block` | `GetPublicAccessBlock` | 組全体の失敗(`throttled`) | 3.1.4 が `skipped`(`collection_failed`)。`global` の組が 1 つだけなので、`related_types` の種別のすべての組が失敗したことになる |

- 失敗の印を置いた後も、ほかのバケットの仕事は続ける(早く打ち切らない)
- 失敗の印が 1 つ以上ある操作は、操作ごとに 1 行の `warn` のログを出す(例: `"3 of 120 GetBucketPolicy calls failed (access_denied: 2, throttled: 1)"`)。バケット名は `debug` にだけ出す。IAM のコレクターと同じ形で、関数は `collectors/s3/bucket.rs` に置く(`support/` への共通化は、共有のファイルの変更になるため別の PR とする)
- ルールの状態は [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「状態の判定順」による。宣言した種別(`target_type`・`related_types`)のいずれかで、すべての組が失敗すれば `skipped`(`collection_failed`)になる。2 種別とも `global` で組は 1 つだけなので、組の失敗は常に「すべての組が失敗」に当たる

## エラーの種類の判定(S3 のコード)

[AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「エラーの種類の判定」の表を、S3 と S3 Control のエラーのコード(API リファレンスの各操作と「List of error codes」)と照らしました。

| コード | 返る場面 | 種類 | 表への追加 |
| --- | --- | --- | --- |
| `AccessDenied` | 権限不足。バケットポリシーが `GetBucketPolicy` などを拒否している場合も含む | `access_denied` | 不要(既にある) |
| `SlowDown` | 要求の速さの制限(HTTP 503) | `throttled` | 不要 |
| `NoSuchBucketPolicy` | `GetBucketPolicy`: バケットポリシーが無い | `GetBucketPolicy` では `null`。ほかの操作で返った場合は `service`(失敗の印) | 不要 |
| `NoSuchPublicAccessBlockConfiguration` | `GetPublicAccessBlock`(バケット・S3 Control): 設定が無い | その `GetPublicAccessBlock` では `null`。ほかの操作で返った場合は `service` | 不要 |
| `NoSuchBucket` | 一覧を取った後にバケットが削除された | `service`(失敗の印) | 不要 |
| `PermanentRedirect`、`AuthorizationHeaderMalformed`、`IllegalLocationConstraintException` | バケットのリージョンと違うリージョンへの要求 | `service`(失敗の印) | 不要 |
| `MethodNotAllowed` | `GetBucketPolicy` を、バケットの所有者のアカウントに属さない ID で呼んだ(`ListBuckets` は所有するバケットだけを返すため、通常は起こらない) | `service` | 不要 |
| `InternalError`、`ServiceUnavailable` | サービスの内部の失敗(SDK が再試行した後) | `service` | 不要 |
| `ExpiredToken`、`InvalidToken`、`InvalidAccessKeyId`、`SignatureDoesNotMatch` | 認証情報の失効・誤り | `service` | 不要 |
| (DNS の解決・接続の失敗) | バケットのリージョンに接続できない | `region_unavailable` | 不要(既存の `DispatchFailure` の行) |

- `support/errors.rs` の表への追加は要らない
- `null` にするエラーは、操作とコードの組で判定する(`GetBucketPolicy` × `NoSuchBucketPolicy`、`GetPublicAccessBlock` × `NoSuchPublicAccessBlockConfiguration`)。ほかの操作で同じコードが返っても `null` にしない。「無い」と読むと、ルールが誤って判定しうるため
- `NoSuchBucket` を `null` にしない。値を `null`(「設定が無い」)にすると、3.1.4 などで誤った判定になりうるため、失敗の印(`service`)にする。リソースそのものを落とすことも、絞り込みになるためしない(IAM の `NoSuchEntity` と同じ扱い)
- 認証情報の失効・誤りのコードを `region_unavailable` にするかは、未決事項「接続できないリージョンのエラーのコード」で、無効なオプトインのリージョンで返るコードを確かめてから決める

図: [S3 のエラーの振り分け](../diagrams/Diagram-0014-S3-Error-Routing.md)

## 振る舞い

この設計は CLI のコマンド・オプションを変えません。

- `yata scan` / `yata collect`: ルールの `target_type` / `related_types` に `aws.s3.bucket`、`aws.s3control.account_public_access_block` があれば、それぞれ `global` の 1 組として収集する。`aws.s3.bucket` の `ResourceSet` は 1 つで、各リソースの `region` はバケットのリージョン
- `yata permissions`: 上の `required_permissions` を出す。AWS 管理ポリシー `SecurityAudit` は、`s3:ListAllMyBuckets`、`s3:GetBucket*`(`s3:GetBucketPolicy`、`s3:GetBucketPublicAccessBlock`、`s3:GetBucketLogging` を含む)、`s3:GetAccountPublicAccessBlock` を含む
- エラー時
  - `ListBuckets` の失敗(権限不足など)は `aws.s3.bucket` の組の `CollectionError` になり、`aws.s3.bucket` を使うルール(3.1.1、3.1.4、4.4)は `skipped`(`collection_failed`)になる(`global` の組が 1 つだけのため。[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「状態の判定順」)
  - バケットごとの操作の失敗は失敗の印になり、`aws.s3.bucket` を使うルールは `incomplete` になる。ルールの状態は種別の単位で決まるため、あるバケットの `GetBucketPolicy` の失敗で、3.1.1 に加えて 3.1.4・4.4 も `incomplete` になる([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「振る舞い」のとおり)
  - S3 Control の `GetPublicAccessBlock` の失敗(`NoSuchPublicAccessBlockConfiguration` を除く)は、その組の `CollectionError` になり、3.1.4 は `skipped`(`collection_failed`)になる
  - いずれもスキャンは不完全で、終了コードは [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) に従う
- ログ: 上の「再試行とスロットリング」の `warn` のほか、`debug` に組ごとのバケットの件数・リージョンの数、失敗したバケット名と SDK のエラーの詳細を出す

## 構成

```
crates/yata-provider-aws/
├── src/
│   └── collectors/
│       ├── s3/
│       │   └── bucket.rs                     # 新規: aws.s3.bucket(ポリシーの JSON のパース、ARN の作成、warn の集計を含む)
│       └── s3control/
│           └── account_public_access_block.rs  # 新規: aws.s3control.account_public_access_block
└── tests/
    └── golden/
        ├── s3/
        │   └── bucket.json
        └── s3control/
            └── account_public_access_block.json
```

- 2 つのコレクターは build.rs が検出し、`aws.s3.bucket`、`aws.s3control.account_public_access_block` の種別になる。一覧・`mod` の宣言・`Cargo.toml` に追記しない
- 各ファイルは `pub(crate) fn collector() -> Box<dyn Collector>` を定義する
- 使う共通部分: `support::fan_out::fan_out`、`CollectContext::fan_out_limit()`、`support::convert`(日時、`Option` → `null`)、`support::errors`(`CollectError::from_sdk`、`failure_mark`、`to_failure_mark()`、`facts_of`)、`support::partition::SUPPORTED_PARTITION`、`crate::is_valid_region_name`、テストでは `support::clients::testing::{mock_factory, mock_factory_counted}` と `support::golden`。いずれも既存のもので、変更しない
- 1 つのコレクターだけが使う処理(ポリシーの JSON のパース、バケットの ARN の作成、リージョンごとのクライアントの用意、`warn` の集計)は、そのコレクターのファイルに置く。`warn` の集計は IAM の 2 つのコレクターにも同じ関数があり、3 つ目になる。`support/` への共通化は共有のファイルの変更になるため、この Issue では行わず、別の整理の PR とする

### 実装の PR の分け方

- 共有のファイルを変更しないため、1 つの PR にする: `src/collectors/s3/bucket.rs`、`src/collectors/s3control/account_public_access_block.rs`、`tests/golden/s3/bucket.json`、`tests/golden/s3control/account_public_access_block.json`(すべて新規)
- 実装の途中で共通部分(`support/`、`context.rs` など)の変更が必要になったら、作業を止めて報告し、別の PR にする
- 順序: 別の Issue(#39)で、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「格納の形」に、一覧の操作の値へ出力の最上位の項目(`ListBuckets` の `Owner`)を加える扱いを反映し、その PR を先に main へ入れる。この設計のマージはそのあとにし、実装の PR はこの設計のマージの後に出す

### 依存

- 使うクレートはすべて基盤の Issue で入っている(`aws-sdk-s3`、`aws-sdk-s3control`、`serde_json`、`tokio`、`tracing`。テストでは各 `test-util`、`aws-smithy-mocks`、`aws-smithy-runtime-api`)
- ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock` は変更しない
- バケットポリシーの JSON のパースは `serde_json` で行い、新しいクレートを足さない

## 使う AWS API と IAM 権限

| 種別 | API | 呼ぶ回数 | 送り先 | IAM 権限 |
| --- | --- | --- | --- | --- |
| `aws.s3.bucket` | `ListBuckets`(`MaxBuckets` = 10,000、`ContinuationToken`) | ページの数 | `us-east-1` | `s3:ListAllMyBuckets` |
| `aws.s3.bucket` | `GetBucketPolicy` | バケットごと | バケットのリージョン | `s3:GetBucketPolicy` |
| `aws.s3.bucket` | `GetPublicAccessBlock` | バケットごと | バケットのリージョン | `s3:GetBucketPublicAccessBlock` |
| `aws.s3.bucket` | `GetBucketLogging` | バケットごと | バケットのリージョン | `s3:GetBucketLogging` |
| `aws.s3control.account_public_access_block` | `GetPublicAccessBlock`(S3 Control) | 1 | `us-east-1` | `s3:GetAccountPublicAccessBlock` |

- **`MaxBuckets` の値**: 10,000 とする。API の最大値で、`MaxBuckets` を付けずにほかのページングのパラメーターを付けた場合に S3 が使う既定のページの大きさと同じ。ページングの形で呼ぶためのページの大きさで、返るバケットの集合を変えない([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「コレクターの方針」)。SDK のページネーター(`into_paginator().page_size(10_000)`)で全ページを取る
- ページングの無い呼び出しは、汎用バケットのクォータが既定の 10,000 を超えるアカウントでは拒否されるため、使わない(API リファレンスの注記)
- どれも `Get` / `List` で始まる読み取りの操作で、読み取り専用ガードの接頭辞の規則で許可される。拒否リスト・例外リストの変更は要らない
- 各コレクターの `required_permissions` は上の表の権限で、基盤の既存のテスト(`required_permissions` の形、拒否リストの権限を含まないこと)の対象に自動でなる

## テスト方針

[テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) の「コレクターのテスト」と、[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「テスト方針」に従い、実 AWS には接続しません。テストは各コレクターのファイルの中の単体テスト(`#[cfg(test)] mod tests`)に置き、基盤の `support::clients::testing::mock_factory`(ダミーの認証情報、モックの HTTP クライアント、再試行なし、`RuleMode::MatchAny`)でクライアントを作ります。アカウント ID は `123456789012`、バケット名・正規ユーザー ID などはすべてダミーの値にします。

### 各コレクター共通

- **構成情報の形**: モックの応答から作った `Vec<Resource>`(`id` の昇順)の JSON を、`tests/golden/<service>/<resource>.json` とパースした `serde_json::Value` どうしで比べる(更新は `YATA_UPDATE_GOLDEN=1`)
- **`id`・`type`・`account`・`region`・`name`・`tags`**: 上の各表のとおりであること
- **ガード**: コレクターが呼ぶ操作が、すべて送信の段階に達すること(`mock_factory_counted` の記録)
- **メタデータ**: `resource_type()`、`scope()`、`required_permissions()` が上の表のとおりであること

### `aws.s3.bucket`

- **件数とページング**: `ListBuckets` を 2 ページ(1 ページ目が `ContinuationToken` を返す)で返し、全バケットがリソースになること(モックの件数 = リソースの件数)。複数のリージョン(`us-east-1`、`eu-west-1`、`ap-northeast-1` など)のバケットを含め、各リソースの `region` がそのバケットの `BucketRegion` であること
- **既定のパラメーター**: `match_requests` で、`ListBuckets` の `MaxBuckets` が 10,000 で、`Prefix`・`BucketRegion` が無いこと。1 ページ目の規則は「`ContinuationToken` が無い」、2 ページ目の規則は「`ContinuationToken` が 1 ページ目の応答の値と等しい」で照合する(`RuleMode::MatchAny` で同じ操作の規則を見分けるため)。バケットごとの 3 つの操作は、`Bucket` が一覧のバケット名で、`ExpectedBucketOwner` が無いこと
- **`Owner`**: 各バケットの `ListBuckets` の値に、そのページの `Owner` が入ること。`Owner` を返さないページの要素では `null` になること
- **`GetBucketPolicy`**: HTTP の応答のモック(本文が JSON の文書)で、パースしたオブジェクトが `Policy` に入ること(SDK が本文を変えずに渡すことの確認を兼ねる)。JSON でない本文で、元の文字列のまま入ること。`NoSuchBucketPolicy` で `"GetBucketPolicy": null` になること(`then_error` の汎用のエラーと、S3 の XML のエラー応答 `<Error><Code>NoSuchBucketPolicy</Code>...</Error>`(HTTP 404)の両方で、SDK がコードを取り出せることを確かめる)。成功の応答で `Policy` が無い場合に `{"Policy": null}` になること
- **`GetPublicAccessBlock`**: 4 項目が入り、返らない項目が `null` になること。`NoSuchPublicAccessBlockConfiguration` で `null`、成功の応答で設定が無い場合に `{"PublicAccessBlockConfiguration": null}` になること
- **`GetBucketLogging`**: ログが有効なバケットで、`TargetGrants`(`Grantee` の `Type` と `Permission` の文字列を含む)と `TargetObjectKeyFormat`(`PartitionedPrefix` と `SimplePrefix` の `{}`)が入ること。無効なバケットで `{"LoggingEnabled": null}` になること。HTTP の応答のモック(XML)で `TargetBucket`・`TargetPrefix`・`Grantee` の `xsi:type` を省き、SDK が補った値(`""`、`"no value was set"`)がそのまま入ること(SDK の補い方が変わったら、このテストで検出する)
- **操作とコードの組**: `GetBucketPolicy` が `NoSuchPublicAccessBlockConfiguration` を、`GetPublicAccessBlock` が `NoSuchBucketPolicy` を返した場合に、`null` ではなく `service` の失敗の印になること
- **失敗の印**: あるバケットの 1 つの操作だけが `AccessDenied`・`SlowDown`・`NoSuchBucket`・`PermanentRedirect` を返すとき、そのバケットのそのキーだけが失敗の印(`access_denied`・`throttled`・`service`・`service`)になり、同じバケットのほかの 2 つのキーと、ほかのバケットは値が入ること。メッセージに操作名とコードがあり、サービスのメッセージの本文(ダミーの ARN を入れておく)が含まれないこと
- **リージョンの選び方**: リージョンごとのクライアントを用意する関数を単体テストし、各バケットに渡すクライアントの `config().region()` がそのバケットの `BucketRegion` であること、同じリージョンのバケットでクライアントを共有することを確かめる。モックの規則は要求の入力だけで照合し、送り先のリージョンを見られないため、関数を分けて確かめる
- **列挙の失敗**: `ListBuckets` の 1 ページ目・2 ページ目の失敗(`AccessDenied`、`SlowDown`、`InternalError`)で組全体の失敗になり、種類とメッセージがそのとおりであること
- **値を変換できない要素**: `Name` の無い要素、`BucketRegion` の無い要素、形式の誤った `BucketRegion`(例: `"not a region"`)、整形できない `CreationDate` で組全体の失敗(`invalid_response`)になり、メッセージが上の「値を変換できない場合」の形であること。このとき、バケットごとの操作が呼ばれないこと(規則の `num_calls()`)
- **0 件**: どのページも `Buckets` が空、または返さない場合に空の配列を返し、バケットごとの操作を呼ばないこと
- **並行**: 上限 1 と 8 の `CollectContext`(`with_fan_out_limit`)で、同じ構成情報になること
- **接続できないリージョン**: `region_unavailable` への分類は `support/errors.rs` の判定の表のテストで確かめ、このコレクターでは確かめない。`aws-smithy-mocks` の規則が返せるのは出力・操作のエラー・HTTP の応答(`then_output` / `then_error` / `then_http_response` / `then_compute_response`)で、接続の失敗(`DispatchFailure`)は返せないため。失敗の印の種類がエラーの種類のまま入ることは、上の「失敗の印」のテストで確かめる

### `aws.s3control.account_public_access_block`

- **`AccountId`**: `match_requests` で、`AccountId` が `CollectContext` のアカウント ID(`123456789012`)であること
- 4 項目が入り、返らない項目が `null` になること
- `NoSuchPublicAccessBlockConfiguration`(HTTP 404 の XML のエラー応答と、`then_error` の汎用のエラー)で、`{"GetPublicAccessBlock": null}` のリソースが 1 件できること
- 成功の応答で設定が無い場合に `{"GetPublicAccessBlock": {"PublicAccessBlockConfiguration": null}}` になり、`null` と区別されること
- ほかのエラー(`AccessDenied`、`SlowDown`、`InternalError`)で組全体の失敗になり、種類とメッセージがそのとおりであること

## 既存の設計との差異

この設計で、既存の設計書・全体設計と次の点が変わります。`docs/architecture/` と既存の設計書は、この Issue の PR では変更しません。全体設計の変更が要る行([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「格納の形」)は、別の Issue(#39)で全体設計を先に更新して main へ入れ、この設計のマージはそのあとにします。

| ページ | 既存の記述 | この設計 |
| --- | --- | --- |
| [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「格納の形」 | 一覧・説明の操作の値は、出力の配列の中のそのリソースの要素 | `ListBuckets` の値は、要素に、出力の最上位の `Owner`(一覧のバケットの所有者)を加えたもの。要素ごとに 1 対 1 で付随する出力の最上位の項目を、要素の値に加える扱いを全体設計に加える。全体設計の更新は別の Issue(#39)で行って先に main へ入れ、この設計のマージはそのあとにする |
| [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「S3 のバケット」 | バケットのリージョンに接続できない場合は、そのキーに失敗の印(`region_unavailable`) | 接続の失敗(DNS・接続)と `OptInRequired` は `region_unavailable`。無効なオプトインのリージョンで S3 が返すコードは確かめられていないため、既存の表に当たらないコードは `service` の失敗の印にする(未決事項「接続できないリージョンのエラーのコード」) |
| [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「各コレクターの構成情報の細部」(S3 の分) | 各コレクターの機能設計書で決める | 上の「種別ごとの設計」。`null` にするエラーは Feature-0005 の表の 2 つのコードのまま、操作との組で判定する |
| [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の未決事項「`access_denied` などのコードの過不足」(S3 の分) | 各コレクターの Issue で確かめる | S3・S3 Control のコードで、表への追加は要らない(上の「エラーの種類の判定(S3 のコード)」) |

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| 接続できないリージョンのエラーのコード | アカウントで無効になっているオプトインのリージョンのバケットに要求を送ったとき、S3 がどのコード(または接続の失敗)を返すかは、公式の文書で確かめられなかった。返るコードを `region_unavailable` にするか(`support/errors.rs` の表への追加。ほかのサービスへの影響も確かめる)。実 AWS での確認にはメンテナーの確認が要る | 最初のリリースの前の実環境での確認、またはリージョンのコレクターの Issue で同じ問題を決めるとき |
| `BucketRegion` が欠けた場合の補い方 | 現在は組全体の失敗(`invalid_response`)にする。実際に欠けることがあると分かった場合に、`GetBucketLocation`(`s3:GetBucketLocation`)で補うか、そのバケットだけを失敗として扱う形(リソースの `region` の決め方を含む)にするか | 欠ける応答が報告された時点 |
| 組の中の並行数の実測による見直し(S3) | 8 は往復の時間の見込みと、S3 のバケットの設定の API の制限が公表されていないことからの判断で、実測していない。バケットが多いアカウント(数千〜1 万)での所要時間、`SlowDown` による失敗の印の数を確かめ、S3 だけ上限を変えるか(`CollectContext` の変更になる) | 最初のリリースの前の実環境での確認、またはスロットリング・所要時間の報告があった時点 |
| ディレクトリバケット | S3 Express One Zone のディレクトリバケットを、リージョンの別の種別(例: `aws.s3.directory_bucket`、`ListDirectoryBuckets`、`s3express:ListAllMyDirectoryBuckets`)として集めるか。集める場合、バケットポリシー(`GetBucketPolicy` を `s3express-control` のエンドポイントに送る)などのどの操作を入れるか | ディレクトリバケットを対象にするルールを加えるとき |
| バケットのタグ | `aws.s3.bucket` はタグを取得しない種別で、`tags` は常に `{}`。タグを使うルールが必要になったら、`GetBucketTagging`(タグが無い場合の `NoSuchTagSet` の扱いを含む)をバケットごとに呼ぶか | タグを使うルールを加えるとき |

## 参考資料

- `ListBuckets`(ページング、`MaxBuckets` の範囲と既定のページの大きさ、ページングの無い呼び出しの制限、`BucketRegion`・`Prefix` の意味、`Owner`、`s3:ListAllMyBuckets`、ディレクトリバケットに対応しない): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBuckets.html>
- `Bucket`(`BucketArn` はディレクトリバケットのみ、`BucketRegion` は要求にパラメーターがある場合に返る、`CreationDate` が変わりうる): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_Bucket.html>
- `Owner`: <https://docs.aws.amazon.com/AmazonS3/latest/API/API_Owner.html>
- `GetBucketPolicy`(所有者のアカウントの要件、403 / 405、`s3:GetBucketPolicy`、応答の本文がポリシーの JSON): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketPolicy.html>
- `GetPublicAccessBlock`(バケット。バケット単位の設定だけを返す、最も制限の強い組み合わせ、`s3:GetBucketPublicAccessBlock`): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetPublicAccessBlock.html>
- `GetBucketLogging`(`LoggingEnabled`、無効なときの応答): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetBucketLogging.html>
- S3 Control の `GetPublicAccessBlock`(`AccountId`、組織の単位のポリシーの引き継ぎ、`NoSuchPublicAccessBlockConfiguration` は HTTP 404): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_control_GetPublicAccessBlock.html>
- `ListDirectoryBuckets`(リージョンのエンドポイント、`s3express:ListAllMyDirectoryBuckets`): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListDirectoryBuckets.html>
- ディレクトリバケット(ブロックパブリックアクセスの設定がすべて有効で変更できない): <https://docs.aws.amazon.com/AmazonS3/latest/userguide/directory-buckets-overview.html>
- S3 のエラーのコード(`NoSuchBucketPolicy`、`NoSuchBucket`、`PermanentRedirect`、`SlowDown` など): <https://docs.aws.amazon.com/AmazonS3/latest/API/ErrorResponses.html>
- S3 のエンドポイントとクォータ(S3 Control のエンドポイントはアカウント ID を先頭に付ける、汎用バケットのクォータ 10,000、バケットポリシーの大きさ 20 KB): <https://docs.aws.amazon.com/general/latest/gr/s3.html>
- S3 の性能の目安(プレフィックスごとの要求の速さ、`503 SlowDown`): <https://docs.aws.amazon.com/AmazonS3/latest/userguide/optimizing-performance.html>
- S3 のアクション・リソースの種類・ARN の形式(Service Authorization Reference): <https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazons3.html>
- ARN の形式: <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference-arns.html>
- AWS 管理ポリシー `SecurityAudit`: <https://docs.aws.amazon.com/aws-managed-policy/latest/reference/SecurityAudit.html>
- SDK の型(`aws-sdk-s3` 1.152.0 の `Bucket`、`ListBucketsOutput`、`LoggingEnabled`、`Grantee`、`TargetGrant`、`PublicAccessBlockConfiguration`、ページネーター): <https://docs.rs/aws-sdk-s3/1.152.0/aws_sdk_s3/types/index.html>、<https://docs.rs/aws-sdk-s3/1.152.0/aws_sdk_s3/operation/list_buckets/paginator/struct.ListBucketsPaginator.html>
- SDK の型(`aws-sdk-s3control` 1.133.0 の `PublicAccessBlockConfiguration`): <https://docs.rs/aws-sdk-s3control/1.133.0/aws_sdk_s3control/types/struct.PublicAccessBlockConfiguration.html>
- 既定の再試行の分割の名前(`aws-sdk-s3` の `src/config.rs`): <https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/s3/src/config.rs>
- S3 Control の `GetPublicAccessBlock` の生成コード(`RequiresAccountId`、サービス名 `S3 Control`): <https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/s3control/src/operation/get_public_access_block.rs>
- `aws-smithy-mocks`: <https://docs.rs/aws-smithy-mocks/latest/aws_smithy_mocks/>
