use std::cmp::Ordering;

/// A message that can be sent between agents.
pub struct Message<T: Send + Sync + Clone> {
    pub data: T,
    pub timestamp: f64,
    pub from: usize,
    pub to: usize,
}

impl<T: Send + Sync + Clone> Message<T> {
    pub fn new(data: T, timestamp: f64, from: usize, to: usize) -> Self {
        Message {
            data,
            timestamp,
            from,
            to,
        }
    }
}

impl<T: Send + Sync + Clone> PartialEq for Message<T> {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}

impl<T: Send + Sync + Clone> Eq for Message<T> {}

impl<T: Send + Sync + Clone> PartialOrd for Message<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Send + Sync + Clone> Ord for Message<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.partial_cmp(&other.timestamp).unwrap()
    }
}
