use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::ipc::IpcClient;
use zenithsw::models::Chain;
use zenithsw::tui::{render_ui, App};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;

    let app = Arc::new(Mutex::new(App::new()));
    if let Ok(mut state) = app.lock() {
        state.min_liquidity_usd = config.min_liquidity_usd;
        state.max_price_delta_pct = config.max_price_delta_pct;
        state.position_size_usd = config.position_size_usd;
        state.stop_loss_pct = config.stop_loss_pct;
        state.take_profit_pct = config.take_profit_pct;
        state.min_whale_buy_usd = config.min_whale_buy_usd;
        state.is_live_mode = !config.dry_run;
    }

    // Helper to filter data by active chains
    let enable_sol = config.enable_solana;
    let enable_rh = config.enable_robinhood;
    let enable_base = config.enable_base;

    // Populate initial state from the authoritative local SQLite database.
    // A failed open is fatal; rendering a partial/default state would break
    // Engine/TUI synchronization.
    let db = SmartWalletDb::new_with_wallet_path(&config.trading_db_path, &config.wallet_db_path)?;
    if let Ok(mut state) = app.lock() {
        match db.get_qualified_wallets() {
            Ok(mut wallets) => {
                wallets.retain(|w| match w.chain {
                    Chain::Solana => enable_sol,
                    Chain::Robinhood => enable_rh,
                    Chain::Base => enable_base,
                    _ => false,
                });
                state.wallets = wallets;
            }
            Err(error) => state.add_log(format!("TUI DB wallets stale: {error}")),
        }
        match db.get_tracked_tokens(50) {
            Ok(mut tokens) => {
                tokens.retain(|t| match t.chain {
                    Chain::Solana => enable_sol,
                    Chain::Robinhood => enable_rh,
                    Chain::Base => enable_base,
                    _ => false,
                });
                state.tracked_tokens = tokens;
            }
            Err(error) => state.add_log(format!("TUI DB tracked_tokens stale: {error}")),
        }
        match db.get_active_positions() {
            Ok(mut positions) => {
                positions.retain(|p| match p.chain {
                    Chain::Solana => enable_sol,
                    Chain::Robinhood => enable_rh,
                    Chain::Base => enable_base,
                    _ => false,
                });
                state.positions = positions;
            }
            Err(error) => state.add_log(format!("TUI DB positions stale: {error}")),
        }
        match db.get_recent_trades(50) {
            Ok(mut trades) => {
                trades.retain(|t| match t.chain {
                    Chain::Solana => enable_sol,
                    Chain::Robinhood => enable_rh,
                    Chain::Base => enable_base,
                    _ => false,
                });
                state.trades = trades;
            }
            Err(error) => state.add_log(format!("TUI DB trades stale: {error}")),
        }
    }

    // Task 1: Real-Time Periodic Local Database Sync (Ensures live PnL, fresh wallets, activities & trades)
    let app_db_sync = app.clone();
    let trading_db_path_sync = config.trading_db_path.clone();
    let wallet_db_path_sync = config.wallet_db_path.clone();
    tokio::spawn(async move {
        if let Ok(db) =
            SmartWalletDb::new_with_wallet_path(&trading_db_path_sync, &wallet_db_path_sync)
        {
            loop {
                if let Ok(mut state) = app_db_sync.lock() {
                    if let Ok(mut wallets) = db.get_qualified_wallets() {
                        wallets.retain(|w| match w.chain {
                            Chain::Solana => enable_sol,
                            Chain::Robinhood => enable_rh,
                            Chain::Base => enable_base,
                            _ => false,
                        });
                        state.wallets = wallets;
                    }
                    match db.get_tracked_tokens(50) {
                        Ok(mut tokens) => {
                            tokens.retain(|t| match t.chain {
                                Chain::Solana => enable_sol,
                                Chain::Robinhood => enable_rh,
                                Chain::Base => enable_base,
                                _ => false,
                            });
                            state.tracked_tokens = tokens;
                        }
                        Err(error) => {
                            state.add_log(format!("TUI DB tracked_tokens stale: {error}"))
                        }
                    }
                    match db.get_active_positions() {
                        Ok(mut positions) => {
                            positions.retain(|p| match p.chain {
                                Chain::Solana => enable_sol,
                                Chain::Robinhood => enable_rh,
                                Chain::Base => enable_base,
                                _ => false,
                            });
                            state.positions = positions;
                        }
                        Err(error) => state.add_log(format!("TUI DB positions stale: {error}")),
                    }
                    match db.get_recent_trades(50) {
                        Ok(mut trades) => {
                            trades.retain(|t| match t.chain {
                                Chain::Solana => enable_sol,
                                Chain::Robinhood => enable_rh,
                                Chain::Base => enable_base,
                                _ => false,
                            });
                            state.trades = trades;
                        }
                        Err(error) => state.add_log(format!("TUI DB trades stale: {error}")),
                    }
                    match db.get_recent_activities(50) {
                        Ok(mut acts) => {
                            acts.retain(|a| match a.chain {
                                Chain::Solana => enable_sol,
                                Chain::Robinhood => enable_rh,
                                Chain::Base => enable_base,
                                _ => false,
                            });
                            state.activities = acts;
                        }
                        Err(error) => state.add_log(format!("TUI DB activities stale: {error}")),
                    }
                    match db.get_recent_signals(50) {
                        Ok(mut sigs) => {
                            sigs.retain(|s| match s.chain {
                                Chain::Solana => enable_sol,
                                Chain::Robinhood => enable_rh,
                                Chain::Base => enable_base,
                                _ => false,
                            });
                            state.signals = sigs;
                        }
                        Err(error) => state.add_log(format!("TUI DB signals stale: {error}")),
                    }
                }
                sleep(Duration::from_millis(1500)).await;
            }
        }
    });

    // Task 2: Connect to Unix Domain Socket IPC Daemon
    let app_ipc = app.clone();
    let socket_path = config.ipc_socket_path.clone();
    let config_ipc = std::sync::Arc::new(config.clone());
    tokio::spawn(async move {
        let client = IpcClient::new(&socket_path);
        loop {
            let app_cb = app_ipc.clone();
            let app_conn = app_ipc.clone();
            let config_cb = std::sync::Arc::clone(&config_ipc);
            let res = client
                .connect_and_listen(
                    move || {
                        if let Ok(mut state) = app_conn.lock() {
                            // Socket connected belum membuktikan heartbeat engine valid.
                            state.is_connected_ipc = false;
                        }
                    },
                    move |msg| {
                        if let Ok(mut state) = app_cb.lock() {
                            zenithsw::tui::apply_ipc_message(&mut state, msg, &config_cb);
                        }
                    },
                )
                .await;

            if res.is_err() {
                if let Ok(mut state) = app_ipc.lock() {
                    state.is_connected_ipc = false;
                }
                sleep(Duration::from_millis(100)).await;
            }
        }
    });

    // Initialize Terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        // Draw TUI frame
        if let Ok(state) = app.lock() {
            terminal.draw(|f| render_ui(f, &state))?;
            if state.should_quit {
                break;
            }
        }

        // Handle user input
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if let Ok(mut state) = app.lock() {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                            state.should_quit = true;
                        }
                        KeyCode::Char('1') => state.current_tab = 0,
                        KeyCode::Char('2') => state.current_tab = 1,
                        KeyCode::Char('3') => state.current_tab = 2,
                        KeyCode::Char('4') => state.current_tab = 3,
                        KeyCode::Char('5') => state.current_tab = 4,
                        KeyCode::Tab => state.next_tab(),
                        KeyCode::BackTab => state.prev_tab(),
                        _ => {}
                    }
                }
            }
        }
    }

    // Restore terminal cleanly
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
