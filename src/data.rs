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
    pub pieces: Vec<u8>,
    pub private: Option<()>,
    pub value: Vec<u8>,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct SingleInfo {
    pub name: String,
    pub length: u64,
    pub md5sum: Option<Vec<u8>>,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct MultiInfo {
    pub dir_name: String,
    pub files: Vec<File>,
    pub md5sum: Option<Vec<u8>>,
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

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Single(SingleInfo),
    Multi(MultiInfo),
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Single(SingleInfo::default())
    }
}
