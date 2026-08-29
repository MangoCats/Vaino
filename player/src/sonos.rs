//! Direct-to-Sonos output `[Sonos/SONOS008]`.
//!
//! The other output Vaino can choose, alongside a local device or a paired
//! Bluetooth speaker `[REQ-VIS-260]` -- shaped like `bluetooth.rs`
//! deliberately, not as a second design: a closed `Verb` enum, a `run`
//! entry point, and a persisted choice a listener made once.
//!
//! Unlike Bluetooth, there is no privileged helper here and nothing to
//! `sudo`: discovery is a plain SSDP multicast, and control is a plain HTTP
//! POST carrying a SOAP body -- the exact calls already validated live
//! against a real Sonos Play:1 pair while writing `Sonos/SONOS001`. A
//! speaker is identified by its RINCON (UDN), never by IP, the same
//! bind-by-identity-not-location principle `[SPEC-DF-035]` already applies
//! to a library file `[Sonos/SONOS008 §2]`.
//!
//! Only ever compiled in behind `sonos` `[Sonos/SONOS008 §9]`: an appliance
//! build that never asks for this carries none of the SSDP listener, the
//! SOAP client, or the encoder.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Standard UPnP discovery port; every ZonePlayer answers M-SEARCH here.
const SSDP_MULTICAST: &str = "239.255.255.250:1900";
/// The service every Sonos unit -- and only a Sonos unit -- advertises.
const SEARCH_TARGET: &str = "urn:schemas-upnp-org:device:ZonePlayer:1";
/// Every ZonePlayer's own control port, fixed since the first generation.
const SONOS_PORT: u16 = 1400;
/// Long enough for every unit on an ordinary home network to answer at
/// least once; short enough that "scan" in the settings panel does not
/// read as hung.
const DISCOVER_TIMEOUT: Duration = Duration::from_secs(3);

/// The verbs the web surface may invoke `[Sonos/SONOS008 §4]`. No `Pair` or
/// `Repair`: Sonos has no pairing step of its own the way Bluetooth does --
/// `Scan` finds what is already broadcasting, `Use` selects it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
    Scan,
    Use,
    Forget,
    Status,
}

impl Verb {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "scan" => Verb::Scan,
            "use" => Verb::Use,
            "forget" => Verb::Forget,
            "status" => Verb::Status,
            _ => return None,
        })
    }

    /// Does this verb name a speaker?
    pub fn needs_target(self) -> bool {
        matches!(self, Verb::Use)
    }
}

pub use crate::path::{SonosMember, SonosTarget};

/// One Sonos target, already resolved to its group coordinator
/// `[Sonos/SONOS008 §4]` -- a bonded stereo pair's satellite never appears
/// as its own entry here, so nothing downstream has to know pairs exist at
/// all; it appears instead, named, in `members` `[Sonos/SONOS010 §9]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SonosSpeaker {
    /// `RINCON_...` -- durable across an IP change, a reboot, a room rename.
    pub udn: String,
    /// The room name as Sonos itself reports it, at the moment of discovery.
    pub name: String,
    pub ip: IpAddr,
    /// Every unit in the group, coordinator included -- "Office" is one row
    /// to choose, but a listener asking "which two speakers is that" gets a
    /// real answer instead of a guess from the name.
    pub members: Vec<SonosMember>,
}

impl SonosSpeaker {
    /// The persisted shape -- `last_ip` named for what it actually is
    /// `[GDE-SONOS-760]`: a hint for logging and a first guess, never
    /// trusted for control without being re-confirmed against a fresh
    /// discovery.
    pub fn as_target(&self) -> SonosTarget {
        SonosTarget {
            udn: self.udn.clone(),
            name: self.name.clone(),
            last_ip: self.ip,
            members: self.members.clone(),
        }
    }
}

/// Run a verb against the discovered/persisted world. `Err` carries a
/// message fit to show a listener, the same contract `bluetooth::run` keeps.
pub fn run(verb: Verb, target: Option<&str>) -> Result<serde_json::Value, String> {
    if verb.needs_target() && target.is_none() {
        return Err("not a speaker".into());
    }
    match verb {
        Verb::Scan | Verb::Status => {
            let found = discover(DISCOVER_TIMEOUT);
            serde_json::to_value(&found).map_err(|e| e.to_string())
        }
        Verb::Use => {
            let udn = target.expect("checked above");
            let found = discover(DISCOVER_TIMEOUT);
            let speaker = found
                .into_iter()
                .find(|s| s.udn == udn)
                .ok_or_else(|| format!("{udn} was not found on the network just now"))?;
            serde_json::to_value(&speaker).map_err(|e| e.to_string())
        }
        Verb::Forget => Ok(serde_json::json!({"ok": true})),
    }
}

/// Broadcast an SSDP M-SEARCH and collect every `ZonePlayer` that answers
/// within `timeout`, deduplicated to one entry per stereo pair or group
/// `[Sonos/SONOS008 §4]`.
///
/// A plain UDP multicast and a short, bounded wait -- this runs off the
/// mixer thread entirely (called only from a web request or at startup),
/// so blocking here costs nothing the way it would in `engine.rs`
/// `[SPEC-APS-040]`.
pub fn discover(timeout: Duration) -> Vec<SonosSpeaker> {
    let responders = match ssdp_search(timeout) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sonos discovery: {e}");
            return Vec::new();
        }
    };

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for ip in responders {
        match topology::coordinators(ip) {
            Ok(coords) => {
                for c in coords {
                    if seen.insert(c.udn.clone()) {
                        out.push(c);
                    }
                }
            }
            Err(e) => eprintln!("sonos topology at {ip}: {e}"),
        }
    }
    out
}

/// What the web layer holds for as long as Sonos output is chosen: the
/// running encoder and which speaker it is pointed at, so `forget` and a
/// later `use` both know who to say `Stop` to `[Sonos/SONOS008 §6]`.
pub struct SonosSession {
    pub stream: stream::SonosStream,
    pub speaker: SonosSpeaker,
}

/// The first half of switching output *to* Sonos: a ring, and the encoder
/// already reading it. Split from [`point_and_play`] rather than done
/// together in one call, found necessary against the real Office pair
/// `[Sonos/SONOS010 §6]`: a coordinator resolves the new `CurrentURI` as
/// part of accepting `SetAVTransportURI`/`Play` themselves, which means the
/// stream must already be *fetchable* -- the caller's own HTTP route
/// subscribed to it -- before either SOAP call is made, not after both
/// return. Calling this before the network round-trip, rather than folding
/// it into one function the way `[Sonos/SONOS010 §2]`'s own fix first did,
/// is what makes that ordering possible.
pub fn start_stream(ring_capacity: usize) -> (crate::output::OutputRing, stream::SonosStream) {
    let ring = crate::output::OutputRing::new(ring_capacity, crate::output::Volume::new(1.0));
    let running = stream::SonosStream::start(ring.clone());
    (ring, running)
}

/// The second half: point the coordinator at a stream already running,
/// already reachable, and -- as of this fix -- already carrying real audio.
///
/// The engine must be told to feed the ring *before* this call, not after
/// `[Sonos/SONOS010 §6]`, reversing what `[Sonos/SONOS010 §2]`'s own fix
/// first tried: found against the real Office pair that `Play` itself reads
/// from the stream to confirm it before acknowledging, so a ring nothing has
/// fed yet leaves that handshake with nothing to read -- neither side able
/// to finish first, which surfaced as a plain socket-read timeout on this
/// end rather than any answer at all. The caller is responsible for the
/// symmetric rollback a failure now needs: telling the engine to stop
/// feeding the ring again, the same as [`deactivate`] already does, so a
/// failed attempt is not left believing a ring is still wanted -- the actual
/// property `[Sonos/SONOS010 §2]` existed to protect, kept here by undoing
/// the optimistic send rather than by delaying it.
///
/// `speaker` is expected freshly resolved by [`discover`], never the raw
/// persisted [`SonosTarget`] `[GDE-SONOS-760]`: an IP that changed since
/// the choice was made must read as "not found," not send a stream nobody
/// asked for to a coordinator that has moved on to being someone's
/// dishwasher's IP address by now.
pub fn point_and_play(speaker: &SonosSpeaker, stream_url: &str) -> Result<(), String> {
    soap::set_uri_and_play(speaker.ip, stream_url)
}

/// The reverse of [`start_stream`]/[`point_and_play`]: stop the coordinator
/// (a courtesy, not a correctness requirement -- dropping `stream` already
/// stops the encoder regardless of whether the speaker heard the `Stop`) and
/// tell the engine to stop feeding a ring nothing is reading any more.
pub fn deactivate(engine: &crate::engine::EngineHandle, last_known_ip: IpAddr) {
    let _ = soap::stop(last_known_ip, Duration::from_secs(5));
    engine.send(crate::engine::Command::SetSonosRing(None));
}

/// This machine's own LAN-facing address -- the one a Sonos speaker,
/// somewhere else on the network, could actually reach `[Sonos/SONOS008
/// §6]`. `localhost`/`127.0.0.1` would resolve fine on this host and mean
/// nothing to the speaker fetching it.
///
/// The standard trick: ask the OS which local address it would use to
/// reach an outside address, without sending it any traffic -- UDP is
/// connectionless, so `connect` here only selects a route and a source
/// address, it never puts a packet on the wire.
pub fn local_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

/// Send the M-SEARCH and return the distinct IPs that answered.
///
/// One socket, one broadcast, and a read loop bounded by `timeout` rather
/// than by a response count -- there is no way to know in advance how many
/// units are on the network, and a fixed count would either wait forever on
/// a house with fewer or miss ones on a house with more.
fn ssdp_search(timeout: Duration) -> std::io::Result<Vec<IpAddr>> {
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_read_timeout(Some(Duration::from_millis(200)))?;
    sock.set_broadcast(true)?;

    let msg = format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: 239.255.255.250:1900\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: 2\r\n\
         ST: {SEARCH_TARGET}\r\n\r\n"
    );
    let dest: SocketAddr = SSDP_MULTICAST.parse().expect("literal address");
    sock.send_to(msg.as_bytes(), dest)?;

    let deadline = std::time::Instant::now() + timeout;
    let mut found = std::collections::HashSet::new();
    let mut buf = [0u8; 2048];
    while std::time::Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                if text.to_uppercase().starts_with("HTTP/1.1 200") {
                    found.insert(from.ip());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(found.into_iter().collect())
}

/// SOAP/UPnP control -- the calls validated live in `Sonos/SONOS001`.
pub mod soap {
    use std::net::IpAddr;
    use std::time::Duration;

    /// Every request here is this shape: a `SOAPACTION` header naming the
    /// service and method, and a small XML envelope. Sonos's own HTTP server
    /// rejects a request whose `Host:` header is not the bare IP -- a real
    /// quirk measured while surveying the pair `[GDE-SONOS-010]`, not a
    /// defensive guess -- so the caller is always addressed by IP, never by
    /// hostname, at this layer.
    pub(crate) fn post_raw(
        ip: IpAddr,
        path: &str,
        service: &str,
        action: &str,
        body: &str,
        timeout: Duration,
    ) -> Result<String, String> {
        let envelope = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
             <s:Body><u:{action} xmlns:u=\"{service}\">{body}</u:{action}></s:Body>\
             </s:Envelope>"
        );
        let request = format!(
            "POST {path} HTTP/1.1\r\n\
             Host: {ip}\r\n\
             Content-Type: text/xml; charset=\"utf-8\"\r\n\
             SOAPACTION: \"{service}#{action}\"\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n{envelope}",
            envelope.len()
        );

        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(ip, super::SONOS_PORT),
            timeout,
        )
        .map_err(|e| format!("connect to {ip}: {e}"))?;
        stream.set_read_timeout(Some(timeout)).ok();
        stream.set_write_timeout(Some(timeout)).ok();
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("write to {ip}: {e}"))?;
        let mut resp = String::new();
        stream
            .read_to_string(&mut resp)
            .map_err(|e| format!("read from {ip}: {e}"))?;

        let status_ok = resp.starts_with("HTTP/1.1 200") || resp.starts_with("HTTP/1.0 200");
        if !status_ok {
            let status_line = resp.lines().next().unwrap_or("(no response)");
            return Err(format!("{ip} refused {action}: {status_line}"));
        }
        // Headers end at the blank line; everything after is the SOAP body.
        // A caller that only needs to know the call succeeded (Play, Stop,
        // SetVolume) simply never looks at it.
        Ok(resp.split_once("\r\n\r\n").map_or(resp.clone(), |(_, body)| body.to_string()))
    }

    const AVTRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:1";
    const RENDERINGCONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:1";
    const AVTRANSPORT_PATH: &str = "/MediaRenderer/AVTransport/Control";
    const RENDERINGCONTROL_PATH: &str = "/MediaRenderer/RenderingControl/Control";

    /// Escape the handful of characters that would otherwise break out of an
    /// XML text node -- a URI Vaino built itself, so this is a formality
    /// against pathological library metadata, not a hostile input.
    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    /// The scheme Sonos itself wants for a continuous, non-seekable source
    /// `[GDE-SONOS-1180]` -- found against the real Office pair, not assumed:
    /// a plain `http://` URI with empty metadata was refused outright with a
    /// 500 before this fix, the same convention `node-sonos-http-api` and
    /// `SoCo` already use for internet radio, cited as this integration's own
    /// precedent in `[Sonos/SONOS002 §4]`. Sonos treats the substituted
    /// scheme as its own internal signal that nothing here has a duration to
    /// seek within; the real, fetchable `http://` address still has to appear
    /// somewhere, which is what the DIDL-Lite `<res>` tag below is for.
    pub(crate) fn radio_uri(stream_url: &str) -> String {
        stream_url.replacen("http://", "x-rincon-mp3radio://", 1)
    }

    /// A minimal DIDL-Lite item, `object.item.audioItem.audioBroadcast`
    /// `[GDE-SONOS-1180]` -- the class UPnP itself defines for a live source
    /// with no fixed length. Escaped once for its own XML content, then
    /// handed to `set_uri_and_play` to be escaped a second time as it goes
    /// into the outer SOAP envelope's own text node -- ordinary
    /// XML-in-XML-in-text, not a special case.
    fn radio_metadata(stream_url: &str) -> String {
        format!(
            "<DIDL-Lite xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
             xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\" \
             xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\">\
             <item id=\"1\" parentID=\"0\" restricted=\"1\">\
             <dc:title>Vaino</dc:title>\
             <upnp:class>object.item.audioItem.audioBroadcast</upnp:class>\
             <res protocolInfo=\"http-get:*:audio/mpeg:*\">{}</res>\
             </item></DIDL-Lite>",
            xml_escape(stream_url)
        )
    }

    /// Longer than every other call in this module, deliberately -- found
    /// necessary against the real Office pair `[Sonos/SONOS010 §6]`: both
    /// calls below can have the coordinator itself connect back to Vaino's
    /// own stream to validate it before acknowledging, and the ordinary 5 s
    /// this module uses everywhere else clipped that round-trip mid-flight,
    /// surfacing as a bare socket-read timeout rather than any answer at all.
    const URI_SET_TIMEOUT: Duration = Duration::from_secs(15);

    /// Point the coordinator at Vaino's own continuous stream and start it
    /// playing -- called once per session, not once per track
    /// `[Sonos/SONOS008 §6]`.
    pub fn set_uri_and_play(ip: IpAddr, stream_url: &str) -> Result<(), String> {
        let body = format!(
            "<InstanceID>0</InstanceID><CurrentURI>{}</CurrentURI>\
             <CurrentURIMetaData>{}</CurrentURIMetaData>",
            xml_escape(&radio_uri(stream_url)),
            xml_escape(&radio_metadata(stream_url))
        );
        post_raw(ip, AVTRANSPORT_PATH, AVTRANSPORT, "SetAVTransportURI", &body, URI_SET_TIMEOUT)?;
        post_raw(
            ip,
            AVTRANSPORT_PATH,
            AVTRANSPORT,
            "Play",
            "<InstanceID>0</InstanceID><Speed>1</Speed>",
            URI_SET_TIMEOUT,
        )?;
        Ok(())
    }

    pub fn stop(ip: IpAddr, timeout: Duration) -> Result<(), String> {
        post_raw(ip, AVTRANSPORT_PATH, AVTRANSPORT, "Stop", "<InstanceID>0</InstanceID>", timeout)?;
        Ok(())
    }

    /// What the coordinator's own `AVTransport` currently believes
    /// `CurrentURI` is -- the one fact that says whether Vaino is still
    /// actually in control, or whether something else already took over
    /// with no other signal at all `[Sonos/SONOS010 §3]`. The same read
    /// already used, by hand, throughout `Sonos/SONOS001`'s own survey.
    pub fn current_uri(ip: IpAddr, timeout: Duration) -> Result<String, String> {
        let xml =
            post_raw(ip, AVTRANSPORT_PATH, AVTRANSPORT, "GetMediaInfo", "<InstanceID>0</InstanceID>", timeout)?;
        extract_tag(&xml, "CurrentURI")
            .ok_or_else(|| format!("{ip} answered GetMediaInfo with no CurrentURI in it"))
    }

    /// `<Tag>value</Tag>` -- SOAP's own XML entities already decoded by the
    /// time this reads a response, so a plain string search is enough for
    /// the one field this needs, the same reasoning `topology`'s own
    /// hand-rolled parser already uses `[GDE-SONOS-020]`.
    fn extract_tag(xml: &str, tag: &str) -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = xml.find(&open)? + open.len();
        let end = xml[start..].find(&close)? + start;
        Some(xml[start..end].to_string())
    }

    /// `0..=100`, the range every Sonos unit's own `Master` volume uses.
    pub fn set_volume(ip: IpAddr, percent: u8, timeout: Duration) -> Result<(), String> {
        let body = format!(
            "<InstanceID>0</InstanceID><Channel>Master</Channel>\
             <DesiredVolume>{}</DesiredVolume>",
            percent.min(100)
        );
        post_raw(ip, RENDERINGCONTROL_PATH, RENDERINGCONTROL, "SetVolume", &body, timeout)?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn xml_escape_covers_the_five_characters_that_matter() {
            assert_eq!(xml_escape("<a & b>\"x\""), "&lt;a &amp; b&gt;&quot;x&quot;");
        }

        /// The exact shape `GetMediaInfo` answered with while surveying the
        /// real pair `[GDE-SONOS-040]`.
        #[test]
        fn current_uri_reads_out_of_a_real_get_media_info_response() {
            let body = "<u:GetMediaInfoResponse xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">\
                         <NrTracks>1</NrTracks><MediaDuration>NOT_IMPLEMENTED</MediaDuration>\
                         <CurrentURI>http://192.168.67.70:8097/flow/media_player.office/x.mp3</CurrentURI>\
                         <CurrentURIMetaData></CurrentURIMetaData></u:GetMediaInfoResponse>";
            assert_eq!(
                extract_tag(body, "CurrentURI").as_deref(),
                Some("http://192.168.67.70:8097/flow/media_player.office/x.mp3")
            );
        }

        #[test]
        fn a_response_with_no_such_tag_reads_as_absent_not_a_panic() {
            assert_eq!(extract_tag("<Envelope></Envelope>", "CurrentURI"), None);
            assert_eq!(extract_tag("", "CurrentURI"), None);
        }

        #[test]
        fn a_stream_url_with_reserved_characters_survives_the_envelope() {
            let url = "http://vainopi:5720/sonos-stream?x=1&y=2";
            let escaped = xml_escape(url);
            assert!(!escaped.contains('&') || escaped.contains("&amp;"));
            assert!(escaped.contains("&amp;y=2"));
        }

        /// `[GDE-SONOS-1180]`: found against the real Office pair, which
        /// refused a plain `http://` `CurrentURI` with empty metadata
        /// outright (a 500, before ever attempting to fetch it) -- the
        /// substitution `node-sonos-http-api` and `SoCo` already use for a
        /// continuous, non-seekable source `[Sonos/SONOS002 §4]`.
        #[test]
        fn the_scheme_sonos_wants_for_a_live_stream_replaces_only_the_leading_http() {
            assert_eq!(
                radio_uri("http://vainopi:5720/audio/sonos/stream"),
                "x-rincon-mp3radio://vainopi:5720/audio/sonos/stream"
            );
        }

        /// The real, fetchable address still has to appear somewhere once
        /// `CurrentURI` itself no longer carries an `http://` scheme -- the
        /// DIDL-Lite `<res>` tag is that somewhere, tagged as a live
        /// broadcast rather than a seekable track.
        #[test]
        fn the_metadata_names_a_live_broadcast_carrying_the_real_url() {
            let didl = radio_metadata("http://vainopi:5720/audio/sonos/stream");
            assert!(didl.contains("object.item.audioItem.audioBroadcast"));
            assert!(didl.contains("http://vainopi:5720/audio/sonos/stream"));
        }
    }
}

/// Group topology: which units answer, which of them is a coordinator, and
/// which are bonded satellites that must never appear as a selectable
/// target on their own `[GDE-SONOS-020]`.
pub mod topology {
    use std::net::IpAddr;
    use std::time::Duration;

    use super::{SonosMember, SonosSpeaker};

    const ZGT_SERVICE: &str = "urn:schemas-upnp-org:service:ZoneGroupTopology:1";
    const ZGT_PATH: &str = "/ZoneGroupTopology/Control";
    const TIMEOUT: Duration = Duration::from_secs(3);

    /// Every coordinator this one unit's own topology view knows about --
    /// every responder in a discovery answers with the same shared view, so
    /// the caller dedupes across units, not this function.
    pub fn coordinators(ip: IpAddr) -> Result<Vec<SonosSpeaker>, String> {
        let xml = super::soap::post_raw(
            ip,
            ZGT_PATH,
            ZGT_SERVICE,
            "GetZoneGroupState",
            "",
            TIMEOUT,
        )?;
        Ok(parse_coordinators(&xml))
    }

    /// Hand-rolled, not a general XML parser: `ZoneGroupState` is a fixed,
    /// well-known shape from one kind of device, doubly-encoded (the SOAP
    /// body's own XML entities wrap an inner XML document as text) --
    /// exactly the shape already read by hand while surveying the real pair
    /// `[GDE-SONOS-020]`. A real parser would cost a new dependency to
    /// handle generality nothing here needs.
    fn parse_coordinators(xml: &str) -> Vec<SonosSpeaker> {
        let decoded = xml
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&");

        let mut out = Vec::new();
        for group in split_between(&decoded, "<ZoneGroup ", "</ZoneGroup>") {
            let Some(coordinator_udn) = attr(group, "Coordinator") else { continue };
            let raw_members = split_between(group, "<ZoneGroupMember ", "/>");

            // Every member in a group carries the same ChannelMapSet -- one
            // reading, from whichever member has it, is enough for the
            // whole group's member list `[Sonos/SONOS010 §9]`.
            let channel_map = raw_members
                .iter()
                .find_map(|m| attr(m, "ChannelMapSet"))
                .map(|s| parse_channel_map(&s))
                .unwrap_or_default();

            let mut coordinator = None;
            for member in &raw_members {
                let Some(uuid) = attr(member, "UUID") else { continue };
                if uuid != coordinator_udn {
                    continue; // a satellite, or another group's member entirely
                }
                if attr(member, "Invisible").as_deref() == Some("1") {
                    continue; // defensive: a coordinator is never invisible in practice
                }
                let Some(name) = attr(member, "ZoneName") else { continue };
                let Some(location) = attr(member, "Location") else { continue };
                let Some(ip) = location
                    .strip_prefix("http://")
                    .and_then(|s| s.split(':').next())
                    .and_then(|s| s.parse().ok())
                else {
                    continue;
                };
                coordinator = Some(SonosSpeaker { udn: uuid, name, ip, members: Vec::new() });
            }
            let Some(mut speaker) = coordinator else { continue };

            // Every unit named in the group, coordinator included, labelled
            // by its own channel where the map says one -- "Coordinator" for
            // a lone unit's own single-member group, which carries no
            // ChannelMapSet worth reading at all.
            speaker.members = raw_members
                .iter()
                .filter_map(|m| attr(m, "UUID"))
                .map(|udn| {
                    let channel = channel_map
                        .iter()
                        .find(|(u, _)| *u == udn)
                        .map(|(_, c)| c.clone())
                        .unwrap_or_else(|| {
                            if udn == coordinator_udn { "Coordinator".into() } else { "?".into() }
                        });
                    SonosMember { udn, channel }
                })
                .collect();
            out.push(speaker);
        }
        out
    }

    /// `RINCON_A:RF,RF;RINCON_B:LF,LF` -> `[(RINCON_A, RF), (RINCON_B, LF)]`
    /// -- takes the first of each pair's two channel labels, which are
    /// identical in every response measured `[GDE-SONOS-020]`.
    fn parse_channel_map(s: &str) -> Vec<(String, String)> {
        s.split(';')
            .filter_map(|entry| {
                let (udn, chans) = entry.split_once(':')?;
                let chan = chans.split(',').next()?;
                Some((udn.to_string(), chan.to_string()))
            })
            .collect()
    }

    /// Every substring starting with `open` and ending at the next `close`,
    /// non-overlapping -- enough structure for a flat list of same-shaped
    /// elements, which is everything this format ever hands back.
    fn split_between<'a>(s: &'a str, open: &str, close: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find(open) {
            let after = &rest[start..];
            if let Some(end) = after.find(close) {
                out.push(&after[..end]);
                rest = &after[end..];
            } else {
                break;
            }
        }
        out
    }

    /// `name="value"` within one element's own opening tag text.
    fn attr(element: &str, name: &str) -> Option<String> {
        let needle = format!("{name}=\"");
        let start = element.find(&needle)? + needle.len();
        let end = element[start..].find('"')? + start;
        Some(element[start..end].to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The real response captured live from the Office pair while
        /// writing `Sonos/SONOS001` `[GDE-SONOS-020]` -- a fixture drawn
        /// from measurement, not invented.
        const REAL_RESPONSE: &str = include_str!("sonos_test_fixtures/zone_group_state.xml");

        #[test]
        fn the_coordinator_is_found_and_the_satellite_is_not() {
            let found = parse_coordinators(REAL_RESPONSE);
            assert_eq!(found.len(), 1, "a bonded pair must yield exactly one entry");
            let office = &found[0];
            assert_eq!(office.udn, "RINCON_347E5CCAE44A01400");
            assert_eq!(office.name, "Office");
            assert_eq!(office.ip, "192.168.67.56".parse::<std::net::IpAddr>().unwrap());
        }

        /// `[Sonos/SONOS010 §9]`: the satellite is invisible as its own
        /// selectable entry, but still named, with its own channel, inside
        /// the coordinator's `members` -- read from the same real fixture,
        /// not invented.
        #[test]
        fn both_units_of_the_bonded_pair_appear_as_members() {
            let found = parse_coordinators(REAL_RESPONSE);
            let members = &found[0].members;
            assert_eq!(members.len(), 2, "a stereo pair has two members, not one");
            let coordinator = members.iter().find(|m| m.udn == "RINCON_347E5CCAE44A01400").unwrap();
            assert_eq!(coordinator.channel, "RF");
            let satellite = members.iter().find(|m| m.udn == "RINCON_347E5CC5950801400").unwrap();
            assert_eq!(satellite.channel, "LF");
        }

        #[test]
        fn an_empty_document_yields_nothing_rather_than_panicking() {
            assert!(parse_coordinators("").is_empty());
            assert!(parse_coordinators("<not-this-at-all/>").is_empty());
        }
    }
}

/// The engine's mixed output, encoded and served as one continuous stream
/// `[Sonos/SONOS008 §6]` -- an internet-radio-style feed, not one HTTP
/// request per track. Vaino's `Cargo.toml` already carries the encoder
/// choice `[Sonos/SONOS005]` and the linking it implies `[Sonos/SONOS006]`.
pub mod stream {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use mp3lame_encoder::{Bitrate, Builder, InterleavedPcm, Quality};
    use tokio::sync::broadcast;

    use crate::output::OutputRing;

    /// Frames (per channel) pulled and encoded per pass. LAME's own frame is
    /// 1152 samples; several at once amortise the ring's lock without
    /// building up latency worth noticing `[Sonos/SONOS004]`.
    const FRAMES_PER_PASS: usize = 1152 * 4;
    const CHANNELS: usize = 2;
    /// How long to wait when the ring had nothing new, rather than spinning
    /// a whole core waiting on silence.
    const IDLE: Duration = Duration::from_millis(20);
    /// Enough queued chunks that a new HTTP connection joining mid-stream
    /// does not immediately see `Lagged` before it has read anything.
    const CHANNEL_DEPTH: usize = 64;

    /// Owns the background encode thread for as long as Sonos output is
    /// chosen. Dropping it stops the thread -- there is nothing left for it
    /// to feed once nobody holds the ring's write side either.
    pub struct SonosStream {
        tx: broadcast::Sender<Vec<u8>>,
        stop: Arc<AtomicBool>,
    }

    impl SonosStream {
        /// Starts the encoder against `ring`, which the engine is expected
        /// to already be feeding via `Command::SetSonosRing`
        /// `[Sonos/SONOS008 §6]` -- this does not itself touch the engine.
        pub fn start(ring: OutputRing) -> Self {
            let (tx, _rx) = broadcast::channel(CHANNEL_DEPTH);
            let stop = Arc::new(AtomicBool::new(false));
            let tx2 = tx.clone();
            let stop2 = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("vaino-sonos-encode".into())
                .spawn(move || encode_loop(ring, tx2, stop2))
                .expect("spawn sonos encoder");
            Self { tx, stop }
        }

        /// A fresh receiver for one HTTP connection. Broadcast, not a
        /// queue any one subscriber must keep up with alone: Sonos itself
        /// is expected to be the only listener in practice, but nothing
        /// here assumes it.
        pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
            self.tx.subscribe()
        }
    }

    impl Drop for SonosStream {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn encode_loop(ring: OutputRing, tx: broadcast::Sender<Vec<u8>>, stop: Arc<AtomicBool>) {
        let Some(mut builder) = Builder::new() else {
            eprintln!("sonos: could not create a LAME encoder");
            return;
        };
        let configured = builder.set_num_channels(CHANNELS as u8).is_ok()
            && builder.set_sample_rate(ring.sample_rate()).is_ok()
            && builder.set_brate(Bitrate::Kbps192).is_ok()
            && builder.set_quality(Quality::Good).is_ok();
        if !configured {
            eprintln!("sonos: could not configure the LAME encoder");
            return;
        }
        let mut encoder = match builder.build() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("sonos: LAME encoder failed to build: {e:?}");
                return;
            }
        };

        let mut pcm = vec![0f32; FRAMES_PER_PASS * CHANNELS];
        // LAME's own documented worst case for one call, `1.25 * samples +
        // 7200` bytes -- found the hard way, against the real Office pair
        // `[Sonos/SONOS010 §6]`: `Vec::new()` never allocates, so
        // `encode_to_vec`'s own `output.spare_capacity_mut()` handed LAME's C
        // encoder a zero-length buffer backed by a dangling pointer on every
        // single call. Nothing crashed immediately only because LAME buffers
        // several passes of lookahead before it has enough to emit its first
        // real frame -- once it did, it wrote through that pointer anyway and
        // took the whole process down with it (`SIGSEGV`), a good ten to
        // fifteen seconds into the very first real activation this encoder
        // had ever actually run against. Reserved once, not grown per pass:
        // `clear()` below drops the *length*, never the *capacity*, so this
        // stays the one allocation for the life of the thread.
        let mut mp3 = Vec::with_capacity(FRAMES_PER_PASS * 5 / 4 + 7200);
        // A real local device paces the whole mixing chain for free -- its
        // own hardware callback only drains at exactly the audio's own rate,
        // which is what makes `path.ring.free()` a trustworthy throttle
        // `[Sonos/SONOS012 §3]`. Nothing plays that role here: with local
        // failed or silenced, `mix_and_submit` paces off `sonos_ring`'s own
        // free space instead, which stays large (this ring holds ~15 s) for
        // as long as THIS loop keeps draining it -- and this loop, run flat
        // out, drains and encodes far faster than real time on any machine
        // worth deploying to. The result, found against the real Office
        // pair: encoding raced ahead, the `CHANNEL_DEPTH`-deep broadcast
        // channel filled faster than Sonos's own network read could drain
        // it, and the overflowed chunks were silently dropped (`.ok()` on a
        // `Lagged` error, in `sonos_stream`) -- audible as continuous but
        // "skippy" playback, not silence. `pacing_delay` below is this
        // loop's own substitute for the hardware callback local output gets
        // for free: never emit faster than the audio itself plays.
        let sample_rate = ring.sample_rate();
        let started = std::time::Instant::now();
        let mut frames_encoded: u64 = 0;
        while !stop.load(Ordering::Relaxed) {
            let got = ring.read(&mut pcm);
            if got == 0 {
                std::thread::sleep(IDLE);
                continue;
            }
            mp3.clear();
            match encoder.encode_to_vec(InterleavedPcm(&pcm[..got]), &mut mp3) {
                Ok(_) if !mp3.is_empty() => {
                    // No receivers is the ordinary case between sessions,
                    // not a fault -- the encoder keeps running either way,
                    // ready for whichever HTTP request comes next.
                    let _ = tx.send(mp3.clone());
                }
                Ok(_) => {}
                Err(e) => eprintln!("sonos: encode failed: {e:?}"),
            }
            frames_encoded += (got / CHANNELS) as u64;
            if let Some(d) = pacing_delay(frames_encoded, sample_rate, started) {
                std::thread::sleep(d);
            }
        }
    }

    /// How long to sleep so this thread never gets ahead of the audio it is
    /// producing -- `None` when it is already at or behind real time, which
    /// is the ordinary case whenever the machine or the network is briefly
    /// busy. Pure and separately tested `[Sonos/SONOS012 §3]`, since the real
    /// timing this substitutes for (a hardware callback) is not something a
    /// fast unit test can wait on directly.
    fn pacing_delay(frames_encoded: u64, sample_rate: u32, started: Instant) -> Option<Duration> {
        if sample_rate == 0 {
            return None;
        }
        let target = Duration::from_secs_f64(frames_encoded as f64 / sample_rate as f64);
        target.checked_sub(started.elapsed())
    }

    #[cfg(test)]
    mod pacing_tests {
        use super::*;

        #[test]
        fn running_far_ahead_of_real_time_asks_for_a_real_wait() {
            let started = Instant::now();
            // A full second of audio claimed encoded, though no real time
            // has passed at all -- exactly what an unthrottled loop does
            // against a ring with plenty of backlog to read from.
            let delay = pacing_delay(44_100, 44_100, started);
            assert!(
                delay.is_some_and(|d| d.as_millis() > 900),
                "should ask to wait close to a second, got {delay:?}"
            );
        }

        #[test]
        fn already_behind_real_time_asks_for_nothing() {
            let started = Instant::now() - Duration::from_secs(2);
            // One second of audio encoded, but two real seconds have
            // actually elapsed -- already slower than real time, the
            // ordinary case whenever the machine or network is briefly busy.
            assert_eq!(pacing_delay(44_100, 44_100, started), None);
        }

        #[test]
        fn an_unconfigured_sample_rate_never_panics_on_the_division() {
            assert_eq!(pacing_delay(1000, 0, Instant::now()), None);
        }
    }
}

