# ADR: AWS SDK for Rust と読み取り専用ガードを使う

> This Japanese version is the authoritative source. The English version (`ADR-0001-AWS-SDK-and-Read-Only-Guard.md`) has not been written yet.

- ステータス: 承認
- 関連: [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md)

## 背景

Yata Cloud は利用者の AWS アカウントに接続して設定を読み取ります。誤ってリソースを作成・変更・削除すると、利用者の環境に重大な影響を与えます。プロジェクトの規約で、クラウドへのアクセスは Describe / List / Get 系の API に限っています。

決める必要があるのは次の 2 点です。

- AWS への接続に何を使うか
- 読み取り専用の規約を、レビューだけでなく仕組みで守るにはどうするか

## 決定

### SDK

- 公式の AWS SDK for Rust(`aws-config`、`aws-sdk-<service>`)を使う
- 認証情報は SDK の既定の認証情報プロバイダチェーンに任せ、Yata Cloud は認証情報を自分で読まない

### 読み取り専用ガード

- すべてのサービスクライアントに、SDK のインターセプターとして「読み取り専用ガード」を付ける
- ガードはリクエストを送る前に操作名とサービス名を調べ、次の操作をエラーにして、送信させない
  - `Describe`、`List`、`Get` のいずれでも始まらない操作(例外リストにあるものを除く)
  - 拒否リストにある操作。`Get` などで始まっても、秘密の値を取得するもの・認証情報を発行するもの・データの中身を読むものを載せる。拒否リストは SDK の操作の名前(サービス + 操作名)で書き、テストのために対応する IAM アクション名を併記する。初期の一覧は [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) を参照
- 判定は拒否リスト → 例外リスト → 接頭辞の順で、拒否リストを優先する
- クライアントは `yata-provider-aws` の生成用マクロ(またはヘルパー)でだけ作る。直接の生成はテストで検出する
- 例外リストと拒否リストは `yata-provider-aws/src/support/` に理由とともに書く。初期状態の例外リストは空。どちらも中央の列挙だが追加の頻度が低い共有箇所とし、変更はメンテナーの判断を経た単独の PR で行う
  - 例外リストには、[ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) で IAM の `GenerateCredentialReport` を加えた。現在の内容は [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) を参照
- 各コレクターは必要な IAM 権限を宣言し、テストでその形(`<service>:(Describe|List|Get)...`)と、拒否リストの操作(併記した IAM アクション名で比べる)を含まないことを確かめる。コードレビューでも確かめる
  - 例外リストにある操作の IAM アクション名も、この形の検査で許す([ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md))

### 認証情報の解決で SDK が内部で呼ぶ操作

- 認証情報プロバイダチェーンは、利用者の設定に応じて SDK の内部で `sts:AssumeRole`、`sts:AssumeRoleWithWebIdentity`、`sso:GetRoleCredentials` などを呼ぶ。これらは `aws-config` の内部のクライアントから呼ばれるため、ガードの対象外になる
- これらは一時的な認証情報を得る操作で、利用者のリソースを作成・変更・削除しない。利用者が選んだ認証方法を実行するものであるため、読み取り専用の規約に反しないものとして許容する
- Yata Cloud のコードからこれらを直接呼ぶことはしない

## 検討した選択肢

### SDK

| 選択肢 | 判断 |
| --- | --- |
| AWS SDK for Rust(採用) | AWS 公式で保守されている。認証情報プロバイダチェーン、再試行、ページネーター、テスト用のモック(`aws-smithy-mocks`)がそろう。Apache-2.0 |
| `rusoto` | 保守が終了している |
| 署名付きの HTTP を自前で実装 | 署名、認証情報、再試行、ページングをすべて作る必要があり、誤りの危険が大きい |
| `aws` CLI を子プロセスで呼ぶ | 外部のツールに依存し、単一バイナリで配布できない |

### 読み取り専用の守り方

| 選択肢 | 判断 |
| --- | --- |
| 実行時のガード + テスト + レビュー(採用) | 新しいコレクターでも、ガードを経由しない限りクライアントを作れず、誤って書き込み系の操作を呼んでも送信前に止まる |
| レビューだけ | 見落としの危険がある |
| 利用者に読み取り専用の IAM 権限を付けてもらうだけ | 最終的な防御として有効で、案内もする。ただし利用者が強い権限で実行した場合に守れない |
| サービスのクレートの機能で書き込み系の操作を除く | SDK のクレートは操作ごとに機能を分けていない |

## 結果

- 書き込み系の操作の呼び出しは、送信前にエラーになる。エラーは収集の失敗として記録され、利用者の環境は変更されない
- 操作名が Describe / List / Get で始まらない読み取りの操作(例: IAM の認証情報レポートの生成 `GenerateCredentialReport`)は、例外リストに載せない限り使えない。`GenerateCredentialReport` は [ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) で例外リストに加えた
- ガードはインターセプターから操作名(SDK が設定に入れる操作のメタデータ)を読む前提で設計している。公開 API でこれが実現できない場合は、クライアントの生成をラップする型で呼べる操作を限定する方法に切り替える(AWS プロバイダを実装する Issue で確認する)
- AWS SDK for Rust の最低対応 Rust バージョンに、Yata Cloud の最低対応バージョンを合わせる必要がある

## 出典

- AWS SDK for Rust 開発者ガイド: <https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html>
- 認証情報プロバイダチェーン: <https://docs.aws.amazon.com/sdk-for-rust/latest/dg/credproviders.html>
- `aws-smithy-mocks`: <https://docs.aws.amazon.com/sdk-for-rust/latest/dg/testing-smithy-mocks.html>
- 操作のメタデータ(`Metadata`): <https://docs.rs/aws-smithy-runtime-api/latest/aws_smithy_runtime_api/client/orchestrator/struct.Metadata.html>
- インターセプター(`Intercept` トレイト): <https://docs.rs/aws-smithy-runtime-api/latest/aws_smithy_runtime_api/client/interceptors/trait.Intercept.html>
