// virtual - cam file
use std::os::raw::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use zune_jpeg::JpegDecoder;

static CAM_RUNNING: AtomicBool = AtomicBool::new(false);
static FRAME_ROTATION: AtomicUsize = AtomicUsize::new(0);
static FRAME_MIRROR: AtomicBool = AtomicBool::new(false);
static CAM_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

#[tauri::command]
pub fn set_frame_transform(rotation: u16, mirror: bool) {
    FRAME_ROTATION.store((rotation as usize) % 360, Ordering::Relaxed);
    FRAME_MIRROR.store(mirror, Ordering::Relaxed);
}

/// What sender.rs hands off, depending on which source it read from.
/// MJPEG frames arrive already-encoded and still need a JPEG decode here.
/// RTSP frames arrive already-decoded (by ffmpeg) and go straight to softcam.
pub enum IncomingFrame {
    Jpeg(Vec<u8>),
    RawBgr(Vec<u8>),
}

static LATEST_FRAME: Mutex<Option<IncomingFrame>> = Mutex::new(None);

#[link(name = "softcam")]
unsafe extern "system" {
    fn scCreateCamera(width: i32, height: i32, frame_rate: f32) -> *mut c_void;
    fn scWaitForConnection(camera: *mut c_void, timeout_sec: f32) -> bool;
    fn scSendFrame(camera: *mut c_void, image_data: *const u8);
    fn scDeleteCamera(camera: *mut c_void) -> bool;
}

/// Called by sender.rs whenever it has a new frame ready, in whichever
/// form its source naturally produces. Overwrites rather than queues.
pub fn push_frame(frame: IncomingFrame) {
    *LATEST_FRAME.lock().unwrap() = Some(frame);
}

fn decode_jpeg_to_bgr(jpeg_bytes: &[u8], width: u32, height: u32, out: &mut [u8]) -> bool {
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
            out[target_index] = pixels[source_index + 2]; // b
            out[target_index + 1] = pixels[source_index + 1]; // g
            out[target_index + 2] = pixels[source_index]; // r
        }
    }

    true
}

fn transform_bgr(source: &[u8], output: &mut [u8], width: u32, height: u32) {
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
    if on {
        if CAM_RUNNING.load(Ordering::Relaxed) {
            return Ok("Virtual camera already running".into());
        }
        CAM_RUNNING.store(true, Ordering::Relaxed);
        let handle = std::thread::spawn(move || {
            start_cam(height, width);
        });
        *CAM_THREAD.lock().unwrap() = Some(handle);
        Ok("Virtual camera started".into())
    } else {
        CAM_RUNNING.store(false, Ordering::Relaxed);
        if let Some(handle) = CAM_THREAD.lock().unwrap().take() {
            handle.join().unwrap();
        }
        Ok("Virtual camera stopped".into())
    }
}

fn start_cam(height: u32, width: u32) {
    println!("Cam loop started");

    let cam = unsafe { scCreateCamera(width as i32, height as i32, 30.0) };
    unsafe { scWaitForConnection(cam, 30.0) };

    // Allocated once, reused for every frame regardless of which branch
    // below fills it — this is the "no per-frame alloc" rule from above.
    let mut bgr_scratch = vec![0u8; (width * height * 3) as usize];
    let mut transformed_bgr = vec![0u8; (width * height * 3) as usize];

    while CAM_RUNNING.load(Ordering::Relaxed) {
        let frame = LATEST_FRAME.lock().unwrap().take();
        let Some(frame) = frame else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };

        match frame {
            IncomingFrame::Jpeg(jpeg_bytes) => {
                if !decode_jpeg_to_bgr(&jpeg_bytes, width, height, &mut bgr_scratch) {
                    continue;
                }
                transform_bgr(&bgr_scratch, &mut transformed_bgr, width, height);
                unsafe { scSendFrame(cam, transformed_bgr.as_ptr()) };
            }
            IncomingFrame::RawBgr(bgr_bytes) => {
                // Already decoded upstream (ffmpeg) — send as-is.
                // Guard the size so a mismatched resolution can't read out of bounds.
                if bgr_bytes.len() != bgr_scratch.len() {
                    continue;
                }
                transform_bgr(&bgr_bytes, &mut transformed_bgr, width, height);
                unsafe { scSendFrame(cam, transformed_bgr.as_ptr()) };
            }
        }
    }

    unsafe { scDeleteCamera(cam) };
}
