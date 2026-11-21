use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::Serialize;

use crate::Host;

const SERVICES: [&str; 4] = [
    "_soundtouch._tcp.local.",
    "_bose-soundtouch._tcp.local.",
    "_streborn._tcp.local.",
    "_soundtouchstick._tcp.local.",
];

#[derive(Debug, Default, Serialize)]
pub struct Discovery {
    pub devices: BTreeMap<String, BTreeSet<String>>,
    pub errors: BTreeMap<String, String>,
}

pub fn discover() -> Discovery {
    let mut result = Discovery::default();
    let Ok(daemon) = ServiceDaemon::new() else {
        result
            .errors
            .insert("mDNS".into(), "could not start local mDNS discovery".into());
        return result;
    };
    let mut browsers = Vec::new();
    for full_name in SERVICES {
        let service = full_name.trim_end_matches(".local.");
        match daemon.browse(full_name) {
            Ok(events) => browsers.push((service, events)),
            Err(_) => {
                result
                    .errors
                    .insert(service.into(), "could not browse for this service".into());
            }
        }
    }
    if !browsers.is_empty() {
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            for (service, events) in &browsers {
                for event in events.try_iter() {
                    if let ServiceEvent::ServiceResolved(info) = event {
                        for address in info.get_addresses_v4() {
                            if let Ok(host) = Host::new(address) {
                                result
                                    .devices
                                    .entry(host.to_string())
                                    .or_default()
                                    .insert((*service).into());
                            }
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(40));
        }
    }
    let _ = daemon.shutdown();
    result
}
