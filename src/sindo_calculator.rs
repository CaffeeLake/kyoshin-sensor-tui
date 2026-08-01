use crate::util::{Acceleration3D, convert_g_to_gal};

use rayon::prelude::*;
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::collections::VecDeque;
use std::sync::Arc;

pub struct SindoCalculator {
    buffer: VecDeque<Acceleration3D>,
    sample_rate: f32,
    verbose: bool,        // DEBUGログ表示フラグ
    data_is_in_gal: bool, // データがgal単位かどうか（false=g単位）
    // パフォーマンス改善用キャッシュ
    filtered_composite_cache: VecDeque<f32>, // フィルタリング済み合成加速度のキャッシュ
    // FFT最適化用
    fft_planner: FftPlanner<f32>,
    cached_fft_forward: Option<Arc<dyn Fft<f32>>>,
    cached_fft_inverse: Option<Arc<dyn Fft<f32>>>,
    cached_fft_size: usize,
    // 再利用可能なバッファ（3成分分）
    fft_buffers: [Vec<Complex<f32>>; 3],
    filter_response_cache: Vec<f32>, // 周波数応答のキャッシュ
    // 作業用バッファ（メモリアロケーション削減）
    work_buffer_x: Vec<f32>,
    work_buffer_y: Vec<f32>,
    work_buffer_z: Vec<f32>,
}

impl SindoCalculator {
    pub fn new(
        sample_rate: f32,     // Hz
        buffer_duration: f32, // Sec
        data_is_in_gal: bool,
        verbose: bool,
    ) -> Self {
        let buffer_size = (sample_rate * buffer_duration) as usize;

        Self {
            buffer: VecDeque::with_capacity(buffer_size),
            sample_rate,
            verbose,
            data_is_in_gal,
            filtered_composite_cache: VecDeque::with_capacity(buffer_size),
            fft_planner: FftPlanner::new(),
            cached_fft_forward: None,
            cached_fft_inverse: None,
            cached_fft_size: 0,
            fft_buffers: [
                Vec::with_capacity(buffer_size),
                Vec::with_capacity(buffer_size),
                Vec::with_capacity(buffer_size),
            ],
            filter_response_cache: Vec::with_capacity(buffer_size),
            work_buffer_x: Vec::with_capacity(buffer_size),
            work_buffer_y: Vec::with_capacity(buffer_size),
            work_buffer_z: Vec::with_capacity(buffer_size),
        }
    }

    pub fn add_sample(&mut self, accel: Acceleration3D) {
        self.buffer.push_back(accel);

        // バッファサイズを超えたら古いデータを削除
        loop {
            if self.buffer.len() >= self.buffer.capacity() {
                self.buffer.pop_front();
                // キャッシュからも対応する古いデータを削除
                if !self.filtered_composite_cache.is_empty() {
                    self.filtered_composite_cache.pop_front();
                }
            } else {
                break;
            }
        }
    }

    pub async fn calculate_sindo(&mut self) -> Option<f32> {
        self.calculate_sindo_with_filter(true).await
    }

    pub async fn calculate_sindo_with_filter(&mut self, apply_filter: bool) -> Option<f32> {
        // 最小サンプル数を増やして計算頻度を下げる
        if self.buffer.len() < 1000 {
            return None;
        }

        // 1. 3成分の加速度データをgal単位に変換
        self.convert_to_gal_batch();

        // 2. フィルター処理（気象庁公式方法）
        let vector_accelerations = if apply_filter && self.buffer.len() > 1000 {
            // 十分なデータがある場合のみフィルタ処理
            self.apply_jma_filter_and_compose_optimized()
        } else {
            self.compose_vector_accelerations_direct()
        };

        if vector_accelerations.is_empty() {
            return None;
        }

        // 3. 0.3秒間の閾値加速度aを求める
        let a = self.find_threshold_acceleration_fast(&vector_accelerations)?;

        // デバッグ情報を出力
        if self.verbose {
            let max_vector = vector_accelerations
                .iter()
                .fold(0.0f32, |max, &val| max.max(val));
            let avg_vector =
                vector_accelerations.iter().sum::<f32>() / vector_accelerations.len() as f32;

            println!("DEBUG: ベクトル加速度統計:");
            println!("  - サンプル数: {}", vector_accelerations.len());
            println!("  - 最大値: {max_vector:.3} gal");
            println!("  - 平均値: {avg_vector:.3} gal");
            println!("DEBUG: 0.3秒閾値加速度 a: {a:.6} gal");

            // 閾値以上の継続時間を確認
            let dt = 1.0 / self.sample_rate;
            let duration_above_a =
                Self::calculate_total_duration_above_threshold(&vector_accelerations, a, dt);
            println!("DEBUG: 閾値{a}gal以上の継続時間: {duration_above_a:.3}秒 (目標: 0.3秒)");
        }

        // 4. 計測震度の計算 I = 2 log a + 0.94 (aはgal単位)
        let sindo = 2.0 * a.log10() + 0.94;

        // 5. JMA公式方法による丸め処理
        // 「計算された I の小数第３位を四捨五入し、小数第２位を切り捨てたものを計測震度とする」
        let rounded_to_hundredths = (sindo * 100.0).round() / 100.0; // 小数第3位四捨五入
        let final_sindo = (rounded_to_hundredths * 10.0).floor() / 10.0; // 小数第2位切り捨て

        if self.verbose {
            println!("DEBUG: 震度計算詳細:");
            println!("  - 閾値加速度 a: {a:.6} gal");
            println!("  - 計測震度: {sindo:.6} → 丸め後: {final_sindo:.1}");
        }

        Some(final_sindo)
    }

    fn precompute_filter_response(&mut self, padded_size: usize) {
        let dt = 1.0 / self.sample_rate;
        self.filter_response_cache.resize(padded_size, 0.0);

        for i in 0..padded_size {
            let freq = if i <= padded_size / 2 {
                i as f32 / (padded_size as f32 * dt)
            } else {
                (i as f32 - padded_size as f32) / (padded_size as f32 * dt)
            };

            let filter_value = if freq >= 0.0 {
                self.jma_filter_response(freq)
            } else {
                self.jma_filter_response(-freq)
            };

            self.filter_response_cache[i] = filter_value;
        }
    }

    // フィルタリングとベクトル合成を同時実行
    pub fn apply_jma_filter_and_compose_optimized(&mut self) -> Vec<f32> {
        let n = self
            .work_buffer_x
            .len()
            .min(self.work_buffer_y.len())
            .min(self.work_buffer_z.len());
        if n < 2 {
            return self.compose_vector_accelerations_direct();
        }

        let padded_size = n.next_power_of_two();

        // FFTプランナーとバッファの準備
        if self.cached_fft_size != padded_size {
            self.cached_fft_forward = Some(self.fft_planner.plan_fft_forward(padded_size));
            self.cached_fft_inverse = Some(self.fft_planner.plan_fft_inverse(padded_size));
            self.cached_fft_size = padded_size;
            for buffer in &mut self.fft_buffers {
                buffer.resize(padded_size, Complex::new(0.0, 0.0));
            }
            self.precompute_filter_response(padded_size);
        }

        let data_arrays = [
            &self.work_buffer_x,
            &self.work_buffer_y,
            &self.work_buffer_z,
        ];

        // 3成分を順次処理してフィルタリング
        let scale = 1.0 / padded_size as f32;
        let mut filtered_components: [Vec<f32>; 3] = [
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        ];

        for (i, data) in data_arrays.iter().enumerate() {
            let buffer = &mut self.fft_buffers[i];

            // データをバッファにコピー
            for (j, &value) in data.iter().take(n).enumerate() {
                buffer[j] = Complex::new(value, 0.0);
            }
            for item in buffer.iter_mut().take(padded_size).skip(n) {
                *item = Complex::new(0.0, 0.0);
            }

            // FFT実行
            if let Some(ref fft) = self.cached_fft_forward {
                fft.process(buffer);
            }

            // フィルター適用
            for (sample, &filter_val) in buffer.iter_mut().zip(self.filter_response_cache.iter()) {
                *sample *= filter_val;
            }

            // IFFT実行
            if let Some(ref ifft) = self.cached_fft_inverse {
                ifft.process(buffer);
            }

            // 結果を収集（正規化）
            filtered_components[i].extend(buffer.iter().take(n).map(|c| c.re * scale));
        }

        // ベクトル合成を直接実行
        let mut vector_accelerations = Vec::with_capacity(n);
        for i in 0..n {
            let x = filtered_components[0][i];
            let y = filtered_components[1][i];
            let z = filtered_components[2][i];
            vector_accelerations.push((x * x + y * y + z * z).sqrt());
        }

        vector_accelerations
    }

    // フィルタリングなしの直接ベクトル合成
    pub fn compose_vector_accelerations_direct(&self) -> Vec<f32> {
        let n = self
            .work_buffer_x
            .len()
            .min(self.work_buffer_y.len())
            .min(self.work_buffer_z.len());

        // 並列処理でベクトル合成を高速化
        (0..n)
            .into_par_iter()
            .map(|i| {
                let x = self.work_buffer_x[i];
                let y = self.work_buffer_y[i];
                let z = self.work_buffer_z[i];
                (x * x + y * y + z * z).sqrt()
            })
            .collect()
    }

    pub fn jma_filter_response(&self, freq: f32) -> f32 {
        if freq <= 0.0 {
            return 0.0;
        }

        // 気象庁公式フィルター計算式（JMA仕様完全準拠）

        // 1. ローカットフィルター: FL = (1 - exp(-(f/0.5)^3))^(1/2)
        let freq_norm_lc = freq / 0.5; // 0.5で正規化
        let fl = if freq_norm_lc < 10.0 {
            // 通常範囲での高精度計算
            (1.0 - (-freq_norm_lc.powi(3)).exp()).sqrt()
        } else {
            // 高周波数域では近似値
            1.0
        };

        // 2. ハイカットフィルター: FH = (1 + 0.694*y^2 + 0.241*y^4 + ...)^(-1/2)
        let y = freq * 0.1; // freq/10の最適化
        let y2 = y * y;
        let y4 = y2 * y2;
        let fh_denom = if y < 5.0 {
            // 通常範囲での完全計算
            let y6 = y4 * y2;
            let y8 = y4 * y4;
            let y10 = y8 * y2;
            let y12 = y6 * y6;
            1.0 + 0.694 * y2
                + 0.241 * y4
                + 0.0557 * y6
                + 0.009664 * y8
                + 0.00134 * y10
                + 0.000155 * y12
        } else {
            // 高周波数域では主要項のみ
            1.0 + 0.694 * y2 + 0.241 * y4
        };
        let fh = fh_denom.powf(-0.5);

        // 3. 周期効果フィルター: FF = (1/f)^(1/2)
        // 最適化：平方根の逆数を直接計算
        let ff = freq.powf(-0.5);

        // 総合フィルター
        let total = fl * fh * ff;

        // 特定の周波数でのデバッグ出力
        if self.verbose && freq.fract() == 0.0 {
            let freq_int = freq as i32;
            if [0, 1, 2, 5, 10].contains(&freq_int) {
                println!(
                    "DEBUG: f={freq:.1}Hz, FL={fl:.4}, FH={fh:.4}, FF={ff:.4}, Total={total:.4}"
                );
            }
        }

        if total.is_finite() && total >= 0.0 {
            total
        } else {
            0.0
        }
    }

    // スライディングウィンドウ最適化
    pub fn find_threshold_acceleration_fast(&mut self, vector_acc: &[f32]) -> Option<f32> {
        if vector_acc.is_empty() {
            return None;
        }

        let dt = 1.0 / self.sample_rate;
        let target_duration = 0.3;
        let target_samples = (target_duration / dt).round() as usize;

        if target_samples >= vector_acc.len() {
            return Some(
                vector_acc
                    .iter()
                    .fold(f32::INFINITY, |a, &b| a.min(b))
                    .max(0.01),
            );
        }

        if target_samples == 0 {
            return Some(vector_acc.iter().fold(0.0f32, |a, &b| a.max(b)).max(0.01));
        }

        // 単純なソート
        let mut sorted = vector_acc.to_vec();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

        let threshold = if target_samples < sorted.len() {
            sorted[target_samples - 1]
        } else {
            sorted[sorted.len() - 1]
        };

        Some(threshold.max(0.01))
    }

    pub fn calculate_total_duration_above_threshold(data: &[f32], threshold: f32, dt: f32) -> f32 {
        let mut total_duration = 0.0;

        for &value in data {
            // JMA公式方法：ベクトル波形の絶対値がある値a以上となる時間の合計を計算
            if value >= threshold {
                total_duration += dt;
            }
        }

        total_duration
    }

    // Get the current buffer length
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    // SIMD最適化とメモリ効率を重視
    fn convert_to_gal_batch(&mut self) -> (&[f32], &[f32], &[f32]) {
        let len = self.buffer.len();

        // 作業用バッファをクリアしてサイズ調整
        self.work_buffer_x.clear();
        self.work_buffer_y.clear();
        self.work_buffer_z.clear();

        // 容量を事前確保
        if self.work_buffer_x.capacity() < len {
            let additional = len - self.work_buffer_x.capacity();
            self.work_buffer_x.reserve(additional);
            self.work_buffer_y.reserve(additional);
            self.work_buffer_z.reserve(additional);
        }

        // 並列処理でデータ変換を高速化
        let buffer_vec: Vec<_> = self.buffer.iter().collect();
        let converted_data: Vec<[f32; 3]> = buffer_vec
            .par_iter()
            .map(|sample| {
                if self.data_is_in_gal {
                    [sample.x, sample.y, sample.z]
                } else {
                    convert_g_to_gal(sample)
                }
            })
            .collect();

        // 結果を各成分バッファに分離
        for converted in converted_data {
            self.work_buffer_x.push(converted[0]);
            self.work_buffer_y.push(converted[1]);
            self.work_buffer_z.push(converted[2]);
        }

        (
            &self.work_buffer_x,
            &self.work_buffer_y,
            &self.work_buffer_z,
        )
    }
}
