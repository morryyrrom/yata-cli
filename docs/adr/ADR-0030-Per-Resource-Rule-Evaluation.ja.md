# ADR: ルールをリソースごとに評価する

> This Japanese version is the authoritative source. The English version (`ADR-0030-Per-Resource-Rule-Evaluation.md`) has not been written yet.

- ステータス: 承認
- 関連: [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md)、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md)、[ADR: ポリシーエンジン](ADR-0001-Policy-Engine-Rego-with-Regorus.ja.md)、[ADR: ポリシーとフィクスチャの形式・配置・自動検出](ADR-0001-Policy-and-Fixture-Layout.ja.md)、[ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md)

## 背景

これまでのルールは、不合格を部分集合ルール `fail contains finding if { ... }` で返していました。

```rego
fail contains finding if {
	some bucket in input.aws.s3.bucket
	actual := object.get(bucket.configuration, ["GetPublicAccessBlock", "PublicAccessBlockConfiguration", "BlockPublicAcls"], null)
	actual != true
	finding := {
		"resource_id": bucket.id,
		"message": sprintf("Bucket %s does not block public ACLs", [bucket.id]),
		"path": "/configuration/GetPublicAccessBlock/PublicAccessBlockConfiguration/BlockPublicAcls",
		"actual": actual,
		"expected": true,
	}
}
```

この書式には次の問題がありました。

- ルールの作者が、`input` の配列を自分でループし、検出ごとに `resource_id`(同じ `id` が複数のリージョンに現れる種別では `resource_region` も)を組み立てる必要がある。Rego に慣れていない人には読み書きしにくい
- エンジンは、要素の `resource_id` / `resource_region` を `target_type` のリソースに対応づける必要があり、対応しない値・型の誤りを評価のエラーにする規則と、その説明の図が要った
- 1 つのリソースに複数の要素を返せたため、1 つのリソースに複数の `fail` の結果が出うる。結果の件数が、判定の内容ではなくルールの書き方で変わる

結果(`RuleResult`)は、もともと `target_type` のリソースごとに作るモデルで、本来の想定は「1 ルール × 1 リソース = 1 件の結果(`pass` か `fail`)」です。この想定に合わせ、ルールの書式と評価の単位を見直しました。

## 決定

### 評価の単位

- エンジンは、ルール × アカウントごとに、`target_type` のリソース 1 つずつについてルールを 1 回評価する
- 結果は 1 ルール × 1 リソースにつき 1 件(`pass` か `fail`)とする。1 つのリソースに複数の不合格の箇所を返す仕組みは設けない。これは結果のモデルが元から想定していた動作で、部分集合ルールで複数件を返せたことの方が想定外だった
- ルールの状態(`evaluated` / `incomplete` / `skipped` / `error`)と判定順は変えない

### ルールの書式

OPA の入門の `default allow := false` と同じ形にします。

```rego
package yata.rules.example_policies.s3.bucket_public_access_block

metadata := { ... }

block_public_acls := object.get(input.configuration, ["GetPublicAccessBlock", "PublicAccessBlockConfiguration", "BlockPublicAcls"], null)

default fail := false

fail if block_public_acls != true

message := sprintf("Bucket %s does not block public ACLs", [input.id])

reason := {
	"path": "/configuration/GetPublicAccessBlock/PublicAccessBlockConfiguration/BlockPublicAcls",
	"actual": block_public_acls,
	"expected": true,
}
```

| 規則 | 必須 | 内容 |
| --- | --- | --- |
| `fail` | 必須 | 真偽値。そのリソースが不合格か。`default fail := false` を必ず書く |
| `message` | 任意 | 空でない文字列。不合格の説明。省略するとメタデータの `title` を使う |
| `reason` | 任意 | オブジェクト `{path, actual, expected}`。不合格の箇所と値。これまでの検出の `path` / `actual` / `expected` の意味と規約を引き継ぐ |

- `message` と `reason` は、`fail` が `true` のリソースでだけ評価して使う
- 型の誤り、Rego の conflict(完全ルールが 2 通りの値になる)、`default` が無く `fail` が undefined になったリソースは、そのルールの評価のエラー(状態 `error`)にする。細部は [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「ルールの規約」

### input と data

- `input` は、評価の対象のリソースのオブジェクトそのもの(`input.id`、`input.region`、`input.configuration` など)
- `data.<provider>.<service>.<resource>` は、そのアカウントで収集した、宣言した種別(`target_type` と `related_types`)の全リソースの配列。中身はこれまでの `input.<provider>.<service>.<resource>` と同じ
- `data.yata.context` は、アカウントの情報(`account`)と対象のリージョン(`regions`)。これまでの `input.account` / `input.regions`。`input` はリソースのオブジェクトそのもので、リソースの項目と混ざるため置けない。アカウント単位の情報なので `data` に置く
- エンジンは、ルール × アカウントごとに `data` を 1 回入れ、リソースごとに `input` だけを差し替える
- フィクスチャも同じ形の `data` を持つ(下の「フィクスチャの形式」)
- ポリシーのパッケージは `yata.rules.*` と `yata.lib.*` に限られるため、`data.<provider>` と `data.yata.context` はポリシーの規則と名前が衝突しない

### 評価のエラーと時間の上限

- あるリソースで評価のエラーが起きたら、そのルール × アカウントの評価をそこで止め、ルールの状態を `error` にして結果を作らない(エラーはルール単位)
- 評価時間の上限(`--policy-timeout`)は、ルール × アカウントのリソースごとの評価の合計に掛ける(リソースごとではない)

### コレクターの方針: リージョンごとの設定

- リージョンごとの設定・有効かどうか(サービスの有効化、既定の設定など)は、存在しない場合も含め、対象リージョンごとに 1 件のリソースとして収集する。例: `aws.ec2.ebs_encryption_default`。IAM Access Analyzer のアナライザーの一覧なら、アナライザーが無いリージョンでも、空の一覧を持つ 1 件を作る
- これで、リージョンごとの判定もリソースごとの評価で書け、結果がリージョンごとに出る(下の「検討した選択肢」の `scope` の案が要らなくなる)
- コレクターは API の結果を絞り込まず・置き換えずにそのまま入れ、結果が空でもリソースを作るだけとする。判定はルールで行う([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「リソース種別」)
- 実際の収集は、該当するコレクターを作るときにこの方針に従う

### なくすもの

- 1 つのリソースに複数件の検出を返すこと
- `fail` の要素の `resource_id` と `resource_region`
- それに関わる評価のエラー(`target_type` のリソースでない `resource_id`、文字列でない `resource_region`、その `id` とリージョンのリソースが無い)と、要素をリソースに対応づける処理
- 同じ `id` が複数のリージョンに現れる種別(CloudTrail の shadow trail など)は、コピーごとに別のリソースとして評価される。ルールは `input.region` でどのコピーかが分かるため、`resource_region` は要らない

### 形式のバージョン

- マニフェストの `format_version` は `1` のままとする
- ルールの書式、`input` の形、フィクスチャの形式の変更は、本来 `format_version` を上げる対象である。ポリシーのセットをまだ 1 つも公開していないため、ポリシーの形式の最初のリリースの前の変更として `1` に含める([ADR: ポリシーとフィクスチャの形式・配置・自動検出](ADR-0001-Policy-and-Fixture-Layout.ja.md) の `resource_region` と同じ扱い)

### フィクスチャの形式

- フィクスチャのキー `input` を `data` に変え、中身を Rego の `data` と同じ形にする: `{"data": {"yata": {"context": {"account": ..., "regions": [...]}}, "aws": {...}}, "expected": {...}}`
- エンジンはフィクスチャの `data` をそのまま Rego の `data` に入れ、`target_type` の配列のリソースを 1 件ずつ Rego の `input` にして評価する
- `expected.fail`(`id` の文字列か `{id, region}`)は変えない
- 理由: フィクスチャの `input` のまま中身をアカウント単位の入力全体にすると、Rego の `input`(評価の対象の 1 件)と同じ名前で意味が違い、ルールの作者が取り違える。フィクスチャを Rego の `data` と同じ形にすれば、ルールの参照(`data.aws.s3.bucket` など)をそのままフィクスチャの中で探せ、エンジンの振り分けも要らない

## 検討した選択肢

### ルールの書式

| 選択肢 | 判断 |
| --- | --- |
| `default fail := false` と `fail if { ... }`(採用) | OPA の入門(`default allow := false`)と同じ形で、Rego を学び始めた人が最初に見る書き方。真偽値なので、値を書き間違える余地が無い。不合格の理由を、任意の `message` / `reason` に分けて書ける |
| `default result := "pass"` と `result := "fail" if { ... }` | 1 つの規則で結果を表せるが、値が文字列なので `"fial"` のような書き間違いが構文の段階で見つからない。値の検証をエンジンに足しても、書き間違いが分かるのは評価したときだけ |
| `fail(r) if { ... }`(リソースを引数に取る関数) | 評価の対象が引数で明示されるが、Rego の関数は入門で扱われず、初心者になじみが薄い。エンジンが関数を呼ぶには `eval_rule` ではなく、問い合わせの文字列を組み立てる必要がある |
| `fail contains finding if { ... }`(これまでの書式) | 1 回の評価でアカウント全体を判定でき、評価の回数が少ない。ただし上の「背景」の問題がある |

### input と data の使い分け

| 選択肢 | 判断 |
| --- | --- |
| 案 B: `input` を評価の対象のリソース、`data` をアカウントの全リソースにする(採用) | 単一のリソースの判定は `input.configuration...` だけで書け、ループが要らない。OPA の用語でも、`input` は判定の対象として同期的に渡す文書、`data` は判定の参照に使う文書で、意味が合う。regorus では `add_data` がエンジンを再準備(解析)させるが、`set_input` はさせないため、`data` をルール × アカウントごとに 1 回入れ、リソースごとに `input` だけを替える形が速い |
| 案 A: `input` に評価の対象と全リソースの両方を入れる(`input.resource` と `input.aws...`) | `data` を使わずに済み、これまでの `input` の形に近い。ただし単一のリソースの判定も `input.resource.configuration...` と長くなり、評価の対象とそれ以外の区別が名前だけになる。リソースごとに大きな `input` を作り直すことにもなる |

### 評価の単位をメタデータ `scope` で選ぶ案

| 選択肢 | 判断 |
| --- | --- |
| `target_type` のリソースごとに評価する(採用) | 評価の単位が `target_type` だけで決まり、`input` はいつも評価の対象の 1 件になる |
| メタデータ `scope`(`regional` / `global`)を足し、`regional` ならリージョンごとにそのリージョンの関連リソースの配列を `input` に入れて評価し、`global` なら全リージョンの配列を `input` に入れて評価する | 採らない。`input` の形がルールによって変わり、「`input` は評価の対象の 1 件」という説明が崩れる。配列を受け取るとルールの作者がループを書くことになり、この ADR の動機とぶつかる。`target_type` と `scope` の 2 つで評価の単位が決まり、組み合わせの規則が要る。`global` は、`target_type` を `aws.account.account` にして `data` を見れば書ける(CIS 4.1 など)。`regional` が要るのは「リージョンごとに判定する」「リージョンごとの結果が欲しい」「そのリージョンの状態を表すリソースが無い」がそろう場合だけ(CIS 2.18 の IAM Access Analyzer、4.3 の AWS Config、5.16 の Security Hub など)で、3 つ目は上の「コレクターの方針: リージョンごとの設定」で無くなる |

### 評価時間の上限の単位

| 選択肢 | 判断 |
| --- | --- |
| ルール × アカウントの合計(採用) | これまでと同じく、1 つのルールが 1 つのアカウントの評価に使える時間の上限になる。リソースの数によらず、1 つのルールの評価が終わるまでの時間に上限がある |
| ルール × リソースごと | 実装は単純だが、リソースが 1 万あれば 1 つのルールが最大で 1 万倍の時間を使え、上限の意味が無くなる |

### 評価のエラーの単位

| 選択肢 | 判断 |
| --- | --- |
| ルール単位(採用) | あるリソースで評価のエラーが起きたら、そのルールの状態を `error` にし、結果を作らない(これまでと同じ)。結果の型(`pass` / `fail`)と出力形式を変えずに済む。エラーの説明には、エラーの起きたリソースの `id` とリージョンを入れる |
| リソース単位 | ほかのリソースの結果を残せるが、`RuleResult` に `error` の状態を足すことになり、出力形式と集計、終了コードの規則に影響する。ルールの誤りは `yata rules test` で見つけるもので、利用者の環境で一部のリソースだけ誤るのはまれと考え、採らない |

## 結果

- ルールの作者は、評価の対象のリソースを `input` で受け取り、合否を真偽値で書けばよい。ほかのリソースは `data` から参照する
- 1 つのリソースに不合格の箇所が複数ある場合は、1 件の結果にまとめる。箇所ごとに結果を分けたい場合は、ルールを分ける。`message` / `reason` を場合ごとに書き分けるときは `else` を使う(conflict を避けるため。[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の「書き方の注意」)
- 結果の件数は、`target_type` のリソースの数と一致する(`skipped` / `error` を除く)
- 評価の回数が、ルール × アカウントごとに 1 回から、`target_type` のリソースの数になる。regorus は評価のたびにルールの値のキャッシュを消すため、`data` だけに依存する補助ルールもリソースごとに計算し直される。`data` の配列を走査するルールの計算量は「対象のリソースの数 × 参照するリソースの数」になる。見積もりと実装時の確認は [ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) の「リソースごとの評価の性能」
- `yata-engine` の評価(`input` の組み立て、`fail` の要素の解釈とリソースへの対応づけ)と、テスト用のポリシーのセットとそのフィクスチャ(`input` から `data` へ)を作り直す。`yata rules test` のコマンドと失敗の種類は変えない。JSON の出力は、`fixture_mismatch` の `unexpected[]` の `messages`(配列)を `message`(文字列)に変える(1 つのリソースに結果が 1 件になるため)
- コレクターは、リージョンごとの設定を、存在しない場合も含めて対象リージョンごとに 1 件のリソースにする(上の「コレクターの方針: リージョンごとの設定」)。既存の `aws.ec2.ebs_encryption_default` の形は元からこの方針に合う
- 全体設計の [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md)、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「ポリシーエンジンへの渡し方」、関連する ADR と機能の設計書を、この決定に合わせて更新した
- ポリシーのリポジトリ(`yata-rules`)のルールは、最初からこの書式で書く

## 出典

- OPA の基本の文書と仮想の文書(`input` と `data`): <https://www.openpolicyagent.org/docs/philosophy>
- Rego の言語仕様(`default`、完全ルールの conflict、`else`): <https://www.openpolicyagent.org/docs/policy-language>
- regorus の `Engine`(`set_input`、`add_data`、`eval_rule`): <https://docs.rs/regorus/0.12.0/regorus/struct.Engine.html>
- regorus 0.12.0 のソース(`add_data` と `set_input` の準備のやり直しの有無、評価ごとの状態の初期化): <https://docs.rs/crate/regorus/0.12.0/source/src/engine.rs>、<https://docs.rs/crate/regorus/0.12.0/source/src/interpreter.rs>
