use std::{
    fs,
    net::{SocketAddr, ToSocketAddrs},
    path::{Path, PathBuf},
};

use color_eyre::Report;
use thiserror::Error;
use tracing::debug;
use url::Url;

use crate::{helpers, udp::Response};

pub type SocketResponse = (Response, SocketAddr);

#[derive(Debug, Default, Clone, PartialEq)]
pub enum Event {
    #[default]
    None = 0,
    Completed,
    Started,
    Stopped,
}

#[derive(Error, Debug)]
pub enum GeneralError {
    #[error("usage: everlasting [torrent file | magnet link]")]
    Usage,
    #[error("first bucket must contain our own node")]
    UninitializedNode,
    #[error("piece was already flushed or incomplete")]
    AlreadyFlushed,
    #[error("info metadata not fetched yet")]
    MissingInfo,
    #[error("file does not exist")]
    NonExistentFile,
    #[error("oops")]
    InvalidPieceIdx,
    #[error("oops")]
    InvalidPieceHash,
    #[error("magnet link contains no valid trackers: `{0:?}`")]
    InvalidUdpTracker(String),
    #[error("magnet link is invalid: `{0:?}`")]
    InvalidMagnet(String),
    #[error("no active trackers for this torrent")]
    DeadUdpTrackers,
    #[error("timeout from remote address: {0:?}")]
    Timeout(Option<SocketAddr>),
    #[error("reconnect")]
    Reconnect,
    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),
    #[error("failed to parse URL: {0}")]
    ParseFailure(String),
    #[error("broken pipe")]
    BrokenPipe,
}

pub const PROTOCOL_ID: i64 = 0x41727101980;
pub const SHA1_LEN: usize = 20;

#[derive(Default, Debug, PartialEq, Clone)]
pub struct Announce {
    pub udp: Vec<SocketAddr>,
    pub http: Vec<String>,
    pub peers: Vec<SocketAddr>,
}

impl Announce {
    pub fn push(&mut self, s: String) -> Result<(), Report> {
        if s.starts_with("http") {
            self.http.push(s.clone());
        }

        if s.starts_with("udp") {
            let url = Url::parse(&s)?;
            let host = url.host().unwrap();
            let port = url.port().unwrap();

            let mut addr = (host.to_string(), port).to_socket_addrs()?;
            let ip = addr
                .next()
                .ok_or(GeneralError::InvalidUdpTracker(s.to_owned()))?;
            self.udp.push(ip);
        }

        Ok(())
    }
}

#[derive(Default, Debug, PartialEq, Clone)]
pub struct TorrentInfo {
    pub info: Option<Info>,
    // no need for Vec<Vec<T>> as torrents rarely have only one tracker
    pub announce: Announce,
    pub created: Option<u64>,
    pub comment: String,
    pub author: Option<String>,
    pub hash: [u8; 20],
}

impl TorrentInfo {
    pub fn length(&self) -> usize {
        match self.info.as_ref().map(|info| info.mode.clone()) {
            Some(Mode::Single { length, .. }) => length.to_owned() as usize,
            Some(Mode::Multi { files, .. }) => files.iter().map(|f| f.length as usize).sum(),
            None => 0usize,
        }
    }

    pub fn bitfield_size(&self) -> usize {
        let len = self.length();
        let piece_len = self
            .info
            .as_ref()
            .map(|info| info.piece_length.clone() as usize)
            .unwrap();

        len / piece_len / 64 + 1
    }
}

impl TryFrom<Url> for TorrentInfo {
    type Error = Report;

    fn try_from(url: Url) -> Result<Self, Self::Error> {
        let pairs = url.query_pairs().into_owned();
        let mut info = TorrentInfo::default();

        // v1: magnet:?xt=urn:btih:<info-hash>&dn=<name>&tr=<tracker-url>&x.pe=<peer-address>
        // v2: magnet:?xt=urn:btmh:<tagged-info-hash>&dn=<name>&tr=<tracker-url>&x.pe=<peer-address>

        for pair in pairs {
            match (pair.0.as_str(), pair.1) {
                ("xt", s) => {
                    let split: Vec<_> = s.split(':').collect();
                    match split[1] {
                        "btih" => {
                            info.hash = helpers::decode(split[2]);
                        }
                        "btmh" => {
                            todo!()
                        }
                        _ => return Err(GeneralError::InvalidMagnet(s).into()),
                    }
                }
                ("tr", s) => {
                    info.announce.push(s);
                }
                ("dn", s) => {
                    info.comment = s;
                }
                ("x.pe", s) => {
                    let s = s
                        .to_socket_addrs()?
                        .next()
                        .ok_or(GeneralError::InvalidUdpTracker(s))?;
                    info.announce.peers.push(s.to_owned());
                }
                _ => {}
            }
        }

        Ok(info)
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Info {
    pub mode: Mode,
    pub piece_length: u64,
    pub pieces: Box<[[u8; SHA1_LEN]]>,
    pub private: Option<()>,
    pub value: [u8; 20],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Single {
        name: String,
        length: u64,
        md5sum: Option<Box<[u8]>>,
    },
    Multi {
        dir_name: String,
        files: Vec<File>,
        md5sum: Option<Box<[u8]>>,
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

impl Mode {
    pub fn create_file_layout(&self) -> Result<(), Report> {
        use std::fs::File;

        let full_path = Path::new("./downloads");
        let x = fs::create_dir(full_path.clone());
        debug!(?x);

        match self {
            Mode::Single { name, .. } => {
                File::create(full_path.join(name))?;
            }
            Mode::Multi {
                dir_name, files, ..
            } => {
                let mut result = Vec::with_capacity(files.len());
                // create parent directory
                let path = full_path.join(dir_name);
                let x = fs::create_dir(path.clone());
                debug!(?x);

                for file in files {
                    match file.path.len() {
                        1 => {
                            let path = path.join(file.path[0].clone());

                            File::create(&path)?;
                            result.push(path);
                        }
                        n => {
                            let path = path.join(
                                file.path[..n - 1]
                                    .iter()
                                    .fold(PathBuf::new(), |path, acc| path.join(acc)),
                            );
                            let x = fs::create_dir_all(path.clone());
                            debug!(?x);

                            File::create(path.join(file.path.last().unwrap()))?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn name(&self) -> String {
        match self {
            Mode::Single { name, .. } => name.to_owned(),
            Mode::Multi { dir_name, .. } => dir_name.to_owned(),
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct File {
    pub length: u64,
    pub md5sum: Option<Box<[u8]>>,
    pub path: Vec<String>,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub struct Peer {
    pub id: Option<[u8; 20]>,
    pub addr: SocketAddr,
}

pub type Peers = Vec<Peer>;

impl From<&SocketAddr> for Peer {
    fn from(addr: &SocketAddr) -> Self {
        Peer {
            id: None,
            addr: *addr,
        }
    }
}

#[derive(Default, Debug, PartialEq)]
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
