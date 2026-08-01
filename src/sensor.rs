use crate::util::Acceleration3D;

use async_std::stream::StreamExt;
use async_std::task;
use ism330dhcx::Ism330Dhcx;
use linux_embedded_hal::I2cdev;
use std::collections::VecDeque;
use std::error::Error;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// I2Cコネクションプール
struct I2cConnectionPool {
    connections: Arc<Mutex<VecDeque<I2cdev>>>,
    i2c_path: String,
    i2c_address: u8,
    max_connections: usize,
}

impl I2cConnectionPool {
    fn new(i2c_path: String, i2c_address: u8, max_connections: usize) -> Self {
        Self {
            connections: Arc::new(Mutex::new(VecDeque::new())),
            i2c_path,
            i2c_address,
            max_connections,
        }
    }

    async fn get_connection(&self) -> Result<I2cdev, Box<dyn Error>> {
        // プールから既存のコネクションを取得を試行
        if let Ok(mut pool) = self.connections.lock()
            && let Some(conn) = pool.pop_front()
        {
            // 既存のコネクションをそのまま返す（検証をスキップして高速化）
            return Ok(conn);
        }

        // 新しいコネクションを作成
        let mut i2c = I2cdev::new(&self.i2c_path)?;
        i2c.set_slave_address(self.i2c_address as u16)?;

        Ok(i2c)
    }

    async fn return_connection(&self, connection: I2cdev) {
        if let Ok(mut pool) = self.connections.lock()
            && pool.len() < self.max_connections
        {
            pool.push_back(connection);
        }
    }
}

// 一時的なセンサー管理構造体（初期化用）
struct TempSensorManager {
    sensor: Arc<Mutex<Option<Ism330Dhcx>>>,
    i2c_path: String,
    i2c_address: u8,
}

pub struct SensorManager {
    sample_rate: f32,                                     // 出力サンプリングレート
    connection_failed_count: Arc<AtomicU32>,              // 接続失敗回数（スレッド間共有）
    last_successful_read: Arc<Mutex<Option<SystemTime>>>, // 最後の成功読み取り時刻
    data_receiver: async_std::channel::Receiver<Option<Acceleration3D>>,
    _data_task_handle: task::JoinHandle<()>,
    _i2c_pool: Arc<I2cConnectionPool>, // I2Cコネクションプール
}

impl SensorManager {
    pub fn new_with_auto_calibrate(
        i2c_bus: &str,
        address: u8,
        verbose: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let target_sample_rate = 100.0f32;
        let sensor = Arc::new(Mutex::new(None));
        let i2c_path = i2c_bus.to_string();

        // チャンネルを作成してデータ取得タスクと通信
        let (sender, receiver) = async_std::channel::bounded(10); // バッファサイズを増やして処理遅延を許容

        // センサー初期化
        let mut temp_manager = TempSensorManager {
            sensor: sensor.clone(),
            i2c_path: i2c_path.clone(),
            i2c_address: address,
        };
        temp_manager.connect_sensor()?;

        // 接続失敗カウンターと最後の成功読み取り時刻を共有
        let connection_failed_count = Arc::new(AtomicU32::new(0));
        let last_successful_read = Arc::new(Mutex::new(Some(SystemTime::now())));

        // I2Cコネクションプールを作成
        let i2c_pool = Arc::new(I2cConnectionPool::new(i2c_path.clone(), address, 1));

        // RTCベースの100Hzデータ取得タスクを開始
        let data_task_handle = task::spawn(Self::rtc_data_acquisition_task(
            sensor.clone(),
            i2c_pool.clone(),
            sender,
            receiver.clone(),
            connection_failed_count.clone(),
            last_successful_read.clone(),
            verbose,
        ));

        let manager = Self {
            sample_rate: target_sample_rate,
            connection_failed_count,
            last_successful_read,
            data_receiver: receiver,
            _data_task_handle: data_task_handle,
            _i2c_pool: i2c_pool,
        };

        Ok(manager)
    }

    // 100Hzのデータ取得タスク
    async fn rtc_data_acquisition_task(
        sensor: Arc<Mutex<Option<Ism330Dhcx>>>,
        i2c_pool: Arc<I2cConnectionPool>,
        sender: async_std::channel::Sender<Option<Acceleration3D>>,
        receiver: async_std::channel::Receiver<Option<Acceleration3D>>,
        connection_failed_count: Arc<AtomicU32>,
        last_successful_read: Arc<Mutex<Option<SystemTime>>>,
        verbose: bool,
    ) {
        let mut interval = async_std::stream::interval(Duration::from_millis(10));

        while (interval.next().await).is_some() {
            // 直接センサーデータを読み取り
            let (data, is_success) = Self::read_sensor_data_internal(
                &sensor,
                &i2c_pool,
                &connection_failed_count,
                &last_successful_read,
                verbose,
            )
            .await;

            // 成功時のみlast_successful_readを更新
            if is_success && let Ok(mut last_read) = last_successful_read.lock() {
                *last_read = Some(SystemTime::now());
            }

            // データ送信
            if let Some(accel_data) = data {
                // バッファが満杯の場合は古いデータを破棄して最新データを送信
                match sender.try_send(Some(accel_data)) {
                    Ok(_) => {
                        // 送信成功
                    }
                    Err(async_std::channel::TrySendError::Full(_)) => {
                        // バッファが満杯の場合、古いデータをすべて破棄
                        while receiver.try_recv().is_ok() {
                            // 古いデータを破棄
                        }
                        // 最新データを送信
                        let _ = sender.try_send(Some(accel_data));
                        if verbose {
                            eprintln!("古いデータを破棄して最新データを送信");
                        }
                    }
                    Err(_) => {
                        // チャンネルが閉じられている場合
                        break;
                    }
                }
            }
        }
    }

    // 内部的なセンサーデータ読み取り関数
    async fn read_sensor_data_internal(
        sensor: &Arc<Mutex<Option<Ism330Dhcx>>>,
        i2c_pool: &Arc<I2cConnectionPool>,
        connection_failed_count: &Arc<AtomicU32>,
        _last_successful_read: &Arc<Mutex<Option<SystemTime>>>,
        verbose: bool,
    ) -> (Option<Acceleration3D>, bool) {
        // プールからI2Cコネクションを取得
        let mut i2c = match i2c_pool.get_connection().await {
            Ok(conn) => conn,
            Err(e) => {
                connection_failed_count.fetch_add(1, Ordering::Relaxed);
                if verbose {
                    eprintln!("I2Cコネクション取得エラー: {e}");
                }
                return (Self::create_zero_data(), false);
            }
        };

        // センサーからデータを読み取り
        let result = {
            let mut sensor_guard = sensor.lock().unwrap();
            if let Some(ref mut sensor_instance) = *sensor_guard {
                match sensor_instance.get_accelerometer(&mut i2c) {
                    Ok(accel_data) => {
                        let g_values = accel_data.as_g();
                        let accel_x = g_values[0] as f32;
                        let accel_y = g_values[1] as f32;
                        let accel_z = g_values[2] as f32;

                        // データが全て0の場合は失敗とみなす（センサーが正常に動作していない可能性）
                        if accel_x == 0.0 && accel_y == 0.0 && accel_z == 0.0 {
                            connection_failed_count.fetch_add(1, Ordering::Relaxed);
                            if verbose {
                                eprintln!(
                                    "センサーから全て0のデータが返されました（センサー異常の可能性）"
                                );
                            }
                            return (Self::create_zero_data(), false);
                        }

                        // 成功時は失敗カウンターをリセット
                        connection_failed_count.store(0, Ordering::Relaxed);

                        (
                            Some(Acceleration3D {
                                x: accel_x,
                                y: accel_y,
                                z: accel_z,
                                timestamp: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default(),
                            }),
                            true,
                        )
                    }
                    Err(e) => {
                        connection_failed_count.fetch_add(1, Ordering::Relaxed);
                        if verbose {
                            eprintln!("センサー読み取りエラー: {e}");
                        }

                        // 連続して失敗が続く場合はセンサーの再初期化を試行
                        let failed_count = connection_failed_count.load(Ordering::Relaxed);
                        if failed_count > 100 && failed_count.is_multiple_of(50) {
                            if verbose {
                                eprintln!("センサー再初期化を試行中... (失敗回数: {failed_count})");
                            }
                            // センサーを再初期化
                            if let Err(reinit_error) = Self::reinitialize_sensor(&mut i2c) {
                                if verbose {
                                    eprintln!("センサー再初期化失敗: {reinit_error}");
                                }
                            } else if verbose {
                                eprintln!("センサー再初期化完了");
                            }
                        }

                        (Self::create_zero_data(), false)
                    }
                }
            } else {
                connection_failed_count.fetch_add(1, Ordering::Relaxed);
                if verbose {
                    eprintln!("センサーインスタンスが初期化されていません");
                }
                (Self::create_zero_data(), false)
            }
        };

        // コネクションをプールに戻す（成功時のみ）
        if result.1 {
            i2c_pool.return_connection(i2c).await;
        }

        result
    }

    // ゼロデータ作成のヘルパー関数
    fn create_zero_data() -> Option<Acceleration3D> {
        Some(Acceleration3D {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default(),
        })
    }

    pub async fn read_acceleration(&mut self) -> Result<Option<Acceleration3D>, Box<dyn Error>> {
        // 単純にバッファから次のデータを取得
        match self.data_receiver.try_recv() {
            Ok(data) => Ok(data),
            Err(async_std::channel::TryRecvError::Empty) => {
                // バッファが空の場合、新しいデータを待機
                match async_std::future::timeout(
                    Duration::from_millis(10),
                    self.data_receiver.recv(),
                )
                .await
                {
                    Ok(Ok(data)) => Ok(data),
                    Ok(Err(_)) => Ok(None), // チャンネルが閉じられた場合
                    Err(_) => Ok(None),     // タイムアウト
                }
            }
            Err(_) => Ok(None),
        }
    }

    pub fn get_sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn get_connection_status(&self) -> (bool, u32) {
        let failed_count = self.connection_failed_count.load(Ordering::Relaxed);

        // 最後の成功読み取りから5秒以上経過している場合は切断とみなす
        let is_connected = if let Ok(last_read_guard) = self.last_successful_read.lock() {
            if let Some(last_read_time) = *last_read_guard {
                let elapsed = SystemTime::now()
                    .duration_since(last_read_time)
                    .unwrap_or(Duration::from_secs(1000));
                elapsed < Duration::from_secs(1) && failed_count < 100
            } else {
                false
            }
        } else {
            false
        };

        (is_connected, failed_count)
    }

    // レジスタ設定のヘルパー関数
    fn write_register(
        i2c: &mut I2cdev,
        register: u8,
        value: u8,
        register_name: &str,
    ) -> Result<(), Box<dyn Error>> {
        use linux_embedded_hal::i2cdev::core::I2CDevice;

        match i2c.smbus_write_byte_data(register, value) {
            Ok(_) => {
                println!("{register_name}設定完了: 0x{value:02x}");

                match i2c.smbus_read_byte_data(register) {
                    Ok(read_value) => {
                        println!("{register_name}読み取り値: 0x{read_value:02x}");

                        let verification_ok = read_value == value;

                        if verification_ok {
                            println!("センサー設定確認OK");
                        } else {
                            eprintln!(
                                "警告: 設定値が一致しません (期待値: 0x{value:02x}, 実際: 0x{read_value:02x})"
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("{register_name}読み取りエラー: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("{register_name}書き込みエラー: {e}");
                return Err(Box::new(e));
            }
        }
        Ok(())
    }

    // センサーを明示的にアクティブモードに設定（高性能モード + ネイティブ重力補正）
    pub fn activate_sensor_static(i2c: &mut I2cdev) -> Result<(), Box<dyn Error>> {
        // 各レジスタの設定
        Self::write_register(i2c, 0x10, 0b01000000, "CTRL1_XL")?;
        Self::write_register(i2c, 0x15, 0b00000000, "CTRL6_C")?;
        Self::write_register(i2c, 0x17, 0b11100100, "CTRL8_XL")?;

        Ok(())
    }

    // センサーの再初期化関数
    fn reinitialize_sensor(i2c: &mut I2cdev) -> Result<(), Box<dyn Error>> {
        // センサーをリセット
        use linux_embedded_hal::i2cdev::core::I2CDevice;

        // ソフトリセットを実行
        i2c.smbus_write_byte_data(0x12, 0x01)?; // CTRL3_Cレジスタでソフトリセット

        // リセット完了を待機
        std::thread::sleep(std::time::Duration::from_millis(10));

        // センサーを再アクティベート
        Self::activate_sensor_static(i2c)?;

        // 初期化完了を待機
        std::thread::sleep(std::time::Duration::from_millis(10));

        Ok(())
    }
}

impl TempSensorManager {
    pub fn connect_sensor(&mut self) -> Result<(), Box<dyn Error>> {
        let mut sensor_guard = self.sensor.lock().unwrap();

        println!("I2Cデバイスを開いています: {}", self.i2c_path);
        let mut i2c = I2cdev::new(&self.i2c_path).map_err(|e| {
            eprintln!("I2Cデバイスのオープンに失敗: {e}");
            e
        })?;

        println!("I2Cアドレスを設定しています: 0x{:02x}", self.i2c_address);
        i2c.set_slave_address(self.i2c_address as u16)
            .map_err(|e| {
                eprintln!("I2Cアドレスの設定に失敗: {e}");
                e
            })?;

        println!("ISM330DHCXセンサーを初期化しています...");

        let sensor = Ism330Dhcx::new_with_address(&mut i2c, self.i2c_address).map_err(|e| {
            eprintln!("ISM330DHCXの初期化に失敗: {e}");
            eprintln!("ヒント: デバイスの電源とI2C接続を確認してください");
            e
        })?;

        // センサーを明示的にアクティブモードに設定
        println!("センサーをアクティブモードに設定中...");
        if let Err(e) = SensorManager::activate_sensor_static(&mut i2c) {
            eprintln!("センサーアクティベーションエラー: {e}");
        } else {
            println!("センサーアクティベーション完了");
        }

        // センサーの設定を確認・設定
        println!("センサーの設定を確認中...");

        // 初期データ取得テスト
        let mut test_i2c = I2cdev::new(&self.i2c_path)?;
        test_i2c.set_slave_address(self.i2c_address as u16)?;
        let mut test_sensor = sensor;

        // 複数回データ取得を試行して、センサーの動作を確認
        println!("センサーデータ取得テスト中...");
        let mut successful_reads = 0;
        let mut non_zero_reads = 0;

        for attempt in 1..=10 {
            match test_sensor.get_accelerometer(&mut test_i2c) {
                Ok(data) => {
                    successful_reads += 1;
                    let g_values = data.as_g();

                    if attempt <= 3 {
                        println!(
                            "テスト{}回目: x={:.6}, y={:.6}, z={:.6}",
                            attempt, g_values[0], g_values[1], g_values[2]
                        );
                    }

                    // 0以外の値があるかチェック
                    if g_values[0] != 0.0 || g_values[1] != 0.0 || g_values[2] != 0.0 {
                        non_zero_reads += 1;
                    }
                }
                Err(e) => {
                    eprintln!("テスト{attempt}回目エラー: {e}");
                }
            }
        }

        println!("テスト結果: {successful_reads}/10回成功, {non_zero_reads}/10回で非ゼロデータ");

        if successful_reads == 0 {
            eprintln!("エラー: センサーからデータを取得できません");
            eprintln!("I2C通信に問題がある可能性があります");
        } else if non_zero_reads == 0 {
            eprintln!("警告: センサーから全て0のデータが返されています");
            eprintln!("センサーの設定または電源に問題がある可能性があります");
            eprintln!("以下を確認してください:");
            eprintln!("  1. センサーの電源供給（3.3V）");
            eprintln!("  2. センサーの初期化設定");
            eprintln!("  3. センサーのスリープモード状態");
        } else {
            println!("センサーから正常にデータを取得できました");
        }

        *sensor_guard = Some(test_sensor);
        println!("センサー接続成功");
        Ok(())
    }
}
