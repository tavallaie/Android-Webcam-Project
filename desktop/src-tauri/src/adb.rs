use std::{env, path::PathBuf};
use serde::Serialize;
#[cfg(target_os = "windows")]
use tauri::path::BaseDirectory;
use tauri::AppHandle;
#[cfg(target_os = "windows")]
use tauri::Manager;
use tokio::process::Command;

#[derive(Clone, Serialize, Debug)]
pub struct Device {
    id: String,
    model: String,
}

pub fn adb_is_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        true
    }
    #[cfg(not(target_os = "windows"))]
    {
        linux_adb_path().is_file()
    }
}

fn adb_bin(handle: &AppHandle) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        get_file_path(handle, "adb/adb.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
        linux_adb_path()
    }
}

#[cfg(not(target_os = "windows"))]
fn linux_adb_path() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(path) = env::var("PATH") {
        for dir in path.split(':') {
            candidates.push(PathBuf::from(dir).join("adb"));
        }
    }
    if let Ok(home) = env::var("HOME") {
        candidates.push(PathBuf::from(&home).join("Android/platform-tools/adb"));
        candidates.push(PathBuf::from(&home).join("Android/Sdk/platform-tools/adb"));
    }
    if let Ok(sdk) = env::var("ANDROID_HOME").or_else(|_| env::var("ANDROID_SDK_ROOT")) {
        candidates.push(PathBuf::from(sdk).join("platform-tools/adb"));
    }
    candidates.push(PathBuf::from("/usr/bin/adb"));
    candidates.push(PathBuf::from("/usr/local/bin/adb"));
    candidates.push(PathBuf::from("/snap/bin/adb"));
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("adb"))
}

#[tauri::command]
pub async fn adb_get_devices(handle: AppHandle) -> Result<Vec<Device>, String> {
    let adb = adb_bin(&handle);
    let output = Command::new(&adb)
        .arg("devices")
        .output()
        .await
        .map_err(|e| {
            format!(
                "adb failed ({:?}): {e}. Install Android platform-tools and put adb on PATH.",
                adb
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "adb devices failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    for line in output_str.lines().skip(1) {
        if !line.contains("\tdevice") {
            continue;
        }
        let id = line.split_whitespace().next().unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let model_out = Command::new(&adb)
            .args(["-s", id, "shell", "getprop", "ro.product.model"])
            .output()
            .await
            .map_err(|e| format!("adb shell failed: {e}"))?;
        let model = String::from_utf8_lossy(&model_out.stdout).trim().to_string();
        println!("Device: {}, Model: {}", id, model);
        devices.push(Device {
            id: id.to_string(),
            model,
        });
    }
    Ok(devices)
}

#[tauri::command]
pub async fn adb_connect_device(
    handle: AppHandle,
    device_id: String,
    device_model: String,
) -> Result<String, String> {
    // Non-blocking disconnect call
    adb_disconnect_device(&handle, &device_id, &device_model).await?;

    let adb = adb_bin(&handle);

    // Async execution for 8080 forward
    let output_8080 = Command::new(&adb)
        .args(["-s", &device_id, "forward", "tcp:8080", "tcp:8080"])
        .output()
        .await
        .map_err(|e| format!("Failed to run ADB 8080 forward: {}", e))?;

    // Async execution for 8554 forward
    let output_8554 = Command::new(&adb)
        .args(["-s", &device_id, "forward", "tcp:8554", "tcp:8554"])
        .output()
        .await
        .map_err(|e| format!("Failed to run ADB 8554 forward: {}", e))?;

    if output_8080.status.success() && output_8554.status.success() {
        Ok(format!("Device {} is ready", device_model))
    } else {
        let err_8080 = String::from_utf8_lossy(&output_8080.stderr);
        let err_8554 = String::from_utf8_lossy(&output_8554.stderr);
        Err(format!(
            "Device {} not ready: {} {}",
            device_model, err_8080, err_8554
        ))
    }
}

pub async fn adb_disconnect_device(
    handle: &AppHandle,
    device_id: &str,
    device_model: &str,
) -> Result<String, String> {
    let adb = adb_bin(handle);

    // Helper closure returning a Future
    let remove_port = |port: &'static str| {
        let adb_path = adb.clone();
        let dev_id = device_id.to_string();
        async move {
            let output = Command::new(&adb_path)
                .args(["-s", &dev_id, "forward", "--remove", port])
                .output()
                .await
                .map_err(|e| format!("ADB error: {}", e))?;

            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("not found") {
                    Ok(()) // Ignore missing forwarding rule
                } else {
                    Err(stderr.to_string())
                }
            }
        }
    };

    // Run both port removal commands concurrently with tokio::join!
    let (res_8080, res_8554) = tokio::join!(remove_port("tcp:8080"), remove_port("tcp:8554"));

    match (res_8080, res_8554) {
        (Ok(_), Ok(_)) => Ok(format!("Device {} is disconnected", device_model)),
        (Err(e1), Err(e2)) => Err(format!("Failed to disconnect {}: {}; {}", device_model, e1, e2)),
        (Err(e), _) | (_, Err(e)) => Err(format!("Failed to disconnect {}: {}", device_model, e)),
    }
}

#[cfg(target_os = "windows")]
fn get_file_path(handle: &AppHandle, file: &str) -> PathBuf {
    if cfg!(debug_assertions) {
        let base = env::current_exe().unwrap().parent().unwrap().to_path_buf();
        // println!("Looking for: {}", base.display());
        return base.join(file);
    } else {
        handle
            .path()
            .resolve(file, BaseDirectory::Resource)
            .unwrap()
    }
}

