use hex::FromHex;
use std::convert::TryFrom;

use bendy::decoding::{Decoder, DictDecoder, FromBencode, Object};
use crypto::digest::Digest;
use crypto::sha1::Sha1;

use crate::data::{
    File, HttpResponse, Info, Metadata, Mode, MultiInfo, Peer, ScrapeResponse, SingleInfo, Status,
};

impl FromBencode for HttpResponse {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(
        object: bendy::decoding::Object,
    ) -> Result<Self, bendy::decoding::Error>
    where
        Self: Sized,
    {
        let mut dict = object.try_into_dictionary()?;

        let mut resp = HttpResponse::default();

        while let Some(pair) = dict.next_pair()? {
            dbg!(&std::str::from_utf8(pair.0).unwrap());
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

                    if let Object::Dict(mut dict) = pair.1 {
                        while let Some(pair) = dict.next_pair()? {
                            let mut peer = Peer::new();
                            match pair {
                                (b"peer id", _) => {
                                    peer.id = String::decode_bencode_object(pair.1)?;
                                }
                                (b"id", _) => {
                                    peer.ip = String::decode_bencode_object(pair.1)?;
                                }
                                (b"port", _) => {
                                    peer.port = u32::decode_bencode_object(pair.1)?.to_string();
                                }
                                _ => {}
                            }
                            peers.push(peer);
                        }
                    } else if let Object::Bytes(bytes) = pair.1 {
                        peers = bytes
                            .windows(6)
                            .map(|s| Peer {
                                id: "".to_string(),
                                ip: s[0..=4]
                                    .iter()
                                    .map(|x| x.to_string())
                                    .collect::<Vec<_>>()
                                    .join("."),
                                port: s[4..]
                                    .iter()
                                    .map(|x| x.to_string())
                                    .collect::<Vec<_>>()
                                    .join(""),
                            })
                            .collect::<Vec<Peer>>();
                    };
                    resp.peers = peers;
                }
                _ => {}
            }
        }

        Ok(resp)
    }
}

impl FromBencode for Info {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(
        object: bendy::decoding::Object,
    ) -> Result<Self, bendy::decoding::Error>
    where
        Self: Sized,
    {
        let mut mode = Mode::default();

        let mut dict = object.try_into_dictionary()?;
        while let Some(pair) = dict.next_pair()? {
            if let (b"files", _) = pair {
                mode = Mode::Multi(MultiInfo::default());
            }
        }

        // Decoder -> Object -> DictDecoder
        let mut dict = Decoder::new(dict.into_raw()?);
        let dict = dict.next_object()?.unwrap();
        let mut dict = dict.try_into_dictionary()?;

        match mode {
            Mode::Multi(_) => {
                let mut info = Info::default();
                let mut multi = MultiInfo::default();

                while let Some(pair) = dict.next_pair()? {
                    dbg!(&std::str::from_utf8(pair.0).unwrap());
                    match pair {
                        (b"piece length", _) => {
                            info.piece_length = u64::decode_bencode_object(pair.1)?;
                        }
                        (b"pieces", _) => {
                            info.pieces = pair.1.try_into_bytes()?.to_vec();
                        }
                        (b"private", _) => {
                            info.private = Some(());
                        }

                        (b"name", _) => {
                            multi.dir_name = String::decode_bencode_object(pair.1).unwrap();
                        }
                        (b"files", list) => {
                            let mut files: Vec<File> = Vec::new();
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
                info.mode = Mode::Multi(multi);
                Ok(info)
            }
            Mode::Single(_) => {
                let mut info = Info::default();
                let mut single = SingleInfo::default();

                while let Some(pair) = dict.next_pair()? {
                    dbg!(&std::str::from_utf8(pair.0).unwrap());
                    match pair {
                        (b"piece length", _) => {
                            info.piece_length = u64::decode_bencode_object(pair.1)?;
                        }
                        (b"pieces", _) => {
                            info.pieces = pair.1.try_into_bytes()?.to_vec();
                        }
                        (b"private", _) => {
                            info.private = Some(());
                        }
                        (b"name", _) => {
                            single.name = String::decode_bencode_object(pair.1)?;
                        }
                        (b"length", _) => {
                            single.length = u64::decode_bencode_object(pair.1)?;
                        }
                        (b"md5sum", _) => {
                            single.md5sum = Some(pair.1.try_into_bytes()?.to_vec());
                        }
                        _ => {}
                    }
                }

                info.mode = Mode::Single(single);
                Ok(info)
            }
        }
    }
}

impl FromBencode for Metadata {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(
        object: bendy::decoding::Object,
    ) -> Result<Self, bendy::decoding::Error>
    where
        Self: Sized,
    {
        let mut dict = object.try_into_dictionary()?;
        let mut md = Metadata::default();

        while let Some(pair) = dict.next_pair()? {
            dbg!(&std::str::from_utf8(pair.0).unwrap());
            match pair {
                (b"info", _) => {
                    if let Object::Dict(dd) = pair.1 {
                        let bytes = dd.into_raw()?;
                        dbg!(bytes
                            .iter()
                            .flat_map(|&x| match x.is_ascii() {
                                true => Some(x as char),
                                false => None,
                            })
                            .collect::<String>());
                        let mut hasher = Sha1::new();

                        hasher.input(bytes);

                        let ok = <[u8; 20]>::from_hex(hasher.result_str())?;
                        dbg!(ok, &ok, &ok.len());

                        let mut dec = Decoder::new(bytes);
                        let info = Info::decode_bencode_object(dec.next_object()?.unwrap())?;

                        md.info = info;
                        md.info.value = ok.to_vec();
                    }
                }
                (b"announce", _) => {
                    let s = String::decode_bencode_object(pair.1)?;
                    md.announce = s;
                }
                (b"announce-list", list) => {
                    let mut v: Vec<Vec<String>> = Vec::new();
                    let mut list = list.try_into_list()?;

                    if let Some(list) = list.next_object()? {
                        let mut list = list.try_into_list()?;

                        let mut w = Vec::new();

                        if let Some(s) = list.next_object()? {
                            let s = String::decode_bencode_object(s)?;
                            w.push(s);
                        }

                        v.push(w);
                    };
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
                    md.info.pieces = pieces.to_vec();
                }
                (b"nodes", _) => {
                    let mut list = pair.1.try_into_list()?;

                    while let Some(x) = list.next_object()? {
                        let mut x = x.try_into_list()?;

                        let fst = x.next_object()?;
                        let fst = String::decode_bencode_object(fst.unwrap()).unwrap();

                        let snd = x.next_object()?;
                        let snd = u64::decode_bencode_object(snd.unwrap()).unwrap();
                    }
                }
                _ => {
                    let s = String::decode_bencode_object(pair.1)?;
                }
            }
        }
        Ok(md)
    }
}

impl FromBencode for ScrapeResponse {
    const EXPECTED_RECURSION_DEPTH: usize = 5;

    fn decode_bencode_object(
        object: bendy::decoding::Object,
    ) -> Result<Self, bendy::decoding::Error>
    where
        Self: Sized,
    {
        // &bytes = b"d5:filesd20:\x1b\x84\xcb\xd2TZf\x8a\xd85\x04!\xd4\x1b\x88\x0f?d\xcc\xf4d8:completei17e10:incompletei2e10:downloadedi1539eeee"

        let mut dict = object.try_into_dictionary()?;
        let mut result = Vec::new();

        while let Some(files) = dict.next_pair()? {
            let mut files = files.1.try_into_dictionary()?;

            while let Some(file) = files.next_pair()? {
                let mut decoder = file.1.try_into_dictionary()?;
                let mut status: Status = Default::default();

                while let Some(pair) = decoder.next_pair()? {
                    dbg!(std::str::from_utf8(pair.0).unwrap());
                    match pair {
                        (b"complete", _) => {
                            status.seeders = u32::decode_bencode_object(pair.1)?;
                        }
                        (b"incomplete", _) => {
                            status.leechers = u32::decode_bencode_object(pair.1)?;
                        }
                        (b"downloaded", _) => {
                            let err = u32::decode_bencode_object(pair.1);
                            dbg!(err.unwrap());
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
