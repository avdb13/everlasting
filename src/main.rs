use crypto::digest::Digest;
use crypto::sha1::Sha1;
use rand::Rng;
use reqwest::{Client, ClientBuilder};
use std::fs;
use std::net::{IpAddr, SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::sleep;
use url::Url;

pub mod bencode;
pub mod data;
pub mod dht;
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
    let s = std::env::args().nth(1).unwrap();
    match s.ends_with(".torrent") {
        true => {
            // let f = fs::read(s).unwrap();
            // request(Metadata::from_bencode(&f.to_vec()).unwrap()).await?
        }
        false => {
            let magnet_info = magnet_decoder(s).unwrap();
            let port = 6881;
            let pid = [
                "-XX1317-",
                &(0..12)
                    .map(|_| (rand::thread_rng().gen::<u8>() % 10).to_string())
                    .collect::<String>(),
            ]
            .concat();
            let options = [
                "info_hash=".to_owned()
                    + &encode(magnet_info.hash.split(':').last().unwrap().as_bytes()),
                "peer_id=".to_string() + &encode(pid.as_bytes()),
                "port=".to_string() + &port.to_string(),
            ]
            .join("&");

            let url = magnet_info.tracker + "?" + &options;
            let url = Url::parse(&url).unwrap();
            println!(
                "making request to {:?} on port {}",
                url.host_str()
                    .unwrap()
                    .to_socket_addrs()
                    .iter()
                    .collect::<Vec<_>>(),
                &url.port().unwrap().to_string(),
            );

            match url.scheme() == "udp" {
                true => {
                    let socket = UdpSocket::bind("0.0.0.0:1317").await.unwrap();
                    let mut msg = Vec::new();
                    msg.append(
                        &mut (0..8)
                            .map(|_| rand::thread_rng().gen::<u8>())
                            .chain((0..8).map(|_| 0))
                            .collect::<Vec<u8>>(),
                    );

                    let host = "89.234.156.205";
                    let ok = socket
                        .send_to(&msg, (host, url.port().unwrap()))
                        .await
                        .unwrap();
                    dbg!(&ok);

                    loop {
                        let mut buf = [0; 1024 * 10];
                        match socket.recv(&mut buf).await {
                            Ok(received) => {
                                println!("received {received} bytes {:?}", &buf[..received])
                            }
                            Err(e) => println!("recv function failed: {e:?}"),
                        }
                        dbg!(std::str::from_utf8(&buf).unwrap());
                    }
                }
                false => {
                    let client = ClientBuilder::new().build().unwrap();
                    let resp = loop {
                        match client.get(url.clone()).send().await {
                            Err(_) => {
                                sleep(Duration::from_secs(2)).await;
                                println!("Connection failed, retrying ...")
                            }
                            Ok(resp) => break resp,
                        }
                    };

                    let bytes = resp.bytes().await.unwrap();
                    // let resp = Response::from_bencode(&bytes).unwrap();
                    dbg!(&std::str::from_utf8(&bytes));
                }
            }
        }
    };

    Ok(())
}

// async fn request(metadata: Metadata) -> Result<(), Error> {
//     let port = 6881;
//     let pid = [
//         "-XX1317-",
//         &(0..12)
//             .map(|_| (rand::thread_rng().gen::<u8>() % 10).to_string())
//             .collect::<String>(),
//     ]
//     .concat();
//     let options = [
//         "info_hash=".to_owned() + &encode(&metadata.info.value),
//         "peer_id=".to_string() + &encode(pid.as_bytes()),
//         "port=".to_string() + &port.to_string(),
//         "uploaded=".to_string() + "0",
//         "downloaded=".to_string() + "0",
//         "left=".to_string() + &metadata.info.piece_length.to_string(),
//         "compact=".to_string() + "0", // + "no_peer_id="
//         "event=".to_string() + "started", // + "ip="
//                                       // + "numwant="
//                                       // + "key="
//                                       // + "trackerid="
//     ]
//     .join("&");

//     let url = metadata.announce.clone() + "?" + &options;
//     dbg!("making request to ".to_owned() + &url);

//     let client = ClientBuilder::new().build().unwrap();
//     let resp = loop {
//         match client.get(url.clone()).send().await {
//             Err(_) => println!("Connection failed, retrying ..."),
//             Ok(resp) => break resp,
//         }
//     };

//     let bytes = resp.bytes().await.unwrap();
//     let resp = Response::from_bencode(&bytes).unwrap();
//     dbg!(&resp);

//     let scraper =
//         Scraper(metadata.announce.clone() + "?" + "info_hash=" + &encode(&metadata.info.value));
//     let ok = scraper.get().unwrap();
//     let resp = client.get(ok).send().await.unwrap();

//     let bytes = resp.bytes().await.unwrap();
//     let resp = ScrapeResponse::from_bencode(&bytes).unwrap();

//     Ok(())
// }
