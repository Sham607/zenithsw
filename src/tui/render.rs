use crate::models::{ActivityType, WalletTier};
use crate::tui::app::App;
use chrono::FixedOffset;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table, Tabs},
    Frame,
};

pub fn render_ui(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab navigation
            Constraint::Min(0),    // Main content
            Constraint::Length(3), // Footer / status
        ])
        .split(frame.area());

    // 1. Tab Bar Navigation [1]-[4]
    let tab_titles = vec![
        Line::from(vec![
            Span::styled("[1]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" LIVE STREAM ⚡"),
        ]),
        Line::from(vec![
            Span::styled("[2]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" TARGET WALLETS 🎯"),
        ]),
        Line::from(vec![
            Span::styled("[3]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" SHADOW POSITIONS 💼"),
        ]),
        Line::from(vec![
            Span::styled("[4]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" LATENCY & STATS 📊"),
        ]),
    ];

    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title(" ZENITHSW - SMART MONEY SHADOW BOT "))
        .select(app.current_tab)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        );

    frame.render_widget(tabs, chunks[0]);

    // 2. Main Content based on active tab
    match app.current_tab {
        0 => render_tab_stream(frame, chunks[1], app),
        1 => render_tab_wallets(frame, chunks[1], app),
        2 => render_tab_positions(frame, chunks[1], app),
        3 => render_tab_stats(frame, chunks[1], app),
        _ => {}
    }

    // 3. Footer / Status bar
    let ipc_status = if app.is_connected_ipc {
        Span::styled(" [ENGINE: ONLINE 🟢] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" [ENGINE: STANDALONE/OFFLINE 🟡] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    };

    let footer_text = Line::from(vec![
        ipc_status,
        Span::raw(" | "),
        Span::styled("[1-4]", Style::default().fg(Color::Cyan)),
        Span::raw(" Switch Tabs | "),
        Span::styled("[Q]", Style::default().fg(Color::Red)),
        Span::raw(" Quit | Tracked Wallets: "),
        Span::styled(format!("{}", app.wallets.len()), Style::default().fg(Color::Yellow)),
        Span::raw(" | Active Positions: "),
        Span::styled(format!("{}", app.positions.len()), Style::default().fg(Color::Green)),
    ]);

    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);

    frame.render_widget(footer, chunks[2]);
}

fn render_tab_stream(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // Left: Live Whale Activities
    let mut act_items = Vec::new();
    for act in app.activities.iter().rev().take(15) {
        let wib_dt = act.timestamp.with_timezone(&FixedOffset::east_opt(7 * 3600).unwrap());
        let ts_str = wib_dt.format("%H:%M:%S").to_string();

        let (type_color, type_str) = match act.activity_type {
            ActivityType::Buy => (Color::Green, "BOUGHT 🟢".to_string()),
            ActivityType::Sell => {
                if act.sell_ratio_pct > 0.0 && act.sell_ratio_pct < 95.0 {
                    (Color::LightRed, format!("SOLD {:.0}% PARTIAL 🟡", act.sell_ratio_pct))
                } else {
                    (Color::Red, "SOLD 100% FULL DUMP 🔴".to_string())
                }
            }
            ActivityType::Transfer => (Color::Yellow, "TRANSFERRED".to_string()),
        };

        let line1 = Line::from(vec![
            Span::styled(format!("[{}] ", ts_str), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("[{}] ", act.chain), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!("@{} ", act.wallet_handle), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(type_str, Style::default().fg(type_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" ${:.2}", act.amount_usd), Style::default().fg(Color::Yellow)),
        ]);

        let line2 = Line::from(vec![
            Span::raw("   └─ Token: "),
            Span::styled(&act.token_symbol, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::raw(" | Mint: "),
            Span::styled(
                if act.token_mint.len() > 14 {
                    format!("{}...{}", &act.token_mint[..6], &act.token_mint[act.token_mint.len() - 4..])
                } else {
                    act.token_mint.clone()
                },
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        act_items.push(ListItem::new(vec![line1, line2]));
    }

    let act_list = List::new(act_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Real-Time Smart Feed (Sub-Second Ingestion & Bag Tracker) ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        );
    frame.render_widget(act_list, chunks[0]);

    // Right: Shadow Confluence Signals & Decisions
    let mut sig_items = Vec::new();
    for sig in app.signals.iter().rev().take(15) {
        let (status_color, status_text) = if sig.is_passed {
            (Color::Green, "COPIED [BUY ENTRY] ✅")
        } else {
            (Color::Red, "REJECTED ❌")
        };

        let line1 = Line::from(vec![
            Span::styled(format!("[{}] ", sig.chain), Style::default().fg(Color::Cyan)),
            Span::styled(&sig.token_symbol, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::raw(" -> "),
            Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]);

        let mut sublines = vec![line1];
        if let Some(reason) = &sig.rejection_reason {
            sublines.push(Line::from(vec![
                Span::raw("   └─ Reason: "),
                Span::styled(reason, Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            sublines.push(Line::from(vec![
                Span::raw("   └─ Delta: "),
                Span::styled(format!("+{:.2}%", sig.price_delta_pct), Style::default().fg(Color::Green)),
                Span::raw(" | Target: "),
                Span::styled(format!("@{}", sig.whale_handle), Style::default().fg(Color::Yellow)),
            ]));
        }

        sig_items.push(ListItem::new(sublines));
    }

    let sig_list = List::new(sig_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Gating Matrix (Max Delta +5.0% | Pure Whale Mirror) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        );
    frame.render_widget(sig_list, chunks[1]);
}

fn render_tab_wallets(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Rank", "Name / Profile", "Source", "Handle", "Chain", "Tier", "Total Realized PnL", "Win Rate", "Trades", "Status",
    ])
    .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .wallets
        .iter()
        .map(|w| {
            let (tier_color, tier_str, status_str) = match w.tier {
                WalletTier::TierS => (Color::Rgb(255, 215, 0), "[TIER-S 👑]", "EXECUTE 🟢"),
                WalletTier::TierA => (Color::Rgb(0, 200, 255), "[TIER-A ⚡]", "EXECUTE 🟢"),
                WalletTier::TierB => (Color::DarkGray, "[TIER-B]", "MONITOR 🟡"),
            };

            let source_str = if w.source == "ONCHAIN_INDEXER" {
                "[ONCHAIN 🌐]"
            } else {
                "[FOMO ⚡]"
            };

            let formatted_pnl = if w.pnl_24h_usd >= 1_000_000.0 {
                format!("+${:.2}M", w.pnl_24h_usd / 1_000_000.0)
            } else if w.pnl_24h_usd >= 1_000.0 {
                format!("+${:.1}k", w.pnl_24h_usd / 1_000.0)
            } else {
                format!("+${:.2}", w.pnl_24h_usd)
            };

            Row::new(vec![
                format!("#{:02}", w.rank),
                w.name.clone(),
                source_str.to_string(),
                format!("@{}", w.handle),
                w.chain.to_string(),
                tier_str.to_string(),
                formatted_pnl,
                format!("{:.1}%", w.win_rate_pct),
                w.total_trades.to_string(),
                status_str.to_string(),
            ])
            .style(Style::default().fg(tier_color))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Percentage(16),
            Constraint::Percentage(12),
            Constraint::Percentage(14),
            Constraint::Percentage(9),
            Constraint::Percentage(11),
            Constraint::Percentage(13),
            Constraint::Percentage(9),
            Constraint::Percentage(8),
            Constraint::Percentage(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" SMART MONEY TARGET WALLETS (FOMO + DIRECT ON-CHAIN INDEXER) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
    );

    frame.render_widget(table, area);
}

fn render_tab_positions(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        "ID / Symbol", "Chain", "Target Whale", "Entry Price", "Current Price", "Bag Remaining", "PnL ($)", "PnL (%)", "Status",
    ])
    .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .positions
        .iter()
        .map(|pos| {
            let pnl_color = if pos.unrealized_pnl_pct >= 0.0 {
                Color::Green
            } else {
                Color::Red
            };

            let portion_str = format!("{:.0}%", pos.remaining_portion_pct);

            Row::new(vec![
                pos.token_symbol.clone(),
                pos.chain.to_string(),
                format!("@{}", pos.target_whale_handle),
                format!("${:.6}", pos.entry_price),
                format!("${:.6}", pos.current_price),
                portion_str,
                format!("${:+.2}", (pos.amount_usd * (pos.remaining_portion_pct / 100.0)) * (pos.unrealized_pnl_pct / 100.0)),
                format!("{:+.2}%", pos.unrealized_pnl_pct),
                pos.status.clone(),
            ])
            .style(Style::default().fg(pnl_color))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(14),
            Constraint::Percentage(10),
            Constraint::Percentage(16),
            Constraint::Percentage(13),
            Constraint::Percentage(13),
            Constraint::Percentage(12),
            Constraint::Percentage(11),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" ACTIVE SHADOW POSITIONS (PURE PROPORTIONAL WHALE MIRROR EXIT) ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
    );

    frame.render_widget(table, area);
}

fn render_tab_stats(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(area);

    let info_text = vec![
        Line::from(vec![
            Span::styled("Multi-Chain Stream Health & Network Latencies:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw(" • Solana (Helius Dedicated WS/RPC): "),
            Span::styled(format!("{} ms", app.solana_latency_ms), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" | Jito Anti-MEV Bundles: Active 🟢"),
        ]),
        Line::from(vec![
            Span::raw(" • Direct On-Chain DEX Graph Indexer: "),
            Span::styled("Active 🟢 (Raydium / Pump.fun Exploding Pool Reverse Profiler)", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw(" • Exit Mechanism: "),
            Span::styled("Pure Whale Mirror (Proportional Partial & Full Exit)", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw(" • Calibrated Gating: Max Entry Delta = "),
            Span::styled("+5.0%", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" | Min Pool Liquidity = "),
            Span::styled("$30,000", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let info_block = Paragraph::new(info_text)
        .block(Block::default().borders(Borders::ALL).title(" Pipeline Latency & Settings "));
    frame.render_widget(info_block, chunks[0]);

    // Trade History Table
    let header = Row::new(vec![
        "Timestamp (WIB)", "Chain", "Action", "Symbol", "Whale Target", "Entry", "Exit", "PnL (%)", "PnL ($)", "Exit Reason",
    ])
    .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .trades
        .iter()
        .map(|tr| {
            let wib_dt = tr.timestamp.with_timezone(&FixedOffset::east_opt(7 * 3600).unwrap());
            let ts_str = wib_dt.format("%H:%M:%S").to_string();
            let pnl_color = if tr.pnl_pct >= 0.0 { Color::Green } else { Color::Red };

            Row::new(vec![
                ts_str,
                tr.chain.to_string(),
                tr.action.clone(),
                tr.symbol.clone(),
                format!("@{}", tr.target_whale),
                format!("${:.6}", tr.entry_price),
                format!("${:.6}", tr.exit_price),
                format!("{:+.2}%", tr.pnl_pct),
                format!("${:+.2}", tr.pnl_usd),
                tr.exit_reason.clone(),
            ])
            .style(Style::default().fg(pnl_color))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(11),
            Constraint::Percentage(9),
            Constraint::Percentage(11),
            Constraint::Percentage(10),
            Constraint::Percentage(14),
            Constraint::Percentage(11),
            Constraint::Percentage(11),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
            Constraint::Percentage(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" RECENT SHADOW EXECUTIONS AUDIT LOG ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
    );

    frame.render_widget(table, chunks[1]);
}
