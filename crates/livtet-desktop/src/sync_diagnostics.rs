//! Why a peer cannot reach this device's sync server.
//!
//! Pairing is mutual, so both devices pull from each other — but a pull is
//! still an *outbound* HTTP request, and the peer's pull towards this device is
//! an inbound one this device has to be able to answer. "Their books arrive,
//! mine do not" is therefore never one symptom: it is either a socket the peer
//! cannot open (bind address, firewall), or a door that opens onto a refusal (a
//! token this device no longer knows, or a pairing approved before it was
//! mutual). Both look identical from the settings page, which is why this
//! module probes and reports rather than guessing.
//!
//! Every probe dials the loopback or LAN address of *this* device's own server
//! with a real HTTP client: reachability is what the socket says, not what the
//! configuration claims.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::commands::sync::{SyncDevice, SyncRequestRecord, SyncStatusInfo};

/// How long a probe waits before calling the address unreachable.
///
/// A peer on the same network answers in microseconds; a longer wait only
/// delays the diagnosis, and a firewall that drops packets looks the same at
/// two seconds as at thirty.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// The `x-device-id` a probe sends, so its own requests can be told apart from
/// a peer's in this device's served-request log.
///
/// Without this the probes answer themselves: the second diagnosis would see
/// the first one's 401s and report that a peer is being refused, which is the
/// opposite of the truth.
const PROBE_DEVICE_ID: &str = "livtet-diagnostics";

/// The address a probe dialled, and what came back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncProbe {
    /// `host:port` dialled, so the reader can see which interface was tested.
    pub target: String,
    /// The status the server answered with; `None` when nothing answered.
    pub status: Option<u16>,
    /// Why no answer came, when none did.
    pub error: Option<String>,
}

impl SyncProbe {
    /// Whether the probe reached a server, whatever it answered.
    fn answered(&self) -> bool {
        self.status.is_some()
    }

    /// Whether the error reads as a refused connection, which means something
    /// is listening on that address — just not this server.
    fn refused(&self) -> bool {
        self.error
            .as_deref()
            .is_some_and(|error| error.contains("refused") || error.contains("Connection refused"))
    }
}

/// Everything one diagnosis collects.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncDiagnostics {
    /// Whether the embedded server claims to be serving.
    pub server_running: bool,
    /// The interface it bound (`0.0.0.0` when LAN sharing is on).
    pub host: String,
    /// The port it bound.
    pub port: u16,
    /// The loopback probe: proves the server itself answers.
    pub loopback: SyncProbe,
    /// The LAN probe: proves a peer could open the socket. Absent when this
    /// machine has no routable address.
    pub lan: Option<SyncProbe>,
    /// Paired devices, as the daemon lists them.
    pub peers: Vec<SyncDevice>,
    /// What this device served recently, newest first.
    pub inbound: Vec<SyncRequestRecord>,
    /// Plain-language verdicts, most actionable first.
    pub findings: Vec<String>,
}

/// The inputs the verdict is computed from.
///
/// Split out from collection so the reasoning is testable without sockets: what
/// a refused connection *means* is the part worth pinning, not that reqwest can
/// reach 127.0.0.1.
pub(crate) struct Evidence<'a> {
    pub server_running: bool,
    pub host: &'a str,
    pub loopback: &'a SyncProbe,
    pub lan: Option<&'a SyncProbe>,
    pub peers: &'a [SyncDevice],
    pub inbound: &'a [SyncRequestRecord],
}

/// Turn the probes into the sentence a reader can act on.
///
/// Ordered by what blocks first: a stopped server cannot be reached from
/// anywhere, a loopback-only bind cannot be reached by a peer, and only then
/// does it matter whether the peer's requests arrive and how they were answered.
pub(crate) fn diagnose(evidence: &Evidence<'_>) -> Vec<String> {
    let mut findings = Vec::new();

    if !evidence.server_running {
        findings.push(
            "The sync server is stopped, so no device can reach this library. Start it above."
                .to_string(),
        );
    } else if evidence.host == "127.0.0.1" || evidence.host == "::1" {
        findings.push(
            "The server listens on loopback only, so a peer on the network cannot connect. \
             Turn LAN sharing on and restart the app."
                .to_string(),
        );
    } else if !evidence.loopback.answered() {
        findings.push(format!(
            "Nothing answered this device's own server at {} — it is not serving, whatever the \
             status says.",
            evidence.loopback.target
        ));
    }

    match evidence.lan {
        None => findings.push(
            "This machine has no routable network address, so no peer can dial it. Check that it \
             is on the network."
                .to_string(),
        ),
        Some(lan) if evidence.server_running && lan.answered() => findings.push(format!(
            "{} answers from the network ({}), so a peer can reach this device.",
            lan.target,
            lan.status.unwrap_or_default()
        )),
        Some(lan) if evidence.server_running && lan.refused() => findings.push(format!(
            "{} refused the connection, which means something else answers there or the server \
             bound a different interface.",
            lan.target
        )),
        Some(lan) if evidence.server_running => findings.push(format!(
            "{} never answered. A firewall, VLAN isolation, or client isolation on the access \
             point drops these packets; outbound pulls are unaffected by all three.",
            lan.target
        )),
        Some(_) => {}
    }

    let refusals = evidence
        .inbound
        .iter()
        .filter(|request| request.status == 401 || request.status == 403)
        .count();
    if !evidence.server_running {
        // Covered above; counting arrivals of a stopped server is noise.
    } else if evidence.inbound.is_empty() {
        findings.push(
            "No peer has connected since the daemon started. Their requests are not reaching this \
             port at all — that is the network half of the problem, not pairing."
                .to_string(),
        );
    } else if refusals == evidence.inbound.len() {
        findings.push(format!(
            "Every one of the last {} requests was refused. The peer reaches this device but its \
             token is not one of this device's pairings — pair again from the peer's side.",
            evidence.inbound.len()
        ));
    } else {
        let served = evidence
            .inbound
            .iter()
            .filter(|request| (200..300).contains(&request.status))
            .count();
        findings.push(format!(
            "{served} of the last {} requests were served, so this device is answering peers.",
            evidence.inbound.len()
        ));
    }

    if evidence.peers.is_empty() && evidence.server_running {
        findings.push(
            "No device is paired here yet, so there is no token for a peer to authenticate with."
                .to_string(),
        );
    }

    findings
}

/// The address a peer on this network would dial, when there is one.
///
/// Connecting a UDP socket sends nothing; it asks the kernel which local
/// address it would use to reach the internet.
fn lan_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let address = socket.local_addr().ok()?;
    match address.ip() {
        IpAddr::V4(ip) if ip.is_loopback() || ip.is_unspecified() => None,
        ip => Some(ip),
    }
}

/// Dial one address of this device's own server.
///
/// `/sync/status` is authenticated, so a `401` is a *successful* probe: it
/// proves the server is listening and speaking HTTP on that interface.
async fn probe(client: &reqwest::Client, target: String) -> SyncProbe {
    let url = format!("http://{target}/sync/status");
    match client
        .get(url)
        .header("x-device-id", PROBE_DEVICE_ID)
        .send()
        .await
    {
        Ok(response) => SyncProbe {
            target,
            status: Some(response.status().as_u16()),
            error: None,
        },
        Err(error) => SyncProbe {
            target,
            status: None,
            error: Some(one_line(&error.to_string())),
        },
    }
}

/// Transport errors carry the whole URL and a source chain; the first line is
/// what a reader can use.
fn one_line(message: &str) -> String {
    message.lines().next().unwrap_or(message).trim().to_string()
}

/// The served requests that came from a peer, not from these probes.
fn peer_requests(inbound: &[SyncRequestRecord]) -> Vec<SyncRequestRecord> {
    inbound
        .iter()
        .filter(|request| request.device_id.as_deref() != Some(PROBE_DEVICE_ID))
        .cloned()
        .collect()
}

/// Collect a full diagnosis: server state, two probes, pairings, served traffic.
pub async fn collect(
    status: &SyncStatusInfo,
    peers: Vec<SyncDevice>,
    inbound: Vec<SyncRequestRecord>,
) -> SyncDiagnostics {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        // A client that cannot be built is a broken TLS/dns stack, not a
        // diagnosis; report an empty probe rather than fail the command.
        .unwrap_or_default();

    // The daemon reports a port as an integer; anything outside `u16` is not a
    // port at all, so the probes below would be dialling nothing.
    let port = u16::try_from(status.port).unwrap_or_default();
    let loopback = probe(
        &client,
        SocketAddr::from(([127, 0, 0, 1], port)).to_string(),
    )
    .await;
    let lan = match lan_address() {
        // A LAN probe only means something when the server bound an interface
        // a peer can dial; the loopback probe already covers the rest.
        Some(ip) if status.server_running && status.host != "127.0.0.1" => {
            Some(probe(&client, SocketAddr::new(ip, port).to_string()).await)
        }
        Some(ip) => Some(SyncProbe {
            target: SocketAddr::new(ip, port).to_string(),
            status: None,
            // Not dialled: the server is not listening for peers.
            error: Some("not probed: the server is not accepting peers".to_string()),
        }),
        None => None,
    };

    let inbound = peer_requests(&inbound);
    let findings = diagnose(&Evidence {
        server_running: status.server_running,
        host: &status.host,
        loopback: &loopback,
        lan: lan.as_ref(),
        peers: &peers,
        inbound: &inbound,
    });

    SyncDiagnostics {
        server_running: status.server_running,
        host: status.host.clone(),
        port,
        loopback,
        lan,
        peers,
        inbound,
        findings,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{Evidence, SyncProbe, diagnose};
    use crate::commands::sync::{SyncDevice, SyncRequestRecord};

    fn probe(target: &str, status: Option<u16>, error: Option<&str>) -> SyncProbe {
        SyncProbe {
            target: target.to_string(),
            status,
            error: error.map(str::to_string),
        }
    }

    fn device(name: &str) -> SyncDevice {
        SyncDevice {
            device_id: "01M652BF35STC9CWKPF4WKDAZV".to_string(),
            name: Some(name.to_string()),
            listen_on: Some("192.168.1.226:7710".to_string()),
            device_type_id: Some("desktop".to_string()),
            paired_at: "2026-11-01 13:56:05".to_string(),
            last_sync_at: Some("2026-11-01 17:50:00".to_string()),
            outbound: true,
        }
    }

    fn served(status: i32) -> SyncRequestRecord {
        SyncRequestRecord {
            method: "GET".to_string(),
            path: "/sync/changes".to_string(),
            status,
            at: "2026-11-01 17:50:02".to_string(),
            device_id: Some("01M652BF35STC9CWKPF4WKDAZV".to_string()),
        }
    }

    fn findings_for<'a>(
        server_running: bool,
        host: &'a str,
        loopback: &'a SyncProbe,
        lan: Option<&'a SyncProbe>,
        peers: &'a [SyncDevice],
        inbound: &'a [SyncRequestRecord],
    ) -> Vec<String> {
        diagnose(&Evidence {
            server_running,
            host,
            loopback,
            lan,
            peers,
            inbound,
        })
    }

    fn joined(findings: &[String]) -> String {
        findings.join("\n")
    }

    /// A probe that answered itself would report its own 401 as a refused
    /// peer, which is the exact wrong advice.
    #[test]
    fn the_probes_own_requests_are_not_mistaken_for_a_peer() {
        let from_probe = SyncRequestRecord {
            method: "GET".to_string(),
            path: "/sync/status".to_string(),
            status: 401,
            at: "2026-11-01 17:50:02".to_string(),
            device_id: Some(super::PROBE_DEVICE_ID.to_string()),
        };
        let from_peer = served(401);

        let kept = super::peer_requests(&[from_probe, from_peer]);

        assert_eq!(kept.len(), 1, "only the peer's request survives");
        assert_eq!(
            kept[0].device_id.as_deref(),
            Some("01M652BF35STC9CWKPF4WKDAZV"),
            "the survivor is the peer's, not the probe's"
        );
    }

    #[test]
    fn a_stopped_server_is_the_first_thing_said() {
        let loopback = probe("127.0.0.1:7710", None, Some("connection refused"));
        let peers = [device("Jacky's MacBook Pro")];
        let findings = findings_for(false, "0.0.0.0", &loopback, None, &peers, &[]);

        assert!(findings[0].contains("stopped"), "{findings:?}");
    }

    /// A peer on another machine cannot dial 127.0.0.1, so a loopback bind is a
    /// complete answer even when the server is healthy.
    #[test]
    fn a_loopback_bind_explains_an_unreachable_peer() {
        let loopback = probe("127.0.0.1:7710", Some(401), None);
        let findings = findings_for(true, "127.0.0.1", &loopback, None, &[], &[]);

        assert!(
            joined(&findings).contains("loopback only"),
            "the bind is the cause, not the pairing: {findings:?}"
        );
    }

    #[test]
    fn a_lan_probe_that_answers_is_reported_as_reachable() {
        let loopback = probe("127.0.0.1:7710", Some(401), None);
        let lan = probe("192.168.1.137:7710", Some(401), None);
        let findings = findings_for(
            true,
            "0.0.0.0",
            &loopback,
            Some(&lan),
            &[device("Jacky's MacBook Pro")],
            &[served(200)],
        );

        assert!(
            joined(&findings).contains("answers from the network"),
            "{findings:?}"
        );
        assert!(
            joined(&findings).contains("were served"),
            "served traffic is evidence the door opens: {findings:?}"
        );
    }

    /// A firewall drops packets and looks like "never answered"; that is a
    /// different fix from "refused".
    #[test]
    fn silence_and_refusal_are_different_answers() {
        let loopback = probe("127.0.0.1:7710", Some(401), None);
        let silent = probe(
            "192.168.1.137:7710",
            None,
            Some("error sending request for url"),
        );
        let refused = probe(
            "192.168.1.137:7710",
            None,
            Some("tcp connect error: Connection refused"),
        );

        let silent_findings = findings_for(true, "0.0.0.0", &loopback, Some(&silent), &[], &[]);
        let refused_findings = findings_for(true, "0.0.0.0", &loopback, Some(&refused), &[], &[]);

        assert!(
            joined(&silent_findings).contains("firewall"),
            "{silent_findings:?}"
        );
        assert!(
            joined(&refused_findings).contains("refused the connection"),
            "{refused_findings:?}"
        );
    }

    /// The pairing half: requests arrive and are refused, which no amount of
    /// network fixes will change.
    #[test]
    fn refusals_point_at_the_pairing_not_the_network() {
        let loopback = probe("127.0.0.1:7710", Some(401), None);
        let lan = probe("192.168.1.137:7710", Some(401), None);
        let inbound = [served(401), served(401)];
        let findings = findings_for(
            true,
            "0.0.0.0",
            &loopback,
            Some(&lan),
            &[device("Jacky's MacBook Pro")],
            &inbound,
        );

        let text = joined(&findings);
        assert!(text.contains("refused"), "{findings:?}");
        assert!(text.contains("pair again"), "{findings:?}");
    }

    #[test]
    fn no_arrivals_at_all_is_reported_as_the_network_half() {
        let loopback = probe("127.0.0.1:7710", Some(401), None);
        let lan = probe("192.168.1.137:7710", Some(401), None);
        let findings = findings_for(
            true,
            "0.0.0.0",
            &loopback,
            Some(&lan),
            &[device("Jacky's MacBook Pro")],
            &[],
        );

        assert!(
            joined(&findings).contains("not reaching this port"),
            "{findings:?}"
        );
    }

    #[test]
    fn a_machine_without_a_routable_address_says_so() {
        let loopback = probe("127.0.0.1:7710", Some(401), None);
        let findings = findings_for(true, "0.0.0.0", &loopback, None, &[], &[]);

        assert!(
            joined(&findings).contains("no routable network address"),
            "{findings:?}"
        );
    }

    #[test]
    fn no_pairings_means_no_token_to_authenticate_with() {
        let loopback = probe("127.0.0.1:7710", Some(401), None);
        let lan = probe("192.168.1.137:7710", Some(401), None);
        let findings = findings_for(true, "0.0.0.0", &loopback, Some(&lan), &[], &[]);

        assert!(
            joined(&findings).contains("No device is paired"),
            "{findings:?}"
        );
    }
}
