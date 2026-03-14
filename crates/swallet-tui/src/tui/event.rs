use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

/// 轮询键盘事件，超时返回 None
pub fn poll_key_event(timeout: Duration) -> std::io::Result<Option<KeyEvent>> {
    if event::poll(timeout)?
        && let Event::Key(key) = event::read()? {
            return Ok(Some(key));
        }
    Ok(None)
}

/// 判断是否是退出快捷键 (Ctrl+C 或 Ctrl+Q)
pub fn is_quit_key(key: &KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Char('c') | KeyCode::Char('q'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
    )
}
