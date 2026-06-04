use arrow::array::{Float32Array, Int32Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use arrow_flight::encode::FlightDataEncoderBuilder;
use arrow_flight::flight_service_client::FlightServiceClient;
use arrow_flight::{FlightDescriptor, Ticket};
use futures::StreamExt;
use futures::TryStreamExt;
use std::sync::Arc;
use tensor_rag::flight::TensorFlightService;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

#[tokio::test]
async fn test_flight_put_get_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let service = TensorFlightService::new();
        Server::builder()
            .add_service(arrow_flight::flight_service_server::FlightServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    // Wait a bit for the server to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut client = FlightServiceClient::connect(format!("http://{}", addr))
        .await
        .unwrap();

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("val", DataType::Float32, false),
    ]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int32Array::from(vec![1, 2, 3])),
            Arc::new(Float32Array::from(vec![1.1, 2.2, 3.3])),
        ],
    )
    .unwrap();

    let mut encoder = FlightDataEncoderBuilder::new()
        .with_schema(schema.clone())
        .build(futures::stream::iter(vec![Ok(batch.clone())]));

    let mut flight_data_msgs = Vec::new();
    while let Some(Ok(data)) = encoder.next().await {
        let mut mut_data = data;
        mut_data.flight_descriptor =
            Some(FlightDescriptor::new_path(vec!["test_dataset".to_string()]));
        flight_data_msgs.push(mut_data);
    }

    let request = tonic::Request::new(futures::stream::iter(flight_data_msgs));
    let _ = client.do_put(request).await.expect("do_put failed");

    let ticket = Ticket {
        ticket: "test_dataset".as_bytes().to_vec().into(),
    };
    let get_stream = client
        .do_get(ticket)
        .await
        .expect("do_get failed")
        .into_inner();

    use arrow_flight::decode::FlightRecordBatchStream;
    let mut batch_stream = FlightRecordBatchStream::new_from_flight_data(
        get_stream.map_err(|e: tonic::Status| e.into()),
    );

    let mut retrieved_batches = Vec::new();
    while let Some(Ok(batch)) = batch_stream.next().await {
        retrieved_batches.push(batch);
    }

    assert!(!retrieved_batches.is_empty(), "Expected retrieved messages");
    assert_eq!(retrieved_batches[0].schema().fields().len(), 2);
}
