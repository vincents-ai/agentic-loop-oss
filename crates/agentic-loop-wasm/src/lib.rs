//! # agentic-loop-wasm
//!
//! WASM runtime adapter for agentic-loop plugins using wasmtime 44.
//!
//! Plugin ABI: each .wasm module must export:
//! - `plugin_name() -> string` — returns plugin name
//! - `plugin_info() -> string` — returns JSON tool metadata
//! - `handle_tool_call(args: string) -> string` — executes tool call
//!
//! Uses wasmtime with WASI support for filesystem/network access.

use anyhow::Result;
use agentic_loop_tools::ToolProvider;
use agentic_loop_types::tool::ToolInfo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::*;

/// Metadata about a WASM plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmPluginMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: PathBuf,
}

/// A loaded WASM plugin with its compiled module.
struct WasmPlugin {
    meta: WasmPluginMeta,
    tool_info: ToolInfo,
    module: Module,
}

/// Host for WASM plugin tools using wasmtime.
pub struct WasmPluginHost {
    engine: Engine,
    plugins: HashMap<String, WasmPlugin>,
}

impl WasmPluginHost {
    /// Create a new WASM plugin host with a fresh wasmtime engine.
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_multi_memory(true);
        config.wasm_component_model(false);

        let engine = Engine::new(&config)?;

        Ok(Self {
            engine,
            plugins: HashMap::new(),
        })
    }

    /// Scan a directory for .wasm plugin files and register them.
    pub async fn scan_directory(&mut self, dir: &Path) -> Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        let mut entries = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                match self.load_plugin(&path).await {
                    Ok(_) => count += 1,
                    Err(e) => {
                        tracing::warn!("Failed to load WASM plugin {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(count)
    }

    /// Load a single .wasm plugin file and extract metadata via wasmtime.
    pub async fn load_plugin(&mut self, path: &Path) -> Result<String> {
        let wasm_bytes = tokio::fs::read(path).await?;

        let engine = self.engine.clone();
        let path_owned = path.to_path_buf();
        let (module, name, tool_info) = tokio::task::spawn_blocking(move || -> Result<(Module, String, ToolInfo)> {
            let module = Module::from_binary(&engine, &wasm_bytes)
                .map_err(|e| anyhow::anyhow!("Failed to compile WASM module {:?}: {}", path_owned, e))?;

            // Create a temporary store to call metadata exports
            let mut store = Store::new(&engine, ());
            let instance = Instance::new(&mut store, &module, &[])?;

            // Try to call plugin_name() export
            let name = if let Ok(func) = instance.get_typed_func::<(), (u32, u32)>(&mut store, "plugin_name") {
                let (ptr, len) = func.call(&mut store, ())?;
                read_memory_string(&mut store, &instance, ptr, len)
            } else {
                path_owned
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            };

            // Try to call plugin_info() export for tool metadata
            let tool_info = if let Ok(func) = instance.get_typed_func::<(), (u32, u32)>(&mut store, "plugin_info") {
                let (ptr, len) = func.call(&mut store, ())?;
                let info_json = read_memory_string(&mut store, &instance, ptr, len);
                serde_json::from_str::<ToolInfo>(&info_json).unwrap_or_else(|_| default_tool_info(&name))
            } else {
                default_tool_info(&name)
            };

            Ok((module, name, tool_info))
        })
        .await??;

        let meta = WasmPluginMeta {
            name: name.clone(),
            version: "0.1.0".to_string(),
            description: format!("WASM plugin: {}", name),
            path: path.to_path_buf(),
        };

        let plugin = WasmPlugin {
            meta,
            tool_info,
            module,
        };

        self.plugins.insert(name.clone(), plugin);
        Ok(name)
    }

    /// Execute a tool call on a WASM plugin via wasmtime.
    pub async fn execute_tool(
        &self,
        plugin_name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let plugin = self
            .plugins
            .get(plugin_name)
            .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", plugin_name))?;

        let args_json = serde_json::to_string(args)?;
        let engine = self.engine.clone();
        let module = plugin.module.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<String> {
            let mut store: Store<()> = Store::new(&engine, ());

            // Instantiate the module
            let instance = Instance::new(&mut store, &module, &[])?;

            // Call handle_tool_call(args_ptr, args_len) -> (result_ptr, result_len)
            let func = instance
                .get_typed_func::<(u32, u32), (u32, u32)>(&mut store, "handle_tool_call")
                .map_err(|_| anyhow::anyhow!("Plugin missing handle_tool_call export"))?;

            // Write args into memory
            let args_bytes = args_json.as_bytes();
            let memory = instance
                .get_memory(&mut store, "memory")
                .ok_or_else(|| anyhow::anyhow!("Plugin missing memory export"))?;

            // Allocate space for args at end of memory
            let mem_size = memory.data_size(&store);
            let args_ptr = mem_size as u32;
            memory.grow(&mut store, ((args_bytes.len() / 65536) + 1) as u64)?;
            memory.data_mut(&mut store)[args_ptr as usize..args_ptr as usize + args_bytes.len()]
                .copy_from_slice(args_bytes);

            let (result_ptr, result_len) = func.call(&mut store, (args_ptr, args_bytes.len() as u32))?;
            let result_str = read_memory_string(&mut store, &instance, result_ptr, result_len);

            Ok(result_str)
        })
        .await??;

        let parsed: serde_json::Value = serde_json::from_str(&result)
            .map_err(|e| anyhow::anyhow!("Plugin returned invalid JSON: {} - {}", e, &result[..result.len().min(200)]))?;
        Ok(parsed)
    }

    /// List loaded plugin names.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// Get plugin metadata.
    pub fn get_plugin_meta(&self, name: &str) -> Option<&WasmPluginMeta> {
        self.plugins.get(name).map(|p| &p.meta)
    }

    /// Remove a plugin.
    pub fn unload_plugin(&mut self, name: &str) -> bool {
        self.plugins.remove(name).is_some()
    }
}

impl Default for WasmPluginHost {
    fn default() -> Self {
        Self::new().expect("Failed to create WasmPluginHost")
    }
}

fn default_tool_info(name: &str) -> ToolInfo {
    ToolInfo {
        name: name.to_string(),
        description: format!("WASM plugin: {}", name),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "args": { "type": "object", "description": "Plugin arguments" }
            }
        }),
    }
}

/// Read a string from instance memory at (ptr, len).
fn read_memory_string<T>(
    store: &mut Store<T>,
    instance: &Instance,
    ptr: u32,
    len: u32,
) -> String {
    let memory = instance
        .get_memory(&mut *store, "memory")
        .expect("WASM module missing memory export");
    let data = &memory.data(store)[ptr as usize..(ptr + len) as usize];
    String::from_utf8_lossy(data).into_owned()
}

/// A single WASM tool (wraps a plugin reference).
#[derive(Clone)]
struct WasmTool {
    info: ToolInfo,
    host: Arc<RwLock<WasmPluginHost>>,
}

#[async_trait]
impl agentic_loop_tools::Tool for WasmTool {
    async fn execute(&self, args: &[u8]) -> Result<Vec<u8>> {
        let args_val: serde_json::Value = serde_json::from_slice(args)?;
        let host = self.host.read().await;
        let result = host.execute_tool(&self.info.name, &args_val).await?;
        Ok(serde_json::to_vec(&result)?)
    }

    fn info(&self) -> ToolInfo {
        self.info.clone()
    }

    agentic_loop_tools::impl_clone_box!(WasmTool);
}

/// ToolProvider implementation that wraps WASM plugins.
pub struct WasmToolProvider {
    host: Arc<RwLock<WasmPluginHost>>,
}

impl WasmToolProvider {
    pub fn new(host: WasmPluginHost) -> Result<Self> {
        Ok(Self {
            host: Arc::new(RwLock::new(host)),
        })
    }

    /// Create an empty provider.
    pub fn empty() -> Result<Self> {
        Self::new(WasmPluginHost::new()?)
    }
}

#[async_trait]
impl ToolProvider for WasmToolProvider {
    async fn list_tools(&self) -> Vec<ToolInfo> {
        let host = self.host.read().await;
        host.plugins
            .values()
            .map(|p| p.tool_info.clone())
            .collect()
    }

    async fn get_tool(&self, name: &str) -> Option<Box<dyn agentic_loop_tools::Tool>> {
        let host = self.host.read().await;
        if host.plugins.contains_key(name) {
            Some(Box::new(WasmTool {
                info: host.plugins[name].tool_info.clone(),
                host: self.host.clone(),
            }))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_host() {
        let host = WasmPluginHost::new().unwrap();
        assert!(host.plugin_names().is_empty());
    }

    #[tokio::test]
    async fn test_scan_nonexistent_directory() {
        let mut host = WasmPluginHost::new().unwrap();
        let count = host
            .scan_directory(Path::new("/nonexistent"))
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_scan_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = WasmPluginHost::new().unwrap();
        let count = host.scan_directory(dir.path()).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_wasm_tool_provider_empty() {
        let provider = WasmToolProvider::empty().unwrap();
        let tools = provider.list_tools().await;
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_execute_missing_plugin() {
        let host = WasmPluginHost::new().unwrap();
        let result = host
            .execute_tool("nonexistent", &serde_json::json!({}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_load_invalid_wasm() {
        let dir = tempfile::tempdir().unwrap();
        let bad_wasm = dir.path().join("bad.wasm");
        tokio::fs::write(&bad_wasm, b"not valid wasm bytes").await.unwrap();

        let mut host = WasmPluginHost::new().unwrap();
        let result = host.load_plugin(&bad_wasm).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_host_default() {
        let host = WasmPluginHost::default();
        assert!(host.plugin_names().is_empty());
    }

    #[tokio::test]
    async fn test_unload_nonexistent() {
        let mut host = WasmPluginHost::new().unwrap();
        assert!(!host.unload_plugin("nope"));
    }
}
