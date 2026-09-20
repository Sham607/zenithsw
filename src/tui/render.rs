use crate::models::{ActivityType, ExecutionTrack, WalletTier};
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

    let total_pos_usd: f64 = app
        .positions
        .iter()
        .map(|p| {
            (p.amount_usd * (p.remaining_portion_pct / 100.0))
                * (1.0 + p.unrealized_pnl_pct / 100.0)
        })
        .sum();

    let pubkey_short = if app.wallet_pubkey.len() > 10 {
        format!(
            "{}...{}",
            &app.wallet_pubkey[..4],
            &app.wallet_pubkey[app.wallet_pubkey.len() - 4..]
        )
    } else {
        "N/A".to_string()
    };
    let mode_str = if app.is_live_mode {
        "LIVE ON-CHAIN 🟢"
    } else {
        "DRY RUN 🟡"
    };
    let heartbeat_fresh = app.heartbeat_is_fresh();
    let sol_value = if heartbeat_fresh && app.sol_price_usd > 0.0 {
        format!("${:.2}", app.wallet_sol_balance * app.sol_price_usd)
    } else {
        "N/A".to_string()
    };
    let total_equity = if heartbeat_fresh && app.wallet_equity_usd.is_finite() {
        format!("${:.2}", app.wallet_equity_usd)
    } else {
        "N/A".to_string()
    };
    let realized = if heartbeat_fresh {
        format!("${:+.2}", app.total_realized_pnl_usd)
    } else {
        "N/A".to_string()
    };

    let header_title = format!(
        " ZENITHSW • {} • [{}] • SOL: {:.4} ({}) • POS: ${:.2} • TOTAL EQUITY: {} • REALIZED: {} ",
        mode_str,
        pubkey_short,
        app.wallet_sol_balance,
        sol_value,
        total_pos_usd,
        total_equity,
        realized
    );

    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title(header_title))
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
    let (ipc_label, ipc_color) = if app.heartbeat_is_fresh() {
        (" [ENGINE: ONLINE 🟢] ", Color::Green)
    } else if app.is_connected_ipc {
        (" [ENGINE: STALE HEARTBEAT 🔴] ", Color::Red)
    } else {
        (" [ENGINE: OFFLINE 🟡] ", Color::Yellow)
    };
    let ipc_status = Span::styled(
        ipc_label,
        Style::default().fg(ipc_color).add_modifier(Modifier::BOLD),
    );

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
        Span::raw(" Switch | "),
        Span::styled("[Q]", Style::default().fg(Color::Red)),
        Span::raw(" Quit | Wallets: "),
        Span::styled(qualified_str, Style::default().fg(Color::Yellow)),
        Span::raw(" | Real Equity: "),
        Span::styled(
            total_equity.clone(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" (SOL: "),
        Span::styled(
            format!("{:.4}", app.wallet_sol_balance),
            Style::default().fg(Color::White),
        ),
        Span::raw(" | Pos: "),
        Span::styled(
            format!("${:.2}", total_pos_usd),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(") | Realized PnL: "),
        Span::styled(
            format!("${:+.2}", app.total_realized_pnl_usd),
            Style::default().fg(if app.total_realized_pnl_usd >= 0.0 {
                Color::Green
            } else {
                Color::Red
            }),
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

        let sig_display = if act.tx_signature.len() > 12 {
            format!(
                "{}...{}",
                &act.tx_signature[..5],
                &act.tx_signature[act.tx_signature.len() - 4..]
            )
        } else if !act.tx_signature.is_empty() {
            act.tx_signature.clone()
        } else {
            "-".to_string()
        };

        let ver_badge = match act.parser_version.as_deref() {
            Some(v) if v.starts_with("dex-") => Span::styled(
                " [VERIFIED]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            _ => Span::styled(" [LEGACY]", Style::default().fg(Color::DarkGray)),
        };

        let line2 = Line::from(vec![
            Span::raw("   └─ Token: "),
            Span::styled(
                &act.token_symbol,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Mint: "),
            Span::styled(
                mint_display,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Span::raw(" | Tx: "),
            Span::styled(sig_display, Style::default().fg(Color::Cyan)),
            ver_badge,
        ]);

        act_items.push(ListItem::new(vec![line1, line2]));
    }

    if act_items.is_empty() {
        act_items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(
                    " [STANDBY] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Multi-Chain Stream Listener Active (Solana & Robinhood)",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("   ├─ Status: "),
                Span::styled(
                    "Listening to qualified wallets; monitor-only candidates excluded",
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(vec![
                Span::raw("   ├─ Pipeline: "),
                Span::styled(
                    "Paced RPC polling with cooldown and fail-closed errors",
                    Style::default().fg(Color::Green),
                ),
            ]),
            Line::from(vec![
                Span::raw("   ├─ Mode: "),
                Span::styled(
                    "Simulated Shadow Execution (DRY-RUN SAFE)",
                    Style::default().fg(Color::Magenta),
                ),
            ]),
            Line::from(vec![
                Span::raw("   └─ Info: "),
                Span::styled(
                    "Waiting for next on-chain DEX Swap. Press [2] for Wallet Registry.",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ]));
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

        let (track_badge, track_color) = match sig.track {
            ExecutionTrack::FastLane => ("[FAST ⚡] ", Color::Cyan),
            ExecutionTrack::VaultLane => ("[VAULT 🏛️] ", Color::Yellow),
        };

        let line1 = Line::from(vec![
            Span::styled(
                track_badge,
                Style::default()
                    .fg(track_color)
                    .add_modifier(Modifier::BOLD),
            ),
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

    let gating_title = format!(
        " Gating Matrix (Max Delta +{:.1}% | Min Liq ${:.0}k | Min Whale ${:.0} | Pos Size ${:.0} | SL -{:.0}% | TP +{:.0}%) ",
        app.max_price_delta_pct,
        app.min_liquidity_usd / 1000.0,
        app.min_whale_buy_usd,
        app.position_size_usd,
        app.stop_loss_pct,
        app.take_profit_pct
    );

    if sig_items.is_empty() {
        sig_items.push(ListItem::new(vec![
            Line::from(vec![
                Span::styled(
                    " [STANDBY] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Dual-Track Confluence Filter Ready",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("   ├─ Fast-Lane: "),
                Span::styled(
                    "Min Liq $15k | Max Delta 10% | Sniper Early Entry",
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(vec![
                Span::raw("   ├─ Vault-Lane: "),
                Span::styled(
                    "Min Liq $15k | Volume Validation | Whale Accumulation",
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::raw("   └─ Status: "),
                Span::styled(
                    "Awaiting first inbound swap to evaluate gating rules.",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ]));
    }

    let sig_list = List::new(sig_items).block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            gating_title,
            Style::default()
                .fg(Color::Yellow)
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
        "#",
        "Ticker",
        "Chain",
        "Mint",
        "Pool / DEX",
        "MCap",
        "Liquidity",
        "Price",
        "24H / 1H",
        "Whales",
        "Clusters",
        "Structure / Price Action",
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .radar_projections
        .iter()
        .enumerate()
        .map(|(idx, projection)| {
            let token = if projection.token_address.len() > 18 {
                format!(
                    "{}...{}",
                    &projection.token_address[..8],
                    &projection.token_address[projection.token_address.len() - 6..]
                )
            } else {
                projection.token_address.clone()
            };
            let status_color = match projection.status {
                crate::radar::RadarStatus::HighPriorityRadar => Color::Green,
                crate::radar::RadarStatus::StructureConfirmed => Color::Yellow,
                crate::radar::RadarStatus::PortfolioOnly => Color::White,
            };
            Row::new(vec![
                Cell::from(format!("#{}", idx + 1)),
                Cell::from(projection.symbol.as_deref().unwrap_or("UNKNOWN")),
                Cell::from(projection.chain.to_string()),
                Cell::from(token).style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!(
                    "{} / {}",
                    projection.pool_address.as_deref().unwrap_or("UNKNOWN"),
                    projection.dex_id.as_deref().unwrap_or("UNKNOWN")
                )),
                Cell::from(format_usd(projection.market_cap_usd)),
                Cell::from(format_usd(projection.liquidity_usd)),
                Cell::from(
                    projection
                        .price_usd
                        .map_or_else(|| "n/a".to_string(), |price| format!("${price:.8}")),
                ),
                Cell::from(format_change(
                    projection.change_24h_pct,
                    projection.change_1h_pct,
                )),
                Cell::from(format!(
                    "Q:{} C:{}",
                    projection.qualified_holder_count, projection.candidate_holder_count
                )),
                Cell::from(projection.independent_cluster_count.to_string()),
                Cell::from(format!(
                    "{} / {}",
                    projection.market_structure, projection.price_action
                ))
                .style(Style::default().fg(status_color)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Percentage(18),
            Constraint::Percentage(18),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(14),
            Constraint::Length(17),
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Percentage(24),
        ],
    )
    .header(header)
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            format!(
                " WHALE GEMS RADAR — {} PROJECTIONS ",
                app.radar_projections.len()
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    );

    frame.render_widget(table, area);
}

fn format_change(change_24h: Option<f64>, change_1h: Option<f64>) -> String {
    let h24 = change_24h.map_or_else(|| "n/a".to_string(), |value| format!("{value:+.1}%"));
    let h1 = change_1h.map_or_else(|| "n/a".to_string(), |value| format!("{value:+.1}%"));
    format!("24H {h24} / 1H {h1}")
}

fn format_usd(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("${:.2}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("${:.1}k", value / 1_000.0)
    } else {
        format!("${value:.0}")
    }
}

fn render_tab_positions(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        "Track",
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

            let (track_badge, track_color) = match pos.track {
                ExecutionTrack::FastLane => ("FAST ⚡", Color::Cyan),
                ExecutionTrack::VaultLane => ("VAULT 🏛️", Color::Yellow),
            };

            Row::new(vec![
                Cell::from(Span::styled(
                    track_badge,
                    Style::default()
                        .fg(track_color)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(pos.token_symbol.clone()),
                Cell::from(pos.chain.to_string()),
                Cell::from(mint_display),
                Cell::from(handle_display),
                Cell::from(format!("${:.6}", pos.entry_price)),
                Cell::from(format!("${:.6}", pos.current_price)),
                Cell::from(portion_str),
                Cell::from(format!(
                    "${:+.2}",
                    (pos.amount_usd * (pos.remaining_portion_pct / 100.0))
                        * (pos.unrealized_pnl_pct / 100.0)
                )),
                Cell::from(Span::styled(
                    format!("{:+.2}%", pos.unrealized_pnl_pct),
                    Style::default().fg(pnl_color).add_modifier(Modifier::BOLD),
                )),
                Cell::from(pos.status.clone()),
            ])
            .style(Style::default().fg(pnl_color))
        })
        .collect();

    let total_pos_usd: f64 = app
        .positions
        .iter()
        .map(|p| {
            (p.amount_usd * (p.remaining_portion_pct / 100.0))
                * (1.0 + p.unrealized_pnl_pct / 100.0)
        })
        .sum();

    let equity_display = if app.heartbeat_is_fresh() && app.wallet_equity_usd.is_finite() {
        format!("${:.2}", app.wallet_equity_usd)
    } else {
        "N/A".to_string()
    };
    let sol_display = if app.heartbeat_is_fresh() && app.sol_price_usd > 0.0 {
        format!("${:.2}", app.wallet_sol_balance * app.sol_price_usd)
    } else {
        "N/A".to_string()
    };
    let price_display = if app.heartbeat_is_fresh() && app.sol_price_usd > 0.0 {
        format!("${:.1}", app.sol_price_usd)
    } else {
        "N/A".to_string()
    };
    let realized_display = if app.heartbeat_is_fresh() {
        format!("${:+.2}", app.total_realized_pnl_usd)
    } else {
        "N/A".to_string()
    };

    let pos_title = format!(
        " ACTIVE SHADOW POSITIONS • Engine Equity: {} (SOL: {:.4} @ {} = {} | Shadow Positions: ${:.2} | Engine Realized: {}) ",
        equity_display,
        app.wallet_sol_balance,
        price_display,
        sol_display,
        total_pos_usd,
        realized_display
    );

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(8),
            Constraint::Percentage(9),
            Constraint::Percentage(7),
            Constraint::Percentage(16),
            Constraint::Percentage(13),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(7),
            Constraint::Percentage(10),
            Constraint::Percentage(9),
            Constraint::Percentage(8),
        ],
    )
    .header(header)
    .block(
        Block::default().borders(Borders::ALL).title(Span::styled(
            pos_title,
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
        .constraints([Constraint::Length(14), Constraint::Min(0)])
        .split(area);

    let total_pos_usd: f64 = app
        .positions
        .iter()
        .map(|p| {
            (p.amount_usd * (p.remaining_portion_pct / 100.0))
                * (1.0 + p.unrealized_pnl_pct / 100.0)
        })
        .sum();

    let fast_trades: Vec<&crate::models::TradeRecord> = app
        .trades
        .iter()
        .filter(|t| t.track == ExecutionTrack::FastLane)
        .collect();
    let vault_trades: Vec<&crate::models::TradeRecord> = app
        .trades
        .iter()
        .filter(|t| t.track == ExecutionTrack::VaultLane)
        .collect();

    let fast_pnl: f64 = fast_trades.iter().map(|t| t.pnl_usd).sum();
    let fast_wins = fast_trades.iter().filter(|t| t.pnl_usd > 0.0).count();
    let fast_wr = if !fast_trades.is_empty() {
        (fast_wins as f64 / fast_trades.len() as f64) * 100.0
    } else {
        0.0
    };

    let vault_pnl: f64 = vault_trades.iter().map(|t| t.pnl_usd).sum();
    let vault_wins = vault_trades.iter().filter(|t| t.pnl_usd > 0.0).count();
    let vault_wr = if !vault_trades.is_empty() {
        (vault_wins as f64 / vault_trades.len() as f64) * 100.0
    } else {
        0.0
    };

    let engine_equity_display = if app.heartbeat_is_fresh() && app.wallet_equity_usd.is_finite() {
        format!("${:.2} USD", app.wallet_equity_usd)
    } else {
        "N/A".to_string()
    };
    let sol_cash_display = if app.heartbeat_is_fresh() && app.sol_price_usd > 0.0 {
        format!(
            "{:.6} SOL (${:.2} USD)",
            app.wallet_sol_balance,
            app.wallet_sol_balance * app.sol_price_usd
        )
    } else {
        format!("{:.6} SOL (N/A)", app.wallet_sol_balance)
    };
    let realized_pnl_display = if app.heartbeat_is_fresh() {
        format!("${:+.2} USD", app.total_realized_pnl_usd)
    } else {
        "N/A".to_string()
    };

    let mode_desc = if app.is_live_mode {
        format!(
            "LIVE ON-CHAIN 🟢 (Signer: {})",
            if app.wallet_pubkey.is_empty() {
                "bLiHABX6WnneYX94mqR7eUh9VsMvZB23772eWAtvvdk"
            } else {
                &app.wallet_pubkey
            }
        )
    } else {
        "DRY RUN SIMULATION 🟡".to_string()
    };

    let info_text = vec![
        Line::from(vec![
            Span::styled(
                "Multi-Chain Stream Health & Real-Time Portfolio:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Mode: "),
            Span::styled(
                mode_desc,
                Style::default()
                    .fg(if app.is_live_mode {
                        Color::Green
                    } else {
                        Color::Yellow
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw(" • Real On-Chain Portfolio: SOL Cash = "),
            Span::styled(
                sol_cash_display.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Open Positions = "),
            Span::styled(
                format!("${:.2} USD", total_pos_usd),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Total Real Equity = "),
            Span::styled(
                engine_equity_display.clone(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Realized PnL = "),
            Span::styled(
                realized_pnl_display.clone(),
                Style::default()
                    .fg(if app.total_realized_pnl_usd >= 0.0 {
                        Color::Green
                    } else {
                        Color::Red
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw(" • Dual-Track Analytics: "),
            Span::styled(
                "⚡ Fast-Lane: ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("${:+.2} (WR: {:.1}%, {} closed) ", fast_pnl, fast_wr, fast_trades.len()),
                Style::default()
                    .fg(if fast_pnl >= 0.0 { Color::Green } else { Color::Red })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("| "),
            Span::styled(
                "🏛️ Vault-Lane: ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("${:+.2} (WR: {:.1}%, {} closed)", vault_pnl, vault_wr, vault_trades.len()),
                Style::default()
                    .fg(if vault_pnl >= 0.0 { Color::Green } else { Color::Red })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw(" • Network Latencies: Solana RPC = "),
            Span::styled(
                if app.solana_latency_ms == 0 {
                    "N/A".to_string()
                } else {
                    format!("{} ms", app.solana_latency_ms)
                },
                Style::default()
                    .fg(if app.solana_latency_ms == 0 { Color::Yellow } else { Color::Green })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" | Solana RPC {}/{} fail={} 429={} | Robinhood RPC {}/{} fail={} 429={} | Scope qualified={} polled={} global={} candidates={}", app.solana_rpc_successes, app.solana_rpc_requests, app.solana_rpc_failures, app.solana_rpc_rate_limited, app.robinhood_rpc_successes, app.robinhood_rpc_requests, app.robinhood_rpc_failures, app.robinhood_rpc_rate_limited, app.qualified_wallets_count, app.polled_wallets_count, app.global_scanned_wallets_count, app.candidate_wallets_count))
        ]),
        Line::from(vec![
            Span::raw(" • Execution Rules: Position Size = "),
            Span::styled(
                format!("${:.2} USD", app.position_size_usd),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Stop Loss = "),
            Span::styled(
                format!("-{:.1}% (Whale Mirror Priority)", app.stop_loss_pct),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Auto-TP = "),
            Span::styled(
                format!("+{:.1}% (50% Partial Free-Roll)", app.take_profit_pct),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw(" • Active Calibrated Gating: Max Price Delta = "),
            Span::styled(
                format!("+{:.1}%", app.max_price_delta_pct),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Min Pool Liquidity = "),
            Span::styled(
                format!("${:.0}", app.min_liquidity_usd),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | Min Whale Order = "),
            Span::styled(
                format!("${:.0}", app.min_whale_buy_usd),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | MCap Range = $50k - $20M"),
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
        "Track",
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

            let (track_badge, track_color) = match tr.track {
                ExecutionTrack::FastLane => ("FAST ⚡", Color::Cyan),
                ExecutionTrack::VaultLane => ("VAULT 🏛️", Color::Yellow),
            };

            Row::new(vec![
                Cell::from(ts_str),
                Cell::from(Span::styled(
                    track_badge,
                    Style::default()
                        .fg(track_color)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(tr.chain.to_string()),
                Cell::from(tr.action.clone()),
                Cell::from(tr.symbol.clone()),
                Cell::from(mint_display),
                Cell::from(format!("@{}", tr.target_whale)),
                Cell::from(format!("${:.6}", tr.entry_price)),
                Cell::from(format!("${:.6}", tr.exit_price)),
                Cell::from(Span::styled(
                    format!("{:+.2}%", tr.pnl_pct),
                    Style::default().fg(pnl_color).add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    format!("${:+.2}", tr.pnl_usd),
                    Style::default().fg(pnl_color).add_modifier(Modifier::BOLD),
                )),
                Cell::from(tr.exit_reason.clone()),
            ])
            .style(Style::default().fg(pnl_color))
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(9),
            Constraint::Percentage(7),
            Constraint::Percentage(7),
            Constraint::Percentage(8),
            Constraint::Percentage(8),
            Constraint::Percentage(15),
            Constraint::Percentage(12),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
            Constraint::Percentage(8),
            Constraint::Percentage(8),
            Constraint::Percentage(13),
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
