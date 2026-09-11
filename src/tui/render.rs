use crate::models::{ActivityType, WalletTier};
use crate::tui::app::App;
use chrono::FixedOffset;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs},
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

    // 1. Tab Bar Navigation [1]-[5]
    let tab_titles = vec![
        Line::from(vec![
            Span::styled(
                "[1]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" LIVE STREAM ⚡"),
        ]),
        Line::from(vec![
            Span::styled(
                "[2]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" TARGET WALLETS 🎯"),
        ]),
        Line::from(vec![
            Span::styled(
                "[3]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" WHALE GEMS RADAR 💎"),
        ]),
        Line::from(vec![
            Span::styled(
                "[4]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" SHADOW POSITIONS 💼"),
        ]),
        Line::from(vec![
            Span::styled(
                "[5]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" LATENCY & STATS 📊"),
        ]),
    ];

    let tabs = Tabs::new(tab_titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ZENITHSW - SMART MONEY SHADOW BOT "),
        )
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
        2 => render_tab_radar(frame, chunks[1], app),
        3 => render_tab_positions(frame, chunks[1], app),
        4 => render_tab_stats(frame, chunks[1], app),
        _ => {}
    }

    // 3. Footer / Status bar
    let ipc_status = if app.is_connected_ipc {
        Span::styled(
            " [ENGINE: ONLINE 🟢] ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " [ENGINE: STANDALONE/OFFLINE 🟡] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    };

    let sol_count = app
        .wallets
        .iter()
        .filter(|w| w.chain == crate::models::Chain::Solana)
        .count();
    let rh_count = app
        .wallets
        .iter()
        .filter(|w| w.chain == crate::models::Chain::Robinhood)
        .count();
    let evm_count = app
        .wallets
        .iter()
        .filter(|w| {
            w.chain == crate::models::Chain::Ethereum || w.chain == crate::models::Chain::Base
        })
        .count();

    let qualified_str = if rh_count == 0 && evm_count == 0 {
        format!("{} SOLANA ⚡", sol_count)
    } else {
        format!(
            "{} ({} SOL, {} RH, {} EVM)",
            app.wallets.len(),
            sol_count,
            rh_count,
            evm_count
        )
    };

    let footer_text = Line::from(vec![
        ipc_status,
        Span::raw(" | "),
        Span::styled("[1-5]", Style::default().fg(Color::Cyan)),
        Span::raw(" Switch Tabs | "),
        Span::styled("[Q]", Style::default().fg(Color::Red)),
        Span::raw(" Quit | Qualified Wallets: "),
        Span::styled(qualified_str, Style::default().fg(Color::Yellow)),
        Span::raw(" | Tracked Gems: "),
        Span::styled(
            format!("{}", app.tracked_tokens.len()),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" | Active Positions: "),
        Span::styled(
            format!("{}", app.positions.len()),
            Style::default().fg(Color::Green),
        ),
    ]);

    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);

    frame.render_widget(footer, chunks[2]);
}

fn render_tab_stream(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(area);

    // Left: Live Whale Activities
    let mut act_items = Vec::new();
    for act in app.activities.iter().rev().take(15) {
        let wib_dt = act
            .timestamp
            .with_timezone(&FixedOffset::east_opt(7 * 3600).unwrap());
        let ts_str = wib_dt.format("%H:%M:%S").to_string();

        let (type_color, type_str) = match act.activity_type {
            ActivityType::Buy => (Color::Green, "BOUGHT 🟢".to_string()),
            ActivityType::Sell => {
                if act.sell_ratio_pct > 0.0 && act.sell_ratio_pct < 95.0 {
                    (
                        Color::LightRed,
                        format!("SOLD {:.0}% PARTIAL 🟡", act.sell_ratio_pct),
                    )
                } else {
                    (Color::Red, "SOLD 100% FULL DUMP 🔴".to_string())
                }
            }
            ActivityType::Transfer => (Color::Yellow, "TRANSFERRED".to_string()),
        };

        let line1 = Line::from(vec![
            Span::styled(
                format!("[{}] ", ts_str),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("[{}] ", act.chain),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("@{} ", act.wallet_handle),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                type_str,
                Style::default().fg(type_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ${:.2}", act.amount_usd),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        let mint_display = if act.token_mint.len() > 18 {
            format!(
                "{}...{}",
                &act.token_mint[..8],
                &act.token_mint[act.token_mint.len() - 6..]
            )
        } else {
            act.token_mint.clone()
        };

        let line2 = Line::from(vec![
            Span::raw("   └─ Token: "),
            Span::styled(
                &act.token_symbol,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Contract: "),
            Span::styled(
                mint_display,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]);

        act_items.push(ListItem::new(vec![line1, line2]));
    }

    let act_list = List::new(act_items).block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            " Real-Time Smart Feed (Multi-Chain Stream & Smart Contract Ingestion) ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
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
            Span::styled(
                format!("[{}] ", sig.chain),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                &sig.token_symbol,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" -> "),
            Span::styled(
                status_text,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        let mint_display = if sig.token_mint.len() > 18 {
            format!(
                "{}...{}",
                &sig.token_mint[..8],
                &sig.token_mint[sig.token_mint.len() - 6..]
            )
        } else {
            sig.token_mint.clone()
        };

        let mut sublines = vec![line1];
        if let Some(reason) = &sig.rejection_reason {
            sublines.push(Line::from(vec![
                Span::raw("   └─ Contract: "),
                Span::styled(mint_display, Style::default().fg(Color::DarkGray)),
                Span::raw(" | Reason: "),
                Span::styled(reason, Style::default().fg(Color::Red)),
            ]));
        } else {
            sublines.push(Line::from(vec![
                Span::raw("   └─ Contract: "),
                Span::styled(mint_display, Style::default().fg(Color::DarkGray)),
                Span::raw(" | Delta: "),
                Span::styled(
                    format!("+{:.2}%", sig.price_delta_pct),
                    Style::default().fg(Color::Green),
                ),
                Span::raw(" | Target: "),
                Span::styled(
                    format!("@{}", sig.whale_handle),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }

        sig_items.push(ListItem::new(sublines));
    }

    let sig_list = List::new(sig_items).block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            " Gating Matrix (Max Delta +10.0% | Min Liq $5k | Pure Whale Mirror) ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    );
    frame.render_widget(sig_list, chunks[1]);
}

fn render_tab_wallets(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Rank",
        "Name / Profile",
        "Source",
        "Handle",
        "Chain",
        "Tier",
        "Total Realized PnL",
        "Win Rate",
        "Trades",
        "Wallet Address / Contract",
        "Status",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    // Count shadows per master handle
    let mut shadow_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for w in &app.wallets {
        if w.handle.contains("_sub_")
            || w.source.contains("CLUSTER")
            || w.source.contains("DETECTIVE")
        {
            let root = w
                .handle
                .split("_sub_")
                .next()
                .unwrap_or(&w.handle)
                .to_string();
            *shadow_counts.entry(root).or_insert(0) += 1;
        }
    }

    // Filter to master wallets to prevent flat table duplication
    let displayed_wallets: Vec<&crate::models::SmartWallet> = app
        .wallets
        .iter()
        .filter(|w| {
            // Include non-shadow wallets, or include shadow wallets only if their root master is not in the list
            if w.handle.contains("_sub_") {
                let root = w.handle.split("_sub_").next().unwrap_or(&w.handle);
                !app.wallets.iter().any(|m| m.handle == root)
            } else {
                true
            }
        })
        .collect();

    let rows: Vec<Row> = displayed_wallets
        .iter()
        .map(|w| {
            let (tier_color, tier_str, status_str) = match w.tier {
                WalletTier::TierS => (Color::Rgb(255, 215, 0), "[TIER-S 👑]", "EXECUTE 🟢"),
                WalletTier::TierA => (Color::Rgb(0, 200, 255), "[TIER-A ⚡]", "EXECUTE 🟢"),
                WalletTier::TierB => (Color::DarkGray, "[TIER-B]", "MONITOR 🟡"),
            };

            let shadow_count = shadow_counts.get(&w.handle).copied().unwrap_or(0);

            let source_str = if shadow_count > 0 {
                "[CLUSTER 🕵️]"
            } else if w.source.contains("CLUSTER") || w.source.contains("DETECTIVE") {
                "[DETECTIVE 🕵️]"
            } else if w.source.contains("NANSEN") {
                "[NANSEN 🧠]"
            } else if w.source.contains("DUNE") {
                "[DUNE 🎯]"
            } else if w.source.contains("OKX") {
                "[OKX 🌐]"
            } else if w.source.contains("SOLANA_TRACKER") {
                "[TRACKER ⚡]"
            } else if w.source.contains("ONCHAIN") {
                "[ONCHAIN 🌐]"
            } else if w.source.contains("FOMO") {
                "[FOMO ⚡]"
            } else {
                "[SMART 🎯]"
            };

            let formatted_pnl = if w.pnl_24h_usd >= 1_000_000.0 {
                format!("+${:.2}M", w.pnl_24h_usd / 1_000_000.0)
            } else if w.pnl_24h_usd >= 1_000.0 {
                format!("+${:.1}k", w.pnl_24h_usd / 1_000.0)
            } else {
                format!("+${:.2}", w.pnl_24h_usd)
            };

            let addr_display = if w.address.len() > 18 {
                format!(
                    "{}...{}",
                    &w.address[..8],
                    &w.address[w.address.len() - 6..]
                )
            } else {
                w.address.clone()
            };

            let name_display = if shadow_count > 0 {
                format!("{} (+{} shadows 🕵️)", w.name, shadow_count)
            } else {
                w.name.clone()
            };

            Row::new(vec![
                format!("#{:02}", w.rank),
                name_display,
                source_str.to_string(),
                format!("@{}", w.handle),
                w.chain.to_string(),
                tier_str.to_string(),
                formatted_pnl,
                format!("{:.1}%", w.win_rate_pct),
                w.total_trades.to_string(),
                addr_display,
                status_str.to_string(),
            ])
            .style(Style::default().fg(tier_color))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Percentage(18),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(8),
            Constraint::Percentage(9),
            Constraint::Percentage(11),
            Constraint::Percentage(7),
            Constraint::Percentage(6),
            Constraint::Percentage(14),
            Constraint::Percentage(9),
        ],
    )
    .header(header)
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            format!(
                " SMART MONEY TARGET WALLETS (SHOWING {} CLUSTERS / MASTERS | TOTAL TRACKED: {}) ",
                displayed_wallets.len(),
                app.wallets.len()
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    );

    frame.render_widget(table, area);
}

fn render_tab_radar(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Rank",
        "Token",
        "Chain",
        "Contract / Mint Address",
        "Whale Cluster",
        "Total Whale ($)",
        "MCap & Upside Potential",
        "Pool Liquidity",
        "Price (USD)",
        "24h / 1h",
        "Market Structure & Action",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .tracked_tokens
        .iter()
        .enumerate()
        .map(|(idx, t)| {
            let status_color =
                if t.status.contains("ACCUMULATION_BOTTOM") || t.status.contains("Ready Buy") {
                    Color::Green
                } else if t.status.contains("PULLBACK") || t.status.contains("OVERSOLD") {
                    Color::Yellow
                } else if t.status.contains("LOCAL_TOP") {
                    Color::Red
                } else {
                    Color::White
                };

            let upside_style = if t.upside_tier.contains("NANO_CAP") {
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD)
            } else if t.upside_tier.contains("MICRO_CAP") {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if t.upside_tier.contains("LOW_CAP") {
                Style::default().fg(Color::LightBlue)
            } else if t.upside_tier.contains("SMALL_CAP") {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let mint_display = if t.token_mint.len() > 18 {
                format!(
                    "{}...{}",
                    &t.token_mint[..8],
                    &t.token_mint[t.token_mint.len() - 6..]
                )
            } else {
                t.token_mint.clone()
            };

            let mcap_str = if t.market_cap_usd >= 1_000_000.0 {
                format!("${:.1}M", t.market_cap_usd / 1_000_000.0)
            } else {
                format!("${:.0}k", t.market_cap_usd / 1_000.0)
            };

            let whale_usd_str = if t.total_whale_usd >= 1_000_000.0 {
                format!("${:.2}M", t.total_whale_usd / 1_000_000.0)
            } else if t.total_whale_usd >= 1_000.0 {
                format!("${:.1}k", t.total_whale_usd / 1_000.0)
            } else {
                format!("${:.0}", t.total_whale_usd)
            };

            let liq_str = if t.liquidity_usd >= 1_000_000.0 {
                format!("${:.2}M", t.liquidity_usd / 1_000_000.0)
            } else if t.liquidity_usd >= 1_000.0 {
                format!("${:.1}k", t.liquidity_usd / 1_000.0)
            } else {
                format!("${:.0}", t.liquidity_usd)
            };

            let cluster_display = format!("{} Whales ({})", t.whale_count, t.top_whale_handles);
            let upside_full = format!("{} [{}]", mcap_str, t.upside_tier);

            Row::new(vec![
                Cell::from(format!("#{}", idx + 1)).style(
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(t.token_symbol.clone()).style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(t.chain.to_string()),
                Cell::from(mint_display).style(Style::default().fg(Color::DarkGray)),
                Cell::from(cluster_display),
                Cell::from(whale_usd_str).style(Style::default().fg(Color::Green)),
                Cell::from(upside_full).style(upside_style),
                Cell::from(liq_str),
                Cell::from(format!("${:.6}", t.price_usd)),
                Cell::from(format!(
                    "{:+.1}% / {:+.1}%",
                    t.change_24h_pct, t.change_1h_pct
                )),
                Cell::from(t.status.clone()).style(Style::default().fg(status_color)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Percentage(9),
            Constraint::Percentage(7),
            Constraint::Percentage(14),
            Constraint::Percentage(18),
            Constraint::Percentage(9),
            Constraint::Percentage(13),
            Constraint::Percentage(8),
            Constraint::Percentage(8),
            Constraint::Percentage(8),
            Constraint::Percentage(12),
        ],
    )
    .header(header)
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            " WHALE GEMS RADAR (TOP 50 7D HOLDINGS • MICRO/SMALL-CAP UPSIDE & BOTTOM CONFIRMATION) ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    );

    frame.render_widget(table, area);
}

fn render_tab_positions(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Symbol",
        "Chain",
        "Contract / Mint Address",
        "Target Whale",
        "Entry Price",
        "Current Price",
        "Bag %",
        "PnL ($)",
        "PnL (%)",
        "Status",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
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
            let mint_display = if pos.token_mint.len() > 18 {
                format!(
                    "{}...{}",
                    &pos.token_mint[..8],
                    &pos.token_mint[pos.token_mint.len() - 6..]
                )
            } else {
                pos.token_mint.clone()
            };

            let handle_display = if pos.target_whale_handle.starts_with('@') {
                pos.target_whale_handle.clone()
            } else {
                format!("@{}", pos.target_whale_handle)
            };

            Row::new(vec![
                pos.token_symbol.clone(),
                pos.chain.to_string(),
                mint_display,
                handle_display,
                format!("${:.6}", pos.entry_price),
                format!("${:.6}", pos.current_price),
                portion_str,
                format!(
                    "${:+.2}",
                    (pos.amount_usd * (pos.remaining_portion_pct / 100.0))
                        * (pos.unrealized_pnl_pct / 100.0)
                ),
                format!("{:+.2}%", pos.unrealized_pnl_pct),
                pos.status.clone(),
            ])
            .style(Style::default().fg(pnl_color))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(10),
            Constraint::Percentage(8),
            Constraint::Percentage(18),
            Constraint::Percentage(13),
            Constraint::Percentage(11),
            Constraint::Percentage(11),
            Constraint::Percentage(8),
            Constraint::Percentage(10),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
        ],
    )
    .header(header)
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            " ACTIVE SHADOW POSITIONS (LIVE REAL-TIME FLOATING PNL & CONTRACT DISPLAY) ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
    );

    frame.render_widget(table, area);
}

fn render_tab_stats(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(area);

    let info_text = vec![
        Line::from(vec![Span::styled(
            "Multi-Chain Stream Health & Network Latencies:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::raw(" • Solana (Helius Dedicated WS/RPC): "),
            Span::styled(
                format!("{} ms", app.solana_latency_ms),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Jito Anti-MEV Bundles: Active 🟢"),
        ]),
        Line::from(vec![
            Span::raw(" • Robinhood / Arbitrum (Alchemy EVM): "),
            Span::styled(
                format!("{} ms", app.arbitrum_latency_ms),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Full Asset Transfers Stream: Active 🟢"),
        ]),
        Line::from(vec![
            Span::raw(" • Exit Mechanism: "),
            Span::styled(
                "Pure Whale Mirror (Proportional Partial & Full Exit)",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw(" • Active Calibrated Gating: Max Entry Delta = "),
            Span::styled(
                "+10.0%",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Min Pool Liquidity = "),
            Span::styled(
                "$5,000",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Position Size = "),
            Span::styled(
                "$5.00",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let info_block = Paragraph::new(info_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Pipeline Latency & Active Settings "),
    );
    frame.render_widget(info_block, chunks[0]);

    // Trade History Table
    let header = Row::new(vec![
        "Timestamp (WIB)",
        "Chain",
        "Action",
        "Symbol",
        "Contract / Mint",
        "Whale Target",
        "Entry",
        "Exit",
        "PnL (%)",
        "PnL ($)",
        "Exit Reason",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
    .bottom_margin(1);

    let rows: Vec<Row> = app
        .trades
        .iter()
        .map(|tr| {
            let wib_dt = tr
                .timestamp
                .with_timezone(&FixedOffset::east_opt(7 * 3600).unwrap());
            let ts_str = wib_dt.format("%H:%M:%S").to_string();
            let pnl_color = if tr.pnl_pct >= 0.0 {
                Color::Green
            } else {
                Color::Red
            };

            let mint_display = if tr.mint.len() > 16 {
                format!("{}...{}", &tr.mint[..7], &tr.mint[tr.mint.len() - 5..])
            } else {
                tr.mint.clone()
            };

            Row::new(vec![
                ts_str,
                tr.chain.to_string(),
                tr.action.clone(),
                tr.symbol.clone(),
                mint_display,
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
            Constraint::Percentage(10),
            Constraint::Percentage(8),
            Constraint::Percentage(9),
            Constraint::Percentage(8),
            Constraint::Percentage(16),
            Constraint::Percentage(13),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(8),
            Constraint::Percentage(8),
            Constraint::Percentage(14),
        ],
    )
    .header(header)
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            " RECENT SHADOW EXECUTIONS AUDIT LOG (WITH SMART CONTRACT MINT) ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    );

    frame.render_widget(table, chunks[1]);
}
