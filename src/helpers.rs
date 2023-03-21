use std::slice::Iter;

use color_eyre::Report;
use rand::seq::IteratorRandom;
use url::Url;

pub fn decode(s: String) -> [u8; 20] {
    let v: Vec<_> = s.chars().collect();
    let v = v
        .chunks(2)
        .map(|src| src.iter().collect::<String>().parse::<u8>().unwrap())
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

#[derive(Default, Debug)]
pub struct MagnetInfo {
    pub hash: String,
    pub name: String,
    pub trackers: Vec<String>,
}

pub fn magnet_decoder(s: String) -> Result<MagnetInfo, Report> {
    let url = Url::parse(&s)?;
    let pairs = url.query_pairs().into_owned();

    let mut info = MagnetInfo::default();
    for pair in pairs {
        match (pair.0.as_str(), pair.1) {
            // "xt", "dn", "xl", "tr", "ws", "as", "xs", "kt", "mt", "so", "x.pe",
            ("xt", s) => {
                let url = s.split(':').last().unwrap();
                info.hash = url.to_owned();
            }
            ("tr", s) => {
                info.trackers.push(s);
            }
            ("dn", s) => {
                info.name = s;
            }
            _ => {}
        }
    }

    dbg!(&info);

    Ok(info)
}
