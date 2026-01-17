use crate::astrobox::psys_host::{self, ui};
use crate::state::app_state;
use crate::transfer;
use crate::utils::format_bytes;

pub const PICK_FILE_EVENT: &str = "pick_file";
pub const SEND_FILE_EVENT: &str = "send_file";
pub const CANCEL_SEND_EVENT: &str = "cancel_send";

struct UiSnapshot {
    root_element_id: Option<String>,
    file_name: Option<String>,
    file_size: usize,
    status_message: Option<String>,
    is_success_message: bool,
    is_sending: bool,
    progress: f32,
    has_file: bool,
}

fn snapshot_state() -> UiSnapshot {
    let state = app_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    UiSnapshot {
        root_element_id: state.root_element_id.clone(),
        file_name: state.file_name.clone(),
        file_size: state.file_size,
        status_message: state.status_message.clone(),
        is_success_message: state.is_success_message,
        is_sending: state.is_sending,
        progress: state.progress,
        has_file: state.file_text.is_some(),
    }
}

pub async fn ui_event_processor(evtype: ui::Event, event: &str) {
    match evtype {
        ui::Event::Click => match event {
            PICK_FILE_EVENT => {
                transfer::pick_file_and_update_state().await;
            }
            SEND_FILE_EVENT => {
                transfer::start_send().await;
            }
            CANCEL_SEND_EVENT => {
                transfer::cancel_send().await;
            }
            _ => {}
        },
        _ => {}
    }
}

pub fn render_main_ui(element_id: &str) {
    {
        let mut state = app_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.root_element_id = Some(element_id.to_string());
    }

    let snapshot = snapshot_state();
    psys_host::ui::render(element_id, build_main_ui(&snapshot));
}

pub fn rerender() {
    let snapshot = snapshot_state();
    if let Some(root_element_id) = snapshot.root_element_id.as_ref() {
        psys_host::ui::render(root_element_id, build_main_ui(&snapshot));
    }
}

fn build_main_ui(snapshot: &UiSnapshot) -> ui::Element {
    let file_label = if let Some(name) = snapshot.file_name.as_ref() {
        if snapshot.file_size > 0 {
            format!("{} ({})", name, format_bytes(snapshot.file_size as f64))
        } else {
            name.to_string()
        }
    } else {
        "未选择文件".to_string()
    };

    let file_card = ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .padding(16)
        .radius(16)
        .border(1, "#2F353D")
        .child(
            ui::Element::new(ui::ElementType::P, Some("文件信息"))
                .size(12)
                .text_color("#9DA7B1"),
        )
        .child(
            ui::Element::new(ui::ElementType::P, Some(file_label.as_str()))
                .size(18)
                .text_color("#E6EDF3")
                .margin_top(6),
        );

    let progress_width = 260u32;
    let fill_width = if snapshot.progress <= 0.0 {
        0
    } else {
        let raw = (snapshot.progress * progress_width as f32).round() as u32;
        raw.clamp(6, progress_width)
    };

    let progress_bar = ui::Element::new(ui::ElementType::Div, None)
        .width(progress_width)
        .height(12)
        .radius(999)
        .border(1, "#2F353D")
        .child(
            ui::Element::new(ui::ElementType::Div, None)
                .width(fill_width)
                .height(12)
                .radius(999)
                .border(1, "#9DA7B1")
                .transition("width 0.2s ease"),
        );

    let progress_text = format!("进度 {:.0}%", snapshot.progress * 100.0);
    let progress_label = ui::Element::new(ui::ElementType::P, Some(progress_text.as_str()))
        .size(12)
        .text_color("#9DA7B1");

    let status_message = snapshot
        .status_message
        .clone()
        .unwrap_or_else(|| "等待操作".to_string());
    let status_color = if snapshot.is_sending {
        "#9DA7B1"
    } else if snapshot.is_success_message {
        "#7AAE92"
    } else {
        "#B07A7A"
    };
    let status_text = ui::Element::new(ui::ElementType::P, Some(status_message.as_str()))
        .size(14)
        .text_color(status_color)
        .margin_top(6);

    let progress_block = ui::Element::new(ui::ElementType::Div, None)
        .width_full()
        .padding(16)
        .radius(16)
        .border(1, "#2F353D")
        .margin_top(16)
        .child(progress_label)
        .child(progress_bar.margin_top(8))
        .child(status_text);

    let mut pick_button = ui::Element::new(ui::ElementType::Button, Some("选择文件"))
        .padding(10)
        .radius(12)
        .border(1, "#2F353D")
        .text_color("#E6EDF3")
        .on(ui::Event::Click, PICK_FILE_EVENT);
    if snapshot.is_sending {
        pick_button = pick_button.disabled().opacity(0.6);
    }

    let (send_label, send_event) = if snapshot.is_sending {
        ("取消发送", CANCEL_SEND_EVENT)
    } else {
        ("发送到设备", SEND_FILE_EVENT)
    };
    let mut send_button = ui::Element::new(ui::ElementType::Button, Some(send_label))
        .padding(10)
        .radius(12)
        .border(1, "#2F353D")
        .text_color("#E6EDF3")
        .margin_left(12)
        .on(ui::Event::Click, send_event);
    if !snapshot.has_file && !snapshot.is_sending {
        send_button = send_button.disabled().opacity(0.5);
    }

    let buttons = ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .flex_direction(ui::FlexDirection::Row)
        .align_center()
        .justify_center()
        .margin_top(20)
        .child(pick_button)
        .child(send_button);

    let helper = ui::Element::new(ui::ElementType::P, Some("提示：推荐使用 UTF-8 文本文件"))
        .size(12)
        .text_color("#9DA7B1")
        .margin_top(16);

    ui::Element::new(ui::ElementType::Div, None)
        .flex()
        .flex_direction(ui::FlexDirection::Column)
        .width_full()
        .height_full()
        .padding(20)
        .child(file_card)
        .child(progress_block)
        .child(buttons)
        .child(helper)
}
