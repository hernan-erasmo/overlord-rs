use alloy::{
    hex, primitives::{Address, Bytes, U256}, providers::{IpcConnect, Provider, ProviderBuilder, RootProvider}, pubsub::{PubSubFrontend, Subscription}, rpc::{client::ClientBuilder, types::{Transaction}}, sol, sol_types::SolCall
};
use ethers_core::abi::{decode, ParamType};
use mev_share_sse::{client::EventStream, Event as MevShareEvent, EventClient};
use futures_util::StreamExt;
use overlord_shared_types::{MessageBundle, PriceUpdateBundle};

use std::{
    error::Error,
    fs::File,
    io::{self, BufRead},
    path::Path,
    str::FromStr,
};
use tokio::{
    sync::broadcast,
    time::{sleep, Duration},
};
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};

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

    #[allow(missing_docs)]
    function transmitSecondary(
        bytes32[3] calldata reportContext,
        bytes calldata report,
        bytes32[] calldata rs,
        bytes32[] calldata ss,
        bytes32 rawVs
    ) external override;

    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    contract ForwardToDestination {
        function transmitters() external view returns (address[] memory);
        function getTransmitters() external view returns (address[] memory);
    }
);

const IPC_URL: &str = "/tmp/reth.ipc";
const MEV_SHARE_MAINNET_SSE_URL: &str = "https://mev-share.flashbots.net";
const SECONDS_BEFORE_RECONNECTION: u64 = 2;
const PATH_TO_ADDRESSES_INPUT: &str = "crates/oops-rs/addresses.txt";
const VEGA_INBOUND_ENDPOINT: &str = "ipc:///tmp/vega_inbound";

struct ProcessingHandles {
    mempool: tokio::task::JoinHandle<()>,
    mevshare: tokio::task::JoinHandle<()>,
    processor: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
enum PendingTxType {
    FromMempool(Transaction),
    FromMevShare(MevShareEvent),
}

fn is_transmit_secondary(calldata: Option<Bytes>) -> bool {
    calldata.as_ref().map_or(false, |data| data.windows(4).any(|w| w == [0xba, 0x0c, 0xb2, 0x9e]))
}

async fn get_tx_sender_from_contract(
    provider: RootProvider<PubSubFrontend>,
    forward_to_contract: Address,
) -> Result<Address, Box<dyn Error>> {
    info!("Attempting to get transmitters from {:?}", forward_to_contract);
    let forward_contract = ForwardToDestination::new(forward_to_contract, provider.clone());
    let transmitters = match forward_contract.getTransmitters().call().await {
        Ok(transmitters) => transmitters._0,
        Err(_) => {
            match forward_contract.transmitters().call().await {
                Ok(transmitters) => transmitters._0,
                Err(e) => return Err(format!("Failed call to getTransmitters() AND transmitters(): {e}").into())
            }
        }
    };
    if transmitters.is_empty() {
        return Err("No transmitters found".into());
    }
    transmitters.first().cloned().ok_or_else(|| "No transmitters found".into())
}

/// Extract the new price from the input data of a transaction
fn get_price_from_input(tx_input: &Bytes) -> Result<(U256, Address), Box<dyn Error>> {
    // get `data` from forward(address to, bytes calldata data)
    let forward_calldata = match forwardCall::abi_decode(tx_input, false) {
        Ok(data) => data,
        Err(e) => return Err(Box::new(e)),
    };
    let forward_data = forward_calldata.data;

    // get `report` from
    // transmit( or tansmitSecondary(
    //   bytes32[3] calldata reportContext,
    //   bytes calldata report,
    //   bytes32[] calldata rs,
    //   bytes32[] calldata ss,
    //   bytes32 rawVs
    // )
    let transmit_report = match transmitCall::abi_decode(&forward_data, false) {
        Ok(data) => data.report,
        Err(e1) => {
            // If transmit fails, try transmitSecondary
            match transmitSecondaryCall::abi_decode(&forward_data, false) {
                Ok(data) => data.report,
                Err(e2) => {
                    error!("Failed to decode both transmit calls: \ntransmit: {e1}\ntransmitSecondary: {e2}");
                    return Err(Box::new(e2));
                }
            }
        }
    };

    // this is what the function _decodeReport(bytes memory rawReport) of OCR2Aggregator.sol does
    let decoded_transmit_report = match decode(
        &[
            ParamType::Uint(32),                             // observationsTimestamp
            ParamType::FixedBytes(32),                       // rawObservers
            ParamType::Array(Box::new(ParamType::Int(192))), // observations
            ParamType::Int(192),                             // juelsPerFeeCoin
        ],
        &transmit_report,
    ) {
        Ok(decoded) => decoded,
        Err(e) => {
            error!("Failed to decode transmit report: {}", e);
            return Err(Box::new(e));
        }
    };

    let observations = decoded_transmit_report[2].clone().into_array().unwrap();
    let median = &observations[observations.len() / 2];
    let answer = U256::from_str_radix(&median.to_string(), 16).unwrap();

    Ok((answer, forward_calldata.to))
}

/// This function reads the file of addresses we identified as senders of
/// new price updates, so that we can filter pending transactions coming from these.
fn read_addresses_from_file(filename: &str) -> io::Result<Vec<alloy::primitives::Address>> {
    let path = Path::new(filename);
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut addresses = Vec::new();
    for line in reader.lines() {
        let line = line?;
        match Address::from_str(line.trim()) {
            Ok(address) => addresses.push(address),
            Err(e) => {
                error!("Failed to parse address from line '{}': {}", line, e);
                continue;
            }
        };
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
    let selector_chunk = tx_input.get(100..132).unwrap_or_else(|| {
        warn!(
            "INVALID TRANSMIT: looked valid, but tx_input length ({}) is too short. tx_hash was {}",
            tx_input.len(),
            tx_body.hash
        );
        &[]
    });
    selector_chunk.starts_with(&transmit_selector)
}

/// Get the list of addresses thah we will listen to for new price updates
fn _init_addresses(file_path: String) -> Result<Vec<Address>, Box<dyn Error>> {
    let allowed_addresses = match read_addresses_from_file(&file_path) {
        Ok(addresses) => addresses,
        Err(e) => {
            error!("Failed to read addresses from file: {}", e);
            return Err(Box::new(e));
        }
    };
    info!(
        "Addresses to listen for price updates: [{}]",
        allowed_addresses
            .iter()
            .map(|addr| format!("{addr:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(allowed_addresses)
}

fn _setup_logging() {
    let log_file =
        rolling::RollingFileAppender::new(Rotation::DAILY, "/var/log/overlord-rs", "oops-rs.log");
    let file_writer = BoxMakeWriter::new(log_file);
    tracing_subscriber::fmt()
        .with_writer(file_writer)
        .with_timer(LocalTime::rfc_3339())
        .with_target(true)
        .init();
}

fn get_slot_information() -> (f32, f32) {
    const SLOT_DURATION: f32 = 12.0;

    // Get current time with subsecond precision
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();

    // Convert to total seconds and nanoseconds
    let total_seconds = now.as_secs();
    let nanos = now.subsec_nanos() as f32 / 1_000_000_000.0;

    // Get seconds within the current minute and add nanoseconds
    let seconds_in_minute = (total_seconds % 60) as f32 + nanos;

    // Calculate seconds within the current slot
    let captured_at = seconds_in_minute % SLOT_DURATION;

    // Calculate remaining time in slot
    let remaining = SLOT_DURATION - captured_at;

    // Round to 1 decimal place
    let captured_at = (captured_at * 10.0).round() / 10.0;
    let remaining = (remaining * 10.0).round() / 10.0;

    (captured_at, remaining)
}

/// Create a mempool stream to listen for pending transactions
/// Returns the subscrition on which to await, and the provider to query the block number or whatever
async fn create_mempool_stream() -> Result<(Subscription<Transaction>, RootProvider<PubSubFrontend>), Box<dyn Error>> {
    let ipc = IpcConnect::new(IPC_URL.to_string());
    let client = match ClientBuilder::default().ipc(ipc).await {
        Ok(client) => {
            client.set_channel_size(2048);
            client
        }
        Err(e) => {
            error!(
                "Failed to connect to IPC: {e}. Retrying in {} seconds...",
                SECONDS_BEFORE_RECONNECTION
            );
            sleep(Duration::from_secs(SECONDS_BEFORE_RECONNECTION)).await;
            return Err(Box::new(e));
        }
    };
    let provider = ProviderBuilder::new().on_client(client);
    let stream = match provider.subscribe_full_pending_transactions().await {
        Ok(stream) => stream,
        Err(e) => return Err(Box::new(e))
    };
    Ok((stream, provider))
}

async fn create_mev_share_stream() -> Result<EventStream<mev_share_sse::Event>, Box<dyn Error>> {
    let client = EventClient::default();
    let stream = match client.events(MEV_SHARE_MAINNET_SSE_URL).await {
        Ok(stream) => stream,
        Err(e) => {
            return Err(Box::new(e));
        }
    };
    Ok(stream)
}

#[tokio::main]
async fn main() {
    _setup_logging();

    let allowed_addresses = match _init_addresses(String::from(PATH_TO_ADDRESSES_INPUT)) {
        Ok(addresses) => addresses,
        Err(e) => {
            error!("Failed to initialize addresses: {}", e);
            std::process::exit(1);
        }
    };

    loop {
        // Outer loop to restart IPC on major connection issues
        let (mut mempool_tx_stream, provider) = match create_mempool_stream().await {
            Ok(stream) => stream,
            Err(e) => {
                error!(
                    "Failed to subscribe to mempool transactions: {e}. Retrying in {} seconds...",
                    SECONDS_BEFORE_RECONNECTION
                );
                sleep(Duration::from_secs(SECONDS_BEFORE_RECONNECTION)).await;
                continue;
            }
        };
        let mut mev_share_tx_stream = match create_mev_share_stream().await {
            Ok(stream) => stream,
            Err(e) => {
                error!(
                    "Failed to create mev-share stream: {e}. Retrying in {} seconds...",
                    SECONDS_BEFORE_RECONNECTION
                );
                sleep(Duration::from_secs(SECONDS_BEFORE_RECONNECTION)).await;
                continue;
            }
        };

        let (tx_buffer, mut rx_buffer) = broadcast::channel::<PendingTxType>(2048);
        let tx_buffer_for_mempool = tx_buffer.clone();
        let tx_buffer_for_mev_share = tx_buffer.clone();

        let mempool_receiver_handle = tokio::spawn(async move {
            loop {
                match mempool_tx_stream.recv().await {
                    Ok(tx_body) => {
                        if let Err(e) = tx_buffer_for_mempool.send(PendingTxType::FromMempool(tx_body)) {
                            error!("Failed to send tx to buffer from mempool receiver: {e}");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Unknow stream enqueue error on mempool receiver: {e}");
                        break;
                    }
                }
            }
        });

        let mev_share_receiver_handle = tokio::spawn(async move {
            while let Some(event) = mev_share_tx_stream.next().await {
                match event {
                    Ok(event) => {
                        if event.transactions.is_empty() {
                            continue;
                        };
                        if let Err(e) = tx_buffer_for_mev_share.send(PendingTxType::FromMevShare(event)) {
                            error!("Failed to send tx to buffer from mev-share receiver: {e}");
                            break;
                        };
                    }
                    Err(e) => {
                        error!("Unknow stream enqueue error on mev-share receiver: {e}");
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
                Ok(_) => info!("Connected to vega inbound endpoint"),
                Err(e) => {
                    error!("Failed to connect to Vega: {e}");
                    continue;
                }
            }
            async move {
                while let Ok(tx_body) = rx_buffer.recv().await {
                    match tx_body {
                        PendingTxType::FromMempool(tx_body) => {
                            let tx_hash = tx_body.hash;
                            let tx_from = tx_body.from;
                            if !is_transmit_call(&tx_body) {
                                continue;
                            }
                            if !allowed_addresses.contains(&tx_from) {
                                warn!(
                                    message = "Found mempool valid transmit() call from non-tracked address",
                                    tx_from = %format!("{:?}", tx_from),
                                    tx_hash = %format!("{:?}", tx_hash),
                                    slot_info = %format!("{:?}", get_slot_information()),
                                );
                                continue;
                            }
                            let (tx_new_price, forward_to) = match get_price_from_input(&tx_body.input) {
                                Ok((price, to)) => (price, to),
                                Err(e) => {
                                    error!("MEMPOOL INVALID PRICE UPDATE: failed to get price from input: {e}");
                                    continue;
                                }
                            };
                            let expected_block = match provider_clone.get_block_number().await {
                                // When reading the block, the provider is going to return the last submitted block
                                // that it's aware of, meaning that a pending tx is expected to land on block + 1
                                // at the earliest
                                Ok(block) => block + 1,
                                Err(e) => {
                                    warn!("Failed to get mempool block number: {e}");
                                    u64::MIN
                                }
                            };
                            let bundle = PriceUpdateBundle {
                                tx_hash: format!("{:?}", &tx_hash).to_string(),
                                inclusion_block: format!("{}", &expected_block).to_string(),
                                trace_id: format!("{:?}", &tx_hash)[2..10].to_string(),
                                tx_new_price,
                                forward_to,
                                tx_to: tx_body.to.expect("This mempool tx didn't define a TO address"),
                                tx_from,
                                tx_input: tx_body.input,
                            };
                            let message_bundle = MessageBundle::PriceUpdate(bundle.clone());
                            let serialized_bundle = match bincode::serialize(&message_bundle) {
                                Ok(bundle) => bundle,
                                Err(e) => {
                                    error!("Failed to serialize mempool message bundle: {e}");
                                    continue;
                                }
                            };
                            match vega_socket.send(&serialized_bundle, 0) {
                                Ok(_) => (),
                                Err(e) => {
                                    error!("Failed to send mempool bundle to Vega: {e}");
                                    continue;
                                }
                            };
                            info!(
                                message = "MEMPOOL update sent.",
                                trace_id = %bundle.trace_id,
                                expected_block = %expected_block,
                                tx_hash = %format!("{:?}", tx_hash),
                                slot_info = %format!("{:?}", get_slot_information()),
                            );
                        }
                        PendingTxType::FromMevShare(event) => {
                            for tx in event.transactions {
                                if tx.to.is_none() {
                                    // MevShare tx doesn't define 'to' field. Nothing to do.
                                    continue;
                                };
                                if tx.function_selector.is_none() {
                                    // MevShare tx doesn't define function selector. Nothing to do.
                                    continue;
                                }
                                if tx.function_selector.as_ref().map_or(true, |selector| selector != &[0x6f, 0xad, 0xcf, 0x72]) {
                                    // Mevshare event function selector doesn't match forward()
                                    continue;
                                }
                                if tx.calldata.is_none() {
                                    // No calldata available for this mevshare event. Nothing to do
                                    continue;
                                }
                                let tx_calldata = tx.calldata.clone();
                                if is_transmit_secondary(tx.calldata.clone()) {
                                    let (tx_new_price, forward_to) = match get_price_from_input(&tx.calldata.unwrap()) {
                                        Ok((price, to)) => (price, to),
                                        Err(e) => {
                                            error!("INVALID PRICE UPDATE for mev-share: failed to get price from input: {e}");
                                            continue;
                                        }
                                    };
                                    let tx_from = match get_tx_sender_from_contract(provider.clone(), forward_to).await {
                                        Ok(from) => from,
                                        Err(e) => {
                                            error!("Failed to get mevshare tx_from from contract: {e}");
                                            continue;
                                        }
                                    };
                                    let expected_block = match provider_clone.get_block_number().await {
                                        // When reading the block, the provider is going to return the last submitted block
                                        // that it's aware of, meaning that a pending tx is expected to land on block + 1
                                        // at the earliest
                                        Ok(block) => block + 1,
                                        Err(e) => {
                                            warn!("Failed to get block number: {e}");
                                            u64::MIN
                                        }
                                    };
                                    let bundle = PriceUpdateBundle {
                                        tx_hash: format!("{:?}", event.hash).to_string(),
                                        trace_id: format!("{:?}", event.hash)[2..10].to_string(),
                                        inclusion_block: format!("{}", &expected_block).to_string(),
                                        tx_new_price,
                                        forward_to, // vega uses this to know which asset(s) the update is for
                                        tx_to: tx.to.unwrap(),
                                        tx_from,
                                        tx_input: tx_calldata.unwrap(),
                                    };
                                    let message_bundle = MessageBundle::PriceUpdate(bundle.clone());
                                    let serialized_bundle = match bincode::serialize(&message_bundle) {
                                        Ok(bundle) => bundle,
                                        Err(e) => {
                                            error!("Failed to serialize message bundle: {e}");
                                            continue;
                                        }
                                    };
                                    match vega_socket.send(&serialized_bundle, 0) {
                                        Ok(_) => (),
                                        Err(e) => {
                                            error!("Failed to send bundle to Vega: {e}");
                                            continue;
                                        }
                                    };
                                    info!(
                                        message = "MEVSHRE update sent.",
                                        trace_id = %bundle.trace_id,
                                        expected_block = %expected_block,
                                        tx_hash = %format!("{:?}", event.hash),
                                        slot_info = %format!("{:?}", get_slot_information()),
                                    );
                                }
                            }
                        }
                    }
                }
                match vega_socket.disconnect(VEGA_INBOUND_ENDPOINT) {
                    Ok(_) => (),
                    Err(e) => warn!("Failed to disconnect from vega inbound socket: {e}"),
                };
            }
        });

        let mut handles = ProcessingHandles {
            mempool: mempool_receiver_handle,
            mevshare: mev_share_receiver_handle,
            processor: processor_handle,
        };

        tokio::select! {
            _ = &mut handles.mempool => error!("Mempool receiver handle ended unexpectedly. Restarting all handlers"),
            _ = &mut handles.mevshare => error!("MevShare receiver handle ended unexpectedly. Restarting all handlers"),
            _ = &mut handles.processor => error!("Processor handle ended unexpectedly. Restarting all handlers"),
        };

        info!("tokio::select finished. Aborting all handlers");
        handles.mempool.abort();
        handles.mevshare.abort();
        handles.processor.abort();
        info!("Handlers ended. Restarting all handlers");

        sleep(Duration::from_secs(SECONDS_BEFORE_RECONNECTION)).await;
    }
}
