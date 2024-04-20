// Code specific to the client in the client-server mode.

use mgen::{log, updater::Updater, HandshakeRef, MessageHeader, SerializedMessage};

use futures::future::try_join_all;
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use hyper_socks2::SocksConnector;
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256PlusPlus};
use serde::Deserialize;
use std::hash::{Hash, Hasher};
use std::result::Result;
use std::sync::Arc;
use tokio::io::{split, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::spawn;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tokio_rustls::{client::TlsStream, TlsConnector};

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
/// Type of messages sent to the writer thread.
type MessageHolder = Box<SerializedMessage>;
/// Type for getting messages from the state thread in the writer thread.
type WriterFromState = mpsc::UnboundedReceiver<MessageHolder>;
/// Type for sending the updated read half of the socket.
type ReadSocketUpdaterIn = Updater<ReadHalf<TlsStream<TcpStream>>>;
/// Type for getting the updated read half of the socket.
type ReadSocketUpdaterOut = Updater<ReadHalf<TlsStream<TcpStream>>>;
/// Type for sending the updated write half of the socket.
type WriteSocketUpdaterIn = Updater<WriteHalf<TlsStream<TcpStream>>>;
/// Type for getting the updated write half of the socket.
type WriteSocketUpdaterOut = Updater<WriteHalf<TlsStream<TcpStream>>>;
/// Type for sending errors to other threads.
type ErrorChannelIn = mpsc::UnboundedSender<MessengerError>;
/// Type for getting errors from other threads.
type ErrorChannelOut = mpsc::UnboundedReceiver<MessengerError>;
/// Type for sending sizes to the attachment sender thread.
type SizeChannelIn = mpsc::UnboundedSender<usize>;
/// Type for getting sizes from other threads.
type SizeChannelOut = mpsc::UnboundedReceiver<usize>;

// we gain a (very) tiny performance win by not bothering to validate the cert
struct NoCertificateVerification {}

impl tokio_rustls::rustls::client::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &tokio_rustls::rustls::Certificate,
        _intermediates: &[tokio_rustls::rustls::Certificate],
        _server_name: &tokio_rustls::rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<tokio_rustls::rustls::client::ServerCertVerified, tokio_rustls::rustls::Error> {
        Ok(tokio_rustls::rustls::client::ServerCertVerified::assertion())
    }
}

/// Create a URL the web server can use to accept or produce traffic.
/// `target` is the IP or host name of the web server,
/// `size` is the number of bytes to download or upload,
/// `user` is to let the server log the user making the request.
/// Panics if the arguments do not produce a valid URI.
fn web_url(target: &str, size: usize, user: &str) -> hyper::Uri {
    let formatted = format!("https://{}/?size={}&user={}", target, size, user);
    formatted
        .parse()
        .unwrap_or_else(|_| panic!("Invalid URI: {}", formatted))
}

fn get_plain_https_client(
    tls_config: tokio_rustls::rustls::ClientConfig,
) -> hyper::client::Client<HttpsConnector<HttpConnector>> {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .build();
    hyper::Client::builder().build(https)
}

fn get_socks_https_client(
    tls_config: tokio_rustls::rustls::ClientConfig,
    username: String,
    password: String,
    proxy: String,
) -> hyper::client::Client<HttpsConnector<SocksConnector<HttpConnector>>> {
    let mut http = hyper::client::HttpConnector::new();
    http.enforce_http(false);

    let auth = hyper_socks2::Auth { username, password };
    let socks = hyper_socks2::SocksConnector {
        proxy_addr: format!("socks5://{}", proxy)
            .parse()
            .expect("Invalid proxy URI"),
        auth: Some(auth),
        connector: http,
    };
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .wrap_connector(socks);
    hyper::Client::builder().build(https)
}

/// The thread responsible for getting incoming messages,
/// checking for any network errors while doing so,
/// and giving messages to the state thread.
async fn reader(
    web_params: SocksParams,
    retry: Duration,
    tls_config: tokio_rustls::rustls::ClientConfig,
    message_channel: ReaderToState,
    socket_updater: ReadSocketUpdaterOut,
    error_channel: ErrorChannelIn,
) {
    match web_params.socks {
        Some(proxy) => {
            let client = get_socks_https_client(
                tls_config,
                web_params.user.clone(),
                web_params.target.clone(),
                proxy,
            );
            worker(
                web_params.target,
                web_params.user,
                retry,
                client,
                message_channel,
                socket_updater,
                error_channel,
            )
            .await
        }
        None => {
            let client = get_plain_https_client(tls_config);
            worker(
                web_params.target,
                web_params.user,
                retry,
                client,
                message_channel,
                socket_updater,
                error_channel,
            )
            .await
        }
    };

    async fn worker<C>(
        target: String,
        user: String,
        retry: Duration,
        client: hyper::Client<C, hyper::Body>,
        message_channel: ReaderToState,
        mut socket_updater: ReadSocketUpdaterOut,
        error_channel: ErrorChannelIn,
    ) where
        C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
    {
        loop {
            let mut message_stream = socket_updater.recv().await;

            loop {
                let msg = match mgen::get_message::<false, _>(&mut message_stream).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        error_channel.send(e.into()).expect("Error channel closed");
                        break;
                    }
                };

                if msg.body.has_attachment() {
                    let url = web_url(&target, msg.body.total_size(), &user);
                    let client = client.clone();
                    spawn(async move {
                        let mut res = client.get(url.clone()).await;
                        while res.is_err() {
                            log!("Error fetching: {}", res.unwrap_err());
                            sleep(retry).await;
                            res = client.get(url.clone()).await;
                        }
                    });
                }

                message_channel
                    .send(msg)
                    .expect("Reader message channel closed");
            }
        }
    }
}

async fn uploader(
    web_params: SocksParams,
    retry: Duration,
    tls_config: tokio_rustls::rustls::ClientConfig,
    size_channel: SizeChannelOut,
) {
    match web_params.socks {
        Some(proxy) => {
            let client = get_socks_https_client(
                tls_config,
                web_params.user.clone(),
                web_params.target.clone(),
                proxy,
            );
            worker(
                web_params.target,
                web_params.user,
                retry,
                client,
                size_channel,
            )
            .await
        }
        None => {
            let client = get_plain_https_client(tls_config);
            worker(
                web_params.target,
                web_params.user,
                retry,
                client,
                size_channel,
            )
            .await
        }
    }

    async fn worker<C>(
        target: String,
        user: String,
        retry: Duration,
        client: hyper::Client<C, hyper::Body>,
        mut size_channel: SizeChannelOut,
    ) where
        C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
    {
        loop {
            let size = size_channel.recv().await.expect("Size channel closed");
            let client = client.clone();
            let url = web_url(&target, size, &user);
            let request = hyper::Request::put(url.clone())
                .body(hyper::Body::empty())
                .expect("Invalid HTTP request attempted to construct");
            let mut res = client.request(request).await;
            while res.is_err() {
                log!("{},{},Error uploading: {}", user, url, res.unwrap_err());
                sleep(retry).await;
                res = client.get(url.clone()).await;
            }
        }
    }
}

/// The thread responsible for sending messages from the state thread,
/// and checking for any network errors while doing so.
async fn writer(
    mut message_channel: WriterFromState,
    attachment_channel: SizeChannelIn,
    mut socket_updater: WriteSocketUpdaterOut,
    error_channel: ErrorChannelIn,
) {
    loop {
        let mut stream = socket_updater.recv().await;
        loop {
            let msg = message_channel
                .recv()
                .await
                .expect("Writer message channel closed");

            if msg.body.has_attachment() {
                attachment_channel
                    .send(msg.body.total_size())
                    .expect("Attachment channel closed");
            }

            if let Err(e) = msg.write_all_to::<false, _>(&mut stream).await {
                error_channel.send(e.into()).expect("Error channel closed");
                break;
            }
        }
    }
}

/// The thread responsible for (re-)establishing connections to the server,
/// and determining how to handle errors this or other threads receive.
async fn socket_updater(
    str_params: SocksParams,
    retry: Duration,
    tls_config: tokio_rustls::rustls::ClientConfig,
    mut error_channel: ErrorChannelOut,
    reader_channel: ReadSocketUpdaterIn,
    writer_channel: WriteSocketUpdaterIn,
) -> FatalError {
    let connector = TlsConnector::from(Arc::new(tls_config));

    // unwrap is safe, split always returns at least one element
    let tls_server_str = str_params.target.split(':').next().unwrap();
    let tls_server_name =
        tokio_rustls::rustls::ServerName::try_from(tls_server_str).expect("invalid server name");

    loop {
        let stream: TcpStream = match connect(&str_params).await {
            Ok(stream) => stream,
            Err(MessengerError::Recoverable(e)) => {
                log!(
                    "{},{},error,TCP,{:?}",
                    str_params.user,
                    str_params.recipient,
                    e
                );
                sleep(retry).await;
                continue;
            }
            Err(MessengerError::Fatal(e)) => return e,
        };

        let mut stream = match connector.connect(tls_server_name.clone(), stream).await {
            Ok(stream) => stream,
            Err(e) => {
                log!(
                    "{},{},error,TLS,{:?}",
                    str_params.user,
                    str_params.recipient,
                    e
                );
                sleep(retry).await;
                continue;
            }
        };

        let handshake = HandshakeRef {
            sender: &str_params.user,
            group: &str_params.recipient,
        };

        if stream.write_all(&handshake.serialize()).await.is_err() {
            continue;
        }
        log!("{},{},handshake", str_params.user, str_params.recipient);

        let (rd, wr) = split(stream);
        reader_channel.send(rd);
        writer_channel.send(wr);

        let res = error_channel.recv().await.expect("Error channel closed");
        if let MessengerError::Fatal(e) = res {
            return e;
        } else {
            log!(
                "{},{},error,{:?}",
                str_params.user,
                str_params.recipient,
                res
            );
        }
    }
}

/// The thread responsible for handling the conversation state
/// (i.e., whether the user is active or idle, and when to send messages).
async fn manage_conversation(
    config: FullConfig,
    mut state_from_reader: StateFromReader,
    mut state_to_writer: StateToWriter<MessageHolder>,
) {
    sleep(Duration::from_secs_f64(config.bootstrap)).await;
    log!("{},{},awake", &config.user, &config.group);

    let mut rng = Xoshiro256PlusPlus::from_entropy();
    let mut state_machine = StateMachine::start(config.distributions, &mut rng);

    loop {
        state_machine = match state_machine {
            StateMachine::Idle(conversation) => {
                manage_idle_conversation::<false, _, _, _>(
                    conversation,
                    &mut state_from_reader,
                    &mut state_to_writer,
                    &config.user,
                    &config.group,
                    &mut rng,
                )
                .await
            }
            StateMachine::Active(conversation) => {
                manage_active_conversation(
                    conversation,
                    &mut state_from_reader,
                    &mut state_to_writer,
                    &config.user,
                    &config.group,
                    false,
                    &mut rng,
                )
                .await
            }
        };
    }
}

/// Spawns all other threads for this conversation.
async fn spawn_threads(config: FullConfig) -> Result<(), MessengerError> {
    // without noise during Shadow's bootstrap period, we can overload the SOMAXCONN of the server,
    // so we wait a small(ish) pseudorandom amount of time to spread things out
    let mut hasher = rustc_hash::FxHasher::default();
    config.user.hash(&mut hasher);
    config.group.hash(&mut hasher);
    let hash = hasher.finish() % 10_000;
    log!("{},{},waiting,{}", config.user, config.group, hash);
    sleep(Duration::from_millis(hash)).await;

    let message_server_params = SocksParams {
        socks: config.socks.clone(),
        target: config.message_server.clone(),
        user: config.user.clone(),
        recipient: config.group.clone(),
    };

    let web_server_params = SocksParams {
        socks: config.socks.clone(),
        target: config.web_server.clone(),
        user: config.user.clone(),
        recipient: config.group.clone(),
    };

    let (reader_to_state, state_from_reader) = mpsc::unbounded_channel();
    let (state_to_writer, writer_from_state) = mpsc::unbounded_channel();
    let read_socket_updater_in = Updater::new();
    let read_socket_updater_out = read_socket_updater_in.clone();
    let write_socket_updater_in = Updater::new();
    let write_socket_updater_out = write_socket_updater_in.clone();
    let (errs_in, errs_out) = mpsc::unbounded_channel();
    let (writer_to_uploader, uploader_from_writer) = mpsc::unbounded_channel();

    let state_to_writer = StateToWriter {
        channel: state_to_writer,
    };

    let retry = Duration::from_secs_f64(config.retry);
    let tls_config = tokio_rustls::rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification {}))
        .with_no_client_auth();

    spawn(reader(
        web_server_params.clone(),
        retry,
        tls_config.clone(),
        reader_to_state,
        read_socket_updater_out,
        errs_in.clone(),
    ));
    spawn(writer(
        writer_from_state,
        writer_to_uploader,
        write_socket_updater_out,
        errs_in,
    ));
    spawn(uploader(
        web_server_params,
        retry,
        tls_config.clone(),
        uploader_from_writer,
    ));
    spawn(manage_conversation(
        config,
        state_from_reader,
        state_to_writer,
    ));

    Err(MessengerError::Fatal(
        socket_updater(
            message_server_params,
            retry,
            tls_config,
            errs_out,
            read_socket_updater_in,
            write_socket_updater_in,
        )
        .await,
    ))
}

struct FullConfig {
    user: String,
    group: String,
    socks: Option<String>,
    message_server: String,
    web_server: String,
    bootstrap: f64,
    retry: f64,
    distributions: Distributions,
}

#[derive(Debug, Deserialize)]
struct ConversationConfig {
    group: String,
    message_server: Option<String>,
    web_server: Option<String>,
    bootstrap: Option<f64>,
    retry: Option<f64>,
    distributions: Option<ConfigDistributions>,
}

#[derive(Debug, Deserialize)]
struct Config {
    user: String,
    socks: Option<String>,
    message_server: String,
    web_server: String,
    bootstrap: f64,
    retry: f64,
    distributions: ConfigDistributions,
    conversations: Vec<ConversationConfig>,
}

async fn main_worker() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "tracing")]
    console_subscriber::init();

    let mut args = std::env::args();
    let _ = args.next();
    let mut handles = vec![];
    for config_file in args.flat_map(|a| glob::glob(a.as_str()).unwrap()) {
        let yaml_s = std::fs::read_to_string(config_file?)?;
        let config: Config = serde_yaml::from_str(&yaml_s)?;
        let default_dists: Distributions = config.distributions.try_into()?;
        for conversation in config.conversations.into_iter() {
            let distributions: Distributions = match conversation.distributions {
                Some(dists) => dists.try_into()?,
                None => default_dists.clone(),
            };
            let filled_conversation = FullConfig {
                user: config.user.clone(),
                group: conversation.group,
                socks: config.socks.clone(),
                message_server: conversation
                    .message_server
                    .unwrap_or_else(|| config.message_server.clone()),
                web_server: conversation
                    .web_server
                    .unwrap_or_else(|| config.web_server.clone()),
                bootstrap: conversation.bootstrap.unwrap_or(config.bootstrap),
                retry: conversation.retry.unwrap_or(config.retry),
                distributions,
            };
            let handle = spawn_threads(filled_conversation);
            handles.push(handle);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_generation() {
        // should panic if any of these are invalid
        web_url("192.0.2.1", 0, "Alice");
        web_url("hostname", 65536, "Bob");
        web_url("web0", 4294967295, "Carol");
        web_url("web1", 1, "");
        web_url("foo.bar.baz", 1, "Dave");

        // IPv6 is not a valid in a URI
        //web_url("2001:0db8:85a3:0000:0000:8a2e:0370:7334", 1, "1");

        // hyper does not automatically convert to punycode
        //web_url("web2", 1, "🦀");
        //web_url("🦀", 1, "Ferris");
    }
}
