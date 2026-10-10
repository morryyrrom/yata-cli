# ADR: 読み取り専用ガードで GenerateCredentialReport を許可する

> This Japanese version is the authoritative source. The English version (`ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.md`) has not been written yet.

- ステータス: 承認
- 関連: [最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md)、[AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[ADR: AWS SDK for Rust と読み取り専用ガード](ADR-0001-AWS-SDK-and-Read-Only-Guard.ja.md)

## 背景

- 読み取り専用ガードは、`Describe`、`List`、`Get` のいずれでも始まらない操作を、例外リストにあるものを除いて拒否する。初期状態の例外リストは空
- CIS AWS Foundations Benchmark の IAM のコントロールのうち、コンソールのパスワードを持つユーザーの MFA(2.10)、使われていない認証情報(2.11)、アクセスキーのローテーション(2.12。番号は v7.0.0 のもの。[最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) を参照)は、CIS の監査の手順で IAM の認証情報レポートを使う
- 認証情報レポートを取る `GetCredentialReport` は、先に `GenerateCredentialReport` でレポートを作っておく必要がある。レポートが無い・古い(4 時間を超えた)場合、`GetCredentialReport` だけでは取れない
- `GenerateCredentialReport` は名前の規則(Describe / List / Get)から外れるため、ガードの例外にするかを決める必要があった([AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の未決事項)

## 決定

- 読み取り専用ガードの例外リストに、IAM の `GenerateCredentialReport`(IAM アクション名 `iam:GenerateCredentialReport`)を加える
- コレクターの `required_permissions` の形の検査は、`<service>:(Describe|List|Get)...` の形に加え、例外リストにある操作の IAM アクション名を許す
- 認証情報レポートを使う IAM のコントロール(2.10〜2.12)を、最初のルール群の範囲に含める
- 例外リストに加えるのはこの操作だけとする。ほかの `Generate*` の操作(例: `GenerateServiceLastAccessedDetails`)は、必要になった時点で同じように判断する

### 許可する理由

- `GenerateCredentialReport` は、アカウントに 1 つだけある認証情報レポートを作り直す操作で、IAM のユーザー、認証情報、ポリシー、アカウントの設定を作成・変更・削除しない
- IAM は、4 時間以内に作ったレポートがあれば新しく作らずにそれを使う。呼び出しが変えうるのは、保存されているレポートの作成の時点だけで、IAM のリソースや設定には影響しない
- AWS 管理ポリシー `SecurityAudit`(設定の監査のための読み取りのポリシー)が、この権限を含んでいる。利用者が監査のための権限で `yata` を実行すれば、追加の権限は要らない
- CIS の監査の手順と同じデータで判定でき、判定の根拠を利用者に説明しやすい

### 受け入れる影響

- アカウントに保存されているレポートが作り直されることがある(4 時間を超えて古い場合)。同じアカウントでレポートを見ている人は、新しいレポートを見ることになる。レポートの内容は IAM の現在の状態から作られるため、情報が失われることはない
- 「Describe / List / Get だけを呼ぶ」という説明に例外ができる。利用者向けの文書(利用者ガイド)で、この操作を使うことと影響を説明する。`yata permissions` の出力でどう示すか(標準エラー出力の注意、ポリシーの `Sid` で分けるなど)は、`yata permissions` を実装する Issue で決める([最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の未決事項)

## 検討した選択肢

| 選択肢 | 判断 |
| --- | --- |
| `GenerateCredentialReport` を例外として許可する(採用) | 上の「許可する理由」のとおり。呼び出しは 2〜数回で、ユーザーの数によらない |
| ユーザーごとの API で代える(`GetLoginProfile`、`ListMFADevices`、`ListAccessKeys`、`GetAccessKeyLastUsed`。パスワードの最終使用は `ListUsers` の `PasswordLastUsed`) | 例外は要らない。ただし、ユーザーごとに 4 つ前後の呼び出しが要り、ユーザーの多いアカウントで呼び出しの回数とスロットリングの危険が大きい。CIS の監査の手順とも違うデータになる |
| `GetCredentialReport` だけを呼び、レポートが無い・古い場合は取得の失敗とする | 例外は要らない。ただし、利用者が事前にレポートを作っておかないと、判定がほぼ常に `incomplete` になる |
| 認証情報レポートを使うコントロールを範囲から除く | 影響の大きい IAM のコントロール(MFA、使われていない認証情報、キーのローテーション)を扱えない |

## 結果

- [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md) の未決事項「Describe / List / Get 以外の読み取り操作」を、この ADR で解決したものとして更新する。例外リストの初期の内容と、`required_permissions` のテストの条件も合わせて直す(全体設計の更新は別の PR で行う)
- [ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md) の「中央の列挙を許すもの」の、例外リストの「原則として空」の記述を直す
- `yata-provider-aws/src/support/` の例外リストへの追加は、理由とともに単独の PR で行う
- 認証情報レポートの生成・取得の失敗は、ユーザーごとの構成情報の失敗の印になり、ルールの状態は `incomplete` になる([最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の「認証情報レポート」)

## 出典

- IAM の認証情報レポート(生成の間隔、必要な権限): <https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_getting-report.html>
- `GenerateCredentialReport`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GenerateCredentialReport.html>
- `GetCredentialReport`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetCredentialReport.html>
- AWS 管理ポリシー `SecurityAudit`: <https://docs.aws.amazon.com/aws-managed-policy/latest/reference/SecurityAudit.html>
