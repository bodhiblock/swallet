use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::chain::solana::{STAKE_PROGRAM, VOTE_PROGRAM};
use crate::chain::{format_balance, BalanceCache};
use crate::storage::data::{WalletStore, WalletType};
use crate::tui::state::UiState;

pub fn render(
    frame: &mut Frame,
    state: &UiState,
    store: &WalletStore,
    balance_cache: &BalanceCache,
    loading: bool,
) {
    let area = frame.area();

    let [header_area, main_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .areas(area);

    render_header(frame, header_area, loading);
    render_wallet_list(frame, main_area, state, store, balance_cache);
    render_footer(frame, footer_area, state);
}

fn render_header(frame: &mut Frame, area: ratatui::layout::Rect, loading: bool) {
    let status = if loading { "刷新中..." } else { "r 刷新" };
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " swallet ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(Color::DarkGray)),
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
    cache: &BalanceCache,
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
                WalletType::Mnemonic { .. } => "助记词钱包".to_string(),
                WalletType::PrivateKey { .. } => "私钥钱包".to_string(),
                WalletType::WatchOnly { .. } => "观察钱包".to_string(),
                WalletType::Multisig { chain_name, .. } => format!("多签钱包 - {chain_name}"),
            };

            let mut title_spans = vec![
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
            ];
            if let WalletType::Multisig { multisig_address, .. } = &wallet.wallet_type {
                title_spans.push(Span::styled(
                    format!(" {multisig_address}"),
                    Style::default().fg(Color::Gray),
                ));
            }
            items.push(ListItem::new(Line::from(title_spans)));

            match &wallet.wallet_type {
                WalletType::Mnemonic {
                    eth_accounts,
                    sol_accounts,
                    ..
                } => {
                    for acc in eth_accounts.iter().filter(|a| !a.hidden) {
                        let label = format_label(&acc.label);
                        let mut spans = vec![
                            Span::raw("    "),
                            Span::styled(
                                format!("#{} ", acc.derivation_index),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled("ETH ", Style::default().fg(Color::Blue)),
                            Span::styled(label, Style::default().fg(Color::Yellow)),
                            Span::styled(
                                acc.address.clone(),
                                Style::default().fg(Color::Gray),
                            ),
                        ];
                        // 添加余额信息
                        append_balance_spans(&mut spans, cache, &acc.address, None);
                        items.push(ListItem::new(Line::from(spans)));
                    }
                    for acc in sol_accounts.iter().filter(|a| !a.hidden) {
                        let label = format_label(&acc.label);
                        let account_type_label = account_type_span(cache, &acc.address);
                        let mut spans = vec![
                            Span::raw("    "),
                            Span::styled(
                                format!("#{} ", acc.derivation_index),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled("SOL ", Style::default().fg(Color::Magenta)),
                        ];
                        if let Some(type_span) = account_type_label {
                            spans.push(type_span);
                        }
                        spans.push(Span::styled(label, Style::default().fg(Color::Yellow)));
                        spans.push(Span::styled(
                            acc.address.clone(),
                            Style::default().fg(Color::Gray),
                        ));
                        append_balance_spans(&mut spans, cache, &acc.address, None);
                        items.push(ListItem::new(Line::from(spans)));
                    }
                }
                WalletType::PrivateKey { address, label, .. } => {
                    let label_str = format_label(label);
                    let mut spans = vec![
                        Span::raw("    "),
                        Span::styled(label_str, Style::default().fg(Color::Yellow)),
                        Span::styled(
                            address.clone(),
                            Style::default().fg(Color::Gray),
                        ),
                    ];
                    append_balance_spans(&mut spans, cache, address, None);
                    items.push(ListItem::new(Line::from(spans)));
                }
                WalletType::WatchOnly { address, label, .. } => {
                    let label_str = format_label(label);
                    let mut spans = vec![
                        Span::raw("    "),
                        Span::styled("👁 ", Style::default().fg(Color::DarkGray)),
                        Span::styled(label_str, Style::default().fg(Color::Yellow)),
                        Span::styled(
                            address.clone(),
                            Style::default().fg(Color::Gray),
                        ),
                    ];
                    append_balance_spans(&mut spans, cache, address, None);
                    items.push(ListItem::new(Line::from(spans)));
                }
                WalletType::Multisig { vaults, chain_id, .. } => {
                    for v in vaults.iter().filter(|v| !v.hidden) {
                        let label = format_label(&v.label);
                        let mut spans = vec![
                            Span::raw("    "),
                            Span::styled(
                                format!("#{} ", v.vault_index),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(label, Style::default().fg(Color::Yellow)),
                            Span::styled(
                                v.address.clone(),
                                Style::default().fg(Color::Gray),
                            ),
                        ];
                        append_balance_spans(&mut spans, cache, &v.address, Some(chain_id));
                        items.push(ListItem::new(Line::from(spans)));
                    }
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
                .bg(Color::Indexed(236))
                .add_modifier(Modifier::BOLD),
        );

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_index));

    frame.render_stateful_widget(list, area, &mut list_state);
}

/// 在地址行后面追加余额摘要（filter_chain_id 限定只显示指定链）
fn append_balance_spans<'a>(
    spans: &mut Vec<Span<'a>>,
    cache: &BalanceCache,
    address: &str,
    filter_chain_id: Option<&str>,
) {
    let portfolio = match cache.get(address) {
        Some(p) => p,
        None => return,
    };

    if portfolio.chains.is_empty() {
        return;
    }

    spans.push(Span::raw("  "));

    let mut first = true;
    for chain_bal in portfolio.chains.iter().filter(|c| {
        filter_chain_id.is_none_or(|id| c.chain_id == id)
    }) {
        if !first {
            spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        }
        first = false;

        // [链名]
        spans.push(Span::styled(
            format!("[{}] ", chain_bal.chain_name),
            Style::default().fg(Color::DarkGray),
        ));

        if chain_bal.rpc_failed {
            // RPC 失败显示 -
            spans.push(Span::styled(
                format!("- {}", chain_bal.native_symbol),
                Style::default().fg(Color::DarkGray),
            ));
            continue;
        }

        // 原生币余额
        let bal_str = format_balance(chain_bal.native_balance, chain_bal.native_decimals);

        let stake_str = if chain_bal.staked_balance > 0 {
            let s = format_balance(chain_bal.staked_balance, chain_bal.native_decimals);
            format!(
                "{} {} (质押 {} {})",
                bal_str, chain_bal.native_symbol, s, chain_bal.native_symbol
            )
        } else {
            format!("{} {}", bal_str, chain_bal.native_symbol)
        };

        spans.push(Span::styled(
            stake_str,
            Style::default().fg(Color::Green),
        ));

        // 代币
        for token in &chain_bal.tokens {
            let t_bal = format_balance(token.balance, token.decimals);
            spans.push(Span::styled(
                format!(", {} {}", t_bal, token.symbol),
                Style::default().fg(Color::Green),
            ));
        }
    }
}

fn render_footer(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let msg = state
        .status_message
        .as_deref()
        .unwrap_or("↑↓ 选择  Enter 操作  r 刷新  s Swap  Ctrl+Q 退出");

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

/// 根据 account_owner 生成类型标签 Span
fn account_type_span<'a>(cache: &BalanceCache, address: &str) -> Option<Span<'a>> {
    let owner = cache.get(address)?.account_owner.as_deref()?;
    match owner {
        VOTE_PROGRAM => Some(Span::styled("[Vote] ", Style::default().fg(Color::Cyan))),
        STAKE_PROGRAM => Some(Span::styled("[Stake] ", Style::default().fg(Color::Green))),
        _ => None,
    }
}
