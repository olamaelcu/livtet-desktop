//! Sum up this device's sync state for the settings page.
//!
//! The daemon reaches peers over iroh: a UDP endpoint, mDNS discovery, and
//! relay-assisted NAT traversal. There is no server to start, no bind address,
//! and no served-request log, so the only things a reader can act on are the
//! pairings this device holds and which direction each one points.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::commands::sync::{SyncDevice, SyncStatusInfo};

/// Everything one diagnosis collects.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncDiagnostics {
    /// Stable device id this daemon reads from the database.
    pub device_id: String,
    /// The iroh endpoint id peers dial; no address or forwarded port needed.
    pub node_id: String,
    /// UDP port the endpoint bound; informational, never dialled directly.
    pub port: u16,
    /// Rows in `paired_devices`.
    pub paired_device_count: i32,
    /// Pairing claims heard but not yet approved or rejected.
    pub pending_pairing_count: i32,
    /// Paired devices, as the daemon lists them.
    pub peers: Vec<SyncDevice>,
    /// Plain-language verdicts, most actionable first.
    pub findings: Vec<String>,
}

/// The inputs the verdict is computed from.
///
/// Split out from collection so the reasoning is testable without a daemon:
/// what a one-way pairing *means* is the part worth pinning.
pub(crate) struct Evidence<'a> {
    pub paired_device_count: i32,
    pub pending_pairing_count: i32,
    pub peers: &'a [SyncDevice],
}

/// Turn the daemon's status into the sentences a reader can act on.
///
/// Ordered by what blocks first: a claim needs a decision before that device
/// can get anywhere; an empty pairing list is the next thing to fix; a pairing
/// that points only one way can receive but not send.
pub(crate) fn diagnose(evidence: &Evidence<'_>) -> Vec<String> {
    let mut findings = Vec::new();

    if evidence.pending_pairing_count > 0 {
        findings.push(format!(
            "{} pairing request(s) are waiting for approval here. Approve one before that device \
             can pull anything.",
            evidence.pending_pairing_count
        ));
    }

    if evidence.peers.is_empty() {
        findings.push(
            "No device is paired yet. Send a pairing claim to a nearby peer, then approve it on \
             that device."
                .to_string(),
        );
    } else {
        findings.push(format!(
            "{} device(s) paired. The daemon pulls from each one on its own interval.",
            evidence.paired_device_count
        ));

        let inbound_only = evidence.peers.iter().filter(|peer| !peer.outbound).count();
        if inbound_only > 0 {
            findings.push(format!(
                "{inbound_only} of them only approved this device, so they can pull from here but \
                 not the other way. Claim each back from Nearby devices to receive their books."
            ));
        }
    }

    findings
}

/// Collect a full diagnosis from the daemon's status and paired devices.
pub fn collect(status: &SyncStatusInfo, peers: Vec<SyncDevice>) -> SyncDiagnostics {
    let port = u16::try_from(status.port).unwrap_or_default();
    let findings = diagnose(&Evidence {
        paired_device_count: status.paired_device_count,
        pending_pairing_count: status.pending_pairing_count,
        peers: &peers,
    });

    SyncDiagnostics {
        device_id: status.device_id.clone(),
        node_id: status.node_id.clone(),
        port,
        paired_device_count: status.paired_device_count,
        pending_pairing_count: status.pending_pairing_count,
        peers,
        findings,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{Evidence, diagnose};
    use crate::commands::sync::SyncDevice;

    fn device(name: &str, outbound: bool) -> SyncDevice {
        SyncDevice {
            device_id: "01M652BF35STC9CWKPF4WKDAZV".to_string(),
            name: Some(name.to_string()),
            listen_on: None,
            device_type_id: Some("desktop".to_string()),
            paired_at: "2026-11-01 13:56:05".to_string(),
            last_sync_at: Some("2026-11-01 17:50:00".to_string()),
            outbound,
        }
    }

    fn joined(findings: &[String]) -> String {
        findings.join("\n")
    }

    /// A claim nobody answered is the first thing to fix, so it leads.
    #[test]
    fn a_pending_claim_is_reported_before_anything_else() {
        let findings = diagnose(&Evidence {
            paired_device_count: 0,
            pending_pairing_count: 2,
            peers: &[],
        });

        assert!(
            findings[0].contains("2 pairing request(s)"),
            "a pending claim must lead: {findings:?}"
        );
    }

    #[test]
    fn an_empty_pairing_list_says_to_claim_a_peer() {
        let findings = joined(&diagnose(&Evidence {
            paired_device_count: 0,
            pending_pairing_count: 0,
            peers: &[],
        }));

        assert!(findings.contains("No device is paired yet"), "{findings}");
    }

    /// A one-way pairing can pull from here but cannot be pulled from; the
    /// reader has to know to claim it back.
    #[test]
    fn a_one_way_pairing_says_to_claim_it_back() {
        let peers = [device("Laptop", false), device("Phone", true)];
        let findings = joined(&diagnose(&Evidence {
            paired_device_count: 2,
            pending_pairing_count: 0,
            peers: &peers,
        }));

        assert!(findings.contains("1 of them only approved"), "{findings}");
    }

    /// Both directions claimed means nothing to fix about the pairing.
    #[test]
    fn a_mutual_pairing_reports_no_direction_problem() {
        let peers = [device("Laptop", true)];
        let findings = joined(&diagnose(&Evidence {
            paired_device_count: 1,
            pending_pairing_count: 0,
            peers: &peers,
        }));

        assert!(!findings.contains("only approved"), "{findings}");
        assert!(findings.contains("1 device(s) paired"), "{findings}");
    }
}
