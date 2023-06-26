use std::net::SocketAddr;

use async_convert::async_trait;
use bendy::decoding::{Decoder as BendyDecoder, FromBencode};
use color_eyre::Report;
use reqwest::Url;
use tracing::debug;

use crate::{
    data::{GeneralError, MagnetInfo, ScrapeResponse, TorrentInfo},
    helpers::{decode, encode, udp_to_ip},
};

pub async fn to_torrent(magnet: MagnetInfo) -> Result<TorrentInfo, Report> {
    for tracker in magnet.trackers {
        // ~http://example.com/announce?x2%0644 -> ~http://example.com/scrape?x2%0644
        if !tracker.starts_with("http") {
            continue;
        }
        let tracker = tracker.replace("/announce", "/scrape");

        match reqwest::get(tracker + "&info_hash=" + &encode(&magnet.hash)).await {
            Ok(resp) => {
                let bytes = resp.bytes().await?;
                let resp = ScrapeResponse::from_bencode(&bytes).unwrap();
                debug!(?resp);

                // TorrentInfo {
                //     info,
                //     announce,
                //     created,
                //     comment,
                //     author,
                // }
            }
            Err(e) => {
                debug!(?e);
                continue;
            }
        }
    }
    unreachable!()
}
