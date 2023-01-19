use std::{path::PathBuf, sync::Arc};

use tokio::sync::RwLock;
use warp::{Filter, Rejection, Reply};

use crate::{db_manager::DbManager, index_manager::IndexManager};

pub mod delete;
pub mod get;
pub mod put;

#[tracing::instrument(skip(db_manager, index_manager, dl_dir_path, dl_path))]
pub fn apis(
    db_manager: Arc<RwLock<impl DbManager>>,
    index_manager: Arc<IndexManager>,
    dl_dir_path: Arc<PathBuf>,
    dl_path: Vec<String>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let routes = self::get::apis(db_manager.clone(), dl_dir_path.clone(), dl_path)
        .or(self::delete::apis(
            db_manager.clone(),
            index_manager.clone(),
        ))
        .or(self::put::apis(
            db_manager.clone(),
            index_manager,
            dl_dir_path,
        ));
    routes
}
