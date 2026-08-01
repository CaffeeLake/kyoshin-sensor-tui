use crate::braille_graph::RealtimeAccelerationGraph;
use crate::event_recorder::EventRecorder;
use crate::util::{
    Acceleration3D, convert_g_to_gal, get_acceleration_color, get_lpgm_color, get_sindo_color,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, ClearType},
};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct LpgmData {
    pub level: u8,
    pub max_sva_30s: f32,
}

// 表示更新に必要なデータをまとめた構造体
#[derive(Debug, Clone)]
pub struct DisplayData {
    pub sindo: Option<f32>,
    pub acceleration: Option<Acceleration3D>,
    pub lpgm_data: Option<LpgmData>,
    pub buffer_size: usize,
    pub is_connected: bool,
    pub failed_count: u32,
}

// 文字列キャッシュ構造体
#[derive(Default)]
struct CachedStrings {
    header_line: String,
    title: String,
    buffer_text: String,
    status_text: String,
    event_text: String,
}

// 前回の値を保持する構造体
#[derive(Default)]
struct LastValues {
    sindo: Option<f32>,
    acceleration: Option<Acceleration3D>,
    lpgm_data: Option<LpgmData>,
    buffer_size: usize,
    is_connected: bool,
    failed_count: u32,
}

fn get_lpgm_class_name(level: u8) -> &'static str {
    match level {
        0 => "階級0",
        1 => "階級1",
        2 => "階級2",
        3 => "階級3",
        4 => "階級4",
        _ => "不明",
    }
}

pub struct RealTimeDisplay {
    last_update: Instant,
    terminal_width: u16,
    terminal_height: u16,
    // リアルタイムグラフ
    realtime_graph: Option<RealtimeAccelerationGraph>,
    // キャッシュ用フィールド
    cached_strings: CachedStrings,
    last_values: LastValues,
}

impl RealTimeDisplay {
    pub fn new() -> Self {
        let (width, height) = terminal::size().unwrap_or((0, 0));
        Self {
            last_update: Instant::now(),
            terminal_width: width,
            terminal_height: height,
            realtime_graph: None,
            cached_strings: CachedStrings::default(),
            last_values: LastValues::default(),
        }
    }

    pub fn init_realtime_graph(&mut self, buffer_duration: f32, sample_rate: f32) {
        self.realtime_graph = Some(RealtimeAccelerationGraph::new(
            buffer_duration,
            sample_rate,
            0,
            0,
        ));
    }

    pub fn init() -> io::Result<()> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::Clear(ClearType::All), cursor::Hide)?;
        Ok(())
    }

    pub fn cleanup() -> io::Result<()> {
        execute!(io::stdout(), cursor::Show, ResetColor)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    pub async fn update_with_acceleration(
        &mut self,
        data: &DisplayData,
        event_recorder: &EventRecorder,
    ) -> io::Result<bool> {
        self.update_with_full_acceleration_info(data, event_recorder)
            .await
    }

    pub async fn update_with_full_acceleration_info(
        &mut self,
        data: &DisplayData,
        event_recorder: &EventRecorder,
    ) -> io::Result<bool> {
        // キーボード入力チェック
        if event::poll(Duration::from_millis(10))?
            && let Event::Key(key_event) = event::read()?
            && (key_event.code == KeyCode::Esc
                || key_event.code == KeyCode::Char('q')
                || (key_event.modifiers.contains(event::KeyModifiers::CONTROL)
                    && key_event.code == KeyCode::Char('c')))
        {
            return Ok(false);
        }

        self.last_update = Instant::now();

        // ターミナルサイズを更新
        let _size_changed = self.update_terminal_size()?;

        // リアルタイムグラフにデータを追加
        if let (Some(graph), Some(accel)) = (self.realtime_graph.as_ref(), data.acceleration) {
            graph.add_acceleration_sample(accel).await;
        }

        // 画面の固定位置に情報を表示
        self.draw_display(data, event_recorder).await?;

        io::stdout().flush()?;
        Ok(true)
    }

    fn update_terminal_size(&mut self) -> io::Result<bool> {
        if let Ok((width, height)) = terminal::size() {
            let size_changed = self.terminal_width != width || self.terminal_height != height;
            if size_changed {
                self.terminal_width = width;
                self.terminal_height = height;
                // サイズが変更された場合は画面をクリア
                execute!(io::stdout(), terminal::Clear(ClearType::All), cursor::Hide)?;
            }
            Ok(size_changed)
        } else {
            Ok(false)
        }
    }

    async fn draw_display(
        &mut self,
        data: &DisplayData,
        event_recorder: &EventRecorder,
    ) -> io::Result<()> {
        // 値が変更されたかチェック
        let values_changed = self.values_changed(data);

        // カーソルを画面左上に移動
        execute!(io::stdout(), cursor::MoveTo(0, 0))?;

        // ヘッダー（行1-3）- キャッシュされた文字列を使用
        if values_changed || self.cached_strings.header_line.is_empty() {
            self.cached_strings.header_line = "═".repeat(self.terminal_width as usize);
            self.cached_strings.title = " リアルタイム震度・長周期地震動CLI".to_string();
        }

        execute!(
            io::stdout(),
            SetForegroundColor(Color::Green),
            Print(&self.cached_strings.header_line),
            cursor::MoveTo(0, 1),
            Print(&self.cached_strings.title),
            cursor::MoveTo(0, 2),
            Print(&self.cached_strings.header_line),
            ResetColor
        )?;

        // リアルタイム震度（行4）
        execute!(
            io::stdout(),
            cursor::MoveTo(0, 4),
            SetForegroundColor(Color::Grey),
            Print("震度: ")
        )?;

        match data.sindo {
            Some(val) => {
                let (color, level) = get_sindo_color(val);
                execute!(
                    io::stdout(),
                    SetForegroundColor(color),
                    Print(format!("{level} (計測震度: {val:.2})")),
                    ResetColor,
                )?;
            }
            None => {
                execute!(
                    io::stdout(),
                    SetForegroundColor(Color::Grey),
                    Print(&format!(
                        "データ不足{}",
                        " ".repeat(self.terminal_width.saturating_sub(15) as usize)
                    )),
                    ResetColor
                )?;
            }
        }

        // 長周期地震動階級表示（行5）
        execute!(
            io::stdout(),
            cursor::MoveTo(0, 5),
            SetForegroundColor(Color::Grey),
            Print("長周期地震動階級: "),
            ResetColor
        )?;

        match data.lpgm_data {
            Some(lpgm) => {
                let (color, _) = get_lpgm_color(lpgm.level);
                execute!(
                    io::stdout(),
                    SetForegroundColor(color),
                    Print(format!(
                        "{} (Max Sva: {:7.2} cm/s)",
                        get_lpgm_class_name(lpgm.level),
                        lpgm.max_sva_30s
                    )),
                    ResetColor
                )?;
            }
            None => {
                execute!(
                    io::stdout(),
                    SetForegroundColor(Color::Grey),
                    Print(&format!(
                        "データ不足{}",
                        " ".repeat(self.terminal_width.saturating_sub(15) as usize)
                    )),
                    ResetColor
                )?;
            }
        }

        // リアルタイム加速度表示（行6）
        execute!(
            io::stdout(),
            cursor::MoveTo(0, 6),
            SetForegroundColor(Color::Grey),
            Print("リアルタイム加速度: "),
            ResetColor
        )?;

        match data.acceleration {
            Some(accel) => {
                // g単位からGal単位に変換（1g = 980.665 Gal）
                let accel_gal = convert_g_to_gal(&accel);
                let composite_gal =
                    (accel_gal[0].powi(2) + accel_gal[1].powi(2) + accel_gal[2].powi(2)).sqrt();
                let (color, _) = get_acceleration_color(composite_gal);

                execute!(
                    io::stdout(),
                    SetForegroundColor(color),
                    Print(format!(
                        "X: {:+8.2}, Y: {:+8.2}, Z: {:+8.2}, |合成|: {:+8.2} [gal]",
                        accel_gal[0], accel_gal[1], accel_gal[2], composite_gal
                    )),
                    ResetColor
                )?;
            }
            None => {
                execute!(
                    io::stdout(),
                    SetForegroundColor(Color::Grey),
                    Print(&format!(
                        "データなし{}",
                        " ".repeat(self.terminal_width.saturating_sub(12) as usize)
                    )),
                    ResetColor
                )?;
            }
        }

        // ステータス情報（行8-12）
        execute!(
            io::stdout(),
            cursor::MoveTo(0, 8),
            SetForegroundColor(Color::Green),
            Print("システム状態:"),
            ResetColor
        )?;

        // センサー接続状態を表示
        execute!(io::stdout(), cursor::MoveTo(0, 9),)?;

        if values_changed || self.cached_strings.status_text.is_empty() {
            self.cached_strings.status_text = if data.is_connected {
                "  センサー状態: 接続".to_string()
            } else {
                "  センサー状態: 切断".to_string()
            };
        }

        let color = if data.is_connected {
            Color::Green
        } else {
            Color::Red
        };

        execute!(
            io::stdout(),
            SetForegroundColor(color),
            Print(&self.cached_strings.status_text),
            ResetColor
        )?;

        if values_changed || self.cached_strings.buffer_text.is_empty() {
            self.cached_strings.buffer_text =
                format!("  バッファサイズ: {} サンプル", data.buffer_size);
        }

        execute!(
            io::stdout(),
            cursor::MoveTo(0, 10),
            Print(&self.cached_strings.buffer_text)
        )?;

        // イベント記録情報を表示 - キャッシュされた文字列を使用
        execute!(io::stdout(), cursor::MoveTo(0, 11),)?;

        let is_recording = event_recorder.is_recording();
        if values_changed || self.cached_strings.event_text.is_empty() {
            self.cached_strings.event_text = if is_recording {
                format!(
                    "  イベント記録: 記録中 (閾値: 計測震度{:.1})",
                    event_recorder.get_threshold()
                )
            } else {
                format!(
                    "  イベント記録: 待機中 (閾値: 計測震度{:.1})",
                    event_recorder.get_threshold()
                )
            };
        }

        let color = if is_recording {
            Color::Red
        } else {
            Color::Grey
        };
        execute!(
            io::stdout(),
            SetForegroundColor(color),
            Print(&self.cached_strings.event_text),
            ResetColor
        )?;

        // リアルタイムグラフ描画（画面サイズが十分な場合のみ）
        let graph_start_row = 13;
        if let Some(ref graph) = self.realtime_graph
            && graph
                .should_draw_graph(self.terminal_width, self.terminal_height)
                .await
        {
            let graph_height = (self.terminal_height.saturating_sub(graph_start_row + 4)).min(13);
            let graph_width = (self.terminal_width.saturating_sub(12)).min(108);

            if graph_height > 0 && graph_width > 0 {
                // グラフタイトル
                execute!(
                    io::stdout(),
                    cursor::MoveTo(0, graph_start_row),
                    SetForegroundColor(Color::Cyan),
                    Print("三成分合成加速度グラフ"),
                    ResetColor
                )?;

                // グラフ描画
                let graph_lines = graph.render_graph(graph_width, graph_height).await;

                for (row_idx, (line, colors)) in graph_lines.iter().enumerate() {
                    let display_row = graph_start_row + 1 + row_idx as u16;

                    // Y軸ラベル（各行に表示）
                    let label = if graph_height > 0 {
                        // 各行に対応する加速度値を計算
                        let min_gal = 0.1f32;
                        let max_gal = 1000.0f32;
                        let log_min = min_gal.log10();
                        let log_max = max_gal.log10();

                        // 現在の行に対応する値を計算（上から下へ：max -> min）
                        let ratio = row_idx as f32 / (graph_height as usize - 1) as f32;
                        let log_value = log_max - (ratio * (log_max - log_min));
                        let gal_value = 10.0f32.powf(log_value);

                        if gal_value >= 1.0 {
                            format!("{:>5.0}", gal_value)
                        } else {
                            format!("{:>5.1}", gal_value)
                        }
                    } else {
                        "     ".to_string()
                    };

                    execute!(
                        io::stdout(),
                        cursor::MoveTo(0, display_row),
                        SetForegroundColor(Color::Grey),
                        Print(&label),
                        Print("│"),
                        ResetColor
                    )?;

                    // グラフ本体
                    for (char_idx, ch) in line.chars().enumerate() {
                        if char_idx < colors.len() && colors[char_idx] != Color::Reset {
                            execute!(
                                io::stdout(),
                                SetForegroundColor(colors[char_idx]),
                                Print(ch),
                                ResetColor
                            )?;
                        } else {
                            execute!(io::stdout(), Print(ch))?;
                        }
                    }
                }

                // X軸情報
                let x_axis_row = graph_start_row + graph_height + 1;
                execute!(
                    io::stdout(),
                    cursor::MoveTo(0, x_axis_row),
                    SetForegroundColor(Color::Grey),
                    Print(&format!("[gal]└{}", "─".repeat(graph_width as usize))),
                    ResetColor
                )?;
            }
        }

        // 操作説明（最下部）
        let controls_row = self.terminal_height.saturating_sub(1);
        execute!(
            io::stdout(),
            cursor::MoveTo(0, controls_row),
            SetForegroundColor(Color::DarkGrey),
            Print("操作: [Esc]終了 [q]終了"),
            ResetColor
        )?;

        // 前回の値を更新
        self.update_last_values(data);

        Ok(())
    }

    // 値が変更されたかチェックする関数
    fn values_changed(&self, data: &DisplayData) -> bool {
        self.last_values.sindo != data.sindo
            || self.acceleration_significantly_changed(data.acceleration)
            || !Self::lpgm_data_equal(self.last_values.lpgm_data, data.lpgm_data)
            || self.last_values.buffer_size != data.buffer_size
            || self.last_values.is_connected != data.is_connected
            || self.last_values.failed_count != data.failed_count
    }

    // 加速度の有意な変化を検出する関数
    fn acceleration_significantly_changed(&self, current: Option<Acceleration3D>) -> bool {
        match (self.last_values.acceleration, current) {
            (None, None) => false,
            (None, Some(_)) | (Some(_), None) => true,
            (Some(last), Some(current)) => {
                // より敏感な閾値で変化を検出（0.001g = 約0.98 Gal）
                let threshold = 0.001;
                (last.x - current.x).abs() > threshold
                    || (last.y - current.y).abs() > threshold
                    || (last.z - current.z).abs() > threshold
            }
        }
    }

    // 前回の値を更新する関数
    fn update_last_values(&mut self, data: &DisplayData) {
        self.last_values.sindo = data.sindo;
        self.last_values.acceleration = data.acceleration;
        self.last_values.lpgm_data = data.lpgm_data;
        self.last_values.buffer_size = data.buffer_size;
        self.last_values.is_connected = data.is_connected;
        self.last_values.failed_count = data.failed_count;
    }

    // LpgmDataの比較関数
    fn lpgm_data_equal(a: Option<LpgmData>, b: Option<LpgmData>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.level == b.level && (a.max_sva_30s - b.max_sva_30s).abs() < 0.01
            }
            _ => false,
        }
    }
}
