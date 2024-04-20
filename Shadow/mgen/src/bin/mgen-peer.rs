// Code specific to the peer in the p2p mode.

use mgen::{log, updater::Updater, MessageHeader, SerializedMessage};

use futures::future::try_join_all;
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256PlusPlus};
use serde::Deserialize;
use std::collections::HashMap;
use std::result::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpListener,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Duration;

mod messenger;

use crate::messenger::dists::{ConfigDistributions, Distributions};
use crate::messenger::error::{FatalError, MessengerError};
use crate::messenger::state::{
    manage_active_conversation, manage_idle_conversation, StateFromReader, StateMachine,
    StateToWriter,
};
use crate::messenger::tcp::{connect, SocksParams};

/// Type for sending messages from the reader thread to the state thread.
type ReaderToState = mpsc::UnboundedSender<MessageHeader>;
/// Type for getting messages from the state thread in the writer thread.
type WriterFromState = mpsc::UnboundedReceiver<Arc<SerializedMessage>>;
/// Type for sending messages from the state thread to the writer thread.
type MessageHolder = Arc<SerializedMessage>;
/// Type for sending the updated read half of the socket.
type ReadSocketUpdaterIn = Updater<OwnedReadHalf>;
/// Type for getting the updated read half of the socket.
type ReadSocketUpdaterOut = Updater<OwnedReadHalf>;
/// Type for sending the updated write half of the socket.
type WriteSocketUpdaterIn = Updater<OwnedWriteHalf>;
/// Type for getting the updated write half of the socket.
type WriteSocketUpdaterOut = Updater<OwnedWriteHalf>;

/// The conversation (state) thread tracks the conversation state
/// (i.e., whether the user is active or idle, and when to send messages).
/// One state thread per conversation.
async fn manage_conversation(
    user: String,
    group: String,
    distributions: Distributions,
    bootstrap: f64,
    mut state_from_reader: StateFromReader,
    mut state_to_writers: HashMap<String, StateToWriter<MessageHolder>>,
) {
    let mut rng = Xoshiro256PlusPlus::from_entropy();
    let user = &user;
    let group = &group;

    let mut state_machine = StateMachine::start(distributions, &mut rng);

    tokio::time::sleep(Duration::from_secs_f64(bootstrap)).await;

    loop {
        state_machine = match state_machine {
            StateMachine::Idle(conversation) => {
                manage_idle_conversation::<true, _, _, _>(
                    conversation,
                    &mut state_from_reader,
                    &mut state_to_writers,
                    user,
                    group,
                    &mut rng,
                )
                .await
            }
            StateMachine::Active(conversation) => {
                manage_active_conversation(
                    conversation,
                    &mut state_from_reader,
                    &mut state_to_writers,
                    user,
                    group,
                    true,
                    &mut rng,
                )
                .await
            }
        };
    }
}

/// The listener thread listens for inbound connections on the given address.
/// It breaks those connections into reader and writer halves,
/// and gives them to the correct reader and writer threads.
/// One listener thread per user.
async fn listener(
    address: String,
    name_to_io_threads: HashMap<String, (ReadSocketUpdaterIn, WriteSocketUpdaterIn)>,
) -> Result<(), FatalError> {
    let listener = TcpListener::bind(&address).await?;
    log!("listening on {}", &address);

    async fn error_collector(
        address: &str,
        listener: &TcpListener,
        name_to_io_threads: &HashMap<String, (ReadSocketUpdaterIn, WriteSocketUpdaterIn)>,
    ) -> Result<(), MessengerError> {
        let (stream, _) = listener.accept().await?;
        let (mut rd, wr) = stream.into_split();

        let from = mgen::parse_identifier(&mut rd).await?;

        let (channel_to_reader, channel_to_writer) = name_to_io_threads
            .get(&from)
            .unwrap_or_else(|| panic!("{} got connection from unknown contact: {}", address, from));
        channel_to_reader.send(rd);
        channel_to_writer.send(wr);
        Ok(())
    }

    loop {
        if let Err(MessengerError::Fatal(e)) =
            error_collector(&address, &listener, &name_to_io_threads).await
        {
            return Err(e);
        }
    }
}

/// The reader thread reads messages from the socket it has been given,
/// and sends them to the correct state thread.
/// One reader thread per (user, recipient) pair.
async fn reader(
    mut connection_channel: ReadSocketUpdaterOut,
    group_to_conversation_thread: HashMap<String, ReaderToState>,
) {
    loop {
        // wait for listener or writer thread to give us a stream to read from
        let mut stream = connection_channel.recv().await;
        loop {
            let Ok(msg) = mgen::get_message::<true, _>(&mut stream).await else {
                // Unlike the client-server case, we can assume that if there
                // were a message someone was trying to send us, they'd make
                // sure to re-establish the connection; so when the socket
                // breaks, don't bother trying to reform it until we need to
                // send a message or the peer reaches out to us.
                break;
            };
            let channel_to_conversation = group_to_conversation_thread
                .get(&msg.group)
                .unwrap_or_else(|| panic!("Unknown group: {}", msg.group));
            channel_to_conversation
                .send(msg)
                .expect("reader: Channel to group closed");
        }
    }
}

/// The writer thread takes in messages from state threads,
/// and sends it to the recipient associated with this thread.
/// If it doesn't have a socket from the listener thread,
/// it'll create its own and give the read half to the reader thread.
/// One writer thread per (user, recipient) pair.
async fn writer<'a>(
    mut messages_to_send: WriterFromState,
    mut write_socket_updater: WriteSocketUpdaterOut,
    read_socket_updater: ReadSocketUpdaterIn,
    socks_params: SocksParams,
    retry: Duration,
) -> Result<(), FatalError> {
    // make sure this is the first step to avoid connections until there's
    // something to send
    let mut msg = messages_to_send
        .recv()
        .await
        .expect("writer: Channel from conversations closed");

    let mut stream = establish_connection(
        &mut write_socket_updater,
        &read_socket_updater,
        &socks_params,
        retry,
    )
    .await
    .expect("Fatal error establishing connection");

    loop {
        while msg.write_all_to::<true, _>(&mut stream).await.is_err() {
            stream = establish_connection(
                &mut write_socket_updater,
                &read_socket_updater,
                &socks_params,
                retry,
            )
            .await
            .expect("Fatal error establishing connection");
        }

        msg = messages_to_send
            .recv()
            .await
            .expect("writer: Channel from conversations closed");
    }

    // helper functions

    /// Attempt to get a connection to the peer,
    /// whether by getting an existing connection from the listener,
    /// or by establishing a new connection.
    async fn establish_connection<'a>(
        write_socket_updater: &mut WriteSocketUpdaterOut,
        read_socket_updater: &ReadSocketUpdaterIn,
        socks_params: &SocksParams,
        retry: Duration,
    ) -> Result<OwnedWriteHalf, FatalError> {
        // first check if the listener thread already has a socket
        if let Some(wr) = write_socket_updater.maybe_recv() {
            return Ok(wr);
        }

        // immediately try to connect to the peer
        tokio::select! {
            connection_attempt = connect(socks_params) => {
                if let Ok(mut stream) = connection_attempt {
                    log!(
                        "connection attempt success from {} to {} on {}",
                        &socks_params.user,
                        &socks_params.recipient,
                        &socks_params.target
                    );
                    stream
                        .write_all(&mgen::serialize_str(&socks_params.user))
                        .await?;
                    let (rd, wr) = stream.into_split();
                    read_socket_updater.send(rd);
                    return Ok(wr);
                } else if let Err(MessengerError::Fatal(e)) = connection_attempt {
                        return Err(e);
                }
            }
            stream = write_socket_updater.recv() => {return Ok(stream);},
        }

        // Usually we'll have returned by now, but sometimes we'll fail to
        // connect for whatever reason. Initiate a loop of waiting Duration,
        // then trying to connect again, allowing it to be inerrupted by
        // the listener thread.

        loop {
            match error_collector(
                write_socket_updater,
                read_socket_updater,
                socks_params,
                retry,
            )
            .await
            {
                Ok(wr) => return Ok(wr),
                Err(MessengerError::Recoverable(_)) => continue,
                Err(MessengerError::Fatal(e)) => return Err(e),
            }
        }

        async fn error_collector<'a>(
            write_socket_updater: &mut WriteSocketUpdaterOut,
            read_socket_updater: &ReadSocketUpdaterIn,
            socks_params: &SocksParams,
            retry: Duration,
        ) -> Result<OwnedWriteHalf, MessengerError> {
            tokio::select! {
                () = tokio::time::sleep(retry) => {
                    let mut stream = connect(socks_params)
                        .await?;
                    stream.write_all(&mgen::serialize_str(&socks_params.user)).await?;

                    let (rd, wr) = stream.into_split();
                    read_socket_updater.send(rd);
                    Ok(wr)
                },
                stream = write_socket_updater.recv() => Ok(stream),
            }
        }
    }
}

fn parse_hosts_file(file_contents: &str) -> HashMap<&str, &str> {
    let mut ret = HashMap::new();
    for line in file_contents.lines() {
        let mut words = line.split_ascii_whitespace();
        if let Some(addr) = words.next() {
            for name in words {
                ret.insert(name, addr);
            }
        }
    }
    ret
}

#[derive(Debug, Deserialize)]
struct ConversationConfig {
    group: String,
    recipients: Vec<String>,
    bootstrap: Option<f64>,
    retry: Option<f64>,
    distributions: Option<ConfigDistributions>,
}

#[derive(Debug, Deserialize)]
struct Config {
    user: String,
    socks: Option<String>,
    listen: Option<String>,
    bootstrap: f64,
    retry: f64,
    distributions: ConfigDistributions,
    conversations: Vec<ConversationConfig>,
}

fn process_config(
    config: Config,
    hosts_map: &HashMap<&str, &str>,
    handles: &mut Vec<JoinHandle<Result<(), FatalError>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    struct ForIoThreads {
        state_to_writer: mpsc::UnboundedSender<MessageHolder>,
        writer_from_state: WriterFromState,
        reader_to_states: HashMap<String, ReaderToState>,
        str_params: SocksParams,
        retry: f64,
    }

    let default_dists: Distributions = config.distributions.try_into()?;

    // map from `recipient` to things the (user, recipient) reader/writer threads will need
    let mut recipient_map = HashMap::<String, ForIoThreads>::new();
    for conversation in config.conversations.into_iter() {
        let (reader_to_state, state_from_reader) = mpsc::unbounded_channel();

        let mut conversation_recipient_map =
            HashMap::<String, StateToWriter<MessageHolder>>::with_capacity(
                conversation.recipients.len(),
            );

        for recipient in conversation.recipients.iter() {
            let for_io = recipient_map
                .entry(recipient.to_string())
                .and_modify(|e| {
                    e.reader_to_states
                        .entry(conversation.group.clone())
                        .or_insert_with(|| reader_to_state.clone());
                })
                .or_insert_with(|| {
                    let (state_to_writer, writer_from_state) = mpsc::unbounded_channel();
                    let mut reader_to_states = HashMap::new();
                    reader_to_states.insert(conversation.group.clone(), reader_to_state.clone());
                    let address = hosts_map
                        .get(recipient.as_str())
                        .unwrap_or_else(|| panic!("recipient not in hosts file: {}", recipient));
                    let str_params = SocksParams {
                        socks: config.socks.clone(),
                        target: address.to_string(),
                        user: config.user.clone(),
                        recipient: recipient.clone(),
                    };
                    let retry = conversation.retry.unwrap_or(config.retry);
                    ForIoThreads {
                        state_to_writer,
                        writer_from_state,
                        reader_to_states,
                        str_params,
                        retry,
                    }
                });
            let state_to_writer = for_io.state_to_writer.clone();
            conversation_recipient_map.insert(
                recipient.clone(),
                StateToWriter {
                    channel: state_to_writer,
                },
            );
        }

        let distributions: Distributions = match conversation.distributions {
            Some(dists) => dists.try_into()?,
            None => default_dists.clone(),
        };
        let bootstrap = conversation.bootstrap.unwrap_or(config.bootstrap);

        tokio::spawn(manage_conversation(
            config.user.clone(),
            conversation.group,
            distributions,
            bootstrap,
            state_from_reader,
            conversation_recipient_map,
        ));
    }

    let mut name_to_io_threads: HashMap<String, (ReadSocketUpdaterIn, WriteSocketUpdaterIn)> =
        HashMap::new();

    for (recipient, for_io) in recipient_map.drain() {
        let listener_writer_to_reader = Updater::new();
        let reader_from_listener_writer = listener_writer_to_reader.clone();
        let listener_to_writer = Updater::new();
        let writer_from_listener = listener_to_writer.clone();
        name_to_io_threads.insert(
            recipient.to_string(),
            (listener_writer_to_reader.clone(), listener_to_writer),
        );

        tokio::spawn(reader(reader_from_listener_writer, for_io.reader_to_states));

        let retry = Duration::from_secs_f64(for_io.retry);
        let handle: JoinHandle<Result<(), FatalError>> = tokio::spawn(writer(
            for_io.writer_from_state,
            writer_from_listener,
            listener_writer_to_reader,
            for_io.str_params,
            retry,
        ));
        handles.push(handle);
    }

    let address = if let Some(address) = config.listen {
        address
    } else {
        hosts_map
            .get(config.user.as_str())
            .unwrap_or_else(|| panic!("user not found in hosts file: {}", config.user))
            .to_string()
    };
    let handle: JoinHandle<Result<(), FatalError>> =
        tokio::spawn(listener(address, name_to_io_threads));
    handles.push(handle);
    Ok(())
}

async fn main_worker() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "tracing")]
    console_subscriber::init();

    let mut args = std::env::args();
    let _ = args.next();
    let hosts_file = args.next().expect("missing hosts file arg");
    let hosts_file = std::fs::read_to_string(hosts_file).expect("could not find hosts file");
    let hosts_map = parse_hosts_file(&hosts_file);

    let mut handles = vec![];
    for config_file in args.flat_map(|a| glob::glob(a.as_str()).unwrap()) {
        let yaml_s = std::fs::read_to_string(config_file?)?;
        let config: Config = serde_yaml::from_str(&yaml_s)?;
        process_config(config, &hosts_map, &mut handles)?;
    }

    try_join_all(handles).await?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .disable_lifo_slot()
        .build()
        .unwrap()
        .block_on(main_worker())
}
