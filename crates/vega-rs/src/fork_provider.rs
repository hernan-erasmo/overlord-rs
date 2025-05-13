use alloy::{
    eips::BlockNumberOrTag,
    network::TransactionBuilder,
    node_bindings::anvil::{
        Anvil,
        AnvilInstance
    }, primitives::{
        FixedBytes,
        keccak256,
        U256
    }, providers::{
        ext::AnvilApi,
        IpcConnect,
        Provider,
        ProviderBuilder,
        RootProvider
    }, pubsub::PubSubFrontend,
    rpc::types::{
        Block,
        BlockId,
        BlockTransactionsKind,
        TransactionRequest
    }
};
use eyre::Result;
use overlord_shared::{
    PriceUpdateBundle,
    sol_bindings::AccessControlledOCR2Aggregator,
};
use std::{fs::File, panic, sync::Arc};
use tracing::{error, info, warn};

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

async fn get_storage_key_for_price_update(
    provider: RootProvider<PubSubFrontend>,
    bundle: &PriceUpdateBundle,
) -> Result<U256, Box<dyn std::error::Error>> {
    // How I got to this number?
    //
    // 1. forge install https://github.com/smartcontractkit/libocr
    // 2. that should include a `lib` folder. If you run `forge inspect ./lib/libocr/contract2/ORC2Aggregator.sol storage-layout`
    //    you should get a table with lots of details, including:
    //
    // ╭-------------------------------+-------------------------------------------------------+------+--------+-------+--------------------------------------------------------╮
    // | Name                          | Type                                                  | Slot | Offset | Bytes | Contract                                               |
    // +========================================================================================================================================================================+
    // | s_transmissions               | mapping(uint32 => struct OCR2Aggregator.Transmission) | 12   | 0      | 32    | lib/libocr/contract2/OCR2Aggregator.sol:OCR2Aggregator |
    // ╰-------------------------------+-------------------------------------------------------+------+--------+-------+--------------------------------------------------------╯
    //
    const S_TRANSMISSIONS_SLOT: u8 = 12;
    let round_id = match AccessControlledOCR2Aggregator::new(bundle.forward_to, provider).latestRound().call().await {
        Ok(latestRound) => latestRound._0,
        Err(e) => return Err(format!("Failed to get latestRound from aggregator {}: {}", bundle.forward_to, e).into())
    };

    // Convert round_id to U256 and left-pad it to 32 bytes
    let padded_key = U256::from(round_id);

    // Convert S_TRANSMISSIONS_SLOT to U256 and left-pad it to 32 bytes
    let padded_slot = U256::from(S_TRANSMISSIONS_SLOT);

    // Calculate the storage key using keccak256(abi.encode(key, slot))
    // This is the Solidity mapping key calculation formula: keccak256(abi.encode(key, slot))
    // We need to concatenate the padded key and slot as bytes
    // First, convert them to 32-byte arrays
    let mut combined = [0u8; 64];

    padded_key.to_be_bytes_vec().iter().enumerate().for_each(|(i, b)| {
        if i < 32 {
            combined[i] = *b;
        }
    });

    padded_slot.to_be_bytes_vec().iter().enumerate().for_each(|(i, b)| {
        if i < 32 {
            combined[i + 32] = *b;
        }
    });

    // Hash the combined bytes
    let hashed = keccak256(&combined);

    // Convert the hash to a U256
    Ok(U256::from_be_slice(hashed.as_slice()))
}

/// Encode the payload for setting the storage slot.
///
/// The payload is a packed struct with the following fields:
/// - int192 answer
/// - uint32 observationsTimestamp
/// - uint32 transmissionTimestamp
///
/// Expected:  0x680d728f680d727a000000000000000000000000000000000000000005f5d6e3
///              └─ts1──┘└──ts2─┘                                 └───answer────┘
///
fn get_payload_for_price_update(bundle: &PriceUpdateBundle) -> FixedBytes<32> {
    // We just need a timestamp, the actual value doesn't really matter
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;

    // Create a 32-byte array to hold our result
    let mut bytes = [0u8; 32];

    // Put timestamps at the beginning (bytes 0-7)
    // First timestamp (bytes 0-3)
    bytes[0..4].copy_from_slice(&timestamp.to_be_bytes());
    // Second timestamp (bytes 4-7)
    bytes[4..8].copy_from_slice(&timestamp.to_be_bytes());

    // The price (answer) needs to be in the last 24 bytes (bytes 8-31)
    // Get the price as bytes - U256 uses 32 bytes, but we need just 24 bytes
    let price_bytes: [u8; 32] = bundle.tx_new_price.to_be_bytes_vec().try_into().unwrap_or([0; 32]);

    // We need to copy the last 24 bytes of the price (32-byte value)
    // This preserves the big-endian representation while truncating to 24 bytes
    bytes[8..32].copy_from_slice(&price_bytes[8..32]);

    // Convert to FixedBytes<32>
    FixedBytes::from_slice(&bytes)
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
        info!("Dropping ForkProvider");
        Self::cleanup_anvil_instance(&mut self._anvil_instance);
    }
}

impl ForkProvider {
    fn cleanup_anvil_instance(anvil: &mut AnvilInstance) {
        info!("Cleaning up anvil instance");
        if let Err(e) = anvil.child_mut().kill() {
            error!("Failed to kill AnvilInstance: {:?}", e);
        }
        if let Err(e) = anvil.child_mut().wait() {
            error!("Failed to wait for AnvilInstance to terminate: {:?}", e);
        }
        info!("Anvil instance killed");
    }

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
        let trace_id = bundle
            .map(|b| b.trace_id.to_string())
            .unwrap_or_else(|| "NO_TRACE_ID".to_string());
        let provider = match ProviderBuilder::new().on_ipc(ipc).await {
            Ok(provider) => provider,
            Err(e) => {
                warn!("Failed to connect to IPC for bundle {}: {:?}", trace_id, e);
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
                warn!("Failed to get block for forking trace id {}", trace_id);
                return Err("Failed to get block for forking".to_string());
            }
        };
        let block_number_to_be_forked = block_to_be_forked.header.number;
        // Step 2: Spin up the anvil fork at the given block
        // Any error raised after this line must properly close the anvil process
        // or it will become a zombie
        let fork_path = format!("./fork_{}.ipc", trace_id,);
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
        let mut anvil = match result {
            Ok(anvil) => anvil,
            Err(e) => {
                error!("Anvil creation panicked for bundle {}: {:?}", trace_id, e);
                return Err("Failed to create Anvil instance".to_string());
            }
        };
        info!(
            "Anvil fork started at block {:?} for bundle {}",
            block_number_to_be_forked, trace_id,
        );
        // Step 3: Get a provider from the fork
        let fork_ipc = IpcConnect::new(fork_path.to_string());
        let fork_provider: AnvilForkProvider = match ProviderBuilder::new().on_ipc(fork_ipc).await {
            Ok(provider) => Ok(provider),
            Err(e) => {
                error!(
                    "Failed to connect to fork IPC for bundle {}: {:?}",
                    trace_id, e
                );
                Self::cleanup_anvil_instance(&mut anvil);
                return Err("Failed to connect to fork IPC".to_string());
            }
        };
        // Step 4: Apply the price update to the fork
        if let Some(bundle) = bundle {
            // Step 4.0: Fund the account the tx comes from, to prevent failures when applying the tx
            let storage_key = get_storage_key_for_price_update(provider.clone(), &bundle.clone()).await.unwrap();
            let storage_value = get_payload_for_price_update(&bundle.clone());
            info!("About to call anvil_setStorageAt({}, {:?}, {:?})", bundle.forward_to, storage_key, storage_value);
            match fork_provider
                .as_ref()
                .unwrap()
                // The way price updates work, is that some address submits
                // a transaction that calls the forward() method on a contract.
                // That forward() method receives 2 args: `to_address` and `data`
                // In anvil_setStorageAt, the first argument is that `to_address`,
                // that we receive from the bundle in the `forward_to` attribute
                .anvil_set_storage_at(
                    bundle.forward_to,
                    storage_key,
                    storage_value,
                )
                .await
            {
                Ok(_) => {
                    info!("Successfuly set storage for bundle {}", trace_id);
                }
                Err(e) => {
                    error!(
                        "Failed to set storage for bundle {}: {:?}",
                        trace_id, e
                    );
                    Self::cleanup_anvil_instance(&mut anvil);
                    return Err("Failed to set storage".to_string());
                }
            }
            info!("Storage in fork for bundle {} has been tweaked", trace_id);
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
