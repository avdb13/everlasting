pub struct State<T> {
    pub items: Vec<T>,
    pub idx: Option<usize>,
}

impl<T> State<T> {
    pub fn new(items: Vec<T>) -> State<T> {
        State { items, idx: None }
    }

    pub fn next(&mut self) {
        self.idx = match self.idx {
            Some(i) => Some((i + 1) % self.items.len()),
            None => Some(0),
        }
    }
    pub fn prev(&mut self) {
        self.idx = match self.idx {
            Some(i) => Some((i - 1) % self.items.len()),
            Some(0) => Some(self.items.len() - 1),
            None => Some(0),
        }
    }

    // pub fn focus(&mut self)
}
