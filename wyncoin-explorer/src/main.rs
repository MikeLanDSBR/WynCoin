use std::collections::{HashMap, HashSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};
use wyncoin_core::blockchain::{Block, Transaction};
use wyncoin_core::protocol::{send_request, NodeStatus, Request};
use wyncoin_core::{format_wyn, Result, WynError};

const INDEX_HTML: &str = include_str!("../static/index.html");
const STYLES_CSS: &str = include_str!("../static/styles.css");
const APP_JS: &str = include_str!("../static/app.js");
const MAX_HTTP_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "wyncoin-explorer",
    version,
    about = "Explorador HTTP local e somente leitura da blockchain WynCoin"
)]
struct Args {
    /// Endereço HTTP do explorador. Por segurança, somente loopback é permitido.
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,

    /// API TCP local do wyncoind.
    #[arg(long, default_value = "127.0.0.1:9332")]
    node: SocketAddr,

    /// Caminho opcional para blockchain.db. Sem este argumento, usa o caminho informado pelo nó.
    #[arg(long)]
    database: Option<PathBuf>,

    /// Não tenta abrir o navegador automaticamente.
    #[arg(long)]
    no_open: bool,

    /// Tempo mínimo entre recargas completas do SQLite.
    #[arg(long, default_value_t = 2)]
    cache_seconds: u64,
}

#[derive(Clone)]
struct ExplorerState {
    node_address: String,
    database_override: Option<PathBuf>,
    cache_ttl: Duration,
    cache: Arc<Mutex<Option<CachedSnapshot>>>,
}

#[derive(Clone)]
struct CachedSnapshot {
    loaded_at: Instant,
    snapshot: Snapshot,
}

#[derive(Clone)]
struct Snapshot {
    status: NodeStatus,
    blocks: Vec<Block>,
    mempool: Vec<Transaction>,
}

#[derive(Serialize)]
struct ApiEnvelope<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct ExplorerStatus {
    explorer_version: &'static str,
    explorer_url: String,
    node: NodeStatusView,
    chain: ChainStats,
}

#[derive(Serialize)]
struct NodeStatusView {
    version: String,
    network_id: String,
    height: u64,
    tip_hash: String,
    difficulty: u32,
    block_reward_atomic: String,
    block_reward_wyn: String,
    mempool_size: usize,
    mining_enabled: bool,
    miner_address: String,
    database: String,
    uptime_seconds: u64,
}

#[derive(Serialize)]
struct ChainStats {
    blocks: usize,
    transactions: usize,
    regular_transactions: usize,
    coinbase_transactions: usize,
    issued_supply_atomic: String,
    issued_supply_wyn: String,
    total_output_atomic: String,
    total_output_wyn: String,
    mempool_total_atomic: String,
    mempool_total_wyn: String,
    average_block_interval_seconds: Option<u64>,
    miners_seen: usize,
    latest_block_timestamp: Option<i64>,
}

#[derive(Serialize, Clone)]
struct BlockSummary {
    height: u64,
    hash: String,
    previous_hash: String,
    merkle_root: String,
    timestamp: i64,
    difficulty: u32,
    nonce: u64,
    transaction_count: usize,
    size_bytes: usize,
    total_output_atomic: String,
    total_output_wyn: String,
    miner_address: Option<String>,
    reward_atomic: Option<String>,
    reward_wyn: Option<String>,
}

#[derive(Serialize)]
struct BlockDetails {
    summary: BlockSummary,
    transactions: Vec<TransactionView>,
    confirmations: u64,
}

#[derive(Serialize, Clone)]
struct TransactionView {
    id: String,
    timestamp: i64,
    is_coinbase: bool,
    coinbase_data: Option<String>,
    input_count: usize,
    output_count: usize,
    total_output_atomic: String,
    total_output_wyn: String,
    block_height: Option<u64>,
    confirmations: u64,
    inputs: Vec<InputView>,
    outputs: Vec<OutputView>,
}

#[derive(Serialize, Clone)]
struct InputView {
    transaction_id: String,
    output_index: usize,
    signature: String,
    public_key: String,
}

#[derive(Serialize, Clone)]
struct OutputView {
    index: usize,
    recipient: String,
    amount_atomic: String,
    amount_wyn: String,
}

#[derive(Serialize)]
struct BlocksPage {
    items: Vec<BlockSummary>,
    newest_height: u64,
    next_before: Option<u64>,
    total_blocks: usize,
}

#[derive(Serialize)]
struct TransactionLocation {
    transaction: TransactionView,
    block: Option<BlockSummary>,
    in_mempool: bool,
}

#[derive(Serialize)]
struct AddressView {
    address: String,
    confirmed_balance_atomic: String,
    confirmed_balance_wyn: String,
    received_atomic: String,
    received_wyn: String,
    sent_atomic: String,
    sent_wyn: String,
    utxos: Vec<UtxoView>,
    activity: Vec<AddressActivity>,
}

#[derive(Serialize)]
struct UtxoView {
    transaction_id: String,
    output_index: usize,
    block_height: u64,
    confirmations: u64,
    timestamp: i64,
    amount_atomic: String,
    amount_wyn: String,
}

#[derive(Serialize)]
struct TransactionsPage {
    items: Vec<TransactionLocation>,
    next_before_height: Option<u64>,
    total_transactions: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransactionFilter {
    All,
    Regular,
    Coinbase,
}

impl TransactionFilter {
    fn from_query(value: Option<&String>) -> Result<Self> {
        match value.map(String::as_str).unwrap_or("all") {
            "all" => Ok(Self::All),
            "regular" => Ok(Self::Regular),
            "coinbase" => Ok(Self::Coinbase),
            _ => Err(WynError::Validation(
                "type deve ser all, regular ou coinbase".into(),
            )),
        }
    }

    fn includes(self, transaction: &Transaction) -> bool {
        matches!(self, Self::All)
            || matches!(self, Self::Regular) && !transaction.is_coinbase
            || matches!(self, Self::Coinbase) && transaction.is_coinbase
    }
}

#[derive(Serialize)]
struct AddressActivity {
    transaction_id: String,
    timestamp: i64,
    block_height: Option<u64>,
    confirmations: u64,
    is_coinbase: bool,
    incoming_atomic: String,
    incoming_wyn: String,
    outgoing_atomic: String,
    outgoing_wyn: String,
    net_atomic: String,
    net_wyn: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("[wyncoin-explorer] erro fatal: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    validate_loopback(args.listen, "--listen")?;
    validate_loopback(args.node, "--node")?;

    let listener = TcpListener::bind(args.listen)?;
    listener.set_nonblocking(true)?;

    let state = ExplorerState {
        node_address: args.node.to_string(),
        database_override: args.database,
        cache_ttl: Duration::from_secs(args.cache_seconds.max(1)),
        cache: Arc::new(Mutex::new(None)),
    };

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))
            .map_err(|error| WynError::Config(format!("falha ao instalar Ctrl+C: {error}")))?;
    }

    let url = format!("http://{}", args.listen);
    println!("WynCoin Explorer v{}", env!("CARGO_PKG_VERSION"));
    println!("  site       : {url}");
    println!("  wyncoind   : {}", state.node_address);
    println!("  acesso     : somente localhost");
    println!("Use Ctrl+C para encerrar.");

    if !args.no_open {
        open_browser_later(url.clone());
    }

    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let state = state.clone();
                let explorer_url = url.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_http_connection(stream, &state, &explorer_url) {
                        eprintln!("[http] {error}");
                    }
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }

    println!("[wyncoin-explorer] encerrado.");
    Ok(())
}

fn validate_loopback(address: SocketAddr, flag: &str) -> Result<()> {
    if !address.ip().is_loopback() {
        return Err(WynError::Config(format!(
            "{flag} deve usar 127.0.0.1 ou ::1 nesta versão"
        )));
    }
    Ok(())
}

fn open_browser_later(url: String) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(350));
        let result = if cfg!(target_os = "windows") {
            Command::new("cmd").args(["/C", "start", "", &url]).spawn()
        } else if cfg!(target_os = "macos") {
            Command::new("open").arg(&url).spawn()
        } else {
            Command::new("xdg-open").arg(&url).spawn()
        };

        if let Err(error) = result {
            eprintln!("[browser] não foi possível abrir automaticamente: {error}");
        }
    });
}

fn handle_http_connection(
    mut stream: TcpStream,
    state: &ExplorerState,
    explorer_url: &str,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    let request = read_http_request(&mut stream)?;
    let head_only = request.method == "HEAD";

    if request.method != "GET" && !head_only {
        return send_json_error(&mut stream, 405, "método HTTP não permitido", head_only);
    }

    let (path, query) = split_target(&request.target);
    match path.as_str() {
        "/" | "/index.html" => send_static(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
            head_only,
        ),
        "/assets/styles.css" => send_static(
            &mut stream,
            200,
            "text/css; charset=utf-8",
            STYLES_CSS.as_bytes(),
            head_only,
        ),
        "/assets/app.js" => send_static(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            APP_JS.as_bytes(),
            head_only,
        ),
        "/favicon.ico" => send_response(
            &mut stream,
            204,
            "image/x-icon",
            &[],
            CachePolicy::Static,
            head_only,
        ),
        "/api/health" => api_health(state, explorer_url, &mut stream, head_only),
        "/api/status" => api_status(state, explorer_url, &mut stream, head_only),
        "/api/blocks" => api_blocks(state, &query, &mut stream, head_only),
        "/api/transactions" => api_transactions(state, &query, &mut stream, head_only),
        "/api/mempool" => api_mempool(state, &mut stream, head_only),
        "/api/search" => api_search(state, &query, &mut stream, head_only),
        _ if path.starts_with("/api/block/") => {
            let value = path.trim_start_matches("/api/block/");
            api_block(state, value, &mut stream, head_only)
        }
        _ if path.starts_with("/api/transaction/") => {
            let value = percent_decode(path.trim_start_matches("/api/transaction/"))?;
            api_transaction(state, &value, &mut stream, head_only)
        }
        _ if path.starts_with("/api/address/") => {
            let value = percent_decode(path.trim_start_matches("/api/address/"))?;
            api_address(state, &value, &query, &mut stream, head_only)
        }
        _ if path.starts_with("/api/") => {
            send_json_error(&mut stream, 404, "endpoint não encontrado", head_only)
        }
        _ => send_static(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes(),
            head_only,
        ),
    }
}

struct HttpRequest {
    method: String,
    target: String,
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];

    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HTTP_REQUEST_BYTES {
            return Err(WynError::Protocol("requisição HTTP excede 16 KiB".into()));
        }
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let request = std::str::from_utf8(&buffer)
        .map_err(|_| WynError::Protocol("cabeçalho HTTP não é UTF-8 válido".into()))?;
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| WynError::Protocol("requisição HTTP vazia".into()))?;
    let mut parts = first_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| WynError::Protocol("método HTTP ausente".into()))?;
    let target = parts
        .next()
        .ok_or_else(|| WynError::Protocol("alvo HTTP ausente".into()))?;

    Ok(HttpRequest {
        method: method.to_string(),
        target: target.to_string(),
    })
}

fn split_target(target: &str) -> (String, HashMap<String, String>) {
    let (path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let mut query = HashMap::new();
    for pair in raw_query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if let (Ok(key), Ok(value)) = (percent_decode(key), percent_decode(value)) {
            query.insert(key, value);
        }
    }
    (path.to_string(), query)
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = decode_hex(bytes[index + 1])?;
                let low = decode_hex(bytes[index + 2])?;
                output.push((high << 4) | low);
                index += 3;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(output)
        .map_err(|_| WynError::Protocol("parâmetro URL não é UTF-8 válido".into()))
}

fn decode_hex(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(WynError::Protocol("escape percentual inválido".into())),
    }
}

impl ExplorerState {
    fn snapshot(&self) -> Result<Snapshot> {
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| WynError::Protocol("cache do explorador foi envenenado".into()))?;
            if let Some(cached) = cache.as_ref() {
                if cached.loaded_at.elapsed() < self.cache_ttl {
                    return Ok(cached.snapshot.clone());
                }
            }
        }

        let response = send_request(&self.node_address, &Request::Status)?;
        let status: NodeStatus = response.require_data()?;
        let database = self
            .database_override
            .clone()
            .unwrap_or_else(|| PathBuf::from(&status.database));
        let snapshot = Snapshot {
            status,
            blocks: load_blocks_read_only(&database)?,
            mempool: load_mempool_read_only(&database)?,
        };

        let mut cache = self
            .cache
            .lock()
            .map_err(|_| WynError::Protocol("cache do explorador foi envenenado".into()))?;
        *cache = Some(CachedSnapshot {
            loaded_at: Instant::now(),
            snapshot: snapshot.clone(),
        });
        Ok(snapshot)
    }
}


fn open_database_read_only(path: &std::path::Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

fn load_blocks_read_only(path: &std::path::Path) -> Result<Vec<Block>> {
    let connection = open_database_read_only(path)?;
    let mut statement = connection.prepare("SELECT data FROM blocks ORDER BY height ASC")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut blocks = Vec::new();
    for row in rows {
        blocks.push(serde_json::from_str(&row?)?);
    }
    Ok(blocks)
}

fn load_mempool_read_only(path: &std::path::Path) -> Result<Vec<Transaction>> {
    let connection = open_database_read_only(path)?;
    let mut statement = connection.prepare(
        "SELECT data FROM mempool ORDER BY received_at ASC, txid ASC",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut transactions = Vec::new();
    for row in rows {
        transactions.push(serde_json::from_str(&row?)?);
    }
    Ok(transactions)
}

fn api_health(
    state: &ExplorerState,
    explorer_url: &str,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    match state.snapshot() {
        Ok(snapshot) => send_json(
            stream,
            200,
            &ApiEnvelope {
                ok: true,
                data: Some(json!({
                    "explorer": "online",
                    "node": "online",
                    "url": explorer_url,
                    "height": snapshot.status.height
                })),
                error: None,
            },
            head_only,
        ),
        Err(error) => send_json(
            stream,
            503,
            &ApiEnvelope::<Value> {
                ok: false,
                data: None,
                error: Some(error.to_string()),
            },
            head_only,
        ),
    }
}

fn api_status(
    state: &ExplorerState,
    explorer_url: &str,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        let snapshot = state.snapshot()?;
        let transactions = snapshot
            .blocks
            .iter()
            .map(|block| block.transactions.len())
            .sum::<usize>();
        let coinbase_transactions = snapshot
            .blocks
            .iter()
            .flat_map(|block| &block.transactions)
            .filter(|transaction| transaction.is_coinbase)
            .count();
        let total_output = snapshot
            .blocks
            .iter()
            .flat_map(|block| &block.transactions)
            .flat_map(|transaction| &transaction.outputs)
            .try_fold(0u64, |total, output| total.checked_add(output.amount))
            .ok_or_else(|| WynError::Validation("overflow nas estatísticas da chain".into()))?;
        let issued_supply = snapshot
            .blocks
            .iter()
            .flat_map(|block| &block.transactions)
            .filter(|transaction| transaction.is_coinbase)
            .flat_map(|transaction| &transaction.outputs)
            .try_fold(0u64, |total, output| total.checked_add(output.amount))
            .ok_or_else(|| WynError::Validation("overflow na oferta emitida".into()))?;
        let mempool_total = snapshot
            .mempool
            .iter()
            .try_fold(0u64, |total, transaction| {
                total
                    .checked_add(transaction.checked_total_output()?)
                    .ok_or_else(|| WynError::Validation("overflow no mempool".into()))
            })?;
        let mut miners = HashSet::new();
        for block in &snapshot.blocks {
            if let Some(output) = block
                .transactions
                .first()
                .filter(|transaction| transaction.is_coinbase)
                .and_then(|transaction| transaction.outputs.first())
            {
                miners.insert(output.recipient.as_str());
            }
        }
        let recent_blocks = snapshot.blocks.iter().rev().take(145).collect::<Vec<_>>();
        let average_block_interval_seconds = recent_blocks
            .first()
            .zip(recent_blocks.last())
            .and_then(|(newest, oldest)| {
                let intervals = recent_blocks.len().checked_sub(1)? as i64;
                let elapsed = newest.header.timestamp.checked_sub(oldest.header.timestamp)?;
                // O cabeçalho guarda milissegundos desde Unix epoch; a API
                // expõe o ritmo em segundos para a interface.
                (intervals > 0 && elapsed >= 0).then(|| (elapsed / intervals / 1_000) as u64)
            });

        Ok(ExplorerStatus {
            explorer_version: env!("CARGO_PKG_VERSION"),
            explorer_url: explorer_url.to_string(),
            node: node_status_view(&snapshot.status),
            chain: ChainStats {
                blocks: snapshot.blocks.len(),
                transactions,
                regular_transactions: transactions.saturating_sub(coinbase_transactions),
                coinbase_transactions,
                issued_supply_atomic: issued_supply.to_string(),
                issued_supply_wyn: format_wyn(issued_supply),
                total_output_atomic: total_output.to_string(),
                total_output_wyn: format_wyn(total_output),
                mempool_total_atomic: mempool_total.to_string(),
                mempool_total_wyn: format_wyn(mempool_total),
                average_block_interval_seconds,
                miners_seen: miners.len(),
                latest_block_timestamp: snapshot.blocks.last().map(|block| block.header.timestamp),
            },
        })
    })
}

fn api_blocks(
    state: &ExplorerState,
    query: &HashMap<String, String>,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        let snapshot = state.snapshot()?;
        let limit = query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(20)
            .clamp(1, 100);
        let newest_height = snapshot.blocks.last().map(|block| block.index).unwrap_or(0);
        let before = query
            .get("before")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(newest_height)
            .min(newest_height);

        let mut items = snapshot
            .blocks
            .iter()
            .rev()
            .filter(|block| block.index <= before)
            .take(limit)
            .map(block_summary)
            .collect::<Result<Vec<_>>>()?;
        items.sort_by(|left, right| right.height.cmp(&left.height));
        let next_before = items
            .last()
            .and_then(|block| block.height.checked_sub(1))
            .filter(|height| snapshot.blocks.iter().any(|block| block.index <= *height));

        Ok(BlocksPage {
            items,
            newest_height,
            next_before,
            total_blocks: snapshot.blocks.len(),
        })
    })
}

fn api_block(
    state: &ExplorerState,
    value: &str,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        let snapshot = state.snapshot()?;
        let block = value
            .parse::<u64>()
            .ok()
            .and_then(|height| snapshot.blocks.iter().find(|block| block.index == height))
            .or_else(|| snapshot.blocks.iter().find(|block| block.hash == value))
            .ok_or_else(|| WynError::Validation("bloco não encontrado".into()))?;
        Ok(block_details(block, snapshot.status.height)?)
    })
}

fn api_transactions(
    state: &ExplorerState,
    query: &HashMap<String, String>,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        let snapshot = state.snapshot()?;
        let limit = query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(25)
            .clamp(1, 100);
        let before_height = query
            .get("before_height")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(snapshot.status.height);
        let filter = TransactionFilter::from_query(query.get("type"))?;
        let mut items = Vec::with_capacity(limit);
        for block in snapshot.blocks.iter().rev().filter(|block| block.index <= before_height) {
            for transaction in block.transactions.iter().rev().filter(|transaction| filter.includes(transaction)) {
                items.push(TransactionLocation {
                    transaction: transaction_view(transaction, Some(block.index), snapshot.status.height)?,
                    block: Some(block_summary(block)?),
                    in_mempool: false,
                });
            }
            // A paginação avança por altura. Mantemos o bloco inteiro para não
            // omitir transações quando o limite cai no meio de um bloco.
            if items.len() >= limit {
                break;
            }
        }
        let next_before_height = items
            .last()
            .and_then(|item| item.transaction.block_height)
            .and_then(|height| height.checked_sub(1));
        Ok(TransactionsPage {
            items,
            next_before_height,
            total_transactions: snapshot
                .blocks
                .iter()
                .flat_map(|block| &block.transactions)
                .filter(|transaction| filter.includes(transaction))
                .count(),
        })
    })
}

fn api_transaction(
    state: &ExplorerState,
    transaction_id: &str,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        let snapshot = state.snapshot()?;
        find_transaction(&snapshot, transaction_id)
            .ok_or_else(|| WynError::Validation("transação não encontrada".into()))
    })
}

fn api_address(
    state: &ExplorerState,
    address: &str,
    query: &HashMap<String, String>,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        if address.trim().is_empty() || address.len() > 256 {
            return Err(WynError::Validation("endereço inválido".into()));
        }
        let snapshot = state.snapshot()?;
        let limit = query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50)
            .clamp(1, 200);
        build_address_view(&snapshot, address, limit)
    })
}

fn api_mempool(
    state: &ExplorerState,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        let snapshot = state.snapshot()?;
        snapshot
            .mempool
            .iter()
            .map(|transaction| transaction_view(transaction, None, snapshot.status.height))
            .collect::<Result<Vec<_>>>()
    })
}

fn api_search(
    state: &ExplorerState,
    query: &HashMap<String, String>,
    stream: &mut TcpStream,
    head_only: bool,
) -> Result<()> {
    respond_with(stream, head_only, || {
        let term = query
            .get("q")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| WynError::Validation("informe um termo de busca".into()))?;
        if term.len() > 256 {
            return Err(WynError::Validation("termo de busca muito longo".into()));
        }

        let snapshot = state.snapshot()?;
        if let Ok(height) = term.parse::<u64>() {
            if let Some(block) = snapshot.blocks.iter().find(|block| block.index == height) {
                return Ok(json!({
                    "kind": "block",
                    "data": block_details(block, snapshot.status.height)?
                }));
            }
        }

        if let Some(block) = snapshot.blocks.iter().find(|block| block.hash == term) {
            return Ok(json!({
                "kind": "block",
                "data": block_details(block, snapshot.status.height)?
            }));
        }

        if let Some(transaction) = find_transaction(&snapshot, term) {
            return Ok(json!({
                "kind": "transaction",
                "data": transaction
            }));
        }

        let address = build_address_view(&snapshot, term, 50)?;
        if address.activity.is_empty() && address.confirmed_balance_atomic == "0" {
            return Err(WynError::Validation(
                "nenhum bloco, transação ou endereço foi encontrado".into(),
            ));
        }

        Ok(json!({
            "kind": "address",
            "data": address
        }))
    })
}

fn node_status_view(status: &NodeStatus) -> NodeStatusView {
    NodeStatusView {
        version: status.version.clone(),
        network_id: status.network_id.clone(),
        height: status.height,
        tip_hash: status.tip_hash.clone(),
        difficulty: status.difficulty,
        block_reward_atomic: status.block_reward.to_string(),
        block_reward_wyn: format_wyn(status.block_reward),
        mempool_size: status.mempool_size,
        mining_enabled: status.mining_enabled,
        miner_address: status.miner_address.clone(),
        database: status.database.clone(),
        uptime_seconds: status.uptime_seconds,
    }
}

fn block_summary(block: &Block) -> Result<BlockSummary> {
    let total_output = block
        .transactions
        .iter()
        .flat_map(|transaction| &transaction.outputs)
        .try_fold(0u64, |total, output| total.checked_add(output.amount))
        .ok_or_else(|| WynError::Validation("overflow nos outputs do bloco".into()))?;
    let coinbase = block.transactions.first().filter(|tx| tx.is_coinbase);
    let reward = coinbase
        .and_then(|transaction| transaction.outputs.first())
        .map(|output| output.amount);
    let miner_address = coinbase
        .and_then(|transaction| transaction.outputs.first())
        .map(|output| output.recipient.clone());
    let size_bytes = serde_json::to_vec(block)?.len();

    Ok(BlockSummary {
        height: block.index,
        hash: block.hash.clone(),
        previous_hash: block.header.prev_hash.clone(),
        merkle_root: block.header.merkle_root.clone(),
        timestamp: block.header.timestamp,
        difficulty: block.header.difficulty,
        nonce: block.header.nonce,
        transaction_count: block.transactions.len(),
        size_bytes,
        total_output_atomic: total_output.to_string(),
        total_output_wyn: format_wyn(total_output),
        miner_address,
        reward_atomic: reward.map(|value| value.to_string()),
        reward_wyn: reward.map(format_wyn),
    })
}

fn block_details(block: &Block, tip_height: u64) -> Result<BlockDetails> {
    Ok(BlockDetails {
        summary: block_summary(block)?,
        transactions: block
            .transactions
            .iter()
            .map(|transaction| transaction_view(transaction, Some(block.index), tip_height))
            .collect::<Result<Vec<_>>>()?,
        confirmations: tip_height.saturating_sub(block.index).saturating_add(1),
    })
}

fn transaction_view(
    transaction: &Transaction,
    block_height: Option<u64>,
    tip_height: u64,
) -> Result<TransactionView> {
    let total_output = transaction.checked_total_output()?;
    let confirmations = block_height
        .map(|height| tip_height.saturating_sub(height).saturating_add(1))
        .unwrap_or(0);

    Ok(TransactionView {
        id: transaction.id.clone(),
        timestamp: transaction.timestamp,
        is_coinbase: transaction.is_coinbase,
        coinbase_data: transaction.coinbase_data.clone(),
        input_count: transaction.inputs.len(),
        output_count: transaction.outputs.len(),
        total_output_atomic: total_output.to_string(),
        total_output_wyn: format_wyn(total_output),
        block_height,
        confirmations,
        inputs: transaction
            .inputs
            .iter()
            .map(|input| InputView {
                transaction_id: input.tx_id.clone(),
                output_index: input.output_index,
                signature: input.signature.clone(),
                public_key: input.public_key.clone(),
            })
            .collect(),
        outputs: transaction
            .outputs
            .iter()
            .enumerate()
            .map(|(index, output)| OutputView {
                index,
                recipient: output.recipient.clone(),
                amount_atomic: output.amount.to_string(),
                amount_wyn: format_wyn(output.amount),
            })
            .collect(),
    })
}

fn find_transaction(snapshot: &Snapshot, transaction_id: &str) -> Option<TransactionLocation> {
    for transaction in &snapshot.mempool {
        if transaction.id == transaction_id {
            return transaction_view(transaction, None, snapshot.status.height)
                .ok()
                .map(|transaction| TransactionLocation {
                    transaction,
                    block: None,
                    in_mempool: true,
                });
        }
    }

    for block in snapshot.blocks.iter().rev() {
        if let Some(transaction) = block
            .transactions
            .iter()
            .find(|transaction| transaction.id == transaction_id)
        {
            let view = transaction_view(transaction, Some(block.index), snapshot.status.height).ok()?;
            return Some(TransactionLocation {
                transaction: view,
                block: block_summary(block).ok(),
                in_mempool: false,
            });
        }
    }
    None
}

fn build_address_view(snapshot: &Snapshot, address: &str, activity_limit: usize) -> Result<AddressView> {
    let mut output_index: HashMap<(String, usize), (String, u64)> = HashMap::new();
    let mut outputs: HashMap<(String, usize), UtxoView> = HashMap::new();
    let mut spent_outputs = HashSet::new();
    let mut activities = Vec::new();
    let mut received = 0u64;
    let mut sent = 0u64;

    for block in &snapshot.blocks {
        for transaction in &block.transactions {
            let mut incoming = 0u64;
            let mut outgoing = 0u64;

            for input in &transaction.inputs {
                spent_outputs.insert((input.tx_id.clone(), input.output_index));
                if let Some((owner, amount)) =
                    output_index.get(&(input.tx_id.clone(), input.output_index))
                {
                    if owner == address {
                        outgoing = outgoing.checked_add(*amount).ok_or_else(|| {
                            WynError::Validation("overflow no histórico do endereço".into())
                        })?;
                    }
                }
            }

            for (index, output) in transaction.outputs.iter().enumerate() {
                if output.recipient == address {
                    incoming = incoming.checked_add(output.amount).ok_or_else(|| {
                        WynError::Validation("overflow no histórico do endereço".into())
                    })?;
                }
                output_index.insert(
                    (transaction.id.clone(), index),
                    (output.recipient.clone(), output.amount),
                );
                if output.recipient == address {
                    outputs.insert(
                        (transaction.id.clone(), index),
                        UtxoView {
                            transaction_id: transaction.id.clone(),
                            output_index: index,
                            block_height: block.index,
                            confirmations: snapshot.status.height.saturating_sub(block.index).saturating_add(1),
                            timestamp: transaction.timestamp,
                            amount_atomic: output.amount.to_string(),
                            amount_wyn: format_wyn(output.amount),
                        },
                    );
                }
            }

            if incoming > 0 || outgoing > 0 {
                received = received.checked_add(incoming).ok_or_else(|| {
                    WynError::Validation("overflow no total recebido".into())
                })?;
                sent = sent.checked_add(outgoing).ok_or_else(|| {
                    WynError::Validation("overflow no total enviado".into())
                })?;
                activities.push(address_activity(
                    transaction,
                    Some(block.index),
                    snapshot.status.height,
                    incoming,
                    outgoing,
                ));
            }
        }
    }

    for transaction in &snapshot.mempool {
        let mut incoming = 0u64;
        let mut outgoing = 0u64;
        for input in &transaction.inputs {
            if let Some((owner, amount)) = output_index.get(&(input.tx_id.clone(), input.output_index)) {
                if owner == address {
                    outgoing = outgoing.checked_add(*amount).ok_or_else(|| {
                        WynError::Validation("overflow no histórico do mempool".into())
                    })?;
                }
            }
        }
        for (index, output) in transaction.outputs.iter().enumerate() {
            if output.recipient == address {
                incoming = incoming.checked_add(output.amount).ok_or_else(|| {
                    WynError::Validation("overflow no histórico do mempool".into())
                })?;
            }
            output_index.insert(
                (transaction.id.clone(), index),
                (output.recipient.clone(), output.amount),
            );
        }
        if incoming > 0 || outgoing > 0 {
            activities.push(address_activity(
                transaction,
                None,
                snapshot.status.height,
                incoming,
                outgoing,
            ));
        }
    }

    activities.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    activities.truncate(activity_limit);
    let mut utxos = outputs
        .into_iter()
        .filter(|(key, _)| !spent_outputs.contains(key))
        .map(|(_, output)| output)
        .collect::<Vec<_>>();
    utxos.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    let confirmed_balance = received
        .checked_sub(sent)
        .ok_or_else(|| WynError::Validation("histórico do endereço ficou inconsistente".into()))?;

    Ok(AddressView {
        address: address.to_string(),
        confirmed_balance_atomic: confirmed_balance.to_string(),
        confirmed_balance_wyn: format_wyn(confirmed_balance),
        received_atomic: received.to_string(),
        received_wyn: format_wyn(received),
        sent_atomic: sent.to_string(),
        sent_wyn: format_wyn(sent),
        utxos,
        activity: activities,
    })
}

fn address_activity(
    transaction: &Transaction,
    block_height: Option<u64>,
    tip_height: u64,
    incoming: u64,
    outgoing: u64,
) -> AddressActivity {
    let net = i128::from(incoming) - i128::from(outgoing);
    AddressActivity {
        transaction_id: transaction.id.clone(),
        timestamp: transaction.timestamp,
        block_height,
        confirmations: block_height
            .map(|height| tip_height.saturating_sub(height).saturating_add(1))
            .unwrap_or(0),
        is_coinbase: transaction.is_coinbase,
        incoming_atomic: incoming.to_string(),
        incoming_wyn: format_wyn(incoming),
        outgoing_atomic: outgoing.to_string(),
        outgoing_wyn: format_wyn(outgoing),
        net_atomic: net.to_string(),
        net_wyn: format_signed_wyn(net),
    }
}

fn format_signed_wyn(value: i128) -> String {
    if value < 0 {
        let absolute = value.unsigned_abs();
        let whole = absolute / 100_000_000;
        let fraction = absolute % 100_000_000;
        format!("-{whole}.{fraction:08}")
    } else {
        let value = value as u128;
        let whole = value / 100_000_000;
        let fraction = value % 100_000_000;
        format!("{whole}.{fraction:08}")
    }
}

fn respond_with<T, F>(stream: &mut TcpStream, head_only: bool, operation: F) -> Result<()>
where
    T: Serialize,
    F: FnOnce() -> Result<T>,
{
    match operation() {
        Ok(data) => send_json(
            stream,
            200,
            &ApiEnvelope {
                ok: true,
                data: Some(data),
                error: None,
            },
            head_only,
        ),
        Err(error) => {
            let status = match &error {
                WynError::Validation(_) => 404,
                WynError::Protocol(_) => 502,
                WynError::Io(_) | WynError::Database(_) => 503,
                _ => 500,
            };
            send_json_error(stream, status, &error.to_string(), head_only)
        }
    }
}

fn send_json<T: Serialize>(
    stream: &mut TcpStream,
    status: u16,
    value: &T,
    head_only: bool,
) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    send_response(
        stream,
        status,
        "application/json; charset=utf-8",
        &body,
        CachePolicy::NoStore,
        head_only,
    )
}

fn send_json_error(
    stream: &mut TcpStream,
    status: u16,
    message: &str,
    head_only: bool,
) -> Result<()> {
    send_json(
        stream,
        status,
        &ApiEnvelope::<Value> {
            ok: false,
            data: None,
            error: Some(message.to_string()),
        },
        head_only,
    )
}

fn send_static(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> Result<()> {
    send_response(
        stream,
        status,
        content_type,
        body,
        CachePolicy::Static,
        head_only,
    )
}

enum CachePolicy {
    NoStore,
    Static,
}

fn send_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    cache_policy: CachePolicy,
    head_only: bool,
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let cache_control = match cache_policy {
        CachePolicy::NoStore => "no-store",
        CachePolicy::Static => "public, max-age=300",
    };
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         Cache-Control: {cache_control}\r\n\
         X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: DENY\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query_values() {
        assert_eq!(percent_decode("WYN%2Babc+123").unwrap(), "WYN+abc 123");
    }

    #[test]
    fn formats_signed_amounts() {
        assert_eq!(format_signed_wyn(150_000_000), "1.50000000");
        assert_eq!(format_signed_wyn(-150_000_000), "-1.50000000");
    }

    #[test]
    fn splits_target_and_query() {
        let (path, query) = split_target("/api/blocks?limit=20&before=100");
        assert_eq!(path, "/api/blocks");
        assert_eq!(query.get("limit").map(String::as_str), Some("20"));
        assert_eq!(query.get("before").map(String::as_str), Some("100"));
    }

    #[test]
    fn only_accepts_loopback() {
        assert!(validate_loopback("127.0.0.1:8080".parse().unwrap(), "--listen").is_ok());
        assert!(validate_loopback("0.0.0.0:8080".parse().unwrap(), "--listen").is_err());
        assert!(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST).is_loopback());
    }
}
