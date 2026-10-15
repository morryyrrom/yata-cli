# ADR: 列挙の操作の値に出力の形を保つ

> This Japanese version is the authoritative source. The English version (`ADR-0039-Keep-Output-Shape-for-Listing-Operation-Values.md`) has not been written yet.

- ステータス: 承認
- 関連: [リソースモデル](../architecture/Architecture-Resource-Model.ja.md)(「コレクターの方針」「格納の形」「列挙の操作の値」)、[テスト戦略](../architecture/Architecture-Testing-Strategy.ja.md)、[最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md)

## 背景

リソースの構成情報(`configuration`)には、API の操作名をキーにして、その操作の出力のうちそのリソースに当たる部分を入れます。リソースを列挙する操作(`ListBuckets`、`ListUsers`、`DescribeInstances` など)の値は、これまで「出力の配列の中のそのリソースの要素」だけでした。

この形では、出力の最上位にある、要素の配列以外の項目が入る場所がありません。

- S3 の `ListBuckets` の出力は `Buckets`、`Owner`、`ContinuationToken`、`Prefix`。`Owner` は「一覧にしたバケットの所有者」で、すべてのバケットに当てはまる情報だが、バケットの要素の中には無い
- EC2 の `DescribeInstances` の出力は `Reservations[]` と `NextToken` で、インスタンスは各予約の `Instances[]` の中にある。インスタンスの要素だけを入れると、予約の `ReservationId`、`OwnerId`、`RequesterId`、`Groups` が失われる([最初のルール群の範囲](../features/Feature-0005-Initial-Rule-Set-Scope.ja.md) の未決事項「`DescribeInstances` の予約の項目」)

コレクターの方針は「返った結果は、絞り込まず・置き換えずにすべて入れる」です。要素だけを入れる形は、これらの項目を黙って落とすことになり、この方針と食い違います。

## 決定

### 値の形

- リソースを列挙する操作の値は、出力の形を保ったまま、リソースの要素の配列を、そのリソースの要素 1 つだけの配列にしたものとする
  - 例(バケット): `{"Buckets": [{"Name": "...", "CreationDate": "...", "BucketRegion": "..."}], "Owner": {...}, "Prefix": null}`
- この形を、すべての列挙の操作にそろえて使う。最上位の項目が要素の配列だけの操作も同じ形にする
  - 例: `ListUsers` は `{"Users": [{...}]}`、`ListPolicies` は `{"Policies": [{...}]}`、`DescribeSecurityGroups` は `{"SecurityGroups": [{...}]}`
- 最上位の項目は、ページングの項目と応答のメタデータを除いて、すべて入れる。要求の値の写し(`ListBuckets` の `Prefix`)も入れる
- ページングの項目は、続きのトークン(`NextToken`、`ContinuationToken` など)、続きがあるかを示す項目(`IsTruncated` など)、ページの大きさの要求パラメーターの写し(`MaxItems`、`MaxKeys`、`MaxBuckets` など)、そのページの件数(`KeyCount` など)とする。どれもページの分け方で値が変わり、リソースの情報ではない
- 1 つのリソースの情報を取る操作の値は、これまでどおり出力全体とする。これには、必須の識別子でリソースを指して呼ぶ操作(`GetBucketPolicy`、`ListAttachedUserPolicies` など)と、識別子を取らずに呼んでアカウント・リージョンに 1 件のリソースを作る操作(`GetAccountPasswordPolicy`、`ListAnalyzers` など)の両方が当たる。一覧の操作でも、列挙の操作でなければ要素 1 つの配列にしない

### ページごとの値

- 列挙の操作では、全ページをつないだ 1 つの出力を作らない。各リソースの値は、その要素を返したページの出力から作る
- 最上位の項目の値は、そのページの値にする。ページによって値が違ってもそのまま入れる
- 入れる項目の集合は、SDK の出力の型の最上位の項目で決まり、ページによって変えない。そのページが返さなかった項目は `null` にする

### 1 つのリソースの情報を取る操作のページ

列挙の操作でページごとの値を決めたのに合わせ、1 つのリソースの情報を取る操作で全ページを 1 つの出力にまとめる規則も、全体の規則として定める。配列の項目をつなぐ部分は、既存の IAM のコレクター([IAM のコレクター](../features/Feature-0012-IAM-Collectors.ja.md) の「ページングの結合」)の規則と同じで、配列でない項目の扱いを新たに定める。

- 配列の最上位の項目は、項目ごとに、各ページの要素をページの順につなぐ。どのページも返さなければ `null`
- 配列でない最上位の項目(ページングの項目を除く)は、どのページでも同じ値であることを前提にする。値を返したページの値がすべて同じならその値、どのページも返さなければ `null`。返したページの間で値が違えば、その操作の応答を不正(`invalid_response`)とする
- 理由: 1 つのリソースの値は 1 つで、ページごとの値を残す場所が無い。どれか 1 つのページの値を選ぶと、ほかのページの値を黙って置き換えることになる。ページングの項目を除けば、こうした項目はページの分け方で変わらないはずで、違うのは通常起こらない応答である

### 入れ子の配列

- リソースの要素が入れ子の配列の中にある操作では、最上位からその要素までの経路にある配列のすべてを、経路の上の要素 1 つだけの配列にする。経路の上の要素のほかの項目はそのまま入れ、経路に無い配列は変えない
- `DescribeInstances` のインスタンスの値は `{"Reservations": [{"ReservationId": ..., "OwnerId": ..., "RequesterId": ..., "Groups": [...], "Instances": [<そのインスタンスの要素>]}]}` になる。予約の項目は、その予約のどのインスタンスの値にも入る
- これで、最初のルール群の範囲の未決事項「`DescribeInstances` の予約の項目」は、この原則から答えが決まる。その種別の機能設計書は、この形に従う

### この決定で決めないもの

- 1 つの出力に別々の要素の配列が複数ある操作(IAM の `GetAccountAuthorizationDetails` など)をリソースの列挙に使う場合の値の形。この形のまま当てはめると、ほかの配列の要素をすべて各リソースに写すことになるため、そうした操作を使うコレクターを設計するときに決める([リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の未決事項)
- アカウントに 1 つの操作の出力の要素を、リソースに 1 対 1 で対応づけるもの(IAM の認証情報レポート)の値の形。これまでどおり、その種別の機能設計書で決める

## 検討した選択肢

### 最上位の項目の入れ方

| 選択肢 | 判断 |
| --- | --- |
| 出力の形を保ち、要素の配列をそのリソースの要素 1 つだけにする(採用) | 要素の項目と最上位の項目の名前が衝突しない。値は API の出力をそのまま切り出したものになり、AWS のドキュメントや `aws` CLI の出力と同じ道筋で項目をたどれる。入れ子の配列(`DescribeInstances` の予約)にも、同じ規則で答えが出る。ルールの参照に添字 `[0]` が要り、少し長くなる |
| 最上位の項目を、同じ名前で要素の中に混ぜる(例: `{"Name": "...", "CreationDate": "...", "Owner": {...}}`) | 採らない。要素と最上位に同じ名前の項目がある操作では衝突し、操作ごとの例外の規則が要る。値が API の出力のどこにも無い形になり、項目がどこから来たかが値から分からない。入れ子の配列で、どの階層の項目かも区別できない |
| 要素の値はそのままにし、最上位の項目を別の名前のキー(例: `ListBuckets` と並ぶ別のキー)に入れる | 採らない。構成情報のキーは操作名という規則が崩れ、yata の決めた名前が増える |
| 要素だけを入れる形のまま(最上位の項目を入れない) | 採らない。`Owner` や予約の項目を黙って落とし、「すべて入れる」方針と食い違う |

### 形をそろえる範囲

| 選択肢 | 判断 |
| --- | --- |
| すべての列挙の操作で同じ形にする(採用) | 値の形が操作によって変わらず、ルールの作者もコレクターも 1 つの規則だけを覚えればよい。SDK に最上位の項目が後から増えても、項目の追加で済み、値の形は変わらない |
| 最上位に要素の配列以外の項目がある操作だけ、出力の形にする | 採らない。操作ごとに形が違い、どちらの形かを機能設計書ごとに確かめることになる。SDK の更新で最上位の項目が増えると、要素だけの形から出力の形に変わり、互換性の無い変更になる |

### 要求の値の写し

| 選択肢 | 判断 |
| --- | --- |
| ページングの項目と応答のメタデータのほかは、すべて入れる(採用) | 「すべて入れる」の規則が単純なまま保てる。どの項目が写しかをコレクターが判断しない |
| 要求の値の写し(`Prefix` など)も除く | 採らない。既定のパラメーターで呼ぶため、写しは通常 `null` で情報は少ないが、除く項目を操作ごとに見分ける規則が増える |

ページの大きさの写し(`MaxItems` など)とページの件数(`KeyCount` など)は、要求の値の写しやページに関わる項目ではあるが、ページングの項目として除く。ページの大きさはコレクターがページングのために決める値で、これらの値はページの分け方だけで変わり、リソースの情報を持たないため。入れると、同じリソースでもページの大きさや何ページ目かで値が変わり、1 つのリソースの情報を取る操作では全ページで同じ値という前提(上の「1 つのリソースの情報を取る操作のページ」)も崩れる。

### ページごとの値

| 選択肢 | 判断 |
| --- | --- |
| 各リソースに、その要素を返したページの値を入れる(採用) | 値は、その要素と同じ応答から来たもので、置き換えが無い。ページによって値が違っても、そのまま残る |
| 最初のページの値をすべてのリソースに入れる | 採らない。ほかのページの値を黙って置き換えることになる |
| 全ページの値を配列にまとめる | 採らない。出力の形が変わり、ページの分け方で値が変わる |

## 結果

- [リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「コレクターの方針」「格納の形」「列挙の操作の値」「全ページをつないだ出力」「ページングの項目と応答のメタデータ」で、この決定を定める。同ページの JSON の例(`ListBuckets`、`DescribeSecurityGroups`)も、この形で示す
- [テスト戦略](../architecture/Architecture-Testing-Strategy.ja.md) の「コレクターのテスト」で、この形を確かめる
- ルールは、列挙の操作の項目を添字 `[0]` で参照する(例: `input.configuration.ListBuckets.Buckets[0].BucketRegion`)。`reason.path` も添字を含む(例: `/configuration/ListBuckets/Buckets/0/BucketRegion`)
- この変更は、[リソースモデル](../architecture/Architecture-Resource-Model.ja.md) の「構成情報の互換性」では互換性の無い変更(型の変更)に当たる。列挙の操作の値を持つ既存の種別(`aws.iam.user`、`aws.iam.policy`)は、まだリリースに含めていないため、新しい種別名を作らずに、同じ種別名のまま形を変える
- 既存のコレクター、その期待するファイル、機能設計書と図のうち、要素だけの形を前提にしたものは、後続の Issue でこの形に合わせる
- 図: [列挙の操作の値の切り分け](../diagrams/Diagram-0039-Listing-Output-Slicing.md)

## 出典

- S3 の `ListBuckets`(出力の項目と、`Owner` が「一覧にしたバケットの所有者」、`Prefix` が要求で指定したときに返ること): <https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListBuckets.html>
- IAM の `ListUsers`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_ListUsers.html>
- IAM の `ListPolicies`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_ListPolicies.html>
- EC2 の `DescribeSecurityGroups`: <https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_DescribeSecurityGroups.html>
- EC2 の `DescribeInstances` と予約(`Reservation`)の項目: <https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_DescribeInstances.html>、<https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_Reservation.html>
- IAM の `GetAccountAuthorizationDetails`: <https://docs.aws.amazon.com/IAM/latest/APIReference/API_GetAccountAuthorizationDetails.html>
