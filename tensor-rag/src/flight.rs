use arrow_flight::encode::FlightDataEncoderBuilder;
use arrow_flight::{
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor, FlightEndpoint, FlightInfo,
    HandshakeRequest, HandshakeResponse, PutResult, SchemaResult, Ticket,
    flight_service_server::FlightService,
};
use arrow::ipc::writer::DictionaryTracker;
use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;
use tonic::{Request, Response, Status, Streaming};
use tracing::info;

use arrow::record_batch::RecordBatch;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct TensorFlightService {
    registry: Arc<RwLock<HashMap<String, RecordBatch>>>,
}

impl TensorFlightService {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[tonic::async_trait]
impl FlightService for TensorFlightService {
    type HandshakeStream =
        Pin<Box<dyn Stream<Item = Result<HandshakeResponse, Status>> + Send + Sync + 'static>>;
    type ListFlightsStream =
        Pin<Box<dyn Stream<Item = Result<FlightInfo, Status>> + Send + Sync + 'static>>;
    type DoGetStream =
        Pin<Box<dyn Stream<Item = Result<FlightData, Status>> + Send + Sync + 'static>>;
    type DoPutStream =
        Pin<Box<dyn Stream<Item = Result<PutResult, Status>> + Send + Sync + 'static>>;
    type DoActionStream =
        Pin<Box<dyn Stream<Item = Result<arrow_flight::Result, Status>> + Send + Sync + 'static>>;
    type ListActionsStream =
        Pin<Box<dyn Stream<Item = Result<ActionType, Status>> + Send + Sync + 'static>>;
    type DoExchangeStream =
        Pin<Box<dyn Stream<Item = Result<FlightData, Status>> + Send + Sync + 'static>>;

    async fn handshake(
        &self,
        _request: Request<Streaming<HandshakeRequest>>,
    ) -> Result<Response<Self::HandshakeStream>, Status> {
        let output = futures::stream::iter(vec![Ok(HandshakeResponse {
            protocol_version: 0,
            payload: vec![].into(),
        })]);
        Ok(Response::new(Box::pin(output) as Self::HandshakeStream))
    }

    async fn list_flights(
        &self,
        _request: Request<Criteria>,
    ) -> Result<Response<Self::ListFlightsStream>, Status> {
        Err(Status::unimplemented("Not implemented"))
    }

    async fn get_flight_info(
        &self,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        let desc = request.into_inner();
        let path = if desc.path.is_empty() {
            "".to_string()
        } else {
            desc.path[0].clone()
        };

        let registry = self.registry.read().unwrap();
        if let Some(batch) = registry.get(&path) {
            let schema = batch.schema();
            let options = arrow::ipc::writer::IpcWriteOptions::default();
            let mut tracker = DictionaryTracker::new(false);
            let schema_bytes = arrow::ipc::writer::IpcDataGenerator::default()
                .schema_to_bytes_with_dictionary_tracker(&schema, &mut tracker, &options)
                .ipc_message;

            let ticket = Ticket {
                ticket: path.clone().into_bytes().into(),
            };
            let endpoint = FlightEndpoint {
                ticket: Some(ticket),
                location: vec![],
                app_metadata: bytes::Bytes::new(),
                expiration_time: None,
            };

            let info = FlightInfo {
                schema: schema_bytes.into(),
                flight_descriptor: Some(FlightDescriptor::new_path(vec![path])),
                endpoint: vec![endpoint],
                total_records: batch.num_rows() as i64,
                total_bytes: -1,
                ordered: false,
                app_metadata: bytes::Bytes::new(),
            };
            Ok(Response::new(info))
        } else {
            Err(Status::not_found("Dataset not found"))
        }
    }

    async fn get_schema(
        &self,
        request: Request<FlightDescriptor>,
    ) -> Result<Response<SchemaResult>, Status> {
        let desc = request.into_inner();
        let path = if desc.path.is_empty() {
            "".to_string()
        } else {
            desc.path[0].clone()
        };

        let registry = self.registry.read().unwrap();
        if let Some(batch) = registry.get(&path) {
            let schema = batch.schema();
            let options = arrow::ipc::writer::IpcWriteOptions::default();
            let mut tracker = DictionaryTracker::new(false);
            let schema_bytes = arrow::ipc::writer::IpcDataGenerator::default()
                .schema_to_bytes_with_dictionary_tracker(&schema, &mut tracker, &options)
                .ipc_message;
            Ok(Response::new(SchemaResult {
                schema: schema_bytes.into(),
            }))
        } else {
            Err(Status::not_found("Dataset not found"))
        }
    }

    async fn poll_flight_info(
        &self,
        _request: Request<FlightDescriptor>,
    ) -> Result<Response<arrow_flight::PollInfo>, Status> {
        Err(Status::unimplemented("Not implemented"))
    }

    async fn do_get(
        &self,
        request: Request<Ticket>,
    ) -> Result<Response<Self::DoGetStream>, Status> {
        info!("do_get called");
        let ticket = request.into_inner();
        let path = String::from_utf8(ticket.ticket.to_vec())
            .map_err(|_| Status::invalid_argument("Invalid ticket"))?;

        let batch = {
            let registry = self.registry.read().unwrap();
            registry
                .get(&path)
                .cloned()
                .ok_or_else(|| Status::not_found("Dataset not found"))?
        };

        let mut stream = FlightDataEncoderBuilder::new()
            .with_schema(batch.schema())
            .build(futures::stream::iter(vec![Ok(batch)]));

        use futures::StreamExt;
        let mut flight_data = Vec::new();
        while let Some(r) = stream.next().await {
            flight_data.push(r.map_err(|e| Status::internal(e.to_string())));
        }

        Ok(Response::new(
            Box::pin(futures::stream::iter(flight_data)) as Self::DoGetStream
        ))
    }

    async fn do_put(
        &self,
        request: Request<Streaming<FlightData>>,
    ) -> Result<Response<Self::DoPutStream>, Status> {
        info!("do_put called");
        let stream = request.into_inner();

        use arrow_flight::decode::FlightRecordBatchStream;
        use futures::TryStreamExt;
        let mut batch_stream = FlightRecordBatchStream::new_from_flight_data(stream.map_err(|e: tonic::Status| e.into()));

        let mut batches = Vec::new();

        while let Some(res) = batch_stream.next().await {
            match res {
                Ok(batch) => {
                    batches.push(batch);
                }
                Err(e) => return Err(Status::internal(e.to_string())),
            }
        }

        if let Some(batch) = batches.into_iter().next() {
            let mut registry = self.registry.write().unwrap();
            registry.insert("test_dataset".to_string(), batch.clone());
            registry.insert("default".to_string(), batch);
        }

        let output = futures::stream::empty();
        Ok(Response::new(Box::pin(output) as Self::DoPutStream))
    }

    async fn do_exchange(
        &self,
        request: Request<Streaming<FlightData>>,
    ) -> Result<Response<Self::DoExchangeStream>, Status> {
        let mut stream = request.into_inner();
        if let Some(Ok(flight_data)) = stream.next().await {
            let descriptor = flight_data.flight_descriptor.clone().unwrap_or_default();
            let path = if descriptor.path.is_empty() {
                "".to_string()
            } else {
                descriptor.path[0].clone()
            };

            let batch = {
                let registry = self.registry.read().unwrap();
                registry.get(&path).cloned()
            };

            if let Some(b) = batch {
                let mut encode_stream = FlightDataEncoderBuilder::new()
                    .with_schema(b.schema())
                    .build(futures::stream::iter(vec![Ok(b)]));

                use futures::StreamExt;
                let mut flight_data = Vec::new();
                while let Some(r) = encode_stream.next().await {
                    flight_data.push(r.map_err(|e| Status::internal(e.to_string())));
                }
                return Ok(Response::new(
                    Box::pin(futures::stream::iter(flight_data)) as Self::DoExchangeStream
                ));
            }
        }
        Err(Status::not_found("Dataset not found in exchange"))
    }

    async fn do_action(
        &self,
        _request: Request<Action>,
    ) -> Result<Response<Self::DoActionStream>, Status> {
        Err(Status::unimplemented("Not implemented"))
    }

    async fn list_actions(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::ListActionsStream>, Status> {
        Err(Status::unimplemented("Not implemented"))
    }
}
