# 初期依存クレート

> This Japanese version is the authoritative source. The English version (`Architecture-Initial-Dependencies.md`) has not been written yet.

初期に採用する依存クレートの候補、採用理由、ライセンスを示します。実際の追加は、骨組み・各機能を実装する Issue で、メンテナーの判断を経てから行います。バージョンは追加する時点の最新の安定版とし、この文書には書きません。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## ライセンスの方針

- Yata Cloud は `MIT OR Apache-2.0` で配布する
- 依存クレートは、MIT、Apache-2.0、BSD、ISC、Unlicense、Zlib など、配布物に含めても Yata Cloud のライセンスを変えずに済む寛容なライセンスのものに限る。コピーレフトのライセンス(GPL、LGPL、MPL など)のクレートは使わない
- Apache-2.0 だけ、MIT だけのクレートも使ってよい(依存クレートのライセンスは Yata Cloud 自身のコードのライセンスに影響しない)。バイナリを配布するときは、依存クレートの著作権表示とライセンス文を同梱する(方法は未決事項)

## 実行時の依存

| クレート | 使うクレート | 用途 | 採用理由 | ライセンス |
| --- | --- | --- | --- | --- |
| `clap`(`derive`) | `yata-cli` | 引数の解析 | Rust の CLI で事実上の標準。サブコマンド、ヘルプ、補完の生成に対応。引数の誤りで終了コード 2 を返す | MIT OR Apache-2.0 |
| `tokio`(`rt-multi-thread`、`macros`、`sync`、`signal`) | `yata-cli`、`yata-provider-aws` | 非同期ランタイム、同時実行数の制限、Ctrl-C の検知 | AWS SDK for Rust の既定のランタイム | MIT |
| `aws-config` | `yata-provider-aws` | 認証情報・リージョンの解決 | AWS 公式。既定の認証情報プロバイダチェーンを使える | Apache-2.0 |
| `aws-sdk-sts` | `yata-provider-aws` | `GetCallerIdentity` | AWS 公式 | Apache-2.0 |
| `aws-sdk-ec2` | `yata-provider-aws` | `DescribeRegions`(CIS のチェックでも EC2 の API を使う) | AWS 公式 | Apache-2.0 |
| `aws-sdk-iam` | `yata-provider-aws` | IAM の種別(アカウントの概要、パスワードポリシー、ユーザー、ポリシー)の収集と認証情報レポート | AWS 公式 | Apache-2.0 |
| `aws-sdk-s3` | `yata-provider-aws` | S3 のバケットの収集 | AWS 公式 | Apache-2.0 |
| `aws-sdk-s3control` | `yata-provider-aws` | アカウント単位の S3 のブロックパブリックアクセスの収集 | AWS 公式。アカウント単位の設定は S3 Control の API にしか無い | Apache-2.0 |
| `aws-sdk-cloudtrail` | `yata-provider-aws` | CloudTrail の証跡の収集 | AWS 公式 | Apache-2.0 |
| `aws-sdk-cloudwatchlogs` | `yata-provider-aws` | CloudWatch Logs のメトリクスフィルターの収集 | AWS 公式 | Apache-2.0 |
| `aws-sdk-cloudwatch` | `yata-provider-aws` | CloudWatch のアラームの収集 | AWS 公式 | Apache-2.0 |
| `aws-sdk-sns` | `yata-provider-aws` | SNS のトピックと購読の収集 | AWS 公式 | Apache-2.0 |
| `aws-smithy-runtime-api` | `yata-provider-aws` | 読み取り専用ガード(インターセプター)の実装 | AWS SDK の一部。操作のメタデータとインターセプターの型を提供する | Apache-2.0 |
| `regorus` | `yata-engine` | Rego の評価 | 純 Rust の Rego 実装(Microsoft)。選定理由は [ADR: ポリシーエンジン](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md) | MIT AND Apache-2.0 AND BSD-3-Clause |
| `serde`(`derive`) | `yata-core`、`yata-report` ほか | シリアライズ | 事実上の標準 | MIT OR Apache-2.0 |
| `serde_json` | `yata-core`、`yata-engine`、`yata-report`、`yata-provider-aws` ほか | JSON の読み書き、構成情報(`serde_json::Value`)、JSON Pointer の解釈(`Value::pointer`) | 事実上の標準 | MIT OR Apache-2.0 |
| `tempfile` | `yata-cli` | `--output` の一時ファイル、スナップショットの一時ディレクトリ(出力先と同じ親ディレクトリに、衝突しない名前で作る) | 一時ファイル・ディレクトリを安全な名前で作り、失敗時に削除できる。開発時の依存としても使う | MIT OR Apache-2.0 |
| `thiserror` | ライブラリのクレート | エラーの enum の定義 | 定型のコードを減らし、`#[source]` で原因を保持できる | MIT OR Apache-2.0 |
| `tracing` | 全クレート | ログ | AWS SDK も `tracing` でログを出すため、まとめて扱える | MIT |
| `tracing-subscriber`(`fmt`、`env-filter`) | `yata-cli` | ログの出力、`YATA_LOG` によるフィルタ | `tracing` の標準の出力先 | MIT |
| `comfy-table` | `yata-report` | table 形式の整形 | 列幅の自動調整、端末幅への対応、色付けができる | MIT |

AWS SDK のサービスのクレートは、[最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) で使うものを先にまとめて追加します。追加の時期と PR の分け方は [ワークスペース構成](Architecture-Workspace-Layout.ja.md) の「共有ファイル」を参照してください。

- バージョンは追加する時点の最新の安定版とし、既にある `aws-config`・`aws-sdk-sts`・`aws-sdk-ec2` と同じ時期のリリースにそろえる
- 既定の機能(`default-features`)を外すかどうかは、依存を追加する PR でビルド時間と合わせて判断する

`regorus` のライセンスは、crates.io の登録では `MIT AND Apache-2.0 AND BSD-3-Clause`(3 つすべてに従う)です。いずれも寛容なライセンスで、著作権表示とライセンス文の同梱を守れば Yata Cloud の配布に使えます。

## 開発時の依存(dev-dependencies)

| クレート | 使うクレート | 用途 | 採用理由 | ライセンス |
| --- | --- | --- | --- | --- |
| `aws-smithy-mocks` | `yata-provider-aws` | SDK の応答のモック、リクエストの照合 | AWS のドキュメントで推奨されているテストの方法。SDK のサービスのクレートの `test-util` 機能とあわせて使う(`yata-provider-aws` の `[dev-dependencies]` で、使う SDK のサービスのクレートすべての `test-util` を有効にする) | Apache-2.0 |
| `assert_cmd` | `yata-cli` | バイナリの統合テスト | 子プロセスの実行、終了コード・出力の確認が簡単に書ける | MIT OR Apache-2.0 |
| `tempfile` | 各クレート | 一時ディレクトリ・ファイル | テストを worktree 間で独立させるため | MIT OR Apache-2.0 |

## 採用しないもの

| クレート | 理由 |
| --- | --- |
| `cel`(CEL の実装) | ポリシーエンジンに Rego を採用したため([ADR](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md)) |
| `anyhow` | `yata-cli` で終了コードを決めるのにエラーの型が必要なため。`thiserror` の enum で足りる |
| `async-trait` | コレクターのトレイトは boxed future を返す形で書けるため |
| `inventory` / `linkme` | コレクターの自動登録は build.rs で行うため([ADR](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md)) |
| `walkdir` | ポリシー・スナップショットのディレクトリの走査は、深さ・ファイル数の上限とシンボリックリンクの検査を自前で行う必要があり、標準ライブラリの `std::fs::read_dir` と `symlink_metadata` で書く方が制御しやすいため |
| AWS SDK の serde 対応(unstable の機能) | 構成情報は SDK の型から自前で変換するため([リソースモデル](Architecture-Resource-Model.ja.md) の「SDK の応答を格納する方式」) |
| `include_dir` | ルールをバイナリに埋め込まないため([ADR](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md)) |
| `semver` | マニフェストの `min_yata_version` は `x.y.z` の数値の比較だけで足り、バージョンの範囲の指定は使わないため |
| `toml` | マニフェストを JSON にし、既存の `serde_json` で読むため |
| `serde-sarif` | 必要な SARIF の項目は少なく、serde の構造体を自分で書けば足りるため。出力の形を自分で管理できる。必要になれば再検討する(ライセンスは MIT) |
| `insta` | 出力の比較は、期待するファイルと比べる簡単なテストで足りるため |

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| Blob の base64 の変換に使うクレート | 構成情報の Blob の項目を base64 の文字列にする([リソースモデル](Architecture-Resource-Model.ja.md) の「項目名と値の規則」)。候補は `aws-smithy-types` の `base64` モジュール(Apache-2.0。AWS SDK が既に依存しているため、依存の木に新しいクレートが増えない。`yata-provider-aws` の直接の依存として書く必要がある)と `base64`(MIT OR Apache-2.0)。採用する場合は追加の了承が必要。どちらでも、ルートの `[workspace.dependencies]`・`crates/yata-provider-aws/Cargo.toml`・`Cargo.lock` の変更になるため、コレクターの PR とは別の、依存を追加する単独の PR で行う | Blob の項目を base64 で入れる最初のコレクターを実装する Issue |
| 日時の整形に使うクレート | RFC 3339 の日時の生成。候補は `time`(MIT OR Apache-2.0)、`jiff`(Unlicense OR MIT)、`chrono`(MIT OR Apache-2.0。regorus の `time` 機能が依存しているため、追加のビルドが少ない) | 出力を実装する Issue |
| 依存クレートのライセンス表示 | バイナリの配布物に同梱する著作権表示の作り方(`cargo-about` などのツール) | 最初のリリースを計画する Issue |
| 依存クレートの検査 | `cargo-deny` によるライセンス・脆弱性の CI での検査 | 骨組みの Issue または別 Issue |
| Windows のアクセス権の設定 | スナップショットを作成者だけが読めるようにする ACL の設定に、`windows-acl` などのクレートが必要になるか | スナップショットを実装する Issue |
| ポリシーのディレクトリの既定の場所を決めるクレート | 既定の場所をプラットフォームのデータディレクトリにする場合に使う。候補は `directories`(MIT OR Apache-2.0。アプリ名から `ProjectDirs::data_dir()` でパスを作れる)と `dirs`(MIT OR Apache-2.0。より低レベルの関数だけを持つ)。採用する場合は追加の了承が必要。既定の場所を設けない間は追加しない | ポリシーのリポジトリの配布方法を決める Issue |

## 出典

- regorus: <https://github.com/microsoft/regorus>、crates.io の登録(ライセンス): <https://crates.io/crates/regorus>
- CEL の Rust 実装(`cel`): <https://github.com/cel-rust/cel-rust>
- AWS SDK for Rust: <https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html>
- `aws-smithy-mocks`: <https://docs.aws.amazon.com/sdk-for-rust/latest/dg/testing-smithy-mocks.html>
- AWS SDK for Rust のクレート(ライセンス): <https://crates.io/crates/aws-sdk-cloudwatchlogs>、<https://crates.io/crates/aws-sdk-s3control>、リポジトリ <https://github.com/awslabs/aws-sdk-rust>
- `aws-smithy-types` の `base64` モジュール: <https://docs.rs/aws-smithy-types/latest/aws_smithy_types/base64/index.html>
- `base64`: <https://crates.io/crates/base64>
- `comfy-table`: <https://github.com/nukesor/comfy-table>
- `serde-sarif`: <https://github.com/psastras/sarif-rs>
- `directories`: <https://crates.io/crates/directories>、`dirs`: <https://crates.io/crates/dirs>
