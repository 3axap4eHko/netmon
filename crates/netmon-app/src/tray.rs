use image::{ImageBuffer, Rgba};
use std::sync::{Arc, Mutex};

use crate::db::Database;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayColor {
    Green,
    Yellow,
    Red,
}

pub fn color_from_loss(max_loss: f64) -> TrayColor {
    if max_loss >= 10.0 {
        TrayColor::Red
    } else if max_loss >= 2.0 {
        TrayColor::Yellow
    } else {
        TrayColor::Green
    }
}

pub fn create_tray_icon(color: TrayColor) -> Vec<u8> {
    let size: u32 = 16;
    let (r, g, b) = match color {
        TrayColor::Green => (0u8, 200u8, 83u8),
        TrayColor::Yellow => (255u8, 193u8, 7u8),
        TrayColor::Red => (244u8, 67u8, 54u8),
    };

    let center = size as f64 / 2.0;
    let radius = center;

    let img = ImageBuffer::from_fn(size, size, |x, y| {
        let dx = x as f64 - center;
        let dy = y as f64 - center;
        let dist = (dx * dx + dy * dy).sqrt();

        if dist < radius - 1.0 {
            Rgba([r, g, b, 255])
        } else if dist < radius {
            let alpha = ((radius - dist) * 255.0).clamp(0.0, 255.0) as u8;
            Rgba([r, g, b, alpha])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    // Encode to PNG in memory
    let mut png_bytes: Vec<u8> = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    image::ImageEncoder::write_image(
        encoder,
        img.as_raw(),
        size,
        size,
        image::ExtendedColorType::Rgba8,
    )
    .expect("Failed to encode tray icon PNG");

    png_bytes
}

/// Get the current max loss across all active targets (last 60 seconds).
pub fn get_current_max_loss(db: &Arc<Mutex<Database>>) -> f64 {
    let db = match db.lock() {
        Ok(db) => db,
        Err(_) => return 0.0,
    };

    let targets = match db.get_active_targets() {
        Ok(t) => t,
        Err(_) => return 0.0,
    };

    let mut max_loss = 0.0f64;
    for t in &targets {
        if let Ok(stats) = db.get_live_stats(&t.address, crate::types::TimeRange::OneHour) {
            for s in &stats {
                if s.loss_pct > max_loss {
                    max_loss = s.loss_pct;
                }
            }
        }
    }
    max_loss
}
