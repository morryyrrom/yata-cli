# エラー処理とログ

> This Japanese version is the authoritative source. The English version (`Architecture-Error-Handling-and-Logging.md`) has not been written yet.

エラーの型と伝播、部分的な失敗の扱い、ログの方針を定めます。入口は [アーキテクチャ概要](Architecture-Overview.ja.md) です。

## エラーの型と伝播

- テスト以外のコードで `unwrap()` / `expect()` を使わない。エラーは `Result` と `?` で伝播する(clippy の `unwrap_used` / `expect_used` を `deny` にして強制する)
- パニックを前提にした処理(配列の範囲外アクセスなど)を避け、`get()` などで扱う
- ライブラリのクレート(`yata-core`、`yata-provider-aws`、`yata-engine`、`yata-report`)は、クレートごとに `thiserror` でエラーの enum を定義する
  - enum には `#[non_exhaustive]` を付ける
  - 元のエラーは `#[source]` で保持し、原因をたどれるようにする
- `yata-cli` はライブラリのエラーを受け取り、終了コードに対応づける。`anyhow` は使わない(終了コードの判定に型が必要なため)
- エラーメッセージは英語で、小文字で始め、末尾に句点を付けない(Rust の慣例)

### 主なエラー

| クレート | エラー | 主な内容 | 終了コード |
| --- | --- | --- | --- |
| `yata-cli` | (ポリシーのディレクトリの決定) | `--policy-dir` も `YATA_POLICY_DIR` も無い | 2 |
| `yata-cli` | (ルールの絞り込み) | `--rule` などで絞り込んだ結果が 0 件 | 2 |
| `yata-cli` | (プロバイダの対応の検査) | 絞り込んだ後のルールが、対応していないリソース種別を宣言している | 2 |
| `yata-cli` | (オプションの検査) | 同時に指定できないオプション(`--snapshot` と `--profile` など)、`collect` の `--output` が無い | 2 |
| `yata-cli` | (スナップショットの書き込み) | 出力先がすでにある(`--overwrite` なし)、`--overwrite` の対象がスナップショットでない | 2 |
| `yata-cli` | (スナップショットの書き込み) | 書き込みの失敗(一時ディレクトリを削除し、既存の出力先は変えない) | 3 |
| `yata-engine` | `PolicyLoadError` | ポリシーのディレクトリが見つからない・読めない、マニフェストが無い・形式の誤り(`name` の文字、`min_yata_version` が `x.y.z` でないを含む)・`format_version` が対応外・`min_yata_version` が新しすぎる・`name` の重複、ライブラリの誤り、ファイルの大きさ・ファイル数・ディレクトリ数・深さの上限を超えた、走査中のシンボリックリンク・reparse point、名前の文字の規則(`[a-z][a-z0-9_]*`、Rego の予約語を除く)に合わない名前、Rego の構文エラー、メタデータの誤り(種別の各部分が名前の文字の規則に合わない、`related_types` にほかのプロバイダの種別があるを含む)、パッケージ名とパスの不一致、ルールが 1 つも無い | 2(`rules test` ではルール単位のものを集めて 1。[ポリシー評価](Architecture-Policy-Evaluation.ja.md) の「失敗の種類と終了コード」) |
| `yata-engine` | `EvaluationError` | 評価の失敗、時間切れ、`fail` の要素の形の不正 | ルールの状態を `error` にして続行(4) |
| `yata-provider-aws` | `SetupError` | 認証情報が無い、`sts:GetCallerIdentity` の失敗、`aws` 以外のパーティション、リージョンの取得の失敗 | 3 |
| `yata-provider-aws` | `CollectError` | 1 つのアカウント × リージョン × リソース種別の収集の失敗(リソースを列挙する操作の失敗) | `CollectionError` として記録して続行(4) |
| `yata-provider-aws` | (構成情報の失敗の印) | 1 つのリソースの付随情報を取る操作の失敗 | 構成情報に `yata_error` を置いて続行。使うルールは `incomplete`(4) |
| `yata-core` | `SnapshotError` | スナップショットの読み込み・検証の誤り(マニフェスト、`schema_version`、中身との不一致、組の過不足、複数アカウント、上限、シンボリックリンク) | 2 |
| `yata-report` | `ReportError` | 出力の書き込みの失敗 | 3 |

## 部分的な失敗の扱い

スキャンは、一部が失敗しても、できる範囲で最後まで行います。

- 収集: アカウント × リージョン × リソース種別の単位で失敗を記録し、ほかは続行する
  - 記録する項目: アカウントキー、リージョン、リソース種別、エラーの種類、メッセージ([リソースモデル](Architecture-Resource-Model.ja.md) の `CollectionError`)
  - エラーの種類: `access_denied`(権限不足)、`throttled`(再試行しても制限された)、`region_unavailable`(リージョンに接続できない)、`timeout`、`service`(そのほかのサービスのエラー)、`invalid_response`(応答を解釈できない)
  - 組全体の失敗にするのは、リソースを列挙する操作(`ListBuckets` など)が失敗した場合。このときは `ResourceSet` を作らない
  - 1 つのリソースの付随情報を取る操作(例: バケットごとの `GetBucketPolicy`)だけが失敗した場合は、組は成功として扱い、そのリソースの構成情報のその操作のキーに失敗の印(`{"yata_error": {"kind": ..., "message": ...}}`)を置く。ほかのリソースは評価できるようにするため([リソースモデル](Architecture-Resource-Model.ja.md) の「操作ごとの取得の失敗」)
    - 失敗の印を持つリソースを使うルールは、状態が `incomplete` になる(スキャンは不完全で終了コード 4)。黙って `pass` だけを出さない
  - API が「設定が存在しない」ことを示すエラー(例: パブリックアクセスブロックの設定が無い)を返した場合は失敗ではなく、構成情報のその操作のキーの値を `null` にする。どのエラーがこれにあたるかは各コレクターの機能設計書で決める
- 評価: ルール単位で失敗を記録し、ほかのルールは続行する(ルールの状態は [ポリシー評価](Architecture-Policy-Evaluation.ja.md) を参照)
- 失敗はレポート(JSON の `collection_errors`、ルールの `status` と `incomplete_resources`、SARIF の `toolExecutionNotifications`、table の末尾)と、標準エラー出力の警告の両方に出す
- 終了コードは [CLI インターフェース](Architecture-CLI-Interface.ja.md) の規則に従う

## ログ

- `tracing` でログを出し、`tracing-subscriber` で標準エラー出力に書く
- 指定は「オプション > 環境変数 > 既定値」の規則に従う([CLI インターフェース](Architecture-CLI-Interface.ja.md) の「オプションの優先度」)
  - `-v` で `info`、`-vv` で `debug`、`-vvv` で `trace`、`-q` で `error` だけ。`-v` / `-q` を指定したら `YATA_LOG` は使わない
  - `-v` / `-q` が無ければ、環境変数 `YATA_LOG`(`tracing-subscriber` の `EnvFilter` の書式)を使う
  - どちらも無ければ `warn`
- 依存クレート(AWS SDK、regorus など)のログは、`YATA_LOG` で明示しない限り `warn` 以上だけを出す(`-v` / `-q` は `yata` 自身のクレートのレベルだけを変える)
- ログの書式は人が読むテキスト。メッセージは英語

### ログに書くもの・書かないもの

| レベル | 内容 |
| --- | --- |
| `error` | 処理を続けられないエラー |
| `warn` | 収集・評価の部分的な失敗、非推奨のオプション |
| `info` | 処理の段階(ルールの読み込み件数、対象リージョン、収集・評価の件数と所要時間) |
| `debug` | コレクター・ルールごとの開始と終了、リソースの ID |
| `trace` | 詳細な調査用の情報 |

- 認証情報(アクセスキー、シークレット、セッショントークン)は、どのレベルでもログに書かない
- リソースの構成情報の値と、検出の `actual` / `expected` はログに書かない(レポートとスナップショットにだけ含める)
- `debug` 以上ではアカウント ID や ARN が出る。ログを共有するときは注意する旨を、利用者ガイドに書く

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| ログの JSON 出力 | 機械処理用の書式を設けるか | 別 Issue |
| エラーの種類の判定方法 | SDK のエラーから `access_denied` などをどう判定するか(エラーコードの対応表) | AWS プロバイダを実装する Issue |
