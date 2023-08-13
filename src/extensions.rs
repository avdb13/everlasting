use std::collections::HashMap;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;

use bendy::decoding::Decoder;
use bendy::decoding::Error as DecodingError;
use bendy::decoding::FromBencode;
use bendy::decoding::Object;
use bendy::encoding::Error as EncodingError;
use bendy::encoding::SingleItemEncoder;
use bendy::encoding::ToBencode;
use color_eyre::Report;

use crate::helpers::range_to_array;
use crate::pwp::Request;

#[derive(Debug, Clone)]
pub enum Message {
    Handshake(Handshake),
    Extension(Extension),
}

impl Default for Message {
    fn default() -> Self {
        let h = Handshake {
            inner: HashMap::new(),
            port: None,
            client: None,
            ext_ip: None,
            ipv4: None,
            ipv6: None,
            reqq: None,
        };

        Message::Handshake(h)
    }
}

#[derive(Default, Clone, Debug)]
pub struct Handshake {
    pub inner: HashMap<String, u32>,
    pub port: Option<u16>,
    pub client: Option<String>,
    pub ext_ip: Option<IpAddr>,
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
    pub reqq: Option<u8>,
}

#[derive(Default, Clone, Debug)]
pub enum Extension {
    #[default]
    None,
    Metadata {
        msg_type: MsgType,
        piece: u32,
        total_size: u32,
        payload: Option<Vec<u8>>,
    },
}

#[repr(u8)]
#[derive(Debug, Copy, Clone)]
pub enum MsgType {
    Request = 0,
    Data,
    Reject,
}

impl ToBencode for Message {
    const MAX_DEPTH: usize = 10;

    fn encode(&self, encoder: SingleItemEncoder) -> Result<(), EncodingError> {
        match self {
            Message::Handshake(h) => {
                let extensions = [("xv_metadata", 1), ("xv_pex", 2)];

                let ok = HashMap::from(extensions);

                encoder.emit_unsorted_dict(|e| {
                    e.emit_pair(b"m", &ok)?;
                    if let Some(port) = h.port {
                        e.emit_pair(b"p", port)?;
                    }
                    if let Some(client) = h.client {
                        e.emit_pair(b"v", &client)?;
                    }
                    if let Some(IpAddr::V4(ext_ip)) = h.ext_ip {
                        e.emit_pair(b"v", format!("{:?}", ext_ip))?;
                    }
                    if let Some(ipv4) = h.ipv4 {
                        e.emit_pair(b"ipv4", format!("{:?}", ipv4))?;
                    }
                    if let Some(ipv6) = h.ipv6 {
                        e.emit_pair(b"ipv6", format!("{:?}", ipv6))?;
                    }
                    if h.reqq.is_some() {
                        e.emit_pair(b"reqq", 250)?;
                    }
                    // e.emit_pair(b"metadate_size", 31235)

                    Ok(())
                })
            }
            Message::Extension(e) => match e {
                Extension::Metadata {
                    msg_type,
                    total_size,
                    piece,
                    payload,
                } => encoder.emit_unsorted_dict(|e| {
                    e.emit_pair(b"msg_type", *msg_type as u8)?;
                    e.emit_pair(b"piece", piece)?;

                    if let MsgType::Data = msg_type {
                        e.emit_pair(b"total_size", total_size)?;
                    }

                    Ok(())
                }),
                Extension::None => Ok(()),
            },
        }
    }
}

impl FromBencode for Message {
    fn decode_bencode_object(object: Object) -> Result<Self, DecodingError>
    where
        Self: Sized,
    {
        let mut message = Message::default();

        let mut dict = object.try_into_dictionary()?;
        while let Some(pair) = dict.next_pair()? {
            if let (b"msg_type", _) = pair {
                message = Message::Extension(Extension::default());
            }
        }

        // Decoder -> Object -> DictDecoder
        let mut dict = Decoder::new(dict.into_raw()?);
        let dict = dict.next_object()?.unwrap();
        let mut dict = dict.try_into_dictionary()?;
        let mut dict = object.try_into_dictionary()?;

        match message {
            Message::Handshake(mut h) => {
                while let Some(pair) = dict.next_pair()? {
                    match pair {
                        (b"m", _) => {
                            let mut dict = pair.1.try_into_dictionary()?;

                            while let Some(pair) = dict.next_pair()? {
                                let k = std::str::from_utf8(pair.0)?;
                                let v = pair.1.try_into_integer()?;

                                h.inner.insert(k.to_owned(), v.parse::<u32>()?);
                            }
                        }
                        (b"p", _) => {
                            let port = pair.1.try_into_integer()?;
                            let port = port.parse::<u16>()?;

                            h.port = Some(port);
                        }
                        (b"v", _) => {
                            let client = std::str::from_utf8(pair.1.try_into_bytes()?)?;

                            h.client = Some(client.to_owned());
                        }
                        (b"yourip", _) => {
                            let ext_ip = pair.1.try_into_bytes()?;
                            let ext_ip = match ext_ip.len() {
                                4 => IpAddr::V4(Ipv4Addr::from(range_to_array(ext_ip))),
                                16 => IpAddr::V6(Ipv6Addr::from(range_to_array(ext_ip))),
                                _ => panic!(),
                            };

                            h.ext_ip = Some(ext_ip);
                        }
                        (b"ipv4", _) => {
                            let ip = pair.1.try_into_bytes()?;
                            let ip = Ipv4Addr::from(range_to_array(ip));

                            h.ipv4 = Some(ip);
                        }
                        (b"ipv6", _) => {
                            let ip = pair.1.try_into_bytes()?;
                            let ip = Ipv6Addr::from(range_to_array(ip));

                            h.ipv6 = Some(ip);
                        }
                        (b"reqq", _) => {
                            let reqq = pair.1.try_into_integer()?;
                            let reqq = reqq.parse::<u8>()?;

                            h.reqq = Some(reqq);
                        }
                        _ => panic!(),
                    }
                }
            }
            Message::Extension(e) => match e {
                Extension::None => {}
                Extension::Metadata {
                    mut msg_type,
                    mut piece,
                    mut payload,
                    mut total_size,
                } => {
                    while let Some(pair) = dict.next_pair()? {
                        match pair {
                            (b"msg_type", _) => {
                                let i = pair.1.try_into_integer()?;
                                let i = i.parse::<u8>()?;
                                let i = match i {
                                    0 => MsgType::Request,
                                    1 => MsgType::Data,
                                    2 => MsgType::Reject,
                                };

                                msg_type = i;
                            }
                            (b"piece", _) => {
                                let i = pair.1.try_into_integer()?;

                                piece = i.parse::<u32>()?;
                            }
                            (b"total_size", _) => {
                                let i = pair.1.try_into_integer()?;

                                total_size = i.parse::<u32>()?;
                            }
                        }
                    }
                }
            },
        }

        Ok(message)
    }
}

impl Request for Message {
    fn to_request(&self) -> Vec<u8> {
        let len = |i: u32| i.to_be_bytes().as_slice();

        match self {
            Message::Handshake(h) => {
                let v = self.to_bencode().unwrap();
                [len(4 + 2), &[20u8], &[0u8], &v].concat()
            }
            Message::Extension(e) => match e {
                Extension::None => todo!(),
                Extension::Metadata { msg_type, .. } => {
                    let v = self.to_bencode().unwrap();
                    [len(4 + 2), &[20u8], &[*msg_type as u8], &v].concat()
                }
            },
        }
    }

    fn from_request(v: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        let prefix = &v[0..6];
        let message = Message::from_bencode(&v[6..]).unwrap();

        Some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bencode_extension() {
        let extensions = [
            ("xv_metadata".to_owned(), 1u32),
            ("xv_pex".to_owned(), 2u32),
        ];
        let h = Handshake {
            inner: HashMap::from(extensions),
            port: Some(6881),
            client: Some("µTorrent 1.2".to_owned()),
            ext_ip: None,
            ipv4: None,
            ipv6: None,
            reqq: None,
        };
        let m = Message::Handshake(h);

        let s = b"d1:md11:xv_metadatai1e6:xv_pexi2ee1:pi6881e1:v13:\xc2\xb5Torrent 1.2e";

        assert_eq!(
            m,
            b"d1:md11:xv_metadatai1e6:xv_pexi2ee1:pi6881e1:v13:\xc2\xb5Torrent 1.2e"
        );
    }

    #[test]
    fn test_request_message() {
        let expected = b"d8:msg_typei0e5:piecei0ee";
        let test = Extension::Metadata {
            msg_type: MsgType::Request,
            piece: 0,
            payload: None,
        };

        assert_eq!(expected.to_vec(), test.to_request().unwrap());
    }
    #[test]
    fn test_data_message() {
        let expected = b"d8:msg_typei1e5:piecei0e10:total_sizei8eexxxxxxxx";
        let test = Extension::Metadata {
            msg_type: MsgType::Data,
            piece: 0,
            payload: Some(b"xxxxxxxx".to_vec()),
        };

        assert_eq!(expected.to_vec(), test.to_request().unwrap());
    }
    #[test]
    fn test_reject_message() {
        let expected = b"d8:msg_typei2e5:piecei0ee";
        let test = Extension::Metadata {
            msg_type: MsgType::Reject,
            piece: 0,
            payload: None,
        };

        assert_eq!(expected.to_vec(), test.to_request().unwrap());
    }
}
