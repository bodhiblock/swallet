use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render(frame: &mut Frame) {
    let area = frame.area();

    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(14),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(50),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  DEX / Swap",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  支持的协议（开发中）："),
        Line::from(""),
        Line::from(Span::styled(
            "  EVM:  Uniswap V3 / 1inch",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "  SOL:  Jupiter / Orca",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  此功能正在开发中，敬请期待",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  按 Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(" Swap ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, center);
}
