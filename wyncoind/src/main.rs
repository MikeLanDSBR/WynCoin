use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use wyncoin_core::blockchain::{Block, Blockchain, ChainParams};
use wyncoin_core::protocol::{
    ApiResponse, NodeStatus, P2pMessage, PeerHello, Request, MAX_P2P_MESSAGE_BYTES,
    MAX_REQUEST_BYTES, P2P_PROTOCOL_VERSION,
};
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
    p2p_enabled: bool,
    p2p_ready: bool,
    p2p_advertise: Option<String>,
    p2p_targets: Vec<String>,
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

    let miner_address = if let Some(address) = config.mining.miner_address.clone() {
        address
    } else {
        let miner_wallet_path =
            config.mining.miner_wallet.as_ref().ok_or_else(|| {
                WynError::Config("mining exige miner_address ou miner_wallet".into())
            })?;
        let wallet = if miner_wallet_path.exists() {
            Wallet::load_from_file(miner_wallet_path)?
        } else {
            let wallet = Wallet::generate()?;
            wallet.save_to_file(miner_wallet_path)?;
            println!(
                "[wyncoind] carteira legada do minerador criada em {}",
                miner_wallet_path.display()
            );
            wallet
        };
        wallet.address
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
        miner_address: miner_address.clone(),
        p2p_enabled: config.p2p.enabled,
        p2p_ready: !config.p2p.enabled,
        p2p_advertise: config.p2p.advertise.clone(),
        p2p_targets: config.p2p.seeds.clone(),
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

    let p2p_handles = spawn_p2p(
        Arc::clone(&runtime),
        Arc::clone(&stop),
        config.p2p.enabled,
        config.p2p.listen.clone(),
        config.p2p.seeds.clone(),
        config.p2p.max_peers,
    )?;
    let mining_handle = spawn_miner(
        Arc::clone(&runtime),
        Arc::clone(&stop),
        config.mining.interval_seconds,
    );

    println!("WynCoin node v{}", env!("CARGO_PKG_VERSION"));
    println!("  rede       : {}", config.network.id);
    println!("  API local  : {}", config.network.listen);
    println!("  banco      : {}", config.storage.database.display());
    println!("  minerador  : {}", miner_address);
    println!("  mineração  : {}", config.mining.enabled);
    if config.p2p.enabled {
        println!(
            "  P2P        : {} (seed: {})",
            config.p2p.listen,
            config.p2p.seeds.join(", ")
        );
    }
    println!("Use Ctrl+C para encerrar com segurança.");

    serve(&config.network.listen, runtime, Arc::clone(&stop))?;
    stop.store(true, Ordering::SeqCst);
    let _ = mining_handle.join();
    for handle in p2p_handles {
        let _ = handle.join();
    }
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
            let (should_mine, wait) = match runtime.lock() {
                Ok(state) => {
                    let should_mine = state.mining_enabled
                        && (!state.p2p_enabled || state.p2p_ready)
                        && (state.mine_empty_blocks || !state.blockchain.mempool.is_empty());
                    let wait = if should_mine {
                        mining_window_wait(&state).unwrap_or_else(|error| {
                            eprintln!("[miner] relógio inválido: {error}");
                            Duration::from_secs(interval_seconds.max(1))
                        })
                    } else {
                        Duration::from_secs(interval_seconds.max(1))
                    };
                    (should_mine, wait)
                }
                Err(_) => (false, Duration::from_secs(interval_seconds.max(1))),
            };

            if should_mine && wait.is_zero() {
                match mine_one(&runtime) {
                    Ok(block) => println!(
                        "[miner] bloco #{} confirmado: {}",
                        block.index,
                        block.hash.get(..20).unwrap_or(&block.hash)
                    ),
                    Err(WynError::Validation(message)) if message == "o topo da chain mudou durante a mineração" => {
                        println!("[miner] trabalho abandonado: novo bloco recebido da rede");
                    }
                    Err(error) => eprintln!("[miner] bloco descartado: {error}"),
                }
            }

            // O relógio da mineração acompanha o timestamp do topo, não o
            // instante em que o processo iniciou. Assim todos os nós chegam
            // à próxima janela de consenso, mesmo após sincronizarem tarde.
            let wait = if should_mine && wait.is_zero() {
                Duration::from_millis(50)
            } else {
                wait
            };
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
            state
                .blockchain
                .build_candidate_block(&state.miner_address)?,
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
    drop(state);
    broadcast_p2p(
        runtime,
        P2pMessage::AnnounceBlock {
            block: candidate.clone(),
        },
    );
    Ok(candidate)
}

fn ensure_mining_window(state: &NodeRuntime) -> Result<()> {
    let wait = mining_window_wait(state)?;
    if !wait.is_zero() {
        let remaining_ms = wait.as_millis();
        return Err(WynError::Validation(format!(
            "próximo bloco liberado em {} ms",
            remaining_ms
        )));
    }
    Ok(())
}

fn mining_window_wait(state: &NodeRuntime) -> Result<Duration> {
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
    let now =
        i64::try_from(now).map_err(|_| WynError::Validation("timestamp atual inválido".into()))?;

    let remaining_ms = next_timestamp.saturating_sub(now);
    Ok(Duration::from_millis(u64::try_from(remaining_ms).map_err(|_| {
        WynError::Validation("tempo de espera da mineração inválido".into())
    })?))
}

fn spawn_p2p(
    runtime: Arc<Mutex<NodeRuntime>>,
    stop: Arc<AtomicBool>,
    enabled: bool,
    listen: String,
    seeds: Vec<String>,
    max_peers: usize,
) -> Result<Vec<thread::JoinHandle<()>>> {
    if !enabled {
        return Ok(Vec::new());
    }
    let listener = TcpListener::bind(&listen)?;
    listener.set_nonblocking(true)?;
    let inbound_runtime = Arc::clone(&runtime);
    let inbound_stop = Arc::clone(&stop);
    let active_inbound = Arc::new(AtomicUsize::new(0));
    let inbound = thread::spawn(move || {
        while !inbound_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, peer)) => {
                    if active_inbound
                        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                            (current < max_peers).then_some(current + 1)
                        })
                        .is_err()
                    {
                        drop(stream);
                        continue;
                    }
                    let runtime = Arc::clone(&inbound_runtime);
                    let active_inbound = Arc::clone(&active_inbound);
                    thread::spawn(move || {
                        if let Err(error) = handle_p2p_inbound(stream, peer, runtime) {
                            eprintln!("[p2p] peer rejeitado: {error}");
                        }
                        active_inbound.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100))
                }
                Err(error) => {
                    eprintln!("[p2p] listener encerrou: {error}");
                    break;
                }
            }
        }
    });

    let sync = thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let mut peers = seeds.clone();
            if let Ok(state) = runtime.lock() {
                if let Ok(saved) = state.storage.load_peers(max_peers) {
                    peers.extend(saved);
                }
            }
            peers.sort();
            peers.dedup();
            peers.truncate(max_peers);
            let mut synchronized = false;
            for peer in peers {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                match synchronize_peer(&peer, &runtime, &listen) {
                    Ok(true) => synchronized = true,
                    Ok(false) => synchronized = true,
                    Err(error) => eprintln!("[p2p] não foi possível sincronizar {peer}: {error}"),
                }
            }
            if synchronized {
                if let Ok(mut state) = runtime.lock() {
                    state.p2p_ready = true;
                }
            }
            for _ in 0..50 {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
    });
    Ok(vec![inbound, sync])
}

fn peer_hello(state: &NodeRuntime) -> PeerHello {
    PeerHello {
        protocol_version: P2P_PROTOCOL_VERSION,
        network_id: state.blockchain.params.network_id.clone(),
        chain_id: state.blockchain.chain_id(),
        height: state.blockchain.height(),
        tip_hash: state.blockchain.last_block().hash.clone(),
        listen_address: state.p2p_advertise.clone(),
    }
}

fn validate_peer_hello(remote: &PeerHello, state: &NodeRuntime) -> Result<()> {
    if remote.protocol_version != P2P_PROTOCOL_VERSION {
        return Err(WynError::Protocol("versão P2P incompatível".into()));
    }
    if remote.network_id != state.blockchain.params.network_id
        || remote.chain_id != state.blockchain.chain_id()
    {
        return Err(WynError::Protocol(
            "peer pertence a outra rede ou possui regras de consenso diferentes".into(),
        ));
    }
    Ok(())
}

fn write_p2p(stream: &mut TcpStream, message: &P2pMessage) -> Result<()> {
    let mut body = serde_json::to_vec(message)?;
    if body.len() > MAX_P2P_MESSAGE_BYTES {
        return Err(WynError::Protocol("mensagem P2P excede o limite".into()));
    }
    body.push(b'\n');
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

/// `None` representa EOF limpo: o peer concluiu sua troca de mensagens e
/// fechou a conexão. Isso não é um protocolo inválido.
fn read_p2p(reader: &mut BufReader<TcpStream>) -> Result<Option<P2pMessage>> {
    let mut body = Vec::new();
    reader
        .by_ref()
        .take((MAX_P2P_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', &mut body)?;
    if body.is_empty() {
        return Ok(None);
    }
    if body.len() > MAX_P2P_MESSAGE_BYTES {
        return Err(WynError::Protocol(
            "mensagem P2P vazia ou excede o limite".into(),
        ));
    }
    Ok(Some(serde_json::from_slice(&body)?))
}

fn handle_p2p_inbound(
    mut stream: TcpStream,
    peer: SocketAddr,
    runtime: Arc<Mutex<NodeRuntime>>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(P2pMessage::Hello(remote)) = read_p2p(&mut reader)? else {
        return Err(WynError::Protocol("handshake P2P ausente".into()));
    };
    let hello = {
        let state = runtime
            .lock()
            .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
        validate_peer_hello(&remote, &state)?;
        peer_hello(&state)
    };
    write_p2p(&mut stream, &P2pMessage::Hello(hello))?;
    let _ = peer;
    if let Some(address) = remote.listen_address.as_deref() {
        remember_peer(&runtime, address);
    }
    loop {
        let Some(message) = read_p2p(&mut reader)? else {
            return Ok(());
        };
        match message {
            P2pMessage::GetBlocks {
                start_height,
                limit,
            } => {
                let (blocks, has_more) = with_p2p_state(&runtime, |state| {
                    let start = usize::try_from(start_height).unwrap_or(usize::MAX);
                    let limit = limit.clamp(1, 100);
                    let blocks = state
                        .blockchain
                        .chain
                        .get(start..)
                        .unwrap_or(&[])
                        .iter()
                        .take(limit)
                        .cloned()
                        .collect::<Vec<_>>();
                    let has_more =
                        start.saturating_add(blocks.len()) < state.blockchain.chain.len();
                    Ok((blocks, has_more))
                })?;
                write_p2p(&mut stream, &P2pMessage::Blocks { blocks, has_more })?;
            }
            P2pMessage::AnnounceBlock { block } => {
                accept_remote_block(&runtime, block)?;
            }
            P2pMessage::AnnounceTransaction { transaction } => {
                let _ = submit_p2p_transaction(&runtime, transaction);
            }
            P2pMessage::Peers { peers } if peers.is_empty() => {
                let peers = with_p2p_state(&runtime, |state| state.storage.load_peers(32))?;
                write_p2p(&mut stream, &P2pMessage::Peers { peers })?;
            }
            P2pMessage::Peers { peers } => {
                for address in peers.into_iter().take(32) {
                    if address.parse::<SocketAddr>().is_ok() {
                        remember_peer(&runtime, &address);
                    }
                }
            }
            _ => return Err(WynError::Protocol("mensagem P2P inválida".into())),
        }
    }
}

fn synchronize_peer(
    address: &str,
    runtime: &Arc<Mutex<NodeRuntime>>,
    listen: &str,
) -> Result<bool> {
    let socket: SocketAddr = address
        .parse()
        .map_err(|_| WynError::Protocol("endereço de peer inválido".into()))?;
    let mut stream = TcpStream::connect_timeout(&socket, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let _ = listen;
    let local = with_p2p_state(runtime, |state| Ok(peer_hello(state)))?;
    write_p2p(&mut stream, &P2pMessage::Hello(local))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(P2pMessage::Hello(remote)) = read_p2p(&mut reader)? else {
        return Err(WynError::Protocol("peer não respondeu handshake".into()));
    };
    with_p2p_state(runtime, |state| validate_peer_hello(&remote, state))?;
    if let Some(peer_address) = remote.listen_address.as_deref() {
        remember_peer(runtime, peer_address);
    }
    write_p2p(&mut stream, &P2pMessage::Peers { peers: Vec::new() })?;
    if let Some(P2pMessage::Peers { peers }) = read_p2p(&mut reader)? {
        for peer in peers.into_iter().take(32) {
            if peer.parse::<SocketAddr>().is_ok() {
                remember_peer(runtime, &peer);
            }
        }
    }
    let (local_height, local_tip) = with_p2p_state(runtime, |state| {
        Ok((
            state.blockchain.height(),
            state.blockchain.last_block().hash.clone(),
        ))
    })?;
    if remote.height < local_height
        || (remote.height == local_height && remote.tip_hash == local_tip)
    {
        return Ok(false);
    }
    let mut all_blocks = Vec::new();
    let mut start = 0u64;
    loop {
        write_p2p(
            &mut stream,
            &P2pMessage::GetBlocks {
                start_height: start,
                limit: 100,
            },
        )?;
        let Some(P2pMessage::Blocks { blocks, has_more }) = read_p2p(&mut reader)? else {
            return Err(WynError::Protocol(
                "peer respondeu sincronização inválida".into(),
            ));
        };
        if blocks.is_empty() {
            return Err(WynError::Protocol(
                "peer encerrou sincronização antes do esperado".into(),
            ));
        }
        start = start.saturating_add(blocks.len() as u64);
        all_blocks.extend(blocks);
        if !has_more {
            break;
        }
    }
    let replaced = {
        let mut state = runtime
            .lock()
            .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
        let mut candidate = state.blockchain.clone();
        if candidate.replace_if_heavier(all_blocks)? {
            state
                .storage
                .replace_chain(&candidate.chain, &candidate.mempool)?;
            state.blockchain = candidate;
            true
        } else {
            false
        }
    };
    if replaced {
        println!("[p2p] chain sincronizada de {address}");
    }
    Ok(replaced)
}

fn with_p2p_state<T>(
    runtime: &Arc<Mutex<NodeRuntime>>,
    operation: impl FnOnce(&NodeRuntime) -> Result<T>,
) -> Result<T> {
    let state = runtime
        .lock()
        .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
    operation(&state)
}
fn remember_peer(runtime: &Arc<Mutex<NodeRuntime>>, address: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|item| i64::try_from(item.as_secs()).ok())
        .unwrap_or(0);
    if let Ok(state) = runtime.lock() {
        let _ = state.storage.remember_peer(address, now);
    }
}
fn accept_remote_block(runtime: &Arc<Mutex<NodeRuntime>>, block: Block) -> Result<()> {
    let mut state = runtime
        .lock()
        .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
    if let Some(existing) = state.blockchain.chain.get(block.index as usize) {
        if existing.hash == block.hash {
            // Anúncios P2P podem retornar ao próprio originador. A mesma
            // unidade já confirmada não é erro nem precisa ser persistida.
            return Ok(());
        }
        return Err(WynError::Validation(
            "fork recebido na mesma altura; aguardando sincronização da chain".into(),
        ));
    }
    let mut next = state.blockchain.clone();
    next.commit_block(block.clone())?;
    state.storage.append_block(&block)?;
    state.blockchain = next;
    Ok(())
}
fn submit_p2p_transaction(
    runtime: &Arc<Mutex<NodeRuntime>>,
    transaction: wyncoin_core::Transaction,
) -> Result<()> {
    let mut state = runtime
        .lock()
        .map_err(|_| WynError::Protocol("estado do nó foi envenenado".into()))?;
    let mut next = state.blockchain.clone();
    next.add_to_mempool(transaction.clone())?;
    state.storage.insert_mempool_transaction(&transaction)?;
    state.blockchain = next;
    Ok(())
}

fn broadcast_p2p(runtime: &Arc<Mutex<NodeRuntime>>, message: P2pMessage) {
    let (mut peers, hello) = match runtime.lock() {
        Ok(state) if state.p2p_enabled => {
            let mut peers = state.p2p_targets.clone();
            if let Ok(saved) = state.storage.load_peers(32) {
                peers.extend(saved);
            }
            (peers, peer_hello(&state))
        }
        _ => return,
    };
    peers.sort();
    peers.dedup();
    thread::spawn(move || {
        for address in peers {
            let Ok(socket) = address.parse::<SocketAddr>() else {
                continue;
            };
            let Ok(mut stream) = TcpStream::connect_timeout(&socket, Duration::from_secs(3)) else {
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            if write_p2p(&mut stream, &P2pMessage::Hello(hello.clone())).is_err() {
                continue;
            }
            let Ok(mut reader) = stream.try_clone().map(BufReader::new) else {
                continue;
            };
            if !matches!(read_p2p(&mut reader), Ok(Some(P2pMessage::Hello(_)))) {
                continue;
            }
            let _ = write_p2p(&mut stream, &message);
        }
    });
}

fn serve(address: &str, runtime: Arc<Mutex<NodeRuntime>>, stop: Arc<AtomicBool>) -> Result<()> {
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
            Ok(ApiResponse::success(state.blockchain.balance_of(&address)?))
        }),
        Request::Utxos { address } => with_state(runtime, |state| {
            Ok(ApiResponse::success(
                state.blockchain.get_utxos_for(&address),
            ))
        }),
        Request::SubmitTransaction { transaction } => submit_transaction(runtime, transaction),
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
    drop(state);
    broadcast_p2p(
        runtime,
        P2pMessage::AnnounceTransaction {
            transaction: transaction.clone(),
        },
    );
    Ok(ApiResponse::success(serde_json::json!({
        "txid": transaction.id,
        "fee": fee
    })))
}

fn with_state<F>(runtime: &Arc<Mutex<NodeRuntime>>, operation: F) -> Result<ApiResponse>
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
