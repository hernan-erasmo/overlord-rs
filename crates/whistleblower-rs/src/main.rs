use alloy::{
    providers::{IpcConnect, Provider, ProviderBuilder},
    sol,
};
use alloy_primitives::keccak256;
use alloy::rpc::types::{Filter, Log};
use futures_util::{stream::select_all, StreamExt};
use overlord_shared_types::{
    MessageBundle,
    WhistleblowerUpdate,
    WhistleblowerEventDetails,
    WhistleblowerEventType,
};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    AAVE_V3_POOL,
    "src/abi/aave_v3_pool.json",
);

fn send_whistleblower_update(log: &Log, event_details: &WhistleblowerEventDetails, socket: &zmq::Socket) {
    let event_update = WhistleblowerUpdate {
        trace_id: log.transaction_hash.as_ref().map_or("".to_string(), |tx_hash| hex::encode(&tx_hash.0)[2..10].to_string()),
        block_number: log.block_number.unwrap_or_default(),
        event_details: event_details.clone(),
    };
    let message_bundle = MessageBundle::WhistleblowerNotification(event_update);
    let serialized_update = bincode::serialize(&message_bundle).expect("Whistleblower update serialization failed");
    socket
        .send(&serialized_update, 0)
        .expect("Failed to send Whistleblower update");
    eprintln!("Whistleblower {:?} update sent to Vega", event_details.event);
}

#[tokio::main]
async fn main() {
    eprintln!("Starting whistleblower-rs");
    let vega_context = zmq::Context::new();
    let vega_socket = vega_context.socket(zmq::PUSH).unwrap();
    vega_socket
        .connect("ipc:///tmp/vega_inbound")
        .expect("Failed to connect to Vega");
    eprintln!("Connected to vega");

    let ipc_url = "/tmp/reth.ipc";

    let liquidation_call_signature = keccak256("LiquidationCall(address,address,address,uint256,uint256,address,bool)".as_bytes());
    let borrow_signature = keccak256("Borrow(address,address,address,uint256,uint8,uint256,uint16)".as_bytes());
    let supply_signature = keccak256("Supply(address,address,address,uint256,uint16)".as_bytes());
    let repay_signature = keccak256("Repay(address,address,address,uint256,bool)".as_bytes());

    loop {
        let ipc = IpcConnect::new(ipc_url.to_string());
        let provider = match ProviderBuilder::new().on_ipc(ipc).await {
            Ok(provider) => Arc::new(provider),
            Err(e) => {
                eprintln!("Failed to connect to IPC: {e}. Retrying in 5 seconds...");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let liquidation_call_sub = match provider.subscribe_logs(&Filter::new().event_signature(liquidation_call_signature)).await {
            Ok(liquidation_call_sub) => liquidation_call_sub,
            Err(e) => {
                eprintln!("Failed to subscribe to liquidation calls: {e}. Retrying in 5 seconds...");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let borrow_sub = match provider.subscribe_logs(&Filter::new().event_signature(borrow_signature)).await {
            Ok(borrow_sub) => borrow_sub,
            Err(e) => {
                eprintln!("Failed to subscribe to borrow events: {e}. Retrying in 5 seconds...");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let supply_sub = match provider.subscribe_logs(&Filter::new().event_signature(supply_signature)).await {
            Ok(supply_sub) => supply_sub,
            Err(e) => {
                eprintln!("Failed to subscribe to supply events: {e}. Retrying in 5 seconds...");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let repay_sub = match provider.subscribe_logs(&Filter::new().event_signature(repay_signature)).await {
            Ok(repay_sub) => repay_sub,
            Err(e) => {
                eprintln!("Failed to subscribe to repay events: {e}. Retrying in 5 seconds...");
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut all_event_streams = select_all(vec![liquidation_call_sub.into_stream(), borrow_sub.into_stream(), supply_sub.into_stream(), repay_sub.into_stream()]);
        eprintln!("Listening for interesting transactions...");

        while let Some(log) = all_event_streams.next().await {
            let block_number = log.block_number.unwrap_or_default();
            match log.topics().get(0) {
                Some(topic) if topic == &liquidation_call_signature => {
                    let AAVE_V3_POOL::LiquidationCall { collateralAsset, debtAsset, user, debtToCover, liquidatedCollateralAmount, liquidator, .. } = log.log_decode().expect("Failed to decode LiquidationCall event").inner.data;
                    eprintln!("LIQUIDATION CALL (block: {:?}) - collateralAsset: {}, debtAsset: {}, user: {}, debtToCover: {}, liquidatedCollateralAmount: {}, liquidator: {}", block_number, collateralAsset, debtAsset, user, debtToCover, liquidatedCollateralAmount, liquidator);
                    let event_details = WhistleblowerEventDetails {
                        event: WhistleblowerEventType::LiquidationCall,
                        args: vec![
                            collateralAsset.to_string(),
                            debtAsset.to_string(),
                            user.to_string(),
                            debtToCover.to_string(),
                            liquidatedCollateralAmount.to_string(),
                            liquidator.to_string(),
                        ],
                    };
                    send_whistleblower_update(&log, &event_details, &vega_socket);
                }
                Some(topic) if topic == &borrow_signature => {
                    let AAVE_V3_POOL::Borrow { reserve, onBehalfOf, .. } = log.log_decode().expect("Failed to decode Borrow event").inner.data;
                    eprintln!("BORROW (block: {:?}) - reserve: {}, onBehalfOf: {}", block_number, reserve, onBehalfOf);
                    let event_details = WhistleblowerEventDetails {
                        event: WhistleblowerEventType::Borrow,
                        args: vec![
                            reserve.to_string(),
                            onBehalfOf.to_string(),
                        ],
                    };
                    send_whistleblower_update(&log, &event_details, &vega_socket);
                }
                Some(topic) if topic == &supply_signature => {
                    let AAVE_V3_POOL::Supply { reserve, onBehalfOf, .. } = log.log_decode().expect("Failed to decode Supply event").inner.data;
                    eprintln!("SUPPLY (block: {:?}) - reserve: {}, onBehalfOf: {}", block_number, reserve, onBehalfOf);
                    let event_details = WhistleblowerEventDetails {
                        event: WhistleblowerEventType::Supply,
                        args: vec![
                            reserve.to_string(),
                            onBehalfOf.to_string(),
                        ],
                    };
                    send_whistleblower_update(&log, &event_details, &vega_socket);
                }
                Some(topic) if topic == &repay_signature => {
                    let AAVE_V3_POOL::Repay { reserve, user, .. } = log.log_decode().expect("Failed to decode Repay event").inner.data;
                    eprintln!("REPAY (block: {:?}) - reserve: {}, user: {}", block_number, reserve, user);
                    let event_details = WhistleblowerEventDetails {
                        event: WhistleblowerEventType::Repay,
                        args: vec![
                            reserve.to_string(),
                            user.to_string(),
                        ],
                    };
                    send_whistleblower_update(&log, &event_details, &vega_socket);
                }
                _ => {
                    eprintln!("Unknown event or empty log topics detected: {:?}", log);
                }
            }
        };
        eprintln!("Stream closed. Reconnecting in 5 seconds...");
        sleep(Duration::from_secs(5)).await;
    }
}
