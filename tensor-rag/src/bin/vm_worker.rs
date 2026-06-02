use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;

use tensor_rag::ir::Program;
use tensor_rag::shared_memory::SharedMemoryManager;
use tensor_rag::vm::{IpcValue, VM, VmExecutionPayload, VmRequest, VmResponse, VmResultPayload};

fn handle_client(mut stream: UnixStream) {
    loop {
        // Read 4 bytes for length
        let mut len_buf = [0u8; 4];
        if stream.read_exact(&mut len_buf).is_err() {
            break;
        }
        let len = u32::from_le_bytes(len_buf) as usize;

        // Read payload
        let mut payload = vec![0u8; len];
        if stream.read_exact(&mut payload).is_err() {
            break;
        }

        // Deserialize request
        let req: VmRequest = match bincode::deserialize(&payload) {
            Ok(req) => req,
            Err(e) => {
                let err_resp = VmResponse::Error {
                    session_id: String::new(),
                    sequence: 0,
                    message: format!("Failed to deserialize request: {}", e),
                };
                send_response(&mut stream, &err_resp);
                continue;
            }
        };

        match req {
            VmRequest::Execute {
                session_id,
                sequence,
                shm_name,
            } => {
                let shm = SharedMemoryManager::new();
                let payload: VmExecutionPayload = match shm.open(&shm_name) {
                    Ok(region) => {
                        let mut file = region.get_file_for_read().unwrap();
                        let mut buf = Vec::new();
                        file.read_to_end(&mut buf).unwrap();
                        bincode::deserialize(&buf).unwrap()
                    }
                    Err(e) => {
                        send_response(
                            &mut stream,
                            &VmResponse::Error {
                                session_id,
                                sequence,
                                message: e,
                            },
                        );
                        continue;
                    }
                };

                let db_pools = tensor_rag::db::DbPools::new();
                let mut vm = VM::new();
                vm.query_vector = ndarray::Array1::from_vec(payload.query_vector);

                let rt = tokio::runtime::Runtime::new().unwrap();
                let mut instruction_count = 0;
                let max_instructions = 100_000;

                let result = rt.block_on(tensor_rag::mcp::execute_program(
                    &payload.program,
                    &mut vm,
                    &db_pools,
                    &mut instruction_count,
                    max_instructions,
                ));

                let resp = match result {
                    Ok(val) => {
                        let res_payload = VmResultPayload {
                            value: val.map(|v| IpcValue::from(&v)),
                        };
                        let out_shm_name = format!("{}_out", shm_name);
                        if let Ok(region) = shm.allocate(&out_shm_name) {
                            let mut file = region.get_file_for_write().unwrap();
                            let encoded = bincode::serialize(&res_payload).unwrap();
                            file.write_all(&encoded).unwrap();
                            VmResponse::Result {
                                session_id,
                                sequence,
                                shm_name: out_shm_name,
                            }
                        } else {
                            VmResponse::Error {
                                session_id,
                                sequence,
                                message: "Failed to allocate SHM for result".into(),
                            }
                        }
                    }
                    Err(e) => VmResponse::Error {
                        session_id,
                        sequence,
                        message: e,
                    },
                };

                send_response(&mut stream, &resp);
            }
            VmRequest::Shutdown => {
                send_response(&mut stream, &VmResponse::ShutdownAck);
                std::process::exit(0);
            }
        }
    }
}

fn send_response(stream: &mut UnixStream, resp: &VmResponse) {
    let payload = bincode::serialize(resp).unwrap();
    let len = payload.len() as u32;
    if stream.write_all(&len.to_le_bytes()).is_ok() {
        let _ = stream.write_all(&payload);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut socket_path = "/tmp/tensor_rag_vm.sock".to_string();

    for arg in args {
        if arg.starts_with("--socket=") {
            socket_path = arg.split('=').nth(1).unwrap().to_string();
        }
    }

    // Clean up old socket if it exists
    if std::path::Path::new(&socket_path).exists() {
        let _ = fs::remove_file(&socket_path);
    }

    let listener = UnixListener::bind(&socket_path).expect("Failed to bind to socket");
    println!("VM Worker listening on {}", socket_path);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // In a real system, we might spawn a thread per connection.
                // For simplicity here, we'll handle it synchronously or spawn a thread.
                std::thread::spawn(move || handle_client(stream));
            }
            Err(err) => {
                println!("Error accepting connection: {}", err);
                break;
            }
        }
    }
}
