use std::net::{SocketAddr, ToSocketAddrs};

use color_eyre::Report;
use rand::Rng;
use url::Url;

use crate::{data::GeneralError, helpers::decode};

#[derive(Default, Debug)]
pub struct MagnetInfo {
    // TODO: For compatability with existing links in the wild,
    // clients should also support the 32 character base32 encoded info-hash.
    pub hash: [u8; 20],
    pub name: Option<String>,
    pub trackers: Option<Vec<String>>,
    pub peers: Option<Vec<SocketAddr>>,
}

impl TryFrom<Url> for MagnetInfo {
    type Error = Report;

    fn try_from(url: Url) -> Result<Self, Self::Error> {
        let pairs = url.query_pairs().into_owned();
        let mut info = MagnetInfo::default();

        // v1: magnet:?xt=urn:btih:<info-hash>&dn=<name>&tr=<tracker-url>&x.pe=<peer-address>
        // v2: magnet:?xt=urn:btmh:<tagged-info-hash>&dn=<name>&tr=<tracker-url>&x.pe=<peer-address>

        for pair in pairs {
            match (pair.0.as_str(), pair.1) {
                ("xt", s) => {
                    let split: Vec<_> = s.split(':').collect();
                    match split[1] {
                        "btih" => {
                            info.hash = decode(split[2]);
                        }
                        "btmh" => {
                            todo!()
                        }
                        _ => return Err(GeneralError::InvalidMagnet(s).into()),
                    }
                }
                ("tr", s) => {
                    // let dst = udp_to_ip(&s).ok_or(GeneralError::InvalidUdpTracker(s))?;
                    info.trackers = match info.trackers {
                        Some(mut trackers) => {
                            trackers.push(s);
                            Some(trackers)
                        }
                        None => Some(vec![s]),
                    };
                }
                ("dn", s) => {
                    info.name = Some(s);
                }
                ("x.pe", s) => {
                    let s = s
                        .to_socket_addrs()?
                        .next()
                        .ok_or(GeneralError::InvalidUdpTracker(s))?;
                    info.peers = match info.peers {
                        Some(mut peers) => {
                            peers.push(s.to_owned());
                            Some(peers)
                        }
                        None => Some(vec![s.to_owned()]),
                    };
                }
                _ => {}
            }
        }

        Ok(info)
    }
}

impl MagnetInfo {}
