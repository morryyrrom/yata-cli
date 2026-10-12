# ADR: ポリシーエンジンに Rego(regorus)を採用する

> This Japanese version is the authoritative source. The English version (`ADR-0001-Policy-Engine-Rego-with-Regorus.md`) has not been written yet.

- ステータス: 承認
- 関連: [アーキテクチャ概要](../architecture/Architecture-Overview.ja.md)、[ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md)

## 背景

Yata Cloud は、AWS から収集したリソースを宣言的なルールで評価します。ルールを書く言語と、それを評価するエンジンを決める必要があります。

要件は次のとおりです。

- ルールを Rust のコードと分け、再ビルドなしで利用者が追加できる
- 1 つのリソースの属性だけでなく、複数のリソースを突き合わせる判定が書ける(例: CloudTrail の証跡と、その保存先の S3 バケットの設定)
- 1 つのルールが、不合格のリソースごとに理由のメッセージを返せる
- Rust から組み込んで使え、外部のプロセスやランタイムを必要としない
- ライセンスが `MIT OR Apache-2.0` での配布と両立する
- 利用者が書いたルールを読み込んでも、ファイル・ネットワークへのアクセスや無限ループで害を与えない

## 決定

- ルールは **Rego**(v1 の構文)で書く
- 評価には **regorus**(Microsoft による純 Rust の Rego 実装)を使う
- regorus は `default-features = false` とし、ルールに必要な組み込み関数の機能だけを有効にする。ネットワークアクセスを伴う機能は有効にしない
- 評価に時間の上限を設ける。regorus の `Engine` の API 文書にある実行時間の制限(`set_execution_timer_config`)を使う前提とする。`eval_rule` での評価を確実に打ち切れるかは、ポリシーエンジンを実装する Issue で確認する。打ち切れない場合は、別スレッドで評価して時間切れで結果を捨てる方法などに切り替える([ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) の未決事項)

## 検討した選択肢

| 選択肢 | 長所 | 短所 |
| --- | --- | --- |
| **Rego + regorus**(採用) | 複数のリソースの突き合わせ、集合の操作、部分集合ルールでの複数の検出が自然に書ける。クラウドのセキュリティ診断で広く使われている言語で、利用者が知っている可能性が高い。regorus は純 Rust で組み込める。OPA の適合テストの大部分に通る。実行時間の制限、cargo feature による機能の絞り込みがある | 学習の負担が CEL より大きい。regorus は OPA のすべての組み込み関数には対応していない(暗号関連などは対象外)。バイナリが大きくなる(機能を絞れば小さくできる) |
| CEL + `cel` クレート | 式 1 つで書け、学習しやすい。チューリング完全でなく、評価が軽い。`cel` クレートは MIT | 1 つの式で真偽を返す用途が中心で、複数のリソースの突き合わせや、検出ごとのメッセージを返すルールを書きにくい。メタデータを書く場所を別に用意する必要がある |
| Rust のコードでルールを書く | 型で守られる。依存クレートが増えない。速い | 宣言的でない。ルールの追加に再ビルドが必要で、利用者が独自のルールを足せない。ルールの作者に Rust の知識が要る |
| 独自の YAML などの DSL | 単純なルールは短く書ける | 表現力を足すたびに DSL の設計と実装が要る。複雑な判定を書きにくい。保守の負担が大きい |
| OPA(Go 製)を外部プロセスとして呼ぶ | OPA の完全な互換性 | 外部のバイナリへの依存が生じ、単一バイナリで配布できない。プロセス間のやり取りが要る |

## 結果

- ルールの作者は Rego を書く。書き方の規約(メタデータ、`fail` の形)は [ポリシー評価](../architecture/Architecture-Policy-Evaluation.ja.md) で定める
- ルールは OPA の `opa` コマンドでも構文の確認ができる(ただし Yata Cloud のテストは regorus で行う)
- regorus が対応していない組み込み関数は使えない。必要になった場合は、regorus の拡張関数(`add_extension`)で足すか、ポリシーのディレクトリのライブラリ(`lib/`)で Rego の関数として書く。構成情報は SDK の応答をそのまま入れる方針のため、コレクターの側では加工しない(ポリシー文書のパースなど、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) で定めた変換を除く)
- regorus のライセンスは crates.io の登録では `MIT AND Apache-2.0 AND BSD-3-Clause` で、いずれも寛容なライセンスのため配布と両立する。著作権表示とライセンス文を配布物に同梱する
- 評価エンジンを差し替える可能性に備え、regorus に依存するのは `yata-engine` だけとする

## 出典

- regorus: <https://github.com/microsoft/regorus>
- regorus の `Engine` の API: <https://docs.rs/regorus/latest/regorus/struct.Engine.html>
- Rego の言語仕様(OPA): <https://www.openpolicyagent.org/docs/latest/policy-language/>
- CEL の Rust 実装: <https://github.com/cel-rust/cel-rust>
- CEL の言語仕様: <https://github.com/google/cel-spec>

## 追記

- 「決定」の評価時間の上限の確認は、[ポリシーエンジンと `yata rules test`](../features/Feature-0008-Policy-Engine-and-Rules-Test.ja.md) で行った。`set_execution_timer_config` で `eval_rule` の評価を打ち切る方式とし(1 回の組み込み関数の呼び出しの途中では止まらない)、別スレッドで評価する方式は採らない。詳細は同ページの「評価時間の上限の実現方法」。有効にする cargo feature も同ページの「regorus の cargo feature」で決めた
