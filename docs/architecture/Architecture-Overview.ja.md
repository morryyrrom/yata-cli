# アーキテクチャ概要

> This Japanese version is the authoritative source. The English version (`Architecture-Overview.md`) has not been written yet.

Yata Cloud 全体設計の入口ページです。全体像と設計原則をここにまとめ、詳細は各ページに分けています。

## 目的

Yata Cloud は Rust 製の OSS CNAPP です。初期スコープは AWS アカウントの CSPM(設定ミスの診断)で、CLI として提供します。

全体設計では、次の 2 点を満たす構成を定めます。

- AWS のリソースを読み取り専用で収集し、宣言的なルールで評価し、結果を複数の形式で出力する
- 複数の機能(ルール、リソースの収集処理、出力形式など)を、別々のブランチで並列に開発しても衝突しない

## 設計原則

| 原則 | 内容 |
| --- | --- |
| 読み取り専用 | AWS には Describe / List / Get 系の API だけを呼ぶ。実行時のガードとテストで強制する([AWS アクセス](Architecture-AWS-Access.ja.md)) |
| 宣言的なルール | ルールは Rego で書き、Rust のコードと分ける([ADR: ポリシーエンジン](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md)) |
| 中央レジストリなし | ルールは 1 ルール 1 ファイルとし、実行時にポリシーのディレクトリを走査して見つける。収集処理は 1 リソース種別 1 ファイルとし、ビルド時に自動検出する。どちらも一覧ファイルに追記しない |
| ポリシーの分離 | ルールはバイナリにもこのリポジトリにも含めず、別のリポジトリで配布して実行時に読み込む。互換性はポリシーのディレクトリのマニフェストで確かめる([ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md)) |
| 層の分離 | 収集・評価・出力を別クレートにし、互いに依存させない。共通の型だけを `yata-core` に置く |
| テストで実 AWS に接続しない | フィクスチャとモックだけでテストする([テスト方針](Architecture-Testing-Strategy.ja.md)) |
| 安全な Rust | `unsafe` 禁止、テスト以外で `unwrap()` / `expect()` 禁止、`pub` 項目に doc コメント必須 |

## 用語

| 用語 | 意味 |
| --- | --- |
| リソース(Resource) | 収集した AWS のリソース 1 件。共通の項目と、SDK が返した構成情報(`configuration`)からなる。S3 バケット、IAM ユーザー、アカウント単位の設定など |
| リソース種別(resource type) | リソースの種類を表す文字列。`<provider>.<service>.<resource>` の 3 階層(例: `aws.s3.bucket`) |
| アカウントキー(account key) | アカウントを表す文字列。`<provider>-<account_id>`(例: `aws-123456789012`) |
| ResourceSet | アカウント × リージョン × リソース種別ごとの収集結果 |
| Inventory | 1 回の収集の全体(ResourceSet の集まりと、収集の失敗の記録) |
| スナップショット(Snapshot) | `yata collect` が Inventory を保存したディレクトリ。ルートにマニフェスト `yata-snapshot.json` を置く |
| コレクター(Collector) | 1 つのリソース種別を AWS API から収集する処理 |
| ルール(Rule) | 1 つのチェック。Rego で書いた 1 ファイル(`<ルール名>.rego`)で、メタデータと不合格を返す `fail` からなる |
| フィクスチャ(Fixture) | ルールの合格・不合格を確かめるための入力データ(ダミー値のみ)。`<ルール名>.fixtures/` に置く |
| ポリシーのディレクトリ(policy directory) | ルールを置いたディレクトリ。ルートにマニフェスト(`yata-policies.json`)を置き、その下の階層は自由。外部のポリシーのリポジトリを取得したものを指す |
| マニフェスト(Manifest) | ポリシーのディレクトリのルートに置く、ポリシーのセットの名前、形式のバージョン、必要な `yata` のバージョンを書いたファイル。ルールの一覧ではない |
| 結果(Result) | ルール × リソースごとの判定。`pass` / `fail` |
| 検出(Finding) | `fail` の結果 |
| スキャン(Scan) | 収集 → 評価 → 出力の一連の処理 |

## コンポーネント

| コンポーネント | クレート | 責務 |
| --- | --- | --- |
| CLI | `yata-cli`(バイナリ名 `yata`) | 引数の解析、ポリシーのディレクトリの決定、処理全体の組み立て、ログの初期化、終了コードの決定 |
| コア | `yata-core` | 共通の型(リソース、ルールのメタデータ、結果、重要度、スキャン結果)。AWS・Rego・出力形式に依存しない |
| AWS プロバイダ | `yata-provider-aws` | 認証情報・リージョンの解決、コレクターの実行、読み取り専用ガード |
| ポリシーエンジン | `yata-engine` | ポリシーのディレクトリの走査、マニフェストの検証、Rego ルールの読み込み・検証・評価、フィクスチャによるルールのテスト |
| レポート | `yata-report` | スキャン結果を table / JSON / SARIF に整形する |

外部の要素として、ルールを置いた「ポリシーのディレクトリ」(別のリポジトリで配布され、利用者が取得したもの)があります。

クレートの構成と依存の向きは [ワークスペース構成](Architecture-Workspace-Layout.ja.md) を参照してください。

図: [コンポーネント図](../diagrams/Diagram-0001-Component-Overview.md)

## データの流れ

`yata scan` は次の順に処理します。

1. **準備**(`yata-cli`): 引数を解析し、ログを初期化する
2. **ルールの読み込み**(`yata-cli`, `yata-engine`): `yata-cli` がポリシーのディレクトリを決め(`--policy-dir` → `YATA_POLICY_DIR`。どちらも無ければ終了コード 2)、`yata-engine` に渡す。`yata-engine` がディレクトリの存在を確かめ(見つからない・読めなければ終了コード 2)、マニフェストで互換性を確かめ、資源の上限の範囲で走査してライブラリとルール(`<ルール名>.rego`)を読み込み、メタデータを検証する。`yata-cli` が `yata-engine` の関数で `--rule` などの絞り込みを適用する(0 件なら終了コード 2)
3. **収集対象の決定**(`yata-cli`): 選ばれたルールのメタデータ `target_type` / `related_types` を集め、`yata-provider-aws` が公開する対応する種別の一覧と照らし合わせる(対応していない種別があれば終了コード 2)。必要なリソース種別だけを収集対象にする
4. **収集**(`yata-provider-aws`): 認証情報を解決し、`sts:GetCallerIdentity` でアカウントを確認し、対象リージョンを決める。アカウント × リージョン × リソース種別ごとにコレクターを並行実行し、成功した組を `ResourceSet` にして、`Inventory` にまとめて `yata-cli` に返す。失敗した組は `Inventory.collection_errors` に記録して続行する
5. **評価**(`yata-engine`): `yata-cli` が `Inventory` を `yata-engine` に渡す。`yata-engine` がルール × アカウントごとに、そのアカウントの全リージョンの、ルールが宣言したリソース種別のリソースを入れ子の形(`input.aws.s3.bucket` など)にまとめて `input` を作り、Rego を評価して `pass` / `fail` の結果とルールの状態を作る
6. **レポートの組み立て**(`yata-cli`): 評価結果(ルールの状態と結果)、収集エラー(`collection_errors`)、メタ情報(ツールのバージョン、開始・終了の日時、アカウント、リージョン、入力の種類)から `ScanReport` を作る
7. **出力**(`yata-report`): `yata-cli` が出力先(標準出力、または `--output` の一時ファイル)を開いて `std::io::Write` として渡し、`yata-report` が指定の形式で書く。ファイルの場合は、書き終えた後に `yata-cli` が一時ファイルを目的の名前に変える
8. **終了**(`yata-cli`): 検出・エラーの有無から終了コードを決める。終了コード 2 / 3 と 130 のときは、レポートを出力しない(理由は標準エラー出力に出す)。例外として、出力の書き込み中の失敗で終了コード 3 になった場合、`--output` のときは一時ファイルを削除して既存のファイルは変えず、標準出力のときは途中まで出力されていることがある([CLI インターフェース](Architecture-CLI-Interface.ja.md))

`yata scan --snapshot <DIR>` では手順 4 の代わりに、`yata-cli` が `yata-core` の読み込み関数でスナップショットのディレクトリ(`Inventory`)を読み込み、検証します(AWS に接続しない)。マニフェストが無い・`schema_version` が一致しない・中身とマニフェストが一致しないなどの場合は終了コード 2 で終了します。`yata collect` は手順 1〜4 の後、`Inventory` をスナップショットのディレクトリとして保存します([リソースモデル](Architecture-Resource-Model.ja.md))。

図: [yata scan の処理順](../diagrams/Diagram-0001-Scan-Sequence.md)

## ページ一覧

| ページ | 内容 |
| --- | --- |
| [ワークスペース構成](Architecture-Workspace-Layout.ja.md) | クレートと責務、依存の向き、並列開発で衝突しないための規則、共通の lint 設定 |
| [リソースモデル](Architecture-Resource-Model.ja.md) | 収集したリソースの表現、ResourceSet と Inventory、スナップショット、ポリシーエンジンへの渡し方 |
| [ポリシー評価](Architecture-Policy-Evaluation.ja.md) | ルールの書式、メタデータ、読み込み、評価、結果の作り方 |
| [AWS アクセス](Architecture-AWS-Access.ja.md) | SDK、認証情報、リージョン、コレクター、読み取り専用の強制、IAM 権限 |
| [CLI インターフェース](Architecture-CLI-Interface.ja.md) | コマンド、オプション、終了コード |
| [出力形式](Architecture-Output-Formats.ja.md) | table / JSON / SARIF |
| [テスト方針](Architecture-Testing-Strategy.ja.md) | フィクスチャ、モック、実 AWS に接続しないための対策 |
| [エラー処理とログ](Architecture-Error-Handling-and-Logging.ja.md) | エラーの型と伝播、部分的な失敗の扱い、ログ |
| [初期依存クレート](Architecture-Initial-Dependencies.ja.md) | 初期に採用する依存クレートの候補、理由、ライセンス |

## ADR

| ADR | 結論 |
| --- | --- |
| [ポリシーエンジン: Rego(regorus)](../adr/ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md) | ルールは Rego v1 で書き、純 Rust 実装の regorus で評価する |
| [ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) | ルールは外部のリポジトリで配布し、実行時に読み込む。階層は自由で、1 ルール 1 ファイル(`<ルール名>.rego` + `<ルール名>.fixtures/`)。ID は `<マニフェストの name>.<ディレクトリ>.<ルール名>` でバージョンを含めない。中央の一覧を持たず、互換性はルートのマニフェスト(`yata-policies.json`)で確かめる |
| [コレクターのビルド時自動検出](../adr/ADR-0001-Collector-Discovery-via-Build-Script.ja.md) | コレクターは 1 リソース種別 1 ファイルとし、build.rs がモジュール宣言と一覧を生成する |
| [AWS SDK for Rust と読み取り専用ガード](../adr/ADR-0001-AWS-SDK-and-Read-Only-Guard.ja.md) | 公式の AWS SDK for Rust を使い、Describe / List / Get 以外の操作を実行時に拒否する |

## 個別機能の設計との関係

- 個別機能(ルールの追加、コレクターの追加など)の設計は `docs/features/Feature-<Issue番号4桁>-<概要>.ja.md` に書き、この全体設計に従う
- 全体設計の変更が必要になった場合は、機能の Issue とは別の Issue・PR で先に main へ入れる
- 典型的な機能追加(新しいチェック)で、このリポジトリで触るのはコレクター 1 ファイルと設計書 1 ファイルだけになるようにしている。ルール 1 ファイル(とフィクスチャ)は外部のポリシーのリポジトリに追加する。詳細は [ワークスペース構成](Architecture-Workspace-Layout.ja.md) の「並列開発で衝突しないための規則」を参照
