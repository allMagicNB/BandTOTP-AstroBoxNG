use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub struct TransferState {
    pub device_addr: String,
    pub file_name: String,
    pub total_chunks: usize,
    pub chunk_size: usize,
    pub chunk_offsets: Vec<usize>,
    pub last_chunk_time: Option<Instant>,
}

pub struct AppState {
    pub root_element_id: Option<String>,
    pub file_name: Option<String>,
    pub file_size: usize,
    pub file_text: Option<String>,
    pub status_message: Option<String>,
    pub is_success_message: bool,
    pub is_sending: bool,
    pub progress: f32,
    pub transfer: Option<TransferState>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            root_element_id: None,
            file_name: None,
            file_size: 0,
            file_text: None,
            status_message: None,
            is_success_message: false,
            is_sending: false,
            progress: 0.0,
            transfer: None,
        }
    }
}

static APP_STATE: OnceLock<Mutex<AppState>> = OnceLock::new();

pub fn app_state() -> &'static Mutex<AppState> {
    APP_STATE.get_or_init(|| Mutex::new(AppState::default()))
}
