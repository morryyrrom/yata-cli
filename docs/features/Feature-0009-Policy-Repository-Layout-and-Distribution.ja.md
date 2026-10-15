# ポリシーのリポジトリの構成と配布

> This Japanese version is the authoritative source. The English version (`Feature-0009-Policy-Repository-Layout-and-Distribution.md`) has not been written yet.

公式のポリシーのリポジトリ `yata-rules`(<https://github.com/morryyrrom/yata-rules>)の構成、マニフェストの初期値とバージョンの上げ方、配布の方法、`yata` からの指定のしかた、リリースの手順、CI、初期のファイルを決めます。

ポリシーのディレクトリの形式(階層、ルールとフィクスチャの配置、ルール ID、ライブラリ、マニフェストの項目、走査、資源の上限)は [ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) と [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) で決まっています。ルールの書式は [ADR: ルールをリソースごとに評価する](../adr/ADR-0030-Per-Resource-Rule-Evaluation.ja.md)、`yata rules test` の検査と終了コードは [ポリシーエンジンと `yata rules test`](Feature-0008-Policy-Engine-and-Rules-Test.ja.md)、最初のルール群は [最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) に従います。この設計書はそれらを繰り返さず、公式のポリシーのリポジトリとしての使い方を決めます。

## 目的と背景

- ルールは `yata` のバイナリにも yata-cli のリポジトリにも含めず、別のリポジトリで配布して実行時に読み込む(ADR-0001)。その配布先として `yata-rules` を作ったが、まだ中身が無い
- ルールを書き始める(最初のルール群の 6a〜6e)前に、リポジトリの構成と配布の方法を決める必要がある
- 全体設計で「ポリシーのリポジトリの配布方法を決める Issue」「ポリシーのリポジトリを作る Issue」で決めるとしていた次の事項を、この設計で決める
  - ポリシーのディレクトリの既定の場所([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md)・[CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) の未決事項)
  - ポリシーの取得・更新を `yata` が行うか([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の未決事項)
  - ポリシーの署名・改ざんの確認(同、未決事項(安全性))
  - ポリシーのリポジトリの構成(名前、CI、リリースの方法、CI で使う `yata` の取得方法)(同、未決事項)
  - ポリシーのディレクトリの既定の場所を決めるクレート([初期依存クレート](../architecture/Architecture-Initial-Dependencies.ja.md) の未決事項)
- 受け入れ条件: この設計書から、ほかに決めることなく `yata-rules` の初期構成を作れること

## スコープ

- `yata-rules` のディレクトリの構成と、ルールを置くディレクトリの決め方
- マニフェスト `yata-policies.json` の初期値と、`version`・`min_yata_version` の上げ方
- 利用者の取得の方法(リリースのアーカイブ、git clone)と、`yata` からの指定のしかた
- リリースの手順(タグ、配布物、チェックサム、CHANGELOG、リリースノート)
- `yata-rules` の CI(`yata rules test`、Rego のフォーマット・リント)と、CI で使う `yata` の入手方法
- 初期のファイル(README、LICENSE、CONTRIBUTING、CHANGELOG、マニフェスト、ワークフロー、`.gitignore` など)と、リポジトリの設定
- 全体設計・ADR に必要な変更の洗い出し(変更はこの設計の PR より先に、別の PR で行う。全体設計・ADR からこの設計書へのリンクの追加は、この設計の PR で行う)

## スコープ外

- ルール本体とそのフィクスチャ(最初のルール群の 6a〜6e)
- `yata rules test` の実装(Feature-0008)
- yata-cli 自身のリリース(配布物の名前、チェックサム、バイナリの対象のプラットフォーム)。この設計は、その決定に依存する部分を未決事項として残す
- `yata` がポリシーをネットワークから取得する機能(作らない。下の「`yata` からの指定」)
- 利用者ガイド(`docs/guides/`)の作成。内容は下の「利用者の取得と指定」で決め、ページは後続の Issue で作る

## 決定の要約

| 項目 | 決定 |
| --- | --- |
| マニフェストの `name` | `yata_aws`(ルール ID は `yata_aws.<サービス>.<ルール名>`) |
| ディレクトリ | ルートの直下にサービスのディレクトリ(`iam/`、`s3/`、`cloudtrail/`、`monitoring/`、`ec2/` など)と `lib/`。ルールはサービスのディレクトリの直下に置く(深さ 2) |
| `version` | SemVer の `x.y.z`(プレリリースの部分は使えない)。下の「バージョンの上げ方」の表で決める |
| 配布 | タグごとの GitHub Release に、`yata-rules-<version>.tar.gz`、`yata-rules-<version>.zip`、`SHA256SUMS` を置く。git clone でタグを checkout してもよい |
| 改ざんの確認 | リポジトリで immutable releases を有効にし、利用者は `SHA256SUMS` と `gh release verify-asset` で確かめる。`yata` は確かめない |
| `yata` からの指定 | `--policy-dir` → `YATA_POLICY_DIR`。既定の場所は設けない。`yata` はポリシーを取得しない |
| CI | `opa fmt`(フォーマット)、Regal(リント)、`yata rules test`(regorus での読み込みとフィクスチャ) |
| CI の `yata` | yata-cli のリリースが無い間は、ワークフローで固定した yata-cli のコミットからビルドする。リリースが出た後は、`min_yata_version` と最新のリリースのバイナリを使う |
| 変更の記録 | PR ごとに `changelog.d/` に断片のファイルを置き、リリースの PR で `CHANGELOG.md` にまとめる |
| リリース | リリースの PR(`version` の更新と断片のまとめ)→ main へのマージ → タグ `v<version>` の push → ワークフローが下書きの Release を作り、配布物を付けて公開する |

## リポジトリの構成

### ディレクトリ

```
yata-rules/
├── yata-policies.json          # マニフェスト
├── README.md
├── CONTRIBUTING.md
├── CHANGELOG.md
├── changelog.d/                # 次のリリースに入る変更の断片(下の「CHANGELOG」)
│   ├── README.md               # 断片の書き方(リリースの PR で消さない)
│   └── <Issue番号>-<概要>.md
├── LICENSE-MIT
├── LICENSE-APACHE
├── .gitattributes
├── .gitignore
├── .regal/
│   └── config.yaml             # Regal の設定
├── .github/
│   ├── actions/
│   │   └── setup-yata/
│   │       └── action.yml      # CI とリリースで共通の `yata` の用意(composite action)
│   └── workflows/
│       ├── ci.yml
│       └── release.yml
├── lib/                        # 共通の関数のライブラリ(最初のライブラリの PR で作る)
│   └── <topic>.rego            # package yata.lib.yata_aws.<topic>
├── iam/
│   ├── <rule>.rego             # package yata.rules.yata_aws.iam.<rule>
│   └── <rule>.fixtures/
│       ├── pass_<case>.json
│       └── fail_<case>.json
├── s3/
├── cloudtrail/
├── monitoring/
└── ec2/
```

- リポジトリのルートが、そのままポリシーのディレクトリになる(`--policy-dir` に指定するディレクトリ)
- `.github/`、`.regal/` は `.` で始まるので `yata` は走査しない。README などの `.rego` 以外のファイルは無視される(全体設計の「走査」)。このため、開発用のファイルをルートに置いても読み込みに影響しない
- 空のディレクトリは Git に残らないため、`lib/` とサービスのディレクトリは、最初のファイルを足す PR で作る。初期構成には含めない
- 初期構成には `docs/` を置かない。ルールの設計(どのコントロールをどう判定するか)をどこに書くかは、この設計では決めない(未決事項)
- `changelog.d/` は `.` で始まらないので `yata` が走査するが、中身は `.md` だけで `.rego` が無いため、ルールにもライブラリにもならない(ファイル数の上限には数えられる)

### ルールを置くディレクトリの決め方

- ディレクトリは、ルールが確かめる設定を持つ AWS のサービスとし、名前はリソース種別の `<service>` の部分(`aws-sdk-<service>` の `<service>`)に合わせる
  - 通常は `target_type` の `<service>`(例: `aws.iam.user` → `iam/`、`aws.ec2.vpc` → `ec2/`)
  - `target_type` が `aws.account.account` のルールは、判定の主な対象の `related_types` のサービス(例: CIS 4.1 → `cloudtrail/`)
  - S3 Control(`aws.s3control.*`)を確かめるルールは `s3/` に置く(利用者から見て S3 の設定のため)
- 例外として、CloudTrail・CloudWatch Logs・CloudWatch・SNS の連なりで判定する監視のルール(CIS 5.x)は `monitoring/` に置く
- ベンチマークの章(CIS のセクション番号)ではなくサービスで分ける。ルール ID をベンチマークの版や章立てから独立させ、同じルールを複数の基準から参照できるようにするため
- サービスのディレクトリの下に、さらに階層は作らない(深さ 2)。ルール ID を短く保つため。1 つのサービスのルールが非常に多くなった場合の分け方は、その時点で決める
- ルール ID は `yata_aws.<サービス>.<ルール名>` になる(例: `yata_aws.s3.bucket_public_access_block`)。パッケージ名は `yata.rules.yata_aws.s3.bucket_public_access_block`

### ルール名

- 名前の文字の規則(`[a-z][a-z0-9_]*`、予約語を除く)に従う(全体設計の「名前の規則」)
- `<リソース>_<望ましい状態>` の形で、確かめる内容を表す(例: `bucket_public_access_block`、`user_mfa_enabled`、`password_policy_min_length`)
- CIS のコントロールの番号を名前に入れない。番号はメタデータの `references` に書く(全体設計の「基準の参照」)
- 公開した後は、ファイルの移動・改名をしない(ルール ID が変わり、互換性の無い変更になる。下の「バージョンの上げ方」)

### ライブラリ

- 使い方は全体設計の「ライブラリ」のとおり(1 ファイル 1 パッケージ、話題ごとにファイルを分ける、既存の関数の意味を変えない、`lib/` の変更は単独の PR)
- パッケージ名は `yata.lib.yata_aws.<topic>`
- 最初に置くことが見込まれる話題: 取得の失敗の印の判定、IAM のポリシー文書の解析、CIDR とポートの判定、監視の連なりの判定([最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の後続の Issue 6a〜6e)。作るかどうか・ファイル名は、それぞれのライブラリの PR で決める
- `lib/` はほかのポリシーのディレクトリから使えない(全体設計)。利用者が独自のルールで同じ関数を使いたい場合は、自分のポリシーのディレクトリに写す

## マニフェスト

### 初期値

```json
{
  "format_version": 1,
  "name": "yata_aws",
  "version": "0.1.0",
  "min_yata_version": "0.1.0"
}
```

| キー | 値 | 理由 |
| --- | --- | --- |
| `format_version` | `1` | `yata` が対応する唯一の形式 |
| `name` | `yata_aws` | 公式のセットであること(`yata`)と、AWS のルールのセットであること(`aws`)が ID から分かる。利用者が独自のセットに付ける名前(`my_policies` など)と区別しやすい。将来ほかのプロバイダの公式のセットを作る場合に、`yata_<provider>` の形で名前が衝突しない |
| `version` | `0.1.0` | 最初のリリースの版。最初のリリースの PR では `version` を変えない(`0.1.0` のまま出す) |
| `min_yata_version` | `0.1.0` | 最初のルール群を評価できる最初の `yata` のリリース。yata-cli の最初のリリースの版が `0.1.0` でなければ、`yata-rules` の最初のリリースの PR でその版に合わせる |

- `name` を変えるとすべてのルール ID が変わる。公開した後は変えない
- `version` は `x.y.z` だけで、プレリリースの部分(`-rc.1` など)は書けない(マニフェストの検証が `x.y.z` の形に限るため。Feature-0008 の「マニフェスト」)。このため、リリースの候補版は作らない
- main の `version` は、2 回目以降のリリースの PR で次の版に上げるまで、直前のリリースの版のままにする。リリースの前の main を使うと、レポートの `policies[].version` は直前のリリースの版になり、中身と一致しない。main をそのまま使わないことを README に書く(下の「利用者の取得と指定」)

### バージョンの上げ方

図: [版の上げ方の判定](../diagrams/Diagram-0009-Version-Bump-Decision.md)

`version` は SemVer 2.0.0 に従います。利用者にとっての互換性は、「同じ設定(`--rule` などの指定、ルール ID による抑制、SARIF のアラートの追跡、結果の比較)と同じ `yata` で、更新後も使い続けられるか」で判断します。

変更の種類ごとに、次の表で上げる部分を決めます。1 つのリリースに複数の種類の変更があれば、最も大きいものに合わせます。

| 変更 | 1.0.0 以降 | 0.y.z の間 |
| --- | --- | --- |
| ルールの削除 | major | y |
| ルール ID の変更(ファイルの移動・改名、マニフェストの `name` の変更) | major | y |
| ルールの `target_type` の変更(結果の対象のリソースが変わる) | major | y |
| `format_version` を上げる(古い `yata` では読めなくなる) | major | y |
| ルールの追加 | minor | z |
| `min_yata_version` を上げる | minor | z |
| ルールの確かめる内容の変更(条件の追加・削除などで `description` が変わるもの) | minor | z |
| メタデータの `severity`・`tags` の変更(`--fail-on`・`--min-severity`・絞り込みの結果が変わる) | minor | z |
| 判定の誤りの修正(`description` のとおりに判定するよう直す。新しい `fail` が出ることを含む) | patch | z |
| `message`・`reason` の変更 | patch | z |
| メタデータの `title`・`description`(意味を変えない書き直し)・`remediation`・`references` の変更 | patch | z |
| `lib/` の変更(どのルールの判定も変えないもの) | patch | z |
| フィクスチャ、README、CONTRIBUTING、CI だけの変更 | 単独ではリリースしない。リリースする場合は patch | 同じ |

- 0.y.z の間は、Cargo の慣習に合わせ、major に当たる変更で y を(z は 0 に戻す)、それ以外で z を上げる。1.0.0 にする時期は未決事項
- `min_yata_version` を上げることは、古い `yata` の利用者が更新後の版を使えなくなる(終了コード 2)が、`yata` を更新すれば同じ設定で使い続けられるため minor とする。新しいリソース種別を使うルールの追加のたびに上がるため、major にすると major が頻繁に上がり、互換性の無い変更の印として機能しなくなる
- `format_version` を上げることは、ポリシーの形式そのものの変更で、ルールの書き直しを伴いうるため major とする
- 判定の誤りの修正で新しい `fail` が出る場合は、CHANGELOG の `Fixed` に、ルール ID と「新しい検出が出うる」ことを書く
- ルールを削除する・ID を変える場合は、1.0.0 以降は、少なくとも 1 つ前のリリースの CHANGELOG の `Deprecated` で告知する。0.y.z の間は告知を必須にしない(`Removed` には必ず書く)
- 確かめる内容を大きく変える場合(別のチェックと言える場合)は、内容を変えずに、新しいルールを足して古いルールを削除する(major)
- 保守するのは最新のリリースの系列だけとする。古い major の系列に修正を戻さない

### `min_yata_version` と `yata` のリリースの関係

図: [yata-cli と yata-rules のリリース順序](../diagrams/Diagram-0009-Release-Order-Sequence.md)

- `min_yata_version` は、そのポリシーのセットのすべてのルールを読み込み・評価できる、最も古い `yata` のリリースの版にする
- 次のときに上げる
  - 新しいリソース種別を使うルールを足すとき(その種別のコレクターを含む `yata` の版に)
  - その `yata` で新しく有効になった組み込み関数を使うとき
  - `format_version` を上げるとき(その形式に対応した `yata` の版に)
  - 判定に関わる新しいメタデータのキーなど、その `yata` で解釈されるキーの効果を必須にするとき(全体設計の「互換性の規則」)
- 新しい `yata` がリリースされたというだけでは上げない
- 下げない(古い `yata` で確かめ直す手間に見合わないため)
- 順序: yata-cli で新しいコレクターを含む版をリリース → `yata-rules` で `min_yata_version` をその版に上げ、その種別を使うルールを main に入れる → `yata-rules` をリリースする。ルールを先に書き始めたい場合は、下の「CI で使う `yata`」のソースからのビルドを使う
- 各リリースの CHANGELOG とリリースノートに、必要な `yata` の版を書く(下の「CHANGELOG」)

## 利用者の取得と指定

図: [利用者の取得から指定まで](../diagrams/Diagram-0009-Policy-Acquisition-and-Selection.md)

### 取得の方法

正式な配布物は GitHub Release のアーカイブです。git clone でタグを checkout してもかまいません。

**リリースのアーカイブ(推奨)**

```sh
VERSION=0.1.0
BASE=https://github.com/morryyrrom/yata-rules/releases/download/v${VERSION}
curl -fLO "${BASE}/yata-rules-${VERSION}.tar.gz"
curl -fLO "${BASE}/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
tar -xzf "yata-rules-${VERSION}.tar.gz"
# 任意: 改ざんの確認(GitHub CLI)
gh release verify-asset "v${VERSION}" "yata-rules-${VERSION}.tar.gz" --repo morryyrrom/yata-rules
```

- macOS では `sha256sum` の代わりに `shasum -a 256 --check --ignore-missing SHA256SUMS` を使う
- Windows では `yata-rules-<version>.zip` を使い、`Get-FileHash -Algorithm SHA256` の値を `SHA256SUMS` と比べる
- GitHub がリリースごとに自動で付ける「Source code」のアーカイブは使わない(`SHA256SUMS` と `gh release verify-asset` で確かめられないため)

**git clone**

```sh
git clone --branch v0.1.0 --depth 1 https://github.com/morryyrrom/yata-rules.git
```

- 必ずリリースのタグを指定する。main はリリースの前のルールを含み、まだリリースされていない `yata` を必要とすることがある(その場合は終了コード 2)。また `version` が中身と一致しない(上の「初期値」)
- 更新は `git fetch --tags` と `git checkout v<version>`

### `yata` からの指定

```sh
yata scan --policy-dir ./yata-rules-0.1.0
# または
export YATA_POLICY_DIR="$PWD/yata-rules-0.1.0"
yata scan
```

- `--policy-dir` → `YATA_POLICY_DIR` の順で決める(全体設計の「ポリシーのディレクトリの決定」)。どちらも無ければ終了コード 2
- **既定の場所は設けない**。理由
  - `yata` はポリシーを取得しないため、決まった場所にポリシーを置くものが無い。既定の場所を設けても、利用者が自分でそこに置く手順が増えるだけになる
  - 版ごとに別のディレクトリに展開し、指定を切り替える使い方(更新と戻しが簡単)と相性がよい。既定の場所があると、どの版が使われたかが指定から分からなくなる
  - パスを決めるクレート(`directories` など)を追加しなくて済む
  - 後から既定の場所を足しても、既存の指定のしかたは変わらない(互換性を壊さない)。必要になった時点で改めて検討できる
- **`yata` はポリシーを取得しない**(ダウンロードのコマンドを作らない)。ネットワークへのアクセス、取得先の信頼、改ざんの確認を `yata` に持ち込まないため。ADR-0001 の「ネットワークからポリシーを取得しない」を維持する
- 互換性の無い組み合わせは、`yata` が読み込みの最初に終了コード 2 で知らせる(`min_yata_version` が新しすぎる → `yata` を更新するか、古い版のポリシーを使う。`format_version` が対応外 → 同じ)。全体設計の「マニフェスト」

### 更新

- 新しい版を別のディレクトリに展開し、`--policy-dir` / `YATA_POLICY_DIR` を切り替える。問題があれば前の版に戻せる
- 更新の前に CHANGELOG を読み、ルール ID の変更・削除(major)と、`min_yata_version` の変更を確かめる

### 独自のルール

- 公式のセットのディレクトリにファイルを足さない。更新で失われ、`policies[].version` が中身と一致しなくなる
- 独自のルールは別のディレクトリに、別の `name`(例: `my_policies`)のマニフェストを置いて作り、`--policy-dir` を複数指定する(全体設計の「マニフェスト」)
- 公式のセットを改変して使う(フォークする)場合は、`name` を変える。`yata_aws` のまま改変すると、レポートの名前と版から公式のセットと区別できなくなる

## 改ざんの確認

- `yata` はポリシーの署名・出所を確かめない(全体設計の「(a) 信頼の前提」のとおり)。確かめるのは利用者で、そのための手段をリリースで用意する
- **`SHA256SUMS`**: 配布物の SHA-256 を `sha256sum` の形式(`<ハッシュ>  <ファイル名>`)で書く。ダウンロードの破損を見つけられる。同じ Release に置くため、Release そのものの改ざんは防げない
- **immutable releases**: `yata-rules` のリポジトリの設定で immutable releases を有効にする。公開した Release の配布物は追加・変更・削除できず、タグも移動・削除できなくなる。公開時に GitHub が Release の attestation(タグ、コミット、配布物を含む署名付きの記録)を作り、利用者は `gh release verify` と `gh release verify-asset` で確かめられる
- これ以上の署名(独自の鍵での署名など)は設けない。必要になれば未決事項として扱う
- 誤った Release を公開した場合は、同じタグを作り直さず(immutable releases ではできない)、patch を上げて新しいリリースを出す

## リリースの手順

### タグと配布物

| 項目 | 内容 |
| --- | --- |
| タグ | `v<version>`(例: `v0.1.0`)。注釈付きタグ。マニフェストの `version` と一致させる |
| Release の題名 | `yata-rules <version>` |
| 配布物 | `yata-rules-<version>.tar.gz`、`yata-rules-<version>.zip`、`SHA256SUMS` |
| アーカイブの中身 | 最上位のディレクトリ `yata-rules-<version>/` の下に、タグのコミットのファイルのうち `export-ignore` のものを除いたすべて(マニフェスト、`lib/`、ルール、フィクスチャ、README、CONTRIBUTING、CHANGELOG、LICENSE-MIT、LICENSE-APACHE) |
| 除くもの | `.github/`、`.regal/`、`.gitattributes`、`.gitignore`、`changelog.d/`(`.gitattributes` の `export-ignore` で指定する。`changelog.d/` はリリースの PR で `CHANGELOG.md` にまとめた後は書き方の README しか残らず、利用者に要らないため) |
| 作り方 | `git archive --format=tar.gz --prefix=yata-rules-<version>/` と `git archive --format=zip --prefix=yata-rules-<version>/` |
| リリースノート | CHANGELOG のその版の節 |

- フィクスチャを含めるのは、利用者が取得したセットに `yata rules test` を実行して確かめたり、改変したルールを確かめたりできるようにするため。フィクスチャは `yata scan` などでは読まれない
- アーカイブは `git archive` で作り、作業ツリーの未追跡のファイルが混ざらないようにする

### 手順

図: [リリースの手順](../diagrams/Diagram-0009-Release-Sequence.md)

1. **リリースの PR**: main から作り、次を 1 つの PR で行う
   - `yata-policies.json` の `version` を上げる(上の「バージョンの上げ方」)。最初のリリースでは変えない(`0.1.0` のまま)
   - `min_yata_version` が正しいことを確かめる(上の「`min_yata_version` と `yata` のリリースの関係」)
   - `changelog.d/` の断片(`README.md` を除く)を `CHANGELOG.md` の新しい節 `## [<version>] - <YYYY-MM-DD>` にまとめ、まとめた断片のファイルを削除する(下の「CHANGELOG」)。版は、まとめた断片の変更の種類から決める
   - CI のワークフローの `YATA_CLI_REF` が空であること(リリースした `yata` で確かめる。下の「CI で使う `yata`」)
2. **マージ**: CI が通ったら main にマージする(Merge commit)
3. **タグ**: メンテナーがマージのコミットに注釈付きタグを付けて push する
   ```sh
   git tag -a v0.1.0 -m "yata-rules 0.1.0"
   git push origin v0.1.0
   ```
4. **リリースのワークフロー**(`release.yml`)が、タグの push で動き、検査・配布物の作成・Release の公開を行う(下の「`release.yml`」)
5. 失敗した場合は、Release は下書きのまま残るか作られない。原因を直す PR を出し、まだ公開していなければ同じ版で、タグを付け直して(下書きの Release とタグを削除して)やり直す。公開した後なら patch を上げる

- リリースの時期は決めない(必要に応じて出す)
- `yata-rules` の最初のリリースは、yata-cli の最初のリリースの後に行う(リリースした `yata` で確かめるため)

### CHANGELOG

- 形式は Keep a Changelog 1.1.0 とし、節は `Added`、`Changed`、`Deprecated`、`Removed`、`Fixed`、`Security` を使う
- 各版の節の最初の行に、必要な `yata` の版を書く(例: `Requires yata >= 0.1.0.`)
- ルールに関する項目には、必ずルール ID を書く(例: `` - `yata_aws.iam.user_mfa_enabled`: ... ``)。利用者が自分の設定(`--rule`、抑制)への影響を探せるようにするため
- ルール ID が変わる場合は、古い ID と新しい ID の両方を書く
- `CHANGELOG.md` はリリースの PR でだけ書き換える。`Unreleased` の節は置かない(次のリリースに入る変更は `changelog.d/` の断片で表す)
- 初期の内容

```markdown
# Changelog

All notable changes to this policy set are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
See the versioning policy in README.md for what counts as a breaking change.
Changes that are not released yet are in the changelog.d/ directory.
```

#### 変更の断片(`changelog.d/`)

ルールの PR は並行して出るため、各 PR が `CHANGELOG.md` の同じ節に追記すると衝突します。PR ごとに別のファイル(断片)を置き、リリースの PR でまとめます。

- **置く PR**: `lib/` の外の `.rego`(ルール)を追加・変更・削除する PR、`yata-policies.json` を変える PR、判定を変える `lib/` の変更の PR。フィクスチャ・CI・文書だけの変更と、判定を変えない `lib/` の変更は置かなくてよい
- **名前**: `changelog.d/<Issue番号>-<概要>.md`(例: `changelog.d/12-iam-user-mfa.md`)。`<概要>` は英小文字・数字・`-` だけ。1 つの PR に 1 つの断片。Issue の無い PR は `<概要>.md`。名前を Issue 番号と概要にするのは、並行する PR で名前が重ならないようにするため
- **中身**: Keep a Changelog の節の見出し(`### Added`、`### Changed`、`### Deprecated`、`### Removed`、`### Fixed`、`### Security` のうち使うものだけ)と、その下の箇条。箇条にはルール ID を書く(上の規則)。`min_yata_version` を上げる PR は `### Changed` に `Requires yata >= <版>.` を書く

```markdown
### Added

- `yata_aws.iam.user_mfa_enabled`: IAM users with a console password must have MFA enabled.
```

- **まとめ方**(リリースの PR): 全断片の箇条を節ごとに集め、上の節の順に並べて新しい版の節にする。節の最初の行に、必要な `yata` の版(`min_yata_version`)を書く。まとめた断片を削除する。手で行う(断片の数が少なく、専用のツールを入れるほどではないため)
- **`changelog.d/README.md`**: 上の名前と中身の規則を英語で書いておく。リリースの PR で削除しない
- **CI での確認**: `ci.yml` の `changelog` ジョブ(下の「`ci.yml`」)で、断片の形式と、断片が要る PR に断片があることを確かめる

## CI

### 使うツール

| ツール | 用途 | ライセンス | 選んだ理由 |
| --- | --- | --- | --- |
| `yata rules test` | マニフェスト・読み込み(regorus での構文と解析)・メタデータ・フィクスチャの検査と実行 | MIT OR Apache-2.0 | ルールを実際に評価する regorus で確かめる唯一の方法。regorus が対応しない構文・組み込み関数の検出は、これを正とする |
| OPA(`opa fmt`) | フォーマットの検査 | Apache-2.0 | Rego の標準のフォーマッター。Regal の `opa-fmt` の規則も同じフォーマッターを使う |
| Regal(`regal lint`) | リント(誤りやすい書き方、慣用の書き方、スタイル) | Apache-2.0 | OPA のプロジェクトの Rego 用のリンター。GitHub の注釈の形式(`--format github`)で出力できる |

- どれも CI と開発者の手元で使うツールで、yata-cli の依存クレートにも配布物にも含めない
- OPA と Regal は、それぞれの公式の GitHub Action(`open-policy-agent/setup-opa`、`open-policy-agent/setup-regal`)で入れ、`version` に作成時点の最新の安定版の `x.y.z` を書いて固定する。更新は単独の PR で行う(Regal の新しい規則で既存のルールが失敗しうるため)

### regorus との互換性

- `opa fmt` と Regal は OPA の Rego の実装(Go)で構文を解析する。regorus は OPA の構文(Rego v1)にほぼ適合しているが、一部の組み込み関数を実装しておらず(JWT、暗号、`net.cidr_merge` など。regorus の README)、`yata` は一部の機能(`http.send`、`uuid.*`、`yaml.*`、`json.match_schema` など)を有効にしていない(Feature-0008 の「regorus の cargo feature」)。OPA と Regal はこれらを使うルールを誤りとして報告しない
- このため、regorus での互換性は `yata rules test` で確かめる。ルールの読み込みでパッケージ・メタデータ・`fail` を評価し、フィクスチャで `fail`・`message`・`reason` を評価するため、評価された経路の対応していない組み込み関数はここで見つかる
- フィクスチャで通らない経路(`else` の一部の枝など)にある組み込み関数は見つからない。CONTRIBUTING で、使える組み込み関数を Feature-0008 の一覧の範囲に限ることと、すべての枝をフィクスチャで通すことを求める
- 静的に検出する手段(regorus と `yata` の機能に合わせた capabilities のファイルを作り、`opa check --capabilities` で検査する)は、今は設けない(未決事項)。capabilities のファイルを手で保守する手間が大きく、`yata rules test` と重なるため
- `opa fmt` が regorus で読めない形に書き換えないことは、フォーマットした後に `yata rules test` が通ることで確かめる(CI は両方を実行する)

### Regal の設定(`.regal/config.yaml`)

```yaml
rules:
  idiomatic:
    directory-package-mismatch:
      # Packages are yata.rules.<rule ID> and yata.lib.<name>.<topic>, which
      # include a prefix that is not part of the directory path (by design).
      level: ignore
  style:
    opa-fmt:
      # Formatting is checked by `opa fmt --list --fail` in CI.
      level: ignore
    line-length:
      # Metadata strings (description, remediation) can be long.
      level: warning
capabilities:
  from:
    engine: opa
    version: v1.2.0
```

- `directory-package-mismatch` は、パッケージ名がディレクトリの構成と一致することを求める規則で、`yata.rules.<ID>` の形(ADR-0001)と合わないため無効にする
- `opa-fmt` は `opa fmt` の検査と重なるため無効にする(同じ違反が 2 回報告されないように)
- `capabilities` は、regorus が適合をうたう OPA の版(regorus の README。この設計の時点で v1.2.0)にする。それより新しい OPA の機能に依存する Regal の規則を動かさないため。作成時に regorus の README で版を確かめる
- ほかの規則は既定のまま使う。誤って報告される規則が見つかったら、理由を書いて `level: ignore` にする PR を出す

### `ci.yml`

| 項目 | 内容 |
| --- | --- |
| きっかけ | `push`(main)、`pull_request`、`workflow_call`(`release.yml` から呼ぶ) |
| 権限 | `permissions: contents: read` |
| Action の固定 | 外部の Action はコミットの SHA で固定し、コメントに版を書く(yata-cli の CI と同じ)。`actions/checkout` は `persist-credentials: false` |
| 実行環境 | `ubuntu-latest` |
| `workflow_call` の入力 | `yata-mode`(`ci` / `release`。既定 `ci`) |

ワークフローの環境変数:

| 変数 | 内容 |
| --- | --- |
| `YATA_CLI_REF` | 空、または yata-cli のコミットの完全な SHA。空でなければ、そのコミットからビルドした `yata` を使う(下の「CI で使う `yata`」) |

ジョブ:

| ジョブ | 手順 |
| --- | --- |
| `lint` | 1. checkout。2. `.rego` があるかを調べる(`find . -path './.*' -prune -o -type f -name '*.rego' -print -quit` の出力が空でないか)。無ければ以降を飛ばし、`::notice::` で知らせる。3. setup-opa。4. `opa fmt --list --fail .`。5. setup-regal。6. `regal lint --format github .` |
| `changelog` | `pull_request` のときだけ。1. checkout(`fetch-depth: 0`)。2. 断片の形式を確かめる(下の表)。3. 断片が要る PR に断片があるかを確かめる(下の表) |
| `plan` | 対象の `yata` の一覧を作り、`rules-test` の matrix として出力する(下の「CI で使う `yata`」の表)。1. `yata-mode` が `release` で `YATA_CLI_REF` が空でなければ、`::error::` を出して失敗する。2. `YATA_CLI_REF` が空でなければ `[{"ref": "<SHA>"}]`、空なら `[{"version": "<min_yata_version>"}, {"version": "<最新>"}]`(同じ版なら 1 つ。初期構成では下の「初期構成での Release のバイナリの経路」のとおり失敗する) |
| `rules-test` | `plan` の matrix で版ごとに動く。1. checkout。2. ルールがあるかを調べる(`find . \( -path ./lib -o -path './.*' \) -prune -o -type f -name '*.rego' -print -quit` の出力が空でないか)。無ければ以降を飛ばし、`::notice::` で知らせる。3. `./.github/actions/setup-yata`(`ref` か `version` を渡す)で `yata` を用意する。4. `yata --version` を出す。5. `yata rules test --policy-dir . --format text` |
| `rules-test-result` | `needs: rules-test`、`if: always()`。`needs.rules-test.result` が `success` でなければ失敗する。matrix のジョブ名(`rules-test (<版>)`)は版ごとに変わるため、main の保護の必須のチェックにはこのジョブを使う |

`changelog` ジョブの確かめ方:

| 確かめること | 内容 |
| --- | --- |
| 断片の形式 | PR で追加・変更した `changelog.d/*.md`(`README.md` を除く)について、名前が `^([0-9]+-)?[a-z0-9]+(-[a-z0-9]+)*\.md$` に合う。`###` の見出しが `Added`・`Changed`・`Deprecated`・`Removed`・`Fixed`・`Security` のどれかで、各見出しの下に `- ` で始まる箇条が 1 つ以上ある。それ以外の見出し(`#`、`##`)が無い |
| 断片の有無 | base との差分(`git diff --name-only <base>...<head>`)に、`lib/` と `.` で始まるディレクトリの外の `.rego`、または `yata-policies.json` があれば、`changelog.d/` に追加・変更した断片が 1 つ以上要る。ただし、`CHANGELOG.md` を変える PR(リリースの PR)と、ラベル `skip-changelog` の付いた PR(判定を変えない書式の修正など)は除く |

- ルールが無い間(初期構成)は、`yata rules test` がルール 0 件のディレクトリ全体の失敗(終了コード 2)になるため、手順 2 で飛ばす。`lib/` だけがある状態はルール 0 件として `yata rules test` で失敗させる(手順 2 で `lib/` を除くのはそのため)
- ジョブの時間の上限は `lint`・`changelog`・`plan`・`rules-test-result` が 10 分、`rules-test` が 45 分(ソースからのビルドを含むため)
- 判定を変える `lib/` の変更は差分からは見分けられないため、断片の有無の確認の対象にしない(CONTRIBUTING で断片を置くよう求める)

**`setup-yata`(composite action)**: `.github/actions/setup-yata/action.yml` に置き、`ci.yml` の `rules-test` と `release.yml` の `package` の両方から使います(手順を重ねて書かないため)。

| 入力 | 内容 |
| --- | --- |
| `ref` | yata-cli のコミットの SHA。指定すればソースからビルドする |
| `version` | yata-cli の Release の版(`x.y.z`)。指定すれば Release のバイナリを使う |

- `ref` と `version` のどちらか 1 つだけを受け付け、両方・どちらも無ければ失敗する
- 用意した `yata` を `$GITHUB_PATH` に足す

### CI で使う `yata`

図: [CI で使う yata の決め方](../diagrams/Diagram-0009-CI-Yata-Selection.md)

`YATA_CLI_REF` と `yata-mode` で決めます。

| 条件 | 使う `yata` |
| --- | --- |
| `YATA_CLI_REF` が空でない(`yata-mode` が `ci`) | そのコミットの yata-cli のソースからビルドしたもの 1 つ |
| `YATA_CLI_REF` が空 | yata-cli の Release のバイナリのうち、`min_yata_version` の版と、最新の版の 2 つ(同じなら 1 つ)。matrix で両方に `yata rules test` を実行する |
| `yata-mode` が `release` で `YATA_CLI_REF` が空でない | 失敗にする(リリースはリリースした `yata` で確かめる) |

- **初期構成での Release のバイナリの経路**: Release のバイナリの取得の手順は yata-cli のリリースの設計が決まるまで書けない(下の「Release のバイナリ」)。初期構成では、`plan` は `YATA_CLI_REF` が空なら `::error::` で「Release のバイナリの経路はまだ無いので `YATA_CLI_REF` を設定する」旨を出して失敗し、`setup-yata` も `version` を渡されたら同じ旨で失敗する。黙って飛ばさず、`yata rules test` を実行しないまま CI が通ることを防ぐため

**ソースからのビルド**(`setup-yata` の `ref`。yata-cli のリリースが無い間と、まだリリースされていない `yata` の機能が要るルールを先に書く間)

1. `actions/cache` で、キー `yata-<runner.os>-<inputs.ref>` のビルド済みのバイナリ(`~/.yata-ci/bin/yata`)を探す。あればビルドを飛ばす
2. `git clone --filter=blob:none https://github.com/morryyrrom/yata-cli.git "$RUNNER_TEMP/yata-cli"` の後、`git -C "$RUNNER_TEMP/yata-cli" checkout "<inputs.ref>"`(composite action の入力 `ref`。ワークフローの `YATA_CLI_REF` を直接は読まない)
3. そのディレクトリで `rustup toolchain install`(yata-cli の `rust-toolchain.toml` の版が入る)と `cargo build --release --locked -p yata-cli`
4. `target/release/yata` を `~/.yata-ci/bin/` に置き、`$GITHUB_PATH` に足す

- yata-cli の Rust の版をワークフローに重ねて書かないよう、yata-cli のディレクトリの中でビルドする
- `YATA_CLI_REF` の更新は PR で行う。初期構成では、作成時点の yata-cli の main の最新のコミットにする
- 実行中の `yata` の版は yata-cli の `Cargo.toml` の版になる。`min_yata_version` がそれより新しいと `yata rules test` が終了コード 2 になるため、ソースからビルドしている間も、`min_yata_version` はリリース済みの版にし、`YATA_CLI_REF` のコミットの版を超えないようにする(上げるのは、その版の `yata` がリリースされた後。`YATA_CLI_REF` を空にする PR と同じでなくてよい。順序は上の「`min_yata_version` と `yata` のリリースの関係」)

**Release のバイナリ**(`setup-yata` の `version`。yata-cli のリリースが出た後)

- yata-cli の Release から、対象の版のバイナリ(Linux の x86_64 のもの)を取得し、yata-cli が配布するチェックサムで確かめてから使う
- `min_yata_version` は `jq -r .min_yata_version yata-policies.json` で読む。最新の版は `gh release view --repo morryyrrom/yata-cli --json tagName` で調べる
- 配布物の名前、タグの形式、チェックサムのファイルは yata-cli のリリースの設計で決まる(未決事項)。決まったら、この節の手順を具体化して `setup-yata` と `plan` を書き換え、上の「初期構成での Release のバイナリの経路」の失敗を外す
- 最初の `yata-rules` のリリースの前に、この方式に切り替える(`YATA_CLI_REF` を空にする)

### `release.yml`

| 項目 | 内容 |
| --- | --- |
| きっかけ | タグの `push`(`v*`)、`workflow_dispatch`(試しの実行。Release を作らない) |
| 同時実行 | `concurrency: release`(同時に 1 つ) |
| 権限 | 既定は `contents: read`。Release を作るジョブだけ `contents: write` |

ジョブ:

| ジョブ | 手順 |
| --- | --- |
| `verify` | 1. checkout(`fetch-depth: 0`)。2. タグが `^v[0-9]+\.[0-9]+\.[0-9]+$` に合い、`v` + マニフェストの `version` と一致する。3. タグのコミットが `origin/main` に含まれる(`git merge-base --is-ancestor`)。4. CHANGELOG に `## [<version>]` の節があり、`changelog.d/` に `README.md` 以外の断片が残っていない。5. `ci.yml` の `env.YATA_CLI_REF` が空(`yq` で読む)。`workflow_dispatch` のときは 2〜5 を飛ばし、版はマニフェストの `version` を使う |
| `ci` | `uses: ./.github/workflows/ci.yml`。`with: yata-mode:` は、タグの `push` では `release`、`workflow_dispatch` では `ci`(`YATA_CLI_REF` が空でなければソースからビルドした `yata` で試せる)。`verify` の後 |
| `package` | 1. checkout。2. `git archive` で tar.gz と zip を `dist/` に作る(上の「タグと配布物」)。3. `dist/` で `sha256sum yata-rules-<version>.tar.gz yata-rules-<version>.zip > SHA256SUMS`。4. tar.gz を一時ディレクトリに展開し、`setup-yata` で用意した `yata` で検査する(`yata` は `ci.yml` の `plan` と同じ規則で選ぶ: `YATA_CLI_REF` が空でなければ `ref` にそれを渡し、空なら `version` に `min_yata_version` を渡す。タグの `push` では `verify` の手順 5 により常に後者)。コマンドは `yata rules test --policy-dir <展開先>/yata-rules-<version>`(`export-ignore` で必要なファイルが落ちていないことの確認。ルールが無ければ `ci.yml` の `rules-test` の手順 2 と同じ条件で飛ばす)。5. `dist/notes.md` を作る。タグの `push` では CHANGELOG のその版の節を取り出す。`workflow_dispatch` では、`changelog.d/` の断片(`README.md` を除く)を名前の順につないだもの(断片が無ければ空のファイル)にする。6. `actions/upload-artifact` で `dist/` を残す。`ci` の後 |
| `publish` | タグの `push` のときだけ。`contents: write`。1. `package` の成果物を取得する。2. `gh release create v<version> --draft --verify-tag --title "yata-rules <version>" --notes-file notes.md` と配布物 3 つ。3. `gh release edit v<version> --draft=false` で公開する |

- 下書きで作って配布物を付けてから公開するのは、immutable releases では公開後に配布物を足せないため
- `workflow_dispatch` は、ワークフローの変更を確かめるために使う(`package` までを実行し、成果物をワークフローの実行に残す)

## 初期のファイル

初期構成の PR(`yata-rules` の Issue)で作るファイルです。文書は英語で書き、日本語版は作りません。

| ファイル | 内容 |
| --- | --- |
| `yata-policies.json` | 上の「初期値」 |
| `README.md` | 下の「README」 |
| `CONTRIBUTING.md` | 下の「CONTRIBUTING」 |
| `CHANGELOG.md` | 上の「CHANGELOG」の初期の内容 |
| `changelog.d/README.md` | 上の「変更の断片」の名前・中身・置く PR の規則(英語) |
| `LICENSE-MIT`、`LICENSE-APACHE` | yata-cli のリポジトリのものと同じ本文(著作権者の表記 `The Yata Cloud Contributors` も同じ) |
| `.gitattributes` | 下の内容 |
| `.gitignore` | 下の内容 |
| `.regal/config.yaml` | 上の「Regal の設定」 |
| `.github/workflows/ci.yml` | 上の「`ci.yml`」。`YATA_CLI_REF` は作成時点の yata-cli の main の最新のコミット |
| `.github/workflows/release.yml` | 上の「`release.yml`」 |
| `.github/actions/setup-yata/action.yml` | 上の「`setup-yata`(composite action)」。初期構成では `ref` の経路だけを持ち、`version` では失敗する |

`.gitattributes`:

```
* text=auto eol=lf
/.github export-ignore
/.regal export-ignore
/.gitattributes export-ignore
/.gitignore export-ignore
/changelog.d export-ignore
```

- 改行を LF にそろえる(Windows で checkout しても、フィクスチャとルールの内容が変わらないように)

`.gitignore`:

```
# Release archives built locally
/dist/
# Editor and OS files
.idea/
.vscode/
.DS_Store
Thumbs.db
```

- `CLAUDE.md` と `.claude/` は `.gitignore` ではなく、各自の `.git/info/exclude` で除外する(yata-cli と同じ扱い。リポジトリには入れない)

### yata-cli の文書へのリンク

README と CONTRIBUTING は `yata-rules` にあるため、yata-cli の `docs/` へは相対パスでリンクできません。当面は `https://github.com/morryyrrom/yata-cli/blob/main/docs/<フォルダ>/<ファイル名>.md` の形の URL でリンクします。`docs/` の Wiki への同期ができたら、Wiki のページの URL に置き換えます。

### README

英語で、次の節を置きます。

1. 概要: Yata Cloud の公式のポリシーのセット(AWS)。CIS AWS Foundations Benchmark などに基づくルールを Rego で書いたもの。`yata` の利用者向け
2. 状態: 開発の初期であること
3. 互換性: 必要な `yata` の版は `yata-policies.json` の `min_yata_version` と各リリースのノートに書いてあること。合わなければ `yata` が終了コード 2 で知らせること
4. 取得: 上の「取得の方法」(アーカイブと `SHA256SUMS`、`gh release verify-asset`、git clone はタグを指定、main を使わないこと)
5. 使い方: `--policy-dir` / `YATA_POLICY_DIR`(上の「`yata` からの指定」)。既定の場所が無いこと
6. 更新と独自のルール: 上の「更新」「独自のルール」
7. バージョンの付け方: 上の「バージョンの上げ方」の表の要約と、この設計書へのリンク(上の「yata-cli の文書へのリンク」の形)
8. ルールの一覧: `yata rules list --policy-dir <DIR>` で見られること(README に一覧を書かない。ルールの追加のたびに書き換える共有のファイルにしないため)
9. 基準について: CIS のコントロールの番号を参照しているが、CIS が承認・保証したものではないこと(ルールの説明は独自に書いたもの)
10. ライセンス: yata-cli の README と同じ文(MIT OR Apache-2.0、Contribution の節を含む)

### CONTRIBUTING

英語で、次の節を置きます。全体設計の内容は繰り返さず、yata-cli の該当ページへのリンク(上の「yata-cli の文書へのリンク」の形)にします。

1. 必要なもの: `yata`(CI と同じ版。`ci.yml` の `YATA_CLI_REF` か `min_yata_version`)、OPA と Regal(`ci.yml` で固定した版)
2. 手元での確認: `opa fmt --write .`、`regal lint .`、`yata rules test --policy-dir .`
3. ルールの追加: 置き場所と名前(上の「ルールを置くディレクトリの決め方」「ルール名」)、書式とメタデータ([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「ルールの書式」「書き方の注意」)。同じ名前の完全ルール(`message` など)を `if` の付いた複数の定義で書くと、両方が成り立つリソースで評価がエラー(conflict)になるため、場合分けは `else` でつなぐ(実装で確かめた結果は [ポリシーエンジンと `yata rules test`](Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「実装時の確認事項」の「リソースごとの評価の実装で確かめた結果」)。フィクスチャ([ADR: ポリシーとフィクスチャの形式・配置・自動検出](../adr/ADR-0001-Policy-and-Fixture-Layout.ja.md) の「フィクスチャの形式」)。1 つの PR で 1 つのルール(またはごく近いルールの組)を足す
4. フィクスチャ: ダミーの値だけを使う(Feature-0008 の「ダミーとして認めるアカウント ID」)。`fail`・`message`・`reason` のすべての枝を通す。取得の失敗の印を含むケース、項目が無い・`null` のケース、複数のリージョンのケースを用意する
5. 組み込み関数: `yata` で使えるものに限る(Feature-0008 の「regorus の cargo feature」)。入力の値を `net.cidr_expand`・`numbers.range` に渡さない(時間の上限で止まらないため)。`time.now_ns` を使うルールの扱い(Feature-0008 の未決事項)
6. ライブラリ: `lib/` の変更は単独の PR、既存の関数の意味を変えない
7. 互換性: 公開したルールのファイルを移動・改名しない。判定・メタデータを変えるときは上の「バージョンの上げ方」の表で影響を確かめる。`changelog.d/` に断片を置き、ルール ID とともに書く(上の「変更の断片」)。`CHANGELOG.md` は直接書き換えない
8. 新しいリソース種別を使うルール: その種別のコレクターを含む `yata` がリリースされるまでは、`YATA_CLI_REF` でソースからビルドした `yata` で確かめる(上の「CI で使う `yata`」)
9. CIS の文書の扱い: CIS Benchmarks の本文(説明、根拠、監査・修正の手順)を写さない。メタデータの文章は独自に書き、`references` にコントロールの番号を書く。CIS Benchmarks の PDF は CC BY-NC-SA 4.0 で、このリポジトリのライセンス(MIT OR Apache-2.0)では再配布できないため
10. ライセンス: 貢献は MIT OR Apache-2.0 で提供されること(README の Contribution の節)
11. PR とブランチ: GitHub Flow(main から feature ブランチ、Merge commit)。コミットと PR の題名は `<Type>: #<Issue> <Title>`

### リポジトリの設定

ファイルではなく GitHub の設定として、初期構成の PR の前後にメンテナーが行います。

| 設定 | 内容 |
| --- | --- |
| 既定のブランチ | `main` |
| main の保護 | PR を必須にし、`ci.yml` の `lint`、`changelog`、`rules-test-result` の成功を必須のチェックにする(`rules-test` は matrix で名前が版ごとに変わるため必須にしない)。直接の push と force push を禁止する |
| ラベル | `skip-changelog` を作る(上の「`changelog` ジョブの確かめ方」) |
| タグの保護 | `v*` のタグの作成・更新・削除をメンテナーに限る(リポジトリのルールセット) |
| immutable releases | 有効にする(上の「改ざんの確認」) |
| マージの方法 | Merge commit だけを許す |
| Actions の権限 | ワークフローの既定の `GITHUB_TOKEN` の権限を読み取りだけにする(書き込みはワークフローの `permissions` で明示する) |

## 振る舞い

この設計は `yata` のコマンド・オプション・終了コードを変えません。

| 場面 | 振る舞い |
| --- | --- |
| ポリシーのディレクトリの指定が無い | 終了コード 2(既定の場所を設けないため、これまでどおり) |
| 取得したセットの `min_yata_version` が実行中の `yata` より新しい | 終了コード 2(`yata_too_old`)。利用者は `yata` を更新するか、古い版のセットを使う |
| `format_version` が対応外 | 終了コード 2(`unsupported_format_version`)。同上 |
| 公式のセットと独自のセットを両方指定する | ディレクトリごとにマニフェストを検証する。`name` が重複すれば終了コード 2 |
| レポート | `ScanReport.policies` に `yata_aws` とその `version` が出る |

## 構成

- yata-cli のリポジトリのコードとテストは変更しない。依存クレートも追加しない(`directories` などを追加しない)
- `yata-rules` のリポジトリに置くものは、上の「初期のファイル」と「リポジトリの構成」のとおり

## 使う AWS API と IAM 権限

ありません。ポリシーのリポジトリの CI と `yata rules test` は AWS に接続しません。

## テスト方針

- yata-cli のテストは変わらない
- `yata-rules` の初期構成の PR で、次を確かめる
  - ルールの無い状態で `ci.yml` が成功し、`lint` と `rules-test` が「ルールが無い」旨の notice を出して飛ばし、`rules-test-result` が成功する
  - `release.yml` を `workflow_dispatch` で実行し(`YATA_CLI_REF` を設定した状態)、`verify` の手順 2〜5 が飛ばされ、`package` までが成功して、`yata-rules-0.1.0.tar.gz`・`.zip`・`SHA256SUMS`・`notes.md`(断片が無いので空)が成果物に残る(ルールが無い間は、`package` の手順 4 の `yata rules test` は飛ばされる)
  - アーカイブに `.github/`、`.regal/`、`.gitattributes`、`.gitignore`、`changelog.d/` が含まれない
  - `YATA_CLI_REF` を空にした試しの PR で、`plan` が「Release のバイナリの経路はまだ無い」旨で失敗する(マージしない)
- 最初のルールの PR で、断片が無ければ `changelog` が失敗し、形式の正しい断片を置けば通ることを確かめる
- ソースからのビルドで、2 回目の実行がキャッシュを使い、ビルドを飛ばすことを確かめる
- 最初のルールの PR で、`yata rules test` が実際に実行され、`opa fmt` と Regal がそのルールを検査することを確かめる
- 実 AWS には接続しない。フィクスチャはダミーの値だけを使う(`yata rules test` の検査)

## 全体設計・ADR に必要な変更

### 順序

1. **全体設計の PR(先)**: #43(PR #44)で main に入れた。`docs/architecture/` と ADR-0001 だけを変える単独の PR を、この設計の PR より先に main へ入れる。この設計書はまだ main に無いため、その PR ではこの設計書へのリンクを置かず、決定そのものを書いて未決事項を閉じる(下の「全体設計の PR の変更」)
2. **この設計の PR(後)**: 1 を取り込んだ後、この設計の PR で、全体設計の該当箇所からこの設計書へのリンクを足す(下の「この設計の PR で足すリンク」)。リンクを足す変更は小さく、1 で決めた文意を変えない

### 全体設計の PR の変更

| ファイル | 箇所(今の記述) | 変更後の文意 |
| --- | --- | --- |
| `docs/architecture/Architecture-Policy-Evaluation.ja.md` | 「読み込み」→「ポリシーのディレクトリの決定」の 1 段落目(「`--policy-dir <DIR>`(複数指定可)→ 環境変数 `YATA_POLICY_DIR`(…)→ 既定の場所、の順に決めます。…既定の場所は未決事項で、決まるまでは「既定なし」として扱い、どちらも無ければ終了コード 2 にします。…」) | 順序から「→ 既定の場所」を外し、「`--policy-dir` → `YATA_POLICY_DIR` の順に決める。既定の場所は設けず、どちらも無ければ終了コード 2」とする。オプションと環境変数を混ぜないこと、優先度の規則への参照はそのまま |
| 同上 | 同じ節の 2 段落目(「`yata` はネットワークからポリシーを取得しません。ポリシーのリポジトリの取得・更新(git clone、ダウンロードなど)は利用者が行います。」) | 「`yata` はポリシーを取得・更新しない(ダウンロードのコマンドも設けない)。取得・更新は利用者が、ポリシーのリポジトリの配布物(リリースのアーカイブ、git clone)で行う。`yata` は取得したポリシーの署名・出所を検証しない」とする |
| 同上 | 「マニフェスト」の表の `version` の行(「ポリシーのセットのバージョン(`x.y.z`)。…」) | 「SemVer に従う `x.y.z`(プレリリースの部分は使えない)」を加える |
| 同上 | 「外部のポリシーの安全性」→「(a) 信頼の前提」の 3 つ目の箇条(「`yata` は、ポリシーの署名や出所を検証しない。署名・改ざんの確認は、未決事項の「ポリシーの取得・更新」に含める」) | 後半を「改ざんの確認は利用者が、ポリシーの配布元が提供する手段(チェックサム、配布元のリリースの署名付きの記録など)で行う」に替える |
| 同上 | 「未決事項(安全性)」の表の「ポリシーの署名・改ざんの確認」の行 | 行を外す(表が空になれば、節ごと外す) |
| 同上 | 「未決事項」の表の「ポリシーのディレクトリの既定の場所」「ポリシーの取得・更新」「ポリシーのリポジトリの構成」の 3 行 | 3 行を外す。「既定の場所」と「取得・更新」の決定は上の各行の本文に移る。「ポリシーのリポジトリの構成」は本文には足さず、行を外すだけにする(構成は機能の設計書で扱い、そのことは ADR-0001 の追記で示す) |
| `docs/architecture/Architecture-CLI-Interface.ja.md` | 「オプションと環境変数・既定値の一覧」の `--policy-dir` の行の既定値(「なし(未決事項)。どちらも無ければ終了コード 2」) | 「なし(既定の場所は設けない)。どちらも無ければ終了コード 2」 |
| 同上 | 「オプションの優先度」の「既定値が未決の項目」の 1 つ目の箇条(「既定の場所が未決の項目(`--policy-dir`)は、決まるまで「既定なし」として扱い、…」)と 2 つ目の箇条(`YATA_POLICY_DIR` の空の要素) | 1 つ目を「`--policy-dir` には既定の場所を設けない。オプションも環境変数も無ければ終了コード 2」に替え、2 つ目とともに「既定値が未決の項目」の外(同じ節の独立した箇条)に移す |
| 同上 | 「オプションの優先度」の「3. 既定値(既定の場所)」 | 変えない。すべてのオプションに当てはめる一般の規則で、既定値を持つほかのオプション(`--format` など)に要るため。`--policy-dir` に既定値が無いことは上の箇条で示す |
| 同上 | 「未決事項」の表の「ポリシーのディレクトリの既定の場所」の行 | 行を外す |
| `docs/architecture/Architecture-Initial-Dependencies.ja.md` | 「未決事項」の表の「ポリシーのディレクトリの既定の場所を決めるクレート」の行 | 行を外す |
| 同上 | 「採用しないもの」の表 | `directories` / `dirs` の行を足す。理由は「ポリシーのディレクトリの既定の場所を設けないため」。「出典」の `directories`・`dirs` の行は残す |
| `docs/adr/ADR-0001-Policy-and-Fixture-Layout.ja.md` | 本文の「配布と読み込み」の 4 つ目の箇条(「…既定の場所は未決事項で、決まるまでは指定が無ければ終了コード 2」)と、「ポリシーのリポジトリでの確認」の最後の箇条(「ポリシーのリポジトリ自体の構成(CI の定義、リリースの方法など)は、この ADR の範囲外とする」) | 本文は変えず、末尾に「追記」の節を足す。追記の文意: 「既定の場所は設けないと決めた(指定が無ければ終了コード 2 のまま)。`yata` はポリシーを取得せず、署名・出所も検証しない。ポリシーのリポジトリの構成・配布・リリースは、機能の設計書で定める」 |

- [アーキテクチャ概要](../architecture/Architecture-Overview.ja.md) の「データの流れ」はすでに「どちらも無ければ終了コード 2」と書いており、変更は要らない
- [テスト方針](../architecture/Architecture-Testing-Strategy.ja.md) は「ポリシーのリポジトリの CI で `yata rules test --policy-dir .` を実行する」と書いており、この設計と矛盾しない。変更は要らない
- [CLI インターフェース](../architecture/Architecture-CLI-Interface.ja.md) の「ポリシーのディレクトリ」の「`yata` はポリシーをネットワークから取得しない。…」の箇条は、この設計と矛盾しない。変更は要らない

### この設計の PR で足すリンク

全体設計の PR がマージされた後、この設計の PR で次のリンクを足します。

| ファイル | 箇所 | 足すリンク |
| --- | --- | --- |
| `docs/architecture/Architecture-Policy-Evaluation.ja.md` | 「ポリシーのディレクトリの決定」の 2 段落目 | 配布の方法と改ざんの確認の手段として、この設計書の「利用者の取得と指定」「改ざんの確認」 |
| 同上 | 「マニフェスト」の表の `version`・`min_yata_version` の行 | 上げ方・上げる時期として、この設計書の「バージョンの上げ方」「`min_yata_version` と `yata` のリリースの関係」 |
| `docs/architecture/Architecture-CLI-Interface.ja.md` | 既定の場所を設けない箇条 | 理由として、この設計書の「`yata` からの指定」 |
| `docs/architecture/Architecture-Initial-Dependencies.ja.md` | 「採用しないもの」の `directories` / `dirs` の行 | 理由として、この設計書の「`yata` からの指定」 |
| `docs/adr/ADR-0001-Policy-and-Fixture-Layout.ja.md` | 末尾の追記 | この設計書 |

## 後続の作業

| # | 作業 | リポジトリ | 依存 |
| --- | --- | --- | --- |
| 1 | 全体設計・ADR の更新(上の「全体設計の PR の変更」) | yata-cli | なし。この設計の PR より先にマージする |
| 2 | この設計の PR に、全体設計からこの設計書へのリンクを足す(上の「この設計の PR で足すリンク」) | yata-cli | 1 のマージ。この設計の PR の中で行う |
| 3 | `yata-rules` の初期構成の作成(上の「初期のファイル」「リポジトリの設定」) | yata-rules | この設計のマージ |
| 4 | 最初のルール群(6a〜6e。共通の関数は `lib/` の単独の PR を先に) | yata-rules | 3、[最初のルール群の範囲](Feature-0005-Initial-Rule-Set-Scope.ja.md) の依存 |
| 5 | yata-cli の最初のリリースの計画(配布物の名前、タグ、チェックサム) | yata-cli | なし |
| 6 | `yata-rules` の CI を Release のバイナリに切り替え、最初のリリースを行う | yata-rules | 4、5 と yata-cli の最初のリリース |
| 7 | 利用者ガイド(ポリシーの取得・検証・指定・更新・独自のルール。上の「利用者の取得と指定」の内容) | yata-cli(`docs/guides/`) | この設計のマージ。公開は 6 の後 |

## 出典

- Semantic Versioning 2.0.0: <https://semver.org/spec/v2.0.0.html>
- Cargo の SemVer の慣習(0.y.z の扱い): <https://doc.rust-lang.org/cargo/reference/semver.html>
- Keep a Changelog 1.1.0: <https://keepachangelog.com/en/1.1.0/>
- `git archive` と `export-ignore`: <https://git-scm.com/docs/git-archive>、<https://git-scm.com/docs/gitattributes#_creating_an_archive>
- GitHub の immutable releases(配布物とタグの保護、attestation): <https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases>、<https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/>
- リリースの確かめ方(`gh release verify`、`gh release verify-asset`。自動の Source code のアーカイブは確かめられない): <https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/secure-your-dependencies/verify-release-integrity>
- リリースの管理(下書きで配布物を付けてから公開する): <https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository>
- OPA の CLI(`opa fmt` の `--list`・`--fail`、`opa check` の `--capabilities`): <https://www.openpolicyagent.org/docs/cli>
- OPA の capabilities(capabilities に無い組み込み関数を使うポリシーを拒否する): <https://www.openpolicyagent.org/docs/operations>
- OPA(ライセンス Apache-2.0): <https://github.com/open-policy-agent/opa>
- `open-policy-agent/setup-opa`: <https://github.com/open-policy-agent/setup-opa>
- Regal(ライセンス Apache-2.0): <https://github.com/open-policy-agent/regal>
- Regal の設定(`.regal/config.yaml`、規則の `level`、`capabilities`): <https://www.openpolicyagent.org/projects/regal/configuration>、<https://www.openpolicyagent.org/projects/regal/configuration/capabilities>
- Regal の `directory-package-mismatch`: <https://www.openpolicyagent.org/projects/regal/rules/idiomatic/directory-package-mismatch>
- `open-policy-agent/setup-regal`: <https://github.com/marketplace/actions/setup-regal>
- regorus の OPA への適合と未実装の組み込み関数: <https://github.com/microsoft/regorus>
- CIS Benchmarks の PDF のライセンス(CC BY-NC-SA 4.0): <https://www.cisecurity.org/insights/blog/making-security-happen>

## 未決事項

| 項目 | 内容 | 決める時期 |
| --- | --- | --- |
| CI の `yata` の取得の細部 | yata-cli の Release の配布物の名前、タグの形式、チェックサムのファイル、Linux の x86_64 のバイナリがあるか。決まったら「Release のバイナリ」の手順を具体化する | yata-cli の最初のリリースを計画する Issue(後続の作業 5) |
| 1.0.0 にする時期 | 例: 最初のルール群(24 件)がそろい、CIS v7.1.0 の本文で番号を確かめ、ルール ID を固定できると判断したとき | 最初のルール群のリリースの後 |
| regorus の互換性の静的な検査 | regorus と `yata` の有効な機能に合わせた capabilities のファイルを作り、`opa check --capabilities` で、フィクスチャで通らない経路の対応していない組み込み関数も見つけるか。regorus が capabilities のファイルを提供すれば、それを使う | 対応していない組み込み関数がリリースに混ざった時点、または regorus が capabilities を提供した時点 |
| 指定が無いときの案内 | ポリシーのディレクトリの指定が無いときの終了コード 2 のメッセージに、`yata-rules` の Release の URL と `--policy-dir` / `YATA_POLICY_DIR` の使い方を出すか(CLI のメッセージの変更) | `yata scan` を実装する Issue |
| 利用者ガイドのページ名 | 後続の作業 7 のページのファイル名(`docs/guides/` の中で重複しない名前) | 後続の作業 7 |
| ルールの設計の置き場所 | 個々のルールの判定の細部(どのコントロールをどの項目でどう判定するか)を、yata-cli の `docs/features/` に書くか、`yata-rules` の Issue・PR に書くか、`yata-rules` に `docs/` を設けるか | 最初のルールの Issue(6a〜6e の最初のもの)の前 |
| CLA・DCO | 貢献に CLA への同意や DCO の署名(`Signed-off-by`)を求めるか。初期の CONTRIBUTING には書かない | 外部からの最初の貢献を受ける前 |
