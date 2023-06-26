use std::{
    borrow::Cow,
    net::{SocketAddr, ToSocketAddrs},
};

use color_eyre::Report;
use rand::seq::SliceRandom;
use tracing::debug;
use url::Url;

use crate::data::{GeneralError, MagnetInfo};

pub fn prettier(s: String) -> String {
    s.chars()
        .map(|c| {
            if c == '(' || c == '{' || c == ',' {
                c.to_string() + "\n"
            } else if c == ')' || c == '}' {
                "\n".to_string() + &c.to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub fn range_to_array<const N: usize>(v: &[u8]) -> [u8; N] {
    core::array::from_fn::<u8, N, _>(|i| v[i])
}

pub fn decode(s: String) -> [u8; 20] {
    let v: Vec<_> = s.chars().collect();
    let v = v
        .chunks(2)
        .map(|src| u8::from_str_radix(&src.iter().collect::<String>(), 16).unwrap())
        .collect::<Vec<_>>();
    v.try_into().unwrap()
}
pub fn encode(arr: &[u8]) -> String {
    arr.iter()
        .map(|&b| {
            match b {
                // hexadecimal number
                0x30..=0x39 | 0x41..=0x59 | 0x61..=0x79 => {
                    char::from_u32(b.into()).unwrap().to_string()
                }
                x => format!("%{x:02x}").to_uppercase(),
            }
        })
        .collect()
}

// TODO: reconsider whether we want to use the same decoder for magnets and torrent files.
pub fn magnet_decoder<S: AsRef<str>>(s: S) -> Result<MagnetInfo, Report> {
    let url = Url::parse(s.as_ref())?;
    let pairs = url.query_pairs().into_owned();

    let mut info = MagnetInfo::default();
    for pair in pairs {
        match (pair.0.as_str(), pair.1) {
            // "xt", "dn", "xl", "tr", "ws", "as", "xs", "kt", "mt", "so", "x.pe",
            ("xt", s) => {
                let url = s.split(':').last().unwrap();
                info.hash = decode(url.to_owned());
            }
            ("tr", s) => {
                // let dst = udp_to_ip(&s).ok_or(GeneralError::InvalidTracker(s))?;
                info.trackers.push(s);
            }
            ("dn", s) => {
                info.name = s;
            }
            _ => {}
        }
    }
    debug!(?info.hash);

    info.trackers.shuffle(&mut rand::thread_rng());
    Ok(info)
}

pub fn udp_to_ip(s: &str) -> Option<SocketAddr> {
    let url: Url = Url::parse(&s).ok()?;

    let (addr, port) = (url.host_str()?, url.port()?);
    (addr, port).to_socket_addrs().ok()?.next()
}
