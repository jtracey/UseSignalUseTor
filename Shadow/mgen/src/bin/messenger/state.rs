// The state machine used to represent one end of a conversation.
// This includes inducing transitions and actions taken during transitions,
// so messages are constructed and passed to other threads from here.

use mgen::{log, MessageHeader, SerializedMessage};
use rand_distr::Distribution;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::messenger::dists::Distributions;
use crate::messenger::message::{construct_message, construct_receipt};

/// All possible Conversation state machine states
pub enum StateMachine {
    Idle(Conversation<Idle>),
    Active(Conversation<Active>),
}

impl StateMachine {
    pub fn start(dists: Distributions, rng: &mut Xoshiro256PlusPlus) -> StateMachine {
        Self::Idle(Conversation::<Idle>::start(dists, rng))
    }

    fn name(&self) -> &str {
        match self {
            Self::Idle(_) => Idle::NAME,
            Self::Active(_) => Active::NAME,
        }
    }
}

/// The state machine representing a conversation state and its transitions.
pub struct Conversation<S: State> {
    dists: Distributions,
    delay: Instant,
    next_id: u32,
    state: S,
}

pub trait State {
    const NAME: &'static str;
    fn sent(conversation: Conversation<Self>, rng: &mut Xoshiro256PlusPlus) -> StateMachine
    where
        Self: Sized;
    fn received(conversation: Conversation<Self>, rng: &mut Xoshiro256PlusPlus) -> StateMachine
    where
        Self: Sized;
    fn to_machine(conversation: Conversation<Self>) -> StateMachine
    where
        Self: Sized;
}

pub struct Idle {}
pub struct Active {
    wait: Instant,
}

impl State for Idle {
    const NAME: &'static str = "Idle";
    fn sent(conversation: Conversation<Self>, rng: &mut Xoshiro256PlusPlus) -> StateMachine {
        let next_id = conversation.next_id + 1;
        if conversation.dists.s.sample(rng) {
            let delay = Instant::now() + conversation.dists.a_s.sample_secs(rng);
            let wait = Instant::now() + conversation.dists.w.sample_secs(rng);
            StateMachine::Active({
                Conversation::<Active> {
                    dists: conversation.dists,
                    delay,
                    next_id,
                    state: Active { wait },
                }
            })
        } else {
            let delay = Instant::now() + conversation.dists.i.sample_secs(rng);
            StateMachine::Idle({
                Conversation::<Idle> {
                    dists: conversation.dists,
                    delay,
                    next_id,
                    state: Idle {},
                }
            })
        }
    }

    fn received(conversation: Conversation<Self>, rng: &mut Xoshiro256PlusPlus) -> StateMachine {
        if conversation.dists.r.sample(rng) {
            let wait = Instant::now() + conversation.dists.w.sample_secs(rng);
            let delay = Instant::now() + conversation.dists.a_r.sample_secs(rng);
            StateMachine::Active({
                Conversation::<Active> {
                    dists: conversation.dists,
                    delay,
                    next_id: conversation.next_id,
                    state: Active { wait },
                }
            })
        } else {
            StateMachine::Idle(conversation)
        }
    }

    fn to_machine(conversation: Conversation<Self>) -> StateMachine {
        StateMachine::Idle(conversation)
    }
}

impl State for Active {
    const NAME: &'static str = "Active";
    fn sent(conversation: Conversation<Self>, rng: &mut Xoshiro256PlusPlus) -> StateMachine {
        let delay = Instant::now() + conversation.dists.a_s.sample_secs(rng);
        StateMachine::Active(Conversation::<Active> {
            dists: conversation.dists,
            delay,
            next_id: conversation.next_id + 1,
            state: conversation.state,
        })
    }

    fn received(conversation: Conversation<Self>, rng: &mut Xoshiro256PlusPlus) -> StateMachine {
        let delay = Instant::now() + conversation.dists.a_r.sample_secs(rng);
        StateMachine::Active(Conversation::<Active> {
            dists: conversation.dists,
            delay,
            next_id: conversation.next_id,
            state: conversation.state,
        })
    }

    fn to_machine(conversation: Conversation<Self>) -> StateMachine {
        StateMachine::Active(conversation)
    }
}

impl Conversation<Idle> {
    fn start(dists: Distributions, rng: &mut Xoshiro256PlusPlus) -> Self {
        let delay = Instant::now() + dists.i.sample_secs(rng);
        Self {
            dists,
            delay,
            next_id: 0,
            state: Idle {},
        }
    }
}

impl Conversation<Active> {
    fn waited(self, rng: &mut Xoshiro256PlusPlus) -> Conversation<Idle> {
        let delay = Instant::now() + self.dists.i.sample_secs(rng);
        Conversation::<Idle> {
            dists: self.dists,
            delay,
            next_id: self.next_id,
            state: Idle {},
        }
    }

    async fn sleep(delay: Instant, wait: Instant) -> ActiveGroupActions {
        if delay < wait {
            tokio::time::sleep_until(delay).await;
            ActiveGroupActions::Send
        } else {
            tokio::time::sleep_until(wait).await;
            ActiveGroupActions::Idle
        }
    }
}

/// Type for getting messages from the reader thread in the state thread.
pub type StateFromReader = mpsc::UnboundedReceiver<MessageHeader>;
/// Type for sending messages from the state thread to the writer thread.
pub struct StateToWriter<S: MessageHolder> {
    pub channel: mpsc::UnboundedSender<S>,
}

pub trait MessageHolder: Borrow<SerializedMessage> + Debug {
    fn new(m: SerializedMessage) -> Self;
    fn clone(&self) -> Self;
}

impl MessageHolder for Arc<SerializedMessage> {
    fn new(m: SerializedMessage) -> Self {
        Self::new(m)
    }

    fn clone(&self) -> Self {
        Clone::clone(self)
    }
}

impl MessageHolder for Box<SerializedMessage> {
    fn new(m: SerializedMessage) -> Self {
        Self::new(m)
    }

    fn clone(&self) -> Self {
        panic!("Box holders should never clone");
    }
}

pub trait StreamMap<'a, S: 'a + MessageHolder, I: Iterator<Item = &'a mut StateToWriter<S>>> {
    fn channel_for(&self, name: &str) -> &StateToWriter<S>;
    fn values(&'a mut self) -> I;
}

impl<'a, S: MessageHolder>
    StreamMap<'a, S, std::collections::hash_map::ValuesMut<'a, String, StateToWriter<S>>>
    for HashMap<String, StateToWriter<S>>
{
    fn channel_for(&self, name: &str) -> &StateToWriter<S> {
        &self[name]
    }

    fn values(&'a mut self) -> std::collections::hash_map::ValuesMut<'a, String, StateToWriter<S>> {
        self.values_mut()
    }
}

impl<'a, S: MessageHolder> StreamMap<'a, S, std::iter::Once<&'a mut StateToWriter<S>>>
    for StateToWriter<S>
{
    fn channel_for(&self, _name: &str) -> &StateToWriter<S> {
        self
    }

    fn values(&'a mut self) -> std::iter::Once<&'a mut StateToWriter<S>> {
        std::iter::once(self)
    }
}

async fn send_action<
    'a,
    S: 'a + MessageHolder,
    T: State,
    I: ExactSizeIterator<Item = &'a mut StateToWriter<S>>,
>(
    conversation: Conversation<T>,
    mut streams: I,
    our_id: &str,
    group: &str,
    rng: &mut Xoshiro256PlusPlus,
) -> StateMachine {
    let size = conversation.dists.m.sample(rng);
    let id = conversation.next_id;
    let m = S::new(construct_message(
        our_id.to_string(),
        group.to_string(),
        id,
        size,
    ));

    if streams.len() == 1 {
        streams
            .next()
            .unwrap()
            .channel
            .send(m)
            .expect("Internal stream closed with messages still being sent");
    } else {
        for stream in streams {
            stream
                .channel
                .send(m.clone())
                .expect("Internal stream closed with messages still being sent");
        }
    }

    let ret = T::sent(conversation, rng);

    log!(
        "{},{},send,{},{},{},{}",
        our_id,
        group,
        T::NAME,
        ret.name(),
        size,
        id
    );

    ret
}

async fn receive_action<
    'a,
    S: 'a + MessageHolder,
    T: State,
    I: std::iter::Iterator<Item = &'a mut StateToWriter<S>>,
    M: StreamMap<'a, S, I>,
>(
    msg: MessageHeader,
    conversation: Conversation<T>,
    stream_map: &mut M,
    our_id: &str,
    group: Option<&str>,
    rng: &mut Xoshiro256PlusPlus,
) -> StateMachine {
    match msg.body {
        mgen::MessageBody::Size(size) => {
            let ret = T::received(conversation, rng);
            log!(
                "{},{},receive,{},{},{},{},{}",
                our_id,
                msg.group,
                T::NAME,
                ret.name(),
                msg.sender,
                size,
                msg.id
            );
            let stream = stream_map.channel_for(&msg.sender);
            let recipient = if group.is_none() {
                msg.group
            } else {
                msg.sender
            };
            let m = construct_receipt(our_id.to_string(), recipient, msg.id);
            stream
                .channel
                .send(S::new(m))
                .expect("channel from receive_action to sender closed");
            ret
        }
        mgen::MessageBody::Receipt => {
            let group = match group {
                Some(group) => group,
                None => &msg.group,
            };
            log!(
                "{},{},receive,{},{},{},receipt,{}",
                our_id,
                group,
                T::NAME,
                T::NAME,
                msg.sender,
                msg.id
            );
            T::to_machine(conversation)
        }
    }
}

enum IdleGroupActions {
    Send,
    Receive(MessageHeader),
}

/// Handle a state transition from Idle, including I/O, for a multi-connection conversation.
/// Used for Idle group p2p conversations.
pub async fn manage_idle_conversation<
    'a,
    const P2P: bool,
    S: 'a + MessageHolder,
    I: std::iter::ExactSizeIterator<Item = &'a mut StateToWriter<S>>,
    M: StreamMap<'a, S, I> + 'a,
>(
    conversation: Conversation<Idle>,
    inbound: &mut StateFromReader,
    stream_map: &'a mut M,
    our_id: &str,
    group: &str,
    rng: &mut Xoshiro256PlusPlus,
) -> StateMachine {
    log!("{},{},Idle", our_id, group);
    let action = tokio::select! {
        () = tokio::time::sleep_until(conversation.delay) => IdleGroupActions::Send,

        res = inbound.recv() =>
            IdleGroupActions::Receive(res.expect("inbound channel closed")),
    };

    match action {
        IdleGroupActions::Send => {
            send_action(conversation, stream_map.values(), our_id, group, rng).await
        }
        IdleGroupActions::Receive(msg) => {
            let group = if P2P { None } else { Some(group) };
            receive_action(msg, conversation, stream_map, our_id, group, rng).await
        }
    }
}

enum ActiveGroupActions {
    Send,
    Receive(MessageHeader),
    Idle,
}

/// Handle a state transition from Active.
pub async fn manage_active_conversation<
    'a,
    S: 'a + MessageHolder,
    I: std::iter::ExactSizeIterator<Item = &'a mut StateToWriter<S>>,
    M: StreamMap<'a, S, I> + 'a,
>(
    conversation: Conversation<Active>,
    inbound: &mut StateFromReader,
    stream_map: &'a mut M,
    our_id: &str,
    group: &str,
    p2p: bool,
    rng: &mut Xoshiro256PlusPlus,
) -> StateMachine {
    let action = tokio::select! {
        action = Conversation::<Active>::sleep(conversation.delay, conversation.state.wait) => action,

        res = inbound.recv() =>
            ActiveGroupActions::Receive(res.expect("inbound channel closed")),
    };

    match action {
        ActiveGroupActions::Send => {
            send_action(conversation, stream_map.values(), our_id, group, rng).await
        }
        ActiveGroupActions::Receive(msg) => {
            let group = if p2p { None } else { Some(group) };
            receive_action(msg, conversation, stream_map, our_id, group, rng).await
        }
        ActiveGroupActions::Idle => StateMachine::Idle(conversation.waited(rng)),
    }
}
