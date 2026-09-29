// vc_linux.rs — Pure virtual camera sink for Linux (v4l2loopback)
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use v4l::video::Output;
use v4l::{Device, Format, FourCC};
use zune_jpeg::JpegDecoder;

pub const CARD_LABEL: &str = "AWC Virtual Cam";

pub fn missing_loopback_error() -> String {
    "v4l2loopback is not available or is not configured for video output. Install and load it, then start Virtual Cam again:\n\
     sudo apt-get install v4l2loopback-dkms\n\
     sudo modprobe v4l2loopback devices=1 card_label=\"AWC Virtual Cam\" exclusive_caps=0"
        .to_string()
}

pub fn loopback_device_path() -> Option<PathBuf> {
    let mut labeled = None;
    let mut first = None;
    let virtual_dir = std::fs::read_dir("/sys/devices/virtual/video4linux").ok();
    if let Some(entries) = virtual_dir {
        for entry in entries.flatten() {
            let dev_name = entry.file_name();
            let path = PathBuf::from(format!("/dev/{}", dev_name.to_string_lossy()));
            if !path.exists() {
                continue;
            }
            let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
            let name = name.trim();
            if name == CARD_LABEL {
                labeled = Some(path);
                break;
            }
            if first.is_none() {
                first = Some(path);
            }
        }
    }
    labeled.or(first)
}

static CAM_RUNNING: AtomicBool = AtomicBool::new(false);
static FRAME_ROTATION: AtomicUsize = AtomicUsize::new(0);
static FRAME_MIRROR: AtomicBool = AtomicBool::new(false);
static CAM_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

#[tauri::command]
pub fn set_frame_transform(rotation: u16, mirror: bool) {
    FRAME_ROTATION.store((rotation as usize) % 360, Ordering::Relaxed);
    FRAME_MIRROR.store(mirror, Ordering::Relaxed);
}

/// Frames pushed directly from sender.rs
pub enum IncomingFrame {
    Jpeg(Vec<u8>),
    RawBgr(Vec<u8>),
}

static LATEST_FRAME: Mutex<Option<IncomingFrame>> = Mutex::new(None);

/// Overwrites LATEST_FRAME with the latest payload sent by sender.rs
pub fn push_frame(frame: IncomingFrame) {
    *LATEST_FRAME.lock().unwrap() = Some(frame);
}

fn decode_to_rgb(jpeg_bytes: &[u8], width: u32, height: u32, out: &mut [u8]) -> bool {
    let mut decoder = JpegDecoder::new(jpeg_bytes);
    let Ok(pixels) = decoder.decode() else {
        return false;
    };
    let Some((dec_w, dec_h)) = decoder.dimensions() else {
        return false;
    };
    let source_width = dec_w as usize;
    let source_height = dec_h as usize;
    let target_width = width as usize;
    let target_height = height as usize;
    let scale = (target_width as f32 / source_width as f32)
        .max(target_height as f32 / source_height as f32);
    let displayed_width = source_width as f32 * scale;
    let displayed_height = source_height as f32 * scale;
    for y in 0..target_height {
        for x in 0..target_width {
            let sx = (((x as f32 + 0.5 - (target_width as f32 - displayed_width) / 2.0) / scale)
                .floor() as isize)
                .clamp(0, source_width as isize - 1) as usize;
            let sy = (((y as f32 + 0.5 - (target_height as f32 - displayed_height) / 2.0) / scale)
                .floor() as isize)
                .clamp(0, source_height as isize - 1) as usize;
            let source_index = (sy * source_width + sx) * 3;
            let target_index = (y * target_width + x) * 3;
            out[target_index..target_index + 3]
                .copy_from_slice(&pixels[source_index..source_index + 3]);
        }
    }
    true
}

fn transform_rgb(source: &[u8], output: &mut [u8], width: u32, height: u32) {
    let width = width as usize;
    let height = height as usize;
    let rotation = FRAME_ROTATION.load(Ordering::Relaxed);
    let mirror = FRAME_MIRROR.load(Ordering::Relaxed);
    let rotated_width = if rotation == 90 || rotation == 270 {
        height
    } else {
        width
    };
    let rotated_height = if rotation == 90 || rotation == 270 {
        width
    } else {
        height
    };
    let scale = (width as f32 / rotated_width as f32).max(height as f32 / rotated_height as f32);
    let displayed_width = rotated_width as f32 * scale;
    let displayed_height = rotated_height as f32 * scale;

    for y in 0..height {
        for x in 0..width {
            let mut rx = (((x as f32 + 0.5 - (width as f32 - displayed_width) / 2.0) / scale)
                .floor() as isize)
                .clamp(0, rotated_width as isize - 1) as usize;
            let ry = (((y as f32 + 0.5 - (height as f32 - displayed_height) / 2.0) / scale).floor()
                as isize)
                .clamp(0, rotated_height as isize - 1) as usize;
            if mirror {
                rx = rotated_width - 1 - rx;
            }
            let (sx, sy) = match rotation {
                90 => (ry, height - 1 - rx),
                180 => (width - 1 - rx, height - 1 - ry),
                270 => (width - 1 - ry, rx),
                _ => (rx, ry),
            };
            let source_index = (sy * width + sx) * 3;
            let output_index = (y * width + x) * 3;
            output[output_index..output_index + 3]
                .copy_from_slice(&source[source_index..source_index + 3]);
        }
    }
}

#[tauri::command]
pub fn init_cam(on: bool, height: u32, width: u32) -> Result<String, String> {
    println!("Cam Init {}", on);
    if !on {
        CAM_RUNNING.store(false, Ordering::Relaxed);
        if let Some(handle) = CAM_THREAD.lock().unwrap().take() {
            let _ = handle.join();
        }
        // v4l2loopback may need a short interval to release the previous
        // output handle before accepting another format negotiation.
        thread::sleep(Duration::from_millis(50));
        return Ok("Virtual camera stopped".into());
    }
    if CAM_RUNNING.load(Ordering::Relaxed) {
        return Ok("Virtual camera already running".into());
    }

    let (tx, rx) = mpsc::channel();
    CAM_RUNNING.store(true, Ordering::Relaxed);
    let handle = thread::spawn(move || start_cam(height, width, tx));
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(msg)) => {
            *CAM_THREAD.lock().unwrap() = Some(handle);
            Ok(msg)
        }
        Ok(Err(e)) => {
            CAM_RUNNING.store(false, Ordering::Relaxed);
            let _ = handle.join();
            Err(e)
        }
        Err(_) => {
            CAM_RUNNING.store(false, Ordering::Relaxed);
            let _ = handle.join();
            Err("Timed out opening the virtual camera device".into())
        }
    }
}

fn rgb_to_yuyv(rgb: &[u8], yuyv: &mut [u8], width: u32, height: u32) {
    let w = width as usize;
    let h = height as usize;
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let i0 = (y * w + x) * 3;
            let r0 = rgb[i0] as i32;
            let g0 = rgb[i0 + 1] as i32;
            let b0 = rgb[i0 + 2] as i32;
            let (r1, g1, b1) = if x + 1 < w {
                let i1 = i0 + 3;
                (rgb[i1] as i32, rgb[i1 + 1] as i32, rgb[i1 + 2] as i32)
            } else {
                (r0, g0, b0)
            };
            let y0 = (((66 * r0 + 129 * g0 + 25 * b0 + 128) >> 8) + 16).clamp(0, 255) as u8;
            let y1 = (((66 * r1 + 129 * g1 + 25 * b1 + 128) >> 8) + 16).clamp(0, 255) as u8;
            let u = (((-38 * r0 - 74 * g0 + 112 * b0 + 128) >> 8) + 128).clamp(0, 255) as u8;
            let v = (((112 * r0 - 94 * g0 - 18 * b0 + 128) >> 8) + 128).clamp(0, 255) as u8;
            let o = (y * w + x) * 2;
            yuyv[o] = y0;
            yuyv[o + 1] = u;
            yuyv[o + 2] = y1;
            yuyv[o + 3] = v;
            x += 2;
        }
    }
}

fn start_cam(height: u32, width: u32, ready: mpsc::Sender<Result<String, String>>) {
    println!("Cam loop started (linux)");
    let v4l_w = width & !1;

    let Some(path) = loopback_device_path() else {
        let _ = ready.send(Err(missing_loopback_error()));
        CAM_RUNNING.store(false, Ordering::Relaxed);
        return;
    };
    println!("Using loopback device {}", path.display());
    let mut dev = match Device::with_path(&path) {
        Ok(dev) => dev,
        Err(e) => {
            let _ = ready.send(Err(format!(
                "Failed to open {}: {e}. Close other apps using this camera, then try again.",
                path.display()
            )));
            CAM_RUNNING.store(false, Ordering::Relaxed);
            return;
        }
    };

    // YUYV is what browsers and conferencing apps list as a capture camera.
    let yuyv = FourCC::new(b"YUYV");
    let mut requested =
        Output::format(&dev).unwrap_or_else(|_| Format::new(v4l_w, height, yuyv));
    requested.width = v4l_w;
    requested.height = height;
    requested.fourcc = yuyv;

    let mut negotiated = None;
    let mut last_error = None;
    for attempt in 0..3 {
        // Reusing the current format avoids a second VIDIOC_S_FMT call when
        // the loopback device retained the same format after disconnect.
        if let Ok(current) = Output::format(&dev) {
            if current.width == requested.width
                && current.height == requested.height
                && current.fourcc == requested.fourcc
            {
                negotiated = Some(current);
                break;
            }
        }

        match Output::set_format(&dev, &requested) {
            Ok(fmt) => {
                negotiated = Some(fmt);
                break;
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < 2 {
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    let Some(fmt) = negotiated else {
        let message = last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "device did not return a usable format".into());
        let _ = ready.send(Err(format!(
            "Failed to set format on {} after reconnect attempts: {message}",
            path.display()
        )));
        CAM_RUNNING.store(false, Ordering::Relaxed);
        return;
    };
    println!("Linux vcam format in use:\n{}", fmt);

    let mut source_rgb = vec![0u8; (width * height * 3) as usize];
    let mut rgb_frame = vec![0u8; (width * height * 3) as usize];
    let mut yuyv_frame = vec![0u8; (v4l_w * height * 2) as usize];
    for chunk in yuyv_frame.chunks_exact_mut(4) {
        chunk[0] = 16;
        chunk[1] = 128;
        chunk[2] = 16;
        chunk[3] = 128;
    }

    let _ = ready.send(Ok(format!(
        "Virtual camera on {} ({}). Leave Virtual Cam running so Zoom and Meet can see it.",
        path.display(),
        std::fs::read_to_string(format!(
            "/sys/class/video4linux/{}/name",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
        .unwrap_or_default()
        .trim()
    )));

    while CAM_RUNNING.load(Ordering::Relaxed) {
        if let Some(frame) = LATEST_FRAME.lock().unwrap().take() {
            match frame {
                IncomingFrame::Jpeg(jpeg_bytes) => {
                    if decode_to_rgb(&jpeg_bytes, width, height, &mut source_rgb) {
                        transform_rgb(&source_rgb, &mut rgb_frame, width, height);
                        rgb_to_yuyv(&rgb_frame, &mut yuyv_frame, v4l_w, height);
                    }
                }
                IncomingFrame::RawBgr(mut bgr_bytes) => {
                    if bgr_bytes.len() == source_rgb.len() {
                        for chunk in bgr_bytes.chunks_exact_mut(3) {
                            chunk.swap(0, 2);
                        }
                        transform_rgb(&bgr_bytes, &mut rgb_frame, width, height);
                        rgb_to_yuyv(&rgb_frame, &mut yuyv_frame, v4l_w, height);
                    }
                }
            }
        }

        if let Err(e) = dev.write_all(&yuyv_frame) {
            eprintln!("Failed writing frame to v4l2loopback: {:?}", e);
        }
        thread::sleep(Duration::from_millis(33));
    }
}
