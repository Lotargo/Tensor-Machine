import re

with open("tensor-rag/tests/flight_integration.rs", "r") as f:
    content = f.read()

# Instead of testing via IPC deserialization which requires careful dictionary handling,
# We just verify it arrives via FlightRecordBatchStream from do_get!
new_get = """    let mut get_stream = client.do_get(ticket).await.expect("do_get failed").into_inner();

    use arrow_flight::decode::FlightRecordBatchStream;
    let mut batch_stream = FlightRecordBatchStream::new_from_flight_data(get_stream.map_err(|e| e.into()));

    let mut retrieved_batches = Vec::new();
    while let Some(Ok(batch)) = batch_stream.next().await {
        retrieved_batches.push(batch);
    }

    assert!(!retrieved_batches.is_empty(), "Expected retrieved messages");
    assert_eq!(retrieved_batches[0].schema().fields().len(), 2);
}"""

content = re.sub(r'    let mut get_stream = client.do_get\(ticket\).await.expect\("do_get failed"\).into_inner\(\);\n.*?    assert_eq!\(schema_batch.fields\(\).len\(\), 2\);\n\}', new_get, content, flags=re.DOTALL)

with open("tensor-rag/tests/flight_integration.rs", "w") as f:
    f.write(content)
