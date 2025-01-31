use alloy::sol;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};

const PROFITO_INBOUND_ENDPOINT: &str = "ipc:///tmp/profito_inbound";
const CHANNEL_CAPACITY: usize = 1000;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(serde::Deserialize, Debug)]
    #[sol(rpc)]
    AaveV3Pool,
    "src/abis/aave_v3_pool.json"
);

fn _setup_logging() {
    let log_file = rolling::RollingFileAppender::new(
        Rotation::DAILY,
        "/var/log/overlord-rs",
        "profito-rs.log",
    );
    let file_writer = BoxMakeWriter::new(log_file);
    tracing_subscriber::fmt()
        .with_writer(file_writer)
        .with_timer(LocalTime::rfc_3339())
        .with_target(true)
        .init();
}

#[tokio::main]
async fn main() {
    _setup_logging();

    let (tx_buffer, mut rx_buffer) =
        mpsc::channel::<AaveV3Pool::getUserAccountDataReturn>(CHANNEL_CAPACITY);

    let receiver_handle = tokio::spawn(async move {
        let context = zmq::Context::new();
        let socket = context.socket(zmq::PULL).unwrap();
        if let Err(e) = socket.bind(PROFITO_INBOUND_ENDPOINT) {
            error!("Failed to bind ZMQ socket: {e}");
            return;
        }
        info!(
            "Listening for health factor alerts on {}",
            PROFITO_INBOUND_ENDPOINT
        );
        loop {
            match socket.recv_bytes(0) {
                Ok(bytes) => {
                    match bincode::deserialize::<AaveV3Pool::getUserAccountDataReturn>(&bytes) {
                        Ok(user_account_data) => {
                            if let Err(e) = tx_buffer.send(user_account_data).await {
                                warn!("Failed to send alert to buffer: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Failed to deserialize message: {e}");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to receive ZMQ message: {e}");
                    continue;
                }
            }
        }
    });
    let processor_handle = tokio::spawn(async move {
        while let Some(user_account_data) = rx_buffer.recv().await {
            info!("processing message from vega-rs: {:?}", user_account_data);
        }
    });
    tokio::select! {
        _ = receiver_handle => error!("Receiver handle ended unexpectedly"),
        _ = processor_handle => error!("Processor handle ended unexpectedly"),
    }
}
