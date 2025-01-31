use super::KafkaSinkCluster;
use crate::{
    frame::{
        kafka::{KafkaFrame, RequestBody},
        Frame,
    },
    message::Message,
};
use kafka_protocol::{
    indexmap::IndexMap,
    messages::{
        add_partitions_to_txn_request::AddPartitionsToTxnTransaction,
        list_offsets_request::ListOffsetsTopic, produce_request::TopicProduceData,
        AddPartitionsToTxnRequest, BrokerId, ListOffsetsRequest, ProduceRequest, TopicName,
        TransactionalId,
    },
};
use std::collections::HashMap;

pub trait RequestSplitAndRouter {
    type SubRequests;
    type Request;
    fn get_request_frame(request: &mut Message) -> Option<&mut Self::Request>;
    fn split_by_destination(
        transform: &mut KafkaSinkCluster,
        request: &mut Self::Request,
    ) -> HashMap<BrokerId, Self::SubRequests>;
    fn reassemble(request: &mut Self::Request, item: Self::SubRequests);
}

pub struct ProduceRequestSplitAndRouter;

impl RequestSplitAndRouter for ProduceRequestSplitAndRouter {
    type Request = ProduceRequest;
    type SubRequests = IndexMap<TopicName, TopicProduceData>;

    fn split_by_destination(
        transform: &mut KafkaSinkCluster,
        request: &mut Self::Request,
    ) -> HashMap<BrokerId, Self::SubRequests> {
        transform.split_produce_request_by_destination(request)
    }

    fn get_request_frame(request: &mut Message) -> Option<&mut Self::Request> {
        match request.frame() {
            Some(Frame::Kafka(KafkaFrame::Request {
                body: RequestBody::Produce(request),
                ..
            })) => Some(request),
            _ => None,
        }
    }

    fn reassemble(request: &mut Self::Request, item: Self::SubRequests) {
        request.topic_data = item;
    }
}

pub struct AddPartitionsToTxnRequestSplitAndRouter;

impl RequestSplitAndRouter for AddPartitionsToTxnRequestSplitAndRouter {
    type Request = AddPartitionsToTxnRequest;
    type SubRequests = IndexMap<TransactionalId, AddPartitionsToTxnTransaction>;

    fn split_by_destination(
        transform: &mut KafkaSinkCluster,
        request: &mut Self::Request,
    ) -> HashMap<BrokerId, Self::SubRequests> {
        transform.split_add_partition_to_txn_request_by_destination(request)
    }

    fn get_request_frame(request: &mut Message) -> Option<&mut Self::Request> {
        match request.frame() {
            Some(Frame::Kafka(KafkaFrame::Request {
                body: RequestBody::AddPartitionsToTxn(request),
                ..
            })) => Some(request),
            _ => None,
        }
    }

    fn reassemble(request: &mut Self::Request, item: Self::SubRequests) {
        request.transactions = item;
    }
}

pub struct ListOffsetsRequestSplitAndRouter;

impl RequestSplitAndRouter for ListOffsetsRequestSplitAndRouter {
    type Request = ListOffsetsRequest;
    type SubRequests = Vec<ListOffsetsTopic>;

    fn split_by_destination(
        transform: &mut KafkaSinkCluster,
        request: &mut Self::Request,
    ) -> HashMap<BrokerId, Self::SubRequests> {
        transform.split_list_offsets_request_by_destination(request)
    }

    fn get_request_frame(request: &mut Message) -> Option<&mut Self::Request> {
        match request.frame() {
            Some(Frame::Kafka(KafkaFrame::Request {
                body: RequestBody::ListOffsets(request),
                ..
            })) => Some(request),
            _ => None,
        }
    }

    fn reassemble(request: &mut Self::Request, item: Self::SubRequests) {
        request.topics = item;
    }
}
