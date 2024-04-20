// Representation of errors encountered while running the state machine.

use crate::messenger::dists::DistParameterError;

/// Errors encountered by the client.
/// Note that I/O errors are Recoverable by default.
#[derive(Debug)]
pub enum MessengerError {
    Recoverable(RecoverableError),
    Fatal(FatalError),
}

/// Errors where it is possible reconnecting could resolve the problem.
#[derive(Debug)]
pub enum RecoverableError {
    /// Recoverable errors from the socks connection.
    Socks(tokio_socks::Error),
    /// Network I/O errors.
    Io(std::io::Error),
}

/// Errors where something is wrong enough we should terminate.
#[derive(Debug)]
pub enum FatalError {
    /// Fatal errors from the socks connection.
    Socks(tokio_socks::Error),
    /// Errors from parsing the conversation files.
    Parameter(DistParameterError),
    /// Error while trying to interpret bytes as a String.
    Utf8Error(std::str::Utf8Error),
    /// A message failed to deserialize.
    MalformedSerialization(Vec<u8>, std::backtrace::Backtrace),
    /// Fatal network I/O errors.
    Io(std::io::Error),
}

impl std::fmt::Display for MessengerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::fmt::Display for FatalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for MessengerError {}
impl std::error::Error for FatalError {}

impl From<mgen::Error> for MessengerError {
    fn from(e: mgen::Error) -> Self {
        match e {
            mgen::Error::Io(e) => Self::Recoverable(RecoverableError::Io(e)),
            mgen::Error::Utf8Error(e) => Self::Fatal(FatalError::Utf8Error(e)),
            mgen::Error::MalformedSerialization(v, b) => {
                Self::Fatal(FatalError::MalformedSerialization(v, b))
            }
        }
    }
}

impl From<DistParameterError> for MessengerError {
    fn from(e: DistParameterError) -> Self {
        Self::Fatal(FatalError::Parameter(e))
    }
}

impl From<DistParameterError> for FatalError {
    fn from(e: DistParameterError) -> Self {
        Self::Parameter(e)
    }
}

impl From<tokio_socks::Error> for MessengerError {
    fn from(e: tokio_socks::Error) -> Self {
        match e {
            tokio_socks::Error::Io(_)
            | tokio_socks::Error::ProxyServerUnreachable
            | tokio_socks::Error::GeneralSocksServerFailure
            | tokio_socks::Error::HostUnreachable
            | tokio_socks::Error::TtlExpired => Self::Recoverable(RecoverableError::Socks(e)),
            _ => Self::Fatal(FatalError::Socks(e)),
        }
    }
}

impl From<std::io::Error> for MessengerError {
    fn from(e: std::io::Error) -> Self {
        Self::Recoverable(RecoverableError::Io(e))
    }
}

impl From<std::io::Error> for FatalError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
