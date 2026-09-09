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
use zenithsw::ipc::{IpcClient, IpcMessage};
use zenithsw::tui::{render_ui, App};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load().unwrap_or_else(|_| Config {
        solana_rpc_url: "".to_string(),
        solana_ws_url: "".to_string(),
        solana_private_key: None,
        arbitrum_rpc_url: "".to_string(),
        arbitrum_ws_url: "".to_string(),
        base_rpc_url: "".to_string(),
        base_ws_url: "".to_string(),
        evm_private_key: None,
        fomo_auth_token: None,
        fomo_sync_interval_sec: 60,
        stream_poll_interval_ms: 500,
        max_price_delta_pct: 5.0,
        min_liquidity_usd: 30000.0,
        position_size_usd: 15.0,
        trailing_stop_pct: 10.0,
        stop_loss_pct: 15.0,
        take_profit_pct: 50.0,
        shadow_exit_enabled: true,
        confluence_mode: "TIER_S_OR_DUAL_A".to_string(),
        dry_run: true,
        db_path: "data/smart_wallets.db".to_string(),
        ipc_socket_path: "/tmp/zenithsw.sock".to_string(),
    });

    let app = Arc::new(Mutex::new(App::new()));

    // Task: Connect to Unix Domain Socket IPC Daemon
    let app_ipc = app.clone();
    let socket_path = config.ipc_socket_path.clone();
    tokio::spawn(async move {
        let client = IpcClient::new(&socket_path);
        loop {
            let app_cb = app_ipc.clone();
            let app_conn = app_ipc.clone();
            let res = client
                .connect_and_listen(
                    move || {
                        if let Ok(mut state) = app_conn.lock() {
                            state.is_connected_ipc = true;
                        }
                    },
                    move |msg| {
                        if let Ok(mut state) = app_cb.lock() {
                            state.is_connected_ipc = true;
                            match msg {
                                IpcMessage::Heartbeat {
                                    solana_latency_ms,
                                    arbitrum_latency_ms,
                                    base_latency_ms,
                                    ..
                                } => {
                                    state.solana_latency_ms = solana_latency_ms;
                                    state.arbitrum_latency_ms = arbitrum_latency_ms;
                                    state.base_latency_ms = base_latency_ms;
                                }
                                IpcMessage::WalletSync { wallets } => {
                                    state.wallets = wallets;
                                }
                                IpcMessage::ActivityBroadcast { activity } => {
                                    state.activities.push(activity);
                                    if state.activities.len() > 100 {
                                        state.activities.remove(0);
                                    }
                                }
                                IpcMessage::SignalBroadcast { signal } => {
                                    state.signals.push(signal);
                                    if state.signals.len() > 100 {
                                        state.signals.remove(0);
                                    }
                                }
                                IpcMessage::PositionUpdate { positions } => {
                                    state.positions = positions;
                                }
                                IpcMessage::EngineLog { message, .. } => {
                                    state.add_log(message);
                                }
                            }
                        }
                    },
                )
                .await;

            if res.is_err() {
                if let Ok(mut state) = app_ipc.lock() {
                    state.is_connected_ipc = false;
                }
                sleep(Duration::from_millis(500)).await;
            }
        }
    });

    // Populate initial seed wallets if empty
    if let Ok(mut state) = app.lock() {
        state.wallets = zenithsw::ingestion::FomoScraper::get_default_seed_traders();
    }

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
