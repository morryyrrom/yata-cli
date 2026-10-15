# IAM の認証情報レポートの収集

> This Japanese version is the authoritative source. The English version (`Feature-0013-IAM-Credential-Report.md`) has not been written yet.

`aws.iam.user` のコレクターに、IAM の認証情報レポート(`GetCredentialReport`)のキーを加えます。レポートの生成(`GenerateCredentialReport`)を待つ手順と定数、CSV の解析、行とユーザーの対応づけ、失敗の印への対応を決めます。

認証情報レポートを使う範囲と収集の流れの大枠は [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「認証情報レポート」、生成を読み取り専用ガードの例外にする判断は [ADR: 読み取り専用ガードで GenerateCredentialReport を許可する](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md) にあります。`aws.iam.user` のほかのキー、組の中の並行(`fan_out`)、値の変換(`convert`)、失敗の印(`to_failure_mark()`)は [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) に従います。構成情報の形の規則は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、エラーの扱いは [エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md)、テストは [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) に従います。

## 目的と背景

- CIS 2.10(コンソールのパスワードを持つユーザーの MFA)、2.11(45 日以上使われていない認証情報)、2.12(90 日を超えたアクセスキー)は、IAM の認証情報レポートの列(`password_enabled`、`mfa_active`、`password_last_used`、`access_key_N_*` など)で判定する
- ルール(ポリシーのリポジトリ)のフィクスチャを書き始めるには、`aws.iam.user` の `GetCredentialReport` のキーの形(値の形、失敗の印、行の無いユーザーの扱い)が先に決まっている必要がある
- 次の未決事項を、この設計で決める
  - [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「レポートに行の無いユーザー」と「認証情報レポートの待ち方」
  - [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「認証情報レポートを加えるときの形」で後に回した、レポートの取得をどこで行うか(ユーザーごとの取得と並行に動かすか)

## スコープ

- `collectors/iam/user.rs` への `GetCredentialReport` のキーの追加と、`required_permissions` への 2 つの権限の追加
- `GenerateCredentialReport` を `COMPLETE` まで呼び直す手順と、間隔・上限の定数
- `GetCredentialReport` の出力(`GeneratedTime`、`ReportFormat`、`Content`)の変換と、`Content`(CSV)の解析
- 行とユーザーの対応づけ(ARN の一致)と、行の無いユーザー・対応するユーザーの無い行の扱い
- 生成・取得・解析の失敗の、失敗の印への対応
- CSV の解析の関数(`support/csv.rs`)
- 上記のテスト(`aws-smithy-mocks`。ダミーの CSV だけを使い、実 AWS には接続しない)

## スコープ外

- ルール本体(2.10〜2.12。ポリシーのリポジトリ)
- ルートユーザーの行(`<root_account>`)の入れ先。2.7 を範囲に加えるときに決める([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の未決事項「認証情報レポートのルートユーザーの行」)
- 読み取り専用ガードの例外リストの変更。IAM / `GenerateCredentialReport` は [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) で例外リストに入っている(`support/guard_lists.rs` の `EXCEPTION_LIST`)。`required_permissions` の形の検査も、例外リストの IAM アクション名を既に許す
- 待ち方の定数を利用者が変える手段(CLI のオプション・環境変数)。定数で持つ(未決事項「待ち方の定数を利用者が変える手段」)
- ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock` の変更(下の「依存」)
- `docs/architecture/` の変更(下の「既存の設計との差異」)
- ユーザーごとの認証情報の API(`ListAccessKeys`、`ListMFADevices` など)。レポートで足りない情報(3 つ目以降のアクセスキー、サービス固有の認証情報)の収集は、それを使うルールを加えるときに検討する

## 振る舞い

この設計は CLI のコマンド・オプションを変えません。

- `yata scan` / `yata collect` で `aws.iam.user` を収集すると、各ユーザーの構成情報に `GetCredentialReport` のキーが加わる
- `yata permissions` の出力に `iam:GenerateCredentialReport` と `iam:GetCredentialReport` が加わる(`aws.iam.user` を使うルールを選んだ場合)。例外の操作をどう示すかは [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の未決事項「`yata permissions` での例外の操作の示し方」のまま
- エラー時
  - 生成・取得・解析の失敗は、すべてのユーザーの `GetCredentialReport` のキーの失敗の印になる。`ListUsers` が成功していれば組の収集は成功のままで、`aws.iam.user` を使うルール(2.10〜2.13)は `incomplete` になる。スキャンは不完全で、終了コードは [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) に従う(`--fail-on` 以上の検出が無ければ 4)
  - レポートに行の無いユーザーは、そのユーザーの `GetCredentialReport` のキーだけが失敗の印になる
  - `ListUsers` の失敗は、これまでどおり組全体の失敗(`CollectionError`)で、レポートの操作は呼ばない

## 構成情報の形

`aws.iam.user` の構成情報に、キー `GetCredentialReport` を加えます。ほかのキー(`ListUsers`、`ListAttachedUserPolicies`、`ListUserPolicies`)は [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) のとおりで、変えません。

```json
{
  "ListUsers": { "Users": [ { "Arn": "arn:aws:iam::123456789012:user/example-user", "UserName": "example-user", "...": "..." } ] },
  "ListAttachedUserPolicies": { "AttachedPolicies": [] },
  "ListUserPolicies": { "PolicyNames": [] },
  "GetCredentialReport": {
    "GeneratedTime": "2026-01-01T00:00:00Z",
    "ReportFormat": "text/csv",
    "Content": {
      "user": "example-user",
      "arn": "arn:aws:iam::123456789012:user/example-user",
      "user_creation_time": "2026-01-01T00:00:00+00:00",
      "password_enabled": "TRUE",
      "password_last_used": "no_information",
      "password_last_changed": "2026-01-01T00:00:00+00:00",
      "password_next_rotation": "N/A",
      "mfa_active": "FALSE",
      "access_key_1_active": "TRUE",
      "access_key_1_last_rotated": "2026-01-01T00:00:00+00:00",
      "access_key_1_last_used_date": "N/A",
      "access_key_1_last_used_region": "N/A",
      "access_key_1_last_used_service": "N/A",
      "access_key_2_active": "FALSE",
      "access_key_2_last_rotated": "N/A",
      "access_key_2_last_used_date": "N/A",
      "access_key_2_last_used_region": "N/A",
      "access_key_2_last_used_service": "N/A",
      "cert_1_active": "FALSE",
      "cert_1_last_rotated": "N/A",
      "cert_2_active": "FALSE",
      "cert_2_last_rotated": "N/A",
      "additional_credentials_info": "N/A"
    }
  }
}
```

(値はすべてダミーです。`ListUsers` の値は、ユーザーの要素の一部の項目だけを示しています。)

- 値は `GetCredentialReport` の出力の 3 項目をすべて持つオブジェクト。`Content` だけを、そのユーザーの行にする([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「格納の形」)
- **`GeneratedTime`**: RFC 3339(UTC、`Z`)の文字列。`support/convert.rs` の `datetime_to_rfc3339` で整形する。API が返さなければ `null`。ルールはこれでレポートの古さを判断できる(IAM は 4 時間以内に作ったレポートを使い回すため)
- **`ReportFormat`**: 列挙の値の文字列(`as_str()`)。API の取りうる値は `text/csv` だけ
- **`Content`**: そのユーザーの行を、ヘッダー行の列名をキー、その行の値を値にしたオブジェクト
  - キーはヘッダー行の列名をそのまま使う(`password_enabled` など。snake_case)。AWS が列を加えた場合も、その列名のまま入る
  - 値は CSV の文字列のまま入れる(`"TRUE"`、`"FALSE"`、`"N/A"`、`"no_information"`、`"not_supported"`、日時の文字列などを変換しない)。日時の列も、CSV の表記(ISO 8601)のままで、`GeneratedTime` のような RFC 3339 の `Z` の形には直さない。値を真偽値・`null`・別の日時の表記に変えると、意味の区別が失われるか、値が API の返したものと変わるため
  - 引用符で囲まれた値は、囲みの引用符を外し、`""` を `"` に戻した文字列にする(CSV の形式の解除で、値の意味は変えない。下の「CSV の解析」)
  - 列の並び順には意味を持たせない(JSON のオブジェクトにするため)
- 秘密の値は含まれない(認証情報レポートにパスワードやアクセスキーのシークレットは無い)。含まれる値(ユーザー名、ARN、最終使用のリージョン・サービスなど)は伏せずにそのまま入れる
- キーの追加は項目の追加なので、構成情報の互換性を壊さない([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「構成情報の互換性」)。公開した後に `Content` の形(構造化した行)を base64 の文字列などに変えることは、型の変更なのでしない

### `GenerateCredentialReport` の出力を入れない理由

`GenerateCredentialReport` の出力(`Description`、`State`)は、構成情報のキーにしません。[最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「コレクターの方針」のとおり、「すべて入れる」の対象はリソースの情報を取る操作の出力で、別の操作の前提として呼ぶ手順の操作の出力は対象外です(「すべて入れる」の例外ではありません)。この操作がそれに当たる理由は次のとおりです。

- この操作は、`GetCredentialReport` でレポートを取るための前提の手順で、出力はその手順の状態(生成を始めた、生成中、完了)を表す。どのユーザーの情報でもなく、アカウントのレポートの内容でもない
- レポートを取れたときの最後の `State` は常に `COMPLETE` で、入れても情報が増えない。レポートの作成の時点は `GetCredentialReport` の `GeneratedTime` に入る
- 生成に失敗した場合は、`GetCredentialReport` のキーの失敗の印のメッセージに `GenerateCredentialReport` の名前とコードが入る(下の「失敗の印」)

## 処理の順

図: [ユーザーの収集の並行](../diagrams/Diagram-0013-User-Collector-Concurrency.md)

1. `ListUsers` の全ページを取る(失敗したら組全体の失敗。[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) のとおり)
2. ユーザーが 0 人なら、レポートの操作を呼ばず、空の配列を返す(入れ先のリソースが無いため。例外リストの操作を必要なときだけ呼ぶ)
3. 次の 2 つを、`tokio::join!` で並行に進める
   - ユーザーごとの `ListAttachedUserPolicies` と `ListUserPolicies`(`fan_out`。[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) のまま)
   - 認証情報レポートの取得: `GenerateCredentialReport` を `COMPLETE` まで呼び、`GetCredentialReport` を呼び、CSV を解析する(下の「レポートの生成を待つ手順」「CSV の解析」)。結果は「解析したレポート」か「すべてのユーザーに置く失敗の印」
4. ユーザーごとに `GetCredentialReport` の値を決める(下の「行とユーザーの対応づけ」)
5. ユーザーごとに `Resource` を組み立てる

- `ListUsers` を先に呼び、レポートの生成をその後に始める。新しく作られるレポートには、生成を始めた時点のユーザーがすべて含まれるはずなので(ドキュメントに明記は無い。未確認)、`ListUsers` が返したユーザーに行が無いのは、主に 4 時間以内に作られたレポートが使い回された場合に限られる。逆の順にすると、2 つの呼び出しの間に作られたユーザーの行が必ず欠ける
- レポートの取得を `fan_out` の仕事にしない。レポートの取得はアカウントで 1 回だけで、ユーザーごとの仕事ではないため。`tokio::join!` は同じタスクの中で 2 つの future を進めるだけで、新しいタスクを作らない(`tokio` の `macros` の機能はワークスペースで有効)
- レポートの取得が待っている間も、ユーザーごとの呼び出しは進む。このため、待つ時間の多くは `fan_out` の時間と重なり、組の所要時間は「ユーザーごとの呼び出しの時間」と「レポートの取得の時間」の長い方になる
- 同じ組の中で IAM への同時の呼び出しは、最大で `fan_out` の 8 とレポートの 1 の 9 になる。IAM 全体の同時の呼び出しの上限([IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「`--concurrency` との関係」の 18)は 19 になる。レポートの呼び出しは数回から十数回で、再試行の割当て(`iam-us-east-1`)への影響は小さい
- 組のタスクが破棄された場合(Ctrl-C による中断など)は、待ちの future も破棄され、以後の呼び出しは行わない。生成は IAM が続けるが、IAM のリソース・設定は変わらない([ADR-0005](../adr/ADR-0005-Allow-GenerateCredentialReport-in-Read-Only-Guard.ja.md))

## レポートの生成を待つ手順

図: [生成を待つ流れ](../diagrams/Diagram-0013-Credential-Report-Polling.md)

### 手順

1. `GenerateCredentialReport` をパラメーターなしで呼ぶ(この操作にパラメーターは無い)
2. 応答の `State` で分ける

| `State` | 扱い |
| --- | --- |
| `COMPLETE` | 待ちを終え、`GetCredentialReport` に進む |
| `STARTED`、`INPROGRESS` | 下の間隔だけ待ってから、手順 1 に戻る。待ちの合計が上限を超える場合は、待たずに時間切れにする |
| 返らない(`None`) | 時間切れを待たずに失敗(`invalid_response`、`MissingState`) |
| そのほかの値(SDK が知らない値) | 時間切れを待たずに失敗(`invalid_response`、`UnknownState`)。値は `debug` のログに出す。AWS が失敗を表す値を加えた場合に、上限まで呼び続けないため |

3. `GenerateCredentialReport` の呼び出しが失敗したら、待ちを終えて失敗にする(下の「失敗の印」)。SDK の再試行(standard、最大 3 回の試行)は、ほかの操作と同じく SDK に任せ、コレクターは呼び直さない

- IAM は、4 時間以内に作ったレポートがあればそれを使い、最初の呼び出しが `COMPLETE` を返す([IAM ユーザーガイド](https://docs.aws.amazon.com/IAM/latest/UserGuide/credential-reports.html))。このときは待たない
- 生成中に `GenerateCredentialReport` を呼び直しても、新しい生成を始めずに状態を返す前提にしている。API リファレンスに明記は無い(未確認)。呼び直しがレポートを作り直したとしても、IAM のリソース・設定は変わらない

### 間隔と上限

| 定数 | 値 | 意味 |
| --- | --- | --- |
| `REPORT_POLL_INITIAL_INTERVAL` | 1 秒 | 最初の待ちの長さ |
| `REPORT_POLL_MAX_INTERVAL` | 8 秒 | 待ちの長さの上限。待つたびに前の 2 倍にし、この値で止める |
| `REPORT_POLL_MAX_TOTAL_WAIT` | 120 秒 | 待ちの合計の上限。次の待ちで合計がこれを超える場合は、待たずに時間切れにする |

- 待ちの列は 1、2、4、8、8、… 秒になる。合計が 120 秒を超えない範囲では、1、2、4 と 8 × 14(17 回、合計 119 秒)の待ちで、`GenerateCredentialReport` の呼び出しは最大 18 回
- 上限は待ちの合計で数え、呼び出しそのものの時間は含めない(テストで実時間に頼らずに確かめられるようにするため)。呼び出しの時間は SDK の既定のタイムアウトと再試行に従う(操作ごとのタイムアウトは [AWS プロバイダの基盤](Feature-0007-AWS-Provider-Foundation.ja.md) の未決事項)。このため、時間切れまでの実際の時間は 120 秒に呼び出しの時間を足したものになる
- 定数は `collectors/iam/user.rs` に置く。CLI のオプション・環境変数では変えない

### 値の根拠

AWS は、生成に掛かる時間も、呼び直しの間隔の推奨も公開していません(API リファレンス・ユーザーガイドに記述が無い。未確認)。AWS の CLI と SDK にも、認証情報レポートを待つ仕組み(waiter)はありません。ほかの OSS の待ち方は次のとおりです(参照した版は、下の「出典」のコミットに固定したリンクのもの)。

| 実装 | 待ち方 |
| --- | --- |
| botocore(AWS CLI・boto3 の定義) | IAM の waiter は `UserExists`・`RoleExists`・`PolicyExists`(1 秒間隔、最大 20 回)と `InstanceProfileExists`(1 秒間隔、最大 40 回)だけで、認証情報レポートの waiter は無い |
| Prowler | `State` が `COMPLETE` になるまで `generate_credential_report` を待たずに呼び続ける。回数の上限は無い。`LimitExceededException` のコードは警告のログにして、レポートを空として扱う |
| ScoutSuite | 最大 3 回、0.1 秒の間隔で `generate_credential_report` を呼ぶ(待ちの合計 0.3 秒未満) |
| Cloud Custodian | レポートが無い・古い場合に `generate_credential_report` を 1 回呼び、10 秒(既定。設定で変えられる)待ってから `get_credential_report` を呼ぶ。`ReportInProgress` の場合も 1 回だけ待って取り直す |
| Steampipe(AWS のプラグイン) | 生成しない。`GetCredentialReport` だけを呼び、`ReportNotPresent` の場合は、利用者に `aws iam generate-credential-report` の実行を求めるエラーにする |

この比較から、次のように決めました。

- **最初の間隔 1 秒**: 使い回しのレポートが無い場合でも、小さなアカウントでは生成がすぐ終わる見込みで(ScoutSuite は 0.3 秒未満の待ちで打ち切る実装である)、最初の待ちを短くして早く終える。botocore の IAM の waiter の間隔(1 秒)とも同じ
- **倍にして 8 秒で止める**: 生成が長いアカウント(ユーザーが数千人)で呼び出しの回数を抑える。Prowler のように待たずに呼び続けると、IAM の再試行の割当てを共有するほかの IAM の組([IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「再試行とスロットリング」)にも影響しうる。上限 120 秒でも呼び出しは最大 18 回で、`fan_out` の呼び出しの数に比べて小さい
- **合計 120 秒**: Cloud Custodian の既定(10 秒の 1 回)より長く取り、ユーザーの多いアカウントでの生成を待てるようにする。一方で、上限の無い待ちでスキャンが終わらなくなることは避ける。`aws.iam.policy` の組の見込み(20〜60 秒。[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「件数の見込みと所要時間」)と同じ程度で、上の「処理の順」のとおりユーザーごとの呼び出しと重なるため、スキャン全体の時間を大きく延ばしにくい
- 実際のアカウントでの生成の時間は確かめていない。実測で見直す(未決事項「待ち方の定数の実測による見直し」)

### 待ちの実装

- 待ちには、SDK のクライアントの設定にある非同期の待ちの実装(`aws_sdk_iam::Client::config().sleep_impl()`。型は `aws_sdk_iam::config::SharedAsyncSleep`)を使う。`aws_config` で読み込んだ設定では、tokio の待ちの実装が入っている
  - `tokio::time::sleep` を直接使うには、`crates/yata-provider-aws/Cargo.toml` の `[dependencies]` の `tokio` に `time` の機能を加える必要がある(今は `[dev-dependencies]` にだけある)。SDK の待ちの実装を使えば、`Cargo.toml` を変えずに済む(下の「依存」)
  - 待ちの実装が無い場合(`None`)は、待つ必要が生じた時点で失敗(`service`、`NoSleepImplementation`)にする。`aws_config` で読み込んだ設定では起こらない。yata の内部の不備を表す種類は無いため、[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の `fan_out` のタスクのパニックの扱いにそろえて `service` にする
- 生成を待つ関数は、待ちの実装(`SharedAsyncSleep`)を引数に取る。`collect` はクライアントの設定から取ったものを渡し、テストは待った長さを記録してすぐに戻る偽の実装(`aws_sdk_iam::config::AsyncSleep` を実装し、`Sleep::new(async {})` を返す)を渡す。テストは実時間で待たず、待ちの列(1、2、4、8、… 秒)をそのまま確かめられる

## `GetCredentialReport` の出力の確かめ

`GenerateCredentialReport` が `COMPLETE` を返したら、`GetCredentialReport` をパラメーターなしで 1 回呼びます。呼び出しが成功した場合、次の順に確かめます。どれかに当たれば、すべてのユーザーに失敗の印(`invalid_response`)を置きます。

| 場合 | 理由のコード |
| --- | --- |
| `ReportFormat` が返らない | `MissingReportFormat` |
| `ReportFormat` が `text/csv` でない(SDK が知らない値) | `UnsupportedReportFormat` |
| `Content` が返らない | `MissingContent` |
| `GeneratedTime` を整形できない(年が範囲外など) | `InvalidDateTime` |
| `Content` が UTF-8 でない | `InvalidUtf8` |
| CSV として解析できない(下の「CSV の解析」) | 下の表のコード |

- `ReportFormat` が `text/csv` 以外・不明の場合に CSV として解析しないのは、`Content` の形式を確かめられないため。形式の違うものを CSV として解析すると、値の意味を変えたものを入れることになる
- `GeneratedTime` が返らない場合は失敗にせず、`null` を入れる(値が無いことをそのまま表す。[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「値の変換の規則」)
- `Content` は API では base64 の Blob で、SDK がデコードしたバイト列を返す。コレクターは base64 をデコードしない。SDK がデコードしていることを、HTTP の応答のモック(base64 の文字列を含む XML)で確かめる(下の「テスト方針」)
- `COMPLETE` の後に `GetCredentialReport` が `ReportInProgress`・`ReportNotPresent`・`ReportExpired` を返した場合(別の利用者が同時にレポートを作り直したなど)も、取り直さずに失敗の印にする。起こりにくく、取り直しの規則を足すより、失敗として見えるようにする方を選ぶ

## CSV の解析

### 解析の規則

`Content` を RFC 4180 の CSV として解析します。認証情報レポートが引用符を使うかは、ユーザーガイドに記述がありません(未確認)。ユーザー名は `,` を含められ(`CreateUser` の `UserName` のパターン `[\w+=,.@-]+`)、パスは `!`(`!`)から `~`(`~`)までの文字(`"` と `,` を含む)を含められるため、引用符で囲まれた値を正しく扱う必要があります。

- 区切りは `,`。行の区切りは LF と CRLF のどちらも受け付ける
- 値は、引用符(`"`)で囲まれていてもよい。囲まれた値の中では `,`・改行をそのまま値の一部とし、`""` を 1 つの `"` に戻す。囲みの引用符は値に含めない
- 囲まれていない値は、次の `,` か行の区切りまでをそのまま値にする(前後の空白も削らない)。値の途中の `"` も、そのまま値の一部にする(RFC 4180 は囲まない値に `"` を含めないが、パスは `"` を含められ、AWS が引用符で囲むかを確かめられないため、レポート全体を解析の失敗にしない)。値が `"` で始まる場合だけ、囲まれた値として扱う
- 最初の行をヘッダー行とし、各値を列名にする
- 長さ 0 の行(連続する行の区切り、末尾の行の区切りの後)は飛ばす。値の無い行で、データを落とすことにはならないため
- 先頭の UTF-8 の BOM(`EF BB BF`)は、あれば外す(テキストの形式の印で、値ではないため)
- 値は文字列のまま返し、型の変換をしない

### 解析できない場合

次の場合は、レポート全体を解析できないものとし、すべてのユーザーに失敗の印(`invalid_response`)を置きます。一部の行だけを入れることはしません(解析の規則が実際の形式と合っていない可能性が高く、ほかの行の対応も信用できないため)。

| 場合 | 理由のコード |
| --- | --- |
| 引用符が閉じていない。閉じた引用符の直後が `,`・行の区切り・終わりのどれでもない | `InvalidCsv` |
| ヘッダー行が無い(`Content` が空、または長さ 0 の行だけ) | `MissingHeader` |
| ヘッダー行に同じ列名が 2 つ以上ある | `DuplicateColumn` |
| ヘッダー行に `arn` の列が無い | `MissingArnColumn` |
| 値の数がヘッダー行の列の数と違う行がある | `FieldCountMismatch` |

- `DuplicateColumn` を失敗にするのは、JSON のオブジェクトにすると片方の値が消えるため
- ヘッダー行だけでデータの行が無いレポートは、解析の失敗にしない(すべてのユーザーが下の「行の無いユーザー」になる)

### 解析の関数の置き場所

- `src/support/csv.rs` に、CSV のテキストをヘッダー行とデータの行(どちらも文字列の配列)に分ける関数を置く。AWS のサービスに依存しない汎用の関数とし、エラーは上の理由のコードを表す小さな enum で返す(`DuplicateColumn`・`MissingArnColumn` の検査はコレクターで行う)
- `collectors/iam/` には、コレクターではない `.rs` を置けず、`user/` のような入れ子のディレクトリも置けない(build.rs の検出の規則)。CSV の解析は `user.rs` の責務(ユーザーの収集)と別で、関数とテストの量も多いため、`support/` に分ける(1 つのコレクターだけが使う処理を `support/` に置く基準は、下の「既存の設計との差異」の IAM のコレクターの「構成」の行)
- 標準ライブラリだけで書き、CSV のクレートを足さない(下の「依存」)

## 行とユーザーの対応づけ

図: [行とユーザーの対応づけ](../diagrams/Diagram-0013-Credential-Report-Row-Mapping.md)

- 解析したデータの行の `arn` 列の値と、`ListUsers` の `Arn`(= リソースの `id`)が完全に一致する(大文字・小文字を区別する)行を、そのユーザーの行とする。`user` 列(ユーザー名)では対応づけない(パスの違う同名のユーザーは無いが、ARN の方が `id` と同じで確かなため)
- 対応づけは、`arn` から行への `HashMap` を作って行う

| 場合 | そのユーザーの `GetCredentialReport` |
| --- | --- |
| 一致する行が 1 つ | 値(`{"GeneratedTime", "ReportFormat", "Content"}`) |
| 一致する行が無い | 失敗の印(`invalid_response`、`MissingUserRow`) |
| 一致する行が 2 つ以上 | 失敗の印(`invalid_response`、`DuplicateArn`)。どちらの行かを選ばない |

### 行の無いユーザー

- 主な原因: IAM が 4 時間以内に作ったレポートを使い回し、そのレポートの後に作られたユーザーには行が無い
- 失敗の印にする(`null` にしない)。`null` にすると、2.10〜2.12 のルールが「認証情報が無い」と読んで `pass` にしうるため。失敗の印なら、そのユーザーを使うルールは `incomplete` になり、黙って `pass` にならない
- 種類は `invalid_response` とする。応答(レポート)が、そのユーザーの構成情報にするのに要る行を含まない場合として、[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「値を変換できない場合」と同じ扱いにする。エラーの種類を新しく足すことは、全体設計(`CollectionErrorKind`)の変更になるため、この設計ではしない(未決事項「行の無いユーザーのエラーの種類」)
- 利用者が 4 時間を待たずに解消する方法は無い(IAM が使い回しを決めるため)。メッセージとログで原因を示す(下の「失敗の印」「ログ」)

### 対応するユーザーの無い行

- どのユーザーにも一致しない行は、どのリソースにも入れない
  - ルートユーザーの行(`arn` が `arn:aws:iam::<account_id>:root`。`user` 列は `<root_account>`): [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「コレクターの方針」の例外のとおり、入れ先が決まるまで入れない。ルートユーザーの ARN の形式は [IAM の識別子](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html) のとおりで、ユーザーの ARN(`:user/` を含む)とは一致しないため、対応づけで特別な処理は要らない
  - そのほかの行: `ListUsers` の後、レポートを取るまでの間に削除されたユーザーの行など。対応するリソースが無いため入れられない。件数を `debug` のログに出す
- `user` 列の値 `<root_account>` は、ユーザーガイドの列の説明には書かれていない(未確認)。このため、ルートユーザーの行の見分け(ログの件数とテスト)は `arn` の末尾が `:root` であることで行う

## 失敗の印

失敗の印の形は [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「操作ごとの取得の失敗」のとおり、`GetCredentialReport` のキーの値を `{"yata_error": {"kind": ..., "message": ...}}` にします。

- SDK のエラーは `failure_mark(<操作名>, &err)`(種類は基盤の判定の表で決まる)、SDK のエラーでないものは `CollectError::new(kind, message).to_failure_mark()` で作る([IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「構成」)。`support/errors.rs` は変更しない
- メッセージは `"<種類の説明> when calling <操作名> (<コード>)"` の形。`<操作名>` は失敗した操作(`GenerateCredentialReport` か `GetCredentialReport`)。SDK のエラーでないものは、SDK のエラーのコードの代わりに yata の決めた理由のコードを入れる。ARN などのリソースの識別子はメッセージに入れず、`debug` のログに出す
- 生成・取得・解析の失敗は、同じ失敗の印(同じ値)をすべてのユーザーに置く

| 場合 | 置く先 | 種類 | メッセージの例 |
| --- | --- | --- | --- |
| `GenerateCredentialReport` の SDK のエラー | すべてのユーザー | 判定の表のとおり(`AccessDenied` → `access_denied`、`Throttling` → `throttled`、`LimitExceeded`・`ServiceFailure` → `service` など) | `"access denied when calling GenerateCredentialReport (AccessDenied)"` |
| `State` が返らない・知らない値 | すべてのユーザー | `invalid_response` | `"invalid response when calling GenerateCredentialReport (MissingState)"`(返らない場合)、`"invalid response when calling GenerateCredentialReport (UnknownState)"`(知らない値の場合) |
| 待ちの合計が上限に達した | すべてのユーザー | `timeout` | `"timeout when calling GenerateCredentialReport (PollingLimitReached)"` |
| 待ちの実装が無い | すべてのユーザー | `service` | `"service error when calling GenerateCredentialReport (NoSleepImplementation)"` |
| `GetCredentialReport` の SDK のエラー | すべてのユーザー | 判定の表のとおり(`ReportNotPresent`・`ReportExpired`・`ReportInProgress`・`ServiceFailure` → `service`、`AccessDenied` → `access_denied` など) | `"service error when calling GetCredentialReport (ReportNotPresent)"` |
| 出力の確かめ・CSV の解析の失敗 | すべてのユーザー | `invalid_response` | `"invalid response when calling GetCredentialReport (FieldCountMismatch)"` |
| 行が無い | そのユーザー | `invalid_response` | `"invalid response when calling GetCredentialReport (MissingUserRow)"` |
| 一致する行が 2 つ以上 | そのユーザー | `invalid_response` | `"invalid response when calling GetCredentialReport (DuplicateArn)"` |

- `GenerateCredentialReport` の `LimitExceeded`(HTTP 409)は、API リファレンスでは「アカウントの制限を超えるリソースを作ろうとした」とだけ書かれ、どういう場合に返るかは未確認。基盤の判定の表では `service` になる(`THROTTLING_CODES` にあるのは `LimitExceededException` で、`LimitExceeded` は無い)。SDK も再試行しない。コレクターも呼び直さず、失敗の印にする(未決事項「`LimitExceeded` の扱い」)
- `GetCredentialReport` の `ReportNotPresent`・`ReportExpired`(HTTP 410)、`ReportInProgress`(HTTP 404)は、「設定が存在しない」には当てず(`null` にしない)、失敗の印にする。レポートが無いことはユーザーの認証情報が無いことを意味しないため
- 待ちの時間切れの種類を `timeout` にするのは、待ちが上限に達したことが、操作の時間切れと同じく「時間内に結果が得られなかった」ことを表すため

## 構成

```
crates/yata-provider-aws/
├── src/
│   ├── support.rs                 # 変更: csv のモジュールの宣言を足す
│   ├── support/
│   │   └── csv.rs                 # 新規: RFC 4180 の CSV の解析(ヘッダー行とデータの行に分ける)
│   └── collectors/
│       └── iam/
│           └── user.rs            # 変更: GetCredentialReport のキー、生成を待つ手順と定数、行の対応づけ、required_permissions
└── tests/
    └── golden/
        └── iam/
            └── user.json          # 変更: GetCredentialReport のキーを加える
```

- `user.rs` に加えるもの: 待ちの定数、生成を待つ関数(待ちの実装を引数に取る)、`GetCredentialReport` の出力の確かめと解析した行の `HashMap` の作成、ユーザーごとの値の決定、ログ。`collect` は、待ちの実装を引数に取る内部の関数を呼ぶ形にし、テストはその内部の関数に偽の待ちの実装を渡す
- `required_permissions` を `["iam:ListUsers", "iam:ListAttachedUserPolicies", "iam:ListUserPolicies", "iam:GenerateCredentialReport", "iam:GetCredentialReport"]` にする
- 読み取り専用ガードの例外リスト(`support/guard_lists.rs`)と、`support/errors.rs`、`context.rs` は変更しない

### 実装の PR の分け方

[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「実装の PR の分け方」と同じく、共通部分を先に main へ入れます。

| PR | 含めるもの | 順 |
| --- | --- | --- |
| 1. 共通部分 | `support/csv.rs`(新規)と、その単体テスト。`support.rs` のモジュールの宣言 | 先に main へ入れる |
| 2. 認証情報レポート | `collectors/iam/user.rs` の変更、`tests/golden/iam/user.json` の更新 | 1 のマージの後、main を取り込んでから入れる |

- PR 1 の `support/csv.rs` には、ほかの共通部分と同じく、ファイルの先頭に `#![allow(dead_code)]` を付ける(使うコレクターが無い PR 1 の時点で、`dead_code` の警告により clippy の `-D warnings` が通らなくなるため)
- PR 2 は `src/collectors/iam/user.rs` と `tests/golden/iam/user.json` だけに触れる。共通部分の変更が必要になったら、作業を止めて報告し、別の PR にする

### 依存

- 新しいクレートを足さない。ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock` は変更しない
- CSV の解析について、クレートを足す案と比べました

| 案 | 判断 |
| --- | --- |
| 標準ライブラリだけで書く(採用) | 必要なのは上の「解析の規則」の、引用符・改行・ヘッダー行の扱いだけで、数十行の状態機械で書ける。共有ファイル(ルートの `Cargo.toml`、`crates/yata-provider-aws/Cargo.toml`、`Cargo.lock`)を変えずに済み、並列のブランチと衝突しない。規則を表のテストで固定する |
| `csv` クレート(1.4.0。ライセンス Unlicense OR MIT。依存 `csv-core`、`itoa`、`ryu`、`serde_core`) | RFC 4180 の解析が実績のある実装で得られる。ただし、共有ファイルの変更を別の PR にする必要があり、使うのは解析の一部だけになる。ライセンスの `Unlicense OR MIT` は、プロジェクトのライセンス(`MIT OR Apache-2.0`)と両立する(MIT を選べる) |

- 待ちには SDK の待ちの実装を使い、`tokio` の `time` の機能を `[dependencies]` に加えない(上の「待ちの実装」)
- テストで使うものは基盤の Issue で入っている(`aws-sdk-iam` の `test-util`、`aws-smithy-mocks`)

## 使う AWS API と IAM 権限

| API | 呼ぶ回数 | IAM 権限 | 読み取り専用ガード |
| --- | --- | --- | --- |
| `GenerateCredentialReport` | 1〜18 回(組の 1 回の実行で。ユーザーが 0 人なら 0 回) | `iam:GenerateCredentialReport` | 例外リストで許可(ADR-0005) |
| `GetCredentialReport` | 0〜1 回 | `iam:GetCredentialReport` | `Get` の接頭辞で許可 |

- どちらもパラメーターが無い。コレクターの方針の「既定のパラメーターで呼ぶ」は満たされる
- AWS 管理ポリシー `SecurityAudit` は、2 つの権限を含む
- ほかの操作(`ListUsers` など)は [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「使う AWS API と IAM 権限」のとおり

## ログ

[エラー処理とログ](../architecture/Architecture-Error-Handling-and-Logging.ja.md) に従います。メッセージは英語で、`warn` には ARN を出しません。

| レベル | 内容 |
| --- | --- |
| `warn` | レポートの生成・取得・解析が失敗した場合に 1 行(例: `"credential report unavailable: timeout when calling GenerateCredentialReport (PollingLimitReached); marked 25 users"`) |
| `warn` | 行の無いユーザーがいる場合に 1 行(例: `"3 of 25 users have no row in the credential report generated at 2026-01-01T00:00:00Z"`)。`DuplicateArn` も同じ形で 1 行 |
| `debug` | `GenerateCredentialReport` の各応答の `State` と、次の待ちの長さ。知らない `State` の値。SDK のエラーの全体(基盤の `CollectError::from_sdk` が出す) |
| `debug` | 行の無いユーザー・`DuplicateArn` のユーザーの ARN。対応するユーザーの無い行の件数(ルートユーザーの行とそれ以外に分ける)と、ルートユーザー以外の行の ARN |

- ユーザーごとの操作の `warn`(`"12 of 1500 ... calls failed"`)は、これまでどおり `ListAttachedUserPolicies` と `ListUserPolicies` についてだけ出す。`GetCredentialReport` はアカウントで 1 回の呼び出しなので、上の形で出す

## テスト方針

[テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) の「コレクターのテスト」と、[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「テスト方針」に従います。実 AWS には接続しません。CSV はダミーの値だけで作り、アカウント ID は `123456789012`、ユーザー名・ARN もダミーにします。生成を待つテストは、待った長さを記録してすぐに戻る偽の待ちの実装を使い、実時間で待ちません。

### `aws.iam.user` の認証情報レポート

- **待ち(`STARTED` → `COMPLETE`)**: `GenerateCredentialReport` が `STARTED`、`INPROGRESS`、`COMPLETE` の順に返すと、呼び出しが 3 回、待ちが 1 秒と 2 秒になり、`GetCredentialReport` が 1 回呼ばれ、各ユーザーに行が入ること。同じ操作の応答を順に変えるため、`aws-smithy-mocks` の規則の応答の列(`sequence()`)を使う
- **使い回し**: 最初の呼び出しが `COMPLETE` のとき、待たない(待ちの記録が空)こと
- **時間切れ**: `INPROGRESS` を返し続けると、呼び出しが 18 回、待ちの列が 1、2、4 と 8 × 14(17 回、合計 119 秒)で、すべてのユーザーに `timeout` の失敗の印(`PollingLimitReached`)が置かれ、`GetCredentialReport` が呼ばれないこと(規則の `num_calls()` で確かめる)
- **`State` の異常**: `State` が無い応答、知らない値(HTTP の応答のモックで `<State>FAILED</State>` など)で、待たずに `invalid_response` の失敗の印になること
- **生成の失敗**: `GenerateCredentialReport` が `AccessDenied`(`access_denied`)、`Throttling`(`throttled`)、`LimitExceeded`(`service`)、`ServiceFailure`(`service`)を返すと、すべてのユーザーの `GetCredentialReport` だけが失敗の印になり、`ListAttachedUserPolicies`・`ListUserPolicies` のキーは値のままで、`GetCredentialReport` が呼ばれないこと。メッセージに `GenerateCredentialReport` とコードがあり、サービスのメッセージの本文(ダミーの ARN を入れておく)を含まないこと
- **取得の失敗**: `GetCredentialReport` が `ReportNotPresent`・`ReportExpired`・`ReportInProgress`(`service`)、`AccessDenied`(`access_denied`)を返すと、すべてのユーザーに失敗の印が置かれ、`null` にはならないこと
- **出力の確かめ**: `ReportFormat` が無い、`text/csv` でない(HTTP の応答のモック)、`Content` が無い、`Content` が UTF-8 でない場合に、それぞれの理由のコードの `invalid_response` の失敗の印になること。`GeneratedTime` が無い場合は `null` が入り、失敗の印にならないこと
- **base64**: HTTP の応答のモック(XML)で、`Content` を base64 の文字列で返し、SDK がデコードした CSV が解析されて行が入ること(コレクターが二重にデコードしないことの確認)
- **行の無いユーザー**: `ListUsers` が 3 人を返し、CSV に 2 人の行しか無いとき、行の無いユーザーだけが `invalid_response` の失敗の印(`MissingUserRow`)になり、ほかの 2 人には行が入ること
- **`DuplicateArn`**: 同じ `arn` の行が 2 つあるユーザーだけが失敗の印になること
- **行数とユーザー数の比較**: ルートユーザーの行(`arn` が `arn:aws:iam::123456789012:root`、`user` が `<root_account>`)を含む CSV で、ルートユーザーの行を除いた行の数と、`GetCredentialReport` のキーに行(失敗の印でない値)が入ったユーザーの数が同じこと。ルートユーザーの行と、どのユーザーにも一致しない行が、どのリソースにも入らないこと
- **値を変えない**: `"TRUE"`、`"FALSE"`、`"N/A"`、`"no_information"`、`"not_supported"`、日時(`+00:00` の表記)、空の値が、そのままの文字列で入ること。列名がヘッダー行のまま(知らない列名を足した CSV でもその列が入る)であること。`GeneratedTime` が RFC 3339 の `Z` の形、`ReportFormat` が `"text/csv"` であること
- **引用符**: ユーザー名に `,`、パスに `"` と `,` を含むユーザー(ダミー)の行を、引用符で囲んだ CSV で返し、ARN で対応づけられ、値が引用符を外した文字列になること
- **解析の失敗**: `FieldCountMismatch` の行が 1 つあると、すべてのユーザーが失敗の印になり、一部の行だけが入らないこと。空の `Content`(`MissingHeader`)、`arn` 列の無いヘッダー(`MissingArnColumn`)、列名の重複(`DuplicateColumn`)、閉じていない引用符(`InvalidCsv`)も同じ
- **ヘッダー行だけ**: データの行が無い CSV で、すべてのユーザーが `MissingUserRow` になること
- **呼ばない場合**: ユーザーが 0 人のとき、`GenerateCredentialReport`・`GetCredentialReport` を送らない(`mock_factory_counted` の記録に無い)こと。`ListUsers` の失敗のときも送らないこと
- **ガード**: `GenerateCredentialReport` と `GetCredentialReport` が送信の段階に達すること(`mock_factory_counted` の記録)。読み取り専用ガードに止められないこと
- **並行**: 上限 1 と 8 の `CollectContext` で、同じ構成情報になること(レポートのキーを含む)
- **期待するファイル**: `tests/golden/iam/user.json` を、`GetCredentialReport` のキーを含む形に更新する。差分は項目の追加だけであること(既存のキーの値が変わらないこと)をレビューで確かめる
- **`required_permissions`**: 5 つの権限になること。基盤の既存のテスト(形の検査、拒否リストを含まないこと)の対象に自動でなる

### `support/csv.rs`

表のテストで、次を確かめます。

- LF と CRLF の行の区切り、末尾の行の区切りの有無、長さ 0 の行を飛ばすこと、先頭の BOM を外すこと
- 引用符で囲んだ値の中の `,`・LF・CRLF・`""`。囲んだ空の値(`""`)と囲まない空の値がどちらも空の文字列になること
- 囲まない値の前後の空白を削らないこと
- 閉じていない引用符、閉じた引用符の直後の余分な文字で `InvalidCsv` になること
- 空の入力・長さ 0 の行だけの入力で `MissingHeader` になること
- 値の数がヘッダー行と違う行で `FieldCountMismatch` になること
- 囲まない値の途中の `"`(例: `a"b`)が、そのまま値の一部になること

## 既存の設計との差異

この設計で、既存の設計書・全体設計と次の点が変わります。機能設計書([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md)、[IAM のコレクター](Feature-0012-IAM-Collectors.ja.md))は、この Issue の PR で合わせて直します。`docs/architecture/` は、この Issue の PR では変更せず、全体設計の後続 Issue(番号未定)の別の PR で反映します(未決事項「全体設計への反映」)。

| ページ | 既存の記述 | この設計 | 反映 |
| --- | --- | --- | --- |
| [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「格納の形」 | 最初のルール群の範囲と Diagram-0005 の値の形(`ReportFormat` を含まない)は、認証情報レポートのコレクターの機能設計書で、このページの形に合わせる | 最初のルール群の範囲は `{GeneratedTime, ReportFormat, Content}` に直す。Diagram-0005 は図の作業で直す。リソースモデルのこの文は、両方を直した後には事実と合わないため消す | 最初のルール群の範囲はこの Issue の PR。Diagram-0005 は図の作業。リソースモデルの文の削除は全体設計の後続 Issue(番号未定) |
| [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「コレクターの方針」 | 「すべて入れる」に対する例外は、`aws.account.account` とルートユーザーの行だけ。対象の操作の範囲は書かれていない | 「すべて入れる」の対象はリソースの情報を取る操作の出力で、別の操作の前提として呼ぶ手順の操作(`GenerateCredentialReport` など)の出力は対象外(例外ではない)。最初のルール群の範囲の「コレクターの方針」に書く | 最初のルール群の範囲はこの Issue の PR。リソースモデルへの追記は全体設計の後続 Issue(番号未定) |
| [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「`aws.iam.user`」 | `required_permissions` は 3 つ(認証情報レポートの Issue で 2 つ足す)。失敗の印は `ListAttachedUserPolicies`、`ListUserPolicies`。処理の順は 3 段 | `required_permissions` は 5 つ。失敗の印に `GetCredentialReport` を加える。処理の順は上の「処理の順」の 5 段(ユーザーごとの取得とレポートの取得を並行に進める) | この Issue の PR で、IAM のコレクターの該当の記述をこの設計への参照に直す |
| [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「`--concurrency` との関係」 | IAM の 4 つの組が同時に動くと、IAM への同時の呼び出しは最大 18 | レポートの呼び出しの 1 を加えて最大 19 | この Issue の PR |
| [IAM のコレクター](Feature-0012-IAM-Collectors.ja.md) の「構成」 | 1 つのコレクターだけが使う処理は、そのコレクターのファイルに置く(`Document` の URL デコードは `policy.rs`) | CSV の解析は今は `user.rs` だけが使うが、`support/csv.rs` に置く。例外にする基準: (1) AWS のサービスに依存しない汎用の処理で、(2) コレクターの責務(リソースの収集)と別の形式の処理であり、(3) 関数とテストの量が多く、コレクターのファイルに置くと 1 ファイル 1 責務が崩れるもの。3 つをすべて満たす場合は、使うコレクターが 1 つでも `support/` に置いてよい。`Document` の URL デコードは (1)(2) を満たすが、処理が 3 段で小さく (3) を満たさないため `policy.rs` のままとする | この設計で決める(例外の基準はこの表に書き、IAM のコレクターの「構成」の規則からこの表へ参照を足す) |
| [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「構成」 | 認証情報レポートの生成・取得・CSV の解析は `collectors/iam/user.rs` に置く | 生成・取得・対応づけは `user.rs`、CSV の解析は `support/csv.rs`(上の「解析の関数の置き場所」) | この Issue の PR |
| [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の「コレクターの方針」「認証情報レポート」と未決事項 | `Content` の構造化は「バイナリは入れない」規則の例外。値は `{GeneratedTime, Content}`。行の無いユーザーと待ち方は未決 | [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の Blob の規則による構造化で、例外ではない。値は `{GeneratedTime, ReportFormat, Content}`。行の無いユーザーは失敗の印、待ち方はこの設計の定数 | この Issue の PR |

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| 全体設計への反映 | [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) に、(1) 「すべて入れる」の対象はリソースの情報を取る操作の出力で、前提の手順の操作の出力は対象外であることを加え、(2) 「格納の形」の「最初のルール群の範囲と Diagram-0005 の値の形(`ReportFormat` を含まない)は…合わせる」の文を消す(上の「既存の設計との差異」) | 全体設計の後続 Issue(番号未定)の別の PR。(2) は Diagram-0005 を直した後 |
| 待ち方の定数の実測による見直し | 最初の間隔 1 秒、上限 8 秒、合計 120 秒は、AWS が生成の時間を公開していないことと、ほかの OSS の待ち方からの判断で、実測していない。ユーザーの多いアカウントで時間切れが起こるか、使い回しが無いときに何回の呼び出しで `COMPLETE` になるかを確かめ、値を見直すか。実 AWS での確認にはメンテナーの確認が要る | 最初のリリースの前の実環境での確認、または時間切れの報告があった時点 |
| 待ち方の定数を利用者が変える手段 | 生成が 120 秒を超えるアカウントのために、CLI のオプション・環境変数で上限を変えられるようにするか | 時間切れの報告があった時点(CLI の変更になるため、全体設計の Issue として) |
| `LimitExceeded` の扱い | `GenerateCredentialReport` の `LimitExceeded` がどういう場合に返るか(生成の頻度の制限か、ほかの制限か)は確かめられなかった。生成の頻度の制限なら、4 時間以内のレポートが残っている見込みがあり、`GetCredentialReport` を試す扱いも考えられる。今は失敗の印(`service`)にする | 実環境で `LimitExceeded` を確かめられた時点 |
| 行の無いユーザーのエラーの種類 | `MissingUserRow` を `invalid_response` にしたが、応答は正しく、使い回しのレポートが古いことが原因である。利用者に原因が伝わりやすいよう、専用の種類(例: `stale_data`)を設けるか。設ける場合は `CollectionErrorKind`(`yata-core`)と全体設計の変更になる | 利用者からの報告があった時点、または失敗の印の種類を見直す全体設計の Issue |
| 認証情報レポートに無い認証情報 | レポートは、ユーザーごとに最初の 2 つのアクセスキーと、IAM が管理する認証情報だけを含み、サービス固有の認証情報(CodeCommit のパスワードなど)は含まない(ユーザーガイドの注記)。3 つ目以降のアクセスキーなどを判定に使う場合に、`ListAccessKeys` などを加えるか | そうしたルールを加えるとき |

## 出典

- `GenerateCredentialReport`(`State` の値 `STARTED | INPROGRESS | COMPLETE`、`LimitExceeded`・`ServiceFailure`): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GenerateCredentialReport.html>
- `GetCredentialReport`(`Content` は base64 の Blob、`GeneratedTime`、`ReportFormat` の値 `text/csv`、`ReportExpired`・`ReportNotPresent`(410)、`ReportInProgress`(404)): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetCredentialReport.html>
- IAM の認証情報レポート(4 時間ごとの生成と使い回し、CSV の列と `N/A`・`no_information`・`not_supported`、必要な権限、レポートに含まれない認証情報): <https://docs.aws.amazon.com/IAM/latest/UserGuide/credential-reports.html>
- IAM の識別子(ルートユーザーの ARN `arn:aws:iam::<account_id>:root`): <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html>
- `CreateUser`(`UserName` のパターン `[\w+=,.@-]+`、`Path` の文字の範囲): <https://docs.aws.amazon.com/IAM/latest/APIReference/API_CreateUser.html>
- RFC 4180(CSV の形式): <https://www.rfc-editor.org/rfc/rfc4180>
- SDK の型(`ReportStateType`、`GetCredentialReportOutput`、`Config::sleep_impl`、`config::{AsyncSleep, SharedAsyncSleep, Sleep}`): <https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/types/enum.ReportStateType.html>、<https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/operation/get_credential_report/struct.GetCredentialReportOutput.html>、<https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/config/struct.Config.html>、<https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/config/index.html>
- botocore の IAM の waiter の定義(参照した版: コミット `b4236ac`): <https://github.com/boto/botocore/blob/b4236ac68edf4bd2b08a32833cbe35528889133e/botocore/data/iam/2010-05-08/waiters-2.json>
- Prowler(`_get_credential_report`。参照した版: コミット `ec0bb53`): <https://github.com/prowler-cloud/prowler/blob/ec0bb5383975c6fc5d4c9bdf26231d0d00f8deca/prowler/providers/aws/services/iam/iam_service.py>
- ScoutSuite(`get_credential_reports`。参照した版: コミット `2d845ac`): <https://github.com/nccgroup/ScoutSuite/blob/2d845ac0cf85ccf7135470b51f58981d36628bd1/ScoutSuite/providers/aws/facade/iam.py>
- Cloud Custodian(`CredentialReport`、`report_delay` の既定 10 秒。参照した版: コミット `c5f66e5`): <https://github.com/cloud-custodian/cloud-custodian/blob/c5f66e5243165b4198cae5f95fb71c742114a726/c7n/resources/iam.py>
- Steampipe の AWS のプラグイン(`aws_iam_credential_report`。参照した版: コミット `7aca2c5`): <https://github.com/turbot/steampipe-plugin-aws/blob/7aca2c52a7cb40bab2588003b318597e002e6c3d/aws/table_aws_iam_credential_report.go>
- `csv` クレート(版 1.4.0、ライセンス、依存。参照した版: コミット `4a3997e`): <https://github.com/BurntSushi/rust-csv/blob/4a3997e91d668ea1d8595bdef15625a77cf2308a/Cargo.toml>
- AWS 管理ポリシー `SecurityAudit`: <https://docs.aws.amazon.com/aws-managed-policy/latest/reference/SecurityAudit.html>
