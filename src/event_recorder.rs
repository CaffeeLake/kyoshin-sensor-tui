use crate::util::{Acceleration3D, convert_g_to_gal};

use chrono::{DateTime, Datelike, Local, Timelike};
use encoding_rs::SHIFT_JIS;
use std::collections::VecDeque;
use std::error::Error;
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct EventRecorder {
    threshold: f32,
    output_dir: String,
    site_code: String,
    site_name: String,
    latitude: f32,
    longitude: f32,
    sample_rate: f32,
    is_recording: bool,
    current_event_data: VecDeque<Acceleration3D>,
    event_start_time: Option<DateTime<Local>>,
    pre_event_buffer: VecDeque<Acceleration3D>,
    pre_event_duration: f32,  // 秒
    post_event_duration: f32, // 秒
    post_event_counter: usize,
    last_above_threshold_time: Option<std::time::Instant>,
}

impl EventRecorder {
    pub fn new(
        threshold: f32,
        output_dir: &str,
        site_code: &str,
        site_name: &str,
        latitude: f32,
        longitude: f32,
        sample_rate: f32,
    ) -> Result<Self, Box<dyn Error>> {
        // 出力ディレクトリを作成
        create_dir_all(output_dir)?;

        let pre_event_duration = 10.0; // 地震前10秒のデータを保存
        let post_event_duration = 10.0; // 地震後10秒のデータを保存
        let pre_buffer_size = (sample_rate * pre_event_duration) as usize;

        Ok(Self {
            threshold,
            output_dir: output_dir.to_string(),
            site_code: site_code.to_string(),
            site_name: site_name.to_string(),
            latitude,
            longitude,
            sample_rate,
            is_recording: false,
            current_event_data: VecDeque::new(),
            event_start_time: None,
            pre_event_buffer: VecDeque::with_capacity(pre_buffer_size),
            pre_event_duration,
            post_event_duration,
            post_event_counter: 0,
            last_above_threshold_time: None,
        })
    }

    pub fn process_sample(
        &mut self,
        accel: Acceleration3D,
        latest_sindo: Option<f32>,
    ) -> Result<(), Box<dyn Error>> {
        // 常に事前バッファにデータを追加
        self.pre_event_buffer.push_back(accel);

        // 事前バッファのサイズ制限
        let max_pre_buffer_size = (self.sample_rate * self.pre_event_duration) as usize;
        while self.pre_event_buffer.len() > max_pre_buffer_size {
            self.pre_event_buffer.pop_front();
        }

        let sindo_value = latest_sindo.unwrap_or(0.0);

        if sindo_value >= self.threshold {
            self.last_above_threshold_time = Some(std::time::Instant::now());

            if !self.is_recording {
                // 記録開始
                self.start_recording()?;
            }
        }

        if self.is_recording {
            // 記録中の場合、データを追加
            self.current_event_data.push_back(accel);

            // 閾値を下回ってから一定時間経過したら記録終了
            if let Some(last_time) = self.last_above_threshold_time {
                let elapsed = last_time.elapsed().as_secs_f32();
                if elapsed > self.post_event_duration {
                    self.stop_recording()?;
                }
            }
        }

        Ok(())
    }

    fn start_recording(&mut self) -> Result<(), Box<dyn Error>> {
        self.is_recording = true;
        self.event_start_time = Some(Local::now());
        self.current_event_data.clear();

        // 事前バッファのデータを記録データに追加
        for sample in &self.pre_event_buffer {
            self.current_event_data.push_back(*sample);
        }

        self.post_event_counter = 0;
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<(), Box<dyn Error>> {
        if !self.is_recording {
            return Ok(());
        }

        self.is_recording = false;

        if let Some(start_time) = self.event_start_time {
            let filename = self.generate_filename(&start_time);
            let filepath = Path::new(&self.output_dir).join(&filename);

            self.write_event_file(&filepath, &start_time)?;
        }

        self.current_event_data.clear();
        self.event_start_time = None;
        Ok(())
    }

    fn generate_filename(&self, start_time: &DateTime<Local>) -> String {
        // JMAフォーマットに準拠したファイル名: acc + YYYYMMDDHHMM + 観測点コード + .csv
        format!(
            "acc{:04}{:02}{:02}{:02}{:02}{}.csv",
            start_time.year(),
            start_time.month(),
            start_time.day(),
            start_time.hour(),
            start_time.minute(),
            self.site_code
        )
    }

    fn write_event_file(
        &self,
        filepath: &Path,
        start_time: &DateTime<Local>,
    ) -> Result<(), Box<dyn Error>> {
        let file = File::create(filepath)?;
        let mut writer = BufWriter::new(file);

        // JMAフォーマットヘッダーを書き込み
        self.write_jma_header(&mut writer, start_time)?;

        // データを書き込み
        for sample in &self.current_event_data {
            let gal = convert_g_to_gal(sample);

            let ns_gal = gal[0];
            let ew_gal = gal[1];
            let ud_gal = gal[2];

            // JMAフォーマット: NS, EW, UD の順で出力（小数点以下3桁）
            let line = format!("{ns_gal:.3},{ew_gal:.3},{ud_gal:.3},,,,,\n");
            let (encoded, _, _) = SHIFT_JIS.encode(&line);
            writer.write_all(&encoded)?;
        }

        writer.flush()?;
        Ok(())
    }

    fn write_jma_header(
        &self,
        writer: &mut BufWriter<File>,
        start_time: &DateTime<Local>,
    ) -> Result<(), Box<dyn Error>> {
        // JMAフォーマットに準拠したヘッダー
        let header_lines = vec![
            format!(
                "SITE CODE= {}{},{},{},,,,,\n",
                self.site_code, self.site_name, self.latitude, self.longitude
            ),
            format!(" LAT.=   {:.4},,,,,,,\n", self.latitude),
            format!(" LON.=  {:.4},,,,,,,\n", self.longitude),
            format!(" SAMPLING RATE= {}Hz,,,,,,,\n", self.sample_rate as u32),
            " UNIT  = gal(cm/s/s),,,,,,,\n".to_string(),
            format!(
                "INITIAL TIME = {} {} {} {} {} {},,,,,,,\n",
                start_time.year(),
                start_time.month(),
                start_time.day(),
                start_time.hour(),
                start_time.minute(),
                start_time.second()
            ),
            " NS,EW,UD,,,,,\n".to_string(),
        ];

        for line in header_lines {
            let (encoded, _, _) = SHIFT_JIS.encode(&line);
            writer.write_all(&encoded)?;
        }

        writer.flush()?;
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording
    }

    pub fn get_threshold(&self) -> f32 {
        self.threshold
    }

    // 強制的に記録を停止（プログラム終了時など）
    pub fn force_stop_recording(&mut self) -> Result<(), Box<dyn Error>> {
        if self.is_recording {
            self.stop_recording()?;
        }
        Ok(())
    }
}

impl Drop for EventRecorder {
    fn drop(&mut self) {
        let _ = self.force_stop_recording();
    }
}
