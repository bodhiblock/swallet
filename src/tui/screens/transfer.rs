use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::chain::format_balance;
use crate::tui::state::{TransferStep, UiState};

pub fn render(frame: &mut Frame, state: &UiState) {
    let area = frame.area();

    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(16),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(70),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    match state.transfer_step {
        TransferStep::SelectAsset => render_select_asset(frame, center, state),
        TransferStep::InputAddress => render_input_address(frame, center, state),
        TransferStep::InputAmount => render_input_amount(frame, center, state),
        TransferStep::Confirm => render_confirm(frame, center, state),
        TransferStep::Sending => render_sending(frame, center),
        TransferStep::Result => render_result(frame, center, state),
    }
}

fn render_select_asset(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let block = Block::default()
        .title(" 转账 - 选择资产 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [info_area, list_area, footer_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    // From address + label
    frame.render_widget(Paragraph::new(Line::from(from_spans(state))), info_area);

    // Asset list
    let items: Vec<ListItem> = state
        .transfer_assets
        .iter()
        .map(|a| {
            ListItem::new(Line::from(Span::styled(
                format!("  {}", a.display_label()),
                Style::default().fg(Color::White),
            )))
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let mut list_state = ListState::default();
    list_state.select(Some(state.transfer_asset_selected));
    frame.render_stateful_widget(list, list_area, &mut list_state);

    // Footer
    let footer = Paragraph::new(Line::from(Span::styled(
        " ↑↓选择  Enter确认  Esc返回",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(footer, footer_area);
}

fn render_input_address(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let asset_label = state
        .transfer_assets
        .get(state.transfer_asset_selected)
        .map(|a| a.display_label())
        .unwrap_or_default();

    let mut lines = vec![
        Line::from(""),
        Line::from(from_spans(state)),
        Line::from(vec![
            Span::styled(" 资产: ", Style::default().fg(Color::DarkGray)),
            Span::styled(asset_label, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 目标地址:",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" > {}", state.transfer_to_address),
            Style::default().fg(Color::Yellow),
        )),
    ];

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认  Esc返回");

    let block = Block::default()
        .title(" 转账 - 输入地址 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_input_amount(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let asset_label = state
        .transfer_assets
        .get(state.transfer_asset_selected)
        .map(|a| a.display_label())
        .unwrap_or_default();

    let mut lines = vec![
        Line::from(""),
        Line::from(from_spans(state)),
        Line::from(vec![
            Span::styled(" 资产: ", Style::default().fg(Color::DarkGray)),
            Span::styled(asset_label, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled(" 到: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                state.transfer_to_address.as_str(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 转账数量:",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" > {}", state.transfer_amount),
            Style::default().fg(Color::Yellow),
        )),
    ];

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认  Esc返回");

    let block = Block::default()
        .title(" 转账 - 输入数量 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_confirm(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let asset = state
        .transfer_assets
        .get(state.transfer_asset_selected);
    let asset_label = asset.map(|a| a.display_label()).unwrap_or_default();
    let symbol = asset.map(|a| a.symbol.as_str()).unwrap_or("");
    let decimals = asset.map(|a| a.decimals).unwrap_or(0);

    // Format amount for display
    let amount_display = if let Ok(raw) = crate::transfer::parse_amount(&state.transfer_amount, decimals) {
        format!("{} {}", format_balance(raw, decimals), symbol)
    } else {
        format!("{} {}", state.transfer_amount, symbol)
    };

    let masked: String = "*".repeat(state.transfer_confirm_password.len());

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " 请确认转账信息:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(from_spans(state)),
        Line::from(vec![
            Span::styled("   到: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                state.transfer_to_address.as_str(),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 资产: ", Style::default().fg(Color::DarkGray)),
            Span::styled(asset_label, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled(" 数量: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                amount_display,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 请输入密码确认:",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" > {masked}"),
            Style::default().fg(Color::Yellow),
        )),
    ];

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认执行  Esc取消");

    let block = Block::default()
        .title(" 确认转账 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_sending(frame: &mut Frame, area: ratatui::layout::Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "   正在发送交易，请稍候...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let block = Block::default()
        .title(" 转账中 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_result(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let (success, message) = state
        .transfer_result
        .as_ref()
        .cloned()
        .unwrap_or((false, "未知状态".into()));

    let (icon, color) = if success {
        ("OK", Color::Green)
    } else {
        ("FAIL", Color::Red)
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            format!("   [{icon}] {}", if success { "交易已发送" } else { "交易失败" }),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    // Wrap long messages
    if message.len() > 50 {
        lines.push(Line::from(Span::styled(
            format!("   {}", &message[..50]),
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(Span::styled(
            format!("   {}", &message[50..]),
            Style::default().fg(Color::White),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("   {message}"),
            Style::default().fg(Color::White),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "   按任意键返回",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" 转账结果 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn append_status(lines: &mut Vec<Line<'_>>, state: &UiState) {
    if let Some(ref msg) = state.status_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("   {msg}"),
            Style::default().fg(Color::Red),
        )));
    }
}

fn append_hint<'a>(lines: &mut Vec<Line<'a>>, hint: &'a str) {
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));
}

/// 构建 "从: 地址 (备注)" 的 spans
fn from_spans<'a>(state: &'a UiState) -> Vec<Span<'a>> {
    let mut spans = vec![
        Span::styled(" 从: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            state.transfer_from_address.as_str(),
            Style::default().fg(Color::Yellow),
        ),
    ];
    if let Some(ref label) = state.transfer_from_label
        && !label.is_empty()
    {
        spans.push(Span::styled(
            format!(" ({label})"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans
}
