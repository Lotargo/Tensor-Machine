use std::sync::Mutex;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::Arc;

/// Represents a named shared memory region mapped via temporary files.
/// In a Linux environment, temp files are typically backed by tmpfs.
pub struct MmapRegion {
    pub name: String,
    pub path: PathBuf,
}

impl MmapRegion {
    /// Returns the path for a given region name.
    fn get_path_for_name(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("tensor_rag_shm_{}", name));
        path
    }

    /// Creates a new memory mapped region with the given name.
    pub fn new(name: &str) -> Result<Self, String> {
        let path = Self::get_path_for_name(name);

        // Create the file. If it already exists, this opens it.
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .map_err(|e| format!("Failed to create shm file: {}", e))?;

        Ok(Self {
            name: name.to_string(),
            path,
        })
    }

    /// Opens an existing memory mapped region with the given name.
    pub fn open_existing(name: &str) -> Result<Self, String> {
        let path = Self::get_path_for_name(name);

        if !path.exists() {
            return Err(format!("Region {} not found", name));
        }

        Ok(Self {
            name: name.to_string(),
            path,
        })
    }

    /// Obtains a file handle for writing.
    pub fn get_file_for_write(&self) -> Result<File, String> {
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)
            .map_err(|e| format!("Failed to open shm file for write: {}", e))
    }

    /// Obtains a file handle for reading and seeking.
    pub fn get_file_for_read(&self) -> Result<File, String> {
        OpenOptions::new()
            .read(true)
            .open(&self.path)
            .map_err(|e| format!("Failed to open shm file for read: {}", e))
    }

    /// Explicitly cleans up the temporary file representing the region.
    /// This should only be called when all readers and writers are finished
    /// across all processes, as it physically deletes the tmpfs file.
    pub fn destroy(&self) -> Result<(), String> {
        std::fs::remove_file(&self.path).map_err(|e| format!("Failed to remove shm file: {}", e))
    }
}

/// Manages multiple shared memory regions.
#[derive(Clone)]
pub struct SharedMemoryManager {
    regions: Arc<Mutex<HashMap<String, Arc<MmapRegion>>>>,
}

impl Default for SharedMemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedMemoryManager {
    pub fn new() -> Self {
        Self {
            regions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Allocates a new region and stores it.
    pub fn allocate(&self, name: &str) -> Result<Arc<MmapRegion>, String> {
        let region = Arc::new(MmapRegion::new(name)?);
        let mut map = self.regions.lock().map_err(|_| "Mutex poison error")?;
        map.insert(name.to_string(), region.clone());
        Ok(region)
    }

    /// Opens an existing region by name.
    pub fn open(&self, name: &str) -> Result<Arc<MmapRegion>, String> {
        let mut map = self.regions.lock().map_err(|_| "Mutex poison error")?;

        if let Some(region) = map.get(name) {
            return Ok(region.clone());
        }

        // Fallback to opening the existing file if it exists, enabling cross-process IPC.
        let region = Arc::new(MmapRegion::open_existing(name)?);
        map.insert(name.to_string(), region.clone());
        Ok(region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn test_allocate_write_read_destroy() {
        let manager = SharedMemoryManager::new();
        let region_name = "test_shm_region_1";

        let region = manager.allocate(region_name).expect("Failed to allocate region");

        // Write to the file
        {
            let mut file = region.get_file_for_write().expect("Failed to open for write");
            file.write_all(b"hello world").expect("Failed to write data");
        }

        // Read from the file via the same region
        {
            let mut file = region.get_file_for_read().expect("Failed to open for read");
            let mut buf = String::new();
            file.read_to_string(&mut buf).expect("Failed to read data");
            assert_eq!(buf, "hello world");
        }

        // Open existing should work
        let region2 = manager.open(region_name).expect("Failed to open region");
        assert_eq!(region.path, region2.path);

        // Destroy
        region.destroy().expect("Failed to destroy region");

        // Ensure it's gone
        assert!(!region.path.exists());
    }
}
