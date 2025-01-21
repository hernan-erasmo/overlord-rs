use alloy::rpc::types::{Filter, Log};
use alloy::{
    primitives::{FixedBytes, U64},
    providers::{IpcConnect, Provider, ProviderBuilder, RootProvider},
    pubsub::{PubSubFrontend, Subscription},
    sol,
};
use alloy_primitives::keccak256;
use futures_util::{stream::select_all, StreamExt};
use overlord_shared_types::{
    MessageBundle, WhistleblowerEventDetails, WhistleblowerEventType, WhistleblowerUpdate,
};
use std::{collections::HashMap, sync::Arc};
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    AAVE_V3_POOL,
    "src/abi/aave_v3_pool.json",
);

#[derive(Debug)]
enum WhistleblowerError {
    ProviderError(String),
    SubscriptionError(String),
    EventProcessingError(String),
}

impl std::error::Error for WhistleblowerError {}
impl std::fmt::Display for WhistleblowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WhistleblowerError::ProviderError(e) => write!(f, "Provider error: {}", e),
            WhistleblowerError::SubscriptionError(e) => write!(f, "Subscription error: {}", e),
            WhistleblowerError::EventProcessingError(e) => {
                write!(f, "Event processing error: {}", e)
            }
        }
    }
}

trait EventProcessor {
    fn process(
        &self,
        log: &Log,
        block_number: U64,
    ) -> Result<WhistleblowerEventDetails, WhistleblowerError>;
}

struct LiquidationCallProcessor;

impl EventProcessor for LiquidationCallProcessor {
    fn process(
        &self,
        log: &Log,
        block_number: U64,
    ) -> Result<WhistleblowerEventDetails, WhistleblowerError> {
        let decoded = log.log_decode().map_err(|e| {
            WhistleblowerError::EventProcessingError(format!(
                "Failed to decode LiquidationCall event: {}",
                e
            ))
        })?;

        let AAVE_V3_POOL::LiquidationCall {
            collateralAsset,
            debtAsset,
            user,
            debtToCover,
            liquidatedCollateralAmount,
            liquidator,
            ..
        } = decoded.inner.data;

        info!(
            block = ?block_number,
            collateral_asset = %collateralAsset,
            debt_asset = %debtAsset,
            user = %user,
            debt_to_cover = %debtToCover,
            liquidated_collateral = %liquidatedCollateralAmount,
            liquidator = %liquidator,
            "LIQUIDATION CALL"
        );

        Ok(WhistleblowerEventDetails {
            event: WhistleblowerEventType::LiquidationCall,
            args: vec![
                collateralAsset.to_string(),
                debtAsset.to_string(),
                user.to_string(),
                debtToCover.to_string(),
                liquidatedCollateralAmount.to_string(),
                liquidator.to_string(),
            ],
        })
    }
}

struct BorrowProcessor;

impl EventProcessor for BorrowProcessor {
    fn process(
        &self,
        log: &Log,
        block_number: U64,
    ) -> Result<WhistleblowerEventDetails, WhistleblowerError> {
        let decoded = log.log_decode().map_err(|e| {
            WhistleblowerError::EventProcessingError(format!(
                "Failed to decode Borrow event: {}",
                e
            ))
        })?;

        let AAVE_V3_POOL::Borrow {
            reserve,
            onBehalfOf,
            ..
        } = decoded.inner.data;

        info!(
            block = ?block_number,
            reserve = %reserve,
            on_behalf_of = %onBehalfOf,
            "BORROW"
        );

        Ok(WhistleblowerEventDetails {
            event: WhistleblowerEventType::Borrow,
            args: vec![reserve.to_string(), onBehalfOf.to_string()],
        })
    }
}

struct SupplyProcessor;

impl EventProcessor for SupplyProcessor {
    fn process(
        &self,
        log: &Log,
        block_number: U64,
    ) -> Result<WhistleblowerEventDetails, WhistleblowerError> {
        let decoded = log.log_decode().map_err(|e| {
            WhistleblowerError::EventProcessingError(format!(
                "Failed to decode Supply event: {}",
                e
            ))
        })?;

        let AAVE_V3_POOL::Supply {
            reserve,
            onBehalfOf,
            ..
        } = decoded.inner.data;

        info!(
            block = ?block_number,
            reserve = %reserve,
            on_behalf_of = %onBehalfOf,
            "SUPPLY"
        );

        Ok(WhistleblowerEventDetails {
            event: WhistleblowerEventType::Supply,
            args: vec![reserve.to_string(), onBehalfOf.to_string()],
        })
    }
}

struct RepayProcessor;

impl EventProcessor for RepayProcessor {
    fn process(
        &self,
        log: &Log,
        block_number: U64,
    ) -> Result<WhistleblowerEventDetails, WhistleblowerError> {
        let decoded = log.log_decode().map_err(|e| {
            WhistleblowerError::EventProcessingError(format!("Failed to decode Repay event: {}", e))
        })?;

        let AAVE_V3_POOL::Repay { reserve, user, .. } = decoded.inner.data;

        info!(
            block = ?block_number,
            reserve = %reserve,
            user = %user,
            "REPAY"
        );

        Ok(WhistleblowerEventDetails {
            event: WhistleblowerEventType::Repay,
            args: vec![reserve.to_string(), user.to_string()],
        })
    }
}

fn send_whistleblower_update(
    log: &Log,
    event_details: &WhistleblowerEventDetails,
    socket: &zmq::Socket,
) {
    let event_update = WhistleblowerUpdate {
        trace_id: log
            .transaction_hash
            .as_ref()
            .map_or("".to_string(), |tx_hash| {
                hex::encode(&tx_hash.0)[2..10].to_string()
            }),
        block_number: log.block_number.unwrap_or_default(),
        event_details: event_details.clone(),
    };
    let message_bundle = MessageBundle::WhistleblowerNotification(event_update);
    let serialized_update = match bincode::serialize(&message_bundle) {
        Ok(update) => update,
        Err(e) => {
            warn!("Failed to serialize Whistleblower update: {}", e);
            return;
        }
    };
    if let Err(e) = socket.send(&serialized_update, 0) {
        warn!("Failed to send Whistleblower update: {}", e);
        return;
    }
    info!(event_type = ?event_details.event, "Whistleblower update sent to Vega");
}

fn _setup_logging() {
    let log_file = rolling::RollingFileAppender::new(
        Rotation::DAILY,
        "/var/log/overlord-rs",
        "whistleblower-rs.log",
    );
    let file_writer = BoxMakeWriter::new(log_file);
    tracing_subscriber::fmt()
        .with_writer(file_writer)
        .with_timer(LocalTime::rfc_3339())
        .with_target(true)
        .init();
}

async fn setup_provider(
    ipc_url: String,
) -> Result<Arc<RootProvider<PubSubFrontend>>, WhistleblowerError> {
    let ipc = IpcConnect::new(ipc_url);
    ProviderBuilder::new()
        .on_ipc(ipc)
        .await
        .map(Arc::new)
        .map_err(|e| {
            error!("Failed to connect to IPC: {}", e);
            WhistleblowerError::ProviderError(e.to_string())
        })
}

async fn setup_subscription(
    provider: Arc<RootProvider<PubSubFrontend>>,
    event_signature: FixedBytes<32>,
    event_name: &str,
) -> Result<Subscription<Log>, WhistleblowerError> {
    provider
        .subscribe_logs(&Filter::new().event_signature(event_signature))
        .await
        .map_err(|e| {
            error!("Failed to subscribe to {} events: {}", event_name, e);
            WhistleblowerError::SubscriptionError(e.to_string())
        })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    _setup_logging();

    info!("Starting whistleblower-rs");
    let vega_context = zmq::Context::new();
    let vega_socket = vega_context.socket(zmq::PUSH).unwrap_or_else(|e| {
        error!("Failed to create ZMQ PUSH socket: {}", e);
        std::process::exit(1);
    });
    if let Err(e) = vega_socket.connect("ipc:///tmp/vega_inbound") {
        error!("Failed to connect to Vega IPC: {}", e);
        std::process::exit(1);
    }
    info!("Connected to vega");

    let ipc_url = "/tmp/reth.ipc";
    let liquidation_call_signature = keccak256(
        "LiquidationCall(address,address,address,uint256,uint256,address,bool)".as_bytes(),
    );
    let borrow_signature =
        keccak256("Borrow(address,address,address,uint256,uint8,uint256,uint16)".as_bytes());
    let supply_signature = keccak256("Supply(address,address,address,uint256,uint16)".as_bytes());
    let repay_signature = keccak256("Repay(address,address,address,uint256,bool)".as_bytes());

    let event_processors: HashMap<FixedBytes<32>, Box<dyn EventProcessor>> = [
        (
            liquidation_call_signature,
            Box::new(LiquidationCallProcessor) as Box<dyn EventProcessor>,
        ),
        (
            borrow_signature,
            Box::new(BorrowProcessor) as Box<dyn EventProcessor>,
        ),
        (
            supply_signature,
            Box::new(SupplyProcessor) as Box<dyn EventProcessor>,
        ),
        (
            repay_signature,
            Box::new(RepayProcessor) as Box<dyn EventProcessor>,
        ),
    ]
    .into();

    loop {
        let provider = setup_provider(ipc_url.to_string()).await?;

        let liquidation_sub =
            setup_subscription(provider.clone(), liquidation_call_signature, "liquidation").await?;

        let borrow_sub = setup_subscription(provider.clone(), borrow_signature, "borrow").await?;

        let supply_sub = setup_subscription(provider.clone(), supply_signature, "supply").await?;

        let repay_sub = setup_subscription(provider.clone(), repay_signature, "repay").await?;

        let mut all_event_streams = select_all(vec![
            liquidation_sub.into_stream(),
            borrow_sub.into_stream(),
            supply_sub.into_stream(),
            repay_sub.into_stream(),
        ]);
        info!("Listening for interesting transactions...");

        while let Some(log) = all_event_streams.next().await {
            let block_number = U64::from(log.block_number.unwrap_or_default());
            if let Some(event_signature) = log.topics().get(0) {
                if let Some(event_processor) = event_processors.get(event_signature) {
                    match event_processor.process(&log, block_number) {
                        Ok(event_details) => {
                            send_whistleblower_update(&log, &event_details, &vega_socket);
                        }
                        Err(e) => {
                            warn!("Failed to process event: {}", e);
                        }
                    }
                } else {
                    warn!("Unknown event or empty log topics detected: {:?}", log);
                }
            } else {
                warn!("Empty log topics detected: {:?}", log);
            }
        }
        warn!("Stream closed. Reconnecting in 5 seconds...");
        sleep(Duration::from_secs(5)).await;
    }
}
