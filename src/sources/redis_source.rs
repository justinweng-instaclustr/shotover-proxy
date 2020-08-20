use crate::transforms::chain::TransformChain;

use crate::config::topology::TopicHolder;
use crate::protocols::redis_codec::RedisCodec;
use crate::server::TcpCodecListener;
use crate::sources::{Sources, SourcesFromConfig};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc, Semaphore};
use tokio::task::JoinHandle;
use tracing::info;

use anyhow::Result;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RedisConfig {
    pub listen_addr: String,
}

#[async_trait]
impl SourcesFromConfig for RedisConfig {
    async fn get_source(
        &self,
        chain: &TransformChain,
        _topics: &mut TopicHolder,
        notify_shutdown: broadcast::Sender<()>,
        shutdown_complete_tx: mpsc::Sender<()>,
    ) -> Result<Vec<Sources>> {
        Ok(vec![Sources::Redis(
            RedisSource::new(
                chain,
                self.listen_addr.clone(),
                notify_shutdown,
                shutdown_complete_tx,
            )
            .await,
        )])
    }
}

#[derive(Debug)]
pub struct RedisSource {
    pub name: &'static str,
    pub join_handle: JoinHandle<Result<()>>,
    pub listen_addr: String,
}

impl RedisSource {
    //"127.0.0.1:9043
    pub async fn new(
        chain: &TransformChain,
        listen_addr: String,
        notify_shutdown: broadcast::Sender<()>,
        shutdown_complete_tx: mpsc::Sender<()>,
    ) -> RedisSource {
        let listener = TcpListener::bind(listen_addr.clone()).await.unwrap();

        info!("Starting Redis source on [{}]", listen_addr);
        let name = "Redis Source";

        let mut listener = TcpCodecListener {
            chain: chain.clone(),
            source_name: name.to_string(),
            listener,
            codec: RedisCodec::new(false),
            limit_connections: Arc::new(Semaphore::new(50)),
            notify_shutdown,
            shutdown_complete_tx,
        };

        let jh = Handle::current().spawn(async move {
            listener.run().await?;

            let TcpCodecListener {
                notify_shutdown,
                shutdown_complete_tx,
                ..
            } = listener;

            drop(shutdown_complete_tx);
            drop(notify_shutdown);

            // let _ shutd

            Ok(())
        });

        RedisSource {
            name,
            join_handle: jh,
            listen_addr: listen_addr.clone(),
        }
    }
}
