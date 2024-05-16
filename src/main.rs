mod classification;
mod config;
mod group;
mod state;
mod threshold;

use bincode::Options;
use config::Config;
use crossbeam::channel::{unbounded, RecvTimeoutError};
use solana_sdk::txingest::TxIngestMsg;
use state::State;
use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;

fn main()
{
    let input_args = std::env::args().skip(1).collect::<Vec<String>>();

    if (input_args.len() < 2) || (input_args.len() > 3) {
        eprintln!("ERROR: Incorrect number of arguments: must be: <LISTEN_ADDRESS> <LISTEN_PORT> [CONFIG_JSON_FILE]");
        eprintln!("Examples:");
        eprintln!("  # To listen on localhost at port 15151, and use the default ./config.json file:");
        eprintln!("  txingest-classifier 127.0.0.1 15151");
        eprintln!("  # To listen on localhost at port 15151, and use the config file /etc/txingest.json file:");
        eprintln!("  txingest-classifier 127.0.0.1 15151 /etc/txingest.json");
        std::process::exit(-1);
    }

    let host = input_args[0]
        .parse::<Ipv4Addr>()
        .unwrap_or_else(|e| error_exit(format!("ERROR: Invalid listen address {}: {e}", input_args[0])));
    let port = input_args[1]
        .parse::<u16>()
        .unwrap_or_else(|e| error_exit(format!("ERROR: Invalid listen port {}: {e}", input_args[1])));
    let config = if input_args.len() == 3 { input_args[2].clone() } else { "config.json".to_string() };
    let config =
        load_config(&config).unwrap_or_else(|e| error_exit(format!("ERROR: Failed to read config file {config}: {e}")));

    // Listen
    let tcp_listener = loop {
        match TcpListener::bind(std::net::SocketAddr::V4(std::net::SocketAddrV4::new(host, port))) {
            Ok(tcp_listener) => break tcp_listener,
            Err(e) => {
                eprintln!("Failed bind because {e}, trying again in 1 second");
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    };

    let (sender, receiver) = unbounded::<TxIngestMsg>();

    let sender = Arc::new(sender);

    // Spawn the listener
    std::thread::spawn(move || {
        loop {
            let mut tcp_stream = loop {
                match tcp_listener.accept() {
                    Ok((tcp_stream, _)) => break tcp_stream,
                    Err(e) => eprintln!("Failed accept because {e}")
                }
            };

            {
                let sender = sender.clone();

                // Spawn a thread to handle this TCP stream.  Multiple streams are accepted at once, to allow e.g.
                // a JITO relayer and a validator to both connect.
                std::thread::spawn(move || {
                    let options = bincode::DefaultOptions::new();

                    loop {
                        match options.deserialize_from::<_, TxIngestMsg>(&mut tcp_stream) {
                            Ok(tx_ingest_msg) => sender.send(tx_ingest_msg).expect("crossbeam failed"),
                            Err(e) => {
                                eprintln!("Failed deserialize because {e}; closing connection");
                                tcp_stream.shutdown(std::net::Shutdown::Both).ok();
                                break;
                            }
                        }
                    }
                });
            }
        }
    });

    let mut state = State::new(config);

    let mut last_log_timestamp = 0;

    loop {
        // Receive with a timeout
        match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => (),
            Ok(TxIngestMsg::Failed { timestamp, peer_addr }) => state.failed(timestamp, peer_addr),
            Ok(TxIngestMsg::Exceeded { timestamp, peer_addr, peer_pubkey, stake }) => {
                state.exceeded(timestamp, peer_addr, peer_pubkey, stake)
            },
            Ok(TxIngestMsg::Started { timestamp, peer_addr, peer_pubkey, stake }) => {
                state.started(timestamp, peer_addr, peer_pubkey, stake)
            },
            Ok(TxIngestMsg::Finished { timestamp, peer_addr }) => state.finished(timestamp, peer_addr),
            Ok(TxIngestMsg::VoteTx { timestamp, peer_addr }) => state.votetx(timestamp, peer_addr),
            Ok(TxIngestMsg::UserTx { timestamp, peer_addr, signature }) => {
                state.usertx(timestamp, peer_addr, signature)
            },
            Ok(TxIngestMsg::Forwarded { timestamp, signature }) => state.forwarded(timestamp, signature),
            Ok(TxIngestMsg::BadFee { timestamp, signature }) => state.badfee(timestamp, signature),
            Ok(TxIngestMsg::Fee { timestamp, signature, cu_limit, cu_used, fee }) => {
                state.fee(timestamp, signature, cu_limit, cu_used, fee)
            },
            Ok(TxIngestMsg::WillBeLeader { timestamp, slots }) => state.will_be_leader(timestamp, slots),
            Ok(TxIngestMsg::BeginLeader { timestamp }) => state.begin_leader(timestamp),
            Ok(TxIngestMsg::EndLeader { timestamp }) => state.end_leader(timestamp),
            Ok(TxIngestMsg::Deprecated) => ()
        }

        let now = now_millis();
        if now < (last_log_timestamp + 1000) {
            continue;
        }

        state.periodic(now);

        last_log_timestamp = now;
    }
}

fn error_exit(msg : String) -> !
{
    eprintln!("{msg}");
    std::process::exit(-1);
}

fn load_config(path : &str) -> Result<Config, String>
{
    let mut config = serde_json::from_reader::<_, Config>(read_file(&path)).map_err(|e| e.to_string())?;

    config.validate()?;

    Ok(config)
}

fn maybe_read_file(path : &str) -> Option<Box<dyn std::io::Read>>
{
    if std::path::Path::exists(std::path::Path::new(&path)) {
        eprintln!("Reading {path}");
        Some(Box::new(std::io::BufReader::<std::fs::File>::new(
            std::fs::File::open(path)
                .unwrap_or_else(|e| error_exit(format!("ERROR: Failed to open {path} for reading: {e}")))
        )))
    }
    else {
        None
    }
}

fn read_file(path : &str) -> Box<dyn std::io::Read>
{
    maybe_read_file(path)
        .unwrap_or_else(|| error_exit(format!("ERROR: Failed to open {path} for reading: file does not exist")))
}

fn now_millis() -> u64
{
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}
