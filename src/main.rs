use color_eyre::{eyre::eyre, Report};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use rand::Rng;
use reqwest::{Client, ClientBuilder};
use std::fs;
use std::net::{IpAddr, SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{self, Interest};
use tokio::net::UdpSocket;
use tokio::time::sleep;
use url::Url;

pub mod bencode;
pub mod data;
pub mod dht;
pub mod helpers;
pub mod scrape;

use crate::dht::DHT;
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

#[derive(Error, Debug)]
pub enum GeneralError {
    #[error("usage: everlasting [torrent file | magnet link]")]
    Usage,
    #[error("magnet link contains no valid trackers: `{0:?}`")]
    NoTrackers(Vec<String>),
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    let s = std::env::args().nth(1).ok_or(GeneralError::Usage)?;

    match s.ends_with(".torrent") {
        false => {
            let src: SocketAddr = "0.0.0.0:1713".parse()?;
            let magnet_info = magnet_decoder(s.clone())?;
            dbg!(&magnet_info.trackers);

            let dst = Url::parse(&magnet_info.trackers[0])?;
            let dst = (dst.host_str().unwrap(), dst.port().unwrap())
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap();

            let dht: DHT = Default::default();
            dht.connect(src, dst).await?;
        }
        true => {
            panic!();
        }
    }
    let f = std::fs::read(&s)?;
    let metadata = Metadata::from_bencode(&f).unwrap();
    dbg!(&metadata.announce, &metadata.announce_list);

    let port = 1713;
    let pid = [
        "-XX1317-",
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
            Err(_) => {
                println!("Connection failed, retrying ...");
                sleep(Duration::from_secs(5)).await
            }
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
