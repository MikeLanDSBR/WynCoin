use clap::{Parser, Subcommand};
use wyncoin_core::blockchain::{Block, Transaction, Utxo};
use wyncoin_core::{format_wyn, send_request, NodeStatus, Request, Result};

#[derive(Debug, Parser)]
#[command(name = "wyncoin-cli", version, about = "Cliente administrativo do wyncoind")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:9332")]
    node: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Ping,
    Status,
    Balance { address: String },
    Utxos { address: String },
    Mine {
        #[arg(default_value_t = 1)]
        blocks: u32,
    },
    Blocks {
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    Block { height: u64 },
    Mempool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("erro: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Ping => {
            let response = send_request(&args.node, &Request::Ping)?;
            let value: String = response.require_data()?;
            println!("{value}");
        }
        Command::Status => {
            let response = send_request(&args.node, &Request::Status)?;
            let status: NodeStatus = response.require_data()?;
            println!("WynCoin node v{}", status.version);
            println!("  rede       : {}", status.network_id);
            println!("  altura     : {}", status.height);
            println!("  topo       : {}", status.tip_hash);
            println!("  dificuldade: {}", status.difficulty);
            println!("  recompensa : {} WYN", format_wyn(status.block_reward));
            println!("  mempool    : {}", status.mempool_size);
            println!("  mineração  : {}", status.mining_enabled);
            println!("  minerador  : {}", status.miner_address);
            println!("  banco      : {}", status.database);
            println!("  uptime     : {} s", status.uptime_seconds);
        }
        Command::Balance { address } => {
            let response = send_request(&args.node, &Request::Balance { address: address.clone() })?;
            let balance: u64 = response.require_data()?;
            println!("{}: {} WYN", address, format_wyn(balance));
        }
        Command::Utxos { address } => {
            let response = send_request(&args.node, &Request::Utxos { address })?;
            let utxos: Vec<Utxo> = response.require_data()?;
            println!("{}", serde_json::to_string_pretty(&utxos)?);
        }
        Command::Mine { blocks } => {
            let response = send_request(&args.node, &Request::Mine { blocks })?;
            let mined: Vec<Block> = response.require_data()?;
            for block in mined {
                println!(
                    "bloco #{} {} nonce={} txs={}",
                    block.index,
                    block.hash,
                    block.header.nonce,
                    block.transactions.len()
                );
            }
        }
        Command::Blocks { limit } => {
            let response = send_request(&args.node, &Request::Blocks { limit })?;
            let blocks: Vec<Block> = response.require_data()?;
            for block in blocks {
                println!(
                    "#{:<8} {} txs={} nonce={}",
                    block.index,
                    block.hash,
                    block.transactions.len(),
                    block.header.nonce
                );
            }
        }
        Command::Block { height } => {
            let response = send_request(&args.node, &Request::Block { height })?;
            let block: Block = response.require_data()?;
            println!("{}", serde_json::to_string_pretty(&block)?);
        }
        Command::Mempool => {
            let response = send_request(&args.node, &Request::Mempool)?;
            let transactions: Vec<Transaction> = response.require_data()?;
            println!("{}", serde_json::to_string_pretty(&transactions)?);
        }
    }
    Ok(())
}
