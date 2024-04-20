use mgen::{log, updater::Updater, Handshake, MessageBody, MessageHeaderRef, SerializedMessage};
use std::collections::HashMap;
use std::error::Error;
use std::io::BufReader;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{split, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::time::{timeout_at, Instant};
use tokio_rustls::{
    rustls::{KeyLogFile, PrivateKey},
    server::TlsStream,
    TlsAcceptor,
};

// FIXME: identifiers should be interned
type ID = String;

type ReaderToSender = mpsc::UnboundedSender<Arc<SerializedMessage>>;
type WriterDb = HashMap<Handshake, Updater<(WriteHalf<TlsStream<TcpStream>>, Arc<Notify>)>>;
type SndDb = HashMap<ID, Arc<RwLock<HashMap<ID, ReaderToSender>>>>;

#[cfg(feature = "tracing")]
async fn tracing(metrics_monitor: tokio_metrics::TaskMonitor) {
    console_subscriber::init();
    let handle = tokio::runtime::Handle::current();
    let runtime_monitor = tokio_metrics::RuntimeMonitor::new(&handle);

    for intervals in std::iter::zip(metrics_monitor.intervals(), runtime_monitor.intervals()) {
        log!("{:?}", intervals.0);
        log!("{:?}", intervals.1);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(10)
        .enable_all()
        .disable_lifo_slot()
        .build()
        .unwrap()
        .block_on(main_worker())
}

async fn main_worker() -> Result<(), Box<dyn Error>> {
    #[cfg(feature = "tracing")]
    let metrics_monitor = {
        let metrics_monitor = tokio_metrics::TaskMonitor::new();
        tokio::spawn(tracing(metrics_monitor.clone()));
        metrics_monitor
    };

    let mut args = std::env::args();
    let _arg0 = args.next().unwrap();

    let cert_filename = args
        .next()
        .unwrap_or_else(|| panic!("no cert file provided"));
    let key_filename = args
        .next()
        .unwrap_or_else(|| panic!("no key file provided"));

    let listen_addr = args.next().unwrap_or("127.0.0.1:6397".to_string());

    let reg_time = args.next().unwrap_or("30".to_string()).parse()?;
    let reg_time = Instant::now() + Duration::from_secs(reg_time);

    let certfile = std::fs::File::open(cert_filename).expect("cannot open certificate file");
    let mut reader = BufReader::new(certfile);
    let certs: Vec<tokio_rustls::rustls::Certificate> = rustls_pemfile::certs(&mut reader)
        .unwrap()
        .iter()
        .map(|v| tokio_rustls::rustls::Certificate(v.clone()))
        .collect();
    let key = load_private_key(&key_filename);

    let key_log = Arc::new(KeyLogFile::new());
    let mut config = tokio_rustls::rustls::ServerConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    config.key_log = key_log;
    let acceptor = TlsAcceptor::from(Arc::new(config));

    let addr = listen_addr.parse().unwrap();
    let socket = TcpSocket::new_v4()?;
    socket.set_nodelay(true)?;
    socket.bind(addr)?;
    let listener = socket.listen(4096)?;
    log!("listening,{}", listen_addr);

    // Maps the (sender, group) pair to the socket updater.
    let writer_db = Arc::new(RwLock::new(WriterDb::new()));
    // Maps group name to the table of message channels.
    let snd_db = Arc::new(RwLock::new(SndDb::new()));
    // Notifies listener threads when registration phase is over.
    let phase_notify = Arc::new(Notify::new());

    // Allow registering or reconnecting during the registration time.
    while let Ok(accepted) = timeout_at(reg_time, listener.accept()).await {
        let stream = match accepted {
            Ok((stream, _)) => stream,
            Err(e) => {
                log!("failed,accept,{}", e.kind());
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let writer_db = writer_db.clone();
        let snd_db = snd_db.clone();
        let phase_notify = phase_notify.clone();
        #[cfg(feature = "tracing")]
        tokio::spawn(metrics_monitor.instrument(async move {
            handle_handshake::</*REGISTRATION_PHASE=*/ true>(
                stream,
                acceptor,
                writer_db,
                snd_db,
                phase_notify,
            )
            .await
        }));
        #[cfg(not(feature = "tracing"))]
        tokio::spawn(async move {
            handle_handshake::</*REGISTRATION_PHASE=*/ true>(
                stream,
                acceptor,
                writer_db,
                snd_db,
                phase_notify,
            )
            .await
        });
    }

    log!("registration phase complete");
    // Notify all the listener threads that registration is over.
    phase_notify.notify_waiters();

    // Now registration phase is over, only allow reconnecting.
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                log!("failed,accept,{}", e.kind());
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let writer_db = writer_db.clone();
        let snd_db = snd_db.clone();
        let phase_notify = phase_notify.clone();
        tokio::spawn(async move {
            handle_handshake::</*REGISTRATION_PHASE=*/ false>(
                stream,
                acceptor,
                writer_db,
                snd_db,
                phase_notify,
            )
            .await
        });
    }
}

/*
An informal proof that the main thread + handshake threads will not deadlock.
(The rest of the code mainly uses channels so is a lot simpler.)

locks:
 - writer_db (WDB)
 - snd_db (SDB)
   - group_snds (GSS)

== CFG ==
WDB.R() |-> Some(socket_updater) -> SDB.R(); drop(WDB.R, SDB.R)
        |-> None -> drop(WDB.R); SDB.R() |-> Some(group_snds) -> drop(SDB.R); GSS.W(); drop(GSS.W)
                                         |-> None -> drop(SDB.R); SDB.W(); GSS.W(); drop(GSS.W, SDB.W)
=========

The program deadlocks iff lock A can't drop until it gets lock B, while lock B can't drop until it
gets lock A, or a transitive equivalent.

We have three potential locks that can deadlock: WDB, SDB, and GSS.

Can WDB ever deadlock?
It only ever locks in one place: at the start, when the thread holds no other locks.
None case: Drops immediately, never takes any other locks, no opportunity to deadlock.
Some case: Get SDB.R. Can locked SDB ever be waiting for WDB? No, SDB only
locks either after it already has the WDB.R (in another copy of this branch), or the WDB isn't
locked (in the other branch).
This covers all branches, therefore, WDB can never deadlock.

Can GSS ever deadlock?
GSS locks in three places (one of which is not shown in the CFG, it's in get_messages() as
global_db, and is extra irrelevant because it doesn't even read lock until all write lock threads
have terminated). In all three places, it immediately drops the lock without doing any other locking
operations.  Therefore, GSS can never deadlock.

Can SDB ever deadlock?
SDB locks in three places: a read lock in the top None (1), a write lock in the bottom None (2), and
a read lock in the top Some (3).
The read lock in (1) drops before doing any locking operations in either option of the next branch,
and therefore has no chance to deadlock.
The read lock in (3) also does no locking operations before dropping, so has no chance to deadlock.
The write lock in (2) can't deadlock with the GSS write lock, since we already proved GSS never
deadlocks. The only remaining operation is then dropping (2).
Therefore, SDB can never deadlock.
*/

async fn handle_handshake<const REGISTRATION_PHASE: bool>(
    stream: TcpStream,
    acceptor: TlsAcceptor,
    writer_db: Arc<RwLock<WriterDb>>,
    snd_db: Arc<RwLock<SndDb>>,
    phase_notify: Arc<Notify>,
) {
    log!("accepted {}", stream.peer_addr().unwrap());
    let stream = match acceptor.accept(stream).await {
        Ok(stream) => stream,
        Err(e) => {
            log!("failed,tls,{}", e.kind());
            return;
        }
    };

    let (mut rd, wr) = split(stream);

    let handshake = match mgen::get_handshake(&mut rd).await {
        Ok(handshake) => handshake,
        Err(mgen::Error::Io(e)) => {
            log!("failed,handshake,{}", e.kind());
            return;
        }
        Err(mgen::Error::Utf8Error(e)) => panic!("{:?}", e),
        Err(mgen::Error::MalformedSerialization(_, _)) => panic!(),
    };
    log!("accept,{},{}", handshake.sender, handshake.group);

    let read_writer_db = writer_db.read().await;
    if let Some(socket_updater) = read_writer_db.get(&handshake) {
        // we've seen this client before

        // start the new reader thread with a new notify
        // (we can't use the existing notify channel, else we get race conditions where
        // the reader thread terminates and spawns again before the sender thread
        // notices and activates its existing notify channel)
        let socket_notify = Arc::new(Notify::new());
        let db = snd_db.read().await[&handshake.group].clone();
        spawn_message_receiver(
            handshake.sender,
            handshake.group,
            rd,
            db,
            phase_notify,
            socket_notify.clone(),
        );

        // give the writer thread the new write half of the socket and notify
        socket_updater.send((wr, socket_notify));
    } else {
        drop(read_writer_db);
        // newly-registered client
        log!("register,{},{}", handshake.sender, handshake.group);

        if REGISTRATION_PHASE {
            // message channel, for sending messages between threads
            let (msg_snd, msg_rcv) = mpsc::unbounded_channel::<Arc<SerializedMessage>>();

            let group_snds = {
                let read_snd_db = snd_db.read().await;
                let group_snds = read_snd_db.get(&handshake.group);
                if let Some(group_snds) = group_snds {
                    let group_snds = group_snds.clone();
                    drop(read_snd_db);
                    group_snds
                        .write()
                        .await
                        .insert(handshake.sender.clone(), msg_snd);
                    group_snds
                } else {
                    drop(read_snd_db);
                    let mut write_snd_db = snd_db.write().await;
                    let group_snds = write_snd_db
                        .entry(handshake.group.clone())
                        .or_insert_with(|| Arc::new(RwLock::new(HashMap::new())));
                    group_snds
                        .write()
                        .await
                        .insert(handshake.sender.clone(), msg_snd);
                    group_snds.clone()
                }
            };

            // socket notify, for terminating the socket if the sender encounters an error
            let socket_notify = Arc::new(Notify::new());

            // socket updater, for giving the sender thread a new socket + notify channel
            let socket_updater_snd = Updater::new();
            let socket_updater_rcv = socket_updater_snd.clone();
            socket_updater_snd.send((wr, socket_notify.clone()));

            spawn_message_receiver(
                handshake.sender.clone(),
                handshake.group.clone(),
                rd,
                group_snds,
                phase_notify,
                socket_notify,
            );

            let sender = handshake.sender.clone();
            let group = handshake.group.clone();
            tokio::spawn(async move {
                send_messages(sender, group, msg_rcv, socket_updater_rcv).await;
            });

            writer_db
                .write()
                .await
                .insert(handshake, socket_updater_snd);
        } else {
            panic!(
                "late registration: {},{}",
                handshake.sender, handshake.group
            );
        };
    }
}

fn spawn_message_receiver(
    sender: String,
    group: String,
    rd: ReadHalf<TlsStream<TcpStream>>,
    db: Arc<RwLock<HashMap<ID, ReaderToSender>>>,
    phase_notify: Arc<Notify>,
    socket_notify: Arc<Notify>,
) {
    tokio::spawn(async move {
        tokio::select! {
            // n.b.: get_message is not cancellation safe,
            // but this is one of the cases where that's expected
            // (we only cancel when something is wrong with the stream anyway)
            ret = get_messages(&sender, &group, rd, phase_notify, db) => {
                match ret {
                    Err(mgen::Error::Io(e)) => log!("failed,receive,{}", e.kind()),
                    Err(mgen::Error::Utf8Error(e)) => panic!("{:?}", e),
                    Err(mgen::Error::MalformedSerialization(v, b)) => panic!(
                        "Malformed Serialization: {:?}\n{:?})", v, b),
                    Ok(()) => panic!("Message receiver returned OK"),
                }
            }
            _ = socket_notify.notified() => {
                log!("terminated,{},{}", sender, group);
                // should cause get_messages to terminate, dropping the socket
            }
        }
    });
}

/// Loop for receiving messages on the socket, figuring out who to deliver them to,
/// and forwarding them locally to the respective channel.
async fn get_messages<T: tokio::io::AsyncRead>(
    sender: &str,
    group: &str,
    mut socket: ReadHalf<T>,
    phase_notify: Arc<Notify>,
    global_db: Arc<RwLock<HashMap<ID, ReaderToSender>>>,
) -> Result<(), mgen::Error> {
    // Wait for the registration phase to end before updating our local copy of the DB
    phase_notify.notified().await;

    let db = global_db.read().await.clone();
    let message_channels: Vec<_> = db
        .iter()
        .filter_map(|(k, v)| if *k != sender { Some(v) } else { None })
        .collect();

    loop {
        let buf = mgen::get_message_bytes::<false, _>(&mut socket).await?;
        let message = MessageHeaderRef::deserialize(&buf[4..])?;
        assert!(message.sender == sender);

        match message.body {
            MessageBody::Size(_) => {
                assert!(message.group == group);
                log!("received,{},{},{}", sender, group, message.id);
                let body = message.body;
                let m = Arc::new(SerializedMessage { header: buf, body });
                for recipient in message_channels.iter() {
                    recipient.send(m.clone()).unwrap();
                }
            }
            MessageBody::Receipt => {
                log!(
                    "receipt,{},{},{},{}",
                    sender,
                    group,
                    message.group,
                    message.id
                );
                let recipient = &db[message.group];
                let body = message.body;
                let m = Arc::new(SerializedMessage { header: buf, body });
                recipient.send(m).unwrap();
            }
        }
    }
}

/// Loop for receiving messages on the mpsc channel for this recipient,
/// and sending them out on the associated socket.
async fn send_messages<T: Send + Sync + tokio::io::AsyncWrite>(
    recipient: ID,
    group: ID,
    mut msg_rcv: mpsc::UnboundedReceiver<Arc<SerializedMessage>>,
    mut socket_updater: Updater<(WriteHalf<T>, Arc<Notify>)>,
) {
    let (mut current_socket, mut current_watch) = socket_updater.recv().await;
    let mut message_cache = None;
    loop {
        let message = if let Some(message) = message_cache {
            message
        } else {
            msg_rcv.recv().await.expect("message channel closed")
        };
        if message
            .write_all_to::<false, _>(&mut current_socket)
            .await
            .is_err()
            || current_socket.flush().await.is_err()
        {
            message_cache = Some(message);
            log!("terminating,{},{}", recipient, group);
            // socket is presumably closed, clean up and notify the listening end to close
            // (all best-effort, we can ignore errors because it presumably means it's done)
            current_watch.notify_one();
            let _ = current_socket.shutdown().await;

            // wait for the new socket
            (current_socket, current_watch) = socket_updater.recv().await;
        } else {
            log!("sent,{},{}", recipient, group);
            message_cache = None;
        }
    }
}

fn load_private_key(filename: &str) -> PrivateKey {
    let keyfile = std::fs::File::open(filename).expect("cannot open private key file");
    let mut reader = BufReader::new(keyfile);

    loop {
        match rustls_pemfile::read_one(&mut reader).expect("cannot parse private key .pem file") {
            Some(rustls_pemfile::Item::RSAKey(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::PKCS8Key(key)) => return PrivateKey(key),
            Some(rustls_pemfile::Item::ECKey(key)) => return PrivateKey(key),
            None => break,
            _ => {}
        }
    }

    panic!(
        "no keys found in {:?} (encrypted keys not supported)",
        filename
    );
}
