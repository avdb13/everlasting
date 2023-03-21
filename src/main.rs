use crate::udp::{AnnounceReq, Event, Request, Response};
use color_eyre::{eyre::eyre, Report};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use futures_util::future;
use rand::seq::SliceRandom;
use rand::Rng;
use reqwest::ClientBuilder;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::sleep;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

pub mod bencode;
pub mod data;
pub mod dht;
pub mod helpers;
pub mod scrape;
pub mod udp;

use crate::dht::DHT;
use crate::helpers::*;
use crate::udp::ConnectReq;
use crate::{data::*, scrape::Scraper};

use bendy::{decoding::FromBencode, encoding::Error};

pub struct HttpRequest {
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
    InvalidTrackers(Vec<String>),
    #[error("no active trackers for this torrent")]
    DeadTrackers,
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "everlasting=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer().pretty())
        .init();

    let s = std::env::args().nth(1).ok_or(GeneralError::Usage)?;

    match s.ends_with(".torrent") {
        false => {
            let src: SocketAddr = "0.0.0.0:1713".parse()?;
            let magnet_info = magnet_decoder(s.clone())?;
            let packet = ConnectReq::build();
            let socket = Arc::new(UdpSocket::bind(src).await?);
            let mut dht = DHT {
                socket,
                target: None,
            };

            let peer_id = rand::thread_rng().gen::<[u8; 20]>();
            let key = rand::thread_rng().gen::<u32>();

            let iter = magnet_info.trackers.iter().map(|s| {
                let packet = packet.clone();
                let dht = dht.clone();

                Box::pin(async move {
                    tracing::debug!("trying {} as tracker ...", s);
                    let url = Url::parse(s)?;
                    let dst = (url.host_str().unwrap(), url.port().unwrap())
                        .to_socket_addrs()
                        .unwrap()
                        .next()
                        .unwrap();

                    match dht.connect(src, dst, packet).await {
                        Ok(x) => Ok((x, dst)),
                        Err(e) => Err(e),
                    }
                })
            });

            let (resp, _, _) = future::select_all(iter).await;

            tracing::debug!("active tracker found!");

            match resp? {
                x if x.0.action == 0i32 && x.0.tid == packet.tid => {
                    dht.target = Some(x.1);
                    println!("transaction ID matches: ({}, {})", packet.tid, x.0.tid);
                    println!("connection ID is {}", x.0.cid);
                }
                _ => {
                    // return GeneralError::DeadTrackers;
                }
            }

            let req = AnnounceReq {
                cid: packet.cid,
                action: 1i32,
                tid: rand::thread_rng().gen::<i32>(),
                hash: &decode(magnet_info.hash),
                peer_id: &peer_id,
                downloaded: 0,
                left: 0,
                uploaded: 0,
                event: Event::Stopped,
                socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
                key,
                num_want: -1i32,
                extensions: 0u16,
            };

            dht.announce(req).await?;
        }
        true => {
            panic!();
        }
    }

    panic!();
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
    let resp = HttpResponse::from_bencode(&bytes).unwrap();
    dbg!(&resp);

    let scraper =
        Scraper(metadata.announce.clone() + "?" + "info_hash=" + &encode(&metadata.info.value));
    let ok = scraper.get().unwrap();
    let resp = client.get(ok).send().await.unwrap();

    let bytes = resp.bytes().await.unwrap();
    let resp = ScrapeResponse::from_bencode(&bytes).unwrap();

    Ok(())
}
