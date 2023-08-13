use std::{fmt, io::Cursor};

use bendy::encoding::ToBencode;

use crate::{
    extensions::{self, Extension},
    framing::{ParseCheck, ParseError},
    PEER_ID,
};

#[derive(Debug)]
pub struct State {
    pub choked: bool,
    pub interested: bool,
    pub peer_choked: bool,
    pub peer_interested: bool,
    pub dht_port: Option<u16>,
}

impl State {
    pub fn new() -> Self {
        Self {
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
            dht_port: None,
        }
    }
}

pub trait Request {
    fn to_request(&self) -> Vec<u8>;
    fn from_request(v: &[u8]) -> Option<Self>
    where
        Self: Sized;
}

// the peer id will presumably be sent after the recipient sends its own handshake
#[derive(Clone, Debug)]
pub struct Handshake {
    pub pstr: Option<String>,
    pub reserved: [u8; 8],
    pub hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub payload: Option<Vec<u8>>,
}

impl Handshake {
    pub fn new(hash: [u8; 20]) -> Self {
        let mut reserved = [0u8; 8];

        // DHT protocol
        reserved[7] |= 0x01;
        // extension protocol
        reserved[5] &= 0x20;

        Self {
            pstr: Some("BitTorrent protocol".to_owned()),
            reserved,
            hash,
            peer_id: *PEER_ID,
            payload: None,
        }
    }
}

impl Request for Handshake {
    fn to_request(&self) -> Vec<u8> {
        let pstr = self.pstr.as_ref().unwrap();
        [
            &[pstr.len() as u8],
            pstr.as_bytes(),
            &self.reserved,
            &self.hash,
            &self.peer_id,
        ]
        .concat()
    }

    fn from_request(v: &[u8]) -> Option<Self> {
        let reserved = v[20..28].try_into().ok()?;
        let hash = v[28..48].try_into().ok()?;
        let peer_id = v[48..68].try_into().ok()?;
        let payload = if v[68..].len() != 0 {
            Some(v[68..].to_vec())
        } else {
            None
        };

        Some(Self {
            pstr: None,
            reserved,
            hash,
            peer_id,
            payload,
        })
    }
}

impl ParseCheck for Handshake {
    // <1:pstrlen><19:pstr><8:reserved><20:info_hash><20:peer_id>

    fn check(v: &mut Cursor<&[u8]>) -> Result<(), ParseError> {
        let n: Vec<_> = (0..68).map_while(|_| Self::get_u8(v)).collect();
        if n.len() != 68 {
            return Err(ParseError::Incomplete);
        }

        Ok(())
    }

    fn parse(v: &mut Cursor<&[u8]>) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let rem: Vec<_> = (0..68).map_while(|_| Self::get_u8(v)).collect();
        if rem.len() != 68 {
            return Err(ParseError::Incomplete);
        }

        let reserved = rem[20..28].try_into().unwrap();
        let hash = rem[28..48].try_into().unwrap();
        let peer_id = rem[48..68].try_into().unwrap();

        Ok(Self {
            pstr: None,
            reserved,
            hash,
            peer_id,
            payload: Some(rem[68..].to_vec()),
        })
    }
}

#[repr(i8)]
#[derive(Clone, Debug)]
pub enum Message {
    Handshake(Handshake) = -1,
    Choke,
    Unchoke,
    Interested,
    Uninterested,
    Have(usize),
    BitField(Vec<usize>),
    Request {
        index: usize,
        begin: usize,
        length: usize,
    },
    Piece {
        index: usize,
        begin: usize,
        block: Vec<u8>,
    },
    Cancel {
        index: usize,
        begin: usize,
        length: usize,
    },
    Port(u16),
    Extended(extensions::Message) = 20,
}

impl Request for Message {
    fn to_request(&self) -> Vec<u8> {
        use Message::*;

        let len = |i: u32| i.to_be_bytes().as_slice();

        match self {
            Handshake(handshake) => handshake.to_request(),
            Choke => [len(1), &[0u8]].concat(),
            Unchoke => [len(1), &[1u8]].concat(),
            Interested => [len(1), &[2u8]].concat(),
            Uninterested => [len(1), &[3u8]].concat(),
            Have(x) => [len(5), &[4u8], &(*x as u32).to_be_bytes()].concat(),
            BitField(v) => {
                let v: Vec<_> = v.iter().flat_map(|v| v.to_be_bytes()).collect();
                [len(v.len() as u32 * 8 + 1), &[5], &v].concat()
            }
            Request {
                index,
                begin,
                length,
            } => [
                len(13),
                &[6u8],
                &(*index as u32).to_be_bytes(),
                &(*begin as u32).to_be_bytes(),
                &(*length as u32).to_be_bytes(),
            ]
            .concat(),
            Piece {
                index,
                begin,
                block,
            } => [
                len(9 + block.len() as u32),
                &[7u8],
                &(*index as u32).to_be_bytes(),
                &(*begin as u32).to_be_bytes(),
                block.as_slice(),
            ]
            .concat(),
            Cancel {
                index,
                begin,
                length,
            } => [
                len(13),
                &[8u8],
                &(*index as u32).to_be_bytes(),
                &(*begin as u32).to_be_bytes(),
                &(*length as u32).to_be_bytes(),
            ]
            .concat(),
            Port(i) => [len(3), &[9u8], &i.to_be_bytes()].concat(),
            Extended(ext_header) => {
                let v = ext_header.to_bencode().unwrap();
                [len(v.len() as u32 + 1), &[20u8], &v].concat()
            }
        }
    }

    fn from_request(v: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        let mut v = Cursor::new(v);
        Message::parse(&mut v).ok()
    }
}
impl ParseCheck for Message {
    fn check(v: &mut Cursor<&[u8]>) -> Result<(), ParseError> {
        let n: Vec<_> = (0..4).map_while(|_| Self::get_u8(v)).collect();
        if n.len() != 4 {
            return Err(ParseError::Incomplete);
        }
        let n = u32::from_be_bytes(n.try_into().unwrap()) as usize;

        let rem: Vec<_> = (0..n).map(|_| Self::get_u8(v)).collect();
        if rem.len() != n {
            return Err(ParseError::Incomplete);
        }

        Ok(())
    }

    fn parse(v: &mut Cursor<&[u8]>) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        use Message::*;

        let n: Vec<_> = (0..4).map_while(|_| Self::get_u8(v)).collect();
        if n.len() != 4 {
            return Err(ParseError::Incomplete);
        }
        let n = u32::from_be_bytes(n.try_into().unwrap()) as usize;

        let mut rem: Vec<_> = (0..n).map_while(|_| Self::get_u8(v)).collect();
        if rem.len() != n {
            return Err(ParseError::Incomplete);
        }

        let res = match rem[0] {
            0 => Choke,
            1 => Unchoke,
            2 => Interested,
            3 => Uninterested,
            4 => Have(u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize),
            5 => {
                let tail = (rem.len() - 1) % 8;
                if tail != 0 {
                    rem.append(&mut vec![0u8; 8 - tail]);
                }

                let payload = rem[1..rem.len()]
                    .chunks(std::mem::size_of::<usize>() / 8)
                    .map(|c| usize::from_be_bytes(c.try_into().unwrap()))
                    .collect();

                BitField(payload)
            }
            6 => {
                let index = u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize;
                let begin = u32::from_be_bytes(rem[5..9].try_into().unwrap()) as usize;
                let length = u32::from_be_bytes(rem[9..13].try_into().unwrap()) as usize;

                Request {
                    index,
                    begin,
                    length,
                }
            }
            7 => {
                let index = u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize;
                let begin = u32::from_be_bytes(rem[5..9].try_into().unwrap()) as usize;

                let block = rem[..rem.len()].to_vec();

                Piece {
                    index,
                    begin,
                    block,
                }
            }
            8 => {
                let index = u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize;
                let begin = u32::from_be_bytes(rem[5..9].try_into().unwrap()) as usize;
                let length = u32::from_be_bytes(rem[9..13].try_into().unwrap()) as usize;

                Cancel {
                    index,
                    begin,
                    length,
                }
            }
            9 => {
                let port = u16::from_be_bytes(rem[1..3].try_into().unwrap());

                Port(port)
            }
            20 => Extended,
            _ => panic!(),
        };

        Ok(res)
    }
}
