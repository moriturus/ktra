use semver::Version;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
struct ErrorMessage {
    errors: Vec<ApiError>,
}

impl ErrorMessage {
    #[tracing::instrument(skip(reasons))]
    fn new(reasons: &[ApiError]) -> ErrorMessage {
        ErrorMessage {
            errors: Vec::from(reasons),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ApiError {
    detail: String,
}

impl ApiError {
    #[tracing::instrument(skip(error))]
    fn from_error(error: &Error) -> ApiError {
        ApiError {
            detail: format!("{}", error),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {}", _0)]
    Io(tokio::io::Error),
    #[error("git error: {}", _0)]
    Git(git2::Error),
    #[error("argon2 error: {}", _0)]
    Argon2(argon2::Error),
    #[error("the given passwords are the same")]
    SamePasswords,
    #[error("the user identified '{}' already exists", _0)]
    UserExists(String),
    #[error("the crate, {}, is overlapped with the another one because ktra considers '_' and '-' are the same", _0)]
    OverlappedCrateName(String),
    #[error("the crate, {} v{}, already exists", _0, _1)]
    VersionExists(String, semver::Version),
    #[error("crate name is not defined")]
    CrateNameNotDefined,
    #[error("no logins are defined")]
    LoginsNotDefined,
    #[error(
        "the specified crate, {} v{}, has already been marked as yanked",
        _0,
        _1
    )]
    AlreadyYanked(String, Version),
    #[error("the specified crate, {} v{}, is not marked as yanked yet", _0, _1)]
    NotYetYanked(String, Version),
    #[error("serialization error: {}", _0)]
    Serialization(serde_json::Error),
    #[error("invalid crate name: {}", _0)]
    InvalidCrateName(String),
    #[error("invalid token: {}", _0)]
    InvalidToken(String),
    #[error("invalid user id: {}", _0)]
    InvalidUser(u32),
    #[error("invalid username: {}", _0)]
    InvalidUsername(String),
    #[error("invalid password")]
    InvalidPassword,
    #[error("one or more invalid login names are detected: {:?}", _0)]
    InvalidLoginNames(Vec<String>),
    #[error("invalid JSON: {}", _0)]
    InvalidJson(serde_json::Error),
    #[error("UTF-8 validation error: {}", _0)]
    InvalidUtf8Bytes(std::string::FromUtf8Error),
    #[error("invalid body length: {}", _0)]
    InvalidBodyLength(usize),
    #[error("crate not found in the database which is named {}", _0)]
    CrateNotFoundInDb(String),
    #[error(
        "crate is found but the specified version is not found in the database: {}",
        _0
    )]
    VersionNotFoundInDb(Version),
    #[cfg(all(feature = "db-sled", not(feature = "db-redis")))]
    #[error("error by database: {}", _0)]
    Db(sled::Error),
    #[cfg(all(feature = "db-redis", not(feature = "db-sled")))]
    #[error("error by database: {}", _0)]
    Db(redis::RedisError),
    #[error("multiple errors: {:?}", _0)]
    Multiple(Vec<Error>),
    #[error("task joinning error: {}", _0)]
    Join(tokio::task::JoinError),
}

impl Error {
    #[tracing::instrument(skip(self))]
    pub fn to_reply(&self) -> (warp::reply::Json, warp::http::StatusCode) {
        let status_code = match self {
            Error::CrateNotFoundInDb(_) | Error::VersionNotFoundInDb(_) => {
                warp::http::StatusCode::NOT_FOUND
            }
            Error::InvalidToken(_) | Error::InvalidUser(_) => warp::http::StatusCode::FORBIDDEN,
            _ => warp::http::StatusCode::OK,
        };
        let json = warp::reply::json(&ErrorMessage::new(&[ApiError::from_error(&self)]));

        (json, status_code)
    }

    #[tracing::instrument(skip(errors))]
    pub fn multiple<I, T>(errors: I) -> Error
    where
        I: IntoIterator<Item = Result<T, Error>>,
        T: std::fmt::Debug,
    {
        Error::Multiple(errors.into_iter().map(Result::unwrap_err).collect())
    }
}

impl warp::reject::Reject for Error {
    // nop
}
