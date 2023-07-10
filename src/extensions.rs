use std::collections::HashMap;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;

use bendy::decoding::Error as DecodingError;
use bendy::decoding::FromBencode;
use bendy::decoding::Object;
use bendy::encoding::Error as EncodingError;
use bendy::encoding::SingleItemEncoder;
use bendy::encoding::ToBencode;
use color_eyre::Report;

use crate::helpers::range_to_array;

#[derive(Default, Debug)]
pub struct ExtensionHeader {
    pub dict: HashMap<String, u32>,
    pub port: Option<u16>,
    pub client: Option<String>,
    pub ext_ip: Option<IpAddr>,
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
    pub reqq: Option<u8>,
}

impl ToBencode for ExtensionHeader {
    const MAX_DEPTH: usize = 3;

    fn encode(&self, encoder: SingleItemEncoder) -> Result<(), EncodingError> {
        let ok = HashMap::from([("xv_metadata", 1), ("xv_pex", 2)]);

        encoder.emit_unsorted_dict(|e| {
            e.emit_pair(b"m", &ok)?;
            e.emit_pair(b"p", 6881)?;
            e.emit_pair(b"v", "µTorrent 1.2")
            // e.emit_pair(b"metadate_size", 31235)
        })
    }
}

impl FromBencode for ExtensionHeader {
    fn decode_bencode_object(object: Object) -> Result<Self, DecodingError>
    where
        Self: Sized,
    {
        let mut dict = object.try_into_dictionary()?;
        let mut extension_header = ExtensionHeader::default();

        while let Some(pair) = dict.next_pair()? {
            match pair {
                (b"m", _) => {
                    let mut dict = pair.1.try_into_dictionary()?;

                    while let Some(pair) = dict.next_pair()? {
                        let k = std::str::from_utf8(pair.0)?;
                        let v = pair.1.try_into_integer()?;

                        extension_header
                            .dict
                            .insert(k.to_owned(), v.parse::<u32>()?);
                    }
                }
                (b"p", _) => {
                    let port = pair.1.try_into_integer()?;
                    let port = port.parse::<u16>()?;

                    extension_header.port = Some(port);
                }
                (b"v", _) => {
                    let client = std::str::from_utf8(pair.1.try_into_bytes()?)?;

                    extension_header.client = Some(client.to_owned());
                }
                (b"yourip", _) => {
                    let ext_ip = pair.1.try_into_bytes()?;
                    let ext_ip = match ext_ip.len() {
                        4 => IpAddr::V4(Ipv4Addr::from(range_to_array(ext_ip))),
                        16 => IpAddr::V6(Ipv6Addr::from(range_to_array(ext_ip))),
                        _ => panic!(),
                    };

                    extension_header.ext_ip = Some(ext_ip);
                }
                (b"ipv4", _) => {
                    let ip = pair.1.try_into_bytes()?;
                    let ip = Ipv4Addr::from(range_to_array(ip));

                    extension_header.ipv4 = Some(ip);
                }
                (b"ipv6", _) => {
                    let ip = pair.1.try_into_bytes()?;
                    let ip = Ipv6Addr::from(range_to_array(ip));

                    extension_header.ipv6 = Some(ip);
                }
                (b"reqq", _) => {
                    let reqq = pair.1.try_into_integer()?;
                    let reqq = reqq.parse::<u8>()?;

                    extension_header.reqq = Some(reqq);
                }
                _ => panic!(),
            }
        }

        Ok(extension_header)
    }
}

pub enum Extensions<'p> {
    Metadata {
        msg_type: MsgType,
        piece: u32,
        payload: Option<&'p [u8]>,
    },
}

#[repr(u8)]
#[derive(Debug, Copy, Clone)]
pub enum MsgType {
    Request = 0,
    Data,
    Reject,
}

impl ToBencode for Extensions<'_> {
    const MAX_DEPTH: usize = 5;

    fn encode(&self, encoder: SingleItemEncoder) -> Result<(), EncodingError> {
        match &self {
            Extensions::Metadata {
                msg_type,
                piece,
                payload,
            } => encoder.emit_unsorted_dict(|e| {
                e.emit_pair(b"msg_type", *msg_type as u8)?;
                e.emit_pair(b"piece", piece)?;

                if let MsgType::Data = msg_type {
                    e.emit_pair(b"total_size", payload.unwrap().len())?;
                }

                Ok(())
            }),
        }
    }
}

impl Extensions<'_> {
    fn to_request(&self) -> Result<Vec<u8>, Report> {
        match self {
            Extensions::Metadata {
                msg_type, payload, ..
            } => {
                if let MsgType::Data = msg_type {
                    Ok([self.to_bencode().unwrap(), payload.unwrap().to_vec()].concat())
                } else {
                    Ok(self.to_bencode().unwrap())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bencode_extension() {
        let ok = ExtensionHeader::default().to_bencode().unwrap();
        let or = b"d1:md11:xv_metadatai1e6:xv_pexi2ee1:pi6881e1:v13:\xc2\xb5Torrent 1.2e";

        dbg!(
            std::str::from_utf8(&ok).unwrap(),
            std::str::from_utf8(or).unwrap()
        );
        assert_eq!(
            ok,
            b"d1:md11:xv_metadatai1e6:xv_pexi2ee1:pi6881e1:v13:\xc2\xb5Torrent 1.2e"
        );
    }

    #[test]
    fn test_request_message() {
        let expected = b"d8:msg_typei0e5:piecei0ee";
        let test = Extensions::Metadata {
            msg_type: MsgType::Request,
            piece: 0,
            payload: None,
        };

        assert_eq!(expected.to_vec(), test.to_request().unwrap());
    }
    #[test]
    fn test_data_message() {
        let expected = b"d8:msg_typei1e5:piecei0e10:total_sizei8eexxxxxxxx";
        let test = Extensions::Metadata {
            msg_type: MsgType::Data,
            piece: 0,
            payload: Some(b"xxxxxxxx"),
        };

        assert_eq!(expected.to_vec(), test.to_request().unwrap());
    }
    #[test]
    fn test_reject_message() {
        let expected = b"d8:msg_typei2e5:piecei0ee";
        let test = Extensions::Metadata {
            msg_type: MsgType::Reject,
            piece: 0,
            payload: None,
        };

        assert_eq!(expected.to_vec(), test.to_request().unwrap());
    }
}
