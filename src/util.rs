use crossterm::style::Color;

// センサーの加速度データを保存するための構造体
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acceleration3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub timestamp: std::time::Duration,
}

// 加速度をgからgalへの変換
pub fn convert_g_to_gal(accel: &Acceleration3D) -> [f32; 3] {
    const G_TO_GAL: f32 = 980.665;
    [accel.x * G_TO_GAL, accel.y * G_TO_GAL, accel.z * G_TO_GAL]
}

// 加速度のカラーマップ
pub const ACCELERATION_COLOR_MAP: [(f32, Color, &str); 10] = [
    (
        0.5,
        Color::Rgb {
            r: 128,
            g: 128,
            b: 128,
        },
        "<0.5gal",
    ),
    (1.0, Color::Rgb { r: 0, g: 0, b: 255 }, "0.5-1gal"),
    (
        2.5,
        Color::Rgb {
            r: 0,
            g: 255,
            b: 255,
        },
        "1-2.5gal",
    ),
    (5.0, Color::Rgb { r: 0, g: 255, b: 0 }, "2.5-5gal"),
    (
        10.0,
        Color::Rgb {
            r: 255,
            g: 255,
            b: 0,
        },
        "5-10gal",
    ),
    (
        25.0,
        Color::Rgb {
            r: 255,
            g: 165,
            b: 0,
        },
        "10-25gal",
    ),
    (50.0, Color::Rgb { r: 255, g: 0, b: 0 }, "25-50gal"),
    (100.0, Color::Rgb { r: 128, g: 0, b: 0 }, "50-100gal"),
    (
        250.0,
        Color::Rgb {
            r: 128,
            g: 0,
            b: 128,
        },
        "100-250gal",
    ),
    (f32::INFINITY, Color::Rgb { r: 64, g: 0, b: 64 }, ">250gal"),
];

// 震度のカラーマップ (Kiwi Monitor カラースキーム 第3版)
pub const SINDO_COLOR_MAP: [(f32, Color, &str); 10] = [
    (
        0.5,
        Color::Rgb {
            r: 128,
            g: 128,
            b: 128,
        },
        "震度0　",
    ),
    (
        1.5,
        Color::Rgb {
            r: 60,
            g: 90,
            b: 130,
        },
        "震度1　",
    ),
    (
        2.5,
        Color::Rgb {
            r: 30,
            g: 130,
            b: 230,
        },
        "震度2　",
    ),
    (
        3.5,
        Color::Rgb {
            r: 120,
            g: 230,
            b: 220,
        },
        "震度3　",
    ),
    (
        4.5,
        Color::Rgb {
            r: 255,
            g: 255,
            b: 150,
        },
        "震度4　",
    ),
    (
        5.0,
        Color::Rgb {
            r: 255,
            g: 210,
            b: 0,
        },
        "震度5弱",
    ),
    (
        5.5,
        Color::Rgb {
            r: 255,
            g: 150,
            b: 0,
        },
        "震度5強",
    ),
    (
        6.0,
        Color::Rgb {
            r: 240,
            g: 50,
            b: 0,
        },
        "震度6弱",
    ),
    (6.5, Color::Rgb { r: 190, g: 0, b: 0 }, "震度6強"),
    (
        f32::INFINITY,
        Color::Rgb {
            r: 140,
            g: 0,
            b: 40,
        },
        "震度7　",
    ),
];

// 長周期地震動階級のカラーマップ
pub const LPGM_COLOR_MAP: [(u8, Color, &str); 6] = [
    (
        0,
        Color::Rgb {
            r: 128,
            g: 128,
            b: 128,
        },
        "階級0",
    ),
    (
        1,
        Color::Rgb {
            r: 0,
            g: 150,
            b: 255,
        },
        "階級1",
    ),
    (
        2,
        Color::Rgb {
            r: 255,
            g: 255,
            b: 0,
        },
        "階級2",
    ),
    (
        3,
        Color::Rgb {
            r: 255,
            g: 150,
            b: 0,
        },
        "階級3",
    ),
    (4, Color::Rgb { r: 255, g: 0, b: 0 }, "階級4"),
    (
        u8::MAX,
        Color::Rgb {
            r: 128,
            g: 128,
            b: 128,
        },
        "不明",
    ),
];

// 加速度のカラーマップを取得
pub fn get_acceleration_color(gal: f32) -> (Color, &'static str) {
    for &(threshold, color, label) in &ACCELERATION_COLOR_MAP {
        if gal < threshold {
            return (color, label);
        }
    }

    // ここには到達しないはず
    let last = ACCELERATION_COLOR_MAP.last().unwrap();
    (last.1, last.2)
}

// 加速度のカラーマップを取得
pub fn get_sindo_color(sindo: f32) -> (Color, &'static str) {
    for &(threshold, color, label) in &SINDO_COLOR_MAP {
        if sindo < threshold {
            return (color, label);
        }
    }

    // ここには到達しないはず
    let last = SINDO_COLOR_MAP.last().unwrap();
    (last.1, last.2)
}

// 加速度のカラーマップを取得
pub fn get_lpgm_color(class: u8) -> (Color, &'static str) {
    for &(level, color, label) in &LPGM_COLOR_MAP {
        if class == level {
            return (color, label);
        }
    }

    // ここには到達しないはず
    let last = LPGM_COLOR_MAP.last().unwrap();
    (last.1, last.2)
}
