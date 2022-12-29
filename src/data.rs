use bendy::{
    decoding::{self, DictDecoder},
    encoding::ToBencode,
};

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

impl ToBencode for SingleInfo {
    const MAX_DEPTH: usize = 5;
    fn encode(
        &self,
        encoder: bendy::encoding::SingleItemEncoder,
    ) -> Result<(), bendy::encoding::Error> {
        let result = encoder.emit_dict(|mut e| {
            e.emit_pair(b"length", self.length)?;
            if let Some(md5sum) = &self.md5sum {
                e.emit_pair(b"md5sum", md5sum)?;
            }
            e.emit_pair(b"name", &self.name)?;

            Ok(())
        });
        dbg!(&result);
        result
    }
}

impl ToBencode for MultiInfo {
    const MAX_DEPTH: usize = 5;
    fn encode(
        &self,
        encoder: bendy::encoding::SingleItemEncoder,
    ) -> Result<(), bendy::encoding::Error> {
        let result = encoder.emit_dict(|mut e| {
            e.emit_pair(b"files", &self.files)?;
            e.emit_pair(b"name", &self.dir_name)?;
            Ok(())
        });
        dbg!(&result);
        result
    }
}

impl ToBencode for File {
    const MAX_DEPTH: usize = 5;
    fn encode(
        &self,
        encoder: bendy::encoding::SingleItemEncoder,
    ) -> Result<(), bendy::encoding::Error> {
        encoder.emit_dict(|mut e| {
            e.emit_pair(b"length", self.length)?;
            if let Some(md5sum) = &self.md5sum {
                e.emit_pair(b"md5sum", md5sum)?;
            }
            e.emit_pair(b"path", &self.path)?;

            Ok(())
        })
    }
}

impl ToBencode for Info {
    const MAX_DEPTH: usize = 5;
    fn encode(
        &self,
        encoder: bendy::encoding::SingleItemEncoder,
    ) -> Result<(), bendy::encoding::Error> {
        encoder.emit_dict(|mut e| {
            e.emit_pair(b"piece length", self.piece_length)?;
            e.emit_pair(b"pieces", self.piece_length)?;
            if self.private.is_some() {
                e.emit_pair(b"private", "0")?;
            }

            match &self.mode {
                Mode::Single(info) => {
                    let result = e.emit_pair(b"info", info);
                    dbg!(&result);
                }
                Mode::Multi(info) => {
                    let result = e.emit_pair(b"info", info);
                    dbg!(&result);
                }
            }

            Ok(())
        })
    }
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
    pub port: u32,
}

impl Peer {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
}

#[derive(Default, Debug, Eq, PartialEq)]
pub struct Response {
    pub failure_reason: Option<String>,
    pub warning: Option<String>,
    pub interval: u64,
    pub min_interval: Option<u64>,
    pub tracker_id: String,
    pub complete: u64,
    pub incomplete: u64,
    pub peers: Vec<Peer>,
}
// "d8:complete
// i0e10:downloadedi0e10:incompletei0e8:intervali86400e12:min intervali86400e5:peers0:6:peers60:15:warning message45:info hash is not authorized with this trackere",

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
