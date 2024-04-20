use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// A channel for updating an object.
/// Unlike a mpsc, there is no queue of objects, only the most recent can be obtained.
/// Unlike a watch, the receiver owns the object received.
/// Any copy of the owner (created via clone) can send or receive objects,
/// but only one copy will receive any particular object.
#[derive(Default)]
pub struct Updater<T: Send + Sync>(Arc<(Mutex<Option<T>>, Notify)>);

impl<T: Send + Sync> Updater<T> {
    /// Send an object T to the receiver end, repacing any currently queued object.
    pub fn send(&self, value: T) {
        let mut locked_object = self.0 .0.lock().expect("send failed to lock mutex");
        *locked_object = Some(value);
        self.0 .1.notify_one();
    }

    /// Get the object most recently sent by the sender end.
    pub async fn recv(&mut self) -> T {
        // According to a dev on GH, tokio's Notify is allowed false notifications.
        // This is conceptually better suited for a condvar, but the only async
        // implementations aren't cancellation safe.
        // Precondition: the only way for the object to be updated is to notify,
        // and no receiver consumes a notify without consuming the object as well.
        loop {
            self.0 .1.notified().await;
            {
                let mut locked_object = self.0 .0.lock().unwrap();
                if locked_object.is_some() {
                    return locked_object.take().unwrap();
                }
            }
        }
    }

    /// Get the object most recently sent by the sender end, if one is already available.
    pub fn maybe_recv(&mut self) -> Option<T> {
        let mut locked_object = self.0 .0.lock().unwrap();
        locked_object.take()
    }

    pub fn new() -> Self {
        Updater(Arc::new((Mutex::new(None), Notify::new())))
    }
}

impl<T: Send + Sync> Clone for Updater<T> {
    fn clone(&self) -> Self {
        Updater(Arc::clone(&self.0))
    }
}
