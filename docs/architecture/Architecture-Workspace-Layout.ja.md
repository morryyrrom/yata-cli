# ワークスペース構成

> This Japanese version is the authoritative source. The English version (`Architecture-Workspace-Layout.md`) has not been written yet.

Cargo workspace のクレート構成、クレート間の依存の向き、並列開発で衝突しないための規則を定めます。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## リポジトリのレイアウト

```
yata-cli/
├── Cargo.toml                  # workspace 定義と共通依存・共通 lint(共有ファイル)
├── Cargo.lock                  # 共有ファイル
├── clippy.toml                 # テストでの unwrap/expect を許可する設定(共有ファイル)
├── rust-toolchain.toml         # 使う Rust のバージョン(共有ファイル)
├── crates/
│   ├── yata-cli/               # バイナリ `yata`
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── cli.rs          # clap の定義(サブコマンドの列挙)
│   │   │   ├── commands/       # 1 サブコマンド 1 モジュール(<name>.rs。必要なら <name>/ のサブモジュールに分ける)
│   │   │   ├── exit_code.rs
│   │   │   └── logging.rs
│   │   └── tests/fixtures/
│   │       ├── policies/       # CLI の統合テスト用の最小のポリシーのセット
│   │       └── snapshots/      # CLI の統合テスト用のスナップショット(ダミー値)
│   ├── yata-core/              # 共通の型
│   ├── yata-provider-aws/      # AWS プロバイダ
│   │   ├── build.rs            # コレクターを自動検出してモジュール宣言と一覧を生成
│   │   └── src/
│   │       ├── collectors/<service>/<resource>.rs   # 1 リソース種別 1 ファイル
│   │       └── support/        # 認証・リージョン・読み取り専用ガードなど
│   ├── yata-engine/            # ポリシーのディレクトリの走査・Rego の読み込み・評価・ルールのテスト
│   │   └── tests/fixtures/policies/   # テスト用の最小のポリシーのセット
│   └── yata-report/            # 出力形式(1 形式 1 ファイル)
└── docs/
```

ポリシー(ルール)はこのリポジトリにもバイナリにも含めません。別のリポジトリで配布し、`yata` が実行時にポリシーのディレクトリから読み込みます([ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md))。このリポジトリにあるのは、テスト用の最小のポリシーのセットだけです([テスト方針](Architecture-Testing-Strategy.ja.md))。

## クレートと責務

| クレート | 種類 | 責務 | 主な依存 |
| --- | --- | --- | --- |
| `yata-cli` | bin(`yata`) | 引数の解析(オプション > 環境変数 > 既定値)、ポリシーのディレクトリの決定(`--policy-dir` / `YATA_POLICY_DIR`)、ルールとプロバイダの対応の検査、スキャン処理の組み立て(収集 → 評価 → 出力)、スナップショットの書き込み、ログの初期化、終了コード | すべての `yata-*`、clap、tokio、tracing-subscriber、tempfile |
| `yata-core` | lib | 共通の型: `Resource`、`ResourceSet`、`Inventory`、`AccountInfo`、`ResourceTypeInfo`、`CollectionError`(収集の失敗の記録)、`RuleMetadata`、`Severity`、`RuleOutcome`(ルール × アカウント単位の評価結果)、`RuleStatus`、`SkipReason`、`RuleResult`(リソース単位の `pass` / `fail`)、`ResultStatus`、`ScanReport` など。シリアライズ形式(JSON)の定義と、スナップショットの読み込み・検証もここ | serde、serde_json、thiserror |
| `yata-provider-aws` | lib | AWS の認証情報・リージョンの解決、コレクターの定義と実行、読み取り専用ガード、必要な IAM 権限の一覧 | `yata-core`、aws-config、aws-sdk-*、tokio、tracing |
| `yata-engine` | lib | ポリシーのディレクトリの走査とファイルの読み込み、マニフェストの検証、ライブラリと Rego ルールの読み込み・メタデータ検証、`input` の組み立てと評価、フィクスチャによるルールのテスト機能(静的検査を含む) | `yata-core`、regorus、serde、serde_json、thiserror、tracing |
| `yata-report` | lib | `ScanReport` を table / JSON / SARIF に整形して `Write` に書く | `yata-core`、serde_json、comfy-table |

### 依存の向き

図: [クレート間の依存関係](../diagrams/Diagram-0001-Crate-Dependencies.md)

- 依存は `yata-cli` → 各クレート → `yata-core` の一方向だけとする
- `yata-provider-aws`、`yata-engine`、`yata-report` は互いに依存しない
- `yata-core` は AWS SDK、regorus、出力用のクレートに依存しない
- `yata-engine` は AWS を知らない。リソース種別は文字列として扱う
- これにより、将来 AWS 以外のプロバイダを追加するときは、新しいプロバイダのクレートを足すだけで済む(他クラウド対応はスコープ外)
- プロバイダのクレート名は `yata-provider-<provider>` とする(例: `yata-provider-aws`)。プロバイダのクレート同士は互いに依存しない

### スキャン処理の組み立て

収集 → 評価 → 出力の組み立ては `yata-cli` の `commands/scan.rs` に置きます。専用のクレートは作りません。組み立ての処理は短く、各段階の中身は各クレートにあるためです。

## 並列開発で衝突しないための規則

複数のセッションが git worktree で別々の機能を同時に開発する前提で、次の規則を守ります。

### 中央の一覧を作らない

| 追加するもの | 置き場所 | 登録の仕組み |
| --- | --- | --- |
| ルール | 外部のポリシーのリポジトリの任意の階層の `<ルール名>.rego`(このリポジトリには置かない) | `yata-engine` が実行時にディレクトリを走査して見つける([ADR](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md)) |
| コレクター | `crates/yata-provider-aws/src/collectors/<service>/<resource>.rs` | build.rs がモジュール宣言と一覧を生成する([ADR](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md)) |
| リソース種別 | コレクターのファイル内の定数 | 文字列(`<provider>.<service>.<resource>` の 3 階層)で表し、共通の enum を作らない |
| ルール ID | ルールのパス | 連番を使わず、マニフェストの `name` とパスから決まる名前にする(連番は並列ブランチで重複するため) |
| 機能の設計書 | `docs/features/` | 1 機能 1 ファイル |

新しいチェックの追加は、このリポジトリでのコレクターの追加(必要な場合)と、ポリシーのリポジトリでのルールの追加に分かれます。新しいリソース種別を使うルールは、その種別に対応した `yata` のリリースの後に、ポリシーのリポジトリのマニフェストで必要な `yata` のバージョンを上げて公開します([ポリシー評価](Architecture-Policy-Evaluation.ja.md))。

### 中央の列挙を許すもの

次は追加の頻度が低く、追加には全体の設計判断を伴うため、1 ファイルに列挙します。変更は機能の PR に混ぜず、単独の PR にします。

- CLI のサブコマンド(`yata-cli/src/cli.rs`)
- 出力形式(`yata-report` の形式の列挙)
- 読み取り専用ガードの例外リスト(`yata-provider-aws/src/support/` 内。Describe / List / Get で始まらないが、リソースを作成・変更・削除しない操作。内容は [AWS アクセス](Architecture-AWS-Access.ja.md) の「読み取り専用の強制」)と拒否リスト(秘密の値の取得・認証情報の発行などを行う操作。[AWS アクセス](Architecture-AWS-Access.ja.md))

### 共有ファイル

次のファイルは並列ブランチで衝突しやすいため、機能の PR では変更しません。変更が必要になったら作業を止めて報告し、単独の PR で先に main へ入れます。

- ルートの `Cargo.toml`、`Cargo.lock`、`clippy.toml`、`rust-toolchain.toml`
- `docs/architecture/`

ルートの `Cargo.toml` は次の方針で、変更の頻度を下げます。

- `members = ["crates/*"]` とし、クレートの追加でルートを書き換えない
- 依存クレートのバージョンは `[workspace.dependencies]` にまとめ、各クレートは `xxx.workspace = true` で参照する
- AWS SDK のサービスのクレート(`aws-sdk-iam`、`aws-sdk-s3` など)は、使う予定のものを先にまとめて追加する。最初のルール群で使うのは、既にある `aws-sdk-sts`・`aws-sdk-ec2` と、追加する 7 つ(`aws-sdk-iam`、`aws-sdk-s3`、`aws-sdk-s3control`、`aws-sdk-cloudtrail`、`aws-sdk-cloudwatchlogs`、`aws-sdk-cloudwatch`、`aws-sdk-sns`)([初期依存クレート](Architecture-Initial-Dependencies.ja.md))
  - ルートの `[workspace.dependencies]` への追加と、`crates/yata-provider-aws/Cargo.toml` の `[dependencies]`・`[dev-dependencies]`(各クレートの `test-util` 機能と `aws-smithy-mocks`)への追加を 1 つの PR にまとめ、`Cargo.lock` の更新を 1 回にする。サービスごとのコレクターの PR は `Cargo.toml` と `Cargo.lock` を変更しない(詳細は [最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の「依存の入れ方」)
  - 後から新しいサービスが必要になった場合も、コレクターの PR とは別の単独の PR で、まとめて追加する

### 実行環境

- ポート番号、キャッシュ・一時ファイルのパスをハードコードしない。Yata Cloud はサーバーを立てず、キャッシュも持たない
- 出力先は `--output` で指定する。書き込みの途中の一時ファイル・一時ディレクトリは、出力先と同じ親ディレクトリに作り、固定の名前にしない。テストは `tempfile` の一時ディレクトリを使う
- ビルド生成物(build.rs の生成物を含む)は `target/`(`OUT_DIR`)に置き、ソースツリーに書き出さない

## 共通の lint とコンパイラ設定

ルートの `Cargo.toml` の `[workspace.lints]` で全クレートに適用し、各クレートは `[lints] workspace = true` とします。

| 設定 | 値 | 目的 |
| --- | --- | --- |
| `rust.unsafe_code` | `forbid` | `unsafe` の禁止。あわせて各クレートのルートに `#![forbid(unsafe_code)]` を書く |
| `rust.missing_docs` | `warn` | `pub` 項目の doc コメント漏れ。CI では `-D warnings` でエラーになる |
| `clippy.unwrap_used` | `deny` | テスト以外での `unwrap()` 禁止 |
| `clippy.expect_used` | `deny` | テスト以外での `expect()` 禁止 |

`clippy.toml` に `allow-unwrap-in-tests = true` と `allow-expect-in-tests = true` を書き、テストでは使えるようにします。

`missing_docs` が警告するのは、クレートの外から見える公開項目だけです。`pub` でもクレートの外に公開されない項目(非公開モジュールの中の `pub` 項目など)の doc コメント漏れは検出されないため、「`pub` 項目には doc コメントを必ず書く」はコードレビューで補います。

- edition は 2024 とする
- 最低対応 Rust バージョン(`rust-version`)は、AWS SDK for Rust が要求するバージョンに合わせる(SDK は最新の安定版から数バージョン前までをサポートする方針のため)。具体的な値は骨組みを作る Issue で決める
- ライセンスは全クレートで `MIT OR Apache-2.0`(`[workspace.package]` で共通化)

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| 骨組みの作成 | ルートの `Cargo.toml`、空のクレート、lint 設定、CI の作成 | 実装の最初の Issue(全体の骨組み) |
| crates.io への公開 | クレート名の確保 | 最初のリリースを計画する Issue |
