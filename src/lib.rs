pub mod lpgm_calculator;
pub mod sindo_calculator;
pub mod util;

use encoding_rs::SHIFT_JIS;
use lpgm_calculator::LpgmCalculator;
use sindo_calculator::SindoCalculator;
use util::Acceleration3D;

use std::fs::File;
use std::io::Read;
use std::path::Path;

// CSVファイルから加速度データを読み込む（Shift_JISエンコーディング対応）
pub fn load_acceleration_data<P: AsRef<Path>>(
    path: P,
) -> Result<Vec<Acceleration3D>, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Shift_JISからUTF-8にデコード
    let (decoded, _, had_errors) = SHIFT_JIS.decode(&buffer);
    if had_errors {
        eprintln!("警告: ファイルのデコード中にエラーが発生しました");
    }

    let content = decoded.into_owned();
    let mut data = Vec::new();
    let mut skip_header = true;

    for line in content.lines() {
        // ヘッダー行をスキップ
        if skip_header {
            if line.contains("NS,EW,UD") {
                skip_header = false;
            }
            continue;
        }

        // 空行をスキップ
        if line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 3
            && let (Ok(x), Ok(y), Ok(z)) = (
                parts[0].trim().parse::<f32>(),
                parts[1].trim().parse::<f32>(),
                parts[2].trim().parse::<f32>(),
            )
        {
            data.push(Acceleration3D {
                x,
                y,
                z,
                timestamp: std::time::Duration::from_secs(0),
            });
        }
    }

    Ok(data)
}

// 震度計算テスト
pub async fn test_sindo_calculation<P: AsRef<Path>>(
    csv_path: P,
    expected_sindo: f32,
    tolerance: f32,
) -> Result<f32, Box<dyn std::error::Error>> {
    println!("震度計算テスト開始: {:?}", csv_path.as_ref());

    let data = load_acceleration_data(csv_path)?;
    println!("データ読み込み完了: {} サンプル", data.len());

    let sample_rate = 100.0; // 100Hz
    let mut calculator = SindoCalculator::new(sample_rate, 30.0, true, false); // data_is_in_gal = true, verbose = false

    let mut max_sindo = 0.0f32;
    let mut sample_count = 0;

    for accel in data {
        calculator.add_sample(accel);
        sample_count += 1;

        // 1000サンプルごとに震度を計算
        if sample_count % 1000 == 0
            && let Some(sindo) = calculator.calculate_sindo_with_filter(true).await
        {
            max_sindo = max_sindo.max(sindo);
        }
    }

    // 最終計算
    if let Some(sindo) = calculator.calculate_sindo_with_filter(true).await {
        max_sindo = max_sindo.max(sindo);
    }

    println!("計算結果: 最大震度 = {max_sindo:.1}");
    println!("期待値: {expected_sindo:.1} (許容誤差: ±{tolerance:.1})");

    let diff = (max_sindo - expected_sindo).abs();
    if diff <= tolerance {
        println!("✓ テスト成功");
    } else {
        println!("✗ テスト失敗: 誤差 {diff:.1}");
    }

    Ok(max_sindo)
}

// 長周期地震動階級計算テスト
pub async fn test_lpgm_calculation<P: AsRef<Path>>(
    csv_path: P,
    expected_lpgm: u8,
) -> Result<u8, Box<dyn std::error::Error>> {
    println!("長周期地震動階級計算テスト開始: {:?}", csv_path.as_ref());

    let data = load_acceleration_data(csv_path)?;
    println!("データ読み込み完了: {} サンプル", data.len());

    let sample_rate = 100.0; // 100Hz
    let mut calculator = LpgmCalculator::new(sample_rate, 30.0, true, false); // data_is_in_gal = true, verbose = false

    let mut max_lpgm = 0u8;
    let mut max_sva_30s = 0.0f32;

    for accel in data {
        calculator.add_sample(accel);
        let current_lpgm = calculator.calculate_lpgm().await;
        let current_sva = calculator.get_max_sva_30s();

        if let Some(lpgm) = current_lpgm {
            max_lpgm = max_lpgm.max(lpgm);
        }
        max_sva_30s = max_sva_30s.max(current_sva);
    }

    println!("計算結果: 最大長周期地震動階級 = {max_lpgm}, 最大Sva(30s) = {max_sva_30s:.3} cm/s");
    println!("期待値: {expected_lpgm}");

    if max_lpgm == expected_lpgm {
        println!("✓ テスト成功");
    } else {
        println!("✗ テスト失敗");
    }

    Ok(max_lpgm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[async_std::test]
    async fn test_sindo_47254() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_sindo_calculation("samples/acc20240101000147254.csv", 4.7, 0.1).await?;
        assert!(
            (result - 4.7).abs() <= 0.1,
            "震度が期待値から大きく外れています: {result}"
        );
        Ok(())
    }

    #[async_std::test]
    async fn test_sindo_41329() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_sindo_calculation("samples/acc20240101000141329.csv", 5.0, 0.1).await?;
        assert!(
            (result - 5.0).abs() <= 0.1,
            "震度が期待値から大きく外れています: {result}"
        );
        Ok(())
    }

    #[async_std::test]
    async fn test_sindo_65034() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_sindo_calculation("samples/acc20240101000165034.csv", 5.5, 0.1).await?;
        assert!(
            (result - 5.5).abs() <= 0.1,
            "震度が期待値から大きく外れています: {result}"
        );
        Ok(())
    }

    #[async_std::test]
    async fn test_sindo_47274() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_sindo_calculation("samples/acc20240101000147274.csv", 6.1, 0.1).await?;
        assert!(
            (result - 6.1).abs() <= 0.1,
            "震度が期待値から大きく外れています: {result}"
        );
        Ok(())
    }

    #[async_std::test]
    async fn test_sindo_67016() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_sindo_calculation("samples/acc20240101000167016.csv", 6.5, 0.1).await?;
        assert!(
            (result - 6.5).abs() <= 0.1,
            "震度が期待値から大きく外れています: {result}"
        );
        Ok(())
    }

    #[async_std::test]
    async fn test_lpgm_47254() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_lpgm_calculation("samples/acc20240101000147254.csv", 2).await?;
        assert_eq!(result, 2, "長周期地震動階級が期待値と異なります: {result}");
        Ok(())
    }

    #[async_std::test]
    async fn test_lpgm_41329() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_lpgm_calculation("samples/acc20240101000141329.csv", 3).await?;
        assert_eq!(result, 3, "長周期地震動階級が期待値と異なります: {result}");
        Ok(())
    }

    #[async_std::test]
    async fn test_lpgm_47274() -> Result<(), Box<dyn std::error::Error>> {
        let result = test_lpgm_calculation("samples/acc20240101000147274.csv", 4).await?;
        assert_eq!(result, 4, "長周期地震動階級が期待値と異なります: {result}");
        Ok(())
    }
}
