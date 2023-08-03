use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use color_eyre::Report;
use thiserror::Error;
use tracing::debug;

use crate::udp::Response;

pub type SocketResponse = (Response, SocketAddr);

#[derive(Debug, Clone)]
pub enum Event {
    None = 0,
    Completed,
    Started,
    Stopped,
}

#[derive(Error, Debug)]
pub enum GeneralError {
    #[error("usage: everlasting [torrent file | magnet link]")]
    Usage,
    #[error("piece was already flushed or incomplete")]
    AlreadyFlushed,
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
}

#[derive(Default, Debug, PartialEq, Clone)]
pub struct TorrentInfo {
    pub info: Info,
    // no need for Vec<Vec<T>> as torrents rarely have only one tracker
    pub announce: Announce,
    pub created: Option<u64>,
    pub comment: String,
    pub author: Option<String>,
}

impl TorrentInfo {
    pub fn length(&self) -> u64 {
        match &self.info.mode {
            Mode::Single { length, .. } => length.to_owned(),
            Mode::Multi { files, .. } => files.iter().map(|f| f.length).sum(),
        }
    }

    pub fn bitfield_size(&self) -> u64 {
        let len = self.length();
        let piece_len = self.info.piece_length;
        len / piece_len / 64 + 1
    }
}

// first value is either multi-mode directory or single mode file

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
