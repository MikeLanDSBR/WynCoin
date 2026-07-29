use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use tauri::State;
use wyncoin_core::{
    format_wyn, parse_wyn, send_request, AddressActivity, EncryptedWalletFile, NodeStatus, Request,
    Utxo, Wallet, WalletMetadata,
};

const DEFAULT_NODE: &str = "127.0.0.1:9332";
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

struct WalletSession {
    wallet: Wallet,
    last_activity: Instant,
}

struct AppState {
    wallet_path: PathBuf,
    settings_path: PathBuf,
    node_address: Mutex<String>,
    session: Mutex<Option<WalletSession>>,
}

#[derive(Serialize, Deserialize)]
struct WalletAppSettings {
    node_address: String,
}

#[derive(Serialize)]
struct WalletState {
    exists: bool,
    unlocked: bool,
    metadata: Option<WalletMetadata>,
    path: String,
}

#[derive(Serialize)]
struct WalletOverview {
    address: String,
    available_atomic: String,
    available_wyn: String,
    utxo_count: usize,
    node: NodeStatus,
}

#[derive(Serialize)]
struct SendResult {
    txid: String,
    amount_wyn: String,
    fee_wyn: String,
}

#[derive(Serialize)]
struct WalletHistoryItem {
    transaction_id: String,
    timestamp: i64,
    block_height: Option<u64>,
    confirmations: u64,
    is_coinbase: bool,
    is_receive: bool,
    amount_wyn: String,
    fee_wyn: String,
}

pub fn run() {
    let wallet_path = default_wallet_path();
    let settings_path = wallet_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("wallet-app.json");
    let initial_node_address = load_node_address(&settings_path);
    tauri::Builder::default()
        .manage(AppState {
            wallet_path,
            settings_path,
            node_address: Mutex::new(initial_node_address),
            session: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            wallet_state,
            create_wallet,
            import_legacy_wallet,
            unlock_wallet,
            lock_wallet,
            set_node_address,
            node_address,
            wallet_overview,
            wallet_history,
            receive_qr_svg,
            send_transaction,
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar a WynCoin Wallet");
}

#[tauri::command]
fn wallet_state(state: State<'_, AppState>) -> Result<WalletState, String> {
    let metadata = if state.wallet_path.exists() {
        Some(
            EncryptedWalletFile::load_from_file(&state.wallet_path)
                .map_err(display_error)?
                .metadata()
                .map_err(display_error)?,
        )
    } else {
        None
    };
    let unlocked = active_session(&state)?.is_some();
    Ok(WalletState {
        exists: metadata.is_some(),
        unlocked,
        metadata,
        path: state.wallet_path.display().to_string(),
    })
}

#[tauri::command]
fn create_wallet(password: String, state: State<'_, AppState>) -> Result<WalletState, String> {
    if state.wallet_path.exists() {
        return Err("já existe uma carteira protegida neste computador".into());
    }
    let (file, wallet) = EncryptedWalletFile::create(&password).map_err(display_error)?;
    file.save_to_file(&state.wallet_path)
        .map_err(display_error)?;
    replace_session(&state, wallet)?;
    wallet_state(state)
}

#[tauri::command]
fn import_legacy_wallet(
    legacy_path: String,
    password: String,
    state: State<'_, AppState>,
) -> Result<WalletState, String> {
    if state.wallet_path.exists() {
        return Err("já existe uma carteira protegida; não sobrescreva-a ao importar".into());
    }
    let legacy = Wallet::load_from_file(&legacy_path).map_err(display_error)?;
    let file = EncryptedWalletFile::from_wallet(&legacy, &password).map_err(display_error)?;
    file.save_to_file(&state.wallet_path)
        .map_err(display_error)?;
    replace_session(&state, legacy)?;
    wallet_state(state)
}

#[tauri::command]
fn unlock_wallet(password: String, state: State<'_, AppState>) -> Result<WalletState, String> {
    let file = EncryptedWalletFile::load_from_file(&state.wallet_path).map_err(display_error)?;
    let wallet = file.unlock(&password).map_err(display_error)?;
    replace_session(&state, wallet)?;
    wallet_state(state)
}

#[tauri::command]
fn lock_wallet(state: State<'_, AppState>) -> Result<(), String> {
    *state
        .session
        .lock()
        .map_err(|_| "estado da carteira indisponível")? = None;
    Ok(())
}

#[tauri::command]
fn set_node_address(address: String, state: State<'_, AppState>) -> Result<(), String> {
    if address.parse::<std::net::SocketAddr>().is_err() {
        return Err("o endereço do nó deve usar o formato host:porta".into());
    }
    if !address.starts_with("127.0.0.1:") && !address.starts_with("[::1]:") {
        return Err("por segurança, esta primeira versão aceita apenas nó local".into());
    }
    *state
        .node_address
        .lock()
        .map_err(|_| "configuração do nó indisponível")? = address.clone();
    save_node_address(&state.settings_path, &address)?;
    Ok(())
}

#[tauri::command]
fn node_address(state: State<'_, AppState>) -> Result<String, String> {
    state
        .node_address
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "configuração do nó indisponível".into())
}

#[tauri::command]
fn wallet_overview(state: State<'_, AppState>) -> Result<WalletOverview, String> {
    let wallet = unlocked_wallet(&state)?;
    let node = request::<NodeStatus>(&state, Request::Status)?;
    let available = request::<u64>(
        &state,
        Request::Balance {
            address: wallet.address.clone(),
        },
    )?;
    let utxos = request::<Vec<Utxo>>(
        &state,
        Request::Utxos {
            address: wallet.address.clone(),
        },
    )?;
    Ok(WalletOverview {
        address: wallet.address.clone(),
        available_atomic: available.to_string(),
        available_wyn: format_wyn(available),
        utxo_count: utxos.len(),
        node,
    })
}

#[tauri::command]
fn wallet_history(state: State<'_, AppState>) -> Result<Vec<WalletHistoryItem>, String> {
    let wallet = unlocked_wallet(&state)?;
    let activity = request::<Vec<AddressActivity>>(
        &state,
        Request::AddressHistory {
            address: wallet.address.clone(),
            limit: 100,
        },
    )?;
    Ok(activity
        .into_iter()
        .map(|item| {
            let is_receive = item.incoming >= item.outgoing;
            let amount = if is_receive {
                item.incoming
            } else {
                item.outgoing
            };
            WalletHistoryItem {
                transaction_id: item.transaction_id,
                timestamp: item.timestamp,
                block_height: item.block_height,
                confirmations: item.confirmations,
                is_coinbase: item.is_coinbase,
                is_receive,
                amount_wyn: format_wyn(amount),
                fee_wyn: format_wyn(item.fee),
            }
        })
        .collect())
}

#[tauri::command]
fn receive_qr_svg(state: State<'_, AppState>) -> Result<String, String> {
    let wallet = unlocked_wallet(&state)?;
    let code =
        QrCode::new(wallet.address.as_bytes()).map_err(|_| "não foi possível gerar QR Code")?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(svg::Color("#06241f"))
        .light_color(svg::Color("#f2fffb"))
        .build())
}

#[tauri::command]
fn send_transaction(
    to: String,
    amount: String,
    fee: String,
    state: State<'_, AppState>,
) -> Result<SendResult, String> {
    let wallet = unlocked_wallet(&state)?;
    let amount_atomic = parse_wyn(&amount).map_err(display_error)?;
    let fee_atomic = parse_wyn(&fee).map_err(display_error)?;
    let utxos = request::<Vec<Utxo>>(
        &state,
        Request::Utxos {
            address: wallet.address.clone(),
        },
    )?;
    let transaction = wallet
        .build_transaction(&utxos, &to, amount_atomic, fee_atomic)
        .map_err(display_error)?;
    let txid = transaction.id.clone();
    let _: serde_json::Value = request(&state, Request::SubmitTransaction { transaction })?;
    Ok(SendResult {
        txid,
        amount_wyn: format_wyn(amount_atomic),
        fee_wyn: format_wyn(fee_atomic),
    })
}

fn request<T: serde::de::DeserializeOwned>(
    state: &AppState,
    request: Request,
) -> Result<T, String> {
    let address = state
        .node_address
        .lock()
        .map_err(|_| "configuração do nó indisponível")?
        .clone();
    send_request(&address, &request)
        .map_err(display_error)?
        .require_data()
        .map_err(display_error)
}

fn unlocked_wallet(state: &AppState) -> Result<Wallet, String> {
    let mut session = state
        .session
        .lock()
        .map_err(|_| "estado da carteira indisponível")?;
    if session
        .as_ref()
        .is_some_and(|current| current.last_activity.elapsed() > SESSION_IDLE_TIMEOUT)
    {
        *session = None;
        return Err("a carteira foi bloqueada por inatividade".into());
    }
    let session = session
        .as_mut()
        .ok_or_else(|| "desbloqueie a carteira antes de continuar".to_string())?;
    session.last_activity = Instant::now();
    Ok(session.wallet.clone())
}

fn active_session(state: &AppState) -> Result<MutexGuard<'_, Option<WalletSession>>, String> {
    let mut session = state
        .session
        .lock()
        .map_err(|_| "estado da carteira indisponível")?;
    if session
        .as_ref()
        .is_some_and(|current| current.last_activity.elapsed() > SESSION_IDLE_TIMEOUT)
    {
        *session = None;
    }
    Ok(session)
}

fn replace_session(state: &AppState, wallet: Wallet) -> Result<(), String> {
    *state
        .session
        .lock()
        .map_err(|_| "estado da carteira indisponível")? = Some(WalletSession {
        wallet,
        last_activity: Instant::now(),
    });
    Ok(())
}

fn default_wallet_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".wyncoin/wallets/default.wynwallet")
}

fn load_node_address(path: &std::path::Path) -> String {
    std::fs::read(path)
        .ok()
        .and_then(|body| serde_json::from_slice::<WalletAppSettings>(&body).ok())
        .map(|settings| settings.node_address)
        .filter(|address| address.starts_with("127.0.0.1:") || address.starts_with("[::1]:"))
        .unwrap_or_else(|| DEFAULT_NODE.into())
}

fn save_node_address(path: &std::path::Path, address: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(display_error)?;
    }
    let body = serde_json::to_vec_pretty(&WalletAppSettings {
        node_address: address.into(),
    })
    .map_err(display_error)?;
    std::fs::write(path, body).map_err(display_error)
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
