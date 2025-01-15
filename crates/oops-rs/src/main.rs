use alloy::{
    hex,
    primitives::{Address, Bytes, U256},
    providers::{IpcConnect, Provider, ProviderBuilder},
    rpc::{client::ClientBuilder, types::Transaction},
    sol,
    sol_types::SolCall,
};
use chrono::Local;
use ethers_core::abi::{decode, ParamType};
use overlord_shared_types::{MessageBundle, PriceUpdateBundle};
use std::{
    error::Error,
    fs::File,
    io::{
        self,
        BufRead,
    },
    path::Path,
    str::FromStr,
};
use tokio::{
    sync::broadcast,
    time::{sleep, Duration},
};
use tracing::{debug, info};
use tracing_appender::rolling::{
    self,
    Rotation,
};
use tracing_subscriber::fmt::{
    time::LocalTime,
    writer::BoxMakeWriter,
};

sol!(
    #[allow(missing_docs)]
    function forward(
        address to,
        bytes calldata data
    ) external;

    #[allow(missing_docs)]
    function transmit(
        bytes32[3] calldata reportContext,
        bytes calldata report,
        bytes32[] calldata rs,
        bytes32[] calldata ss,
        bytes32 rawVs
    ) external override;
);

const SECONDS_BEFORE_RECONNECTION: u64 = 2;
const PATH_TO_ADDRESSES_INPUT: &str = "crates/oops-rs/addresses.txt";
const VEGA_INBOUND_ENDPOINT: &str = "ipc:///tmp/vega_inbound";

fn get_price_from_input(tx_input: &Bytes) -> Result<(U256, Address), Box<dyn Error>> {
    // get `data` from forward(address to, bytes calldata data)
    let forward_calldata = match forwardCall::abi_decode(tx_input, false) {
        Ok(data) => data,
        Err(e) => return Err(Box::new(e)),
    };
    let forward_data = forward_calldata.data;

    // get `report` from transmit(bytes32[3] calldata reportContext, bytes calldata report, bytes32[] calldata rs, bytes32[] calldata ss, bytes32 rawVs)
    let transmit_report = transmitCall::abi_decode(&forward_data, false).unwrap();
    let transmit_report = transmit_report.report;

    // this is what the function _decodeReport(bytes memory rawReport) of OCR2Aggregator.sol does
    let decoded_transmit_report = decode(
        &[
            ParamType::Uint(32),                             // observationsTimestamp
            ParamType::FixedBytes(32),                       // rawObservers
            ParamType::Array(Box::new(ParamType::Int(192))), // observations
            ParamType::Int(192),                             // juelsPerFeeCoin
        ],
        &transmit_report,
    )
    .unwrap();

    let observations = decoded_transmit_report[2].clone().into_array().unwrap();
    let median = &observations[observations.len() / 2];
    let answer = U256::from_str_radix(&median.to_string(), 16).unwrap();

    Ok((answer, forward_calldata.to))
}

fn read_addresses_from_file(filename: &str) -> io::Result<Vec<alloy::primitives::Address>> {
    let path = Path::new(filename);
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut addresses = Vec::new();
    for line in reader.lines() {
        let line = line?;
        addresses.push(Address::from_str(str::trim(&line)).expect("Failed to parse address"));
    }
    Ok(addresses)
}

/// Check if the input data of a transaction is a call to the `transmit` function
///
/// The `transmit` function is defined in OCR2Aggregator.sol as:
///
/// ```solidity
///function transmit(
///    // reportContext consists of:
///    // reportContext[0]: ConfigDigest
///    // reportContext[1]: 27 byte padding, 4-byte epoch and 1-byte round
///    // reportContext[2]: ExtraHash
///    bytes32[3] calldata reportContext,
///    bytes calldata report,
///    // ECDSA signatures
///    bytes32[] calldata rs,
///    bytes32[] calldata ss,
///    bytes32 rawVs
///  )
/// ```
///
/// so when parsed with a Keccak calculator, the input is
///
/// `transmit(bytes32[3],bytes,bytes32[],bytes32[],bytes32)`
///
/// and the output is
///
/// `b1dc65a4ef09ffb2b382edbb04cc9015e5cbfbfa065219b3e7c00664cb13397a`
///
/// and the selector is
///
/// `b1dc65a4`
fn is_transmit_call(tx_body: &Transaction) -> bool {
    let tx_input = &tx_body.input;
    let transmit_selector = hex::decode("b1dc65a4").expect("Decoding failed");
    if tx_input.len() < 128 {
        // input data is too short. This is not a valid transmit call
        // and it's quite frequent so not logging this
        return false;
    }
    // the first 8 bytes of the input is the selector for forward() function
    // so we begin looking at position 100 because the transmit() selector
    // would be at slot 3 (check Input Data default view in etherscan)
    // so then 3 * 32 = 96 + 4 = 100
    if tx_input.len() < 132 {
        eprintln!(
            "INVALID TRANSMIT: looked valid, but tx_input length ({}) is too short. tx_hash was {}",
            tx_input.len(),
            tx_body.hash
        );
        return false;
    }
    let selector_chunk = &tx_input[100..132];
    selector_chunk.starts_with(&transmit_selector)
}

fn _init_addresses(file_path: String) -> Result<Vec<Address>, Box<dyn Error>> {
    let allowed_addresses =
        read_addresses_from_file(&file_path).expect("Failed to read addresses from file");
    let addresses_str = allowed_addresses
        .iter()
        .map(|addr| format!("{:?}", addr))
        .collect::<Vec<String>>()
        .join(", ");
    debug!("Allowed addresses: {}", addresses_str);
    Ok(allowed_addresses)
}

fn _setup_logging() {
    let log_file = rolling::RollingFileAppender::new(Rotation::DAILY, "/var/log/overlord-rs", "oops-rs.log");
    let file_writer = BoxMakeWriter::new(log_file);
    tracing_subscriber::fmt()
        .with_writer(file_writer)
        .with_timer(LocalTime::rfc_3339())
        .init();
}

#[tokio::main]
async fn main() {
    _setup_logging();

    let allowed_addresses =
        _init_addresses(String::from(PATH_TO_ADDRESSES_INPUT)).expect("Failed to initialize addresses");

    loop {
        // Outer loop to restart IPC on major connection issues
        let ipc_url = "/tmp/reth.ipc";
        let ipc = IpcConnect::new(ipc_url.to_string());
        let client = match ClientBuilder::default().ipc(ipc).await {
            Ok(client) => {
                client.set_channel_size(2048);
                client
            }
            Err(e) => {
                eprintln!(
                    "Failed to connect to IPC: {e}. Retrying in {} seconds...",
                    SECONDS_BEFORE_RECONNECTION
                );
                sleep(Duration::from_secs(SECONDS_BEFORE_RECONNECTION)).await;
                continue;
            }
        };
        let provider = ProviderBuilder::new().on_client(client);
        let mut pending_tx_stream = match provider.subscribe_full_pending_transactions().await {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("Failed to subscribe to pending transactions: {e}. Retrying...");
                continue;
            }
        };

        let (tx_buffer, mut rx_buffer) = broadcast::channel::<Transaction>(1024);

        let receiver_handle = tokio::spawn(async move {
            loop {
                match pending_tx_stream.recv().await {
                    Ok(tx_body) => {
                        if let Err(e) = tx_buffer.send(tx_body) {
                            eprintln!("Failed to send tx to buffer: {e}");
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("Unknow stream enqueue error: {e}");
                        break;
                    }
                }
            }
        });

        let processor_handle = tokio::spawn({
            let allowed_addresses = allowed_addresses.clone();
            let provider_clone = provider.clone();
            let vega_context = zmq::Context::new();
            let vega_socket = vega_context.socket(zmq::PUSH).unwrap();
            match vega_socket.connect(VEGA_INBOUND_ENDPOINT) {
                Ok(_) => eprintln!("Connected to vega"),
                Err(e) => {
                    eprintln!("Failed to connect to Vega: {e}");
                    continue;
                }
            }
            async move {
                while let Ok(tx_body) = rx_buffer.recv().await {
                    let tx_hash = tx_body.hash;
                    let tx_from = tx_body.from;
                    if !allowed_addresses.contains(&tx_from) && !is_transmit_call(&tx_body) {
                        continue;
                    }
                    let (tx_new_price, forward_to) = match get_price_from_input(&tx_body.input) {
                        Ok((price, to)) => (price, to),
                        Err(e) => {
                            eprintln!("INVALID PRICE UPDATE: failed to get price from input: {e}");
                            continue;
                        }
                    };
                    let bundle = PriceUpdateBundle {
                        trace_id: format!("{:?}", &tx_hash)[2..10].to_string(),
                        tx_new_price,
                        forward_to,
                        tx_to: tx_body.to.expect("This tx didn't define a TO address"),
                        tx_from,
                        tx_input: tx_body.input,
                    };
                    let message_bundle = MessageBundle::PriceUpdate(bundle.clone());
                    let serialized_bundle = bincode::serialize(&message_bundle)
                        .expect("message bundle serialization failed");
                    vega_socket
                        .send(&serialized_bundle, 0)
                        .expect("failed to send bundle");
                    eprintln!("Price update sent to Vega");
                    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    info!(
                        "[{}] - {} - {} - https://etherscan.io/tx/{:?}",
                        now,
                        bundle.trace_id,
                        (provider_clone.get_block_number().await.unwrap() + 1),
                        tx_hash
                    );
                }
                vega_socket.disconnect(VEGA_INBOUND_ENDPOINT).unwrap();
            }
        });

        tokio::select! {
            _ = receiver_handle => eprintln!("Receiver handle ended. This should NEVER happen during operation."),
            _ = processor_handle => eprintln!("Processor handle ended. This should NEVER happen during operation."),
        }

        sleep(Duration::from_secs(SECONDS_BEFORE_RECONNECTION)).await;
    }
}
