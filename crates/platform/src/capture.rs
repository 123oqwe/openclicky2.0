//! Screen capture through `xcap`. Never decides *when*; reachable only with a
//! `CaptureGuard` supplied by the core.

use openclicky_core::error::{Error, Permission, Result};
use openclicky_core::ports::{DisplayInfo, DisplayShot, ScreenSource};

pub struct XcapScreen;

impl XcapScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Default for XcapScreen {
    fn default() -> Self {
        Self
    }
}

fn map_err(e: impl std::fmt::Display) -> Error {
    Error::Internal(format!("capture: {e}"))
}

fn permission_error() -> Error {
    if cfg!(target_os = "macos") {
        Error::NoPermission(Permission::ScreenRecording)
    } else {
        Error::Internal("capture failed".into())
    }
}

impl ScreenSource for XcapScreen {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        let monitors = xcap::Monitor::all().map_err(|e| {
            if cfg!(target_os = "macos") {
                permission_error()
            } else {
                map_err(e)
            }
        })?;
        Ok(monitors
            .iter()
            .enumerate()
            .map(|(i, m)| DisplayInfo {
                index: i,
                id: m.id().unwrap_or(i as u32),
                name: m.name().unwrap_or_else(|_| format!("display {i}")),
                origin_x: m.x().unwrap_or(0) as f64,
                origin_y: m.y().unwrap_or(0) as f64,
                width: m.width().unwrap_or(0) as f64,
                height: m.height().unwrap_or(0) as f64,
                scale: m.scale_factor().unwrap_or(1.0) as f64,
                is_primary: m.is_primary().unwrap_or(i == 0),
                is_cursor_display: i == 0,
            })
            .collect())
    }

    fn capture_all(
        &self,
        guard: &openclicky_core::privacy::CaptureGuard<'_>,
    ) -> Result<Vec<DisplayShot>> {
        let _reason = guard.reason();
        let monitors = xcap::Monitor::all().map_err(map_err)?;
        let cursor = cursor_position();
        let mut shots: Vec<DisplayShot> = Vec::default();
        for (i, m) in monitors.iter().enumerate() {
            let image = match m.capture_image() {
                Ok(img) => img,
                Err(e) => {
                    if cfg!(target_os = "macos") {
                        return Err(permission_error());
                    }
                    return Err(map_err(e));
                }
            };
            let (w, h) = (image.width(), image.height());
            let dynimg = image::DynamicImage::ImageRgba8(image);
            let scaled = downscale(&dynimg, 1280);
            let mut cursor_out = std::io::Cursor::default();
            scaled
                .write_to(&mut cursor_out, image::ImageFormat::Jpeg)
                .map_err(map_err)?;
            let png = cursor_out.into_inner();

            let origin_x = m.x().unwrap_or(0) as f64;
            let origin_y = m.y().unwrap_or(0) as f64;
            let width = m.width().unwrap_or(w) as f64;
            let height = m.height().unwrap_or(h) as f64;
            let is_cursor = cursor
                .map(|(cx, cy)| {
                    cx >= origin_x
                        && cx < origin_x + width
                        && cy >= origin_y
                        && cy < origin_y + height
                })
                .unwrap_or(i == 0);
            shots.push(DisplayShot {
                display: DisplayInfo {
                    index: i,
                    id: m.id().unwrap_or(i as u32),
                    name: m.name().unwrap_or_else(|_| format!("display {i}")),
                    origin_x,
                    origin_y,
                    width,
                    height,
                    scale: m.scale_factor().unwrap_or(1.0) as f64,
                    is_primary: m.is_primary().unwrap_or(i == 0),
                    is_cursor_display: is_cursor,
                },
                width_px: w,
                height_px: h,
                png,
            });
        }
        if shots.is_empty() {
            return Err(Error::Internal("没有找到显示器".into()));
        }
        Ok(shots)
    }
}

fn downscale(img: &image::DynamicImage, max_edge: u32) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let longest = w.max(h);
    if longest <= max_edge {
        return img.clone();
    }
    let scale = max_edge as f32 / longest as f32;
    let nw = (w as f32 * scale).max(1.0) as u32;
    let nh = (h as f32 * scale).max(1.0) as u32;
    img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
}

fn cursor_position() -> Option<(f64, f64)> {
    use enigo::{Enigo, Mouse, Settings};
    let enigo = Enigo::new(&Settings::default()).ok()?;
    enigo.location().ok().map(|(x, y)| (x as f64, y as f64))
}
