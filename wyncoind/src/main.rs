use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use wyncoin_core::blockchain::{Block, Blockchain, ChainParams};
use wyncoin_core::protocol::{ApiResponse, NodeStatus, Request, MAX_REQUEST_BYTES};
use wyncoin_core::{NodeConfig, Result, Storage, Wallet, WynError};

#[derive(Debug, Parser)]
#[command(name = "wyncoind", version, about = "Nó persistente WynCoin v0.1.0")]
struct Args {
    #[arg(long, default_value = "data/node.toml")]
    config: PathBuf,

    #[arg(long)]
    init_config: bool,
}

struct NodeRuntime {
    blockchain: Blockchain,
    storage: Storage,
    mining_enabled: bool,
    mine_empty_blocks: bool,
    min_block_interval_seconds: u64,
    miner_address: String,
    started_at: Instant,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("[wyncoind] erro fatal: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    if args.init_config {
        NodeConfig::write_default(&args.config)?;
        println!("Configuração criada em {}", args.config.display());
        return Ok(());
    }

    let config = NodeConfig::load(&args.config)?;
    let storage = Storage::new(config.storage.database.clone());
    storage.initialize()?;

    let miner_wallet = if config.mining.miner_wallet.exists() {
        Wallet::load_from_file(&config.mining.miner_wallet)?
    } else {
        let wallet = Wallet::generate()?;
        wallet.save_to_file(&config.mining.miner_wallet)?;
        println!(
            "[wyncoind] carteira do minerador criada em {}",
            config.mining.miner_wallet.display()
        );
        wallet
    };

    let params = ChainParams {
        network_id: config.network.id.clone(),
        difficulty: config.chain.difficulty,
        block_reward: config.chain.block_reward,
        min_block_interval_seconds: config.chain.min_block_interval_seconds,
        max_transactions_per_block: config.chain.max_transactions_per_block,
    };

    let fresh_chain = Blockchain::new(params.clone());
    storage.insert_genesis_if_empty(&fresh_chain.chain[0])?;
    let blocks = storage.load_blocks()?;
    let mempool = storage.load_mempool()?;
    let blockchain = Blockchain::from_persisted(params, blocks, mempool)?;

    let runtime = Arc::new(Mutex::new(NodeRuntime {
        blockchain,
        storage,
        mining_enabled: config.mining.enabled,
        mine_empty_blocks: config.mining.mine_empty_blocks,
        min_block_interval_seconds: config.chain.min_block_interval_seconds,
        miner_address: miner_wallet.address.clone(),
        started_at: Instant::now(),
    }));
    let stop = Arc::new(AtomicBool::new(false));

    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || {
            stop.store(true, Ordering::SeqCst);
        })
        .map_err(|error| WynError::Config(format!("falha ao instalar Ctrl+C: {error}")))?;
    }

    let mining_handle = spawn_miner(
        Arc::clone(&runtime),
        Arc::clone(&stop),
        config.mining.interval_seconds,
    );

    println!("WynCoin node v{}", env!("CARGO_PKG_VERSION"));
    println!("  rede       : {}", config.network.id);
    println!("  API local  : {}", config.network.listen);
    println!("  banco      : {}", config.storage.database.display());
    println!("  minerador  : {}", miner_wallet.address);
    println!("  mineração  : {}", config.mining.enabled);
    println!("Use Ctrl+C para encerrar com segurança.");

    serve(&config.network.listen, runtime, Arc::clone(&stop))?;
    stop.store(true, Ordering::SeqCst);
    let _ = mining_handle.join();
    println!("[wyncoind] encerrado.");
    Ok(())
}

fn spawn_miner(
    runtime: Arc<Mutex<NodeRuntime>>,
    stop: Arc<AtomicBool>,
    interval_seconds: u64,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let should_mine = match runtime.lock() {
                Ok(state) => {
                    state.mining_enabled
                        && (state.mine_empty_blocks || !state.blockchain.mempool.is_empty())
                }
                Err(_) => false,
            };

            if should_mine {
                match mine_one(&runtime) {
                    Ok(block) => println!(
                        "[miner] bloco #{} confirmado: {}",
                        block.index,
                        block.hash.get(..20).unwrap_or(&block.hash)
                    ),
                    Err(error) => eprintln!("[miner] bloco descartado: {error}"),
                }
            }

            let wait = Duration::from_secs(interval_seconds.max(1));
            let start = Instant::now();
            while start.elapsed() < wait && !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(200));
            }
        }
    })
}

fn mine_one(runtime: &Arc<Mutex<NodeRuntime>>) -> Result<Block> {
    let (mut candidate, expected_tip) = {
        let state = runtime
            .lock()
            .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
        ensure_mining_window(&state)?;
        (
            state.blockchain.build_candidate_block(&state.miner_address)?,
            state.blockchain.last_block().hash.clone(),
        )
    };

    candidate.mine()?;

    let mut state = runtime
        .lock()
        .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
    if state.blockchain.last_block().hash != expected_tip {
        return Err(WynError::Validation(
            "o topo da chain mudou durante a mineração".into(),
        ));
    }

    let mut next = state.blockchain.clone();
    next.commit_block(candidate.clone())?;
    state.storage.append_block(&candidate)?;
    state.blockchain = next;
    Ok(candidate)
}

fn ensure_mining_window(state: &NodeRuntime) -> Result<()> {
    let interval_ms = i64::try_from(state.min_block_interval_seconds)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000))
        .ok_or_else(|| WynError::Validation("intervalo mínimo de bloco inválido".into()))?;
    let next_timestamp = state
        .blockchain
        .last_block()
        .header
        .timestamp
        .checked_add(interval_ms)
        .ok_or_else(|| WynError::Validation("timestamp mínimo de bloco excedido".into()))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WynError::Validation("relógio do sistema anterior ao Unix epoch".into()))?
        .as_millis();
    let now = i64::try_from(now)
        .map_err(|_| WynError::Validation("timestamp atual inválido".into()))?;

    if now < next_timestamp {
        let remaining_ms = next_timestamp - now;
        return Err(WynError::Validation(format!(
            "próximo bloco liberado em {} ms",
            remaining_ms
        )));
    }
    Ok(())
}

fn serve(
    address: &str,
    runtime: Arc<Mutex<NodeRuntime>>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let listener = TcpListener::bind(address)?;
    listener.set_nonblocking(true)?;

    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _peer)) => {
                let runtime = Arc::clone(&runtime);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, runtime) {
                        eprintln!("[api] {error}");
                    }
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, runtime: Arc<Mutex<NodeRuntime>>) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = Vec::new();
    reader.read_until(b'\n', &mut request_line)?;
    if request_line.len() > MAX_REQUEST_BYTES {
        write_response(&mut stream, ApiResponse::failure("requisição excede 1 MiB"))?;
        return Ok(());
    }

    let request: Request = match serde_json::from_slice(&request_line) {
        Ok(request) => request,
        Err(error) => {
            write_response(
                &mut stream,
                ApiResponse::failure(format!("JSON de requisição inválido: {error}")),
            )?;
            return Ok(());
        }
    };

    let response = handle_request(request, &runtime);
    write_response(&mut stream, response)
}

fn handle_request(request: Request, runtime: &Arc<Mutex<NodeRuntime>>) -> ApiResponse {
    match handle_request_inner(request, runtime) {
        Ok(response) => response,
        Err(error) => ApiResponse::failure(error.to_string()),
    }
}

fn handle_request_inner(
    request: Request,
    runtime: &Arc<Mutex<NodeRuntime>>,
) -> Result<ApiResponse> {
    match request {
        Request::Ping => Ok(ApiResponse::success("pong")),
        Request::Status => status_response(runtime),
        Request::Balance { address } => with_state(runtime, |state| {
            Ok(ApiResponse::success(
                state.blockchain.balance_of(&address)?,
            ))
        }),
        Request::Utxos { address } => with_state(runtime, |state| {
            Ok(ApiResponse::success(
                state.blockchain.get_utxos_for(&address),
            ))
        }),
        Request::SubmitTransaction { transaction } => {
            submit_transaction(runtime, transaction)
        }
        Request::Mine { blocks } => {
            if blocks != 1 {
                return Err(WynError::Validation(
                    "a mineração administrativa confirma exatamente 1 bloco e respeita o intervalo da rede".into(),
                ));
            }
            Ok(ApiResponse::success(vec![mine_one(runtime)?]))
        }
        Request::Blocks { limit } => with_state(runtime, |state| {
            let limit = limit.clamp(1, 100);
            let start = state.blockchain.chain.len().saturating_sub(limit);
            Ok(ApiResponse::success(
                state.blockchain.chain[start..].to_vec(),
            ))
        }),
        Request::Block { height } => with_state(runtime, |state| {
            let block = state
                .blockchain
                .chain
                .get(height as usize)
                .cloned()
                .ok_or_else(|| WynError::Validation("bloco não encontrado".into()))?;
            Ok(ApiResponse::success(block))
        }),
        Request::Mempool => with_state(runtime, |state| {
            Ok(ApiResponse::success(state.blockchain.mempool.clone()))
        }),
    }
}

fn status_response(runtime: &Arc<Mutex<NodeRuntime>>) -> Result<ApiResponse> {
    with_state(runtime, |state| {
        Ok(ApiResponse::success(NodeStatus {
            version: env!("CARGO_PKG_VERSION").into(),
            network_id: state.blockchain.params.network_id.clone(),
            height: state.blockchain.height(),
            tip_hash: state.blockchain.last_block().hash.clone(),
            difficulty: state.blockchain.params.difficulty,
            block_reward: state.blockchain.params.block_reward,
            mempool_size: state.blockchain.mempool.len(),
            mining_enabled: state.mining_enabled,
            miner_address: state.miner_address.clone(),
            database: state.storage.path().display().to_string(),
            uptime_seconds: state.started_at.elapsed().as_secs(),
        }))
    })
}

fn submit_transaction(
    runtime: &Arc<Mutex<NodeRuntime>>,
    transaction: wyncoin_core::Transaction,
) -> Result<ApiResponse> {
    let mut state = runtime
        .lock()
        .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
    let mut next = state.blockchain.clone();
    let fee = next.add_to_mempool(transaction.clone())?;
    state.storage.insert_mempool_transaction(&transaction)?;
    state.blockchain = next;
    Ok(ApiResponse::success(serde_json::json!({
        "txid": transaction.id,
        "fee": fee
    })))
}

fn with_state<F>(
    runtime: &Arc<Mutex<NodeRuntime>>,
    operation: F,
) -> Result<ApiResponse>
where
    F: FnOnce(&NodeRuntime) -> Result<ApiResponse>,
{
    let state = runtime
        .lock()
        .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
    operation(&state)
}

fn write_response(stream: &mut TcpStream, response: ApiResponse) -> Result<()> {
    let mut body = serde_json::to_vec(&response)?;
    body.push(b'\n');
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}
