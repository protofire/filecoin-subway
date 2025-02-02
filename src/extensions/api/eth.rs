use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use jsonrpsee::core::JsonValue;
use serde::Deserialize;
use tokio::{sync::watch, task::JoinHandle};

use crate::extensions::{
    api::{BaseApi, ValueHandle},
    client::Client,
    Extension, ExtensionRegistry,
};

pub struct EthApi {
    inner: BaseApi,
    stale_timeout: Duration,
    background_tasks: Vec<JoinHandle<()>>,
}

impl Drop for EthApi {
    fn drop(&mut self) {
        self.background_tasks.drain(..).for_each(|handle| handle.abort());
    }
}

#[derive(Deserialize, Debug)]
pub struct EthApiConfig {
    pub stale_timeout_seconds: u64,
}

#[async_trait]
impl Extension for EthApi {
    type Config = EthApiConfig;

    async fn from_config(config: &Self::Config, registry: &ExtensionRegistry) -> Result<Self, anyhow::Error> {
        let client = registry.get::<Client>().await.expect("Client not found");

        Ok(Self::new(client, Duration::from_secs(config.stale_timeout_seconds)))
    }
}

impl EthApi {
    pub fn new(client: Arc<Client>, stale_timeout: Duration) -> Self {
        let (head_tx, head_rx) = watch::channel::<Option<(JsonValue, u64)>>(None);
        let (_finalized_head_tx, finalized_head_rx) = watch::channel::<Option<(JsonValue, u64)>>(None);

        let mut this = Self {
            inner: BaseApi::new(head_rx, finalized_head_rx),
            stale_timeout,
            background_tasks: Vec::new(),
        };

        this.start_background_task(client, head_tx, _finalized_head_tx);

        this
    }

    pub fn get_head(&self) -> ValueHandle<(JsonValue, u64)> {
        self.inner.get_head()
    }

    pub fn get_finalized_head(&self) -> ValueHandle<(JsonValue, u64)> {
        self.inner.get_finalized_head()
    }

    pub fn current_head(&self) -> Option<(JsonValue, u64)> {
        self.inner.head_rx.borrow().to_owned()
    }

    pub fn current_finalized_head(&self) -> Option<(JsonValue, u64)> {
        self.inner.finalized_head_rx.borrow().to_owned()
    }

    fn start_background_task(
        &mut self,
        client: Arc<Client>,
        head_tx: watch::Sender<Option<(JsonValue, u64)>>,
        _finalized_head_tx: watch::Sender<Option<(JsonValue, u64)>>,
    ) {
        let stale_timeout = self.stale_timeout;

        let client2 = client.clone();
        self.background_tasks.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(stale_timeout);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            let client = client2.clone();

            loop {
                let run = async {
                    // query current head
                    let head = client
                        .request("eth_getBlockByNumber", vec!["latest".into(), true.into()])
                        .await?;
                    let number = super::get_number(&head)?;
                    let hash = super::get_hash(&head)?;

                    tracing::debug!("New head: {number} {hash}");
                    head_tx.send_replace(Some((hash, number)));

                    let mut sub = client
                        .subscribe("eth_subscribe", ["newHeads".into()].into(), "eth_unsubscribe")
                        .await?;

                    // Reset the interval
                    interval.reset();

                    loop {
                        tokio::select! {
                            val = sub.next() => {
                                if let Some(Ok(val)) = val {
                                    interval.reset();

                                    let number = super::get_number(&val)?;
                                    let hash = super::get_hash(&val)?;

                                    tracing::debug!("New head: {number} {hash}");
                                    head_tx.send_replace(Some((hash, number)));
                                } else {
                                    break;
                                }
                            }
                            _ = interval.tick() => {
                                tracing::warn!("No new blocks for {stale_timeout:?} seconds, rotating endpoint");
                                client.rotate_endpoint().await;
                                break;
                            }
                            _ = client.on_rotation() => {
                                // endpoint is rotated, break the loop and restart subscription
                                break;
                            }
                        }
                    }

                    Ok::<(), anyhow::Error>(())
                };

                if let Err(e) = run.await {
                    tracing::error!("Error in background task: {e}");
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }));
    }
}
