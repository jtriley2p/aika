use super::Message;
use tokio::sync::{
    mpsc::{channel, error, Receiver, Sender},
    watch,
};

/// A mailbox for agents to send and receive messages. WIP
pub struct Mailbox<T: Send + Sync + Clone> {
    tx: Sender<Message<T>>,
    rx: Receiver<Message<T>>,
    mailbox: Vec<Message<T>>,
    pause_rx: watch::Receiver<bool>,
}

impl<T: Send + Sync + Clone> Mailbox<T> {
    pub fn new(buffer_size: usize, pause_rx: watch::Receiver<bool>) -> Self {
        let (tx, rx) = channel(buffer_size);
        Mailbox {
            tx,
            rx,
            mailbox: Vec::new(),
            pause_rx,
        }
    }

    pub fn is_paused(&self) -> bool {
        *self.pause_rx.borrow()
    }

    pub async fn wait_for_resume(&mut self) {
        while self.is_paused() {
            self.pause_rx.changed().await.unwrap();
        }
    }

    pub async fn send(&self, msg: Message<T>) -> Result<(), error::SendError<Message<T>>> {
        self.tx.send(msg).await
    }

    pub async fn receive(&mut self) -> Option<Message<T>> {
        self.rx.recv().await
    }

    pub fn peek_messages(&self) -> &[Message<T>] {
        &self.mailbox
    }

    pub async fn collect_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.mailbox.push(msg);
        }
    }
}
