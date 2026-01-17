use serde_json::{json, Value};
use std::thread;
use std::time::{Duration, Instant};

use crate::astrobox::psys_host::{device, dialog, interconnect, register, thirdpartyapp};
use crate::state::{app_state, TransferState};
use crate::ui;
use crate::utils::format_bytes;

pub const PACKAGE_NAME: &str = "com.bandbbs.ebook";
const CHUNK_TARGET_BYTES: usize = 32 * 1024;
const DEVICE_USAGE_LIMIT_BYTES: u64 = 25 * 1024 * 1024;

pub async fn pick_file_and_update_state() {
    let config = dialog::PickConfig {
        read: true,
        copy_to: None,
    };
    let filter = dialog::FilterConfig {
        multiple: false,
        extensions: Vec::new(),
        default_directory: String::new(),
        default_file_name: String::new(),
    };

    tracing::info!("picking file");

    let result = dialog::pick_file(&config, &filter).await;

    tracing::info!("picked file {}", result.name);

    if result.name.is_empty() {
        update_status("已取消选择文件", false);
        return;
    }

    let size = result.data.len();
    match String::from_utf8(result.data) {
        Ok(text) => {
            {
                let mut state = app_state()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.file_name = Some(result.name);
                state.file_size = size;
                state.file_text = Some(text);
                state.is_sending = false;
                state.progress = 0.0;
                state.transfer = None;
                state.status_message = Some("已选择文件，准备发送".to_string());
                state.is_success_message = true;
            }
            ui::rerender();
        }
        Err(_) => {
            {
                let mut state = app_state()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.file_name = Some(result.name);
                state.file_size = size;
                state.file_text = None;
                state.is_sending = false;
                state.progress = 0.0;
                state.transfer = None;
                state.status_message = Some("文件不是 UTF-8 文本，无法发送".to_string());
                state.is_success_message = false;
            }
            ui::rerender();
        }
    }
}

pub async fn start_send() {
    let (file_name, file_size) = {
        let state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (state.file_name.clone(), state.file_size)
    };

    let Some(file_name) = file_name else {
        update_status("请先选择文件", false);
        return;
    };

    set_sending_state("正在检查设备...", false);

    let Some(device_addr) = check_device().await else {
        reset_transfer_state();
        return;
    };

    let Some(app) = check_app(&device_addr).await else {
        reset_transfer_state();
        return;
    };

    if let Err(_) = thirdpartyapp::launch_qa(&device_addr, &app, "/index").await {
        update_status("启动应用失败，请重试", false);
        reset_transfer_state();
        return;
    }

    let _ = register::register_interconnect_recv(&device_addr, PACKAGE_NAME).await;
    thread::sleep(Duration::from_millis(800));

    let chunk_offsets = {
        let state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(file_text) = state.file_text.as_ref() else {
            update_status("文件无法读取或不是文本格式", false);
            reset_transfer_state();
            return;
        };
        compute_chunk_offsets(file_text, CHUNK_TARGET_BYTES)
    };
    if chunk_offsets.len() < 2 {
        update_status("文件为空，无法发送", false);
        reset_transfer_state();
        return;
    }

    let total_chunks = chunk_offsets.len() - 1;
    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.transfer = Some(TransferState {
            device_addr: device_addr.clone(),
            file_name: file_name.clone(),
            total_chunks,
            chunk_size: CHUNK_TARGET_BYTES,
            chunk_offsets,
            last_chunk_time: None,
        });
        state.is_sending = true;
        state.progress = 0.0;
        state.status_message = Some("等待设备响应...".to_string());
        state.is_success_message = false;
    }
    ui::rerender();

    let payload = json!({
        "tag": "file",
        "stat": "startTransfer",
        "filename": file_name,
        "total": total_chunks,
        "chunkSize": CHUNK_TARGET_BYTES,
        "size": file_size,
    });

    if let Err(_) = interconnect::send_qaic_message(&device_addr, PACKAGE_NAME, &payload.to_string()).await {
        update_status("发送失败，请重试", false);
        reset_transfer_state();
    }
}

pub async fn cancel_send() {
    let device_addr = {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(transfer) = &state.transfer else {
            state.is_sending = false;
            state.progress = 0.0;
            state.status_message = Some("未在发送中".to_string());
            state.is_success_message = false;
            drop(state);
            ui::rerender();
            return;
        };
        transfer.device_addr.clone()
    };

    let payload = json!({
        "tag": "file",
        "stat": "cancel",
    });
    let _ = interconnect::send_qaic_message(&device_addr, PACKAGE_NAME, &payload.to_string()).await;

    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.is_sending = false;
        state.progress = 0.0;
        state.transfer = None;
        state.status_message = Some("已取消发送".to_string());
        state.is_success_message = false;
    }
    ui::rerender();
}

pub fn handle_interconnect_message(payload: &str) {
    let parsed = match serde_json::from_str::<Value>(payload) {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!("Interconnect payload parse failed: {}", err);
            return;
        }
    };

    let inner_text = parsed
        .get("payloadText")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let value = if let Some(text) = inner_text {
        serde_json::from_str::<Value>(&text).unwrap_or(parsed)
    } else {
        parsed
    };

    let Some(obj) = value.as_object() else {
        tracing::warn!("Interconnect payload is not an object");
        return;
    };

    let tag = obj
        .get("tag")
        .and_then(|v| v.as_str())
        .or_else(|| {
            obj.get("data")
                .and_then(|data| data.get("tag"))
                .and_then(|v| v.as_str())
        });

    let Some(tag) = tag else {
        return;
    };

    if tag == "file" {
        let data_value = obj.get("data").unwrap_or(&value);
        let data_obj = data_value.as_object().unwrap_or(obj);
        let message_type = data_obj
            .get("type")
            .or_else(|| data_obj.get("stat"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match message_type {
            "ready" => {
                let usage = data_obj.get("usage").and_then(|v| v.as_u64()).unwrap_or(0);
                let found = data_obj.get("found").and_then(|v| v.as_bool()).unwrap_or(false);
                let length = data_obj.get("length").and_then(|v| v.as_u64()).unwrap_or(0);
                wit_bindgen::rt::async_support::block_on(async move {
                    handle_ready(found, usage, length).await;
                });
            }
            "next" => {
                let count = data_obj.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                wit_bindgen::rt::async_support::block_on(async move {
                    send_chunk(count, false).await;
                });
            }
            "error" => {
                let count = data_obj.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                wit_bindgen::rt::async_support::block_on(async move {
                    update_status("传输中断，正在重试...", false);
                    send_chunk(count, true).await;
                });
            }
            "success" => {
                finish_transfer("发送成功", true);
            }
            "cancel" => {
                finish_transfer("传输已取消", false);
            }
            _ => {}
        }
    }
}

fn compute_chunk_offsets(text: &str, target_bytes: usize) -> Vec<usize> {
    let mut offsets = Vec::new();
    offsets.push(0);

    let mut last = 0;
    for (idx, ch) in text.char_indices() {
        let next = idx + ch.len_utf8();
        if next - last >= target_bytes {
            offsets.push(next);
            last = next;
        }
    }

    if *offsets.last().unwrap_or(&0) != text.len() {
        offsets.push(text.len());
    }

    offsets
}

async fn handle_ready(found: bool, usage: u64, length: u64) {
    if usage > DEVICE_USAGE_LIMIT_BYTES {
        finish_transfer("设备存储空间不足", false);
        return;
    }

    let mut current_chunk = if found && length > 0 {
        (length as usize) / CHUNK_TARGET_BYTES
    } else {
        0
    };

    let total_chunks = {
        let state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.transfer.as_ref().map(|t| t.total_chunks).unwrap_or(0)
    };
    if total_chunks > 0 && current_chunk >= total_chunks {
        current_chunk = 0;
    }

    send_chunk(current_chunk, found).await;
}

async fn send_chunk(current_chunk: usize, is_resend: bool) {
    let (device_addr, total_chunks, chunk, last_chunk_time, chunk_len) = {
        let state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(transfer) = state.transfer.as_ref() else {
            return;
        };
        let Some(file_text) = state.file_text.as_ref() else {
            return;
        };

        if current_chunk >= transfer.total_chunks {
            return;
        }

        let start = transfer.chunk_offsets.get(current_chunk).copied().unwrap_or(0);
        let end = transfer
            .chunk_offsets
            .get(current_chunk + 1)
            .copied()
            .unwrap_or(file_text.len());
        let chunk = file_text[start..end].to_string();

        (
            transfer.device_addr.clone(),
            transfer.total_chunks,
            chunk,
            transfer.last_chunk_time,
            end.saturating_sub(start),
        )
    };

    let now = Instant::now();
    let mut speed_note = None;
    if let Some(last) = last_chunk_time {
        let elapsed = now.duration_since(last).as_secs_f64();
        if elapsed > 0.0 {
            let speed = chunk_len as f64 / elapsed;
            let remaining = total_chunks.saturating_sub(current_chunk);
            let eta = (remaining as f64 * elapsed).round();
            speed_note = Some(format!(
                "{} /s, {}s",
                format_bytes(speed),
                eta.max(1.0) as u64
            ));
        }
    }

    let progress = if total_chunks > 0 {
        current_chunk as f32 / total_chunks as f32
    } else {
        0.0
    };
    let status_message = if let Some(note) = speed_note {
        format!("传输中 {:.0}% - {}", progress * 100.0, note)
    } else {
        format!("传输中 {:.0}%", progress * 100.0)
    };

    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.progress = progress;
        state.is_success_message = false;
        state.status_message = Some(status_message);
        if let Some(transfer) = state.transfer.as_mut() {
            transfer.last_chunk_time = Some(now);
        }
    }

    ui::rerender();

    if current_chunk >= total_chunks {
        return;
    }

    let payload = json!({
        "tag": "file",
        "stat": "d",
        "count": current_chunk,
        "data": chunk,
        "setCount": if is_resend { json!(current_chunk) } else { Value::Null },
    });

    if let Err(_) = interconnect::send_qaic_message(&device_addr, PACKAGE_NAME, &payload.to_string()).await {
        update_status("发送分片失败", false);
        reset_transfer_state();
    }
}

async fn check_device() -> Option<String> {
    let devices = device::get_connected_device_list().await;
    if let Some(device) = devices.first() {
        Some(device.addr.clone())
    } else {
        update_status("未找到设备", false);
        None
    }
}

async fn check_app(device_addr: &str) -> Option<thirdpartyapp::AppInfo> {
    let app_list = thirdpartyapp::get_thirdparty_app_list(device_addr).await;
    match app_list {
        Ok(apps) => {
            let app = apps.into_iter().find(|app| app.package_name == PACKAGE_NAME);
            if app.is_none() {
                update_status("请先安装 BandBBS 客户端", false);
            }
            app
        }
        Err(_) => {
            update_status("获取应用列表失败", false);
            None
        }
    }
}

fn finish_transfer(message: &str, success: bool) {
    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.is_sending = false;
        state.progress = if success { 1.0 } else { state.progress };
        state.transfer = None;
        state.status_message = Some(message.to_string());
        state.is_success_message = success;
    }
    ui::rerender();
}

fn reset_transfer_state() {
    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.is_sending = false;
        state.progress = 0.0;
        state.transfer = None;
    }
    ui::rerender();
}

fn update_status(message: &str, success: bool) {
    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.status_message = Some(message.to_string());
        state.is_success_message = success;
    }
    ui::rerender();
}

fn set_sending_state(message: &str, success: bool) {
    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.is_sending = true;
        state.status_message = Some(message.to_string());
        state.is_success_message = success;
    }
    ui::rerender();
}
