use crate::codec::{kafka::KafkaCodecBuilder, CodecBuilder, Direction};
use crate::frame::kafka::{strbytes, KafkaFrame, RequestBody, ResponseBody};
use crate::frame::Frame;
use crate::message::{Message, Messages};
use crate::tcp;
use crate::transforms::util::cluster_connection_pool::{spawn_read_write_tasks, Connection};
use crate::transforms::util::{Request, Response};
use crate::transforms::{Transform, TransformBuilder, Transforms, Wrapper};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kafka_protocol::protocol::StrBytes;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use serde::Deserialize;
use std::collections::HashMap;
use std::hash::Hasher;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

#[derive(Deserialize, Debug)]
pub struct KafkaSinkClusterConfig {
    pub first_contact_points: Vec<String>,
    pub shotover_nodes: Vec<String>,
    pub connect_timeout_ms: u64,
    pub read_timeout: Option<u64>,
}

#[cfg(feature = "alpha-transforms")]
use crate::transforms::TransformConfig;

use super::common::produce_channel;

#[cfg(feature = "alpha-transforms")]
#[typetag::deserialize(name = "KafkaSinkCluster")]
#[async_trait(?Send)]
impl TransformConfig for KafkaSinkClusterConfig {
    async fn get_builder(&self, chain_name: String) -> Result<Box<dyn TransformBuilder>> {
        Ok(Box::new(KafkaSinkClusterBuilder::new(
            self.first_contact_points.clone(),
            self.shotover_nodes.clone(),
            chain_name,
            self.connect_timeout_ms,
            self.read_timeout,
        )))
    }
}

pub struct KafkaSinkClusterBuilder {
    // contains address and port
    first_contact_points: Vec<String>,
    shotover_nodes: Vec<KafkaAddress>,
    connect_timeout: Duration,
    read_timeout: Option<Duration>,
}

impl KafkaSinkClusterBuilder {
    pub fn new(
        first_contact_points: Vec<String>,
        shotover_nodes: Vec<String>,
        _chain_name: String,
        connect_timeout_ms: u64,
        timeout: Option<u64>,
    ) -> KafkaSinkClusterBuilder {
        let receive_timeout = timeout.map(Duration::from_secs);

        let shotover_nodes = shotover_nodes
            .into_iter()
            .map(|node| {
                let address: SocketAddr = node.parse().unwrap();
                KafkaAddress {
                    host: strbytes(&address.ip().to_string()),
                    port: address.port() as i32,
                }
            })
            .collect();

        KafkaSinkClusterBuilder {
            first_contact_points,
            shotover_nodes,
            connect_timeout: Duration::from_millis(connect_timeout_ms),
            read_timeout: receive_timeout,
        }
    }
}

impl TransformBuilder for KafkaSinkClusterBuilder {
    fn build(&self) -> Transforms {
        Transforms::KafkaSinkCluster(KafkaSinkCluster {
            first_contact_points: self.first_contact_points.clone(),
            shotover_nodes: self.shotover_nodes.clone(),
            pushed_messages_tx: None,
            connect_timeout: self.connect_timeout,
            read_timeout: self.read_timeout,
            nodes: vec![],
            topics: HashMap::new(),
            rng: SmallRng::from_rng(rand::thread_rng()).unwrap(),
        })
    }

    fn get_name(&self) -> &'static str {
        "KafkaClusterSink"
    }

    fn is_terminating(&self) -> bool {
        true
    }
}

pub struct KafkaSinkCluster {
    first_contact_points: Vec<String>,
    shotover_nodes: Vec<KafkaAddress>,
    pushed_messages_tx: Option<mpsc::UnboundedSender<Messages>>,
    connect_timeout: Duration,
    read_timeout: Option<Duration>,
    nodes: Vec<KafkaNode>,
    topics: HashMap<StrBytes, Topic>,
    rng: SmallRng,
}

#[async_trait]
impl Transform for KafkaSinkCluster {
    async fn transform<'a>(&'a mut self, requests_wrapper: Wrapper<'a>) -> Result<Messages> {
        if self.nodes.is_empty() {
            self.nodes = self
                .first_contact_points
                .iter()
                .map(|address| {
                    let address = address.parse().unwrap();
                    KafkaNode {
                        connection: None,
                        address,
                        kafka_address: KafkaAddress {
                            host: strbytes(&address.ip().to_string()),
                            port: address.port() as i32,
                        },
                        broker_id: -1,
                    }
                })
                .collect();
        }

        let responses = self.send_requests(requests_wrapper.requests).await?;

        // TODO: since kafka will never send requests out of order I wonder if it would be faster to use an mpsc instead of a oneshot or maybe just directly run the sending/receiving here?
        let mut responses = if let Some(read_timeout) = self.read_timeout {
            timeout(read_timeout, read_responses(responses)).await?
        } else {
            read_responses(responses).await
        }?;

        // Rewrite responses to use shotovers port instead of kafkas port
        for response in &mut responses {
            match response.frame() {
                Some(Frame::Kafka(KafkaFrame::Response {
                    body: ResponseBody::FindCoordinator(find_coordinator),
                    version,
                    ..
                })) => {
                    if *version <= 3 {
                        rewrite_address(
                            &self.shotover_nodes,
                            &mut find_coordinator.host,
                            &mut find_coordinator.port,
                        )
                    } else {
                        for coordinator in &mut find_coordinator.coordinators {
                            rewrite_address(
                                &self.shotover_nodes,
                                &mut coordinator.host,
                                &mut coordinator.port,
                            )
                        }
                    }
                    response.invalidate_cache();
                }
                Some(Frame::Kafka(KafkaFrame::Response {
                    body: ResponseBody::Metadata(metadata),
                    ..
                })) => {
                    for broker in &mut metadata.brokers {
                        let mut found = false;
                        for node in &mut self.nodes {
                            if broker.1.host == node.kafka_address.host
                                && broker.1.port == node.kafka_address.port
                            {
                                found = true;
                                node.broker_id = **broker.0;
                            }
                        }
                        if !found {
                            let host = broker.1.host.clone();
                            let port = broker.1.port;
                            self.nodes.push(KafkaNode {
                                broker_id: **broker.0,
                                address: format!("{host}:{port}").parse().map_err(|_| {
                                    anyhow!(
                                        "Failed to parse address from kafka message: {host}:{port}"
                                    )
                                })?,
                                kafka_address: KafkaAddress { host, port },
                                connection: None,
                            });
                        }
                        rewrite_address(
                            &self.shotover_nodes,
                            &mut broker.1.host,
                            &mut broker.1.port,
                        )
                    }

                    for topic in &metadata.topics {
                        self.topics.insert(
                            topic.0.clone().0,
                            Topic {
                                partitions: topic
                                    .1
                                    .partitions
                                    .iter()
                                    .map(|partition| Partition {
                                        leader_id: *partition.leader_id,
                                        _replica_nodes: partition
                                            .replica_nodes
                                            .iter()
                                            .map(|x| x.0)
                                            .collect(),
                                    })
                                    .collect(),
                            },
                        );
                    }
                    response.invalidate_cache();
                }
                Some(Frame::Kafka(KafkaFrame::Response {
                    body: ResponseBody::DescribeCluster(describe_cluster),
                    ..
                })) => {
                    for broker in &mut describe_cluster.brokers {
                        rewrite_address(
                            &self.shotover_nodes,
                            &mut broker.1.host,
                            &mut broker.1.port,
                        )
                    }
                    response.invalidate_cache();
                }
                _ => {}
            }
        }

        Ok(responses)
    }

    fn set_pushed_messages_tx(&mut self, pushed_messages_tx: mpsc::UnboundedSender<Messages>) {
        self.pushed_messages_tx = Some(pushed_messages_tx);
    }
}

impl KafkaSinkCluster {
    async fn send_requests(
        &mut self,
        requests: Vec<Message>,
    ) -> Result<Vec<oneshot::Receiver<Response>>> {
        let mut results = Vec::with_capacity(requests.len());

        for mut message in requests {
            match message.frame() {
                Some(Frame::Kafka(KafkaFrame::Request {
                    body: RequestBody::Produce(produce),
                    ..
                })) => {
                    let mut connection = None;
                    // assume that all topics in this message have the same routing requirements
                    let (topic_name, topic_data) = produce
                        .topic_data
                        .iter()
                        .next()
                        .ok_or_else(|| anyhow!("No topics in produce message"))?;
                    if let Some(topic) = self.topics.get(&topic_name.0) {
                        // assume that all partitions in this topic have the same routing requirements
                        let partition = &topic.partitions[topic_data
                            .partition_data
                            .first()
                            .ok_or_else(|| anyhow!("No partitions in topic"))?
                            .index
                            as usize];
                        for node in &mut self.nodes {
                            if node.broker_id == partition.leader_id {
                                connection =
                                    Some(node.get_connection(self.connect_timeout).await?.clone());
                            }
                        }
                    }
                    let connection = match connection {
                        Some(connection) => connection,
                        None => self
                            .nodes
                            .choose_mut(&mut self.rng)
                            .unwrap()
                            .get_connection(self.connect_timeout)
                            .await?
                            .clone(),
                    };

                    let (return_chan, rx) = produce_channel(produce);

                    connection
                        .send(Request {
                            message,
                            return_chan,
                        })
                        .map_err(|_| anyhow!("Failed to send"))?;
                    results.push(rx);
                }
                _ => {
                    let connection = self
                        .nodes
                        .choose_mut(&mut self.rng)
                        .unwrap()
                        .get_connection(self.connect_timeout)
                        .await?;
                    let (tx, rx) = oneshot::channel();
                    connection
                        .send(Request {
                            message,
                            return_chan: Some(tx),
                        })
                        .map_err(|_| anyhow!("Failed to send"))?;
                    results.push(rx);
                }
            }
        }
        Ok(results)
    }
}

async fn read_responses(responses: Vec<oneshot::Receiver<Response>>) -> Result<Messages> {
    let mut result = Vec::with_capacity(responses.len());
    for response in responses {
        result.push(response.await.unwrap().response?);
    }
    Ok(result)
}

fn hash_address(host: &str, port: i32) -> u64 {
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    hasher.write(host.as_bytes());
    hasher.write(&port.to_be_bytes());
    hasher.finish()
}

fn rewrite_address(shotover_nodes: &[KafkaAddress], host: &mut StrBytes, port: &mut i32) {
    // do not attempt to rewrite if the port is not provided (-1)
    // this is known to occur in an error response
    if *port >= 0 {
        let shotover_node =
            &shotover_nodes[hash_address(host, *port) as usize % shotover_nodes.len()];
        *host = shotover_node.host.clone();
        *port = shotover_node.port;
    }
}

struct KafkaNode {
    broker_id: i32,
    address: SocketAddr,
    // Same address as `address` the duplication makes it fast to compare against addresses in kafka messages.
    kafka_address: KafkaAddress,
    connection: Option<Connection>,
}

impl KafkaNode {
    async fn get_connection(&mut self, connect_timeout: Duration) -> Result<&Connection> {
        if self.connection.is_none() {
            let codec = KafkaCodecBuilder::new(Direction::Sink);
            let tcp_stream = tcp::tcp_stream(connect_timeout, &self.address).await?;
            let (rx, tx) = tcp_stream.into_split();
            self.connection = Some(spawn_read_write_tasks(&codec, rx, tx));
        }
        Ok(self.connection.as_ref().unwrap())
    }
}

struct Topic {
    partitions: Vec<Partition>,
}
struct Partition {
    leader_id: i32,
    _replica_nodes: Vec<i32>,
}

#[derive(Clone)]
struct KafkaAddress {
    host: StrBytes,
    port: i32,
}