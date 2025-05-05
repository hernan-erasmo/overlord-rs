use alloy::{
    hex, primitives::{Address, Bytes, U256}, providers::{IpcConnect, Provider, ProviderBuilder, RootProvider}, pubsub::{PubSubFrontend, Subscription}, rpc::{client::ClientBuilder, types::Transaction}, sol_types::SolCall
};
use ethers_core::abi::{decode, ParamType};
use mev_share_sse::{client::EventStream, Event as MevShareEvent, EventClient};
use futures_util::StreamExt;
use lru::LruCache;
use overlord_shared::{
    common::get_reserves_data,
    constants::GHO_PRICE_ORACLE,
    sol_bindings::{
        AccessControlledOCR2Aggregator, AuthorizedForwarder, EACAggregatorProxy, IUiPoolDataProviderV3::AggregatedReserveData
    },
    MessageBundle,
    NewPrice,
    PriceUpdateBundle
};

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    num::NonZeroUsize,
    sync::Arc,
};

use tokio::{
    sync::broadcast,
    time::{sleep, Duration},
};
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};
use futures::stream::FuturesUnordered;

mod sol_bindings;
use sol_bindings::{forwardCall, transmitCall, transmitSecondaryCall, ForwardToDestination};

mod resolvers;
use resolvers::resolve_aggregator;

const IPC_URL: &str = "/tmp/reth.ipc";
const MEV_SHARE_MAINNET_SSE_URL: &str = "https://mev-share.flashbots.net";
const SECONDS_BEFORE_RECONNECTION: u64 = 2;
const VEGA_INBOUND_ENDPOINT: &str = "ipc:///tmp/vega_inbound";
const OOPS_PRICE_CACHE_SIZE: usize = 10;

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
fn get_price_from_input(tx_input: &Bytes) -> Result<NewPrice, Box<dyn Error>> {
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

    Ok(
        NewPrice {
            price: answer,
            chainlink_address: forward_calldata.to,
        }
    )
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

async fn task_get_transmitters(
    provider_clone: Arc<RootProvider<PubSubFrontend>>,
    price_oracle: Address,
    symbol: String,
) -> Result<Option<Vec<Address>>, Box<dyn Error + Send + Sync>> {
    info!("Resolving aggregator for {}", &symbol);

    // First get the aggregator address
    // This can be anywhere from 1 to 3 or 4 RPC calls
    let addr = match resolve_aggregator(provider_clone.clone(), price_oracle).await {
        Ok(addr) => addr,
        Err(e) => return Err(format!("resolve_aggregator() call failed for {}: {}", symbol, e).into())
    };

    if addr == GHO_PRICE_ORACLE {
        return Ok(Some(vec![]));
    }

    // Then get the actual aggregator from the proxy
    // 1 RPC call
    let agg_address = match EACAggregatorProxy::new(addr, provider_clone.clone()).aggregator().call().await {
        Ok(agg) => agg._0,
        Err(e) => return Err(format!("Couldn't get aggregator() from address {} (price oracle: {}): {}",
                                    addr, price_oracle, e).into())
    };

    let transmitters = match AccessControlledOCR2Aggregator::new(agg_address, provider_clone).getTransmitters().call().await {
        Ok(response) => response._0,
        Err(e) => {
            return Err(format!("Couldn't get transmitters from aggregator {}: {}", agg_address, e).into())
        }
    };

    // Return the transmitters
    Ok(Some(transmitters))
}

/// Get authorized senders from a transmitter address
async fn task_get_authorized_senders(
    provider: Arc<RootProvider<PubSubFrontend>>,
    transmitter: Address
) -> Result<Vec<Address>, Box<dyn Error + Send + Sync>> {
    info!("Getting authorized senders from transmitter {:?}", transmitter);
    match AuthorizedForwarder::new(transmitter, provider).getAuthorizedSenders().call().await {
        Ok(result) => Ok(result._0),
        Err(e) => Err(format!("Failed to get authorized senders from transmitter {:?}: {}", transmitter, e).into())
    }
}

/// Get the list of addresses that we will listen to for new price updates
///
/// 1. Collect all values from each item in AAVE_V3_UI_POOL_DATA's `getReservesData(AAVE_V3_PROVIDER_ADDRESS)`
/// 2. The "priceOracle" attributes are contracts which are either EACAggregatorInterface, or
///    some subtype of it, but eventually resolve to an EACAggregatorInterface and, more importantly,
///    to something that implements a `getTransmitters()` function
/// 3. Call `getTransmitters()` on each of these contracts and collect the addresses. Remove
///    duplicates and return the list. Those are all the addresses authorized to send
///    price updates to relevant assets, and those are the only ones we need to listen to.
async fn collect_transmitters(
    provider: Arc<RootProvider<PubSubFrontend>>,
) -> Result<Vec<Address>, Box<dyn Error>> {
    // one RPC call
    let reserves = match get_reserves_data(provider.clone()).await {
        Ok(response) => response,
        Err(e) => return Err(format!("Error fetching reserves data in collect_transmitters(): {}", e).into())
    };

    // Create a HashMap that maps price oracles to their respective reserve data
    let mut reserves_mapping: HashMap<Address, AggregatedReserveData> = HashMap::new();
    for reserve in reserves.iter().cloned() {
        reserves_mapping.insert(reserve.priceOracle, reserve);
    }

    info!("Resolving {} aggregators concurrently", reserves.len());

    let mut aggregator_tasks = FuturesUnordered::new();

    // Start all aggregator resolution tasks
    for reserve in reserves.iter() {
        let provider_clone = provider.clone();
        let price_oracle = reserve.priceOracle;
        let symbol = reserve.symbol.clone();

        // Spawn each task into the FuturesUnordered collection
        aggregator_tasks.push(task_get_transmitters(
            provider_clone,
            price_oracle,
            symbol,
        ));
    }

    // Track unique transmitters to avoid duplicate calls
    let mut unique_transmitters = HashSet::new();
    let mut sender_tasks = FuturesUnordered::new();
    let mut collected_authorized_forwarders = Vec::new();

    // Process aggregator tasks as they complete
    while let Some(result) = aggregator_tasks.next().await {
        match result {
            Ok(Some(transmitters)) => {
                for transmitter in transmitters {
                    // Skip if we've already processed this transmitter
                    if unique_transmitters.insert(transmitter) {
                        // Only process new/unique transmitters
                        let provider_clone = provider.clone();
                        sender_tasks.push(task_get_authorized_senders(provider_clone, transmitter));
                    } else {
                        info!("Authorized senders for transmitter {} already accounted for", transmitter);
                    }
                }
            },
            Ok(None) => {
                // This was a GHO_PRICE_ORACLE, skip it
                continue;
            },
            Err(e) => return Err(e)
        }
    }

    // Process all the sender tasks
    info!("Processing {} unique transmitters to get authorized senders", sender_tasks.len());
    while let Some(result) = sender_tasks.next().await {
        match result {
            Ok(senders) => {
                collected_authorized_forwarders.extend(senders);
            },
            Err(e) => {
                // Log error but continue with other transmitters
                error!("Error getting authorized senders: {}", e);
            }
        }
    }
    // Remove duplicates from collected authorized forwarders
    let unique_forwarders: HashSet<_> = collected_authorized_forwarders.drain(..).collect();
    collected_authorized_forwarders = unique_forwarders.into_iter().collect();
    Ok(collected_authorized_forwarders)
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

/// Create a new provider to connect to the Ethereum node
async fn create_provider() -> Result<RootProvider<PubSubFrontend>, Box<dyn Error>> {
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
    Ok(provider)
}

/// Create a mempool stream to listen for pending transactions
async fn create_subscription_stream(
    provider: &RootProvider<PubSubFrontend>,
) -> Result<Subscription<Transaction>, Box<dyn Error>> {
    let stream = match provider.subscribe_full_pending_transactions().await {
        Ok(stream) => stream,
        Err(e) => return Err(Box::new(e))
    };
    Ok(stream)
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

    let new_price_cache: LruCache<NewPrice, ()> = LruCache::new(NonZeroUsize::new(OOPS_PRICE_CACHE_SIZE).unwrap());
    info!("Price cache initialized with size: {}", OOPS_PRICE_CACHE_SIZE);

    let provider = match create_provider().await {
        Ok(provider) => provider,
        Err(e) => {
            error!("Failed to create provider: {e}");
            std::process::exit(1);
        }
    };

    let transmitters = match collect_transmitters(Arc::new(provider.clone())).await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to collect transmitters: {e}");
            std::process::exit(1);
        }
    };
    info!("Transmitters we would listen to for price updates: {:?}", transmitters);

    loop {
        let provider = provider.clone();
        // Outer loop to restart IPC on major connection issues
        let mut mempool_tx_stream = match create_subscription_stream(&provider).await {
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
            let allowed_addresses = transmitters.clone();
            let provider_clone = provider.clone();
            let vega_context = zmq::Context::new();
            let vega_socket = vega_context.socket(zmq::PUSH).unwrap();
            let mut new_price_cache = new_price_cache.clone();
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
                            let new_price = match get_price_from_input(&tx_body.input) {
                                Ok(new_price) => new_price,
                                Err(e) => {
                                    error!("MEMPOOL INVALID PRICE UPDATE: failed to get price from input: {e}");
                                    continue;
                                }
                            };
                            if new_price_cache.get(&new_price).is_some() {
                                info!(
                                    message = "Ignoring cached MEMPOOL update.",
                                    trace_id = %format!("{:?}", tx_hash)[2..10],
                                    tx_hash = %format!("{:?}", tx_hash),
                                    price = %new_price.price,
                                    forward_to = %new_price.chainlink_address,
                                );
                                continue;
                            } else {
                                new_price_cache.put(new_price.clone(), ());
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
                            let raw_tx = match provider_clone.get_raw_transaction_by_hash(tx_hash).await {
                                Ok(raw_tx) => raw_tx,
                                Err(e) => {
                                    error!("Failed to get mempool raw transaction by hash: {e}");
                                    continue;
                                }
                            };
                            let bundle = PriceUpdateBundle {
                                tx_hash: format!("{:?}", &tx_hash).to_string(),
                                raw_tx: raw_tx,
                                inclusion_block: format!("{}", &expected_block).to_string(),
                                trace_id: format!("{:?}", &tx_hash)[2..10].to_string(),
                                tx_new_price: new_price.price,
                                forward_to: new_price.chainlink_address,
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
                                price = %format!("{:?}", new_price.price),
                                forward_to = %new_price.chainlink_address,
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
                                    let new_price = match get_price_from_input(&tx.calldata.unwrap()) {
                                        Ok(new_price) => new_price,
                                        Err(e) => {
                                            error!("INVALID PRICE UPDATE for mev-share: failed to get price from input: {e}");
                                            continue;
                                        }
                                    };
                                    if new_price_cache.get(&new_price).is_some() {
                                        info!(
                                            message = "Ignoring cached MEVSHRE update.",
                                            trace_id = %format!("{:?}", event.hash)[2..10].to_string(),
                                            tx_hash = %format!("{:?}", event.hash).to_string(),
                                            price = %new_price.price,
                                            forward_to = %new_price.chainlink_address,
                                        );
                                        continue;
                                    } else {
                                        new_price_cache.put(new_price.clone(), ());
                                    };
                                    let tx_from = match get_tx_sender_from_contract(provider.clone(), new_price.chainlink_address).await {
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
                                        raw_tx: None, // I believe that we can pass the hash if it's a mevshare update
                                        trace_id: format!("{:?}", event.hash)[2..10].to_string(),
                                        inclusion_block: format!("{}", &expected_block).to_string(),
                                        tx_new_price: new_price.price,
                                        forward_to: new_price.chainlink_address, // vega uses this to know which asset(s) the update is for
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
                                        price = %format!("{:?}", new_price.price),
                                        forward_to = %new_price.chainlink_address,
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
