use crate::util::{Acceleration3D, convert_g_to_gal};

use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct LpgmCalculator {
    _sample_rate: f32,
    sample_time: f32,
    _buffer_duration: f32,
    verbose: bool,        // DEBUGログ表示フラグ
    data_is_in_gal: bool, // データがgal単位かどうか（false=g単位）

    // ハイパスフィルタの状態 (二次バターワース, 0.05 Hz カットオフ)
    hpf_b: [f32; 3],
    hpf_a: [f32; 3],
    acc_h: [f32; 3],
    acc_h_1: [f32; 3],
    acc_h_2: [f32; 3],
    acc_hf: [f32; 3],
    acc_hf_1: [f32; 3],
    acc_hf_2: [f32; 3],

    // 速度積分
    velocity: [f32; 3],

    // オシレーターパラメータ
    periods: Vec<f32>,
    a_matrices: Vec<[[f32; 2]; 2]>,
    b_matrices: Vec<[[f32; 2]; 2]>,
    xi_states: Vec<[[f32; 2]; 2]>,

    // Sva計算
    sva: Vec<f32>,
    max_sva_buffer: VecDeque<f32>,
    buffer_size: usize,
    max_sva_30s: f32,

    // LPGM階級
    current_lpgm: u8,

    // バッファ管理
    sample_count: usize,

    // 初期化フラグ
    initialized: bool,
    acc0: [f32; 3],
}

impl LpgmCalculator {
    pub fn new(
        sample_rate: f32,     // Hz
        buffer_duration: f32, // Sec
        data_is_in_gal: bool,
        verbose: bool,
    ) -> Self {
        let sample_time = 1.0 / sample_rate;

        // ハイパスフィルタ係数 (二次バターワース, 0.05 Hz カットオフ)
        let (hpf_b, hpf_a) = Self::butterworth_highpass(0.05, sample_rate);

        // 1.6秒から7.8秒まで0.2秒刻みの周期 (32周期)
        let periods: Vec<f32> = (0..32).map(|i| 1.6 + i as f32 * 0.2).collect();
        let num_periods = periods.len();

        // オシレーター行列を初期化
        let mut a_matrices = Vec::with_capacity(num_periods);
        let mut b_matrices = Vec::with_capacity(num_periods);
        let xi_states = vec![[[0.0; 2]; 2]; num_periods];

        let beta = 0.05; // 減衰率 (5%)

        for &period in &periods {
            let w = 2.0 * std::f32::consts::PI / period;
            let sw = (w * (1.0f32 - beta * beta).sqrt() * sample_time).sin();
            let cw = (w * (1.0f32 - beta * beta).sqrt() * sample_time).cos();
            let e = (-beta * w * sample_time).exp();

            // A行列
            let a = [
                [
                    e * (beta / (1.0 - beta * beta).sqrt() * sw + cw),
                    sw * e / (w * (1.0 - beta * beta).sqrt()),
                ],
                [
                    -sw * e * w / (1.0 - beta * beta).sqrt(),
                    e * (-beta / (1.0 - beta * beta).sqrt() * sw + cw),
                ],
            ];

            // B行列
            let b = [
                [
                    e * (((2.0 * beta * beta - 1.0) / (w * w * sample_time) + beta / w) * sw
                        / (w * (1.0 - beta * beta).sqrt())
                        + ((2.0 * beta) / (w * w * w * sample_time) + 1.0 / (w * w)) * cw)
                        - 2.0 * beta / (w * w * w * sample_time),
                    -e * (((2.0 * beta * beta - 1.0) / (w * w * sample_time)) * sw
                        / (w * (1.0 - beta * beta).sqrt())
                        + ((2.0 * beta) / (w * w * w * sample_time)) * cw)
                        + 2.0 * beta / (w * w * w * sample_time)
                        - 1.0 / (w * w),
                ],
                [
                    e * (((2.0 * beta * beta - 1.0) / (w * w * sample_time) + beta / w)
                        * (cw - beta * sw / (1.0 - beta * beta).sqrt())
                        - ((2.0 * beta) / (w * w * w * sample_time) + 1.0 / (w * w))
                            * (w * (1.0 - beta * beta).sqrt() * sw + beta * w * cw))
                        + 1.0 / (w * w * sample_time),
                    -e * (((2.0 * beta * beta - 1.0) / (w * w * sample_time))
                        * (cw - beta * sw / (1.0 - beta * beta).sqrt())
                        - ((2.0 * beta) / (w * w * w * sample_time))
                            * (w * (1.0 - beta * beta).sqrt() * sw + beta * w * cw))
                        - 1.0 / (w * w * sample_time),
                ],
            ];

            a_matrices.push(a);
            b_matrices.push(b);
        }

        let buffer_size = (sample_rate * buffer_duration) as usize; // 30秒バッファ
        let max_sva_buffer = VecDeque::with_capacity(buffer_size);

        Self {
            _sample_rate: sample_rate,
            sample_time,
            _buffer_duration: buffer_duration,
            verbose,
            data_is_in_gal,
            hpf_b,
            hpf_a,
            acc_h: [0.0; 3],
            acc_h_1: [0.0; 3],
            acc_h_2: [0.0; 3],
            acc_hf: [0.0; 3],
            acc_hf_1: [0.0; 3],
            acc_hf_2: [0.0; 3],
            velocity: [0.0; 3],
            periods,
            a_matrices,
            b_matrices,
            xi_states,
            sva: vec![0.0; num_periods],
            max_sva_buffer,
            buffer_size,
            max_sva_30s: 0.0,
            current_lpgm: 0,
            sample_count: 0,
            initialized: false,
            acc0: [0.0; 3],
        }
    }

    pub fn butterworth_highpass(cutoff: f32, sample_rate: f32) -> ([f32; 3], [f32; 3]) {
        // 汎用サンプルレート対応の実装
        let nyquist = sample_rate / 2.0;
        let wn = cutoff / nyquist;
        let c = (std::f32::consts::PI * wn).tan();
        let c2 = c * c;
        let sqrt2c = std::f32::consts::SQRT_2 * c;
        let k = 1.0 + sqrt2c + c2;

        let b = [1.0 / k, -2.0 / k, 1.0 / k];
        let a = [1.0, (2.0 * (c2 - 1.0)) / k, (1.0 - sqrt2c + c2) / k];

        (b, a)
    }

    pub fn add_sample(&mut self, accel: Acceleration3D) {
        let raw_acc = if self.data_is_in_gal {
            [accel.x, accel.y, accel.z]
        } else {
            convert_g_to_gal(&accel)
        };

        if !self.initialized {
            self.acc0 = raw_acc;
            self.initialized = true;
        }

        self.sample_count += 1;

        // 十分なデータがない場合は重い計算をスキップ
        if self.sample_count < 1000 {
            return;
        }

        // DC成分を除去
        let acc_corrected = [
            raw_acc[0] - self.acc0[0],
            raw_acc[1] - self.acc0[1],
            raw_acc[2] - self.acc0[2],
        ];

        // フィルタ入力レジスタをシフト
        self.acc_h_2 = self.acc_h_1;
        self.acc_h_1 = self.acc_h;
        self.acc_h = acc_corrected;

        // ハイパスフィルタを適用
        let mut acc_hf = [0.0; 3];
        for (i, acc_hf_item) in acc_hf.iter_mut().enumerate() {
            *acc_hf_item = self.hpf_b[0] * self.acc_h[i]
                + self.hpf_b[1] * self.acc_h_1[i]
                + self.hpf_b[2] * self.acc_h_2[i]
                - self.hpf_a[1] * self.acc_hf[i]
                - self.hpf_a[2] * self.acc_hf_1[i];
        }

        // フィルタ出力メモリを更新
        self.acc_hf_2 = self.acc_hf_1;
        self.acc_hf_1 = self.acc_hf;
        self.acc_hf = acc_hf;

        // 速度を得るための積分 (台形則)
        for i in 0..3 {
            self.velocity[i] += (self.acc_hf_1[i] + self.acc_hf[i]) * self.sample_time / 2.0;
        }

        self.calculate_oscillator_response();

        // Max Svaを検索
        let max_sva = self.sva.iter().fold(0.0f32, |a, &b| a.max(b));

        // NaNまたは無限大をチェックし、見つかった場合はスキップ
        if !max_sva.is_finite() {
            if self.verbose {
                println!("Warning: Non-finite Sva value detected: {max_sva}");
            }
            return;
        }

        // 30秒バッファを更新
        self.max_sva_buffer.push_front(max_sva);

        // バッファが30秒を超えたら最古のデータを削除
        if self.max_sva_buffer.len() > self.buffer_size {
            self.max_sva_buffer.pop_back();
        }

        // 過去30秒間のMax Svaを計算
        self.max_sva_30s = self.max_sva_buffer.iter().fold(0.0f32, |a, &b| a.max(b));

        // LPGM階級を判定
        self.current_lpgm = if self.max_sva_30s < 5.0 {
            0
        } else if self.max_sva_30s < 15.0 {
            1
        } else if self.max_sva_30s < 50.0 {
            2
        } else if self.max_sva_30s < 100.0 {
            3
        } else {
            4
        };

        if self.verbose && max_sva > 1.0 {
            println!(
                "LPGM: Max Sva = {:.2} cm/s, Max Sva 30s = {:.2} cm/s, Level = {}",
                max_sva, self.max_sva_30s, self.current_lpgm
            );
        }
    }

    pub async fn calculate_lpgm(&self) -> Option<u8> {
        // 十分なデータがない場合は計算しない
        if !self.initialized || self.sample_count < 1000 {
            return None;
        }

        // 計算処理を非同期で実行
        let result = async_std::task::spawn_blocking({
            let current_lpgm = self.current_lpgm;
            move || current_lpgm
        })
        .await;

        Some(result)
    }

    pub fn get_max_sva_30s(&self) -> f32 {
        self.max_sva_30s
    }

    // オシレーター応答計算
    fn calculate_oscillator_response(&mut self) {
        for j in 0..self.periods.len() {
            let acc_input = [
                [self.acc_hf_1[0], self.acc_hf_1[1]],
                [self.acc_hf[0], self.acc_hf[1]],
            ];

            // 行列乗算
            let mut new_xi = [[0.0; 2]; 2];

            // A
            for (i, new_xi_row) in new_xi.iter_mut().enumerate() {
                for (j_inner, new_xi_elem) in new_xi_row.iter_mut().enumerate() {
                    for k in 0..2 {
                        *new_xi_elem += self.a_matrices[j][i][k] * self.xi_states[j][k][j_inner];
                    }
                }
            }

            // B
            for (i, new_xi_row) in new_xi.iter_mut().enumerate() {
                for (j_inner, new_xi_elem) in new_xi_row.iter_mut().enumerate() {
                    for (k, acc_input_row) in acc_input.iter().enumerate() {
                        *new_xi_elem += self.b_matrices[j][i][k] * acc_input_row[j_inner];
                    }
                }
            }

            self.xi_states[j] = new_xi;

            // 絶対速度応答スペクトル(Sva)を計算
            let abs_vel_x = self.xi_states[j][1][0] + self.velocity[0];
            let abs_vel_y = self.xi_states[j][1][1] + self.velocity[1];
            self.sva[j] = (abs_vel_x * abs_vel_x + abs_vel_y * abs_vel_y).sqrt();
        }
    }
}
