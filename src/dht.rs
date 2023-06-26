use chrono::Utc;

const CAPACITY: usize = 8;

pub struct Table {
    id: Node,
    inner: Vec<Bucket>,
}
pub struct Bucket {
    nodes: [Node; 8],
    range: (usize, usize),
    last_changed: chrono::DateTime<chrono::Utc>,
}

#[derive(Default)]
pub struct Node(pub [u8; 20]);

impl Node {
    fn distance(&self, other: Node) -> u64 {
        self.0
            .into_iter()
            .zip(other.0)
            .map(|(x, y)| (x ^ y) as u64)
            .sum()
    }
}

// When a bucket is full of known good nodes,
// no more nodes may be added unless our own node ID falls within the range of the bucket.
// In that case, the bucket is replaced by two new buckets each with half the range of
// the old bucket and the nodes from the old bucket are distributed among the two new ones.
// For a new table with only one bucket,
// the full bucket is always split into two new buckets covering
// the ranges 0..2159 and 2159..2160.
//
impl Table {
    fn new(id: Node) -> Self {
        let bucket = Bucket {
            nodes: Default::default(),
            range: (0, 160),
            last_changed: Utc::now(),
        };

        Table {
            id,
            inner: vec![bucket],
        }
    }

    fn split(&mut self) {
        let left = Bucket {
            nodes: Default::default(),
            range: (0, 159),
            last_changed: Utc::now(),
        };
        let right = Bucket {
            nodes: Default::default(),
            range: (159, 160),
            last_changed: Utc::now(),
        };

        if self.inner.len() == 1 {
            self.inner = vec![left, right];
        }
    }

    fn add(&mut self, node: Node, i: usize) {
        let v = &self.inner[i];

        todo!()
    }
}
