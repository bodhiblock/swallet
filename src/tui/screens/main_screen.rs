use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::storage::data::{WalletStore, WalletType};
use crate::tui::state::UiState;

pub fn render(frame: &mut Frame, state: &UiState, store: &WalletStore) {
    let area = frame.area();

    let [header_area, main_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .areas(area);

    render_header(frame, header_area);
    render_wallet_list(frame, main_area, state, store);
    render_footer(frame, footer_area, state);
}

fn render_header(frame: &mut Frame, area: ratatui::layout::Rect) {
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " swallet ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  总资产: "),
        Span::styled("暂未开放", Style::default().fg(Color::DarkGray)),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header, area);
}

fn render_wallet_list(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
    store: &WalletStore,
) {
    let mut items: Vec<ListItem> = Vec::new();

    let visible_wallets: Vec<_> = store.wallets.iter().filter(|w| !w.hidden).collect();

    if visible_wallets.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  暂无钱包，请添加",
            Style::default().fg(Color::DarkGray),
        ))));
    } else {
        for (i, wallet) in visible_wallets.iter().enumerate() {
            let type_label = match &wallet.wallet_type {
                WalletType::Mnemonic { .. } => "助记词钱包",
                WalletType::PrivateKey { .. } => "私钥钱包",
                WalletType::WatchOnly { .. } => "观察钱包",
            };

            items.push(ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{}. ", i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    wallet.name.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" [{type_label}]"),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));

            match &wallet.wallet_type {
                WalletType::Mnemonic {
                    eth_accounts,
                    sol_accounts,
                    ..
                } => {
                    for acc in eth_accounts.iter().filter(|a| !a.hidden) {
                        let label = format_label(&acc.label);
                        items.push(ListItem::new(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("ETH ", Style::default().fg(Color::Blue)),
                            Span::styled(label, Style::default().fg(Color::Yellow)),
                            Span::styled(
                                shorten_address(&acc.address),
                                Style::default().fg(Color::Gray),
                            ),
                        ])));
                    }
                    for acc in sol_accounts.iter().filter(|a| !a.hidden) {
                        let label = format_label(&acc.label);
                        items.push(ListItem::new(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("SOL ", Style::default().fg(Color::Magenta)),
                            Span::styled(label, Style::default().fg(Color::Yellow)),
                            Span::styled(
                                shorten_address(&acc.address),
                                Style::default().fg(Color::Gray),
                            ),
                        ])));
                    }
                }
                WalletType::PrivateKey { address, label, .. } => {
                    let label_str = format_label(label);
                    items.push(ListItem::new(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(label_str, Style::default().fg(Color::Yellow)),
                        Span::styled(
                            shorten_address(address),
                            Style::default().fg(Color::Gray),
                        ),
                    ])));
                }
                WalletType::WatchOnly { address, label, .. } => {
                    let label_str = format_label(label);
                    items.push(ListItem::new(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("👁 ", Style::default().fg(Color::DarkGray)),
                        Span::styled(label_str, Style::default().fg(Color::Yellow)),
                        Span::styled(
                            shorten_address(address),
                            Style::default().fg(Color::Gray),
                        ),
                    ])));
                }
            }
        }
    }

    // 添加钱包选项
    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(Span::styled(
        "  [+] 添加钱包",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    ))));

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_index));

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_footer(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let msg = state
        .status_message
        .as_deref()
        .unwrap_or("↑↓ 选择  Enter 操作  Ctrl+Q 退出");

    let footer = Paragraph::new(Line::from(Span::styled(
        format!(" {msg}"),
        Style::default().fg(Color::DarkGray),
    )))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(footer, area);
}

/// 格式化标签：Some("xxx") -> "[xxx] ", None -> ""
fn format_label(label: &Option<String>) -> String {
    label
        .as_deref()
        .map(|l| format!("[{l}] "))
        .unwrap_or_default()
}

/// 缩短地址显示：0x1234...abcd
fn shorten_address(addr: &str) -> String {
    if addr.len() > 16 {
        format!("{}...{}", &addr[..8], &addr[addr.len() - 6..])
    } else {
        addr.to_string()
    }
}
