# 出力形式

> This Japanese version is the authoritative source. The English version (`Architecture-Output-Formats.md`) has not been written yet.

スキャン結果の出力形式を定めます。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## 共通

- 出力は `yata-report` が `yata-core` の `ScanReport` から作る。形式ごとに 1 ファイル(`yata-report/src/<format>.rs`)とする
- 各形式は `ScanReport` と出力先(`std::io::Write`)を受け取る関数として実装する。`yata-report` は渡された `Write` に書くだけで、ファイルを開かない
- 出力先を開くのは `yata-cli` で、標準出力か、`--output` のファイルを渡す
- `--output` でファイルに書く場合は、`yata-cli` が同じディレクトリの一時ファイルを開いて `yata-report` に渡し、書き終えてから名前を変える。途中で失敗したときに壊れたファイルを残さない
- 結果の並び順は、重要度の高い順 → ルール ID → リソース ID → リージョンとし、同じ入力から同じ出力になるようにする。同じリソース ID が複数のリージョンに現れることがあるため、リージョンも順序に含める([リソースモデル](Architecture-Resource-Model.ja.md) の「id の一意性」)
- 対応する形式は `table`、`json`、`sarif` の 3 つ
- `ScanReport` は `yata-cli` が、評価結果・収集エラー・メタ情報から組み立てる
- 終了コード 2 / 3 と 130 のときは、どの形式でもレポートを出力しない(理由は標準エラー出力に出す)。例外として、出力の書き込み中の失敗で終了コード 3 になった場合、`--output` のときは一時ファイルを削除して既存のファイルは変えず、標準出力のときは途中まで出力されていることがある([CLI インターフェース](Architecture-CLI-Interface.ja.md))

### ScanReport の型

`ScanReport` と、その項目の型は `yata-core` に置きます(項目名の細部は実装時に決めてよい)。

| 型 | 項目 | 内容 |
| --- | --- | --- |
| `ScanReport` | `schema_version: u32` | 出力の JSON の形式のバージョン |
| | `tool: ToolInfo` | ツールの情報 |
| | `policies: Vec<PolicySetInfo>` | 使ったポリシーのセット。ポリシーのディレクトリごとに 1 要素(指定した順) |
| | `scan: ScanInfo` | スキャンのメタ情報 |
| | `outcomes: Vec<RuleOutcome>` | 対象にしたルール × アカウントごとの評価結果(ルール ID → アカウントの順)。`RuleOutcome` は [ポリシー評価](Architecture-Policy-Evaluation.ja.md) を参照 |
| | `collection_errors: Vec<CollectionError>` | 収集の失敗の記録。`Inventory.collection_errors` をそのまま入れる([リソースモデル](Architecture-Resource-Model.ja.md)) |
| | `summary: Summary` | 集計。`yata-cli` が `outcomes` から計算して入れる |
| `ToolInfo` | `name: String`、`version: String` | `yata` と、そのバージョン |
| `PolicySetInfo` | `name: String`、`version: String`、`format_version: u32` | マニフェストの値。ポリシーのディレクトリのパスは含めない(利用者の環境のパスを出力に残さないため) |
| `ScanInfo` | `started_at`、`finished_at` | 開始・終了の日時(RFC 3339、UTC) |
| | `source: ScanSource` | `Aws`(AWS から収集)/ `Snapshot`(`--snapshot` で評価) |
| | `accounts: Vec<AccountInfo>` | 対象のアカウント(アカウントキー、ID、プロバイダ)。当面は 1 つだけ |
| | `regions: Vec<String>` | 対象リージョン |
| | `values_omitted: bool` | `--omit-values` で値を省いたか |
| `Summary` | `rules`(状態ごとの件数)、`results`(`pass` / `fail` の件数)、`findings_by_severity`(重要度ごとの検出の件数) | 集計 |

`PolicySetInfo` は `yata-core` の型で、`yata-engine` が読み込んだマニフェストから作ります。マニフェスト自体の型(`min_yata_version` を含む)は `yata-engine` に置きます。

JSON の形式では、`outcomes` を `rules[]`(`results` を除いたルール × アカウント単位の情報)と `results[]`(全ルールの `RuleResult` をまとめたもの)に分けて出力します(下の例)。

### 値の扱い

検出(`fail`)は、ルールの `reason` が返した `path`(不合格の箇所。JSON Pointer)、`actual`(実際の値)、`expected`(期待する値)を持ちます([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「ルールの規約」)。

- `actual` は構成情報から取り出した値で、構成情報は伏せずに入っているため、機微な値(メールアドレス、URL に埋め込まれた認証情報など)が入りうる([リソースモデル](Architecture-Resource-Model.ja.md) の「秘密の値の扱い」)。レポートを外部に出すときは `--omit-values` を使う
- 既定では、すべての形式に `actual` / `expected` を含める。`--omit-values` を指定した場合は、どの形式でも `actual` / `expected` を省き、`scan.values_omitted` を `true` にする(`path` と `message` は残す)。状態が `error` のルールの `error` からも、値を含みうる regorus のメッセージだけを省く。`yata-engine` が作る説明(値を含まない。リソースの `id` とリージョンを含む)は残す([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「評価のエラー」)。レポートを外部(CI の公開ログ、GitHub code scanning など)に出すときに使う
- `message` はルールが作る文字列なので、ルールが値を埋め込んでいれば `--omit-values` でも残る。ルールの作者向けの規約として、`message` には値を埋め込まず、値は `reason` の `actual` に入れる([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「ルールの規約」)
- table では、`actual` / `expected` を JSON の文字列にし、長いものは一定の長さで切り詰めて表示する(長さは実装時に決める)。JSON と SARIF では切り詰めない

## table

人が端末で読むための形式で、既定の形式です。

- 先頭に集計(重要度ごとの検出の件数、ルールの状態ごとの件数)を表示する
- 続けて検出(`fail`)を表で表示する。列は重要度、ルール ID、リソース ID、リージョン、メッセージ、箇所(`path`)、値(`actual` と `expected`。`--omit-values` のときは表示しない)
- `--show-passed` のときは `pass` の結果も表示する
- 収集エラーとルールの `skipped` / `error` / `incomplete` があれば、最後にまとめて表示する。`incomplete` では、取得の失敗の印を持っていたリソース(`incomplete_resources`)と失敗した操作名も表示する。`--show-passed` で表示する `pass` のうち、`incomplete_resources` に挙がったリソース(リソース ID とリージョンで対応づける)のものには印を付ける
- 色付けは `--color` と `NO_COLOR` に従う
- table の書式は互換性を保証しない(機械処理には JSON を使う)

## json

機械処理のための形式です。

```json
{
  "schema_version": 1,
  "tool": { "name": "yata", "version": "0.1.0" },
  "policies": [
    { "name": "example_policies", "version": "0.1.0", "format_version": 1 }
  ],
  "scan": {
    "started_at": "2026-01-01T00:00:00Z",
    "finished_at": "2026-01-01T00:01:00Z",
    "source": "snapshot",
    "accounts": [
      { "account": "aws-123456789012", "id": "123456789012", "provider": "aws" }
    ],
    "regions": ["us-east-1"],
    "values_omitted": false
  },
  "summary": {
    "rules": { "evaluated": 1, "incomplete": 0, "skipped": 1, "error": 0 },
    "results": { "pass": 0, "fail": 1 },
    "findings_by_severity": { "critical": 0, "high": 1, "medium": 0, "low": 0, "info": 0 }
  },
  "rules": [
    {
      "id": "example_policies.s3.bucket_public_access_block",
      "account": "aws-123456789012",
      "title": "S3 buckets should block public ACLs",
      "severity": "high",
      "target_type": "aws.s3.bucket",
      "related_types": [],
      "status": "evaluated",
      "skip_reason": null,
      "error": null,
      "incomplete_resources": [],
      "references": []
    },
    {
      "id": "example_policies.example.widget_check",
      "account": "aws-123456789012",
      "title": "Example rule whose resource type was not collected",
      "severity": "medium",
      "target_type": "aws.example.widget",
      "related_types": [],
      "status": "skipped",
      "skip_reason": "not_collected",
      "error": null,
      "incomplete_resources": [],
      "references": []
    }
  ],
  "results": [
    {
      "rule_id": "example_policies.s3.bucket_public_access_block",
      "status": "fail",
      "severity": "high",
      "resource": {
        "id": "arn:aws:s3:::example-bucket",
        "type": "aws.s3.bucket",
        "account": "aws-123456789012",
        "region": "us-east-1"
      },
      "message": "Bucket arn:aws:s3:::example-bucket does not block public ACLs",
      "path": "/configuration/GetPublicAccessBlock/PublicAccessBlockConfiguration/BlockPublicAcls",
      "actual": false,
      "expected": true
    }
  ],
  "collection_errors": []
}
```

- `pass` の結果も常に含める(`--show-passed` は table だけに効く)
- `rules[]` の `skip_reason` と `error` は常に置き、該当しないときは `null` にする。`skip_reason` は `status` が `skipped` のときだけ値(`not_collected` / `collection_failed`)を持ち、`error` は `status` が `error` のときだけ値(説明の文字列)を持つ
- `rules[]` の `incomplete_resources` は、構成情報に取得の失敗の印を持っていたリソースの一覧(`resource_id`、`resource_type`、`region`、`operation`、`kind`)。`region` はリソースの `region`(同じ `resource_id` が複数のリージョンに現れる場合に区別するため)。無ければ空の配列
- `results[]` はリソース単位の `pass` / `fail` の結果(`RuleResult`)を、全ルール分まとめて並べる
- `results[]` は 1 ルール × 1 リソースにつき 1 件。`fail` では `message`、`path`、`actual`、`expected` を常に置く。`message` はルールの `message`(無ければメタデータの `title`)で、いつも文字列。`path`、`actual`、`expected` は、ルールの `reason` に無かったものを `null` にする(`actual` / `expected` の値そのものが `null` の場合と区別しない)。`--omit-values` のときは `actual` / `expected` のキーごと省く。`pass` ではこの 4 つを置かない
- `schema_version` は互換性の無い変更(項目の削除・意味の変更)のときに上げる。項目の追加では上げない
- `scan.source` は収集元を表す。AWS から直接収集した場合は `aws`、`--snapshot` で評価した場合は `snapshot`。上の例は、別のルールを選んで `yata collect` したスナップショットを評価したため、1 件が `not_collected` になった場合を示す
- 値はすべてダミーの例です

## sarif

CI のコードスキャン結果の表示(GitHub code scanning など)と連携するための形式です。SARIF 2.1.0 に従います。

| SARIF の項目 | 値 |
| --- | --- |
| `version` | `2.1.0` |
| `runs[0].tool.driver.name` / `version` | `yata` / バージョン |
| `runs[0].tool.driver.rules[]` | 評価したルール。`id` = ルール ID、`shortDescription` = `title`、`fullDescription` = `description`、`help` = `remediation` |
| `rules[].properties.security-severity` | 重要度を数値にしたもの(critical = 9.5、high = 8.0、medium = 5.5、low = 2.0、info は付けない) |
| `runs[0].results[]` | 検出(`fail`)だけ。`ruleId`、`message.text`、`level` |
| `level` | critical / high → `error`、medium → `warning`、low / info → `note` |
| `results[].locations[].logicalLocations[]` | リソース ID(`fullyQualifiedName`)と種別(`kind`) |
| `results[].properties` | `path`、`actual`、`expected`、`account`、`region`(`--omit-values` のときは `actual` / `expected` を省く) |
| `runs[0].invocations[]` | 収集エラー、ルールのエラー、`incomplete_resources` を `toolExecutionNotifications` に入れる |

- SARIF の型は、必要な部分だけを `yata-report` 内に serde の構造体として定義する(専用のクレートを使わない。理由は [初期依存クレート](Architecture-Initial-Dependencies.ja.md))
- GitHub code scanning は、結果にファイルの位置(`physicalLocation` / `artifactLocation.uri`)が無いと表示しない。クラウドのリソースにはファイルの位置が無いため、この扱いは未決事項とする

## 出典

- SARIF 2.1.0: <https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html>
- GitHub の SARIF 対応(`security-severity`、必須の項目): <https://docs.github.com/en/code-security/code-scanning/integrating-with-code-scanning/sarif-support-for-code-scanning>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| GitHub code scanning での表示 | 結果にファイルの位置が必要なため、IaC のファイルとの対応付け、または仮の位置を入れるかどうか。SARIF としての妥当性は位置なしでも保てる | SARIF 出力を実装する Issue |
| 日時の整形に使うクレート | RFC 3339 の日時を作るクレート([初期依存クレート](Architecture-Initial-Dependencies.ja.md) の未決事項) | 出力を実装する Issue |
| SARIF での同じリソース ID の区別 | 同じリソース ID でリージョンの違う検出(CloudTrail の shadow trail など)は、`logicalLocations` の `fullyQualifiedName` が同じになり、区別が `properties.region` だけになる。GitHub code scanning などで同じ結果として扱われうるため、リージョンを `logicalLocations` に含める(2 つ目の logical location にするなど)か | SARIF 出力を実装する Issue |
| SARIF のスキーマ検証 | テストで SARIF の JSON Schema による検証を行うか(検証用の dev-dependency が増える) | SARIF 出力を実装する Issue |
| ほかの形式 | CSV、HTML、OCSF など | 需要が出た時点で別 Issue |
