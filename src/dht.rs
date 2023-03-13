use chrono::{DateTime, Utc};

pub type RoutingTable = Vec<Node>;

pub struct Node {
    pub id: [u8; 20],
    pub routing_table: RoutingTable,
}

pub struct Bucket {
    last_changed: DateTime<Utc>,
}

impl Node {
    fn compare(&self, hash: [u8; 20]) -> [u8; 20] {
        todo!()
    }
}
