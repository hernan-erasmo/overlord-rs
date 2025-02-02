use overlord_shared_types::UnderwaterUserEvent;
use tracing::{error, info, warn};
use tracing_appender::rolling::{self, Rotation};
use tracing_subscriber::fmt::{time::LocalTime, writer::BoxMakeWriter};

const PROFITO_INBOUND_ENDPOINT: &str = "ipc:///tmp/profito_inbound";

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

    loop { // Outer loop handles reconnection
        info!("Starting Profito RS (outer loop)");
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
        loop { // Inner loop handles recv
            match socket.recv_bytes(0) {
                Ok(bytes) => {
                    match bincode::deserialize::<UnderwaterUserEvent>(&bytes) {
                        Ok(uw_event) => {
                            tokio::spawn(async move {
                                info!("processing candidate | {} | {} | {} |", uw_event.trace_id, uw_event.address, uw_event.user_account_data.totalCollateralBase);
                            });
                        }
                        Err(e) => {
                            warn!("Failed to deserialize message: {e}");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to receive ZMQ message: {e}");
                    continue;
                }
            }
        }
    }
}
