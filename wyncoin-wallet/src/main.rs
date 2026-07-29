use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::Value;
use wyncoin_core::blockchain::Utxo;
use wyncoin_core::{format_wyn, parse_wyn, send_request, Request, Result, Wallet};

#[derive(Debug, Parser)]
#[command(name = "wyncoin-wallet", version, about = "Carteira WynCoin v0.1.0")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:9332")]
    node: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    New {
        #[arg(short, long)]
        output: PathBuf,
    },
    Info {
        #[arg(short, long)]
        file: PathBuf,
    },
    /// Imprime apenas o endereço; destinado a scripts de instalação.
    Address {
        #[arg(short, long)]
        file: PathBuf,
    },
    Balance {
        #[arg(short, long)]
        file: Option<PathBuf>,
        #[arg(long)]
        address: Option<String>,
    },
    Send {
        #[arg(short, long)]
        file: PathBuf,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: String,
        #[arg(long, default_value = "0.001")]
        fee: String,
    },
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
        Command::New { output } => {
            if output.exists() {
                return Err(wyncoin_core::WynError::Validation(format!(
                    "o arquivo {} já existe",
                    output.display()
                )));
            }
            let wallet = Wallet::generate()?;
            wallet.save_to_file(&output)?;
            println!("Carteira criada: {}", wallet.address);
            println!("Arquivo: {}", output.display());
            println!("ATENÇÃO: v0.1.0 salva a chave privada sem senha; proteja o arquivo.");
        }
        Command::Info { file } => {
            let wallet = Wallet::load_from_file(&file)?;
            println!("Endereço: {}", wallet.address);
            println!("Arquivo : {}", file.display());
        }
        Command::Address { file } => {
            println!("{}", Wallet::load_from_file(&file)?.address);
        }
        Command::Balance { file, address } => {
            let address = match (file, address) {
                (Some(file), None) => Wallet::load_from_file(file)?.address.clone(),
                (None, Some(address)) => address,
                _ => {
                    return Err(wyncoin_core::WynError::Validation(
                        "informe exatamente um entre --file e --address".into(),
                    ))
                }
            };
            let response = send_request(
                &args.node,
                &Request::Balance {
                    address: address.clone(),
                },
            )?;
            let balance: u64 = response.require_data()?;
            println!("{}: {} WYN", address, format_wyn(balance));
        }
        Command::Send {
            file,
            to,
            amount,
            fee,
        } => {
            let wallet = Wallet::load_from_file(&file)?;
            let amount = parse_wyn(&amount)?;
            let fee = parse_wyn(&fee)?;

            let response = send_request(
                &args.node,
                &Request::Utxos {
                    address: wallet.address.clone(),
                },
            )?;
            let utxos: Vec<Utxo> = response.require_data()?;
            let transaction = wallet.build_transaction(&utxos, &to, amount, fee)?;
            let txid = transaction.id.clone();

            let response = send_request(&args.node, &Request::SubmitTransaction { transaction })?;
            let result: Value = response.require_data()?;
            println!("Transação aceita no mempool.");
            println!("TXID : {txid}");
            println!("Taxa : {} WYN", format_wyn(fee));
            println!("Resposta do nó: {}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}
