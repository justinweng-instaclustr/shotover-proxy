use atomic_enum::atomic_enum;
use kafka_protocol::messages::BrokerId;
use kafka_protocol::protocol::StrBytes;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ShotoverNodeConfig {
    pub address: String,
    pub rack: String,
    pub broker_id: i32,
}

impl ShotoverNodeConfig {
    pub(crate) fn build(self) -> anyhow::Result<ShotoverNode> {
        Ok(ShotoverNode {
            address: self.address.parse::<SocketAddr>()?,
            rack: StrBytes::from_string(self.rack),
            broker_id: BrokerId(self.broker_id),
            state: Arc::new(AtomicShotoverNodeState::new(ShotoverNodeState::Up)),
        })
    }
}

#[derive(Clone)]
pub(crate) struct ShotoverNode {
    pub address: SocketAddr,
    pub rack: StrBytes,
    pub broker_id: BrokerId,
    #[allow(unused)]
    state: Arc<AtomicShotoverNodeState>,
}

impl ShotoverNode {
    #![allow(unused)]
    pub(crate) fn is_up(&self) -> bool {
        self.state.load(Ordering::Relaxed) == ShotoverNodeState::Up
    }

    pub(crate) fn set_state(&self, state: ShotoverNodeState) {
        self.state.store(state, Ordering::Relaxed)
    }
}

#[atomic_enum]
#[derive(PartialEq)]
pub(crate) enum ShotoverNodeState {
    Up,
    Down,
}

pub(crate) fn start_shotover_peers_check(
    shotover_peers: Vec<ShotoverNode>,
    check_shotover_peers_delay_ms: u64,
    connect_timeout_ms: u64,
) {
    tokio::spawn(async move {
        loop {
            check_shotover_peers(
                &shotover_peers,
                check_shotover_peers_delay_ms,
                connect_timeout_ms,
            )
            .await;
        }
    });
}

async fn check_shotover_peers(
    shotover_peers: &[ShotoverNode],
    check_shotover_peers_delay_ms: u64,
    connect_timeout_ms: u64,
) {
    let mut shotover_peers_cycle = shotover_peers.iter().cycle();
    loop {
        if let Some(shotover_peer) = shotover_peers_cycle.next() {
            let is_up = TcpStream::connect_timeout(
                &shotover_peer.address,
                Duration::from_millis(connect_timeout_ms),
            )
            .is_ok();
            shotover_peer.set_state(if is_up {
                ShotoverNodeState::Up
            } else {
                tracing::warn!("Shotover peer {} is down", shotover_peer.address);
                ShotoverNodeState::Down
            });

            sleep(Duration::from_millis(check_shotover_peers_delay_ms)).await;
        }
    }
}
