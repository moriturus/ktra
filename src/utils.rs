use crate::db_manager::DbManager;
use crate::error::Error;
use crate::index_manager::IndexManager;
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use warp::{Filter, Rejection, Reply};

#[inline]
pub fn sink<T>(_: T) {
    // nop
}

#[tracing::instrument(skip(crate_name))]
pub fn package_dir_path(crate_name: &str) -> Result<impl AsRef<Path>, Error> {
    let len = crate_name.len();

    match len {
        0 => Err(Error::CrateNameNotDefined),
        1 | 2 => Ok(format!("{}", len)),
        3 => Ok(format!("3/{}", &crate_name[0..1])),
        _ => Ok(format!("{}/{}", &crate_name[..2], &crate_name[2..4])),
    }
}

#[tracing::instrument]
pub fn empty_json_message<T>(_: T) -> impl Reply {
    tracing::info!("just returns an empty message JSON.");
    warp::reply::json(&serde_json::json!({ "warning": null }))
}

#[tracing::instrument]
pub fn ok_json_message<T>(_: T) -> impl Reply {
    tracing::info!("just returns an OK message JSON.");
    warp::reply::json(&serde_json::json!({"ok":true}))
}

#[tracing::instrument(skip(msg))]
pub fn ok_with_msg_json_message(msg: impl Into<String>) -> impl Reply {
    warp::reply::json(&serde_json::json!({
        "ok": true,
        "msg": msg.into()
    }))
}

#[tracing::instrument(skip(db_manager))]
pub fn with_db_manager(
    db_manager: Arc<Mutex<DbManager>>,
) -> impl Filter<Extract = (Arc<Mutex<DbManager>>,), Error = Infallible> + Clone {
    warp::any().map(move || db_manager.clone())
}

#[tracing::instrument(skip(index_manager))]
pub fn with_index_manager(
    index_manager: Arc<Mutex<IndexManager>>,
) -> impl Filter<Extract = (Arc<Mutex<IndexManager>>,), Error = Infallible> + Clone {
    warp::any().map(move || index_manager.clone())
}

#[tracing::instrument]
pub fn authorization_header() -> impl Filter<Extract = (String,), Error = Rejection> + Copy {
    warp::header::<String>("Authorization")
}

#[cfg(test)]
mod tests {
    use super::package_dir_path;

    #[test]
    fn test_package_dir_path_a() -> anyhow::Result<()> {
        let dir = package_dir_path("a")?;
        assert_eq!(dir.as_ref().to_str().unwrap(), "1");

        Ok(())
    }

    #[test]
    fn test_package_dir_path_ab() -> anyhow::Result<()> {
        let dir = package_dir_path("ab")?;
        assert_eq!(dir.as_ref().to_str().unwrap(), "2");

        Ok(())
    }

    #[test]
    fn test_package_dir_path_abc() -> anyhow::Result<()> {
        let dir = package_dir_path("abc")?;
        assert_eq!(dir.as_ref().to_str().unwrap(), "3/a");

        Ok(())
    }

    #[test]
    fn test_package_dir_path_abcd() -> anyhow::Result<()> {
        let dir = package_dir_path("abcd")?;
        assert_eq!(dir.as_ref().to_str().unwrap(), "ab/cd");

        Ok(())
    }
}
