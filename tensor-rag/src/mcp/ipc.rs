use crate::shared_memory::SharedMemoryManager;
use crate::vm::{VmExecutionPayload, VmRequest, VmResponse, VmResultPayload};
use std::io::{Read, Write};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

#[cfg(not(unix))]
pub struct UnixStream;

#[cfg(not(unix))]
impl UnixStream {
    pub fn connect<P: AsRef<std::path::Path>>(_path: P) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Unix domain sockets are not supported on Windows",
        ))
    }
}

#[cfg(not(unix))]
impl std::io::Read for UnixStream {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

#[cfg(not(unix))]
impl std::io::Write for UnixStream {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Ok(0)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct VmIpcClient {
    socket_path: String,
}

impl VmIpcClient {
    pub fn from_config() -> Self {
        Self::new(crate::config::get_vm_socket_path())
    }

    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }

    pub fn execute_program(
        &self,
        session_id: &str,
        sequence: u64,
        program: crate::ir::Program,
        query_vector: Vec<f32>,
    ) -> Result<Option<crate::vm::Value>, String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("Failed to connect to VM worker: {}", e))?;

        let shm_manager = SharedMemoryManager::new();
        let shm_name = format!("req_{}_{}", session_id, sequence);

        let req_payload = VmExecutionPayload {
            program,
            query_vector,
        };
        let encoded =
            bincode::serialize(&req_payload).map_err(|e| format!("Serialization error: {}", e))?;

        let region = shm_manager.allocate(&shm_name)?;
        {
            let mut file = region.get_file_for_write()?;
            file.write_all(&encoded)
                .map_err(|e| format!("Write error: {}", e))?;
        }

        let req = VmRequest::Execute {
            session_id: session_id.to_string(),
            sequence,
            shm_name: shm_name.clone(),
        };

        let payload = bincode::serialize(&req).unwrap();
        let len = payload.len() as u32;
        stream.write_all(&len.to_le_bytes()).unwrap();
        stream.write_all(&payload).unwrap();

        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .map_err(|e| format!("Read len error: {}", e))?;
        let len = u32::from_le_bytes(len_buf) as usize;

        let mut resp_payload = vec![0u8; len];
        stream
            .read_exact(&mut resp_payload)
            .map_err(|e| format!("Read payload error: {}", e))?;

        let resp: VmResponse = bincode::deserialize(&resp_payload).unwrap();

        let _ = region.destroy();

        match resp {
            VmResponse::Result {
                shm_name: out_shm, ..
            } => {
                let out_region = shm_manager.open(&out_shm)?;
                let mut file = out_region.get_file_for_read()?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .map_err(|e| format!("Read out error: {}", e))?;

                let res_payload: VmResultPayload = bincode::deserialize(&buf)
                    .map_err(|e| format!("Deserialization error: {}", e))?;
                let _ = out_region.destroy();

                Ok(res_payload.value.map(|v| crate::vm::Value::from(v)))
            }
            VmResponse::Error { message, .. } => Err(message),
            _ => Err("Unexpected response type".to_string()),
        }
    }
}
