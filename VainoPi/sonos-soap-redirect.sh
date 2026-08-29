#!/bin/bash
# One-off SOAP redirect for the Icecast reconnect-gap experiment
# [Sonos/SONOS012 §6/§8], [Sonos/SONOS002 §3 Option B].
#
# Points the Office coordinator's CurrentURI at whatever stream URL is
# given, using the exact envelope shape player/src/sonos.rs's own
# soap::set_uri_and_play already validated against this hardware
# (x-rincon-mp3radio:// scheme substitution, minimal audioBroadcast
# DIDL-Lite, Host header pinned to the bare IP). No Vaino code involved --
# this is a standalone script so the redirect can point at Icecast
# (or back at Vaino) without touching the running process.
#
#   VainoPi/sonos-soap-redirect.sh <coordinator-ip> <stream-url>
#
#   VainoPi/sonos-soap-redirect.sh 192.168.67.56 http://192.168.67.20:8000/vaino.mp3
#   VainoPi/sonos-soap-redirect.sh 192.168.67.56 http://192.168.67.20:5720/audio/sonos/stream   # revert to Vaino direct
set -euo pipefail

IP="${1:?usage: sonos-soap-redirect.sh <coordinator-ip> <stream-url>}"
STREAM_URL="${2:?usage: sonos-soap-redirect.sh <coordinator-ip> <stream-url>}"
RADIO_URI="${STREAM_URL/http:\/\//x-rincon-mp3radio://}"

xml_escape() {
    sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g'
}

DIDL="<DIDL-Lite xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\" xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\"><item id=\"1\" parentID=\"0\" restricted=\"1\"><dc:title>Vaino</dc:title><upnp:class>object.item.audioItem.audioBroadcast</upnp:class><res protocolInfo=\"http-get:*:audio/mpeg:*\">${STREAM_URL}</res></item></DIDL-Lite>"
DIDL_ESC=$(printf '%s' "$DIDL" | xml_escape)
URI_ESC=$(printf '%s' "$RADIO_URI" | xml_escape)

soap_call() {
    local action="$1" body="$2"
    local envelope="<?xml version=\"1.0\" encoding=\"utf-8\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:${action} xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">${body}</u:${action}></s:Body></s:Envelope>"
    curl -sS --max-time 20 \
        -X POST "http://${IP}:1400/MediaRenderer/AVTransport/Control" \
        -H "Host: ${IP}" \
        -H "Content-Type: text/xml; charset=\"utf-8\"" \
        -H "SOAPACTION: \"urn:schemas-upnp-org:service:AVTransport:1#${action}\"" \
        --data-binary "$envelope" \
        -w '\nHTTP %{http_code}\n'
}

echo "== SetAVTransportURI -> ${RADIO_URI} =="
soap_call "SetAVTransportURI" "<InstanceID>0</InstanceID><CurrentURI>${URI_ESC}</CurrentURI><CurrentURIMetaData>${DIDL_ESC}</CurrentURIMetaData>"

echo "== Play =="
soap_call "Play" "<InstanceID>0</InstanceID><Speed>1</Speed>"
