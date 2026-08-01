use crate::util::{Acceleration3D, convert_g_to_gal, get_acceleration_color};
use async_std::sync::{Arc, RwLock};
use crossterm::style::Color;
use std::collections::VecDeque;

pub const BRAILLE_CHARS: [char; 25] = [
    ' ', '⢀', '⢠', '⢰', '⢸', '⡀', '⣀', '⣠', '⣰', '⣸', '⡄', '⣄', '⣤', '⣴', '⣼', '⡆', '⣆', '⣦', '⣶',
    '⣾', '⡇', '⣇', '⣧', '⣷', '⣿',
];

pub struct GraphDataPoint {
    pub value: f32,
    pub color: Color,
}

pub struct RealtimeAccelerationGraph {
    buffer: Arc<RwLock<VecDeque<GraphDataPoint>>>,
    max_capacity: usize,
    min_graph_width: u16,
    min_graph_height: u16,
}

impl RealtimeAccelerationGraph {
    pub fn new(
        buffer_duration_seconds: f32,
        sample_rate: f32,
        min_width: u16,
        min_height: u16,
    ) -> Self {
        let max_capacity = (buffer_duration_seconds * sample_rate) as usize;

        Self {
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max_capacity))),
            max_capacity,
            min_graph_width: min_width,
            min_graph_height: min_height,
        }
    }

    pub async fn add_acceleration_sample(&self, accel: Acceleration3D) {
        let gal_values = convert_g_to_gal(&accel);
        let composite_gal =
            (gal_values[0].powi(2) + gal_values[1].powi(2) + gal_values[2].powi(2)).sqrt();
        let (color, _) = get_acceleration_color(composite_gal);

        let point = GraphDataPoint {
            value: composite_gal,
            color,
        };

        let mut buffer = self.buffer.write().await;
        buffer.push_back(point);

        if buffer.len() > self.max_capacity {
            buffer.pop_front();
        }
    }

    pub async fn should_draw_graph(&self, terminal_width: u16, terminal_height: u16) -> bool {
        terminal_width >= self.min_graph_width && terminal_height >= self.min_graph_height
    }

    pub async fn render_graph(&self, width: u16, height: u16) -> Vec<(String, Vec<Color>)> {
        let buffer = self.buffer.read().await;

        if buffer.is_empty() {
            return vec![(String::new(), Vec::new()); height as usize];
        }

        let min_gal = 0.1f32;
        let max_gal = 1000.0f32;

        let mut graph_lines = vec![(String::new(), Vec::new()); height as usize];
        let data_len = buffer.len();

        if data_len == 0 {
            return graph_lines;
        }

        for col in 0..width {
            let data_index = if data_len >= width as usize {
                data_len - width as usize + col as usize
            } else {
                if col < width - data_len as u16 {
                    continue;
                }
                col as usize - (width - data_len as u16) as usize
            };

            if data_index < data_len {
                let point = &buffer[data_index];
                let log_value = point.value.max(min_gal).log10();
                let log_min = min_gal.log10();
                let log_max = max_gal.log10();
                let normalized = ((log_value - log_min) / (log_max - log_min)).clamp(0.0, 1.0);

                let fill_height = (normalized * height as f32) as usize;
                let fill_height = fill_height.min(height as usize);

                for (row, (line, colors)) in
                    graph_lines.iter_mut().enumerate().take(height as usize)
                {
                    let graph_row = (height as usize - 1) - row;

                    if graph_row < fill_height {
                        let braille_char = if graph_row == fill_height - 1 {
                            let partial_fill = (normalized * height as f32) - fill_height as f32;
                            let char_index =
                                (partial_fill * (BRAILLE_CHARS.len() - 1) as f32) as usize;
                            BRAILLE_CHARS[char_index.min(BRAILLE_CHARS.len() - 1)]
                        } else {
                            BRAILLE_CHARS[24]
                        };

                        line.push(braille_char);
                        colors.push(point.color);
                    } else {
                        line.push(BRAILLE_CHARS[0]);
                        colors.push(Color::Reset);
                    }
                }
            } else {
                for (line, colors) in graph_lines.iter_mut().take(height as usize) {
                    line.push(BRAILLE_CHARS[0]);
                    colors.push(Color::Reset);
                }
            }
        }

        graph_lines
    }
}
