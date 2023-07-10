use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use bendy::{
    decoding::{self, Decoder, FromBencode, Object},
    encoding::{self, AsString, Encoder, SingleItemEncoder, ToBencode},
};
use rand::Rng;

use crate::dht::Node;

pub type NodeContact = (Node, SocketAddr);

#[derive(Debug, PartialEq)]
pub enum Message {
    Query(Arguments),
    Response(Values),
    Err(Error),
}

#[derive(Debug, PartialEq)]
pub struct ExtMessage {
    pub inner: Message,
    pub transaction_id: String,
}

impl From<Message> for ExtMessage {
    fn from(inner: Message) -> ExtMessage {
        let transaction_id = rand::thread_rng().gen::<[char; 2]>().iter().collect();
        ExtMessage {
            inner,
            transaction_id,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Error {
    pub description: String,
    pub kind: ErrorKind,
}

#[derive(Default, Debug, Clone, PartialEq)]
pub enum ErrorKind {
    #[default]
    Generic = 201,
    Server,
    Protocol,
    MethodUnknown,
}

impl From<i64> for ErrorKind {
    fn from(v: i64) -> Self {
        use ErrorKind::*;

        match v {
            x if x == Generic as i64 => Generic,
            x if x == Server as i64 => Server,
            x if x == Protocol as i64 => Protocol,
            x if x == MethodUnknown as i64 => MethodUnknown,
            _ => panic!(),
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub enum Method {
    #[default]
    Ping,
    FindNode,
    GetPeers,
    AnnouncePeer,
}

impl From<&Method> for Vec<u8> {
    fn from(value: &Method) -> Self {
        match value {
            Method::Ping => b"ping".to_vec(),
            Method::FindNode => b"find_node".to_vec(),
            Method::GetPeers => b"get_peers".to_vec(),
            Method::AnnouncePeer => b"announce_peer".to_vec(),
        }
    }
}

impl From<&ExtMessage> for Vec<u8> {
    fn from(value: &ExtMessage) -> Self {
        match value.inner {
            Message::Query(_) => b"q".to_vec(),
            Message::Response(_) => b"r".to_vec(),
            Message::Err(_) => b"e".to_vec(),
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Arguments {
    pub method: Method,
    pub id: [u8; 20],
    pub target: Option<[u8; 20]>,
    pub info_hash: Option<[u8; 20]>,
    pub port: Option<u16>,
    pub implied_port: Option<bool>,
    pub token: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct CompactNode {
    pub id: [u8; 20],
    pub ip: SocketAddr,
}

impl From<Vec<u8>> for CompactNode {
    fn from(v: Vec<u8>) -> Self {
        assert!(v.len() == 26);

        let ip = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(v[20], v[21], v[22], v[23])),
            u16::from_be_bytes(v[24..26].try_into().unwrap()),
        );
        CompactNode {
            id: v[..20].try_into().unwrap(),
            ip,
        }
    }
}

impl From<&CompactNode> for Vec<u8> {
    fn from(v: &CompactNode) -> Self {
        match v.ip.ip() {
            IpAddr::V4(ip) => [
                v.id.as_slice(),
                ip.octets().as_slice(),
                &v.ip.port().to_be_bytes(),
            ]
            .concat()
            .to_vec(),
            IpAddr::V6(_) => todo!(),
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Values {
    pub id: [u8; 20],
    pub nodes: Option<Vec<CompactNode>>,
    pub values: Option<Vec<CompactNode>>,
    pub token: Option<String>,
}

impl FromBencode for ExtMessage {
    fn decode_bencode_object(object: Object) -> Result<Self, decoding::Error>
    where
        Self: Sized,
    {
        let mut dict = object.try_into_dictionary()?;

        let mut message: ExtMessage = Message::Err(Error::default()).into();
        let mut method: Method = Default::default();
        let mut transaction_id: String = Default::default();
        let mut payload: Option<Vec<u8>> = None;

        while let Some(pair) = dict.next_pair()? {
            match pair {
                (b"t", _) => {
                    transaction_id = String::decode_bencode_object(pair.1)?;
                }
                (b"y", _) => {
                    message = match String::decode_bencode_object(pair.1)?.as_str() {
                        "q" => Message::Query(Arguments::default()).into(),
                        "r" => Message::Response(Values::default()).into(),
                        "e" => Message::Err(Error::default()).into(),
                        _ => panic!(),
                    };
                }
                (b"q", _) => {
                    method = match String::decode_bencode_object(pair.1)?.as_str() {
                        "ping" => Method::Ping,
                        "find_node" => Method::FindNode,
                        "get_peers" => Method::GetPeers,
                        "announce_peer" => Method::AnnouncePeer,
                        _ => panic!(),
                    };
                }
                (b"a", _) | (b"r", _) => {
                    payload = Some(pair.1.try_into_dictionary()?.into_raw()?.to_vec());
                }
                (b"e", _) => {
                    payload = Some(pair.1.try_into_list()?.into_raw()?.to_vec());
                }
                _ => panic!(),
            };
        }

        let payload = payload.unwrap();
        let mut dict = Decoder::new(payload.as_slice());
        let dict = dict.next_object()?.unwrap();

        match message.inner {
            Message::Query(mut arguments) => {
                let mut dict = dict.try_into_dictionary()?;

                arguments.method = method;

                while let Some(pair) = dict.next_pair()? {
                    match pair {
                        (b"id", _) => {
                            let s = String::decode_bencode_object(pair.1)?;
                            arguments.id = s.as_bytes().try_into()?;
                        }
                        (b"target", _) => {
                            let s = String::decode_bencode_object(pair.1)?;
                            arguments.target = Some(s.as_bytes().try_into()?);
                        }
                        (b"info_hash", _) => {
                            let s = String::decode_bencode_object(pair.1)?;
                            arguments.info_hash = Some(s.as_bytes().try_into()?);
                        }
                        (b"implied_port", _) => {
                            let i = u64::decode_bencode_object(pair.1)?;
                            arguments.implied_port = Some(i != 0);
                        }
                        (b"port", _) => {
                            let i = u16::decode_bencode_object(pair.1)?;
                            arguments.port = Some(i);
                        }
                        (b"token", _) => {
                            let s = String::decode_bencode_object(pair.1)?;
                            arguments.token = Some(s);
                        }
                        _ => panic!(),
                    }
                }

                Ok(ExtMessage {
                    inner: Message::Query(arguments),
                    transaction_id,
                })
            }
            Message::Response(mut values) => {
                let mut dict = dict.try_into_dictionary()?;

                while let Some(pair) = dict.next_pair()? {
                    match pair {
                        (b"id", _) => {
                            let s = String::decode_bencode_object(pair.1)?;
                            values.id = s.as_bytes().try_into()?;
                        }
                        (b"nodes", _) => {
                            let s = String::decode_bencode_object(pair.1)?;
                            let v = s.as_bytes();

                            let nodes = v
                                .chunks_exact(26)
                                .map(|x| x.to_vec().into())
                                .collect::<Vec<_>>();

                            values.nodes = Some(nodes);
                        }
                        (b"values", _) => {
                            let mut list = pair.1.try_into_list()?;
                            let mut res: Vec<CompactNode> = Vec::new();

                            while let Some(v) = list.next_object()? {
                                let s = String::decode_bencode_object(v)?;
                                res.push(s.as_bytes().to_vec().into());
                            }

                            values.values = Some(res);
                        }
                        (b"token", _) => {
                            let s = String::decode_bencode_object(pair.1)?;

                            values.token = Some(s);
                        }
                        _ => {}
                    }
                }

                Ok(ExtMessage {
                    inner: Message::Response(values),
                    transaction_id,
                })
            }
            Message::Err(mut e) => {
                let mut list = dict.try_into_list()?;

                let code = list.next_object()?.unwrap();
                e.kind = ErrorKind::try_from(i64::decode_bencode_object(code)?).unwrap();

                let description = list.next_object()?.unwrap();
                e.description = String::decode_bencode_object(description)?;

                Ok(ExtMessage {
                    inner: Message::Err(e),
                    transaction_id,
                })
            }
        }
    }
}

impl ToBencode for ExtMessage {
    const MAX_DEPTH: usize = 5;

    fn encode(&self, encoder: SingleItemEncoder) -> Result<(), encoding::Error> {
        let mut tokens: Vec<(&[u8], Vec<u8>)> = Vec::new();

        let message = <Vec<u8>>::try_from(self).unwrap();
        tokens.push((b"y", message.clone()));
        tokens.push((b"t", self.transaction_id.clone().into()));

        if let Message::Query(args) = &self.inner {
            let method = <Vec<u8>>::from(&args.method);
            tokens.push((b"q", method));
        }

        encoder.emit_unsorted_dict(|e| {
            for (k, v) in tokens {
                let v = AsString(v);
                e.emit_pair(k, v)?;
            }

            let dict_key = match message.as_slice() {
                b"q" => b"a",
                b"r" => b"r",
                b"e" => b"e",
                _ => panic!(),
            };

            match &self.inner {
                Message::Query(args) => e.emit_pair(dict_key, args),
                Message::Response(values) => e.emit_pair(dict_key, values),
                Message::Err(err) => e.emit_pair(dict_key, err),
            }
        })
    }
}

impl ToBencode for Arguments {
    const MAX_DEPTH: usize = 5;

    fn encode(&self, encoder: SingleItemEncoder) -> Result<(), encoding::Error> {
        let mut tokens: Vec<(&[u8], Vec<u8>)> = Vec::new();

        tokens.push((b"id", self.id.to_vec()));

        match self.method {
            Method::Ping => {}
            Method::FindNode => tokens.push((b"target", self.target.unwrap().to_vec())),
            Method::GetPeers => tokens.push((b"info_hash", self.info_hash.unwrap().to_vec())),
            Method::AnnouncePeer => {
                tokens.push((b"implied_port", [self.implied_port.unwrap() as u8].to_vec()));
                tokens.push((b"info_hash", self.info_hash.unwrap().to_vec()));
                tokens.push((b"port", self.port.unwrap().to_be_bytes().to_vec()));
                tokens.push((b"token", self.token.clone().unwrap().into()));
            }
        }

        encoder.emit_unsorted_dict(|e| {
            for (k, v) in tokens {
                match k {
                    b"values" => {
                        let mut list = Encoder::new();

                        list.emit_list(|l| {
                            for value in v.chunks_exact(26) {
                                l.emit_bytes(value)?;
                            }

                            Ok(())
                        })?;

                        e.emit_pair(k, list.get_output()?)?;
                    }
                    b"port" => {
                        e.emit_pair(k, u16::from_be_bytes(v.try_into().unwrap()))?;
                    }
                    b"implied_port" => {
                        e.emit_pair(k, u8::from_be_bytes(v.try_into().unwrap()))?;
                    }
                    _ => {
                        let v = AsString(v);
                        e.emit_pair(k, v)?;
                    }
                }
            }

            Ok(())
        })
    }
}

impl ToBencode for Values {
    const MAX_DEPTH: usize = 5;

    fn encode(&self, encoder: SingleItemEncoder) -> Result<(), encoding::Error> {
        let mut tokens: Vec<(&[u8], Vec<u8>)> = Vec::new();

        tokens.push((b"id", self.id.to_vec()));

        if let Some(nodes) = &self.nodes {
            let v: Vec<_> = nodes
                .iter()
                .flat_map(|n| <Vec<u8>>::try_from(n).unwrap())
                .collect();

            tokens.push((b"nodes", v));
        }

        if let Some(values) = &self.values {
            let v = values
                .iter()
                .flat_map(|v| <Vec<u8>>::try_from(v).unwrap())
                .collect();
            tokens.push((b"values", v));
        }

        if let Some(token) = &self.token {
            tokens.push((b"token", token.as_bytes().to_vec()));
        }

        encoder.emit_unsorted_dict(|e| {
            for (k, v) in tokens {
                match k {
                    b"values" => {
                        e.emit_pair(k, v.chunks_exact(26).map(AsString).collect::<Vec<_>>())?;
                    }
                    _ => {
                        let v = AsString(v);
                        e.emit_pair(k, v)?;
                    }
                }
            }

            Ok(())
        })
    }
}

impl ToBencode for Error {
    const MAX_DEPTH: usize = 5;

    fn encode(&self, encoder: SingleItemEncoder) -> Result<(), encoding::Error> {
        encoder.emit_list(|e| {
            e.emit_int(self.kind.clone() as u8)?;
            e.emit_str(&self.description)
        })
    }
}

#[cfg(test)]
mod tests {
    fn compact_node_gen() -> String {
        let id = rand::thread_rng()
            .sample_iter(Alphanumeric)
            .take(20)
            .map(char::from)
            .collect::<String>();
        let socket = rand::thread_rng().gen::<[u8; 6]>();

        id.chars()
            .chain(
                socket
                    .into_iter()
                    .map(|i| i.to_string().chars().next().unwrap()),
            )
            .collect::<String>()
    }
    use rand::{distributions::Alphanumeric, Rng};

    use super::*;

    #[test]
    fn test_query_ping() {
        let v = b"d1:ad2:id20:abcdefghij0123456789e1:q4:ping1:t2:aa1:y1:qe";
        let encoded = ExtMessage::from_bencode(v).unwrap();

        let inner = Message::Query(Arguments {
            method: Method::Ping,
            id: "abcdefghij0123456789".as_bytes().try_into().unwrap(),
            target: None,
            info_hash: None,
            port: None,
            implied_port: None,
            token: None,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(encoded, real);
        assert_eq!(v.as_slice(), decoded);
    }

    #[test]
    fn test_query_find_node() {
        let v = b"d1:ad2:id20:abcdefghij01234567896:target20:mnopqrstuvwxyz123456e1:q9:find_node1:t2:aa1:y1:qe";
        let bencoded = ExtMessage::from_bencode(v).unwrap();

        let inner = Message::Query(Arguments {
            method: Method::FindNode,
            id: "abcdefghij0123456789".as_bytes().try_into().unwrap(),
            target: Some("mnopqrstuvwxyz123456".as_bytes().try_into().unwrap()),
            info_hash: None,
            port: None,
            implied_port: None,
            token: None,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_slice(), decoded);
    }

    #[test]
    fn test_query_get_peers() {
        let v = b"d1:ad2:id20:abcdefghij01234567899:info_hash20:mnopqrstuvwxyz123456e1:q9:get_peers1:t2:aa1:y1:qe";
        let bencoded = ExtMessage::from_bencode(v).unwrap();

        let inner = Message::Query(Arguments {
            method: Method::GetPeers,
            id: "abcdefghij0123456789".as_bytes().try_into().unwrap(),
            target: None,
            info_hash: Some("mnopqrstuvwxyz123456".as_bytes().try_into().unwrap()),
            port: None,
            implied_port: None,
            token: None,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_slice(), decoded);
    }

    #[test]
    fn test_query_announce_peer() {
        let v = b"d1:ad2:id20:abcdefghij012345678912:implied_porti1e9:info_hash20:mnopqrstuvwxyz1234564:porti6881e5:token8:aoeusnthe1:q13:announce_peer1:t2:aa1:y1:qe";

        let bencoded = ExtMessage::from_bencode(v).unwrap();

        let inner = Message::Query(Arguments {
            method: Method::AnnouncePeer,
            id: "abcdefghij0123456789".as_bytes().try_into().unwrap(),
            target: None,
            info_hash: Some("mnopqrstuvwxyz123456".as_bytes().try_into().unwrap()),
            port: Some(6881),
            implied_port: Some(true),
            token: Some("aoeusnth".to_owned()),
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_slice(), decoded);
    }

    #[test]
    fn test_response_ping() {
        let v = b"d1:rd2:id20:mnopqrstuvwxyz123456e1:t2:aa1:y1:re";

        let bencoded = ExtMessage::from_bencode(v).unwrap();

        let inner = Message::Response(Values {
            id: "mnopqrstuvwxyz123456".as_bytes().try_into().unwrap(),
            nodes: None,
            values: None,
            token: None,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_slice(), decoded);
    }

    #[test]
    fn test_response_find_node_target_node() {
        let node = compact_node_gen();

        let v = format!(
            "d1:rd2:id20:0123456789abcdefghij5:nodes{}:{}e1:t2:aa1:y1:re",
            node.len(),
            node
        );

        let bencoded = ExtMessage::from_bencode(v.as_bytes()).unwrap();

        let node = CompactNode::try_from(node.as_bytes().to_vec()).unwrap();

        let inner = Message::Response(Values {
            id: "0123456789abcdefghij".as_bytes().try_into().unwrap(),
            nodes: Some(vec![node]),
            values: None,
            token: None,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_bytes(), decoded);
    }

    #[test]
    fn test_response_find_node_closest_nodes() {
        let nodes = (0..8).map(|_| compact_node_gen()).collect::<String>();

        let v = format!(
            "d1:rd2:id20:0123456789abcdefghij5:nodes{}:{}e1:t2:aa1:y1:re",
            nodes.len(),
            nodes
        );

        let bencoded = ExtMessage::from_bencode(v.as_bytes()).unwrap();

        let nodes: Vec<_> = nodes
            .as_bytes()
            .chunks_exact(26)
            .map(|value| CompactNode::try_from(value.to_vec()).unwrap())
            .collect();

        let inner = Message::Response(Values {
            id: "0123456789abcdefghij".as_bytes().try_into().unwrap(),
            nodes: Some(nodes),
            values: None,
            token: None,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_bytes(), decoded);
    }

    #[test]
    fn test_response_get_peers() {
        let values = (0..2).map(|_| compact_node_gen()).collect::<Vec<_>>();

        let v = format!(
            "d1:rd2:id20:abcdefghij01234567895:token8:aoeusnth6:valuesl26:{}26:{}ee1:t2:aa1:y1:re",
            values[0], values[1],
        );

        let bencoded = ExtMessage::from_bencode(v.as_bytes()).unwrap();

        let values: Vec<_> = values
            .iter()
            .map(|v| CompactNode::try_from(v.as_bytes().to_vec()).unwrap())
            .collect();

        let inner = Message::Response(Values {
            id: "abcdefghij0123456789".as_bytes().try_into().unwrap(),
            nodes: None,
            values: Some(values),
            token: Some("aoeusnth".to_owned()),
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_bytes(), decoded);
    }

    #[test]
    fn test_response_get_peers_with_closest_nodes() {
        let nodes = (0..8).map(|_| compact_node_gen()).collect::<String>();

        let v = format!(
            "d1:rd2:id20:abcdefghij01234567895:nodes{}:{}5:token8:aoeusnthe1:t2:aa1:y1:re",
            nodes.len(),
            nodes
        );

        let bencoded = ExtMessage::from_bencode(v.as_bytes()).unwrap();

        let nodes: Vec<_> = nodes
            .as_bytes()
            .chunks_exact(26)
            .map(|value| CompactNode::try_from(value.to_vec()).unwrap())
            .collect();

        let inner = Message::Response(Values {
            id: "abcdefghij0123456789".as_bytes().try_into().unwrap(),
            nodes: Some(nodes),
            values: None,
            token: Some("aoeusnth".to_owned()),
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_bytes(), decoded);
    }

    #[test]
    fn test_response_announce_peer() {
        let v = b"d1:rd2:id20:mnopqrstuvwxyz123456e1:t2:aa1:y1:re";

        let bencoded = ExtMessage::from_bencode(v).unwrap();

        let inner = Message::Response(Values {
            id: "mnopqrstuvwxyz123456".as_bytes().try_into().unwrap(),
            nodes: None,
            values: None,
            token: None,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_slice(), decoded);
    }

    #[test]
    fn test_error() {
        let v = b"d1:eli201e23:A Generic Error Ocurrede1:t2:aa1:y1:ee";
        let bencoded = ExtMessage::from_bencode(v).unwrap();

        let inner = Message::Err(Error {
            description: "A Generic Error Ocurred".to_owned(),
            kind: ErrorKind::Generic,
        });

        let real = ExtMessage {
            inner,
            transaction_id: "aa".to_owned(),
        };

        let decoded = real.to_bencode().unwrap();

        assert_eq!(bencoded, real);
        assert_eq!(v.as_slice(), decoded);
    }
}
