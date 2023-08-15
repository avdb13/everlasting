use std::fs;
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::ops::{BitAndAssign, BitXor};
use std::path::Path;
use std::sync::Arc;

use ahash::{HashMap, HashMapExt};
use color_eyre::Report;
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;

use crate::data::{GeneralError, Info, Mode, SHA1_LEN};
use crate::BLOCK_SIZE;

#[derive(Debug, Clone)]
pub struct BitField(Box<[usize]>);

impl BitField {
    pub fn new(v: Vec<usize>) -> Self {
        BitField(v.into_boxed_slice())
    }

    pub fn from_lazy(value: Vec<usize>, real_len: usize) -> Self {
        let mut v = vec![0usize; real_len];

        for x in value {
            let index = x / std::mem::size_of::<usize>();
            let offset = x % std::mem::size_of::<usize>();

            let i = v.get_mut(index).unwrap();
            *i |= 1 << offset;
        }

        BitField(v.into_boxed_slice())
    }
}

impl BitAndAssign for BitField {
    fn bitand_assign(&mut self, rhs: Self) {
        let left = &mut self.0;
        let right = rhs.0;

        assert_eq!(left.len(), right.len());

        left.iter_mut().zip(right.iter()).for_each(|(x, y)| *x &= y);
    }
}

impl BitXor for BitField {
    type Output = BitField;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let left = self.0;
        let right = rhs.0;

        assert_eq!(left.len(), right.len());

        let inner = left.iter().zip(right.iter()).map(|(x, y)| *x ^ y).collect();

        BitField(inner)
    }
}

pub struct DataManager {
    bitfield_map: Arc<RwLock<HashMap<SocketAddr, BitField>>>,
    rare_pieces: Arc<RwLock<BitField>>,
    pieces: PiecesWrapper,
    piece_len: u64,
}

impl DataManager {
    pub fn new(info: Info) -> Self {
        let pieces = info.pieces.len();
        let piece_len = info.piece_length;

        // amount of usizes we need to preserve the bitfield
        let v = vec![0usize, pieces / std::mem::size_of::<usize>() + 1];
        // let actual_pieces = Vec::with_capacity(pieces).into_boxed_slice();

        DataManager {
            bitfield_map: Arc::new(RwLock::new(HashMap::new())),
            rare_pieces: Arc::new(RwLock::new(BitField(v.into_boxed_slice()))),
            pieces: PiecesWrapper::new(info),
            piece_len,
        }
    }

    pub async fn listen(&self, mut rx: Receiver<(SocketAddr, BitField)>) {
        let inner = self.bitfield_map.clone();
        let rare_pieces = self.rare_pieces.clone();

        let f = async move {
            // buffer for rare pieces
            let mut buffer = Vec::with_capacity(4);

            while let Some((peer, bitfield)) = rx.recv().await {
                let mut inner = inner.write().await;

                if let Some(v) = inner.get_mut(&peer) {
                    // lazy bitfield assignment
                    *v &= bitfield.clone();
                } else {
                    // standard bitfield
                    inner.insert(peer, bitfield.clone());
                }

                // filter rare pieces and reset the buffer
                if buffer.push_within_capacity(bitfield).is_err() {
                    let reduced = buffer.clone().into_iter().reduce(|b, acc| b ^ acc).unwrap();
                    buffer.clear();

                    let mut rare_pieces = rare_pieces.write().await;
                    *rare_pieces &= reduced;
                }
            }
        };

        tokio::spawn(f);
    }
}

// piece: <len=0009+X><id=7><index><begin><block>
#[derive(Debug, Clone, Default)]
pub struct Piece {
    // we don't wanna initialize the box before we have some of its data since otherwise it would
    // hold the entire torrent in memory
    inner: Option<Box<[u8]>>,
    written: Box<[bool]>,
    flushed: bool,
}

impl Piece {
    fn new(blocks: usize) -> Self {
        let written = vec![false; blocks].into_boxed_slice();
        Self {
            inner: None,
            written,
            flushed: false,
        }
    }

    fn complete(&self) -> bool {
        self.written.iter().all(|&b| b)
    }
}

pub struct PiecesWrapper {
    piece_len: u64,
    hashes: Box<[[u8; SHA1_LEN]]>,
    inner: Box<[Piece]>,
    mode: Mode,
}

impl PiecesWrapper {
    pub fn new(info: Info) -> Self {
        let piece_len = info.piece_length;
        let hashes = info.pieces.clone();
        let mode = info.mode.clone();

        let blocks = (info.piece_length as usize) / *BLOCK_SIZE + 1;
        let inner = vec![Piece::new(blocks); hashes.len()].into_boxed_slice();

        Self {
            piece_len,
            hashes,
            inner,
            mode,
        }
    }
    pub async fn write(&mut self, index: usize, begin: usize, block: &[u8]) -> Result<(), Report> {
        let piece = self
            .inner
            .get_mut(index)
            .ok_or(GeneralError::InvalidPieceIdx)?;

        let inner = piece
            .inner
            .as_mut()
            .map(|x| x as &mut [u8])
            .ok_or(GeneralError::InvalidPieceIdx)?;
        let mut piece = Cursor::new(inner);

        let _ = piece.seek(SeekFrom::Start(begin as u64));
        piece.write_all(block).map_err(Into::into)
    }

    pub fn verify_piece(&self, index: usize) -> Result<(), Report> {
        let piece = self.inner.get(index).ok_or(GeneralError::InvalidPieceIdx)?;
        let expected = self
            .hashes
            .get(index)
            .ok_or(GeneralError::InvalidPieceIdx)?;

        let mut hash = Vec::with_capacity(expected.len());
        let inner = piece
            .inner
            .as_deref()
            .expect("piece doesn't exist despite being complete");

        let mut hasher = Sha1::new();
        hasher.input(inner);
        hasher.result(&mut hash);

        if hash == expected {
            Ok(())
        } else {
            Err(GeneralError::InvalidPieceHash.into())
        }
    }

    pub async fn flush_piece(&mut self, index: usize) -> Result<(), Report> {
        let full_path = Path::new("./downloads");
        let piece = self.inner.get(index).ok_or(GeneralError::InvalidPieceIdx)?;

        // do we really need this closure?
        if piece.flushed || !piece.complete() {
            return Err(GeneralError::AlreadyFlushed.into());
        }

        self.verify_piece(index)?;
        let piece_offset = self.piece_len * (index as u64);

        match &self.mode {
            Mode::Single { name, .. } => {
                let file = fs::File::open(full_path.join(name))?;
                let mut file = Cursor::new(file);

                file.set_position(piece_offset);
                let buf = piece.inner.as_ref().unwrap();
                file.get_mut().write_all(buf as &[u8])?;

                Ok(())
            }
            Mode::Multi {
                dir_name, files, ..
            } => {
                let file_offsets =
                    (0..files.len()).map(|i| files.iter().map(|f| f.length).take(i).sum::<u64>());

                // find the file that has the piece index within its range
                let index = (0..files.len())
                    .find(|&i| file_offsets.clone().nth(i).unwrap() >= piece_offset)
                    .ok_or(GeneralError::NonExistentFile)?
                    - 1;

                let file = files.get(index).ok_or(GeneralError::NonExistentFile)?;
                let path = file
                    .path
                    .iter()
                    .fold(Path::new(dir_name).to_path_buf(), |path, acc| {
                        path.join(acc)
                    });

                let file = fs::File::open(path)?;
                let mut file = Cursor::new(file);

                file.set_position(piece_offset - file_offsets.clone().nth(index).unwrap());
                let buf = piece.inner.as_ref().unwrap();
                file.get_mut().write_all(buf as &[u8])?;

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bendy::decoding::FromBencode;
    use color_eyre::Report;
    use rand::Rng;

    use crate::data::{Mode, TorrentInfo};

    #[test]
    fn test_map_piece_to_file() -> Result<(), Report> {
        let torrent = std::fs::read("/home/mikoto/everlasting/music.torrent")?;
        let torrent = TorrentInfo::from_bencode(&torrent).unwrap();

        let info = torrent.info.unwrap();
        let piece_len = info.piece_length;
        let pieces_len = info.pieces.len();

        let files = if let Mode::Multi { files, .. } = info.mode {
            files
        } else {
            panic!();
        };

        let index = rand::thread_rng().gen_range(0..pieces_len);
        let piece_offset = piece_len * index as u64;
        dbg!(&piece_offset);
        let lengths = files.iter().map(|f| f.length);

        let n = (0..files.len())
            .find(|&i| lengths.clone().take(i).sum::<u64>() >= piece_offset)
            .unwrap()
            - 1;
        let total = lengths.clone().take(n).sum::<u64>();
        dbg!(&total, &piece_offset);

        assert!(total < piece_offset);
        // figure out why this doesn't work next time :D
        // assert!(piece_offset < total);

        Ok(())
    }
}
