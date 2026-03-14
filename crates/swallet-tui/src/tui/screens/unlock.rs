use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui::state::{UiState, UnlockMode};

pub fn render(frame: &mut Frame, state: &UiState) {
    let area = frame.area();

    // 居中布局
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(10),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(50),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let title = match state.unlock_mode {
        UnlockMode::Create => " 创建密码 ",
        UnlockMode::Enter => " 输入密码 ",
        UnlockMode::Confirm => " 确认密码 ",
    };

    let hint = match state.unlock_mode {
        UnlockMode::Create => "首次使用，请设置加密密码：",
        UnlockMode::Enter => "请输入密码解锁钱包：",
        UnlockMode::Confirm => "请再次输入密码确认：",
    };

    let masked: String = "*".repeat(state.password_input.len());

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " swallet - Web3 命令行钱包",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(hint),
        Line::from(""),
        Line::from(Span::styled(
            format!("  > {masked}"),
            Style::default().fg(Color::Yellow),
        )),
    ];

    if let Some(ref msg) = state.status_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {msg}"),
            Style::default().fg(Color::Red),
        )));
    }

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines).block(block);

    frame.render_widget(paragraph, center);
}
