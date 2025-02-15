use alloy::{
    providers::{IpcConnect, ProviderBuilder, RootProvider},
    pubsub::PubSubFrontend,
};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tokio::sync::Mutex;

static PROVIDER: OnceCell<Arc<RootProvider<PubSubFrontend>>> = OnceCell::new();

#[derive(Clone)]
pub struct ProviderCache {
    initialization: Arc<Mutex<()>>,
}

impl Default for ProviderCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderCache {
    pub fn new() -> Self {
        Self {
            initialization: Arc::new(Mutex::new(())),
        }
    }

    pub async fn get_provider(
        &self,
    ) -> Result<Arc<RootProvider<PubSubFrontend>>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(provider) = PROVIDER.get() {
            return Ok(provider.clone());
        }
        let _lock = self.initialization.lock().await;

        if let Some(provider) = PROVIDER.get() {
            return Ok(provider.clone());
        }

        let ipc = IpcConnect::new("/tmp/reth.ipc".to_string());
        let provider = ProviderBuilder::new().on_ipc(ipc).await?;
        let provider = Arc::new(provider);

        PROVIDER
            .set(provider.clone())
            .map_err(|_| "Failed to SET provider on cache")?;
        Ok(provider)
    }
}
