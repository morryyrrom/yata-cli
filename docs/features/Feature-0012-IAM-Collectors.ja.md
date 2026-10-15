# IAM のコレクター

> This Japanese version is the authoritative source. The English version (`Feature-0012-IAM-Collectors.md`) has not been written yet.

IAM の 4 つのリソース種別(`aws.iam.account_summary`、`aws.iam.password_policy`、`aws.iam.user`、`aws.iam.policy`)のコレクターを設計します。認証情報レポート(`GetCredentialReport`)は含めません。

最初のルール群と、使う種別・操作・権限は [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) で決まっています。コレクターの枠組み(`Collector`、`CollectContext`、`CollectError`、失敗の印、エラーの種類の判定、ビルド時の検出、読み取り専用ガード)は [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) に従います。構成情報の形の規則は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、エラーの扱いは [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md)、テストは [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) に従います。

## 目的と背景

- 最初のルール群の CIS 2.4、2.5、2.8、2.9、2.13、2.14 は、IAM の 4 つの種別を使う。ルール(ポリシーのリポジトリ)のフィクスチャを書き始めるには、構成情報の形(全項目、`null` にするエラー、`id` の形式)が先に決まっている必要がある
- `aws.iam.policy` は `ListPolicies` を既定のパラメーターで呼ぶため、AWS 管理ポリシーを含めて 1,000 件を超えるポリシーが返り、ポリシーごとの `GetPolicyVersion` が 1,000 回を超える。順に呼ぶと収集の時間が長くなるため、コレクターの中で並行に呼ぶ
- 次の未決事項を、この設計で決める
  - [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「コレクターの中の並行の呼び出し」と、「`access_denied` などのコードの過不足」のうち IAM の分
  - [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「`aws.iam.policy`」の「呼び出しの並行数・スロットリングの際の再試行の扱い」
  - [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の未決事項「変換の補助」(SDK の型から構成情報への変換を補助する形)

## スコープ

- `src/collectors/iam/` の 4 つのコレクター(`account_summary.rs`、`password_policy.rs`、`user.rs`、`policy.rs`)
- 各種別の構成情報の全項目、`id`・`name`・`tags` の決め方、`null` にするエラー、組全体の失敗と失敗の印に分ける操作、`required_permissions`
- コレクターの中の並行の呼び出し(上限付き)と、その上限の持たせ方(`CollectContext`)
- 再試行を尽くしたスロットリングの扱い(種別・操作ごと)
- IAM のエラーのコードの、エラーの種類への対応(`support/errors.rs` への追加)
- 複数のコレクターが使う変換の小さな関数(`support/convert.rs`)、組の中の並行の実行(`support/fan_out.rs`)、期待するファイルとの比較(`support/golden.rs`)。実装では、これらの共通部分を IAM のコレクターより先の PR に分ける(下の「実装の PR の分け方」)
- 上記のテスト(`aws-smithy-mocks`。実 AWS には接続しない)

## スコープ外

- 認証情報レポート(`GenerateCredentialReport`、`GetCredentialReport`)。`aws.iam.user` の `GetCredentialReport` のキーは [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) で加える。この設計では、そのキーを足せる形にしておく(下の「`aws.iam.user`」の「認証情報レポートを加えるときの形」)
- ルール本体(ポリシーのリポジトリ)
- ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock` の変更。必要な依存はすべて基盤の Issue で入っている(下の「依存」)
- `docs/architecture/` の変更。この設計と全体設計・既存の設計書の差は、下の「既存の設計との差異」にまとめる
- IAM のほかの種別(ロール、グループ、インラインポリシーの文書、タグの取得など)
- CLI のオプションの追加(組の中の並行数を利用者が変える手段は設けない)

## 共通の方針

4 つのコレクターは、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「コレクターの方針」に従います。

- **既定のパラメーターで呼ぶ**: 結果を絞り込む・広げるパラメーターを付けない。付けるのは、ページングの続きのトークン(`Marker`。SDK のページネーターが付ける)と、必須の識別子(`UserName`、`PolicyArn`、`VersionId`)だけ。ページの大きさ(`MaxItems`)も付けない(API の既定は 100 件)
- **絞り込まない・置き換えない**: 返った要素はすべてリソースにする。アタッチされていないポリシー、AWS 管理ポリシー、パスの違うユーザーも入れる
- **許す変換**: SDK の型から JSON への変換、日時の RFC 3339 の文字列への変換、列挙の値の文字列への変換、ポリシー文書の URL デコードと JSON のパースだけ
- **秘密の値も伏せない**: この 4 種別の応答には、パスワードやアクセスキーのシークレットは含まれない。含まれる値(ユーザー名、パス、ポリシーの文書など)は、そのまま入れる
- **ページングの結合**: ページのある操作(`ListUsers`、`ListPolicies`、`ListAttachedUserPolicies`、`ListUserPolicies`)は、SDK のページネーター(`into_paginator()`)で全ページを取り、要素を API が返した順に 1 つの配列につなぐ。ページングの項目(`IsTruncated`、`Marker`)と応答のメタデータは入れない
  - SDK で `Option` の配列(`ListAttachedUserPolicies` の `AttachedPolicies` など)は、ページネーターの `items()` で平らにせず、ページ(`send()` の各出力)ごとに見る。どのページも配列を返さなければ `null`、1 ページでも返せば、返したページの要素をつないだ配列にする。返らなかったものを黙って空の配列にすると、「空なら合格」のルール(2.13 など)が誤って `pass` になりうるため
  - SDK で `Option` でない配列(`ListUserPolicies` の `PolicyNames`、`ListUsers` の `Users`。`aws-sdk-iam` 1.128.0 ではどちらも `Vec`)は、そのままつなぐ
  - `ListPolicies` の `Policies` は SDK で `Option<Vec<Policy>>` だが、要素がリソースそのものになるため、`null` を入れる場所が無い。1 ページでも返せば、返したページの要素をつなぐ。どのページも `Policies` を返さない場合は、組全体の失敗(`invalid_response`)にする(下の「値を変換できない場合」)。AWS 管理ポリシーは常に存在するため、通常は起こらない応答で、0 件として黙って成功すると 2.14 の評価の対象が無くなるため
- **途中のページの失敗**: あるページの取得が失敗したら、それまでのページの結果は使わず、その操作全体を失敗として扱う(組全体の失敗、または失敗の印。一部のページだけを入れることはしない)
- **グローバルの扱い**: 4 種別とも `Scope::Global`。`collect` に渡されるリージョン(`us-east-1`)に向けたクライアントを `ctx.client::<aws_sdk_iam::Client>(region)` で作る。リソースの `region` は `global`

### 値の変換の規則

| SDK の値 | JSON |
| --- | --- |
| 文字列、数値、真偽値 | そのまま |
| `Option` の `None`(API が値を返さなかった) | `null` |
| 日時(`primitives::DateTime`) | `DateTime::fmt(DateTimeFormat::DateTime)` の RFC 3339 の文字列(UTC、`Z`。例: `"2026-01-01T00:00:00Z"`)。秒の小数がある場合は SDK の出力のまま |
| 列挙(`SummaryKeyType`、`PermissionsBoundaryAttachmentType` など) | `as_str()` の文字列。SDK が知らない値(`Unknown`)も、API が返した文字列のまま |
| 文字列 → 整数の写像(`SummaryMap`) | キーを列挙の文字列にし、昇順に並べ替えて入れた JSON のオブジェクト |
| 構造体の配列(`Tags`、`AttachedPolicies` など) | 要素ごとにオブジェクトにした配列 |

- 項目名は API リファレンスのメンバー名(PascalCase)をそのまま使う
- SDK が `Option` でなく値を必ず持つ型にしている項目(`PasswordPolicy` の 6 つの真偽値、`Policy.IsAttachable`、`PolicyVersion.IsDefaultVersion`)は、API が値を返さなかった場合も SDK が `false` にするため、`null` にはならない。SDK の型の制約で、yata の変換では区別できない
- JSON のオブジェクトのキーの順序には意味を持たせず、`serde_json::Map` の実装(既定では名前の昇順、`preserve_order` の機能が有効なら挿入の順)に頼らない。`yata-provider-aws` のテストのビルドでは、開発時の依存(`aws-smithy-mocks` の依存)が `serde_json` の `preserve_order` を有効にするため、テストと本番でキーの順序が変わりうる
  - SDK で `HashMap` の写像(`SummaryMap`)は、反復の順が実行ごとに変わるため、キーを昇順に並べ替えてから JSON のオブジェクトに入れる。どちらのビルドでも同じ順になる
  - 期待するファイルとの比較は、パースした `serde_json::Value` どうしで行う(キーの順序によらない。下の「テスト方針」)
  - スナップショット・レポートの出力でキーの順序をそろえるかは、出力側の責務とし、この設計では決めない(未決事項「出力のキーの順序」)
- 日時の整形は SDK の関数(`DateTime::fmt(DateTimeFormat::DateTime)`)を使い、日時のクレートを新たに足さない。型は SDK のクレートの再エクスポート(`aws_sdk_iam::primitives::{DateTime, DateTimeFormat}` など)で名指しし、`aws-smithy-types` を直接の依存にしない([AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「`crates/yata-provider-aws/Cargo.toml`」)。どの SDK のクレートの `primitives` も同じ `aws_smithy_types` の型を再エクスポートしているため、どのクレートから名指ししても同じ型になる。整形が失敗した場合(年が範囲外など)は、下の「値を変換できない場合」のとおり扱う

### 値を変換できない場合

応答が SDK の型としては受け取れても、構成情報にできない場合は、`invalid_response` として次のように扱います。

| 場合 | 扱い |
| --- | --- |
| リソースを列挙する操作の要素(`ListUsers`、`ListPolicies` の要素)の日時を整形できない | 組全体の失敗(`CollectError`、`invalid_response`) |
| リソースを列挙する操作の要素に、`id` に使う ARN が無い(`ListPolicies` の `Arn` が無い) | 組全体の失敗(`invalid_response`)。`id` を決められず、要素を落とすと絞り込みになるため |
| `ListPolicies` のどのページも `Policies` を返さない | 組全体の失敗(`invalid_response`) |
| リソースごとの操作(`GetPolicyVersion` など)の出力の日時を整形できない | そのキーに失敗の印(`invalid_response`) |
| `GetPolicyVersion` に渡す `DefaultVersionId` が `ListPolicies` の要素に無い | `GetPolicyVersion` を呼ばず、そのキーに失敗の印(`invalid_response`) |

- メッセージは [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の `"<種類の説明> when calling <操作名> (<コード>)"` の形にそろえ、SDK のエラーのコードの代わりに、次の yata の決めた理由のコードを入れる。ARN などのリソースの識別子は含めず、`debug` のログに出す

| 場合 | メッセージ |
| --- | --- |
| `Arn` が無い | `"invalid response when calling ListPolicies (MissingArn)"` |
| どのページも `Policies` を返さない | `"invalid response when calling ListPolicies (MissingPolicies)"` |
| `DefaultVersionId` が無い | `"invalid response when calling ListPolicies (MissingDefaultVersionId)"` |
| 日時を整形できない | `"invalid response when calling <操作名> (InvalidDateTime)"` |

## 種別ごとの設計

### `aws.iam.account_summary`

| 項目 | 内容 |
| --- | --- |
| ファイル | `collectors/iam/account_summary.rs` |
| スコープ | global |
| 操作 | `GetAccountSummary`(パラメーターなし) |
| `required_permissions` | `["iam:GetAccountSummary"]` |
| リソースの件数 | 常に 1 件 |
| `id` | `aws.iam.account_summary:<account_id>:global`(例: `aws.iam.account_summary:123456789012:global`) |
| `name` | なし(キーを出さない) |
| `tags` | `{}` |
| `null` にするエラー | なし |
| 組全体の失敗 | `GetAccountSummary` のすべての失敗 |
| 失敗の印 | なし |

構成情報の形:

```json
{
  "GetAccountSummary": {
    "SummaryMap": {
      "AccountAccessKeysPresent": 0,
      "AccountMFAEnabled": 1,
      "AccountPasswordPresent": 1,
      "Users": 3,
      "UsersQuota": 5000
    }
  }
}
```

(値はダミーで、キーは一部だけを示しています。)

- 値は出力全体(`{"SummaryMap": {...}}`)。`SummaryMap` の全キーを入れる。キーは API リファレンスの `SummaryKeyType` の値(`Users`、`AccountMFAEnabled`、`AccountAccessKeysPresent` など 34 個)で、SDK が知らない新しいキーも文字列のまま入れる
- `SummaryMap` が返らなかった場合は `{"SummaryMap": null}`
- 2.4 は `SummaryMap.AccountAccessKeysPresent`、2.5 は `SummaryMap.AccountMFAEnabled` を使う(値は整数の 0 / 1。真偽値に変えない)

### `aws.iam.password_policy`

| 項目 | 内容 |
| --- | --- |
| ファイル | `collectors/iam/password_policy.rs` |
| スコープ | global |
| 操作 | `GetAccountPasswordPolicy`(パラメーターなし) |
| `required_permissions` | `["iam:GetAccountPasswordPolicy"]` |
| リソースの件数 | 常に 1 件(パスワードポリシーが無い場合も 1 件) |
| `id` | `aws.iam.password_policy:<account_id>:global` |
| `name` | なし |
| `tags` | `{}` |
| `null` にするエラー | `NoSuchEntity`(`ServiceError` で、エラーのコードが `NoSuchEntity`。アカウントにパスワードポリシーが設定されていない) |
| 組全体の失敗 | `NoSuchEntity` 以外の `GetAccountPasswordPolicy` の失敗 |
| 失敗の印 | なし |

構成情報の形:

```json
{
  "GetAccountPasswordPolicy": {
    "PasswordPolicy": {
      "AllowUsersToChangePassword": true,
      "ExpirePasswords": false,
      "HardExpiry": null,
      "MaxPasswordAge": null,
      "MinimumPasswordLength": 14,
      "PasswordReusePrevention": 24,
      "RequireLowercaseCharacters": true,
      "RequireNumbers": true,
      "RequireSymbols": true,
      "RequireUppercaseCharacters": true
    }
  }
}
```

パスワードポリシーが無い場合:

```json
{ "GetAccountPasswordPolicy": null }
```

- 値は出力全体(`{"PasswordPolicy": {...}}`)。`PasswordPolicy` の 10 項目をすべて入れる
- 呼び出しは成功したが `PasswordPolicy` が返らなかった場合は `{"GetAccountPasswordPolicy": {"PasswordPolicy": null}}` とし、`NoSuchEntity` の場合の `{"GetAccountPasswordPolicy": null}` と区別する
- `MinimumPasswordLength`、`MaxPasswordAge`、`PasswordReusePrevention`、`HardExpiry` は、API が返さなければ `null`。ほかの 6 つの真偽値は、上の「値の変換の規則」のとおり SDK が `false` にする
- `NoSuchEntity` の判定は、`ProvideErrorMetadata::code()` が `NoSuchEntity` であることで行う(SDK のエラーの変種 `NoSuchEntityException` には頼らない。[AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「`CollectError` と失敗の印」)

### `aws.iam.user`

| 項目 | 内容 |
| --- | --- |
| ファイル | `collectors/iam/user.rs` |
| スコープ | global |
| 操作 | `ListUsers`(列挙)、`ListAttachedUserPolicies`・`ListUserPolicies`(ユーザーごと。`UserName` は必須の識別子) |
| `required_permissions` | `["iam:ListUsers", "iam:ListAttachedUserPolicies", "iam:ListUserPolicies"]`。認証情報レポートの 2 つ(`iam:GenerateCredentialReport`、`iam:GetCredentialReport`)を加えた 5 つの一覧は [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「構成」 |
| リソースの件数 | `ListUsers` が返したユーザーの数(全ページ) |
| `id` | ユーザーの ARN(`ListUsers` の `Arn`。例: `arn:aws:iam::123456789012:user/example-user`) |
| `name` | `UserName` |
| `tags` | 常に `{}`(タグを取得しない種別。`ListUsers` は API の仕様で `Tags` を返さない。下の注記) |
| `null` にするエラー | なし |
| 組全体の失敗 | `ListUsers` の失敗(どのページでも) |
| 失敗の印 | `ListAttachedUserPolicies`、`ListUserPolicies` の失敗(そのユーザーのそのキーだけ)。`GetCredentialReport` のキーの失敗の印は [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「失敗の印」 |

構成情報の形:

```json
{
  "ListUsers": {
    "Arn": "arn:aws:iam::123456789012:user/example-user",
    "CreateDate": "2026-01-01T00:00:00Z",
    "PasswordLastUsed": "2026-02-01T00:00:00Z",
    "Path": "/",
    "PermissionsBoundary": null,
    "Tags": null,
    "UserId": "AIDAEXAMPLEEXAMPLE01",
    "UserName": "example-user"
  },
  "ListAttachedUserPolicies": {
    "AttachedPolicies": [
      { "PolicyArn": "arn:aws:iam::aws:policy/ReadOnlyAccess", "PolicyName": "ReadOnlyAccess" }
    ]
  },
  "ListUserPolicies": {
    "PolicyNames": ["example-inline-policy"]
  }
}
```

- `ListUsers` の値は、出力の `Users` の中のそのユーザーの要素。SDK の `User` の 8 項目(`Path`、`UserName`、`UserId`、`Arn`、`CreateDate`、`PasswordLastUsed`、`PermissionsBoundary`、`Tags`)をすべて入れる
  - `PermissionsBoundary` が返った場合は `{"PermissionsBoundaryArn": ..., "PermissionsBoundaryType": ...}`、`Tags` が返った場合は `[{"Key": ..., "Value": ...}]`
  - `ListUsers` は API の仕様で `PermissionsBoundary` と `Tags` を返さない(API リファレンスの注記)。このため通常はどちらも `null` で、リソースの `tags` も `{}` になる。ルールは、`null` を「境界・タグが無い」と読んではならない。これらを取るには `GetUser` / `ListUserTags` が要り、この範囲では取らない(未決事項「ユーザーとポリシーのタグ」)
  - `aws.iam.user` は、この範囲ではタグを取得しない種別として扱う。リソースの `tags` は常に `{}` で、「タグが無い」ことを表さない。ルールはこの種別の `tags` を判定に使わない
  - `PasswordLastUsed` は、パスワードを使ったことが無い場合などに返らず `null`
- `ListAttachedUserPolicies` の値は、出力のうちページングの項目を除いたもの(`{"AttachedPolicies": [...]}`。全ページの要素をつなぐ)。要素は `PolicyName`、`PolicyArn`。どのページも `AttachedPolicies` を返さなければ `{"AttachedPolicies": null}`(上の「共通の方針」のページの結合)
- `ListUserPolicies` の値は `{"PolicyNames": [...]}`(全ページをつなぐ)。インラインポリシーの名前だけで、文書(`GetUserPolicy`)は取らない([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「`aws.iam.user`」)
- 2.13 は、`ListAttachedUserPolicies.AttachedPolicies` と `ListUserPolicies.PolicyNames` が空かを見る

図: [構成情報のキー](../diagrams/Diagram-0012-IAM-User-Config-Keys.md)

**処理の順**

1. `ListUsers` の全ページを取る(失敗したら組全体の失敗)
2. ユーザーごとに、`ListAttachedUserPolicies` → `ListUserPolicies` の順に呼ぶ 1 つの仕事を作り、上限付きで並行に実行する(下の「組の中の並行の呼び出し」)。2 つの操作は、それぞれ独立に成功・失敗を記録する(片方の失敗で、もう片方を呼ばないことはしない)
3. ユーザーごとに `Resource` を組み立てる

認証情報レポートを加えた後の処理の順(ユーザーが 0 人ならレポートの操作を呼ばない、手順 2 とレポートの取得を並行に進める、など)は [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「処理の順」です。

**認証情報レポートを加えるときの形**

- 構成情報は操作名をキーにしたオブジェクトなので、認証情報レポートの Issue は、手順 3 の後に `GetCredentialReport` のキーを各ユーザーに足すだけでよい。レポートの行とユーザーの対応づけは、`ListUsers` の `Arn`(= `id`)で行う
- 手順 2 の並行の仕事は、ユーザーごとの 2 つのキーだけを返す形にし、`Resource` の組み立て(手順 3)と分ける。レポートの取得は手順 2 と並行に行う([IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「処理の順」)
- `required_permissions` に `iam:GenerateCredentialReport` と `iam:GetCredentialReport` を足す。キーの追加は項目の追加なので、構成情報の互換性を壊さない

### `aws.iam.policy`

| 項目 | 内容 |
| --- | --- |
| ファイル | `collectors/iam/policy.rs` |
| スコープ | global |
| 操作 | `ListPolicies`(列挙)、`GetPolicyVersion`(ポリシーごと。`PolicyArn` と `VersionId` は必須の識別子) |
| `required_permissions` | `["iam:ListPolicies", "iam:GetPolicyVersion"]` |
| リソースの件数 | `ListPolicies` が返したポリシーの数(全ページ。カスタマー管理ポリシーと AWS 管理ポリシーのすべて) |
| `id` | ポリシーの ARN(`ListPolicies` の `Arn`。例: `arn:aws:iam::aws:policy/AdministratorAccess`、`arn:aws:iam::123456789012:policy/example-policy`) |
| `name` | `PolicyName`(返らなければキーを出さない) |
| `tags` | 常に `{}`(タグを取得しない種別。`ListPolicies` は API の仕様でタグを返さない。下の注記) |
| `null` にするエラー | なし |
| 組全体の失敗 | `ListPolicies` の失敗(どのページでも)。`Arn` の無い要素。どのページも `Policies` を返さない応答(上の「値を変換できない場合」) |
| 失敗の印 | `GetPolicyVersion` の失敗(そのポリシーだけ)。`DefaultVersionId` の無い要素 |

`ListPolicies` は、`Scope`、`OnlyAttached`、`PathPrefix`、`PolicyUsageFilter`、`MaxItems` を付けずに呼びます。API の既定では、カスタマー管理ポリシーと AWS 管理ポリシーのすべてが、アタッチの有無にかかわらず返ります。

構成情報の形:

```json
{
  "ListPolicies": {
    "Arn": "arn:aws:iam::aws:policy/AdministratorAccess",
    "AttachmentCount": 1,
    "CreateDate": "2015-02-06T18:39:46Z",
    "DefaultVersionId": "v1",
    "Description": null,
    "IsAttachable": true,
    "Path": "/",
    "PermissionsBoundaryUsageCount": 0,
    "PolicyId": "ANPAEXAMPLEEXAMPLE01",
    "PolicyName": "AdministratorAccess",
    "Tags": null,
    "UpdateDate": "2015-02-06T18:39:46Z"
  },
  "GetPolicyVersion": {
    "PolicyVersion": {
      "CreateDate": "2015-02-06T18:39:46Z",
      "Document": {
        "Version": "2012-10-17",
        "Statement": [ { "Effect": "Allow", "Action": "*", "Resource": "*" } ]
      },
      "IsDefaultVersion": true,
      "VersionId": "v1"
    }
  }
}
```

- `ListPolicies` の値は、出力の `Policies` の中のそのポリシーの要素。SDK の `Policy` の 12 項目(`PolicyName`、`PolicyId`、`Arn`、`Path`、`DefaultVersionId`、`AttachmentCount`、`PermissionsBoundaryUsageCount`、`IsAttachable`、`Description`、`CreateDate`、`UpdateDate`、`Tags`)をすべて入れる。返らない項目は `null`
- `GetPolicyVersion` は、そのポリシーの `DefaultVersionId` の版を取る。値は出力全体(`{"PolicyVersion": {...}}`)。`PolicyVersion` の 4 項目(`Document`、`VersionId`、`IsDefaultVersion`、`CreateDate`)をすべて入れる。`PolicyVersion` が返らなかった場合は `{"PolicyVersion": null}`
- 既定の版以外の版(`ListPolicyVersions`)は取らない
- `ListPolicies` はタグを返さないため、構成情報の `Tags` は通常 `null`。`aws.iam.policy` も、この範囲ではタグを取得しない種別として扱い、リソースの `tags` は常に `{}` で「タグが無い」ことを表さない。ルールはこの種別の `tags` と `ListPolicies.Tags` を判定に使わない(未決事項「ユーザーとポリシーのタグ」)
- アタッチされているかの絞り込み(`AttachmentCount`)と、AWS 管理ポリシーを対象にするかは、2.14 のルールで行う

**`Document` の変換**

`GetPolicyVersion` の `Document` は、RFC 3986 に従って URL エンコードされた JSON の文字列で返ります(API リファレンスの注記)。次の順に変換します。

1. URL デコード: `%` と 16 進数 2 桁の並びを 1 バイトに戻す。それ以外の文字はそのまま残す(`+` を空白にしない。RFC 3986 のパーセントエンコーディングでは `+` は空白を表さないため)
2. デコードしたバイト列を UTF-8 の文字列にする
3. JSON としてパースし、オブジェクト(など)にする

- どの段階で失敗しても(`%` の後に 16 進数 2 桁が無い、UTF-8 でない、JSON でない)、API が返した文字列(デコードする前のもの)をそのまま `Document` に入れる。失敗の印にはしない(値は API が返した値のままで、ルールは型で見分けられるため)。失敗したことは `debug` のログに出す
- `Document` が返らなかった場合は `null`
- デコードは標準ライブラリだけで書き、URL デコードのクレートを足さない(処理が上の 3 段で小さく、依存の追加は共有ファイルの変更になるため)。関数は `collectors/iam/policy.rs` に置く(この Issue で使うのはこのコレクターだけ。インラインポリシーの文書を取る種別などで共有が必要になった時点で `support/` に移す)
- AWS SDK for Rust は `Document` を自動ではデコードしない前提で設計する。SDK の生成コードに IAM の文書をデコードする処理が無いことを、下の「テスト方針」の HTTP の応答のモック(エンコードされた文書を返す)で確かめる。SDK が自動でデコードするように変わると、二重にデコードして `%` を含む文書が壊れるため、このテストで検出する

図: [収集の流れ](../diagrams/Diagram-0012-IAM-Policy-Collection-Sequence.md)

**処理の順**

1. `ListPolicies` の全ページを取る(失敗したら組全体の失敗)。`Arn` の無い要素があれば組全体の失敗(`invalid_response`)
2. ポリシーごとに、`GetPolicyVersion`(`PolicyArn` = `Arn`、`VersionId` = `DefaultVersionId`)を呼ぶ仕事を作り、上限付きで並行に実行する(下の「組の中の並行の呼び出し」)
3. ポリシーごとに `Resource` を組み立てる

## 組の中の並行の呼び出し

### 方式

コレクターの中で、リソースごとの付随情報を取る操作を、上限付きで並行に呼べるようにします。この Issue では `aws.iam.user`(ユーザーごとの 2 つの操作)と `aws.iam.policy`(`GetPolicyVersion`)が使います。

- `src/support/fan_out.rs` に、次の関数を置く(名前・シグネチャの細部は実装時に変えてよい)

```rust
/// Runs `task` for every item with at most `limit` tasks running at once and returns
/// the outputs in the order of `items`.
pub(crate) async fn fan_out<I, T, F, Fut>(
    limit: NonZeroUsize,
    items: Vec<I>,
    task: F,
) -> Result<Vec<T>, CollectError>
where
    I: Send + 'static,
    T: Send + 'static,
    F: FnMut(I) -> Fut,
    Fut: Future<Output = T> + Send + 'static;
```

- `tokio::task::JoinSet` で仕事をタスクとして実行する。`items` の順に生成し、実行中のタスクが `limit` に達したら、どれかが終わるまで次を生成しない(同時に実行中のタスクが `limit` を超えない)
- 結果は `items` と同じ順に並べて返す。終わる順にはよらないので、構成情報は実行のたびに同じになる
- 仕事(`task`)は SDK のエラーを自分で失敗の印などに変え、失敗しない値(各キーの JSON の値)を返す。このため、1 つのリソースの失敗はほかのリソースの仕事を止めない
- タスクのパニック(`JoinError`)は、その組全体の失敗(`CollectError`、`service`、メッセージ `"collector panicked"`)にして返す。残りのタスクは `JoinSet` の破棄で中止する(基盤のタスクのパニックの扱いと同じ)
- 組のタスク(コレクターの future)が破棄されると(Ctrl-C による中断など)、`JoinSet` も破棄され、実行中の仕事はすべて中止される。中止される呼び出しはすべて読み取りの操作なので、AWS 側に影響は残らない
- タスクは `'static` の future にするため、仕事にはクライアント(`aws_sdk_iam::Client`。中身は共有の参照で、複製は安い)の複製と、所有する文字列(`UserName`、`PolicyArn` など)を渡す
- 使う tokio の機能は `JoinSet`(`rt`。ワークスペースの `rt-multi-thread` で有効)だけで、新しい機能・クレートを足さない。並行数の窓は `JoinSet` の中のタスクの数で数えるため、`Semaphore` は使わない

### 上限の持たせ方(`CollectContext`)

- `CollectContext` に、組の中の並行数の上限を返す `fan_out_limit() -> NonZeroUsize` を加える
- 値は `context.rs` の定数 `FAN_OUT_LIMIT`(8)。`CollectContext::new` はこの値を使い、`new` の引数は変えない(`runner.rs` を変更しないため)
- テストのときだけ、上限を変えた `CollectContext` を作る関数(`#[cfg(test)]`)を置き、上限 1 と 8 で同じ構成情報になることを確かめる
- コレクターは `fan_out(ctx.fan_out_limit(), items, task)` の形で使い、上限の値をコレクターに書かない
- 上限は組(1 つのコレクターの 1 回の実行)ごとに数える。コレクターの間で共有する枠にはしない。AWS の制限の単位はサービス・API・リージョンで、共有の枠にすると、関係の無いサービス・リージョンのコレクターが互いの待ちになるため

### `--concurrency` との関係

- `--concurrency` は、同時に実行する組(タスク)の数の上限で、[AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) のセマフォの許可で数える。組の中の仕事は、この許可を取らない
- このため行き詰まりは起こらない: 組のタスクは許可を 1 つ持ったまま組の中の仕事の終わりを待つが、組の中の仕事は許可を待たず、ほかのどの待ちにも依存しない
- 同時に送る API の呼び出しの数の上限は、「実行中の組のうち並行に呼ばない組の数 + 並行に呼ぶ組の数 × 8」になる。IAM の 4 つの組が同時に動くと、IAM への同時の呼び出しは最大 1 + 1 + 8 + 8 = 18。認証情報レポートの取得を加えると、`aws.iam.user` の組でレポートの呼び出しの 1 が加わり、最大 19 になる([IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「処理の順」)
- `--concurrency` を小さくしても、組の中の並行数は変わらない(`--concurrency 1` でも、`aws.iam.policy` の組は最大 8 の呼び出しを同時に送る)。利用者が組の中の並行数を変える手段は設けない(未決事項「組の中の並行数を利用者が変える手段」)

図: [並行の構造](../diagrams/Diagram-0012-IAM-Concurrency-Structure.md)

### 上限の値: 8

- **時間**: 1 回の呼び出しの往復を 100〜300 ms(端末から `us-east-1` までの距離による。実測ではなく見込み)とすると、ポリシーが 1,500 件のとき、`GetPolicyVersion` を順に呼ぶと 150〜450 秒、8 並行で 19〜56 秒、16 並行で 9〜28 秒。8 で、`aws.iam.policy` の組を 1 分前後に収め、ほかの組(リージョンの組 170 前後)の収集とおおむね同じ時間に終えられる
- **IAM の制限**: IAM の API のリクエストの制限は公開されていない。IAM はグローバルなサービスで、制限はアカウントの単位で掛かると見られ、同じアカウントのほかの利用者(デプロイのパイプラインなど)と枠を分け合う。並行数を増やすほど、スロットリングに当たる見込みと、ほかの利用者への影響が大きくなる。16 にしても時間の短縮は半分に満たない(往復の時間の見込みの幅の方が大きい)ため、控えめな 8 にする
- **並行に呼ぶ組が 2 つあること**: `aws.iam.user` と `aws.iam.policy` が同時に動くため、IAM への同時の呼び出しは最大 18(認証情報レポートを加えた後は 19)になる。基盤の `--concurrency` の既定値(16)と同じ程度の同時の接続数で、手元の資源の点でも問題にならない
- **見直し**: 実際のアカウントでスロットリングによる失敗の印が多い場合や、時間が問題になる場合は、値を見直す(未決事項「組の中の並行数の実測による見直し」)

### 件数の見込みと所要時間

| 種別 | 呼び出しの回数 | 所要時間の見込み(往復 100〜300 ms) |
| --- | --- | --- |
| `aws.iam.account_summary` | 1 | 1 秒未満 |
| `aws.iam.password_policy` | 1 | 1 秒未満 |
| `aws.iam.user`(ユーザー U 人) | `ListUsers` が U / 100 ページ(順に)+ ユーザーごとに 2 回(8 並行。ページがあればその分増える) | U = 100 で 3〜8 秒、U = 1,000 で 25〜80 秒 |
| `aws.iam.policy`(ポリシー P 件) | `ListPolicies` が P / 100 ページ(順に)+ P 回の `GetPolicyVersion`(8 並行) | P = 1,500 で 20〜60 秒 |

- P は、AWS 管理ポリシー(1,000 件を超える。アカウントによらずほぼ同じ)と、カスタマー管理ポリシー(既定のクォータは 1,500、上限 10,000)の和
- `ListUsers`・`ListPolicies` のページは、続きのトークンが前のページの応答にあるため、順に取る
- IAM の組は、基盤の実行の順で最初に始まる(グローバルな組を先に始める)。収集の全体の時間の下限は、`aws.iam.policy` の組(または大きなアカウントの `aws.iam.user` の組)で決まる
- メモリ: ポリシーの文書(1 件あたり数 KB 以下。カスタマー管理ポリシーは 6,144 文字以下)を全件持つため、ポリシーが数千件でも数十 MB に収まる見込み

## 再試行とスロットリング

- 再試行は [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の決定のまま、SDK の既定(standard、最大 3 回の試行)とし、コードで `retry_config` を設定しない。利用者は AWS の標準の設定(`AWS_MAX_ATTEMPTS`、`AWS_RETRY_MODE`、プロファイルの `max_attempts`・`retry_mode`)で変えられる
- **再試行の割当ての共有**: SDK の再試行の割当て(トークンバケット)は、再試行の分割(retry partition)ごとに、プロセス全体で 1 つだけある。SDK のソース(下の版)では次のとおり
  - `aws-sdk-iam` 1.128.0 の `src/config.rs`: 既定の分割の名前は `<サービス>-<リージョン>`(`format!("{default_retry_partition}-{region}")`)。IAM のクライアントは `us-east-1` に向けるため、`iam-us-east-1`
  - `aws-smithy-runtime` 1.16.0 の `client/retries/strategy/standard.rs`: トークンバケットはプロセス全体の静的な表(`static TOKEN_BUCKET: StaticPartitionMap<RetryPartition, TokenBucket>`)に、分割ごとに入る
  - このため、IAM の 4 つの組と、組の中の並行の仕事のすべてが、1 つの割当てを共有する(クライアントを組ごとに作っても分かれない)
- **ほかの IAM の組への影響**: `aws.iam.policy` の `GetPolicyVersion` などでスロットリングが続いて割当てが尽きると、以後は IAM のどの呼び出しのスロットリングも再試行されずにすぐ失敗する。同じ時期に動く `aws.iam.account_summary`・`aws.iam.password_policy` の呼び出しがスロットリングに当たると、組全体の失敗になり、2.4・2.5・2.8・2.9 が `skipped` になりうる。いずれも失敗として記録され、黙って `pass` にはならない。実測による見直し(未決事項「組の中の並行数の実測による見直し」)では、失敗の印の数に加えて、ほかの IAM の組の失敗も確かめる
- 再試行を尽くしたスロットリング(種類 `throttled`)の扱いは、ほかのエラーと同じく、操作の種類で決まる

| 種別 | 操作 | 再試行を尽くしたスロットリング | ルールへの影響 |
| --- | --- | --- | --- |
| `aws.iam.account_summary` | `GetAccountSummary` | 組全体の失敗(`CollectionError`、`throttled`) | 2.4、2.5 が `skipped`(`collection_failed`) |
| `aws.iam.password_policy` | `GetAccountPasswordPolicy` | 組全体の失敗(`throttled`) | 2.8、2.9 が `skipped` |
| `aws.iam.user` | `ListUsers`(どのページでも) | 組全体の失敗(`throttled`) | `aws.iam.user` を使うルールが `skipped` |
| `aws.iam.user` | `ListAttachedUserPolicies`、`ListUserPolicies`(どのページでも) | そのユーザーのそのキーに失敗の印(`throttled`) | `aws.iam.user` を使うルールが `incomplete` |
| `aws.iam.policy` | `ListPolicies`(どのページでも) | 組全体の失敗(`throttled`) | 2.14 が `skipped` |
| `aws.iam.policy` | `GetPolicyVersion` | そのポリシーのキーに失敗の印(`throttled`) | 2.14 が `incomplete` |

- 失敗の印を置いた後も、ほかのリソースの仕事は続ける(早く打ち切らない)。一時的なスロットリングで一部だけが失敗した場合に、ほかのリソースを評価できるようにするため
- 失敗の印が 1 つ以上ある操作は、コレクターが操作ごとに 1 行の `warn` のログを出す(例: `"12 of 1500 GetPolicyVersion calls failed (throttled: 12)"`)。ARN などは `debug` にだけ出す。[エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) の「`warn`: 収集の部分的な失敗」に当たる

## エラーの種類の判定(IAM のコード)

[AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「エラーの種類の判定」の表を、IAM のエラーのコード(API リファレンスの各操作の Errors と Common Error Types)と照らしました。

| コード | 返る場面 | 種類 | 表への追加 |
| --- | --- | --- | --- |
| `AccessDenied` | 権限不足(IAM の実際の応答のコード) | `access_denied` | 不要(既にある) |
| `AccessDeniedException` | 権限不足(Common Error Types) | `access_denied` | 不要 |
| `NotAuthorized` | 権限不足(Common Error Types。HTTP 401) | `access_denied` | **追加する** |
| `Throttling`、`ThrottlingException` | 要求の速さの制限 | `throttled` | 不要 |
| `NoSuchEntity` | `GetAccountPasswordPolicy`: 設定が無い。ほかの操作: 一覧の後にユーザー・ポリシーが削除された | `GetAccountPasswordPolicy` では `null`。ほかの操作では `service`(失敗の印) | 不要 |
| `ServiceFailure`、`InternalFailure`、`ServiceUnavailable` | サービスの内部の失敗(SDK が再試行した後) | `service` | 不要 |
| `InvalidInput`、`ValidationError` など | 入力の誤り(コレクターの誤り) | `service` | 不要 |
| `ExpiredToken`、`ExpiredTokenException`、`InvalidClientTokenId`、`UnrecognizedClientException`、`SignatureDoesNotMatch`、`IncompleteSignature`、`MissingAuthenticationToken` | 認証情報の失効・誤り | `service` | 不要 |

- `support/errors.rs` の `ACCESS_DENIED_CODES` に `NotAuthorized` を加え、判定の表のテストに行を足す。ほかのサービスのコレクターにも効くが、どのサービスでも意味は「権限が無い」なので、誤った分類にはならない
- 認証情報の失効・誤りのコードは、IAM では `region_unavailable` にしない。IAM の API は常に `us-east-1`(パーティションの既定リージョン。すべてのアカウントで有効)に送るため、これらのコードがリージョンの未有効を表すことは無い。収集の途中で一時的な認証情報が失効した場合は `service` になり、メッセージのコード(`ExpiredToken` など)で原因が分かる。リージョンのコレクターでこれらを `region_unavailable` にするかは、[AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の未決事項のまま、リージョンのコレクターの Issue で決める
- `NoSuchEntity` を「設定が存在しない」とするのは `GetAccountPasswordPolicy` だけとする。`ListAttachedUserPolicies`・`ListUserPolicies`・`GetPolicyVersion` の `NoSuchEntity` は、一覧を取った後にユーザー・ポリシーが削除された場合に起こる。値を `null`(「無い」)にすると、2.13 などで誤って `pass` になりうるため、失敗の印(`service`)にする。リソースそのものを落とすことも、絞り込みになるためしない

図: [操作ごとのエラーの振り分け](../diagrams/Diagram-0012-IAM-Error-Routing.md)

## 構成

```
crates/yata-provider-aws/
├── src/
│   ├── collector.rs               # 変更: doc コメントの「コレクターの中の呼び出しは順に行う」を、fan_out による並行の規則に直す
│   ├── context.rs                 # 変更: FAN_OUT_LIMIT、fan_out_limit()、テスト用の上限の変更
│   ├── support.rs                 # 変更: convert、fan_out、golden(#[cfg(test)])のモジュールの宣言を足す
│   ├── support/
│   │   ├── convert.rs             # 新規: 日時の RFC 3339 の文字列、Option → null、HashMap → キーを並べ替えたオブジェクトなどの小さな関数
│   │   ├── fan_out.rs             # 新規: 組の中の上限付きの並行の実行
│   │   ├── golden.rs              # 新規(#[cfg(test)]): コレクターの出力と期待するファイルの比較(パースした Value どうし)・更新(キーを並べ替えた正規の形で書く)
│   │   └── errors.rs              # 変更: NotAuthorized を access_denied に。CollectError から失敗の印を作る関数
│   └── collectors/
│       └── iam/
│           ├── account_summary.rs # 新規
│           ├── password_policy.rs # 新規
│           ├── user.rs            # 新規
│           └── policy.rs          # 新規(Document の URL デコードを含む)
└── tests/
    └── golden/
        └── iam/
            ├── account_summary.json
            ├── password_policy.json
            ├── user.json
            └── policy.json
```

- 4 つのコレクターは build.rs が `src/collectors/iam/*.rs` から検出し、`aws.iam.<resource>` の種別になる。一覧・`mod` の宣言・`Cargo.toml` に追記しない
- 各コレクターのファイルは `pub(crate) fn collector() -> Box<dyn Collector>` を定義する
- `src/collectors/iam/` には、コレクターではない `.rs` を置けない(検出の規則で、置いた `.rs` はすべてコレクターとして扱われる)。このため、IAM の 2 つ以上のコレクターが使う処理(日時の変換など)は `support/` に置く。1 つのコレクターだけが使う処理(`Document` のデコード、ユーザーごとの 2 つの操作の組み立て)は、そのコレクターのファイルに置く。1 つのコレクターだけが使う処理でも `support/` に置く例外の基準は、[IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「既存の設計との差異」の「構成」の行を参照する
- **変換の補助**([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の未決事項)は、マクロではなく、`support/convert.rs` の小さな関数とする。SDK の構造体から JSON のオブジェクトを作る部分は、各コレクターが `serde_json::json!` で項目を明示して書く。項目を明示することで、機能設計書の項目の一覧とコードレビューで「全項目を入れたか」を照らし合わせやすくするため
  - `convert.rs` の関数は SDK のサービスに依存しない形にする(日時の型は SDK のクレートの再エクスポート(例: `aws_sdk_sts::primitives::DateTime`。`errors.rs` が `aws_sdk_sts::error` を使うのと同じ形)で名指しする。どの SDK のクレートの再エクスポートも同じ `aws_smithy_types::DateTime` なので、どのサービスのコレクターからも同じ関数を使える)
- `support/errors.rs` には、`CollectError` から失敗の印の値を作る関数(例: `CollectError::to_failure_mark()`)を加え、既存の `failure_mark` もこれを使う形にする。`invalid_response` の失敗の印(SDK のエラーではないもの)を作るため
- 期待するファイルは `crates/yata-provider-aws/tests/golden/<service>/<resource>.json` に置く(Cargo は `tests/` の下のディレクトリの JSON をテストとしてビルドしない)。サービスごとのディレクトリに分け、並列のコレクターの PR で同じファイルを触らないようにする
### 実装の PR の分け方

設計はこの Issue でまとめて決めますが、実装は、ほかのコレクターの Issue も使う共通部分を先に main へ入れ、その後に IAM のコレクターを入れる 2 つの PR に分けます。

| PR | 含めるもの | 順 |
| --- | --- | --- |
| 1. 共通部分 | `support/fan_out.rs`(新規)、`support/convert.rs`(新規)、`support/golden.rs`(新規、`#[cfg(test)]`)、`support.rs`(3 つのモジュールの宣言)、`support/errors.rs`(`NotAuthorized` の追加、`CollectError::to_failure_mark()`、`failure_mark` の書き換え、判定の表のテスト)、`context.rs`(`FAN_OUT_LIMIT`、`fan_out_limit()`、テスト用の上限の変更)、`collector.rs`(doc コメントの規則の変更)と、それぞれの単体テスト | 先に main へ入れる |
| 2. IAM のコレクター | `collectors/iam/account_summary.rs`、`password_policy.rs`、`user.rs`、`policy.rs`(新規)と、`tests/golden/iam/*.json`(新規) | 1 のマージの後、main を取り込んでから入れる |

- PR 1 は共通部分だけで、IAM の種別を加えない(`supported_resource_types()` は変わらない)。PR 1 の中では、共通部分の関数をテスト用の偽の仕事・値で確かめる(下の「テスト方針」の「`support/` の追加」)
- PR 1 の新しいファイル(`support/fan_out.rs`、`support/convert.rs`)には、既存の `context.rs`・`support/errors.rs` と同じく、ファイルの先頭に `#![allow(dead_code)]` を付ける(使うコレクターが無い PR 1 の時点で、テスト以外のビルドの `dead_code` の警告により `cargo clippy ... -D warnings` が通らなくなるため。`support/golden.rs` は `#[cfg(test)]` なので不要)。PR 2 では外さない(外すのは、共通部分を使うコレクターがそろった後の整理の PR)
- PR 2 は `src/collectors/iam/` と `tests/golden/iam/` だけに触れ、共有のファイルを変更しない。PR 2 で共通部分の変更が必要になったら、作業を止めて報告し、別の PR にする
- ほかのコレクターの Issue(認証情報レポート、S3、CloudTrail、監視、EC2 など)は、PR 1 のマージの後に main を取り込み、組の中の並行(`fan_out`、`fan_out_limit()`)、日時などの変換(`convert`)、期待するファイルとの比較(`golden`)、SDK のエラーでない失敗の印(`to_failure_mark()`)を、この共通部分で行う。同じ役割の補助を自前で作らない。足りない機能は、共通部分への追加として別の PR にする

### 依存

- 使うクレートはすべて基盤の Issue で入っている(`aws-sdk-iam`、`serde_json`、`tokio`、`tracing`。テストでは `aws-sdk-iam` の `test-util`、`aws-smithy-mocks`、`tokio` の `time`・`test-util`)
- ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock` は変更しない
- 日時の整形・URL デコード・並行の実行に、新しいクレートを足さない(上の各節)

## 使う AWS API と IAM 権限

| 種別 | API | 呼ぶ回数 | IAM 権限 |
| --- | --- | --- | --- |
| `aws.iam.account_summary` | `GetAccountSummary` | 1 | `iam:GetAccountSummary` |
| `aws.iam.password_policy` | `GetAccountPasswordPolicy` | 1 | `iam:GetAccountPasswordPolicy` |
| `aws.iam.user` | `ListUsers` | ページの数 | `iam:ListUsers` |
| `aws.iam.user` | `ListAttachedUserPolicies` | ユーザーごと(ページの数) | `iam:ListAttachedUserPolicies` |
| `aws.iam.user` | `ListUserPolicies` | ユーザーごと(ページの数) | `iam:ListUserPolicies` |
| `aws.iam.policy` | `ListPolicies` | ページの数 | `iam:ListPolicies` |
| `aws.iam.policy` | `GetPolicyVersion` | ポリシーごと | `iam:GetPolicyVersion` |

- どれも `Get` / `List` で始まる読み取りの操作で、読み取り専用ガードの接頭辞の規則で許可される。拒否リスト・例外リストの変更は要らない
- 認証情報レポートの操作(`GenerateCredentialReport`、`GetCredentialReport`)は [IAM の認証情報レポートの収集](Feature-0013-IAM-Credential-Report.ja.md) の「使う AWS API と IAM 権限」
- 各コレクターの `required_permissions` は上の表の権限で、基盤の既存のテスト(`required_permissions` の形、拒否リストの権限を含まないこと)の対象に自動でなる
- AWS 管理ポリシー `SecurityAudit` は、これらの権限をすべて含む

## テスト方針

[テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) の「コレクターのテスト」に従い、実 AWS には接続しません。テストは各コレクターのファイルの中の単体テスト(`#[cfg(test)] mod tests`)に置き、基盤の `support::clients::testing::mock_factory`(ダミーの認証情報、モックの HTTP クライアント、再試行なし、規則は `RuleMode::MatchAny`)でクライアントを作ります。アカウント ID は `123456789012`、ARN・名前・ID はすべてダミーの値にします。

### 各コレクター共通

- **構成情報の形**: モックの応答から作った `Vec<Resource>`(`id` の昇順)の JSON を、`tests/golden/iam/<resource>.json` と比べる。比べるのは、両方をパースした `serde_json::Value` どうしで、文字列としては比べない(テストのビルドでは `serde_json` の `preserve_order` が有効になり、キーの順序が本番と違うため)。期待するファイルは、環境変数 `YATA_UPDATE_GOLDEN=1` を付けて実行したときだけ書き換える。書き出すときは、オブジェクトのキーを再帰的に昇順に並べ替えた形(整形した JSON)にし、実行のたびに同じファイルになるようにする。差分が項目の追加以外を含む場合は、互換性の無い変更として扱う([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「構成情報の互換性」)
- **`id`・`type`・`account`・`region`・`name`・`tags`**: 上の各表のとおりであること
- **列挙の操作の失敗**: `AccessDenied`(`access_denied`)、`Throttling`(`throttled`)、`ServiceFailure`(`service`)、`NotAuthorized`(`access_denied`)を返すと、`CollectError` の種類がそのとおりで、メッセージに操作名とコードがあり、サービスのメッセージの本文(ダミーの ARN を入れておく)が含まれないこと
- **ガード**: コレクターが呼ぶ操作が、すべて送信の段階に達すること(`mock_factory_counted` の記録)。読み取り専用ガードに止められないこと

### `aws.iam.account_summary`

- `SummaryMap` のすべてのキーが入ること。HTTP の応答のモック(XML)で、SDK が知らないキー(例: `ExampleFutureKey`)を返し、そのキーも文字列のまま入ること
- `SummaryMap` が無い応答で `{"SummaryMap": null}` になること
- `SummaryMap` のキーが、応答の順・`HashMap` の反復の順によらず昇順に入ること(構成情報の `Value` のキーの並びで確かめる)

### `aws.iam.password_policy`

- 10 項目が入り、返らない `Option` の項目が `null` になること
- `NoSuchEntity`(HTTP 404 の XML のエラー応答)で、`{"GetAccountPasswordPolicy": null}` のリソースが 1 件できること
- 成功の応答で `PasswordPolicy` が無い場合に `{"GetAccountPasswordPolicy": {"PasswordPolicy": null}}` になり、`NoSuchEntity` の `null` と区別されること
- `NoSuchEntity` 以外のエラー(`AccessDenied`、`ServiceFailure`)で組全体の失敗になること

### `aws.iam.user`

- **件数**: `ListUsers` を 2 ページ(`IsTruncated` と `Marker`)で返し、全ユーザーがリソースになること(モックの件数 = リソースの件数)。パスの違うユーザーも入ること
- **既定のパラメーター**: `match_requests` で、`ListUsers` に `PathPrefix`・`MaxItems` が無いこと、`ListAttachedUserPolicies` に `PathPrefix`・`MaxItems` が無く `UserName` が一覧のユーザー名であること、`ListUserPolicies` に `MaxItems` が無いことを確かめる(合わない要求には規則が当たらず、テストが失敗する)
- **ページの規則の見分け方**: `mock_factory` は `RuleMode::MatchAny` で、同じ操作の規則を見分けないと 1 ページ目が繰り返し当たる。1 ページ目の規則は「`Marker` が無い」、2 ページ目の規則は「`Marker` が 1 ページ目の応答の値と等しい」で照合する。既定のパラメーターの検査(`PathPrefix`・`MaxItems` などが無いこと)は、すべてのページの規則の照合に入れる。`aws.iam.policy` の `ListPolicies` も同じ
- **返らない配列**: `ListAttachedUserPolicies` のどのページも `AttachedPolicies` を返さない応答(HTTP の応答のモックで要素を省く)で `{"AttachedPolicies": null}` になり、空の配列にならないこと。1 ページ目だけが返す場合は、そのページの要素の配列になること
- **ページングの結合**: `ListAttachedUserPolicies` を 2 ページで返し、要素が順につながること
- **失敗の印**: あるユーザーの `ListUserPolicies` だけが `AccessDenied` のとき、そのユーザーのそのキーだけが失敗の印になり、同じユーザーの `ListAttachedUserPolicies` と、ほかのユーザーは値が入ること。`ListAttachedUserPolicies` の 2 ページ目の失敗で、1 ページ目の要素を入れずに失敗の印になること。`NoSuchEntity` で失敗の印(`service`)になること。`Throttling` で `throttled` の失敗の印になること
- **列挙の失敗**: `ListUsers` の 2 ページ目の失敗で組全体の失敗になること
- **並行**: 上限 1 と 8 の `CollectContext` で、同じ構成情報になること
- 0 人のときに空の配列を返すこと

### `aws.iam.policy`

- **件数**: `ListPolicies` を 2 ページで返し、AWS 管理ポリシー(`arn:aws:iam::aws:policy/...`)、`AttachmentCount` が 0 のポリシー、カスタマー管理ポリシーのすべてがリソースになること
- **既定のパラメーター**: `match_requests` で、`ListPolicies` に `Scope`・`OnlyAttached`・`PathPrefix`・`PolicyUsageFilter`・`MaxItems` が無いこと。`GetPolicyVersion` の `PolicyArn` がポリシーの `Arn`、`VersionId` が `DefaultVersionId` であること
- **`Document`**: HTTP の応答のモック(XML)で、URL エンコードされた文書(`%7B%22Version%22...`。日本語などの複数バイトの文字と `+` を含める)を返し、デコードしてパースしたオブジェクトが入ること(SDK が自動でデコードしないことの確認を兼ねる)。デコードの関数の表のテスト(`%` の後に 16 進数が無い、`%zz`、UTF-8 でない、JSON でない場合に元の文字列のまま、`+` を空白にしない)
- **失敗の印**: あるポリシーの `GetPolicyVersion` だけが `Throttling`・`AccessDenied`・`NoSuchEntity` のとき、そのポリシーのキーだけが失敗の印になること。`DefaultVersionId` の無い要素で `GetPolicyVersion` を呼ばず(規則の `num_calls()` で確かめる)、`invalid_response` の失敗の印になること
- **返らない値**: `GetPolicyVersion` の成功の応答で `PolicyVersion` が無い場合に `{"PolicyVersion": null}` になること
- **列挙の失敗**: `ListPolicies` の失敗、`Arn` の無い要素で組全体の失敗になること。メッセージが `"invalid response when calling ListPolicies (MissingArn)"` の形であること。どのページも `Policies` を返さない応答(HTTP の応答のモックで要素を省く)で、組全体の失敗(`"invalid response when calling ListPolicies (MissingPolicies)"`)になること。2 ページのうち 1 ページだけが `Policies` を返す場合は、そのページの要素がリソースになること
- **並行**: 上限 1 と 8 で同じ構成情報になること

### `support/` の追加

- `fan_out`: 偽の仕事(`tokio::time::sleep` の遅延と原子的なカウンター。`tokio::time::pause` で時間を進める)で、同時に実行中の仕事が上限を超えないこと、結果が入力の順に並ぶこと、空の入力で空の結果になること、仕事のパニックで `service` の `CollectError` になること、`fan_out` の future を破棄すると実行中の仕事が中止されること(仕事が持つ値の `Drop` で確かめる)
- `convert`: 日時が `Z` 付きの RFC 3339 の文字列になること(秒の小数がある場合を含む)。`HashMap` から作ったオブジェクトのキーが昇順になること
- `golden`: キーの順序だけが違う JSON を一致とみなすこと。書き出したファイルのキーが再帰的に昇順であること
- `errors`: 判定の表のテストに `NotAuthorized` → `access_denied` の行を足す。`to_failure_mark()` が `{"yata_error": {"kind", "message"}}` だけを持つこと

## 既存の設計との差異

この設計で、既存の設計書・全体設計と次の点が変わります。`docs/architecture/` と既存の設計書は、この Issue の PR では変更しません。

| ページ | 既存の記述 | この設計 |
| --- | --- | --- |
| [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「`Collector` トレイト」 | コレクターの中の API の呼び出しは順に行う(並行に呼ばない)。並行に呼ぶ場合の扱いは未決 | リソースごとの付随情報を取る操作は、`support::fan_out` と `CollectContext::fan_out_limit()`(8)で上限付きで並行に呼んでよい。`--concurrency` の許可とは別に、組ごとに数える。並行に呼ぶかは各コレクターの機能設計書で決める |
| [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「`CollectContext`」 | `account_id`、`account_key`、`client` | `fan_out_limit()` を加える |
| [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「エラーの種類の判定」 | `access_denied` のコードは `AccessDenied`、`AccessDeniedException`、`UnauthorizedOperation`、`AuthorizationError` | `NotAuthorized` を加える |
| [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の「再試行の回数」 | 再試行の割当て(トークンバケット)はクライアントのインスタンスごと。adaptive のクライアント側の速さの制限もクライアントのインスタンスごとで、組ごとにクライアントを作るため効果が小さい | どちらも、サービス × リージョンの再試行の分割ごと(IAM は `iam-us-east-1`)に、プロセス全体で共有される(トークンバケットは `TOKEN_BUCKET`、速さの制限は `CLIENT_RATE_LIMITER` の、どちらも `StaticPartitionMap`)。組ごとにクライアントを作っても分かれない(上の「再試行とスロットリング」) |
| [初期依存クレート](../architecture/Architecture-Initial-Dependencies.ja.md) の未決事項「日時の整形に使うクレート」、[AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) のスコープ外(日時の RFC 3339 の整形は後の Issue) | 日時の整形のクレートは未決 | コレクターの構成情報の日時は、SDK の `DateTime::fmt(DateTimeFormat::DateTime)` で整形し、クレートを足さない。`yata` 自身の日時(スナップショット・出力の `started_at` など)の整形と解析に使うクレートの選定は、未決のまま |
| [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の未決事項「変換の補助」 | 最初のコレクターを実装する Issue で決める | `support/convert.rs` の小さな関数とし、マクロは使わない。構造体の項目は各コレクターが明示して書く |
| [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「`aws.iam.policy`」 | 呼び出しの並行数・スロットリングの際の再試行の扱いは後続の Issue で決める | 上の「組の中の並行の呼び出し」「再試行とスロットリング」 |
| [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) の「コレクターのテスト」 | 期待するファイル(コレクターごと)の置き場所は書かれていない | `crates/yata-provider-aws/tests/golden/<service>/<resource>.json`。更新は `YATA_UPDATE_GOLDEN=1` |

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| 組の中の並行数の実測による見直し | 8 は往復の時間の見込みと、IAM の制限が公開されていないことからの判断で、実測していない。実際のアカウントでの所要時間、スロットリングの失敗の印の数、ほかの IAM の組(再試行の割当てを共有する)の失敗を確かめ、値を見直すか。実 AWS での確認にはメンテナーの確認が要る | 最初のリリースの前の実環境での確認、またはスロットリングの報告があった時点 |
| 組の中の並行数を利用者が変える手段 | `--concurrency` を小さくしても組の中の並行数は変わらない。制限の厳しい環境のために、`--concurrency` に連動させる(例: 両者の小さい方)、オプション・環境変数を設けるなどの手段を設けるか | スロットリングが問題になった時点(CLI の変更になるため、全体設計の Issue として) |
| 呼び出しの回数を減らす代わりの方法 | `GetAccountAuthorizationDetails` はポリシーの文書をまとめて返すため、`GetPolicyVersion` の 1,000 回を超える呼び出しを数十回のページに減らせる可能性がある。ただし、既定のパラメーターでユーザー・グループ・ロールも返り、構成情報のキーと形が変わる(互換性の無い変更になるため、公開の前に決める必要がある) | スロットリング・所要時間が問題になった時点。公開した後なら新しい種別名で |
| ユーザーとポリシーのタグ | `ListUsers`・`ListPolicies` はタグを返さないため、`aws.iam.user`・`aws.iam.policy` はタグを取得しない種別として扱い、リソースの `tags` は常に `{}`(タグが無いことを表さない)、構成情報の `Tags` は通常 `null`。タグを使うルールが必要になったら、`ListUserTags`・`ListPolicyTags` をリソースごとに呼ぶか | タグを使うルールを加えるとき |
| 出力のキーの順序 | 構成情報のオブジェクトのキーの順序は `serde_json` の機能(`preserve_order`)の有無で変わる。スナップショット・レポートの出力でキーの順序をそろえるか(書き出す前に並べ替えるなど) | スナップショット・出力を実装する Issue(全体設計の未決事項として扱う) |

## 参考資料

- `GetAccountSummary`(`SummaryMap` のキー): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetAccountSummary.html>
- `GetAccountPasswordPolicy`(`NoSuchEntity`): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetAccountPasswordPolicy.html>
- `PasswordPolicy`(項目): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_PasswordPolicy.html>
- `ListUsers`(`PermissionsBoundary`・`Tags` を返さない、ページング、`PathPrefix` の既定): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_ListUsers.html>
- `User`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_User.html>
- `ListAttachedUserPolicies`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_ListAttachedUserPolicies.html>
- `ListUserPolicies`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_ListUserPolicies.html>
- `ListPolicies`(既定で全ポリシー、`Scope`・`OnlyAttached`・`PolicyUsageFilter` の既定、タグを返さない): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_ListPolicies.html>
- `Policy`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_Policy.html>
- `GetPolicyVersion`(文書は RFC 3986 で URL エンコード、`NoSuchEntity`): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetPolicyVersion.html>
- `PolicyVersion`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_PolicyVersion.html>
- IAM の Common Error Types(`AccessDeniedException`、`NotAuthorized`、`ThrottlingException` など): <https://docs.aws.amazon.com/IAM/latest/APIReference/CommonErrors.html>
- IAM のクォータ(カスタマー管理ポリシーの数、管理ポリシーの文書の大きさ): <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html>
- RFC 3986(パーセントエンコーディング): <https://www.rfc-editor.org/rfc/rfc3986>
- SDK の型(`aws-sdk-iam` の `User`、`Policy`、`PasswordPolicy`、`PolicyVersion`、`primitives`): <https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/types/index.html>、<https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/primitives/index.html>
- 日時の形式(`DateTimeFormat::DateTime` は RFC 3339): <https://docs.rs/aws-smithy-types/latest/aws_smithy_types/date_time/enum.Format.html>
- 再試行の分割(`RetryPartition`。同じ名前の分割でトークンバケットを共有): <https://docs.rs/aws-smithy-runtime/latest/aws_smithy_runtime/client/retries/struct.RetryPartition.html>
- 既定の再試行の分割の名前(`aws-sdk-iam` の `src/config.rs`): <https://github.com/awslabs/aws-sdk-rust/blob/main/sdk/iam/src/config.rs>
- トークンバケットのプロセス全体の表(`aws-smithy-runtime` の `client/retries/strategy/standard.rs`): <https://github.com/smithy-lang/smithy-rs/blob/main/rust-runtime/aws-smithy-runtime/src/client/retries/strategy/standard.rs>
- SDK の再試行の動作: <https://docs.aws.amazon.com/sdkref/latest/guide/feature-retry-behavior.html>
- `aws-smithy-mocks`(`match_requests`、`RuleMode::MatchAny` で規則を繰り返し使う): <https://docs.rs/aws-smithy-mocks/latest/aws_smithy_mocks/>
- `tokio::task::JoinSet`: <https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html>
- AWS 管理ポリシー `SecurityAudit`: <https://docs.aws.amazon.com/aws-managed-policy/latest/reference/SecurityAudit.html>
