//! Opening this device's sync port in the host firewall.
//!
//! `ufw`'s default `deny (incoming)` policy drops every new inbound TCP
//! connection, which is exactly the direction a peer needs to claim this
//! device and pull from it. Nothing in the app can observe that from the
//! outside — a firewalled port is indistinguishable from an absent one — so
//! this module reads the host's own configuration instead and offers the one
//! command that fixes it.
//!
//! The rule needs root. Rather than pre-authorise anything at install time,
//! the app hands the command to `pkexec`, which is the desktop's own
//! authentication surface: the user sees polkit's prompt and either approves
//! that command or does not. There is no Tauri API for firewalls, and no
//! portable one at the OS level.
//!
//! Only Linux is handled. macOS's application firewall is off by default,
//! prompts per app rather than per port, and has no scriptable equivalent; a
//! Windows network profile is out of scope for this desktop.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::SyncError;

/// A detected firewall that would drop a peer's inbound connection, and what
/// to run about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncFirewallHint {
    /// `ufw` or `firewalld`; the tool whose rules apply.
    pub tool: String,
    /// The port the sync server bound.
    pub port: i32,
    /// The command, with `sudo` in front, for a reader who would rather run it
    /// themselves or is on a machine where the app cannot elevate.
    pub command: String,
    /// Whether the app can run it: false when `pkexec` is missing or the app is
    /// sandboxed, where the command is the whole remedy.
    pub automatic: bool,
}

/// The firewall tools this module can reason about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Ufw,
    Firewalld,
}

impl Tool {
    /// The tool's executable, if it is installed where a packaged build puts it.
    fn executable(self) -> Option<&'static str> {
        let candidates: &[&str] = match self {
            Tool::Ufw => &["/usr/sbin/ufw", "/usr/bin/ufw", "/sbin/ufw"],
            Tool::Firewalld => &["/usr/bin/firewall-cmd", "/usr/sbin/firewall-cmd"],
        };
        candidates
            .iter()
            .copied()
            .find(|path| std::path::Path::new(path).exists())
    }

    fn name(self) -> &'static str {
        match self {
            Tool::Ufw => "ufw",
            Tool::Firewalld => "firewalld",
        }
    }

    /// The arguments that let `port` through, as the tool understands them.
    ///
    /// ufw rules outlive the process, which is what we want: the remembered
    /// port is stable, so the rule keeps matching. firewalld's runtime rules do
    /// not, and `--permanent` alone would not take effect until a reload, so
    /// this adds a runtime rule and the hint says it lasts until firewalld is
    /// reloaded or the machine reboots.
    fn args(self, path: &str, port: u16) -> Vec<String> {
        match self {
            Tool::Ufw => vec![path.to_string(), "allow".to_string(), format!("{port}/tcp")],
            Tool::Firewalld => vec![path.to_string(), format!("--add-port={port}/tcp")],
        }
    }
}

/// Whether `pkexec` is installed, so the command can be offered as automatic.
fn pkexec() -> Option<&'static str> {
    ["/usr/bin/pkexec", "/usr/local/bin/pkexec"]
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
}

/// A sandboxed build cannot reach the host's polkit agent, so elevation has to
/// be the reader's move. Flatpak and Snap both advertise themselves this way.
fn sandboxed() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
        || std::env::var_os("SNAP").is_some()
        || std::path::Path::new("/.flatpak-info").exists()
}

/// Whether a ufw configuration enables the firewall.
///
/// `/etc/ufw/ufw.conf` is world-readable, so this needs no elevation. A missing
/// `ENABLED` line means ufw is installed but off, which is not a reason to
/// prompt anyone.
fn ufw_enabled(contents: &str) -> bool {
    contents.lines().any(|line| {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        key.trim().eq_ignore_ascii_case("ENABLED") && value.trim().eq_ignore_ascii_case("yes")
    })
}

/// The firewall that would drop a peer's connection on `port`, if any.
///
/// Returns `None` on every platform this module does not handle, and on Linux
/// when no firewall is enabled — silence is the normal case and must not be
/// reported as a problem.
#[cfg(target_os = "linux")]
pub fn detect(port: u16) -> Option<SyncFirewallHint> {
    let tool = ufw_configuration().or_else(firewalld_configuration)?;
    let path = tool.executable()?;
    let automatic = !sandboxed() && pkexec().is_some();
    let args = tool.args(path, port);
    Some(SyncFirewallHint {
        tool: tool.name().to_string(),
        port: i32::from(port),
        command: format!("sudo {}", shell_join(&args)),
        automatic,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn detect(_port: u16) -> Option<SyncFirewallHint> {
    None
}

#[cfg(target_os = "linux")]
fn ufw_configuration() -> Option<Tool> {
    let contents = std::fs::read_to_string("/etc/ufw/ufw.conf").ok()?;
    ufw_enabled(&contents).then_some(Tool::Ufw)
}

/// firewalld keeps no readable "enabled" flag; its daemon is the state.
#[cfg(target_os = "linux")]
fn firewalld_configuration() -> Option<Tool> {
    let tool = Tool::Firewalld;
    let path = tool.executable()?;
    let output = std::process::Command::new(path)
        .arg("--state")
        .output()
        .ok()?;
    (output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "running")
        .then_some(tool)
}

/// Run the detected tool's allow command under `pkexec`.
///
/// The exit status is the answer: ufw reports "Rule added" on an add and
/// "Skipping adding existing rule" on a repeat, both success, so this is
/// idempotent. Anything else is surfaced with the tool's own stderr, which is
/// more useful than a generic failure.
#[cfg(target_os = "linux")]
pub fn allow(port: u16) -> Result<(), String> {
    let tool = ufw_configuration()
        .or_else(firewalld_configuration)
        .ok_or_else(|| {
            "no firewall is enabled on this machine, so there is no rule to add".to_string()
        })?;
    let path = tool.executable().ok_or_else(|| {
        format!(
            "{} is enabled but its command is not installed",
            tool.name()
        )
    })?;
    if sandboxed() {
        return Err("this build is sandboxed, so the rule has to be added outside it".to_string());
    }
    let elevation = pkexec().ok_or_else(|| {
        "pkexec is not installed; run the command shown in the panel instead".to_string()
    })?;

    let output = std::process::Command::new(elevation)
        .args(tool.args(path, port))
        .output()
        .map_err(|error| format!("could not ask for permission: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("{} exited with {}", tool.name(), output.status)
    } else {
        stderr
    })
}

#[cfg(not(target_os = "linux"))]
pub fn allow(_port: u16) -> Result<(), String> {
    Err("this platform has no firewall rule to add".to_string())
}

/// Quote an argv for display, so a copy-paste works even for an odd argument.
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/-_=.,+:".contains(c))
            {
                arg.clone()
            } else {
                format!("'{}'", arg.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn ufw_counts_as_enabled_only_when_the_flag_says_yes() {
        assert!(ufw_enabled(
            "IPV6=yes\nENABLED=yes\nDEFAULT_INPUT_POLICY=\"DROP\"\n"
        ));
        assert!(ufw_enabled("  enabled = YES  \n"));
    }

    #[test]
    fn an_installed_but_disabled_ufw_is_not_a_problem_to_report() {
        assert!(!ufw_enabled("ENABLED=no\n"));
        assert!(!ufw_enabled("IPV6=yes\n"));
        assert!(!ufw_enabled(""));
    }

    #[test]
    fn ufw_rule_is_an_allow_for_the_given_port() {
        let args = Tool::Ufw.args("/usr/sbin/ufw", 7710);

        assert_eq!(args, ["/usr/sbin/ufw", "allow", "7710/tcp"]);
    }

    #[test]
    fn firewalld_rule_names_the_port_in_its_own_syntax() {
        let args = Tool::Firewalld.args("/usr/bin/firewall-cmd", 7710);

        assert_eq!(args, ["/usr/bin/firewall-cmd", "--add-port=7710/tcp"]);
    }

    #[test]
    fn the_displayed_command_is_the_one_that_would_run() {
        let args = Tool::Ufw.args("/usr/sbin/ufw", 7710);

        assert_eq!(shell_join(&args), "/usr/sbin/ufw allow 7710/tcp");
    }

    #[test]
    fn arguments_with_spaces_stay_one_argument_when_displayed() {
        let args = vec!["cmd".to_string(), "two words".to_string()];

        assert_eq!(shell_join(&args), "cmd 'two words'");
    }

    /// The normal case: no firewall configured means no callout in the panel.
    /// This machine has ufw unconfigured or no /etc/ufw/ufw.conf, and either
    /// way detection must not invent a hint.
    #[test]
    fn a_machine_without_an_enabled_firewall_gets_no_hint() {
        if cfg!(target_os = "linux") && std::path::Path::new("/etc/ufw/ufw.conf").exists() {
            return;
        }
        assert_eq!(detect(7710), None);
    }
}

/// Validate the port the panel passes in: it comes from the daemon's reported
/// bind, and an out-of-range value would produce a rule that matches nothing.
fn checked_port(port: i32) -> Result<u16, SyncError> {
    u16::try_from(port)
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| SyncError::rpc(format!("invalid sync port: {port}")))
}

/// The firewall rule this device would need for a peer to reach `port`.
///
/// `null` is the common answer — no firewall, or one that is not enabled — and
/// means the panel has nothing to say.
#[tauri::command]
#[specta::specta]
pub async fn sync_firewall_hint(port: i32) -> Result<Option<SyncFirewallHint>, SyncError> {
    let port = checked_port(port)?;
    tokio::task::spawn_blocking(move || detect(port))
        .await
        .map_err(SyncError::rpc)
}

/// Add the detected rule, letting the desktop's polkit agent ask the reader.
///
/// Returns the tool's own failure text on refusal, which is what tells the
/// reader whether they cancelled the prompt or the rule was rejected.
#[tauri::command]
#[specta::specta]
pub async fn sync_firewall_allow(port: i32) -> Result<(), SyncError> {
    let port = checked_port(port)?;
    tokio::task::spawn_blocking(move || allow(port))
        .await
        .map_err(SyncError::rpc)?
        .map_err(SyncError::rpc)
}
