// vc_linux.rs — Pure virtual camera sink for Linux (v4l2loopback)
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use v4l::video::Output;
use v4l::{Device, FourCC, Format};
use zune_jpeg::JpegDecoder;

pub const CARD_LABEL: &str = "AWC Virtual Cam";

pub fn missing_loopback_error() -> String {
    "v4l2loopback is not available. Install and load it, then start Virtual Cam again:\n\
     sudo apt-get install v4l2loopback-dkms\n\
     sudo modprobe v4l2loopback devices=1 card_label=\"AWC Virtual Cam\" exclusive_caps=1"
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
pub fn init_cam(on: bool, height: u32, width: u32) -> Result<(), String> {
    println!("Cam Init {}", on);
    if on {
        if loopback_device_path().is_none() {
            return Err(missing_loopback_error());
        }
        if CAM_RUNNING.load(Ordering::Relaxed) {
            return Ok(());
        }
        CAM_RUNNING.store(true, Ordering::Relaxed);
        let handle = thread::spawn(move || {
            start_cam(height, width);
        });
        *CAM_THREAD.lock().unwrap() = Some(handle);
    } else {
        CAM_RUNNING.store(false, Ordering::Relaxed);
        if let Some(handle) = CAM_THREAD.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
    Ok(())
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

fn start_cam(height: u32, width: u32) {
    println!("Cam loop started (linux)");
    let v4l_w = width & !1;

    let Some(path) = loopback_device_path() else {
        eprintln!("{}", missing_loopback_error());
        CAM_RUNNING.store(false, Ordering::Relaxed);
        return;
    };
    println!("Using loopback device {}", path.display());
    let mut dev = match Device::with_path(&path) {
        Ok(dev) => dev,
        Err(e) => {
            eprintln!("Failed to open {}: {:?}", path.display(), e);
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
        Err(e) => {
            eprintln!("Failed to set YUYV on {}: {:?}", path.display(), e);
            CAM_RUNNING.store(false, Ordering::Relaxed);
            return;
        }
    };
    println!("Linux vcam format in use:\n{}", fmt);

    let mut rgb_frame = vec![0u8; (width * height * 3) as usize];
    let mut yuyv_frame = vec![0u8; (v4l_w * height * 2) as usize];

    while CAM_RUNNING.load(Ordering::Relaxed) {
        let frame = LATEST_FRAME.lock().unwrap().take();
        let Some(frame) = frame else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };

        match frame {
            IncomingFrame::Jpeg(jpeg_bytes) => {
                if !decode_to_rgb(&jpeg_bytes, width, height, &mut rgb_frame) {
                    continue;
                }
            }
            IncomingFrame::RawBgr(mut bgr_bytes) => {
                if bgr_bytes.len() != rgb_frame.len() {
                    continue;
                }
                for chunk in bgr_bytes.chunks_exact_mut(3) {
                    chunk.swap(0, 2);
                }
                rgb_frame.copy_from_slice(&bgr_bytes);
            }
        }

        rgb_to_yuyv(&rgb_frame, &mut yuyv_frame, v4l_w, height);
        if let Err(e) = dev.write_all(&yuyv_frame) {
            eprintln!("Failed writing frame to v4l2loopback: {:?}", e);
        }
    }
}