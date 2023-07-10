use std::{
    borrow::Cow,
    net::{SocketAddr, ToSocketAddrs},
};
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

pub fn range_to_array<const N: usize>(v: &[u8]) -> [u8; N] {
    core::array::from_fn::<u8, N, _>(|i| v[i])
}

pub fn decode<S: AsRef<str>>(s: S) -> [u8; 20] {
    let v: Vec<_> = s.as_ref().chars().collect();
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

pub fn udp_to_ip<S: AsRef<str>>(s: S) -> Option<SocketAddr> {
    let url: Url = Url::parse(s.as_ref()).ok()?;

    let (addr, port) = (url.host_str()?, url.port()?);
    (addr, port).to_socket_addrs().ok()?.next()
}
