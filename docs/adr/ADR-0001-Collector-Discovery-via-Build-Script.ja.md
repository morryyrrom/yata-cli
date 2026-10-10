# ADR: コレクターをビルドスクリプトで自動検出する

> This Japanese version is the authoritative source. The English version (`ADR-0001-Collector-Discovery-via-Build-Script.md`) has not been written yet.

- ステータス: 承認
- 関連: [AWS アクセス](../architecture/Architecture-AWS-Access.ja.md)、[ワークスペース構成](../architecture/Architecture-Workspace-Layout.ja.md)

## 背景

新しいチェックを追加するときは、多くの場合、新しいリソース種別のコレクター(AWS からリソースを収集する Rust のコード)も追加します。Rust ではモジュールを `mod` で宣言し、コレクターの一覧も誰かが作る必要があります。素直に書くと、`collectors/mod.rs` や一覧の関数に全ブランチが追記することになり、並列のブランチ同士が衝突します。

## 決定

- コレクターは 1 リソース種別 1 ファイルとし、`crates/yata-provider-aws/src/collectors/<service>/<resource>.rs` に置く
- 各ファイルは `pub(crate) fn collector() -> Box<dyn Collector>` を定義する
- `yata-provider-aws` の `build.rs` が `src/collectors/` 以下の `.rs` ファイルを標準ライブラリだけで走査し、`OUT_DIR` に次を生成する
  - 各ファイルのモジュール宣言(`#[path = "<絶対パス>"] mod <service>_<resource>;`)
  - 全コレクターを返す関数(`pub(crate) fn all_collectors() -> Vec<Box<dyn Collector>>`)
- `src/collectors.rs`(または `lib.rs`)は生成物を `include!` するだけで、手で一覧を書かない
- 生成物はファイルのパスの順に並べ、毎回同じにする。`cargo:rerun-if-changed=src/collectors` を出す
- 同じリソース種別を 2 つのコレクターが返したら、テストで失敗させる
- 複数のコレクターで共有する処理は `src/support/` に置き、検出の対象外とする

図: [ビルド時の自動検出](../diagrams/Diagram-0001-Build-Time-Discovery.md)

## 検討した選択肢

| 選択肢 | 判断 |
| --- | --- |
| build.rs でモジュール宣言と一覧を生成(採用) | ファイルを足すだけで登録される。依存クレートが増えない。全プラットフォームで動く |
| `mod.rs` と一覧の関数に手で追記 | 全ブランチが同じファイルに追記し、衝突する |
| `inventory` クレート(リンク時の自動登録) | 登録は自動になるが、`mod` の宣言は別に必要で、結局 `mod.rs` に追記することになる。依存クレートも増える |
| `linkme` クレート(分散スライス) | `inventory` と同じく `mod` の宣言が必要。リンカーの機能に依存する |
| サービスごとに 1 ファイル | 同じサービスのコレクターを並列に追加すると衝突する |

## 結果

- コレクターの追加は新しいファイル 1 つで完結する
- 生成されたコードは `OUT_DIR` にあるため、エディタで追いにくいことがある。生成するコードは短く保ち、生成の規則を `build.rs` の doc コメントに書く
- 生成するパスの文字列は、Windows の区切り文字(`\`)や空白を含むパスでも正しく扱えるように書き出す(`{:?}` で文字列リテラルにするなど)
- ファイル名からモジュール名を作るため、ファイル名は小文字の snake_case に限る。規則に合わないファイルは build.rs がエラーにする
- 将来、CLI のサブコマンドや出力形式でも同じ問題が出た場合は、この方式を使える(現時点では追加の頻度が低いため、手で列挙する)

## 出典

- Cargo のビルドスクリプト: <https://doc.rust-lang.org/cargo/reference/build-scripts.html>
- `#[path]` 属性: <https://doc.rust-lang.org/reference/items/modules.html#the-path-attribute>
- `inventory`: <https://github.com/dtolnay/inventory>
- `linkme`: <https://github.com/dtolnay/linkme>
