mod braille_graph;
mod display;
mod event_recorder;
mod lpgm_calculator;
mod sensor;
mod sindo_calculator;
mod util;

use display::{DisplayData, LpgmData, RealTimeDisplay};
use event_recorder::EventRecorder;
use lpgm_calculator::LpgmCalculator;
use sensor::SensorManager;
use sindo_calculator::SindoCalculator;
use util::Acceleration3D;

use clap::{Arg, Command};
use futures::future::join;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use std::{env, thread};

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::new("i2c-path")
                .short('i')
                .long("i2c-path")
                .env("I2C_PATH")
                .value_name("PATH")
                .help("I2Cパス")
                .default_value("/dev/i2c-6"),
        )
        .arg(
            Arg::new("i2c-address")
                .short('a')
                .long("i2c-address")
                .env("I2C_ADDR")
                .value_name("ADDRESS")
                .help("I2Cアドレス")
                .default_value("6a"),
        )
        .arg(
            Arg::new("buffer")
                .short('b')
                .long("buffer")
                .env("BUFFER")
                .value_name("SECONDS")
                .help("データバッファ秒数")
                .default_value("30"),
        )
        .arg(
            Arg::new("site-code")
                .long("site-code")
                .env("SITE_CODE")
                .value_name("CODE")
                .help("観測点コード")
                .default_value("SNJK01"),
        )
        .arg(
            Arg::new("site-name")
                .long("site-name")
                .env("SITE_NAME")
                .value_name("NAME")
                .help("観測点名")
                .default_value("東京都渋谷区道玄坂"),
        )
        .arg(
            Arg::new("latitude")
                .long("latitude")
                .env("LATITUDE")
                .value_name("LAT")
                .help("緯度")
                .default_value("35.6585"),
        )
        .arg(
            Arg::new("longitude")
                .long("longitude")
                .env("LONGITUDE")
                .value_name("LON")
                .help("経度")
                .default_value("139.7013"),
        )
        .arg(
            Arg::new("event-output-dir")
                .long("event-output-dir")
                .env("EVENT_OUTPUT_DIR")
                .value_name("DIR")
                .help("地震イベントCSVファイルの出力ディレクトリ")
                .default_value("./exports"),
        )
        .arg(
            Arg::new("sindo-threshold")
                .long("sindo-threshold")
                .env("SINDO_THRESHOLD")
                .value_name("THRESHOLD")
                .help("震度検知閾値（この値以上でCSV出力を開始）")
                .default_value("4.5"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .env("VERBOSE")
                .help("DEBUGログを表示")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let i2c_path = matches.get_one::<String>("i2c-path").unwrap();
    let i2c_address = matches.get_one::<String>("i2c-address").unwrap();
    let buffer_duration: f32 = matches.get_one::<String>("buffer").unwrap().parse()?;
    let site_code = matches.get_one::<String>("site-code").unwrap();
    let site_name = matches.get_one::<String>("site-name").unwrap();
    let latitude: f32 = matches.get_one::<String>("latitude").unwrap().parse()?;
    let longitude: f32 = matches.get_one::<String>("longitude").unwrap().parse()?;
    let sindo_threshold: f32 = matches
        .get_one::<String>("sindo-threshold")
        .unwrap()
        .parse()?;
    let event_output_dir = matches.get_one::<String>("event-output-dir").unwrap();

    // I2Cアドレスを16進数から変換
    let address = u8::from_str_radix(i2c_address, 16)?;

    println!("ISM330DHCX 震度計測システムを初期化中...");
    println!("I2Cパス: {i2c_path}");
    println!("I2Cアドレス: 0x{address:02x}");

    // センサー初期化（手動キャリブレーションが指定されている場合は自動キャリブレーションをスキップ）
    let mut sensor =
        SensorManager::new_with_auto_calibrate(i2c_path, address, matches.get_flag("verbose"))?;
    let sample_rate = sensor.get_sample_rate();

    println!("サンプリングレート: {sample_rate:.1} Hz");
    println!("震度検知閾値: {sindo_threshold:.1}");
    println!("イベント出力ディレクトリ: {event_output_dir}");

    // 震度計算器初期化
    let mut sindo_calculator = SindoCalculator::new(
        sample_rate,
        buffer_duration,
        false, // data_is_in_gal = false (センサーデータはG単位)
        matches.get_flag("verbose"),
    );

    // 長周期地震動計算器初期化
    let mut lpgm_calculator = LpgmCalculator::new(
        sample_rate,
        buffer_duration,
        false, // data_is_in_gal = false (センサーデータはG単位)
        matches.get_flag("verbose"),
    );

    // イベント記録器初期化
    let mut event_recorder = EventRecorder::new(
        sindo_threshold,
        event_output_dir,
        site_code,
        site_name,
        latitude,
        longitude,
        sample_rate,
    )?;

    // システム開始メッセージ（raw mode前に表示）
    println!("システム開始中...");
    thread::sleep(Duration::from_secs(1));

    // 表示システム初期化（raw mode開始）
    let mut display = RealTimeDisplay::new();
    display.init_realtime_graph(buffer_duration, sample_rate);
    RealTimeDisplay::init()?;

    // メインループ
    let result = run_main_loop(
        &mut sensor,
        &mut sindo_calculator,
        &mut lpgm_calculator,
        &mut display,
        &mut event_recorder,
        matches.get_flag("verbose"),
    )
    .await;

    // クリーンアップ
    RealTimeDisplay::cleanup()?;

    match result {
        Ok(_) => println!(" システムを正常終了しました。"),
        Err(e) => {
            eprintln!("エラーが発生しました: {e}");
            return Err(e);
        }
    }

    Ok(())
}

async fn run_main_loop(
    sensor: &mut SensorManager,
    sindo_calculator: &mut SindoCalculator,
    lpgm_calculator: &mut LpgmCalculator,
    display: &mut RealTimeDisplay,
    event_recorder: &mut EventRecorder,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut latest_accel_data: Option<Acceleration3D>;
    let mut sample_count = 0u32;
    let calc_interval = 10; // 計算を10サンプルに1回（10Hz）に大幅削減
    let display_interval = 10; // 画面更新を10サンプルに1回（10Hz）に削減

    // 表示用に最新の計算結果を保持
    let mut latest_sindo: Option<f32> = None;
    let mut latest_lpgm_data: Option<LpgmData> = None;

    // センサー接続状態をキャッシュ
    let mut cached_connection_status = (true, 0u32);
    let mut last_status_check = 0u32;

    loop {
        // センサー接続状態のチェック（100サンプルに1回に大幅削減）
        if sample_count - last_status_check >= 100 {
            cached_connection_status = sensor.get_connection_status();
            last_status_check = sample_count;

            let (is_connected, failed_count) = cached_connection_status;
            if !is_connected && failed_count > 0 && verbose {
                eprintln!("センサー接続エラー (失敗回数: {failed_count})");
            }
        }

        // センサーデータ読み取り
        match sensor.read_acceleration().await? {
            Some(accel_data) => {
                // 最新の加速度データを保存
                latest_accel_data = Some(accel_data);

                // デバッグ情報（verboseモード時）
                if verbose && sample_count.is_multiple_of(100) {
                    static DEBUG_COUNTER: AtomicU32 = AtomicU32::new(0);
                    let count = DEBUG_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
                    println!(
                        "DEBUG[{}]: 加速度データ取得成功 - X:{:.6}g, Y:{:.6}g, Z:{:.6}g",
                        count, accel_data.x, accel_data.y, accel_data.z
                    );
                }

                // 震度計算器にデータ追加
                sindo_calculator.add_sample(accel_data);

                // 長周期地震動計算器にデータ追加
                lpgm_calculator.add_sample(accel_data);
            }
            None => {
                // センサーからデータが取得できない場合
                if verbose && sample_count.is_multiple_of(100) {
                    println!("DEBUG: センサーからデータが取得できません");
                }
                continue; // データがない場合は次のループへ
            }
        };

        // サンプルカウントは常に増加（データ欠損があっても時間は進む）
        sample_count += 1;

        // 震度計算と長周期地震動計算を並列実行
        if sample_count.is_multiple_of(calc_interval) {
            // 震度計算と長周期地震動計算を並列で実行
            let (calculated_sindo, calculated_lpgm) = join(
                sindo_calculator.calculate_sindo(),
                lpgm_calculator.calculate_lpgm(),
            )
            .await;

            if calculated_sindo.is_some() {
                latest_sindo = calculated_sindo; // 最新値を保持
            }

            if let Some(level) = calculated_lpgm {
                let data = LpgmData {
                    level,
                    max_sva_30s: lpgm_calculator.get_max_sva_30s(),
                };
                latest_lpgm_data = Some(data); // 最新値を保持
            }
        }

        // イベント記録処理（計算時のみ）
        if let Some(ref accel) = latest_accel_data {
            event_recorder.process_sample(*accel, latest_sindo)?;
        }

        // 画面更新（display_intervalサンプルに1回のみ）
        if sample_count.is_multiple_of(display_interval) {
            let display_data = DisplayData {
                sindo: latest_sindo, // 最新の震度値を使用
                acceleration: latest_accel_data,
                lpgm_data: latest_lpgm_data, // 最新の長周期地震動データを使用
                buffer_size: sindo_calculator.buffer_len(),
                is_connected: cached_connection_status.0,
                failed_count: cached_connection_status.1,
            };

            let continue_running = display
                .update_with_acceleration(&display_data, event_recorder)
                .await?;

            if !continue_running {
                break;
            }
        }
    }

    Ok(())
}
