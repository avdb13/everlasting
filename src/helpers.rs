use color_eyre::Report;
use futures_util::Future;
use std::{
    net::ToSocketAddrs,
    thread::sleep,
    time::{self, Duration},
};
use url::Url;

use crate::data::GeneralError;

pub struct Timer(time::Instant, time::Duration);

impl Timer {
    pub fn new(duration: time::Duration) -> Self {
        Self(time::Instant::now(), duration)
    }

    pub fn reset(&mut self) {
        self.0 = time::Instant::now();
    }

    pub fn elapsed(&self) -> bool {
        self.0.elapsed() >= self.1
    }
}

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

pub async fn attempt<T, F, C>(func: C, count: u8, interval: u8) -> Result<T, Report>
where
    C: Fn() -> F,
    F: Future<Output = Result<T, Report>>,
{
    for _ in 0..count {
        match func().await {
            Ok(res) => return Ok(res),
            Err(_) => {
                sleep(Duration::from_secs(interval.into()));
                continue;
            }
        }
    }

    Err(GeneralError::Timeout(None).into())
}
