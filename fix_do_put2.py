import re

with open("tensor-rag/src/flight.rs", "r") as f:
    content = f.read()

content = content.replace(
    "let mut batch_stream = FlightRecordBatchStream::new_from_flight_data(stream.map_err(|e| e.into()));",
    "use futures::TryStreamExt;\n        let mut batch_stream = FlightRecordBatchStream::new_from_flight_data(stream.map_err(|e: tonic::Status| e.into()));"
)

with open("tensor-rag/src/flight.rs", "w") as f:
    f.write(content)
