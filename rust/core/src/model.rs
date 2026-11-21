use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::net::Ipv4Addr;
use std::str::FromStr;

use base64::{
    Engine, alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
};
use percent_encoding::percent_decode_str;
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result};

pub const SLOTS: std::ops::RangeInclusive<u8> = 1..=6;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Host(Ipv4Addr);

impl Host {
    pub fn new(address: Ipv4Addr) -> Result<Self> {
        if address.is_private() || address.is_link_local() {
            Ok(Self(address))
        } else {
            Err(Error::Invalid(
                "use a private or link-local IPv4 address".into(),
            ))
        }
    }
}

impl FromStr for Host {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let address = value
            .trim()
            .parse()
            .map_err(|_| Error::Invalid("use a numeric LAN IPv4 address".into()))?;
        Self::new(address)
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

fn printable(ch: char) -> bool {
    !ch.is_control()
        && !matches!(ch, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
}

pub fn clean_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| if printable(ch) { ch } else { ' ' })
        .collect::<String>()
        .trim()
        .chars()
        .take(180)
        .collect()
}

fn clean(value: Option<&str>) -> Option<String> {
    value.map(clean_text).filter(|s| !s.is_empty())
}

pub fn validate_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if (1..=64).contains(&name.chars().count()) && name.chars().all(printable) {
        Ok(name)
    } else {
        Err(Error::Invalid(
            "use a name of 1–64 printable characters".into(),
        ))
    }
}

pub fn validate_url(value: &str) -> Result<&str> {
    let parsed = url::Url::parse(value);
    if matches!(&parsed, Ok(url) if matches!(url.scheme(), "http" | "https")
        && value.split_once("://").is_some_and(|(_, rest)| !rest.starts_with('/')
            && !rest.split(['/', '?', '#']).next().unwrap_or_default().contains('@'))
        && url.has_host() && url.port() != Some(0) && url.username().is_empty()
        && url.password().is_none() && url.fragment().is_none())
        && !value.contains('\\')
        && value.chars().all(|c| printable(c) && !c.is_whitespace())
    {
        Ok(value)
    } else {
        Err(Error::Invalid(
            "use a direct HTTP or HTTPS stream URL without credentials or a fragment".into(),
        ))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Codec {
    #[default]
    Auto,
    Mp3,
    Aac,
    AacPlus,
}

impl Codec {
    pub const OPTIONS: [Self; 4] = [Self::Auto, Self::Mp3, Self::Aac, Self::AacPlus];
    pub fn str_value(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Mp3 => Some("MP3"),
            Self::Aac => Some("AAC"),
            Self::AacPlus => Some("AAC+"),
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.str_value().unwrap_or("Auto"))
    }
}

impl FromStr for Codec {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        Self::OPTIONS
            .into_iter()
            .find(|codec| codec.to_string().eq_ignore_ascii_case(value))
            .ok_or_else(|| Error::Invalid("codec must be Auto, MP3, AAC, or AAC+".into()))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Preset {
    pub slot: u8,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    details: Map<String, Value>,
}

pub type Presets = BTreeMap<u8, Preset>;

impl Preset {
    pub fn radio(
        slot: u8,
        name: &str,
        url: &str,
        codec: Codec,
        previous: Option<&Self>,
    ) -> Result<Self> {
        check_slot(slot)?;
        let name = validate_name(name)?.to_owned();
        let url = validate_url(url)?;
        let mut details = previous
            .filter(|item| item.is_radio() && item.stream_url() == Some(url))
            .map(|item| item.details.clone())
            .unwrap_or_default();
        details.insert("stream_url".into(), Value::String(url.into()));
        if let Some(codec) = codec.str_value() {
            details.insert("codec".into(), Value::String(codec.into()));
        } else {
            details.remove("codec");
        }
        Ok(Self {
            slot,
            name,
            kind: "radio".into(),
            details,
        })
    }

    pub fn stream_url(&self) -> Option<&str> {
        self.details.get("stream_url").and_then(Value::as_str)
    }

    pub fn codec_label(&self) -> Option<&str> {
        self.details.get("codec").and_then(Value::as_str)
    }

    pub fn is_radio(&self) -> bool {
        self.kind == "radio"
            && self
                .details
                .get("source")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
    }

    pub(crate) fn matches_saved_radio(&self, saved: &Self) -> bool {
        self.slot == saved.slot
            && self.kind == saved.kind
            && self.name == saved.name
            && self.stream_url() == saved.stream_url()
            && self
                .codec_label()
                .is_none_or(|codec| saved.codec_label() == Some(codec))
    }
}

pub(crate) fn check_slot(slot: u8) -> Result<()> {
    if SLOTS.contains(&slot) {
        Ok(())
    } else {
        Err(Error::Invalid("use a physical slot from 1 to 6".into()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HardwarePreset {
    pub slot: u8,
    pub source: Option<String>,
    pub name: Option<String>,
    #[serde(skip)]
    pub(crate) location: Option<String>,
    #[serde(skip)]
    pub(crate) has_item: bool,
}

pub(crate) fn hardware_matches(preset: &Preset, physical: Option<&HardwarePreset>) -> bool {
    let Some(key) = physical else { return false };
    let source = key.source.as_deref();
    let location = key.location.as_deref().unwrap_or_default();
    if preset.kind == "native" {
        let Some(native) = preset.details.get("native") else {
            return false;
        };
        return source == native["source"].as_str()
            && Some(location) == native["location"].as_str();
    }
    let path = if preset.kind == "spotify" {
        format!("/spotify/stream-{}.ogg", preset.slot)
    } else {
        format!("/stream/{}", preset.slot)
    };
    let expected = format!("http://127.0.0.1:8888{path}");
    match source {
        Some("UPNP") => location == expected,
        Some("LOCAL_INTERNET_RADIO") => {
            let Some((_, encoded)) = location.split_once("/station?data=") else {
                return false;
            };
            let data = percent_decode_str(encoded).collect::<Vec<_>>();
            let config = GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent);
            let decoded = GeneralPurpose::new(&alphabet::URL_SAFE, config)
                .decode(&data)
                .or_else(|_| GeneralPurpose::new(&alphabet::STANDARD, config).decode(&data));
            decoded
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|station| station["streamUrl"].as_str() == Some(expected.as_str()))
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Agent {
    pub port: u16,
    pub version: String,
    pub build: Option<String>,
    #[serde(skip)]
    pub friendly_name: Option<String>,
}

pub(crate) fn parse_agent(body: &[u8], port: u16) -> Result<Agent> {
    let reply: Value =
        serde_json::from_slice(body).map_err(|_| Error::Response("malformed STR JSON".into()))?;
    let version = reply["version"].as_str().filter(|version| {
        if version.len() > 80 {
            return false;
        }
        let version = version.strip_prefix('v').unwrap_or(version);
        let numbers = version.split(['-', '+']).next().unwrap_or_default();
        let segments = numbers.split('.').collect::<Vec<_>>();
        segments.len() == 3
            && segments
                .iter()
                .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            && version[numbers.len()..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
            && version.len() != numbers.len() + 1
    });
    match (version, reply["build"].as_str()) {
        (Some(version), Some(build)) => Ok(Agent {
            port,
            version: version.into(),
            build: clean(Some(build)),
            friendly_name: clean(reply["friendlyName"].as_str()),
        }),
        _ => Err(Error::Response(
            "response does not match the STR version schema".into(),
        )),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Component {
    pub category: Option<String>,
    pub software_version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpeakerInfo {
    pub model: String,
    pub module_type: Option<String>,
    pub variant: Option<String>,
    pub variant_mode: Option<String>,
    pub firmware: Option<String>,
    pub components: Vec<Component>,
    #[serde(skip)]
    pub friendly_name: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub info: Option<SpeakerInfo>,
    pub presets: Option<Vec<HardwarePreset>>,
    pub str_agent: Option<Agent>,
    pub errors: BTreeMap<String, String>,
}

fn xml_text(body: &[u8]) -> Cow<'_, str> {
    let body = body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(body);
    match std::str::from_utf8(body) {
        Ok(text) => Cow::Borrowed(text),
        // Some Bose firmware writes names as Latin-1 despite declaring UTF-8.
        Err(_) => Cow::Owned(body.iter().map(|byte| char::from(*byte)).collect()),
    }
}

fn parse_xml<'a>(text: &'a str, expected: &str) -> Result<Document<'a>> {
    let doc = Document::parse(text)
        .map_err(|_| Error::Response("malformed or unsupported Bose XML".into()))?;
    if doc.root_element().tag_name().name() == expected {
        Ok(doc)
    } else {
        Err(Error::Response(format!("expected <{expected}> XML")))
    }
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|node| node.is_element() && node.tag_name().name() == name)
}

fn field(node: Node<'_, '_>, name: &str) -> Option<String> {
    clean(child(node, name).and_then(|node| node.text()))
}

pub(crate) fn parse_info(body: &[u8]) -> Result<SpeakerInfo> {
    let text = xml_text(body);
    let doc = parse_xml(&text, "info")?;
    let root = doc.root_element();
    let model = field(root, "type")
        .ok_or_else(|| Error::Response("missing device type in /info".into()))?;
    let components: Vec<_> = child(root, "components")
        .into_iter()
        .flat_map(|node| node.children())
        .filter(|node| node.is_element() && node.tag_name().name() == "component")
        .map(|node| Component {
            category: field(node, "componentCategory"),
            software_version: field(node, "softwareVersion"),
        })
        .collect();
    let firmware = components
        .iter()
        .filter(|component| {
            component
                .category
                .as_ref()
                .is_some_and(|s| s.eq_ignore_ascii_case("scm"))
        })
        .find_map(|component| component.software_version.clone());

    Ok(SpeakerInfo {
        model,
        module_type: field(root, "moduleType"),
        variant: field(root, "variant"),
        variant_mode: field(root, "variantMode"),
        firmware,
        components,
        friendly_name: field(root, "name"),
    })
}

pub(crate) fn parse_hardware(body: &[u8]) -> Result<BTreeMap<u8, HardwarePreset>> {
    let text = xml_text(body);
    let doc = parse_xml(&text, "presets")?;
    let mut presets = BTreeMap::new();
    for node in doc.root_element().children().filter(Node::is_element) {
        let slot = node
            .attribute("id")
            .and_then(|id| id.trim().trim_start_matches('0').parse::<u8>().ok())
            .filter(|slot| node.tag_name().name() == "preset" && SLOTS.contains(slot))
            .ok_or_else(|| Error::Response("unexpected physical preset slot".into()))?;
        let item = child(node, "ContentItem");
        let entry = HardwarePreset {
            slot,
            source: clean(item.and_then(|item| item.attribute("source"))),
            name: item.and_then(|item| field(item, "itemName")),
            location: item
                .and_then(|item| item.attribute("location"))
                .map(str::to_owned),
            has_item: item.is_some(),
        };
        if presets.insert(slot, entry).is_some() {
            return Err(Error::Response("duplicate physical preset slot".into()));
        }
    }
    Ok(presets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_allows_namespace_and_latin1_and_keeps_identifiers_private() {
        let xml = b"<info xmlns='urn:bose' deviceID='private'><name>K\xfcche</name><type>SoundTouch 20</type><components><component><componentCategory>SCM</componentCategory><softwareVersion>27.0.6 build</softwareVersion><serialNumber>private</serialNumber></component></components></info>";
        let info = parse_info(xml).unwrap();
        assert_eq!(info.friendly_name.as_deref(), Some("Küche"));
        assert_eq!(info.firmware.as_deref(), Some("27.0.6 build"));
        let exported = serde_json::to_string(&info).unwrap();
        assert!(!exported.contains("private"));
        assert!(!exported.contains("Küche"));
    }

    #[test]
    fn validates_agent_shape_and_rejects_dtd_and_duplicate_slots() {
        assert!(parse_agent(br#"{"version":"v1.8.2","build":"2026-11-19"}"#, 17008).is_ok());
        assert!(parse_agent(br#"{"version":"v1.8.2"}"#, 8888).is_err());
        assert!(
            parse_info(b"<!DOCTYPE info [<!ENTITY bad 'x'>]><info><type>&bad;</type></info>")
                .is_err()
        );
        assert!(parse_hardware(b"<presets><preset id='1'/><preset id='01'/></presets>").is_err());
    }

    #[test]
    fn validates_lan_and_stream_addresses() {
        for bad in ["example.org", "https://192.168.1.7", "127.0.0.1", "8.8.8.8"] {
            assert!(bad.parse::<Host>().is_err(), "{bad}");
        }
        for bad in [
            "file:///mp3",
            "https://u:p@host/a",
            "https:///host",
            "https://host/a b",
            "https://host/a#t",
        ] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
        assert!(validate_url("https://example.org:8000/live?q=1").is_ok());
    }

    #[test]
    fn verifies_native_radio_registration_without_exposing_location() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let data = URL_SAFE_NO_PAD.encode(br#"{"streamUrl":"http://127.0.0.1:8888/stream/2"}"#);
        let key = HardwarePreset {
            slot: 2,
            source: Some("LOCAL_INTERNET_RADIO".into()),
            location: Some(format!("/station?data={data}")),
            name: None,
            has_item: true,
        };
        let preset =
            Preset::radio(2, "Example", "https://example.org/live", Codec::Mp3, None).unwrap();
        assert!(hardware_matches(&preset, Some(&key)));
        assert!(!hardware_matches(&Preset { slot: 1, ..preset }, Some(&key)));
        assert!(!serde_json::to_string(&key).unwrap().contains("station?"));
    }
}
