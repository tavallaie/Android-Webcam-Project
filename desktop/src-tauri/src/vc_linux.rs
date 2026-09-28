// vc_linux.rs — Pure virtual camera sink for Linux (v4l2loopback)
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use v4l::video::Output;
use v4l::{Device, FourCC, Format};
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
static CAM_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

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
    if dec_w as u32 != width || dec_h as u32 != height {
        return false;
    }
    out.copy_from_slice(&pixels);
    true
}

#[tauri::command]
pub fn init_cam(on: bool, height: u32, width: u32) -> Result<String, String> {
    println!("Cam Init {}", on);
    if !on {
        CAM_RUNNING.store(false, Ordering::Relaxed);
        if let Some(handle) = CAM_THREAD.lock().unwrap().take() {
            let _ = handle.join();
        }
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
    let mut fmt = Output::format(&dev)
        .unwrap_or_else(|_| Format::new(v4l_w, height, FourCC::new(b"YUYV")));
    fmt.width = v4l_w;
    fmt.height = height;
    fmt.fourcc = FourCC::new(b"YUYV");
    let fmt = match Output::set_format(&dev, &fmt) {
        Ok(fmt) => fmt,
        Err(_) => match Output::format(&dev) {
            Ok(fmt) => fmt,
            Err(e) => {
                let _ = ready.send(Err(format!(
                    "Failed to set format on {}: {e}",
                    path.display()
                )));
                CAM_RUNNING.store(false, Ordering::Relaxed);
                return;
            }
        },
    };
    println!("Linux vcam format in use:\n{}", fmt);

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
                    if decode_to_rgb(&jpeg_bytes, width, height, &mut rgb_frame) {
                        rgb_to_yuyv(&rgb_frame, &mut yuyv_frame, v4l_w, height);
                    }
                }
                IncomingFrame::RawBgr(mut bgr_bytes) => {
                    if bgr_bytes.len() == rgb_frame.len() {
                        for chunk in bgr_bytes.chunks_exact_mut(3) {
                            chunk.swap(0, 2);
                        }
                        rgb_frame.copy_from_slice(&bgr_bytes);
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
