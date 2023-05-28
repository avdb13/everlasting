use std::net::SocketAddr;

use thiserror::Error;

use crate::udp::Response;

pub type SocketResponse = (Response, SocketAddr);

#[derive(Error, Debug)]
pub enum GeneralError {
    #[error("usage: everlasting [torrent file | magnet link]")]
    Usage,
    #[error("magnet link contains no valid trackers: `{0:?}`")]
    InvalidTracker(String),
    #[error("no active trackers for this torrent")]
    DeadTrackers,
    #[error("timeout")]
    Timeout,
    #[error("reconnect")]
    Reconnect,
    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),
}

pub const PROTOCOL_ID: i64 = 0x41727101980;
pub const SHA1_LEN: usize = 20;

#[derive(Default, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub info: Info,
    pub announce: String,
    pub announce_list: Option<Vec<Vec<String>>>,
    pub created: Option<u64>,
    pub comment: String,
    pub author: Option<String>,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Info {
    pub mode: Mode,
    pub piece_length: u64,
    pub pieces: Vec<[u8; SHA1_LEN]>,
    pub private: Option<()>,
    pub value: [u8; 20],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Single {
        name: String,
        length: u64,
        md5sum: Option<Vec<u8>>,
    },
    Multi {
        dir_name: String,
        files: Vec<File>,
        md5sum: Option<Vec<u8>>,
    },
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Single {
            length: 0,
            name: Default::default(),
            md5sum: None,
        }
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct File {
    pub length: u64,
    pub md5sum: Option<Vec<u8>>,
    pub path: Vec<String>,
}

#[derive(Default, Debug, Eq, PartialEq)]
pub struct Peer {
    pub id: String,
    pub ip: String,
    pub port: String,
}

impl Peer {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
}

#[derive(Default, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub failure_reason: Option<String>,
    pub warning: Option<String>,
    pub interval: u64,
    pub min_interval: Option<u64>,
    pub tracker_id: String,
    pub complete: u64,
    pub incomplete: u64,
    pub peers: Vec<Peer>,
}

#[derive(Default, Debug)]
pub struct ScrapeResponse {
    pub files: Vec<(Vec<u8>, Status)>,
}

impl ScrapeResponse {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }
}

#[derive(Default, Debug)]
pub struct Status {
    pub seeders: u32,
    pub finished: u32,
    pub leechers: u32,
    pub name: Option<String>,
}

pub struct HttpRequest {
    hash: [u8; SHA1_LEN],
    id: [u8; SHA1_LEN],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    compact: bool,
}
