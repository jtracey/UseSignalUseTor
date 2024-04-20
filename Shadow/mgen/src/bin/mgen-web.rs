// An extremely minimal http server.
// Accepts https GET requests with a "size" parameter, and responds with that many bytes.
// Accepts all other https requests, and ignores whatever was given.

use hyper::server::conn::AddrIncoming;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper_rustls::TlsAcceptor;
use mgen::log;
use std::collections::HashMap;
use std::io::BufReader;
use std::str::FromStr;
use tokio_rustls::rustls::PrivateKey;

static MISSING: &[u8] = b"Missing field";
static NOTNUMERIC: &[u8] = b"Number field is not numeric";

async fn process(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let Some(query) = req.uri().query()
    else {
        return Ok(Response::builder()
                  .status(StatusCode::UNPROCESSABLE_ENTITY)
                  .body(Body::from(MISSING))
                  .unwrap());
    };

    let params = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect::<HashMap<String, String>>();

    let Some(from) = params.get("user")
    else {
        return Ok(Response::builder()
                  .status(StatusCode::UNPROCESSABLE_ENTITY)
                  .body(Body::from(MISSING))
                  .unwrap());
    };

    match req.method() {
        &Method::GET => {
            let Ok(size) = params
                .get("size")
                .map_or(Ok(0), |s| usize::from_str(s.as_str()))
            else {
                return Ok(Response::builder()
                          .status(StatusCode::UNPROCESSABLE_ENTITY)
                          .body(Body::from(NOTNUMERIC))
                          .unwrap());
            };
            log!("sending,{},{}", from, size);

            let body = Body::from(vec![0; size]);
            Ok(Response::new(body))
        }

        _ => {
            log!("receiving,{}", from);
            Ok(Response::new(Body::empty()))
        }
    }
}

// FIXME: move this out into module to share with server (along with other shared functionality)
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = std::env::args();
    let _arg0 = args.next().unwrap();

    let cert_filename = args
        .next()
        .unwrap_or_else(|| panic!("no cert file provided"));
    let key_filename = args
        .next()
        .unwrap_or_else(|| panic!("no key file provided"));

    let listen_addr = args.next().unwrap_or("127.0.0.1:6398".to_string());

    let certfile = std::fs::File::open(cert_filename).expect("cannot open certificate file");
    let mut reader = BufReader::new(certfile);
    let certs: Vec<tokio_rustls::rustls::Certificate> = rustls_pemfile::certs(&mut reader)
        .unwrap()
        .iter()
        .map(|v| tokio_rustls::rustls::Certificate(v.clone()))
        .collect();
    let key = load_private_key(&key_filename);

    let config = tokio_rustls::rustls::ServerConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    let incoming = AddrIncoming::bind(&listen_addr.parse()?)?;

    let acceptor = TlsAcceptor::builder()
        .with_tls_config(config)
        .with_http11_alpn()
        .with_incoming(incoming);

    let service = make_service_fn(|_| async { Ok::<_, std::io::Error>(service_fn(process)) });
    let server = Server::builder(acceptor).serve(service);

    log!("listening,{}", &listen_addr);
    server.await?;
    Ok(())
}
