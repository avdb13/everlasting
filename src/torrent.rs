use crate::data::{Event, Peer, TorrentInfo};

pub struct Torrent {
    inner: TorrentInfo,
    // file_structure: todo!(),
    uploaded: i32,
    downloaded: i32,
    status: Event,
    peers: Vec<Peer>,
}
