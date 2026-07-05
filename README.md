# num_gpu_optimize

ollamaモデルの`num_gpu`パラメータを最適化するためのRustツールです。

## 概要

このツールは、ollamaモデルのGPUレイヤー数（`num_gpu`）を0から99まで5刻みで変化させながら実行し、各設定でのトークン生成速度とリソース使用率を計測します。これにより、お使いのPC環境に最適な`num_gpu`値を見つけることができます。 実行してみるとわかるとおり、ollama で igpu だと num_gpu は意味をもたないことがわかります。 AMD の Lemonade だと igpu を認識して実行し 6 〜 8 tokens/sec くらいのスペックです。
このコードでは、ollama create する際の Modelsfile に記述した num_gpu を 0 から 99 の範囲で +5 しながら ollama run を繰り返して実行し、
--verbose を表示させます。

## 目的

- Ryzen 5 2400G（16GBメモリ）などのAPU環境で、ollamaモデルの最適なGPUレイヤー数を特定する
- CPU/GPU/メモリの使用率とトークン生成速度の関係を分析する
- CSV形式で計測結果を出力し、データ分析を容易にする

## 前提条件

- **Rust**: バージョン1.96.1（`rust-toolchain.toml`で指定）
- **ollama**: インストール済みであること
- **モデルファイル**: `Bonsai-8B.gguf` がプロジェクトディレクトリに存在すること
- **AMD GPU**: `/sys/class/drm/card1/device/gpu_busy_percent` からGPU使用率を取得可能な環境

## インストール

1. リポジトリをクローンまたはダウンロード
2. Rustツールチェーンが自動的に1.96.1に設定されます
3. 依存クレートをビルド：

```bash
cargo build --release
```

## 使用方法

### 基本的な実行

```bash
cargo run --release
```

### 実行内容

プログラムは以下の手順で実行されます：

1. **CSVファイルの初期化**
   - `log.csv`: num_gpuとトークン/秒の記録
   - `resource_log.csv`: タイムスタンプ、CPU使用率、メモリ使用量、GPU使用率の記録

2. **num_gpuループ（0から99まで5刻み）**
   - 既存モデルの削除（`ollama rm`）
   - Modelfileの`num_gpu`パラメータを変更
   - モデル作成（`ollama create`）
   - リターンコードが0の場合のみ次へ進む

3. **リソース監視開始**
   - バックグラウンドスレッドで1秒ごとにCPU/GPU/メモリ使用率を記録

4. **ollama実行**
   - CLIモードで実行：`ollama run Bonsai-8B --verbose "n! を計算するJavaコードを見せて"`
   - リターンコードが0でない場合はエラーとして処理
   - stdoutから`eval rate`を解析してトークン/秒を取得

5. **結果記録**
   - `log.csv`にnum_gpuとトークン/秒を書き込み
   - リソース監視を停止

6. **ループ継続**
   - 次のnum_gpu値で同様の処理を繰り返す

7. **終了処理**
   - 一時ファイル（`Modelfile.tmp`）を削除

## 出力ファイル

### log.csv

num_gpuとトークン生成速度の記録：

```csv
num_gpu,tokens_per_sec
0,5.04
5,8.32
10,12.45
...
```

### resource_log.csv

リソース使用率の時系列データ：

```csv
timestamp,cpu_usage,mem_used_gb,gpu_usage
12:34:56,85.2,12.45,78
12:34:57,87.1,12.50,82
...
```

### pipe_comm.log

デバッグ用の詳細ログ（時刻付き）：

```
[12:34:56] --- CLI実行開始: Bonsai-8B ---
[12:34:57] [EVENT] ollama run成功、出力解析開始
[12:35:00] [EVENT] eval rate検出: 5.04
[12:35:01] --- CLI実行完了: tokens/sec = 5.04 ---
```

## Modelfile

プロジェクトには以下のModelfileが含まれています：

```dockerfile
FROM Bonsai-8B.gguf

# パラメータ設定
PARAMETER num_gpu 0
PARAMETER num_predict 256
PARAMETER temperature 0.5
PARAMETER top_p 0.85
PARAMETER top_k 20

# システムプロンプト
SYSTEM "あなたは経験豊富で優秀なコーディング能力を有するソフトウェア開発支援AIです"
```

`num_gpu`パラメータはプログラム実行中に自動的に変更されます。

## 依存クレート

- **sysinfo**: システム情報（CPU、メモリ、GPU使用率など）を取得
- **chrono**: 日時処理（タイムスタンプ生成）
- **regex**: 正規表現（eval rateの解析）

## 注意事項

- モデル作成と実行に時間がかかる場合があります（特にnum_gpuが大きい場合）
- GPU使用率の取得はAMD GPUのsysfs経由で行っています（`/sys/class/drm/card1/device/gpu_busy_percent`）
- NVIDIA GPUの場合は`read_gpu_usage`関数を修正して`nvidia-smi`を使用するように変更してください
- モデルファイル（`Bonsai-8B.gguf`）はプロジェクトディレクトリに配置してください

## ライセンス

MIT License

## 実装経緯

当初、Rustでollamaをpipeを使った対話型で実装しようとしましたが、ollamaのプロンプト記号「>>>」を確実にキャッチすることができず、タイミングの問題で安定した動作が得られませんでした。そのため、CLIモードでプロンプトを直接指定する方式に変更しました。

- **対話型（当初の実装）**: `ollama run model --verbose` を実行し、stdin/stdoutをパイプして「>>>」を検出→プロンプト送信→「>>> Send a message」を検出→`/bye`送信
- **CLIモード（現在の実装）**: `ollama run model --verbose "プロンプト"` を直接実行し、リターンコードが0の場合のみ処理を継続

CLIモードへの変更により、以下のメリットが得られました：
- プロンプト検出のタイミング問題が解消
- リターンコードチェックによるエラーハンドリングが容易
- コードがシンプルになり、保守性が向上

## 貢献

バグ報告や機能改善の提案は歓迎します。
