use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    Idle,
    Recording { started_at: Instant },
    Transcribing,
    Injecting,
}

pub type SharedState = Arc<parking_lot::Mutex<AppState>>;

pub fn new_shared_state() -> SharedState {
    Arc::new(parking_lot::Mutex::new(AppState::Idle))
}
