use alloy::{
    eips::BlockNumberOrTag,
    hex,
    network::TransactionBuilder,
    node_bindings::anvil::{Anvil, AnvilInstance},
    providers::{IpcConnect, Provider, ProviderBuilder, RootProvider},
    rpc::types::{Block, BlockId, BlockTransactionsKind, TransactionRequest},
};
use eyre::Result;
use overlord_shared_types::PriceUpdateBundle;
use std::{fs::File, panic, sync::Arc};
use tracing::{error, info, warn};
use uuid::Uuid;

////////////////////////////////////////////////////
//
//  Fork provider section begins here
//
////////////////////////////////////////////////////

type AnvilForkProvider = Result<
    RootProvider<alloy::pubsub::PubSubFrontend>,
    alloy::transports::RpcError<alloy::transports::TransportErrorKind>,
>;

struct IpcForkFile {
    path: String,
}

impl IpcForkFile {
    fn new(path: String) -> Self {
        File::create(&path).expect("Failed to create IPC fork file");
        IpcForkFile { path }
    }
}

impl Drop for IpcForkFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).expect("Failed to remove IPC fork file");
    }
}

async fn build_pub_tx(
    bundle: &PriceUpdateBundle,
    forked_block: Block,
    nonce: u64,
) -> TransactionRequest {
    let base_fee = forked_block.header.base_fee_per_gas.unwrap();
    let max_priority_fee = 1_000_000_000u128; // 1 gwei
    let max_fee = base_fee as u128 * 2u128 + max_priority_fee;
    TransactionRequest::default()
        .to(bundle.tx_to)
        .from(bundle.tx_from)
        .with_input(bundle.tx_input.clone())
        .nonce(nonce)
        .gas_limit(forked_block.header.gas_limit)
        .max_fee_per_gas(max_fee)
        .max_priority_fee_per_gas(max_priority_fee)
}

pub struct ForkProvider {
    // The instance of Anvil that is running the fork
    pub _anvil_instance: AnvilInstance,

    // The provider for that instance
    pub fork_provider: AnvilForkProvider,

    // The provider for the fork
    _fork_file: Arc<IpcForkFile>,
}

impl Drop for ForkProvider {
    fn drop(&mut self) {
        if let Err(e) = self._anvil_instance.child_mut().kill() {
            error!("Failed to kill AnvilInstance: {:?}", e);
        }
        if let Err(e) = self._anvil_instance.child_mut().wait() {
            error!("Failed to wait for AnvilInstance to terminate: {:?}", e);
        }
    }
}

impl ForkProvider {
    pub async fn new(bundle: Option<&PriceUpdateBundle>) -> Result<ForkProvider, String> {
        let (_anvil_instance, fork_provider, _fork_file) =
            ForkProvider::spin_up_fork(bundle).await?;
        Ok(ForkProvider {
            _anvil_instance,
            fork_provider,
            _fork_file,
        })
    }

    async fn spin_up_fork(
        bundle: Option<&PriceUpdateBundle>,
    ) -> Result<(AnvilInstance, AnvilForkProvider, Arc<IpcForkFile>), String> {
        // Step 0: Get a provider for the main chain
        let ipc_url = "/tmp/reth.ipc";
        let ipc = IpcConnect::new(ipc_url.to_string());
        let provider = match ProviderBuilder::new().on_ipc(ipc).await {
            Ok(provider) => provider,
            Err(e) => {
                warn!("Failed to connect to IPC: {:?}", e);
                return Err("Failed to connect to IPC".to_string());
            }
        };
        // Step 1: Get the block number we will fork from
        let latest_block_id = BlockId::Number(BlockNumberOrTag::Latest);
        let block_to_be_forked = match provider
            .get_block(latest_block_id, BlockTransactionsKind::Hashes)
            .await
            .unwrap()
        {
            Some(block) => block,
            None => {
                warn!("Failed to get block for forking");
                return Err("Failed to get block for forking".to_string());
            }
        };
        let block_number_to_be_forked = block_to_be_forked.header.number;
        // Step 2: Spin up the anvil fork at the given block
        let fork_path = format!("./fork_{}.ipc", hex::encode(Uuid::new_v4().as_bytes()));
        let ipc_fork_file = Arc::new(IpcForkFile::new(fork_path.clone()));
        let result = panic::catch_unwind(|| {
            Anvil::new()
                .fork(ipc_url)
                .fork_block_number(block_number_to_be_forked)
                .block_time(1_u64)
                .args(vec![
                    "--ipc".to_string(),
                    fork_path.clone(),
                    "--auto-impersonate".to_string(),
                ])
                .spawn()
        });
        let anvil = match result {
            Ok(anvil) => anvil,
            Err(e) => {
                error!("Anvil creation panicked: {:?}", e);
                return Err("Failed to create Anvil instance".to_string());
            }
        };
        info!(
            "Anvil fork started at block: {:?}",
            block_number_to_be_forked
        );
        // Step 3: Get a provider from the fork
        let fork_ipc = IpcConnect::new(fork_path.to_string());
        let fork_provider: AnvilForkProvider = match ProviderBuilder::new().on_ipc(fork_ipc).await {
            Ok(provider) => Ok(provider),
            Err(e) => {
                error!("Failed to connect to fork IPC: {:?}", e);
                return Err("Failed to connect to fork IPC".to_string());
            }
        };
        // Step 4: Apply the price update to the fork
        if let Some(bundle) = bundle {
            // Step 4.1: Build the price update tx
            let nonce = provider
                .get_transaction_count(bundle.tx_from)
                .await
                .unwrap();
            let forked_block_clone = block_to_be_forked.clone();
            let pub_tx = build_pub_tx(bundle, forked_block_clone, nonce).await;
            // Step 4.2: Send it to the fork
            let pending_tx = match fork_provider
                .as_ref()
                .unwrap()
                .send_transaction(pub_tx)
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    error!("Failed to send price update tx to fork: {:?}", e);
                    return Err("Failed to send price update tx to fork".to_string());
                }
            };
            // Step 4.3: Get the price updat tx receipt
            let tx_receipt = match pending_tx.get_receipt().await {
                Ok(receipt) => receipt,
                Err(e) => {
                    error!("Failed to get receipt for price update tx: {:?}", e);
                    return Err("Failed to get receipt for price update tx".to_string());
                }
            };
            // Step 4.4: Validate a new block was mined on the fork
            let new_block_number = match fork_provider.as_ref().unwrap().get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    error!(
                        "Failed to get block number after applying price update tx: {:?}",
                        e
                    );
                    return Err(
                        "Failed to get block number after applying price update tx".to_string()
                    );
                }
            };
            info!(
                trace_id = %bundle.trace_id,
                block_number = %new_block_number,
                receipt = ?tx_receipt,
                "Applied bundle tx receipt"
            );
        }
        // Step 5: Return the fork provider with the new state
        Ok((anvil, fork_provider, ipc_fork_file))
    }
}

////////////////////////////////////////////////////
//
//  Fork provider section ends here
//
////////////////////////////////////////////////////
