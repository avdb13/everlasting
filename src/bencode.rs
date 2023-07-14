use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs},
};

use hex::FromHex;

use bendy::{
    decoding::{Decoder, Error as DecodingError, FromBencode, Object},
    encoding::{
        AsString, Encoder, Error as EncodingError, SingleItemEncoder, ToBencode,
        UnsortedDictEncoder,
    },
};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use tracing::debug;
use url::Url;

use crate::{
    data::{
        File, GeneralError, HttpResponse, Info, Mode, Peer, ScrapeResponse, Status, TorrentInfo,
        SHA1_LEN,
    },
    helpers::range_to_array,
};

impl FromBencode for TorrentInfo {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(object: bendy::decoding::Object) -> Result<Self, DecodingError>
    where
        Self: Sized,
    {
        let mut dict = object.try_into_dictionary()?;
        let mut md = TorrentInfo::default();

        while let Some(pair) = dict.next_pair()? {
            match pair {
                (b"info", _) => {
                    if let Object::Dict(dd) = pair.1 {
                        let bytes = dd.into_raw()?;
                        let mut hasher = Sha1::new();

                        hasher.input(bytes);

                        let hash = <[u8; 20]>::from_hex(hasher.result_str())?;

                        let mut dec = Decoder::new(bytes);
                        let info = Info::decode_bencode_object(dec.next_object()?.unwrap())?;

                        md.info = info;
                        md.info.value = hash;
                    }
                }
                (b"announce", _) => {
                    let s = String::decode_bencode_object(pair.1)?;
                    if s.starts_with("http") {
                        md.announce.http.push(s.clone());
                    }
                    if s.starts_with("udp") {
                        let url = Url::parse(&s)?;
                        let host = url.host().unwrap();
                        let port = url.port().unwrap();

                        if let Ok(mut addr) = (host.to_string(), port).to_socket_addrs() {
                            md.announce.udp.push(addr.next().unwrap());
                        }
                    }
                }
                (b"announce-list", list) => {
                    // Not bothering separating announce and announce-list since they both contain
                    // tracker URLs.
                    let mut list = list.try_into_list()?;

                    while let Some(list) = list.next_object()? {
                        let mut list = list.try_into_list()?;

                        while let Some(s) = list.next_object()? {
                            let s = String::decode_bencode_object(s)?;

                            if s.starts_with("http") {
                                md.announce.http.push(s.clone());
                            }
                            if s.starts_with("udp") {
                                let url = Url::parse(&s)?;
                                let host = url.host().ok_or(GeneralError::InvalidUdpTracker(s))?;
                                let port = url.port().unwrap_or(80);

                                if let Ok(mut addr) = (host.to_string(), port).to_socket_addrs() {
                                    md.announce.udp.push(addr.next().unwrap());
                                }
                            }
                        }
                    }
                }
                (b"creation date", _) => {
                    let i = u64::decode_bencode_object(pair.1)?;
                    md.created = Some(i);
                }
                (b"comment", _) => {
                    let s = String::decode_bencode_object(pair.1)?;
                    md.comment = s;
                }
                (b"created by", _) => {
                    let s = String::decode_bencode_object(pair.1)?;
                    md.author = Some(s);
                }

                (b"piece length", _) => {
                    let i = u64::decode_bencode_object(pair.1)?;
                    md.info.piece_length = i;
                }
                (b"pieces", _) => {
                    let pieces = pair.1.try_into_bytes()?;
                    let pieces: Vec<_> = pieces
                        .chunks(pieces.len() / SHA1_LEN)
                        .map(|x| range_to_array(&x[0..=20]))
                        .collect();
                    md.info.pieces = pieces;
                }
                (b"nodes", _) => {
                    let mut list = pair.1.try_into_list()?;

                    while let Some(x) = list.next_object()? {
                        let mut x = x.try_into_list()?;

                        let fst = x.next_object()?;
                        let _fst = String::decode_bencode_object(fst.unwrap()).unwrap();

                        let snd = x.next_object()?;
                        let _snd = u64::decode_bencode_object(snd.unwrap()).unwrap();
                    }
                }
                _ => {
                    let _s = String::decode_bencode_object(pair.1)?;
                }
            }
        }
        Ok(md)
    }
}

impl FromBencode for Info {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(object: bendy::decoding::Object) -> Result<Self, DecodingError>
    where
        Self: Sized,
    {
        let mut info = Info::default();

        let mut dict = object.try_into_dictionary()?;
        while let Some(pair) = dict.next_pair()? {
            if let (b"files", _) = pair {
                info.mode = Mode::Multi {
                    dir_name: Default::default(),
                    files: Default::default(),
                    md5sum: Default::default(),
                };
            }
        }

        // Decoder -> Object -> DictDecoder
        let mut dict = Decoder::new(dict.into_raw()?);
        let dict = dict.next_object()?.unwrap();
        let mut dict = dict.try_into_dictionary()?;

        let mode: Mode = match info.mode {
            Mode::Multi {
                mut dir_name,
                mut files,
                // TODO: check whether md5sums are still relevant in $CURRENT_YEAR.
                md5sum,
            } => {
                while let Some(pair) = dict.next_pair()? {
                    dbg!(&std::str::from_utf8(pair.0).unwrap());
                    match pair {
                        (b"piece length", _) => {
                            let i = u64::decode_bencode_object(pair.1)?;
                            dbg!("piece length: {}", i);
                            info.piece_length = i;
                        }
                        (b"pieces", _) => {
                            let pieces = pair.1.try_into_bytes()?;
                            dbg!(&pieces.len());
                            let pieces: Vec<_> = pieces
                                .chunks(pieces.len() / SHA1_LEN)
                                .map(|x| range_to_array(&x[0..=20]))
                                .collect();
                            dbg!(&pieces, &pieces.len());
                            info.pieces = pieces;
                        }
                        (b"private", _) => {
                            info.private = Some(());
                        }

                        (b"name", _) => {
                            dir_name = String::decode_bencode_object(pair.1).unwrap();
                        }
                        (b"files", list) => {
                            let mut list = list.try_into_list()?;

                            while let Some(file) = list.next_object()? {
                                let mut file = file.try_into_dictionary()?;

                                while let Some(pair) = file.next_pair()? {
                                    let mut file = File::default();

                                    match pair {
                                        (b"length", _) => {
                                            file.length = u64::decode_bencode_object(pair.1)?;
                                        }
                                        (b"md5sum", _) => {
                                            file.md5sum =
                                                Some(pair.1.try_into_bytes().unwrap().to_vec());
                                        }
                                        (b"path", _) => {
                                            let mut path = pair.1.try_into_list()?;
                                            while let Some(x) = path.next_object()? {
                                                let s = String::decode_bencode_object(x)?;
                                                file.path.push(s);
                                            }
                                        }
                                        _ => {}
                                    }

                                    files.push(file);
                                }
                            }
                        }
                        _ => {}
                    }
                }

                Mode::Multi {
                    dir_name,
                    files,
                    md5sum,
                }
            }
            Mode::Single {
                mut name,
                mut length,
                mut md5sum,
            } => {
                while let Some(pair) = dict.next_pair()? {
                    match pair {
                        (b"piece length", _) => {
                            let len = u64::decode_bencode_object(pair.1)?;
                            dbg!(&len);
                            info.piece_length = len;
                        }
                        (b"pieces", _) => {
                            let pieces = pair.1.try_into_bytes()?;
                            let pieces: Vec<_> = pieces
                                .chunks(SHA1_LEN)
                                .map(|x| range_to_array(&x[0..20]))
                                .collect();
                            dbg!(&pieces.len());
                            info.pieces = pieces;
                        }
                        (b"private", _) => {
                            info.private = Some(());
                        }
                        (b"name", _) => {
                            name = String::decode_bencode_object(pair.1)?;
                        }
                        (b"length", _) => {
                            length = u64::decode_bencode_object(pair.1)?;
                        }
                        (b"md5sum", _) => {
                            md5sum = Some(pair.1.try_into_bytes()?.to_vec());
                        }
                        _ => {}
                    }
                }

                Mode::Single {
                    name,
                    length,
                    md5sum,
                }
            }
        };

        info.mode = mode;

        Ok(info)
    }
}

impl FromBencode for HttpResponse {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(object: bendy::decoding::Object) -> Result<Self, DecodingError>
    where
        Self: Sized,
    {
        let mut dict = object.try_into_dictionary()?;

        let mut resp = HttpResponse::default();

        while let Some(pair) = dict.next_pair()? {
            match pair {
                (b"failure reason", _) => {
                    let s = String::decode_bencode_object(pair.1)?;
                    resp.failure_reason = Some(s);
                }
                (b"warning message", _) => {
                    let s = String::decode_bencode_object(pair.1)?;
                    resp.warning = Some(s);
                }
                (b"interval", _) => {
                    let i = u64::decode_bencode_object(pair.1)?;
                    resp.interval = i;
                }
                (b"min interval", _) => {
                    let i = u64::decode_bencode_object(pair.1)?;
                    resp.min_interval = Some(i);
                }
                (b"peers", _) => {
                    let mut peers = Vec::new();

                    match pair.1 {
                        Object::List(mut list) => {
                            while let Some(dict) = list.next_object()? {
                                let mut peer_id: Option<[u8; 20]> = Default::default();
                                let mut ip = Default::default();
                                let mut port = Default::default();

                                let mut dict = dict.try_into_dictionary()?;

                                while let Some(pair) = dict.next_pair()? {
                                    match pair {
                                        (b"peer id", _) => {
                                            let AsString(s) =
                                                AsString::decode_bencode_object(pair.1)?;
                                            peer_id = Some(range_to_array(&s));
                                        }
                                        (b"ip", _) => {
                                            let s = String::decode_bencode_object(pair.1)?;
                                            ip = Some(s.parse::<IpAddr>()?);
                                        }
                                        (b"port", _) => {
                                            port = Some(u16::decode_bencode_object(pair.1)?);
                                        }
                                        _ => {}
                                    }
                                }
                                let peer = Peer {
                                    // Unwrapping because all values have to be initialized before
                                    // continuing.
                                    id: Some(peer_id.unwrap()),
                                    addr: SocketAddr::from((ip.unwrap(), port.unwrap())),
                                };
                                peers.push(peer);
                            }
                        }
                        Object::Bytes(bytes) => {
                            assert_eq!(bytes.len() % 6, 0);

                            for chunk in bytes.chunks(6) {
                                let ip = Ipv4Addr::from(range_to_array(&chunk[..4]));
                                let port = u16::from_be_bytes(range_to_array(&chunk[4..]));

                                let peer = Peer {
                                    id: None,
                                    addr: SocketAddr::from((ip, port)),
                                };

                                peers.push(peer);
                            }
                        }
                        _ => {
                            panic!();
                        }
                    }
                    resp.peers = peers;
                }
                _ => {}
            }
        }

        Ok(resp)
    }
}

impl FromBencode for ScrapeResponse {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(object: bendy::decoding::Object) -> Result<Self, DecodingError>
    where
        Self: Sized,
    {
        let mut dict = object.try_into_dictionary()?;
        let mut result = Vec::new();

        while let Some(files) = dict.next_pair()? {
            let mut files = files.1.try_into_dictionary()?;

            while let Some(file) = files.next_pair()? {
                let mut decoder = file.1.try_into_dictionary()?;
                let mut status: Status = Default::default();

                while let Some(pair) = decoder.next_pair()? {
                    match pair {
                        (b"complete", _) => {
                            status.seeders = u32::decode_bencode_object(pair.1)?;
                        }
                        (b"incomplete", _) => {
                            status.leechers = u32::decode_bencode_object(pair.1)?;
                        }
                        (b"downloaded", _) => {
                            let err = u32::decode_bencode_object(pair.1)?;
                        }
                        _ => {}
                    }
                }
                result.push((file.0.to_vec(), status));
            }
        }
        Ok(Self { files: result })
    }
}
