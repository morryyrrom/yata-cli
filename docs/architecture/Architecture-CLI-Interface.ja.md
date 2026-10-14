# CLI インターフェース

> This Japanese version is the authoritative source. The English version (`Architecture-CLI-Interface.md`) has not been written yet.

コマンド、オプション、終了コードの概略を定めます。各コマンドの詳細な仕様は、実装する Issue の機能設計書で決めます。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## 基本

- バイナリ名は `yata`
- 引数の解析は clap(derive)で行う
- 結果(レポート)は標準出力、ログ・進捗・警告は標準エラー出力に書く。標準出力をパイプやファイルにそのまま渡せるようにする
- サブコマンドは `yata-cli/src/cli.rs` に列挙し、処理は 1 サブコマンド 1 モジュール(`yata-cli/src/commands/<name>.rs`。必要なら `commands/<name>/` のサブモジュールに分ける)に置く

## コマンド

| コマンド | 内容 | AWS への接続 |
| --- | --- | --- |
| `yata scan` | 収集 → 評価 → 出力を行う | する(`--snapshot` 指定時はしない) |
| `yata collect` | 収集だけを行い、スナップショット(ディレクトリ)として保存する | する |
| `yata rules list` | ポリシーのディレクトリから読み込んだルールのメタデータと、マニフェストの `name` / `version` を表示する | しない |
| `yata rules test` | ポリシーのディレクトリのルールを検査する(マニフェスト、読み込みの検証、フィクスチャのそろい方、ダミー値、フィクスチャの実行、プロバイダの対応)。ポリシーのリポジトリの CI と、独自のルールの確認に使う | しない |
| `yata permissions` | 選んだルールに必要な IAM 権限を IAM ポリシーの JSON で出力する | しない |

`--version` と `--help` は clap の標準のものを使います。

## オプションの優先度

すべてのオプションに、次の規則を当てはめます。

1. コマンドラインのオプション
2. 環境変数
3. 既定値(既定の場所)

- 上から順に見て、最初に指定されているものだけを使う。オプションと環境変数の両方が指定されたら、**混ぜずにオプションだけを使う**。複数指定できるオプション(`--policy-dir` など)でも同じで、`--policy-dir` を 1 つでも指定すれば `YATA_POLICY_DIR` は使わない
- 環境変数を持つのは、実行する環境ごとに決まる設定(パス、ログ、色、認証のプロファイル)だけとする。1 回の実行ごとに選ぶ設定(出力形式、ルールの絞り込みなど)はオプションだけで指定する。環境変数を増やしすぎると、どの設定が効いているかが分かりにくくなるため。設定ファイルは未決事項
- **既定値が未決の項目**
  - 既定の場所が未決の項目(`--policy-dir`)は、決まるまで「既定なし」として扱い、オプションも環境変数も無ければ終了コード 2 にする
  - `YATA_POLICY_DIR` を分けた結果の空の要素(`a::b` の間など)は捨てる。空の要素しか無い(空文字列を含む)ときは指定が無いものとして扱い、`--policy-dir` も無ければ終了コード 2 にする
  - 既定の値が未決の項目(`--fail-on`、`--concurrency`)は、それを実装する Issue で値を決め、最初のリリースまでにこの表に反映する

### オプションと環境変数・既定値の一覧

| オプション | 環境変数 | 既定値 | 使うコマンド |
| --- | --- | --- | --- |
| `-v`, `--verbose`(繰り返し可) / `-q`, `--quiet` | `YATA_LOG`(`tracing-subscriber` の `EnvFilter` の書式) | `warn` | すべて |
| `--color <auto\|always\|never>` | `NO_COLOR`(値にかかわらず、あれば `never`) | `auto`(端末のときだけ色を付ける) | すべて |
| `--policy-dir <DIR>`(複数指定可) | `YATA_POLICY_DIR`(OS のパスの区切り文字で複数指定可。Linux / macOS は `:`、Windows は `;`) | なし(未決事項)。どちらも無ければ終了コード 2 | `scan`、`collect`、`rules list`、`rules test`、`permissions` |
| `--rule <PATTERN>`(複数指定可) | なし | すべてのルール | `scan`、`collect`、`rules list`、`permissions` |
| `--exclude-rule <PATTERN>`(複数指定可) | なし | なし | 同上 |
| `--min-severity <SEVERITY>` | なし | `info`(すべて) | 同上 |
| `--profile <NAME>` | `AWS_PROFILE`(AWS SDK が読む) | AWS SDK の既定の認証情報プロバイダチェーン | `scan`、`collect` |
| `--region <REGION>`(複数指定可) | なし | アカウントで有効なリージョンすべて | `scan`、`collect` |
| `--concurrency <N>` | なし | 未決事項 | `scan`、`collect` |
| `--snapshot <DIR>` | なし | なし(AWS から収集する) | `scan` |
| `--format <table\|json\|sarif>` | なし | `table` | `scan` |
| `--format <text\|json>` | なし | `text` | `rules test`([ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md)) |
| `--output <FILE>` | なし | 標準出力 | `scan`、`rules list`、`permissions` |
| `--output <DIR>` | なし | なし(必須) | `collect` |
| `--overwrite` | なし | 無効 | `collect` |
| `--show-passed` | なし | 無効 | `scan` |
| `--omit-values` | なし | 無効 | `scan` |
| `--fail-on <SEVERITY\|none>` | なし | 未決事項 | `scan` |
| `--policy-timeout <SECONDS>`(1〜3600 の整数) | なし | `30`(1 回の評価ごと。[ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「(b) 資源の上限」) | `scan`、`rules test` |

### ログの指定

- `-v` / `-q` のどちらかを指定したら、それを使い、`YATA_LOG` は使わない
- どちらも無ければ `YATA_LOG` を使う。`YATA_LOG` も無ければ `warn`
  - `YATA_LOG` が空、または空白だけのときは、無いものとして扱う(`warn`)
  - `YATA_LOG` が UTF-8 でない、または `EnvFilter` の書式として誤っているときは、使い方の誤り(終了コード 2)にする。`-v` / `-q` を指定したときは `YATA_LOG` を読まないため、値が誤っていても誤りにしない
- 詳細は [エラー処理とログ](Architecture-Error-Handling-and-Logging.ja.md)

### AWS の環境変数との関係

AWS の認証情報とリージョンの環境変数(`AWS_ACCESS_KEY_ID`、`AWS_SECRET_ACCESS_KEY`、`AWS_SESSION_TOKEN`、`AWS_PROFILE`、`AWS_REGION` など)は、`yata` が自分で読まず、AWS SDK の既定のチェーンに任せます([AWS アクセス](Architecture-AWS-Access.ja.md))。優先度の規則とは、次のように合わせます。

- `--profile` を指定した場合は、そのプロファイルを SDK に明示的に渡し、そのプロファイルの認証情報だけを使う。環境変数の認証情報(`AWS_ACCESS_KEY_ID` など)と `AWS_PROFILE` は使わない(オプション > 環境変数)
  - SDK の既定のチェーンでは環境変数の認証情報がプロファイルより先に見られるため、`--profile` のときはプロファイルの認証情報プロバイダを明示して作る。実現方法は実装時に確かめる(未決事項)
- `--profile` を指定しない場合は、SDK の既定のチェーン(環境変数の認証情報 → `AWS_PROFILE` のプロファイル → …)に任せる
- 最初の API(`sts:GetCallerIdentity`、`ec2:DescribeRegions`)を呼ぶリージョンは、次の順で決める。スキャンの対象リージョンには使わない(対象は `--region`、無ければ有効なリージョンすべて)
  - `--profile` を指定した場合: 1. `AWS_REGION` → 2. そのプロファイルの `region` → 3. `us-east-1`
  - `--profile` を指定しない場合: SDK の既定の解決(`AWS_REGION` → `AWS_PROFILE` などのプロファイルの `region`)→ 決まらなければ `us-east-1`
  - `--profile` のときも `AWS_REGION` をプロファイルの `region` より優先する。AWS CLI の挙動(`AWS_REGION` はプロファイルの `region` より優先される。出典を参照)に合わせるためで、`yata` の優先度の規則(オプション > 環境変数)の例外とする

## コマンドごとの補足

### ポリシーのディレクトリ

ルールは `yata` に含まれていないため、ルールを使うコマンドではポリシーのディレクトリの指定が必要です。

図: [ポリシーの読み込みの流れ](../diagrams/Diagram-0001-Policy-Loading.md)

- `--policy-dir` には、ルートにマニフェスト `yata-policies.json` があるディレクトリを指定する
- `yata` はポリシーをネットワークから取得しない。ポリシーのリポジトリの取得・更新は利用者が行う
- 複数のディレクトリを指定した場合は、ディレクトリごとにマニフェストを検証する。読み込み、絞り込み、プロバイダの対応の検査の順序と、失敗の種類ごとの終了コードは [ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「処理の順序」「失敗の種類と終了コード」で定める

### ルールの選択

- `--rule` / `--exclude-rule` のパターンはルール ID(`<マニフェストの name>.<ディレクトリ名>.….<ルール名>`)に対して照合する。`*` を使える
- 絞り込みは、すべてのルールを読み込んで検証した後、プロバイダの対応の検査の前に行う。プロバイダの対応の検査は、絞り込んだ後のルールだけが対象。絞り込んだ結果が 0 件なら終了コード 2
- `yata rules test` には絞り込みのオプションが無く、常にすべてのルールを検査する
- `yata collect` は、選んだルールの `target_type` / `related_types` の種別だけを収集する

### スナップショット(`collect` と `scan --snapshot`)

- `yata collect --output <DIR>` でディレクトリに保存し、`yata scan --snapshot <DIR>` で読み込む。構成とマニフェスト、上書き、書き込みの途中の失敗、読み込みの検証は [リソースモデル](Architecture-Resource-Model.ja.md) の「スナップショット」で定める
- `--snapshot` と、`--profile` / `--region` / `--concurrency` は同時に指定できない(終了コード 2)
- スナップショットには実環境の情報(アカウント ID、ARN、SDK の応答の構成情報)が含まれるため、ヘルプ、`collect` の終了時の警告、利用者ガイドで、リポジトリにコミットしたり外部に共有したりしないよう注意を示す

### 値の出力(`--omit-values`)

- 既定では、検出の `actual` / `expected`(不合格の箇所の実際の値と期待する値)を、すべての出力形式に含める
- `--omit-values` を指定すると、`actual` / `expected` を出力に含めない(`path` と `message` は含める)。レポートを外部に共有するときに使う
- 扱いの詳細は [出力形式](Architecture-Output-Formats.ja.md) の「値の扱い」

## 終了コード

### `yata scan`

| コード | 意味 |
| --- | --- |
| 0 | 正常に完了し、`--fail-on` 以上の検出が無く、スキャンも不完全でない |
| 1 | `--fail-on` 以上の重要度の検出がある |
| 2 | 使い方・入力の誤り(引数の誤り、同時に指定できないオプション、ポリシーのディレクトリの指定が無い・見つからない、マニフェストが無い・合わない、資源の上限を超えた、ルールが 0 件、ルールの読み込みエラー、絞り込みの結果が 0 件、絞り込んだ後のルールが対応していないリソース種別を宣言している、スナップショットの読み込み・検証の誤り) |
| 3 | 実行時の致命的なエラー(認証情報が無い、`sts:GetCallerIdentity` / `ec2:DescribeRegions` の失敗、`aws` 以外のパーティション、出力の書き込みの失敗など) |
| 4 | スキャンが不完全で、`--fail-on` 以上の検出は無い。不完全とは、収集の一部が失敗した、または状態が `incomplete` / `error` / `skipped`(未収集 `not_collected`・収集の失敗 `collection_failed` のどちらも)のルールがあること([ポリシー評価](Architecture-Policy-Evaluation.ja.md)) |
| 130 | Ctrl-C で中断した |

- Ctrl-C で中断した場合は、ほかの条件にかかわらず常に 130 を返す(レポートは出力しない)。下の優先順位は、中断せずに処理が終わった場合だけに適用する
- 終了コード 2 / 3 と 130 のときは、レポートを出力しない
  - 標準出力にレポートを書かない
  - `--output` のファイルは作らず、既存のファイルがあっても変更しない
  - 理由は標準エラー出力に出す
  - 例外: 出力の書き込み中の失敗で終了コード 3 になった場合
    - `--output` のときは、一時ファイルを削除し、既存のファイルは変えない
    - 標準出力のときは、途中まで出力されていることがある
- 複数に当てはまる場合は、2 → 3 → 1 → 4 → 0 の順に先のものを返す
  - 検出(1)は利用者がすぐ対応すべき情報なので、不完全(4)より優先する。不完全であることはレポートと標準エラー出力の警告で分かるようにする
- 2 は clap が引数の誤りのときに返すコードと同じ
- 図: [ルール状態と終了コードの決まり方](../diagrams/Diagram-0001-Rule-Status-and-Exit-Code.md)

### そのほかのコマンド

| コマンド | 0 | 1 | 2 | 3 | 4 | 130 |
| --- | --- | --- | --- | --- | --- | --- |
| `yata collect` | すべての収集に成功し、スナップショットを書いた | 使わない | 引数の誤り、`--output` が無い、出力先がすでにある(`--overwrite` なし)、`--overwrite` の対象がスナップショットでない、ポリシーのディレクトリ・マニフェストの誤り、資源の上限、ルールの読み込みエラー、絞り込みの結果が 0 件、対応していない種別 | 認証情報が無い、`sts:GetCallerIdentity` / `ec2:DescribeRegions` の失敗、`aws` 以外のパーティション、スナップショットの書き込みの失敗 | 収集の一部が失敗した。成功した分と `collection_errors` を含むスナップショットを書いた | 中断 |
| `yata rules list` | 一覧を出力した | 使わない | 引数の誤り、ポリシーのディレクトリ・マニフェストの誤り、資源の上限、ルールの読み込みエラー、絞り込みの結果が 0 件 | 出力の書き込みの失敗 | 使わない | 中断 |
| `yata rules test` | すべての検査に合格した | ルール単位の失敗がある(名前の規則、大きさの上限を超えた(ルールのファイル、フィクスチャ)、ルールのファイルまたはフィクスチャのディレクトリの中にシンボリックリンク・reparse point がある、構文エラー、パッケージ名とパスの不一致、メタデータの誤り、プロバイダの対応、対応するルールの無い `.fixtures`、フィクスチャのそろい方・形式、ダミー値、フィクスチャの結果) | 引数の誤り、`YATA_LOG` の誤り、ポリシーのディレクトリの指定が無い・見つからない、マニフェストが無い・合わない、マニフェストの `name` の重複、ライブラリの誤り、マニフェストの大きさ・ファイル数・ディレクトリ数・深さの上限を超えた、ルールのファイル・フィクスチャ以外にシンボリックリンク・reparse point がある、ルールが 0 件 | 出力の書き込みの失敗、内部のエラー(テストを実行するタスクの異常終了、パッケージのバージョンの解析の失敗、非同期ランタイム・ログの初期化の失敗) | 使わない | 中断 |
| `yata permissions` | IAM ポリシーを出力した | 使わない | 引数の誤り、ポリシーのディレクトリ・マニフェストの誤り、資源の上限、ルールの読み込みエラー、絞り込みの結果が 0 件、対応していない種別 | 出力の書き込みの失敗 | 使わない | 中断 |

- どのコマンドも、終了コード 2 / 3 と 130 のときの出力の扱い(出力の書き込み中の失敗の例外を含む)は `yata scan` と同じ。`yata collect` では、一時ディレクトリを削除し、既存の出力先は変えない
- `yata rules test` は、ルールごとの検査の失敗を終了コード 2 ではなく 1 として扱う。1 つのルールの問題で止めずに全ルールを検査し、失敗をまとめて報告するため。ポリシーのディレクトリ全体に関わる誤りだけを 2 にする。項目ごとの対応は [ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「失敗の種類と終了コード」の表を参照

## 出典

- AWS CLI の環境変数と優先度(`AWS_REGION` とプロファイルの `region` の関係): <https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-envvars.html>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| 設定ファイル | オプションを設定ファイル(例: `yata.toml`)で指定できるようにするか。設けるなら、優先度は「オプション > 環境変数 > 設定ファイル > 既定値」とする | 別 Issue |
| ログの JSON 出力 | `--log-format json` を設けるか | 別 Issue |
| `--fail-on` の既定値 | 既定を `info`(すべての検出で 1)とするか、より高い重要度とするか | CLI の `scan` を実装する Issue(メンテナーの判断が必要) |
| `--concurrency` の既定値 | [AWS アクセス](Architecture-AWS-Access.ja.md) の未決事項 | AWS プロバイダを実装する Issue |
| 進捗表示 | 収集中の進捗を標準エラー出力に表示するか | CLI の `scan` を実装する Issue |
| ポリシーのディレクトリの既定の場所 | 既定の場所を設けるか([ポリシー評価](Architecture-Policy-Evaluation.ja.md) の未決事項)。決まるまでは既定なし | ポリシーのリポジトリの配布方法を決める Issue |
| `--profile` の認証情報の限定 | `--profile` のとき、環境変数の認証情報を使わずにプロファイルの認証情報だけを使う方法(`aws-config` でプロファイルの認証情報プロバイダを明示する方法)の確認 | AWS プロバイダを実装する Issue |
