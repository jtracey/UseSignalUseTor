use std::mem::size_of;
use std::num::NonZeroU32;
use tokio::io::{copy, sink, AsyncReadExt, AsyncWriteExt};

pub mod updater;

/// The padding interval in bytes. All message bodies are a size of some multiple of this.
/// All messages bodies are a minimum  of this size.
// from https://github.com/signalapp/libsignal/blob/af7bb8567c812aa13625fc90076bf71a59d64ff5/rust/protocol/src/crypto.rs#L92C41-L92C41
pub const PADDING_BLOCK_SIZE: u32 = 10 * 128 / 8;
/// The most blocks a message body can contain.
// from https://github.com/signalapp/Signal-Android/blob/36a8c4d8ba9fdb62905ecb9a20e3eeba4d2f9022/app/src/main/java/org/thoughtcrime/securesms/mms/PushMediaConstraints.java
pub const MAX_BLOCKS_IN_BODY: u32 = (100 * 1024 * 1024) / PADDING_BLOCK_SIZE;
/// The maxmimum number of bytes that can be sent inline; larger values use the HTTP server.
// In actuality, this is 2000 for Signal:
// https://github.com/signalapp/Signal-Android/blob/244902ecfc30e21287a35bb1680e2dbe6366975b/app/src/main/java/org/thoughtcrime/securesms/util/PushCharacterCalculator.java#L23
// but we align to a close block count since in practice we sample from block counts
pub const INLINE_MAX_SIZE: u32 = 14 * PADDING_BLOCK_SIZE;

#[macro_export]
macro_rules! log {
    ( $( $x:expr ),* ) => {
        println!("{}{}",
                 chrono::offset::Utc::now().format("%F %T,%s.%f,"),
                 format_args!($( $x ),*)
        );
    }
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Utf8Error(std::str::Utf8Error),
    MalformedSerialization(Vec<u8>, std::backtrace::Backtrace),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Self {
        Self::Utf8Error(e)
    }
}

/// Metadata for the body of the message.
///
/// Message contents are always 0-filled buffers, so never represented.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum MessageBody {
    Receipt,
    Size(NonZeroU32),
}

impl MessageBody {
    /// Whether the body of the message requires an HTTP GET
    /// (attachment size is the message size).
    pub fn has_attachment(&self) -> bool {
        match self {
            MessageBody::Receipt => false,
            MessageBody::Size(size) => size > &NonZeroU32::new(INLINE_MAX_SIZE).unwrap(),
        }
    }

    /// Size on the wire of the message's body, exluding bytes fetched via http
    fn inline_size<const P2P: bool>(&self) -> usize {
        match self {
            MessageBody::Receipt => PADDING_BLOCK_SIZE as usize,
            MessageBody::Size(size) => {
                let size = size.get();
                if P2P || size <= INLINE_MAX_SIZE {
                    size as usize
                } else {
                    INLINE_MAX_SIZE as usize
                }
            }
        }
    }

    /// Size of the message's body, including bytes fetched via http
    pub fn total_size(&self) -> usize {
        match self {
            MessageBody::Receipt => PADDING_BLOCK_SIZE as usize,
            MessageBody::Size(size) => size.get() as usize,
        }
    }
}

/// Message metadata.
///
/// This has everything needed to reconstruct a message.
// FIXME: we should try to replace MessageHeader with MessageHeaderRef
#[derive(Debug, PartialEq)]
pub struct MessageHeader {
    /// User who constructed the message.
    pub sender: String,
    /// Group associated with the message.
    /// In client-server mode receipts, this is the recipient instead.
    pub group: String,
    /// ID unique to a message and its receipt for a (sender, group) pair.
    pub id: u32,
    /// The type and size of the message payload.
    pub body: MessageBody,
}

impl MessageHeader {
    /// Generate a concise serialization of the Message.
    pub fn serialize(&self) -> SerializedMessage {
        // serialized message header: {
        //   header_len: u32,
        //   sender: {u32, utf-8},
        //   group: {u32, utf-8},
        //   id: u32,
        //   body_type: MessageBody (i.e., u32)
        // }

        let body_type = match self.body {
            MessageBody::Receipt => 0,
            MessageBody::Size(s) => s.get(),
        };

        let header_len =
            (1 + 1 + 1 + 1 + 1) * size_of::<u32>() + self.sender.len() + self.group.len();

        let mut header: Vec<u8> = Vec::with_capacity(header_len);

        let header_len = header_len as u32;
        header.extend(header_len.to_be_bytes());

        serialize_str_to(&self.sender, &mut header);
        serialize_str_to(&self.group, &mut header);

        header.extend(self.id.to_be_bytes());

        header.extend(body_type.to_be_bytes());

        assert!(header.len() == header_len as usize);
        SerializedMessage {
            header,
            body: self.body,
        }
    }

    /// Creates a MessageHeader from bytes created via serialization,
    /// but with the size already parsed out.
    fn deserialize(buf: &[u8]) -> Result<Self, Error> {
        let (sender, buf) = deserialize_str(buf)?;
        let sender = sender.to_string();

        let (group, buf) = deserialize_str(buf)?;
        let group = group.to_string();

        let (id, buf) = deserialize_u32(buf)?;

        let (body, _) = deserialize_u32(buf)?;
        let body = if let Some(size) = NonZeroU32::new(body) {
            MessageBody::Size(size)
        } else {
            MessageBody::Receipt
        };
        Ok(Self {
            sender,
            group,
            id,
            body,
        })
    }
}

/// Message metadata.
///
/// This has everything needed to reconstruct a message.
#[derive(Debug)]
pub struct MessageHeaderRef<'a> {
    pub sender: &'a str,
    pub group: &'a str,
    pub id: u32,
    pub body: MessageBody,
}

impl<'a> MessageHeaderRef<'a> {
    /// Creates a MessageHeader from bytes created via serialization,
    /// but with the size already parsed out.
    pub fn deserialize(buf: &'a [u8]) -> Result<Self, Error> {
        let (sender, buf) = deserialize_str(buf)?;

        let (group, buf) = deserialize_str(buf)?;

        let (id, buf) = deserialize_u32(buf)?;

        let (body, _) = deserialize_u32(buf)?;
        let body = if let Some(size) = NonZeroU32::new(body) {
            MessageBody::Size(size)
        } else {
            MessageBody::Receipt
        };
        Ok(Self {
            sender,
            group,
            id,
            body,
        })
    }
}

/// Parse the identifier from the start of the TcpStream.
pub async fn parse_identifier<T: AsyncReadExt + std::marker::Unpin>(
    stream: &mut T,
) -> Result<String, Error> {
    // this should maybe be buffered
    let strlen = stream.read_u32().await?;
    let mut buf = vec![0u8; strlen as usize];
    stream.read_exact(&mut buf).await?;
    let s = std::str::from_utf8(&buf)?;
    Ok(s.to_string())
}

/// Gets a message from the stream, returning the raw byte buffer
pub async fn get_message_bytes<const P2P: bool, T: AsyncReadExt + std::marker::Unpin>(
    stream: &mut T,
) -> Result<Vec<u8>, Error> {
    let mut header_size_bytes = [0u8; 4];
    stream.read_exact(&mut header_size_bytes).await?;
    get_message_with_header_size::<P2P, _>(stream, header_size_bytes).await
}

/// Gets a message from the stream and constructs a MessageHeader object
pub async fn get_message<const P2P: bool, T: AsyncReadExt + std::marker::Unpin>(
    stream: &mut T,
) -> Result<MessageHeader, Error> {
    let buf = get_message_bytes::<P2P, _>(stream).await?;
    let msg = MessageHeader::deserialize(&buf[4..])?;
    Ok(msg)
}

async fn get_message_with_header_size<const P2P: bool, T: AsyncReadExt + std::marker::Unpin>(
    stream: &mut T,
    header_size_bytes: [u8; 4],
) -> Result<Vec<u8>, Error> {
    let header_size = u32::from_be_bytes(header_size_bytes);
    let mut header_buf = vec![0; header_size as usize];
    stream.read_exact(&mut header_buf[4..]).await?;
    let header = MessageHeader::deserialize(&header_buf[4..])?;
    let header_size_buf = &mut header_buf[..4];
    header_size_buf.copy_from_slice(&header_size_bytes);
    copy(
        &mut stream.take(header.body.inline_size::<P2P>() as u64),
        &mut sink(),
    )
    .await?;
    Ok(header_buf)
}

pub fn serialize_str(s: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(s.len() + size_of::<u32>());
    serialize_str_to(s, &mut buf);
    buf
}

pub fn serialize_str_to(s: &str, buf: &mut Vec<u8>) {
    let strlen = s.len() as u32;
    buf.extend(strlen.to_be_bytes());
    buf.extend(s.as_bytes());
}

fn deserialize_u32(buf: &[u8]) -> Result<(u32, &[u8]), Error> {
    let bytes = buf.get(0..4).ok_or_else(|| {
        Error::MalformedSerialization(buf.to_vec(), std::backtrace::Backtrace::capture())
    })?;
    Ok((u32::from_be_bytes(bytes.try_into().unwrap()), &buf[4..]))
}

fn deserialize_str(buf: &[u8]) -> Result<(&str, &[u8]), Error> {
    let (strlen, buf) = deserialize_u32(buf)?;
    let strlen = strlen as usize;
    let strbytes = buf.get(..strlen).ok_or_else(|| {
        Error::MalformedSerialization(buf.to_vec(), std::backtrace::Backtrace::capture())
    })?;
    Ok((std::str::from_utf8(strbytes)?, &buf[strlen..]))
}

/// A message almost ready for sending.
///
/// We represent each message in two halves: the header, and the body.
/// This way, the server can parse out the header in its own buf,
/// and just pass that around intact, without keeping a (possibly large)
/// 0-filled body around.
#[derive(Debug)]
pub struct SerializedMessage {
    pub header: Vec<u8>,
    pub body: MessageBody,
}

impl SerializedMessage {
    pub async fn write_all_to<const P2P: bool, T: AsyncWriteExt + std::marker::Unpin>(
        &self,
        writer: &mut T,
    ) -> std::io::Result<()> {
        let body_buf = vec![0; self.body.inline_size::<P2P>()];

        // write_all_vectored is not yet stable x_x
        // https://github.com/rust-lang/rust/issues/70436
        let mut header: &[u8] = &self.header;
        let mut body: &[u8] = &body_buf;
        loop {
            let bufs = [std::io::IoSlice::new(header), std::io::IoSlice::new(body)];
            match writer.write_vectored(&bufs).await {
                Ok(written) => {
                    if written == header.len() + body.len() {
                        return Ok(());
                    }

                    if written >= header.len() {
                        body = &body[written - header.len()..];
                        break;
                    } else if written == 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WriteZero,
                            "failed to write any bytes from message with bytes remaining",
                        ));
                    } else {
                        header = &header[written..];
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        writer.write_all(body).await
    }
}

/// Handshake between client and server (peers do not use).
#[derive(Eq, Debug, Hash, PartialEq)]
pub struct Handshake {
    /// Who is sending this handshake.
    pub sender: String,
    /// For normal messages, the group the message was sent to.
    /// For receipts, the client the receipt is for.
    pub group: String,
}

impl Handshake {
    /// Generate a serialized handshake message.
    pub fn serialize(&self) -> Vec<u8> {
        serialize_handshake(&self.sender, &self.group)
    }
}

/// Gets a handshake from the stream and constructs a Handshake object
pub async fn get_handshake<T: AsyncReadExt + std::marker::Unpin>(
    stream: &mut T,
) -> Result<Handshake, Error> {
    let sender = parse_identifier(stream).await?;
    let group = parse_identifier(stream).await?;
    Ok(Handshake { sender, group })
}

/// A reference to a Handshake's fields.
pub struct HandshakeRef<'a> {
    pub sender: &'a str,
    pub group: &'a str,
}

impl HandshakeRef<'_> {
    /// Generate a serialized handshake message.
    pub fn serialize(&self) -> Vec<u8> {
        serialize_handshake(self.sender, self.group)
    }
}

fn serialize_handshake(sender: &str, group: &str) -> Vec<u8> {
    // serialized handshake: {
    //   sender: {u32, utf-8}
    //   group: {u32, utf-8}
    // }

    let handshake_len = (1 + 1) * size_of::<u32>() + sender.len() + group.len();

    let mut handshake: Vec<u8> = Vec::with_capacity(handshake_len);

    serialize_str_to(sender, &mut handshake);
    serialize_str_to(group, &mut handshake);

    debug_assert!(handshake.len() == handshake_len);
    handshake
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs::{File, OpenOptions};

    /// creates a temporary file for writing
    async fn generate_tmp_file(name: &str) -> File {
        let filename = format!("mgen-test-{}", name);
        let mut path = std::env::temp_dir();
        path.push(filename);
        OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)
            .await
            .unwrap()
    }

    /// get an existing temp file for reading
    async fn get_tmp_file(name: &str) -> File {
        let filename = format!("mgen-test-{}", name);
        let mut path = std::env::temp_dir();
        path.push(filename);
        OpenOptions::new().read(true).open(path).await.unwrap()
    }

    #[test]
    fn serialize_deserialize_message() {
        let m1 = MessageHeader {
            sender: "Alice".to_string(),
            group: "group".to_string(),
            id: 1024,
            body: MessageBody::Size(NonZeroU32::new(256).unwrap()),
        };

        let serialized = m1.serialize();

        let m2 = MessageHeader::deserialize(&serialized.header[4..]).unwrap();
        assert_eq!(m1, m2);
    }

    #[test]
    fn serialize_deserialize_receipt() {
        let m1 = MessageHeader {
            sender: "Alice".to_string(),
            group: "group".to_string(),
            id: 1024,
            body: MessageBody::Receipt,
        };

        let serialized = m1.serialize();

        let m2 = MessageHeader::deserialize(&serialized.header[4..]).unwrap();
        assert_eq!(m1, m2);
    }

    #[test]
    fn deserialize_message_ref() {
        let m1 = MessageHeader {
            sender: "Alice".to_string(),
            group: "group".to_string(),
            id: 1024,
            body: MessageBody::Size(NonZeroU32::new(256).unwrap()),
        };

        let serialized = m1.serialize();

        let m2 = MessageHeaderRef::deserialize(&serialized.header[4..]).unwrap();

        assert_eq!(m1.sender, m2.sender);
        assert_eq!(m1.group, m2.group);
        assert_eq!(m1.body, m2.body);
    }

    #[test]
    fn deserialize_receipt_ref() {
        let m1 = MessageHeader {
            sender: "Alice".to_string(),
            group: "group".to_string(),
            id: 1024,
            body: MessageBody::Receipt,
        };

        let serialized = m1.serialize();

        let m2 = MessageHeaderRef::deserialize(&serialized.header[4..]).unwrap();

        assert_eq!(m1.sender, m2.sender);
        assert_eq!(m1.group, m2.group);
        assert_eq!(m1.body, m2.body);
    }

    async fn serialize_get_message_generic<const P2P: bool>() {
        let m1 = MessageHeader {
            sender: "Alice".to_string(),
            group: "group".to_string(),
            id: 1024,
            body: MessageBody::Size(NonZeroU32::new(256).unwrap()),
        };

        let serialized = m1.serialize();

        let file_name = "serialize_message_get";
        let mut f = generate_tmp_file(file_name).await;
        serialized.write_all_to::<P2P, File>(&mut f).await.unwrap();

        let mut f = get_tmp_file(file_name).await;
        let m2 = get_message::<P2P, File>(&mut f).await.unwrap();

        assert_eq!(m1, m2);
    }

    #[tokio::test]
    async fn serialize_get_message_client() {
        serialize_get_message_generic::<false>().await;
    }

    #[tokio::test]
    async fn serialize_get_message_p2p() {
        serialize_get_message_generic::<true>().await;
    }

    async fn serialize_get_receipt_generic<const P2P: bool>() {
        let m1 = MessageHeader {
            sender: "Alice".to_string(),
            group: "group".to_string(),
            id: 1024,
            body: MessageBody::Receipt,
        };

        let serialized = m1.serialize();

        let file_name = "serialize_receipt_get";
        let mut f = generate_tmp_file(file_name).await;
        serialized.write_all_to::<P2P, File>(&mut f).await.unwrap();

        let mut f = get_tmp_file(file_name).await;
        let m2 = get_message::<P2P, File>(&mut f).await.unwrap();

        assert_eq!(m1, m2);
    }

    #[tokio::test]
    async fn serialize_get_receipt_client() {
        serialize_get_receipt_generic::<false>().await;
    }

    #[tokio::test]
    async fn serialize_get_receipt_p2p() {
        serialize_get_receipt_generic::<true>().await;
    }

    #[tokio::test]
    async fn serialize_get_handshake() {
        let h1 = Handshake {
            sender: "Alice".to_string(),
            group: "group".to_string(),
        };

        let file_name = "handshake";
        let mut f = generate_tmp_file(file_name).await;
        f.write_all(&h1.serialize()).await.unwrap();

        let mut f = get_tmp_file(file_name).await;
        let h2 = get_handshake(&mut f).await.unwrap();

        assert_eq!(h1, h2);
    }

    #[tokio::test]
    async fn serialize_get_handshake_ref() {
        let h1 = HandshakeRef {
            sender: "Alice",
            group: "group",
        };

        let file_name = "handshake-ref";
        let mut f = generate_tmp_file(file_name).await;
        f.write_all(&h1.serialize()).await.unwrap();

        let mut f = get_tmp_file(file_name).await;
        let h2 = get_handshake(&mut f).await.unwrap();

        assert_eq!(h1.sender, h2.sender);
        assert_eq!(h1.group, h2.group);
    }
}
