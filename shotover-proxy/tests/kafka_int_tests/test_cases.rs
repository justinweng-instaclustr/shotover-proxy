use futures::{stream::FuturesUnordered, StreamExt};
use std::{collections::HashMap, time::Duration};
use test_helpers::{
    connection::kafka::{
        Acl, AclOperation, AclPermissionType, AlterConfig, ConfigEntry, ConsumerConfig,
        ExpectedResponse, KafkaConnectionBuilder, KafkaConsumer, KafkaDriver, KafkaProducer,
        ListOffsetsResultInfo, NewPartition, NewTopic, OffsetSpec, Record, ResourcePatternType,
        ResourceSpecifier, ResourceType, TopicPartition,
    },
    docker_compose::DockerCompose,
};
use tokio_bin_process::BinProcess;

async fn admin_setup(connection_builder: &KafkaConnectionBuilder) {
    let admin = connection_builder.connect_admin().await;
    admin
        .create_topics(&[
            NewTopic {
                name: "partitions1",
                num_partitions: 1,
                replication_factor: 1,
            },
            NewTopic {
                name: "partitions1_with_offset",
                num_partitions: 1,
                replication_factor: 1,
            },
            NewTopic {
                name: "partitions3",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "partitions3_case1",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "partitions3_case2",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "partitions3_case3",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "partitions3_case4",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "acks0",
                num_partitions: 1,
                replication_factor: 1,
            },
            NewTopic {
                name: "to_delete",
                num_partitions: 1,
                replication_factor: 1,
            },
            NewTopic {
                name: "multi_topic_batch_partitions_1",
                num_partitions: 1,
                replication_factor: 1,
            },
            NewTopic {
                name: "multi_topic_batch_partitions_3",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "multi_partitions_batch",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "transaction_topic1",
                num_partitions: 3,
                replication_factor: 1,
            },
            NewTopic {
                name: "transaction_topic2",
                num_partitions: 3,
                replication_factor: 1,
            },
        ])
        .await;

    admin
        .create_partitions(&[NewPartition {
            topic_name: "to_delete",
            new_partition_count: 2,
        }])
        .await;

    admin
        // TODO: test ResourceSpecifier::Broker and ResourceSpecifier::Group as well.
        //       Will need to find a way to get a valid broker id and to create a group.
        .describe_configs(&[ResourceSpecifier::Topic("to_delete")])
        .await;

    admin
        .alter_configs(&[AlterConfig {
            specifier: ResourceSpecifier::Topic("to_delete"),
            entries: &[ConfigEntry {
                key: "delete.retention.ms".to_owned(),
                value: "86400001".to_owned(),
            }],
        }])
        .await;

    admin.delete_topics(&["to_delete"]).await
}

/// Attempt to make the driver batch produce requests for different topics into the same request
/// This is important to test since shotover has complex logic for splitting these batch requests into individual requests.
pub async fn produce_consume_multi_topic_batch(connection_builder: &KafkaConnectionBuilder) {
    // set linger to 100ms to ensure that the concurrent produce requests are combined into a single batched request.
    let producer = connection_builder.connect_producer("all", 100).await;
    // create an initial record to force kafka to create the topic if it doesnt yet exist
    tokio::join!(
        producer.assert_produce(
            Record {
                payload: "initial1",
                topic_name: "multi_topic_batch_partitions_1",
                key: None,
            },
            Some(0),
        ),
        producer.assert_produce(
            Record {
                payload: "initial2",
                topic_name: "multi_topic_batch_partitions_3",
                key: Some("foo".into()),
            },
            Some(0),
        ),
        producer.assert_produce(
            Record {
                payload: "initial3",
                topic_name: "batch_test_unknown",
                key: None,
            },
            Some(0),
        )
    );

    let mut consumer_partitions_1 = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic("multi_topic_batch_partitions_1".to_owned())
                .with_group("multi_topic_batch_partitions_1_group"),
        )
        .await;
    let mut consumer_partitions_3 = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic("multi_topic_batch_partitions_3".to_owned())
                .with_group("multi_topic_batch_partitions_3_group"),
        )
        .await;
    let mut consumer_unknown = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic("batch_test_unknown".to_owned())
                .with_group("batch_test_unknown_group"),
        )
        .await;

    tokio::join!(
        consumer_partitions_1.assert_consume(ExpectedResponse {
            message: "initial1".to_owned(),
            key: None,
            topic_name: "multi_topic_batch_partitions_1".to_owned(),
            offset: Some(0),
        }),
        consumer_partitions_3.assert_consume(ExpectedResponse {
            message: "initial2".to_owned(),
            // ensure we route to the same partition every time, so we can assert on the offset when consuming.
            key: Some("foo".to_owned()),
            topic_name: "multi_topic_batch_partitions_3".to_owned(),
            offset: Some(0),
        }),
        consumer_unknown.assert_consume(ExpectedResponse {
            message: "initial3".to_owned(),
            key: None,
            topic_name: "batch_test_unknown".to_owned(),
            offset: Some(0),
        })
    );

    // create and consume records
    for i in 0..5 {
        tokio::join!(
            producer.assert_produce(
                Record {
                    payload: "Message1",
                    topic_name: "multi_topic_batch_partitions_1",
                    key: None,
                },
                Some(i + 1),
            ),
            producer.assert_produce(
                Record {
                    payload: "Message2",
                    topic_name: "multi_topic_batch_partitions_3",
                    key: Some("foo".into()),
                },
                None,
            ),
            producer.assert_produce(
                Record {
                    payload: "Message3",
                    topic_name: "batch_test_unknown",
                    key: None,
                },
                Some(i + 1),
            )
        );

        tokio::join!(
            consumer_partitions_1.assert_consume(ExpectedResponse {
                message: "Message1".to_owned(),
                key: None,
                topic_name: "multi_topic_batch_partitions_1".to_owned(),
                offset: Some(i + 1),
            }),
            consumer_partitions_3.assert_consume(ExpectedResponse {
                message: "Message2".to_owned(),
                key: Some("foo".to_owned()),
                topic_name: "multi_topic_batch_partitions_3".to_owned(),
                offset: Some(i + 1),
            }),
            consumer_unknown.assert_consume(ExpectedResponse {
                message: "Message3".to_owned(),
                key: None,
                topic_name: "batch_test_unknown".to_owned(),
                offset: Some(i + 1),
            })
        );
    }
}

/// Attempt to make the driver batch produce requests for different partitions of the same topic into the same request
/// This is important to test since shotover has complex logic for splitting these batch requests into individual requests.
pub async fn produce_consume_multi_partition_batch(connection_builder: &KafkaConnectionBuilder) {
    // set linger to 100ms to ensure that the concurrent produce requests are combined into a single batched request.
    let producer = connection_builder.connect_producer("all", 100).await;
    // create an initial record to force kafka to create the topic if it doesnt yet exist
    producer
        .assert_produce(
            Record {
                payload: "initial2",
                topic_name: "multi_partitions_batch",
                key: Some("foo".into()),
            },
            Some(0),
        )
        .await;

    let mut consumer = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic("multi_partitions_batch".to_owned())
                .with_group("multi_partitions_batch_group"),
        )
        .await;

    consumer
        .assert_consume(ExpectedResponse {
            message: "initial2".to_owned(),
            // ensure we route to the same partition every time, so we can assert on the offset when consuming.
            key: Some("foo".to_owned()),
            topic_name: "multi_partitions_batch".to_owned(),
            offset: Some(0),
        })
        .await;

    // create and consume records
    let mut futures = FuturesUnordered::new();
    for i in 0..2000 {
        futures.push(producer.assert_produce(
            Record {
                payload: "Message",
                topic_name: "multi_partitions_batch",
                key: Some(format!("Key{i}")),
            },
            None,
        ))
    }
    while let Some(()) = futures.next().await {}

    // TODO: Would be good to assert this, but first we would need to allow producing to be properly ordered by adding a `produce` method that returns a future.
    //       So its disabled for now.
    // for i in 0..2000 {
    //     consumer
    //         .assert_consume(ExpectedResponse {
    //             message: "Message".to_owned(),
    //             key: Some(format!("Key{i}")),
    //             topic_name: "multi_partitions_batch".to_owned(),
    //             offset: Some(i + 1),
    //         })
    //         .await;
    // }
}

pub async fn produce_consume_partitions1(
    connection_builder: &KafkaConnectionBuilder,
    topic_name: &str,
) {
    {
        let producer = connection_builder.connect_producer("all", 0).await;
        // create an initial record to force kafka to create the topic if it doesnt yet exist
        producer
            .assert_produce(
                Record {
                    payload: "initial",
                    topic_name,
                    key: Some("Key".into()),
                },
                Some(0),
            )
            .await;

        let mut consumer = connection_builder
            .connect_consumer(
                ConsumerConfig::consume_from_topic(topic_name.to_owned()).with_group("some_group"),
            )
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "initial".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(0),
            })
            .await;

        // create and consume records
        for i in 0..5 {
            producer
                .assert_produce(
                    Record {
                        payload: "Message1",
                        topic_name,
                        key: Some("Key".into()),
                    },
                    Some(i * 2 + 1),
                )
                .await;
            producer
                .assert_produce(
                    Record {
                        payload: "Message2",
                        topic_name,
                        key: None,
                    },
                    Some(i * 2 + 2),
                )
                .await;

            consumer
                .assert_consume(ExpectedResponse {
                    message: "Message1".to_owned(),
                    key: Some("Key".to_owned()),
                    topic_name: topic_name.to_owned(),
                    offset: Some(i * 2 + 1),
                })
                .await;

            consumer
                .assert_consume(ExpectedResponse {
                    message: "Message2".to_owned(),
                    key: None,
                    topic_name: topic_name.to_owned(),
                    offset: Some(i * 2 + 2),
                })
                .await;
        }
    }

    // if we create a new consumer it will start from the beginning since auto.offset.reset = earliest and enable.auto.commit false
    // so we test that we can access all records ever created on this topic
    let mut consumer = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic(topic_name.to_owned()).with_group("some_group"),
        )
        .await;
    consumer
        .assert_consume(ExpectedResponse {
            message: "initial".to_owned(),
            key: Some("Key".to_owned()),
            topic_name: topic_name.to_owned(),
            offset: Some(0),
        })
        .await;
    for i in 0..5 {
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message1".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(i * 2 + 1),
            })
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message2".to_owned(),
                key: None,
                topic_name: topic_name.to_owned(),
                offset: Some(i * 2 + 2),
            })
            .await;
    }
}

pub async fn produce_consume_partitions1_kafka_node_goes_down(
    driver: KafkaDriver,
    docker_compose: &DockerCompose,
    connection_builder: &KafkaConnectionBuilder,
    topic_name: &str,
) {
    if driver.is_cpp() {
        // Skip this test for CPP driver.
        // While the cpp driver has some retry capabilities,
        // in many cases it will mark a shotover node as down for a single failed request
        // and then immediately return the error to the caller, without waiting the full timeout period,
        // since it has no more nodes to attempt sending to.
        //
        // So we skip this test on the CPP driver to avoid flaky tests.
        return;
    }

    {
        let admin = connection_builder.connect_admin().await;
        admin
            .create_topics(&[NewTopic {
                name: topic_name,
                num_partitions: 1,
                replication_factor: 3,
            }])
            .await;
    }

    {
        let producer = connection_builder.connect_producer("all", 0).await;
        // create an initial record to force kafka to create the topic if it doesnt yet exist
        producer
            .assert_produce(
                Record {
                    payload: "initial",
                    topic_name,
                    key: Some("Key".into()),
                },
                Some(0),
            )
            .await;

        let mut consumer = connection_builder
            .connect_consumer(
                ConsumerConfig::consume_from_topic(topic_name.to_owned())
                    .with_group("kafka_node_goes_down_test_group"),
            )
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "initial".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(0),
            })
            .await;

        docker_compose.kill_service("kafka1");

        // create and consume records
        for i in 0..5 {
            producer
                .assert_produce(
                    Record {
                        payload: "Message1",
                        topic_name,
                        key: Some("Key".into()),
                    },
                    Some(i * 2 + 1),
                )
                .await;
            producer
                .assert_produce(
                    Record {
                        payload: "Message2",
                        topic_name,
                        key: None,
                    },
                    Some(i * 2 + 2),
                )
                .await;

            consumer
                .assert_consume(ExpectedResponse {
                    message: "Message1".to_owned(),
                    key: Some("Key".to_owned()),
                    topic_name: topic_name.to_owned(),
                    offset: Some(i * 2 + 1),
                })
                .await;

            consumer
                .assert_consume(ExpectedResponse {
                    message: "Message2".to_owned(),
                    key: None,
                    topic_name: topic_name.to_owned(),
                    offset: Some(i * 2 + 2),
                })
                .await;
        }
    }

    // if we create a new consumer it will start from the beginning since auto.offset.reset = earliest and enable.auto.commit false
    // so we test that we can access all records ever created on this topic
    let mut consumer = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic(topic_name.to_owned())
                .with_group("kafka_node_goes_down_test_group_new"),
        )
        .await;
    consumer
        .assert_consume(ExpectedResponse {
            message: "initial".to_owned(),
            key: Some("Key".to_owned()),
            topic_name: topic_name.to_owned(),
            offset: Some(0),
        })
        .await;
    for i in 0..5 {
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message1".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(i * 2 + 1),
            })
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message2".to_owned(),
                key: None,
                topic_name: topic_name.to_owned(),
                offset: Some(i * 2 + 2),
            })
            .await;
    }
}

pub async fn produce_consume_partitions1_shotover_nodes_go_down(
    shotover_nodes_to_kill: Vec<BinProcess>,
    connection_builder: &KafkaConnectionBuilder,
    topic_name: &str,
) {
    {
        let admin = connection_builder.connect_admin().await;
        admin
            .create_topics(&[NewTopic {
                name: topic_name,
                num_partitions: 1,
                replication_factor: 3,
            }])
            .await;
    }

    {
        let producer = connection_builder.connect_producer("all", 0).await;
        // create an initial record to force kafka to create the topic if it doesnt yet exist
        producer
            .assert_produce(
                Record {
                    payload: "initial",
                    topic_name,
                    key: Some("Key".into()),
                },
                Some(0),
            )
            .await;

        let mut consumer = connection_builder
            .connect_consumer(
                ConsumerConfig::consume_from_topic(topic_name.to_owned())
                    .with_group("kafka_node_goes_down_test_group"),
            )
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "initial".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(0),
            })
            .await;

        // kill shotover node(s)
        for shotover_node in shotover_nodes_to_kill {
            tokio::time::timeout(
                Duration::from_secs(10),
                shotover_node.shutdown_and_then_consume_events(&[]),
            )
            .await
            .expect("Shotover did not shutdown within 10s");
        }

        // Wait for the up shotover nodes to detect the down nodes
        tokio::time::sleep(Duration::from_secs(10)).await;

        // create and consume records
        for i in 0..5 {
            producer
                .assert_produce(
                    Record {
                        payload: "Message1",
                        topic_name,
                        key: Some("Key".into()),
                    },
                    Some(i * 2 + 1),
                )
                .await;
            producer
                .assert_produce(
                    Record {
                        payload: "Message2",
                        topic_name,
                        key: None,
                    },
                    Some(i * 2 + 2),
                )
                .await;

            consumer
                .assert_consume(ExpectedResponse {
                    message: "Message1".to_owned(),
                    key: Some("Key".to_owned()),
                    topic_name: topic_name.to_owned(),
                    offset: Some(i * 2 + 1),
                })
                .await;

            consumer
                .assert_consume(ExpectedResponse {
                    message: "Message2".to_owned(),
                    key: None,
                    topic_name: topic_name.to_owned(),
                    offset: Some(i * 2 + 2),
                })
                .await;
        }
    }

    // if we create a new consumer it will start from the beginning since auto.offset.reset = earliest and enable.auto.commit false
    // so we test that we can access all records ever created on this topic
    let mut consumer = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic(topic_name.to_owned())
                .with_group("kafka_node_goes_down_test_group_new"),
        )
        .await;
    consumer
        .assert_consume(ExpectedResponse {
            message: "initial".to_owned(),
            key: Some("Key".to_owned()),
            topic_name: topic_name.to_owned(),
            offset: Some(0),
        })
        .await;
    for i in 0..5 {
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message1".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(i * 2 + 1),
            })
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message2".to_owned(),
                key: None,
                topic_name: topic_name.to_owned(),
                offset: Some(i * 2 + 2),
            })
            .await;
    }
}

pub async fn produce_consume_commit_offsets_partitions1(
    connection_builder: &KafkaConnectionBuilder,
    topic_name: &str,
) {
    {
        let producer = connection_builder.connect_producer("1", 0).await;
        producer
            .assert_produce(
                Record {
                    payload: "Initial",
                    topic_name,
                    key: Some("Key".into()),
                },
                Some(0),
            )
            .await;

        let mut consumer = connection_builder
            .connect_consumer(
                ConsumerConfig::consume_from_topic(topic_name.to_owned())
                    .with_group("consumer_group_with_offsets"),
            )
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "Initial".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(0),
            })
            .await;

        // The offset to be committed should be lastProcessedMessageOffset + 1
        let offset1 = HashMap::from([(
            TopicPartition {
                topic_name: topic_name.to_owned(),
                partition: 0,
            },
            1,
        )]);
        consumer.assert_commit_offsets(offset1);

        producer
            .assert_produce(
                Record {
                    payload: "Message1",
                    topic_name,
                    key: Some("Key".into()),
                },
                Some(1),
            )
            .await;

        consumer
            .assert_consume(ExpectedResponse {
                message: "Message1".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(1),
            })
            .await;

        let offset2 = HashMap::from([(
            TopicPartition {
                topic_name: topic_name.to_owned(),
                partition: 0,
            },
            2,
        )]);
        consumer.assert_commit_offsets(offset2);

        producer
            .assert_produce(
                Record {
                    payload: "Message2",
                    topic_name,
                    key: Some("Key".into()),
                },
                Some(2),
            )
            .await;
    }

    {
        // The new consumer should consume Message2 which is at the last uncommitted offset
        let mut consumer = connection_builder
            .connect_consumer(
                ConsumerConfig::consume_from_topic(topic_name.to_owned())
                    .with_group("consumer_group_with_offsets"),
            )
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message2".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(2),
            })
            .await;
    }

    {
        // The new consumer should still consume Message2 as its offset has not been committed
        let mut consumer = connection_builder
            .connect_consumer(
                ConsumerConfig::consume_from_topic(topic_name.to_owned())
                    .with_group("consumer_group_with_offsets"),
            )
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "Message2".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(2),
            })
            .await;
    }

    {
        // A new consumer in another group should consume from the beginning since auto.offset.reset = earliest and enable.auto.commit false
        let mut consumer = connection_builder
            .connect_consumer(
                ConsumerConfig::consume_from_topic(topic_name.to_owned())
                    .with_group("consumer_group_without_offsets"),
            )
            .await;
        consumer
            .assert_consume(ExpectedResponse {
                message: "Initial".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(0),
            })
            .await;
    }
}

pub async fn produce_consume_partitions3(
    connection_builder: &KafkaConnectionBuilder,
    topic_name: &str,
    fetch_min_bytes: i32,
    fetch_wait_max_ms: i32,
) {
    let producer = connection_builder.connect_producer("1", 0).await;
    let mut consumer = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic(topic_name.to_owned())
                .with_group("some_group")
                .with_fetch_min_bytes(fetch_min_bytes)
                .with_fetch_max_wait_ms(fetch_wait_max_ms),
        )
        .await;

    for _ in 0..5 {
        producer
            .assert_produce(
                Record {
                    payload: "Message1",
                    topic_name,
                    key: Some("Key".into()),
                },
                // We cant predict the offsets since that will depend on which partition the keyless record ends up in
                None,
            )
            .await;
        producer
            .assert_produce(
                Record {
                    payload: "Message2",
                    topic_name,
                    key: None,
                },
                None,
            )
            .await;

        consumer
            .assert_consume_in_any_order(vec![
                ExpectedResponse {
                    message: "Message1".to_owned(),
                    key: Some("Key".to_owned()),
                    topic_name: topic_name.to_owned(),
                    offset: None,
                },
                ExpectedResponse {
                    message: "Message2".to_owned(),
                    key: None,
                    topic_name: topic_name.to_owned(),
                    offset: None,
                },
            ])
            .await;
    }
}

async fn produce_consume_transactions(connection_builder: &KafkaConnectionBuilder) {
    let producer = connection_builder.connect_producer("1", 0).await;
    for i in 0..5 {
        producer
            .assert_produce(
                Record {
                    payload: &format!("Message1_{i}"),
                    topic_name: "transaction_topic1",
                    key: Some("Key".into()),
                },
                Some(i * 2),
            )
            .await;
        producer
            .assert_produce(
                Record {
                    payload: &format!("Message2_{i}"),
                    topic_name: "transaction_topic1",
                    key: Some("Key".into()),
                },
                Some(i * 2 + 1),
            )
            .await;
    }

    let transaction_producer = connection_builder
        .connect_producer_with_transactions("some_transaction_id".to_owned())
        .await;
    let mut consumer_topic1 = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic("transaction_topic1".to_owned())
                .with_group("some_group1"),
        )
        .await;
    let mut consumer_topic2 = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic("transaction_topic2".to_owned())
                .with_group("some_group2"),
        )
        .await;

    for i in 0..5 {
        consumer_topic1
            .assert_consume(ExpectedResponse {
                message: format!("Message1_{i}"),
                key: Some("Key".to_owned()),
                topic_name: "transaction_topic1".to_owned(),
                offset: Some(i * 2),
            })
            .await;
        consumer_topic1
            .assert_consume(ExpectedResponse {
                message: format!("Message2_{i}"),
                key: Some("Key".into()),
                topic_name: "transaction_topic1".to_owned(),
                offset: Some(i * 2 + 1),
            })
            .await;
        transaction_producer.begin_transaction();

        transaction_producer
            .assert_produce(
                Record {
                    payload: &format!("Message1_{i}"),
                    topic_name: "transaction_topic2",
                    key: Some("Key".into()),
                },
                // Not sure where the extra offset per loop comes from
                // Possibly the transaction commit counts as a record
                Some(i * 3),
            )
            .await;
        transaction_producer
            .assert_produce(
                Record {
                    payload: &format!("Message2_{i}"),
                    topic_name: "transaction_topic2",
                    key: Some("Key".into()),
                },
                Some(i * 3 + 1),
            )
            .await;

        transaction_producer.send_offsets_to_transaction(&consumer_topic1);
        transaction_producer.commit_transaction();

        consumer_topic2
            .assert_consume_in_any_order(vec![
                ExpectedResponse {
                    message: format!("Message1_{i}"),
                    key: Some("Key".to_owned()),
                    topic_name: "transaction_topic2".to_owned(),
                    offset: Some(i * 3),
                },
                ExpectedResponse {
                    message: format!("Message2_{i}"),
                    key: Some("Key".to_owned()),
                    topic_name: "transaction_topic2".to_owned(),
                    offset: Some(i * 3 + 1),
                },
            ])
            .await;
    }
}

async fn produce_consume_acks0(connection_builder: &KafkaConnectionBuilder) {
    let topic_name = "acks0";
    let producer = connection_builder.connect_producer("0", 0).await;

    for _ in 0..10 {
        producer
            .assert_produce(
                Record {
                    payload: "MessageAcks0",
                    topic_name,
                    key: Some("KeyAcks0".into()),
                },
                None,
            )
            .await;
    }

    let mut consumer = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic(topic_name.to_owned()).with_group("some_group"),
        )
        .await;

    for j in 0..10 {
        consumer
            .assert_consume(ExpectedResponse {
                message: "MessageAcks0".to_owned(),
                key: Some("KeyAcks0".to_owned()),
                topic_name: topic_name.to_owned(),
                offset: Some(j),
            })
            .await;
    }
}

pub async fn test_broker_idle_timeout(connection_builder: &KafkaConnectionBuilder) {
    let admin = connection_builder.connect_admin().await;
    admin
        .create_topics(&[NewTopic {
            name: "partitions3",
            num_partitions: 3,
            replication_factor: 1,
        }])
        .await;

    // cpp driver hits race condition here
    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut producer = connection_builder.connect_producer("all", 0).await;
    let mut consumer = connection_builder
        .connect_consumer(
            ConsumerConfig::consume_from_topic("partitions3".to_owned()).with_group("some_group"),
        )
        .await;

    // write to some open shotover connections
    test_produce_consume_10_times(&mut producer, &mut consumer).await;

    // allow the broker idle timeout to expire with plenty of buffer
    let broker_idle_timeout = Duration::from_secs(20);
    tokio::time::sleep(broker_idle_timeout.mul_f32(1.5)).await;

    // write to some open shotover connections,
    // ensuring shotover reopens any connections closed by the broker due to idle timeout.
    test_produce_consume_10_times(&mut producer, &mut consumer).await;
}

async fn test_produce_consume_10_times(producer: &mut KafkaProducer, consumer: &mut KafkaConsumer) {
    for _ in 0..10 {
        // create an initial record to force kafka to create the topic if it doesnt yet exist
        producer
            .assert_produce(
                Record {
                    payload: "initial",
                    topic_name: "partitions3",
                    key: Some("Key".into()),
                },
                None,
            )
            .await;

        consumer
            .assert_consume(ExpectedResponse {
                message: "initial".to_owned(),
                key: Some("Key".to_owned()),
                topic_name: "partitions3".to_owned(),
                offset: None,
            })
            .await;
    }
}

pub async fn standard_test_suite(connection_builder: &KafkaConnectionBuilder) {
    admin_setup(connection_builder).await;
    produce_consume_partitions1(connection_builder, "partitions1").await;
    produce_consume_partitions1(connection_builder, "unknown_topic").await;
    produce_consume_commit_offsets_partitions1(connection_builder, "partitions1_with_offset").await;
    produce_consume_multi_topic_batch(connection_builder).await;
    produce_consume_multi_partition_batch(connection_builder).await;

    // test with minimum limits
    produce_consume_partitions3(connection_builder, "partitions3_case1", 1, 0).await;
    // test with minimum limits that results in a delay
    produce_consume_partitions3(connection_builder, "partitions3_case2", 1, 1).await;
    // test with default limits
    produce_consume_partitions3(connection_builder, "partitions3_case3", 1, 500).await;
    // set the bytes limit to 1MB so that we will not reach it and will hit the 100ms timeout every time.
    produce_consume_partitions3(connection_builder, "partitions3_case4", 1_000_000, 100).await;

    produce_consume_transactions(connection_builder).await;

    // Only run this test case on the java driver,
    // since even without going through shotover the cpp driver fails this test.
    #[allow(irrefutable_let_patterns)]
    if let KafkaConnectionBuilder::Java(_) = connection_builder {
        // delete and recreate topic to force shotover to adjust its existing routing metadata
        let admin = connection_builder.connect_admin().await;
        admin.delete_topics(&["partitions1"]).await;
        admin
            .create_topics(&[NewTopic {
                name: "partitions1",
                num_partitions: 1,
                replication_factor: 1,
            }])
            .await;
        produce_consume_partitions1(connection_builder, "partitions1").await;

        let results = admin
            .list_offsets(HashMap::from([
                (
                    TopicPartition {
                        topic_name: "partitions3_case3".to_owned(),
                        partition: 0,
                    },
                    OffsetSpec::Earliest,
                ),
                (
                    TopicPartition {
                        topic_name: "partitions3_case3".to_owned(),
                        partition: 1,
                    },
                    OffsetSpec::Earliest,
                ),
                (
                    TopicPartition {
                        topic_name: "partitions3_case3".to_owned(),
                        partition: 2,
                    },
                    OffsetSpec::Earliest,
                ),
                (
                    TopicPartition {
                        topic_name: "partitions1".to_owned(),
                        partition: 0,
                    },
                    OffsetSpec::Latest,
                ),
            ]))
            .await;

        let expected = HashMap::from([
            (
                TopicPartition {
                    topic_name: "partitions3_case3".to_owned(),
                    partition: 0,
                },
                ListOffsetsResultInfo { offset: 0 },
            ),
            (
                TopicPartition {
                    topic_name: "partitions3_case3".to_owned(),
                    partition: 1,
                },
                ListOffsetsResultInfo { offset: 0 },
            ),
            (
                TopicPartition {
                    topic_name: "partitions3_case3".to_owned(),
                    partition: 2,
                },
                ListOffsetsResultInfo { offset: 0 },
            ),
            (
                TopicPartition {
                    topic_name: "partitions1".to_owned(),
                    partition: 0,
                },
                ListOffsetsResultInfo { offset: 11 },
            ),
        ]);
        assert_eq!(results, expected);
    }

    produce_consume_acks0(connection_builder).await;
    connection_builder.admin_cleanup().await;
}

pub async fn cluster_test_suite(connection_builder: &KafkaConnectionBuilder) {
    standard_test_suite(connection_builder).await;
    let admin = connection_builder.connect_admin().await;
    admin
        .create_topics(&[
            NewTopic {
                name: "partitions1_rf3",
                num_partitions: 1,
                replication_factor: 3,
            },
            NewTopic {
                name: "partitions3_rf3",
                num_partitions: 3,
                replication_factor: 3,
            },
        ])
        .await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    produce_consume_partitions1(connection_builder, "partitions1_rf3").await;
    produce_consume_partitions3(connection_builder, "partitions3_rf3", 1, 500).await;
}

pub async fn setup_basic_user_acls(connection: &KafkaConnectionBuilder, username: &str) {
    let admin = connection.connect_admin().await;
    admin
        .create_acls(vec![Acl {
            resource_type: ResourceType::Topic,
            resource_name: "*".to_owned(),
            resource_pattern_type: ResourcePatternType::Literal,
            principal: format!("User:{username}"),
            host: "*".to_owned(),
            operation: AclOperation::Describe,
            permission_type: AclPermissionType::Allow,
        }])
        .await;
}

/// Invariants:
/// * The passed connection is a user setup with the ACL's of `setup_basic_user_acls`
///
/// Assertions:
/// * Asserts that the user cannot perform the admin operation of creating new topics (not allowed by ACL)
///     + Asserts that the topic was not created as a result of the failed topic creation.
/// * Asserts that the user can perform the describe operation on topics (explicitly allowed by ACL)
pub async fn assert_topic_creation_is_denied_due_to_acl(connection: &KafkaConnectionBuilder) {
    let admin = connection.connect_admin().await;
    // attempt to create topic and get auth failure due to missing ACL
    assert_eq!(
        admin
            .create_topics_fallible(&[NewTopic {
                name: "acl_check_topic",
                num_partitions: 1,
                replication_factor: 1,
            }])
            .await
            .unwrap_err()
            .to_string(),
        "org.apache.kafka.common.errors.TopicAuthorizationException: Authorization failed.\n"
    );

    // attempt to describe topic:
    // * The request succeeds because user has AclOperation::Describe.
    // * But no topic is found since the topic creation was denied.
    assert_eq!(
        admin.describe_topic("acl_check_topic").await.unwrap_err().to_string(),
        "org.apache.kafka.common.errors.UnknownTopicOrPartitionException: This server does not host this topic-partition.\n"
    )
}
