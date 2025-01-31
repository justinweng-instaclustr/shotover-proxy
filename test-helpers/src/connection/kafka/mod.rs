use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[cfg(feature = "kafka-cpp-driver-tests")]
pub mod cpp;
pub mod java;
pub mod node;

use anyhow::Result;
#[cfg(feature = "kafka-cpp-driver-tests")]
use cpp::*;
use java::*;

#[derive(Clone, Copy)]
pub enum KafkaDriver {
    #[cfg(feature = "kafka-cpp-driver-tests")]
    Cpp,
    Java,
}

impl KafkaDriver {
    pub fn is_cpp(&self) -> bool {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaDriver::Cpp => true,
            KafkaDriver::Java => false,
        }
    }
}

pub enum KafkaConnectionBuilder {
    #[cfg(feature = "kafka-cpp-driver-tests")]
    Cpp(KafkaConnectionBuilderCpp),
    Java(KafkaConnectionBuilderJava),
}

impl KafkaConnectionBuilder {
    pub fn new(driver: KafkaDriver, address: &str) -> Self {
        match driver {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaDriver::Cpp => Self::Cpp(KafkaConnectionBuilderCpp::new(address)),
            KafkaDriver::Java => Self::Java(KafkaConnectionBuilderJava::new(address)),
        }
    }

    pub fn use_tls(self, certs: &str) -> Self {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(_) => todo!("TLS not implemented for cpp driver"),
            Self::Java(java) => Self::Java(java.use_tls(certs)),
        }
    }

    pub fn use_sasl_scram(self, user: &str, pass: &str) -> Self {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => Self::Cpp(cpp.use_sasl_scram(user, pass)),
            Self::Java(java) => Self::Java(java.use_sasl_scram(user, pass)),
        }
    }

    pub fn use_sasl_plain(self, user: &str, pass: &str) -> Self {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => Self::Cpp(cpp.use_sasl_plain(user, pass)),
            Self::Java(java) => Self::Java(java.use_sasl_plain(user, pass)),
        }
    }

    pub async fn connect_producer(&self, acks: &str, linger_ms: u32) -> KafkaProducer {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => KafkaProducer::Cpp(cpp.connect_producer(acks, linger_ms).await),
            Self::Java(java) => KafkaProducer::Java(java.connect_producer(acks, linger_ms).await),
        }
    }

    pub async fn connect_producer_with_transactions(
        &self,
        transaction_id: String,
    ) -> KafkaProducer {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => {
                KafkaProducer::Cpp(cpp.connect_producer_with_transactions(transaction_id))
            }
            Self::Java(java) => KafkaProducer::Java(
                java.connect_producer_with_transactions(transaction_id)
                    .await,
            ),
        }
    }

    pub async fn connect_consumer(&self, config: ConsumerConfig) -> KafkaConsumer {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => KafkaConsumer::Cpp(cpp.connect_consumer(config).await),
            Self::Java(java) => KafkaConsumer::Java(java.connect_consumer(config).await),
        }
    }

    pub async fn connect_admin(&self) -> KafkaAdmin {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => KafkaAdmin::Cpp(cpp.connect_admin().await),
            Self::Java(java) => KafkaAdmin::Java(java.connect_admin().await),
        }
    }

    pub async fn assert_admin_error(&self) -> anyhow::Error {
        let admin = self.connect_admin().await;

        admin
            .create_topics_fallible(&[NewTopic {
                name: "partitions1",
                num_partitions: 1,
                replication_factor: 1,
            }])
            .await
            .unwrap_err()
    }

    pub async fn admin_cleanup(&self) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.admin_cleanup().await,
            Self::Java(_) => {}
        }
    }
}

pub enum KafkaProducer {
    #[cfg(feature = "kafka-cpp-driver-tests")]
    Cpp(KafkaProducerCpp),
    Java(KafkaProducerJava),
}

impl KafkaProducer {
    pub async fn assert_produce(&self, record: Record<'_>, expected_offset: Option<i64>) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.assert_produce(record, expected_offset).await,
            Self::Java(java) => java.assert_produce(record, expected_offset).await,
        }
    }

    pub fn begin_transaction(&self) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.begin_transaction(),
            Self::Java(java) => java.begin_transaction(),
        }
    }

    pub fn commit_transaction(&self) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.commit_transaction(),
            Self::Java(java) => java.commit_transaction(),
        }
    }

    pub fn send_offsets_to_transaction(&self, consumer: &KafkaConsumer) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => match consumer {
                KafkaConsumer::Cpp(consumer) => cpp.send_offsets_to_transaction(consumer),
                KafkaConsumer::Java(_) => {
                    panic!("Cannot use transactions across java and cpp driver")
                }
            },
            Self::Java(java) => match consumer {
                KafkaConsumer::Java(consumer) => java.send_offsets_to_transaction(consumer),
                #[cfg(feature = "kafka-cpp-driver-tests")]
                KafkaConsumer::Cpp(_) => {
                    panic!("Cannot use transactions across java and cpp driver")
                }
            },
        }
    }
}

pub struct Record<'a> {
    pub payload: &'a str,
    pub topic_name: &'a str,
    pub key: Option<String>,
}

pub enum KafkaConsumer {
    #[cfg(feature = "kafka-cpp-driver-tests")]
    Cpp(KafkaConsumerCpp),
    Java(KafkaConsumerJava),
}

impl KafkaConsumer {
    pub async fn assert_consume(&mut self, expected_response: ExpectedResponse) {
        let response = match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.consume().await,
            Self::Java(java) => java.consume().await,
        };

        let topic = &expected_response.topic_name;
        assert_eq!(
            expected_response.topic_name, response.topic_name,
            "Unexpected topic"
        );
        assert_eq!(
            expected_response.message, response.message,
            "Unexpected message for topic {topic}"
        );
        assert_eq!(
            expected_response.key, response.key,
            "Unexpected key for topic {topic}"
        );

        if expected_response.offset.is_some() {
            assert_eq!(
                expected_response.offset, response.offset,
                "Unexpected offset for topic {topic}"
            );
        }
    }

    pub async fn assert_consume_in_any_order(&mut self, expected_responses: Vec<ExpectedResponse>) {
        let mut responses = vec![];
        while responses.len() < expected_responses.len() {
            match self {
                #[cfg(feature = "kafka-cpp-driver-tests")]
                Self::Cpp(cpp) => responses.push(cpp.consume().await),
                Self::Java(java) => responses.push(java.consume().await),
            }
        }
        let full_responses = responses.clone();
        let full_expected_responses = expected_responses.clone();

        for expected_response in expected_responses {
            match responses.iter().enumerate().find(|(_, x)| **x == expected_response) {
                Some((i, _)) => {
                    responses.remove(i);
                }
                None => panic!("An expected response was not found in the actual responses\nExpected responses:{full_expected_responses:#?}\nActual responses:{full_responses:#?}"),
            }
        }
    }

    /// The offset to be committed should be lastProcessedMessageOffset + 1.
    pub fn assert_commit_offsets(&self, offsets: HashMap<TopicPartition, i64>) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.commit(&offsets),
            Self::Java(java) => java.commit(&offsets),
        }

        let partitions = offsets.keys().cloned().collect::<HashSet<_>>();

        let responses = match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.committed_offsets(partitions),
            Self::Java(java) => java.committed_offsets(partitions),
        };

        assert_eq!(responses.len(), offsets.len());
        for (topic_partition, offset) in offsets {
            let response_offset = responses.get(&topic_partition).unwrap();
            assert_eq!(*response_offset, offset);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExpectedResponse {
    pub message: String,
    pub key: Option<String>,
    pub topic_name: String,
    /// Responses will always have this set to Some.
    /// The test case can set this to None to ignore the value of the actual response.
    pub offset: Option<i64>,
}

impl PartialEq for ExpectedResponse {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
            && self.key == other.key
            && self.topic_name == other.topic_name
            && match (self.offset, other.offset) {
                (None, None) => true,
                (None, Some(_)) => true,
                (Some(_), None) => true,
                (Some(a), Some(b)) => a == b,
            }
    }
}

pub enum KafkaAdmin {
    #[cfg(feature = "kafka-cpp-driver-tests")]
    Cpp(KafkaAdminCpp),
    Java(KafkaAdminJava),
}

impl KafkaAdmin {
    pub async fn create_topics_fallible(&self, topics: &[NewTopic<'_>]) -> Result<()> {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaAdmin::Cpp(cpp) => cpp.create_topics_fallible(topics).await,
            KafkaAdmin::Java(java) => java.create_topics_fallible(topics).await,
        }
    }

    pub async fn create_topics(&self, topics: &[NewTopic<'_>]) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaAdmin::Cpp(cpp) => cpp.create_topics(topics).await,
            KafkaAdmin::Java(java) => java.create_topics(topics).await,
        }
    }

    pub async fn describe_topic(&self, topic_name: &str) -> Result<TopicDescription> {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaAdmin::Cpp(_) => unimplemented!(),
            KafkaAdmin::Java(java) => java.describe_topic(topic_name).await,
        }
    }

    pub async fn delete_topics(&self, to_delete: &[&str]) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(cpp) => cpp.delete_topics(to_delete).await,
            Self::Java(java) => java.delete_topics(to_delete).await,
        }
    }

    pub async fn list_offsets(
        &self,
        topic_partitions: HashMap<TopicPartition, OffsetSpec>,
    ) -> HashMap<TopicPartition, ListOffsetsResultInfo> {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            Self::Cpp(_) => panic!("rdkafka-rs driver does not support list_offsets"),
            Self::Java(java) => java.list_offsets(topic_partitions).await,
        }
    }

    pub async fn create_partitions(&self, partitions: &[NewPartition<'_>]) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaAdmin::Cpp(cpp) => cpp.create_partitions(partitions).await,
            KafkaAdmin::Java(java) => java.create_partitions(partitions).await,
        }
    }

    pub async fn describe_configs(&self, resources: &[ResourceSpecifier<'_>]) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaAdmin::Cpp(cpp) => cpp.describe_configs(resources).await,
            KafkaAdmin::Java(java) => java.describe_configs(resources).await,
        }
    }

    pub async fn alter_configs(&self, alter_configs: &[AlterConfig<'_>]) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaAdmin::Cpp(cpp) => cpp.alter_configs(alter_configs).await,
            KafkaAdmin::Java(java) => java.alter_configs(alter_configs).await,
        }
    }

    pub async fn create_acls(&self, acls: Vec<Acl>) {
        match self {
            #[cfg(feature = "kafka-cpp-driver-tests")]
            KafkaAdmin::Cpp(_) => unimplemented!("CPP driver does not support creating ACL's"),
            KafkaAdmin::Java(java) => java.create_acls(acls).await,
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
pub struct ListOffsetsResultInfo {
    pub offset: i32,
}

pub struct NewTopic<'a> {
    pub name: &'a str,
    pub num_partitions: i32,
    pub replication_factor: i16,
}

pub struct NewPartition<'a> {
    pub topic_name: &'a str,
    pub new_partition_count: i32,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct TopicPartition {
    pub topic_name: String,
    pub partition: i32,
}

pub enum ResourceSpecifier<'a> {
    Topic(&'a str),
}

pub struct AlterConfig<'a> {
    pub specifier: ResourceSpecifier<'a>,
    pub entries: &'a [ConfigEntry],
}

pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

pub struct Acl {
    pub resource_type: ResourceType,
    pub resource_name: String,
    pub resource_pattern_type: ResourcePatternType,
    pub principal: String,
    pub host: String,
    pub operation: AclOperation,
    pub permission_type: AclPermissionType,
}

/// https://docs.confluent.io/platform/current/clients/javadocs/javadoc/org/apache/kafka/common/resource/ResourceType.html
pub enum ResourceType {
    Cluster,
    DelegationToken,
    Group,
    Topic,
    TransactionalId,
    User,
}

/// https://docs.confluent.io/platform/current/clients/javadocs/javadoc/org/apache/kafka/common/resource/PatternType.html
pub enum ResourcePatternType {
    Literal,
    Prefixed,
}

/// https://docs.confluent.io/platform/current/clients/javadocs/javadoc/org/apache/kafka/common/acl/AclOperation.html
pub enum AclOperation {
    All,
    Alter,
    AlterConfigs,
    ClusterAction,
    Create,
    CreateTokens,
    Delete,
    Describe,
    DescribeConfigs,
    DescribeTokens,
    Read,
    Write,
}

/// https://docs.confluent.io/platform/current/clients/javadocs/javadoc/org/apache/kafka/common/acl/AclPermissionType.html
pub enum AclPermissionType {
    Allow,
    Deny,
}

#[derive(Debug)]
pub struct TopicDescription {
    // None of our tests actually make use of the contents of TopicDescription,
    // instead they just check if the describe succeeded or failed,
    // so this is intentionally left empty for now
}

pub enum OffsetSpec {
    Earliest,
    Latest,
}

#[derive(Default)]
pub struct ConsumerConfig {
    topic_name: String,
    group: String,
    fetch_min_bytes: i32,
    fetch_max_wait_ms: i32,
}

impl ConsumerConfig {
    pub fn consume_from_topic(topic_name: String) -> Self {
        Self {
            topic_name,
            group: "default_group".to_owned(),
            fetch_min_bytes: 1,
            fetch_max_wait_ms: 500,
        }
    }

    pub fn with_group(mut self, group: &str) -> Self {
        self.group = group.to_owned();
        self
    }

    pub fn with_fetch_min_bytes(mut self, fetch_min_bytes: i32) -> Self {
        self.fetch_min_bytes = fetch_min_bytes;
        self
    }

    pub fn with_fetch_max_wait_ms(mut self, fetch_max_wait_ms: i32) -> Self {
        self.fetch_max_wait_ms = fetch_max_wait_ms;
        self
    }
}
