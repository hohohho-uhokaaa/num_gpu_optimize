// 正規表現ライブラリ（eval rateの解析に使用）
use regex::Regex;
// ファイル操作関連のインポート
use std::fs::{self, OpenOptions};
// 書き込み操作のインポート
use std::io::Write;
// プロセス実行関連のインポート
use std::process::{Command, Stdio};
// スレッド同期関連のインポート
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
// スレッド、時間関連のインポート
use std::thread;
use std::time::Duration;
// システム情報取得ライブラリ
use sysinfo::System;

// モデル名定数
const MODEL_NAME: &str = "Bonsai-8B";
// Modelfileのパス
const MODELFILE_PATH: &str = "./Modelfile";

/// 時刻付きでログファイルに書き込むヘルパー関数
/// msg: ログメッセージ
fn log_to_pipe(msg: &str) {
    if let Ok(mut log_file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("pipe_comm.log")
    {
        let ts = chrono::Local::now().format("%H:%M:%S");
        let _ = writeln!(log_file, "[{}] {}", ts, msg);
    }
}

/// メイン関数
/// num_gpuを0から99まで5刻みで変化させながらollamaを実行し、
/// トークン生成速度とリソース使用率を計測する
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CSVファイルの初期化（トークン速度ログ）
    let mut log_csv = std::fs::File::create("log.csv")?;
    writeln!(log_csv, "num_gpu,tokens_per_sec")?;

    // num_gpuを0から99まで5刻みでループ
    for num_gpu in (0..=99).step_by(5) {
        println!("--- テスト開始: num_gpu = {} ---", num_gpu);

        // 既存モデルを削除
        let _ = Command::new("ollama")
            .args(["rm", MODEL_NAME])
            .stdout(Stdio::null())
            .status();

        // Modelfileのnum_gpu値を変更
        let modelfile_content = modify_modelfile(num_gpu)?;
        fs::write("Modelfile.tmp", &modelfile_content)?;

        // モデル作成（リターンコードチェック）
        if !Command::new("ollama")
            .args(["create", MODEL_NAME, "-f", "Modelfile.tmp"])
            .status()?
            .success()
        {
            eprintln!("モデル作成失敗: num_gpu {}", num_gpu);
            continue;
        }

        // バックグラウンドスレッドでリソース監視を開始
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let monitor_handle = thread::spawn(move || monitor_resources(running_clone));

        // ollamaをCLIモードで実行
        match run_ollama_cli(MODEL_NAME) {
            Ok(tokens_per_sec) => {
                // 結果をCSVに書き込み
                writeln!(log_csv, "{},{}", num_gpu, tokens_per_sec)?;
                println!("  完了: {} tokens/sec", tokens_per_sec);
            }
            Err(e) => eprintln!("  計測エラー: {}", e),
        }

        // リソース監視を停止
        running.store(false, Ordering::SeqCst);
        let _ = monitor_handle.join();
    }

    // 一時ファイルの削除
    let _ = fs::remove_file("Modelfile.tmp");
    Ok(())
}

/// ollamaをCLIモードで実行し、トークン生成速度を計測する関数
/// model_name: 実行するモデル名
/// 戻り値: トークン/秒
fn run_ollama_cli(model_name: &str) -> Result<String, Box<dyn std::error::Error>> {
    log_to_pipe(&format!("--- CLI実行開始: {} ---", model_name));

    // ollama runコマンドをプロンプト付きで実行
    let result = Command::new("ollama")
        .args([
            "run",
            model_name,
            "--verbose",
            "n! を計算するJavaコードを見せて",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    // return codeが0でない場合はエラー
    if !result.status.success() {
        let exit_code = result.status.code().unwrap_or(-1);
        log_to_pipe(&format!("[ERROR] ollama run失敗: exit code {}", exit_code));
        return Err(format!("ollama run failed with exit code: {}", exit_code).into());
    }

    log_to_pipe("[EVENT] ollama run成功、出力解析開始");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);

    // stdoutを表示
    println!("{}", stdout);
    eprintln!("{}", stderr);

    // stdoutからeval rateを解析（正規表現使用）
    let mut tokens_per_sec = "0.0".to_string();
    let re_rate = Regex::new(r"eval rate:\s+([\d\.]+)").expect("Regex fail");

    for line in stdout.lines() {
        log_to_pipe(&format!("[OUT] {}", line));
        if let Some(caps) = re_rate.captures(line) {
            tokens_per_sec = caps[1].to_string();
            log_to_pipe(&format!("[EVENT] eval rate検出: {}", tokens_per_sec));
        }
    }

    log_to_pipe(&format!(
        "--- CLI実行完了: tokens/sec = {} ---",
        tokens_per_sec
    ));
    Ok(tokens_per_sec)
}

/// リソース使用率を監視し、CSVに記録する関数
/// running: 監視継続フラグ
fn monitor_resources(running: Arc<AtomicBool>) {
    let mut sys = System::new_all();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("resource_log.csv")
        .unwrap();

    // runningがtrueの間、1秒ごとにリソース使用率を記録
    while running.load(Ordering::SeqCst) {
        sys.refresh_cpu_all();
        sys.refresh_memory();

        // CPU使用率を取得
        let cpu = sys.global_cpu_usage();
        // メモリ使用量をGB単位で計算
        let mem = sys.used_memory() as f64 / 1073741824.0;
        // GPU使用率を取得
        let gpu = read_gpu_usage();

        // タイムスタンプ付きでCSVに記録
        let ts = chrono::Local::now().format("%H:%M:%S");
        let _ = writeln!(file, "{},{:.1},{:.2},{}", ts, cpu, mem, gpu);
        thread::sleep(Duration::from_secs(1));
    }
}

/// GPU使用率を取得する関数
/// AMD GPUのsysfs経由で使用率を取得
/// 戻り値: GPU使用率（文字列）
fn read_gpu_usage() -> String {
    fs::read_to_string("/sys/class/drm/card1/device/gpu_busy_percent")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

/// Modelfileのnum_gpuパラメータを変更する関数
/// num_gpu: 設定するGPUレイヤー数
/// 戻り値: 変更後のModelfile内容
fn modify_modelfile(num_gpu: i32) -> Result<String, std::io::Error> {
    let content = fs::read_to_string(MODELFILE_PATH)?;
    // Modelfileを行ごとに読み込み、num_gpu行を置換
    Ok(content
        .lines()
        .map(|l| {
            if l.starts_with("PARAMETER num_gpu") {
                format!("PARAMETER num_gpu {}", num_gpu)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
