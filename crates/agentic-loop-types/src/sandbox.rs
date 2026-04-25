//! Sandbox trait definitions and types for secure tool execution.
//!
//! The sandbox system controls what operations tools can perform:
//! - File system access (read/write/traverse)
//! - Command execution (allowlisted commands, argument validation)
//! - Network access (outbound connections, DNS)
//! - Resource limits (memory, CPU, time)
//!
//! Levels: None (no restrictions) → Standard (safe defaults) → Strict (minimal access)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── Sandbox trait ────────────────────────────────────────────────────────────

/// Sandbox controlling tool execution permissions.
///
/// Implementations:
/// - `ApplicationSandbox` — command allowlist, path traversal checks, network gating
/// - `LandlockSandbox` — Linux Landlock LSM (Phase 4)
/// - `SandboxExecutorSandbox` — macOS sandbox-exec (Phase 5)
/// - `JobObjectSandbox` — Windows Job Objects (Phase 6)
pub trait Sandbox: Send + Sync {
    /// Name of this sandbox implementation.
    fn name(&self) -> &str;

    /// Check if a file read is allowed.
    fn can_read(&self, path: &std::path::Path) -> bool;

    /// Check if a file write is allowed.
    fn can_write(&self, path: &std::path::Path) -> bool;

    /// Check if a command execution is allowed.
    fn can_execute(&self, command: &str, args: &[String]) -> bool;

    /// Check if a network connection to the given host:port is allowed.
    fn can_connect(&self, host: &str, port: u16) -> bool;

    /// Check if an environment variable can be read.
    fn can_read_env(&self, key: &str) -> bool;

    /// Get the sandbox level.
    fn level(&self) -> SandboxLevel;

    /// Get the current permission set.
    fn permissions(&self) -> &PermissionSet;

    /// Validate and potentially modify a command before execution.
    /// Returns None if the command is denied, or Some(modified_args) if allowed.
    fn validate_command(&self, command: &str, args: &[String]) -> Option<Vec<String>>;

    /// Get the working directory prefix (all file ops must be under this).
    fn workspace_root(&self) -> Option<&std::path::Path>;
}

// ─── Sandbox level ────────────────────────────────────────────────────────────

/// How restrictive the sandbox is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SandboxLevel {
    /// No restrictions. All operations allowed.
    #[default]
    None,
    /// Safe defaults: workspace-only file access, allowlisted commands, no raw network.
    Standard,
    /// Minimal access: explicit allowlist for everything.
    Strict,
}

// ─── Permission set ───────────────────────────────────────────────────────────

/// Defines what operations are permitted within the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSet {
    /// Allowed read paths (globs). Empty = all allowed.
    #[serde(default)]
    pub read_paths: Vec<String>,

    /// Allowed write paths (globs). Empty = all allowed.
    #[serde(default)]
    pub write_paths: Vec<String>,

    /// Denied paths (takes precedence over allowed).
    #[serde(default)]
    pub denied_paths: Vec<String>,

    /// Allowed commands (empty = all allowed in Standard, none in Strict).
    #[serde(default)]
    pub allowed_commands: Vec<String>,

    /// Denied commands (takes precedence over allowed).
    #[serde(default)]
    pub denied_commands: Vec<String>,

    /// Allowed network hosts (empty = all allowed in Standard, none in Strict).
    #[serde(default)]
    pub allowed_hosts: Vec<String>,

    /// Whether environment variables can be read.
    #[serde(default = "default_true")]
    pub allow_env: bool,

    /// Maximum command execution time in seconds.
    #[serde(default)]
    pub max_exec_time_secs: Option<u64>,

    /// Maximum output size in bytes.
    #[serde(default)]
    pub max_output_bytes: Option<usize>,
}

impl Default for PermissionSet {
    fn default() -> Self {
        Self {
            read_paths: Vec::new(),
            write_paths: Vec::new(),
            denied_paths: vec![
                "/etc/shadow".into(),
                "/etc/passwd".into(),
                "**/.ssh/**".into(),
                "**/.gnupg/**".into(),
                "**/.env".into(),
            ],
            allowed_commands: Vec::new(),
            denied_commands: vec![
                "rm".into(), "rmdir".into(), "mkfs".into(), "dd".into(),
                "chmod".into(), "chown".into(), "sudo".into(), "su".into(),
                "curl".into(), "wget".into(), "nc".into(), "ncat".into(),
            ],
            allowed_hosts: Vec::new(),
            allow_env: true,
            max_exec_time_secs: Some(120),
            max_output_bytes: Some(1_048_576),
        }
    }
}

impl PermissionSet {
    /// Standard permissions: workspace-only file ops, safe commands.
    pub fn standard(workspace: &str) -> Self {
        Self {
            read_paths: vec![workspace.to_string(), "/tmp".into()],
            write_paths: vec![workspace.to_string(), "/tmp".into()],
            denied_paths: vec![
                "**/.ssh/**".into(),
                "**/.gnupg/**".into(),
                "**/.env".into(),
                "**/.git/**".into(),
            ],
            allowed_commands: vec![
                "ls".into(), "cat".into(), "head".into(), "tail".into(),
                "grep".into(), "find".into(), "wc".into(), "sort".into(),
                "echo".into(), "mkdir".into(), "cp".into(), "mv".into(),
                "git".into(), "cargo".into(), "rustc".into(),
                "node".into(), "npm".into(), "python3".into(),
                "test".into(), "diff".into(),
            ],
            denied_commands: vec![
                "rm".into(), "rmdir".into(), "mkfs".into(), "dd".into(),
                "chmod".into(), "chown".into(), "sudo".into(), "su".into(),
            ],
            allowed_hosts: vec![], // No raw network in standard
            allow_env: false,
            max_exec_time_secs: Some(60),
            max_output_bytes: Some(512 * 1024),
        }
    }

    /// Strict permissions: minimal access, explicit allowlist only.
    pub fn strict(workspace: &str) -> Self {
        Self {
            read_paths: vec![workspace.to_string()],
            write_paths: vec![workspace.to_string()],
            denied_paths: vec!["**".into()],
            allowed_commands: vec!["ls".into(), "cat".into(), "echo".into()],
            denied_commands: vec!["**".into()],
            allowed_hosts: vec![],
            allow_env: false,
            max_exec_time_secs: Some(30),
            max_output_bytes: Some(64 * 1024),
        }
    }

    /// No restrictions.
    pub fn none() -> Self {
        Self {
            read_paths: vec![],
            write_paths: vec![],
            denied_paths: vec![],
            allowed_commands: vec![],
            denied_commands: vec![],
            allowed_hosts: vec![],
            allow_env: true,
            max_exec_time_secs: None,
            max_output_bytes: None,
        }
    }
}

fn default_true() -> bool { true }

// ─── Sandbox config ───────────────────────────────────────────────────────────

/// Configuration for sandbox creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Sandbox level.
    #[serde(default)]
    pub level: SandboxLevel,

    /// Workspace root directory (all file ops scoped to this).
    #[serde(default)]
    pub workspace_root: Option<PathBuf>,

    /// Custom permission set (overrides level defaults).
    #[serde(default)]
    pub permissions: Option<PermissionSet>,

    /// Whether to enforce resource limits.
    #[serde(default = "default_true")]
    pub enforce_limits: bool,

    /// Log all sandbox decisions for audit.
    #[serde(default)]
    pub audit_log: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            level: SandboxLevel::None,
            workspace_root: None,
            permissions: None,
            enforce_limits: true,
            audit_log: false,
        }
    }
}

impl SandboxConfig {
    /// Standard sandbox scoped to a workspace.
    pub fn standard(workspace: impl Into<PathBuf>) -> Self {
        Self {
            level: SandboxLevel::Standard,
            workspace_root: Some(workspace.into()),
            permissions: None, // Will use standard defaults based on level
            enforce_limits: true,
            audit_log: false,
        }
    }

    /// Strict sandbox scoped to a workspace.
    pub fn strict(workspace: impl Into<PathBuf>) -> Self {
        Self {
            level: SandboxLevel::Strict,
            workspace_root: Some(workspace.into()),
            permissions: None,
            enforce_limits: true,
            audit_log: true,
        }
    }

    /// Resolve effective permissions based on level.
    pub fn effective_permissions(&self) -> PermissionSet {
        if let Some(ref perms) = self.permissions {
            return perms.clone();
        }

        let workspace = self.workspace_root
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());

        match self.level {
            SandboxLevel::None => PermissionSet::none(),
            SandboxLevel::Standard => PermissionSet::standard(&workspace),
            SandboxLevel::Strict => PermissionSet::strict(&workspace),
        }
    }
}

// ─── Sandbox error ────────────────────────────────────────────────────────────

/// Error from a sandbox denial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxDenial {
    /// What operation was denied.
    pub operation: String,
    /// Why it was denied.
    pub reason: String,
    /// The target of the operation (path, command, host).
    pub target: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_level_default() {
        assert_eq!(SandboxLevel::default(), SandboxLevel::None);
    }

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::default();
        assert_eq!(config.level, SandboxLevel::None);
        assert!(config.workspace_root.is_none());
    }

    #[test]
    fn test_permission_set_default_denies_dangerous() {
        let perms = PermissionSet::default();
        assert!(perms.denied_commands.contains(&"sudo".to_string()));
        assert!(perms.denied_commands.contains(&"rm".to_string()));
        assert!(perms.denied_paths.contains(&"**/.ssh/**".to_string()));
    }

    #[test]
    fn test_standard_permissions() {
        let perms = PermissionSet::standard("/workspace");
        assert!(perms.allowed_commands.contains(&"git".to_string()));
        assert!(perms.allowed_commands.contains(&"cargo".to_string()));
        assert!(!perms.allow_env);
    }

    #[test]
    fn test_strict_permissions() {
        let perms = PermissionSet::strict("/workspace");
        assert_eq!(perms.allowed_commands.len(), 3);
        assert!(!perms.allow_env);
        assert_eq!(perms.max_exec_time_secs, Some(30));
    }

    #[test]
    fn test_none_permissions() {
        let perms = PermissionSet::none();
        assert!(perms.read_paths.is_empty());
        assert!(perms.denied_commands.is_empty());
        assert!(perms.allow_env);
        assert!(perms.max_exec_time_secs.is_none());
    }

    #[test]
    fn test_effective_permissions_none() {
        let config = SandboxConfig::default();
        let perms = config.effective_permissions();
        assert!(perms.read_paths.is_empty());
    }

    #[test]
    fn test_effective_permissions_standard() {
        let config = SandboxConfig::standard("/workspace");
        let perms = config.effective_permissions();
        assert!(perms.allowed_commands.contains(&"git".to_string()));
    }

    #[test]
    fn test_effective_permissions_custom_override() {
        let custom = PermissionSet {
            allowed_commands: vec!["my-cmd".into()],
            ..Default::default()
        };
        let config = SandboxConfig {
            level: SandboxLevel::Standard,
            permissions: Some(custom),
            ..Default::default()
        };
        let perms = config.effective_permissions();
        assert_eq!(perms.allowed_commands, vec!["my-cmd"]);
    }

    #[test]
    fn test_sandbox_denial() {
        let denial = SandboxDenial {
            operation: "execute".into(),
            reason: "Command not in allowlist".into(),
            target: "rm -rf /".into(),
        };
        assert_eq!(denial.operation, "execute");
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = SandboxConfig::standard("/tmp/ws");
        let json = serde_json::to_string(&config).unwrap();
        let parsed: SandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.level, SandboxLevel::Standard);
        assert_eq!(parsed.workspace_root, Some(PathBuf::from("/tmp/ws")));
    }
}
