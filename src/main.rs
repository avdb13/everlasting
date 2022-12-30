use crypto::digest::Digest;
use crypto::sha1::Sha1;
use rand::Rng;
use reqwest::{Client, ClientBuilder};
use std::fs;

pub mod bencode;
pub mod data;
pub mod helpers;
pub mod scrape;

use crate::helpers::*;
use crate::{data::*, scrape::Scraper};

use bendy::{decoding::FromBencode, encoding::Error};

pub struct Request {
    hash: [u8; 20],
    id: [u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    compact: bool,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let f = fs::read("3.torrent").unwrap();
    let metadata = Metadata::from_bencode(&f.to_vec()).unwrap();

    let port = 6881;

    let pid = [
        "-XP1000-",
        &(0..12)
            .map(|_| (rand::thread_rng().gen::<u8>() % 10).to_string())
            .collect::<String>(),
    ]
    .concat();

    let options = [
        "info_hash=".to_owned() + &encode(&metadata.info.value),
        "peer_id=".to_string() + &encode(pid.as_bytes()),
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

    let url = metadata.announce.clone() + "?" + &options;
    dbg!("making request to ".to_owned() + &url);

    let client = ClientBuilder::new().build().unwrap();
    let resp = loop {
        match client.get(url.clone()).send().await {
            Err(_) => println!("Connection failed, retrying ..."),
            Ok(resp) => break resp,
        }
    };

    let bytes = resp.bytes().await.unwrap();
    let resp = Response::from_bencode(&bytes).unwrap();
    dbg!(&resp);

    let scraper =
        Scraper(metadata.announce.clone() + "?" + "info_hash=" + &encode(&metadata.info.value));
    let ok = scraper.get().unwrap();
    let resp = client.get(ok).send().await.unwrap();

    let bytes = resp.bytes().await.unwrap();
    let resp = ScrapeResponse::from_bencode(&bytes).unwrap();

    Ok(())
}
