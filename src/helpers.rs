use std::borrow::Cow;

use color_eyre::Report;
use url::Url;

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

#[derive(Clone, Default, Debug)]
pub struct MagnetInfo {
    pub hash: String,
    pub name: String,
    pub trackers: Vec<String>,
}

pub fn magnet_decoder<S: AsRef<str>>(s: S) -> Result<MagnetInfo, Report> {
    let url = Url::parse(s.as_ref())?;
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

    Ok(info)
}
