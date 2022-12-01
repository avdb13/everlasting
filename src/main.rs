use crypto::digest::Digest;
use crypto::sha1::Sha1;
use rand::Rng;
use std::fs;
use url::Url;

use bendy::{decoding::FromBencode, encoding::Error};

pub struct Request {
    hash: [u8; 20],
    id: [u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    compact: bool,
}

fn encode(arr: &[u8]) -> String {
    arr.iter()
        .map(|&b| {
            match b {
                // hexadecimal number
                0x30..=0x39 | 0x41..=0x59 | 0x61..=0x79 => {
                    char::from_u32(b.into()).unwrap().to_string()
                }
                x => format!("%{:02x}", x).to_uppercase(),
            }
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let f = fs::read("1.torrent").unwrap();
    let metadata = Metadata::from_bencode(&f.to_vec()).unwrap();
    let mut hasher = Sha1::new();

    hasher.input(metadata.info.value.as_slice());

    let mut buf: [u8; 20] = Default::default();
    hasher.result(&mut buf);

    let id = rand::thread_rng().gen::<[u8; 20]>();
    let port = 6881;

    let options = [
        "info_hash=".to_owned() + &encode(&buf),
        "peer_id=".to_string() + &encode(&id),
        "port=".to_string() + &port.to_string(),
        "uploaded=".to_string() + "0",
        "downloaded=".to_string() + "0",
        "left=".to_string() + &metadata.info.piece_length.to_string(),
        "compact=".to_string() + "0", // + "no_peer_id="
        "event=".to_string() + "started", // + "ip="
                                      // + "numwant="
                                      // + "key="
                                      // + "trackerid="
    ]
    .join("&");

    let url = metadata.announce + "?" + &options;
    dbg!(&url);
    let resp = reqwest::get(url).await.unwrap();

    dbg!(resp.text().await.unwrap());

    Ok(())
}

pub enum Mode {
    Single,
    Multi,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Info {
    value: Vec<u8>,
    piece_length: u64,
    pieces: Vec<u8>,
    private: Option<bool>,
    name: String,
}

#[derive(Default, Debug, Eq, PartialEq)]
struct Metadata {
    info: Info,
    announce: String,
    announce_list: Option<Vec<Vec<String>>>,
    created: Option<u64>,
    comment: String,
    author: Option<String>,
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
            match pair {
                (b"info", _) => {
                    let value = pair.1.try_into_dictionary().unwrap();
                    md.info.value = value.into_raw().unwrap().to_vec();
                }
                (b"announce", _) => {
                    let s = String::decode_bencode_object(pair.1).unwrap();
                    md.announce = s;
                }
                (b"creation date", _) => {
                    let i = u64::decode_bencode_object(pair.1).unwrap();
                    md.created = Some(i);
                }
                (b"comment", _) => {
                    let s = String::decode_bencode_object(pair.1).unwrap();
                    md.comment = s;
                }
                (b"created by", _) => {
                    let s = String::decode_bencode_object(pair.1).unwrap();
                    md.author = Some(s);
                }

                (b"piece length", _) => {
                    let i = u64::decode_bencode_object(pair.1).unwrap();
                    md.info.piece_length = i;
                }
                (b"pieces", _) => {
                    let pieces = pair.1.try_into_bytes().unwrap();
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
                    let s = String::decode_bencode_object(pair.1).unwrap();
                }
            }
        }
        // Ok(Torrent {
        //     key: String::new(),
        //     value: String::new(),
        // })
        Ok(md)
    }
}

struct Node {
    url: Url,
}
