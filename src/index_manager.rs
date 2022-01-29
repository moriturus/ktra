use crate::config::IndexConfig;
use crate::error::Error;
use crate::models::Package;
use crate::utils::package_dir_path;
use futures::TryFutureExt;
use git2::{
    self, AnnotatedCommit, Commit, Cred, CredentialType, ObjectType, PushOptions, Reference,
    Repository, Signature,
};
use semver::Version;
use std::io::SeekFrom;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

pub struct IndexManager {
    config: IndexConfig,
    repository: Arc<Mutex<Repository>>,
}

impl IndexManager {
    #[tracing::instrument(skip(config))]
    pub async fn new(config: IndexConfig) -> Result<IndexManager, Error> {
        let repository = tokio::task::block_in_place(|| clone_or_open_repository(&config))
            .map(Mutex::new)
            .map(Arc::new)
            .map_err(Error::Git)?;
        let manager = IndexManager { config, repository };
        Ok(manager)
    }

    #[tracing::instrument(skip(self))]
    pub async fn pull(&self) -> Result<(), Error> {
        let repository = self.repository.lock().await;
        tokio::task::block_in_place(|| {
            let fetch_commit = fetch(&repository, &self.config)?;
            merge(&repository, &self.config, fetch_commit)?;
            repository.checkout_head(None)
        })
        .map_err(Error::Git)
    }

    #[tracing::instrument(skip(self, package))]
    pub async fn add_package(&self, package: Package) -> Result<(), Error> {
        let name = package.name.to_ascii_lowercase();

        let mut index_path = self.config.local_path.clone();
        index_path.push(package_dir_path(&name)?);
        tokio::fs::create_dir_all(&index_path)
            .map_err(Error::Io)
            .await?;

        index_path.push(&name);
        let package_path = index_path;
        let package_json_string = package.to_json_string().map_err(Error::Serialization)?;

        tracing::debug!("try to open or create index file");

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(package_path)
            .await?;

        let mut buf = String::new();
        file.read_to_string(&mut buf).await?;
        let content = buf
            .lines()
            .chain(std::iter::once(package_json_string.as_str()))
            .collect::<Vec<_>>()
            .join("\n");

        file.set_len(0).await?;
        file.seek(SeekFrom::Start(0)).await?;
        file.write_all(content.as_bytes()).await?;
        file.flush().await?;

        let message = format!("Updating crate `{}#{}`", package.name, package.vers);
        let repository = self.repository.lock().await;
        tokio::task::block_in_place(|| {
            add_all(&repository)?;
            commit(&repository, &self.config, message)?;
            push_to_origin(&repository, &self.config)
        })
        .map_err(Error::Git)
    }

    #[tracing::instrument(skip(self, name, version, yanked))]
    async fn change_yanked(
        &self,
        name: impl Into<String>,
        version: Version,
        yanked: bool,
    ) -> Result<(), Error> {
        let name = name.into();
        let mut index_path = self.config.local_path.clone();
        index_path.push(package_dir_path(&name)?);
        index_path.push(&name);
        let package_path = index_path;

        tracing::debug!("try to open index file");

        let version_cloned = version.clone();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(package_path)
            .await?;

        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(Error::Io).await?;
        let (oks, errors): (Vec<_>, Vec<_>) = buf
            .lines()
            .map(|l| serde_json::from_str::<Package>(l).map_err(Error::InvalidJson))
            .partition(Result::is_ok);

        if !errors.is_empty() {
            return Err(Error::multiple(errors));
        }

        let (oks, errors): (Vec<_>, Vec<_>) = oks
            .into_iter()
            .map(Result::unwrap)
            .map(|mut p| {
                if p.vers == version_cloned {
                    p.yanked = yanked;
                }
                p.to_json_string().map_err(Error::InvalidJson)
            })
            .partition(Result::is_ok);

        if !errors.is_empty() {
            return Err(Error::multiple(errors));
        }

        let content = oks
            .into_iter()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join("\n");

        file.set_len(0).await?;
        file.seek(SeekFrom::Start(0)).await?;
        file.write_all(content.as_bytes()).await?;
        file.flush().await?;

        let message = if yanked {
            format!("Yanking crate `{}#{}`", name, version)
        } else {
            format!("Unyanking crate `{}#{}`", name, version)
        };
        let repository = self.repository.lock().await;
        tokio::task::block_in_place(|| {
            add_all(&repository)?;
            commit(&repository, &self.config, message)?;
            push_to_origin(&repository, &self.config)
        })
        .map_err(Error::Git)
    }

    #[tracing::instrument(skip(self, name, version))]
    pub async fn yank(&self, name: impl Into<String>, version: Version) -> Result<(), Error> {
        self.change_yanked(name, version, true).await
    }

    #[tracing::instrument(skip(self, name, version))]
    pub async fn unyank(&self, name: impl Into<String>, version: Version) -> Result<(), Error> {
        self.change_yanked(name, version, false).await
    }
}

#[tracing::instrument(skip(config))]
fn credentials_callback<'a>(
    config: &'a IndexConfig,
) -> impl FnMut(&str, Option<&str>, CredentialType) -> Result<Cred, git2::Error> + 'a {
    move |_url, username, credential_type| {
        if credential_type.contains(CredentialType::USER_PASS_PLAINTEXT) {
            let username = username
                .or_else(|| config.https_username.as_deref())
                .ok_or_else(|| git2::Error::from_str("username not defined"))?;
            let password = config
                .https_password
                .clone()
                .ok_or_else(|| git2::Error::from_str("password not defined"))?;
            Cred::userpass_plaintext(username, &password)
        } else {
            let username = username
                .or_else(|| config.ssh_username.as_deref())
                .ok_or_else(|| git2::Error::from_str("username not defined"))?;
            let pubkey_path = config.ssh_pubkey_path.as_deref();
            let privkey_path = config
                .ssh_privkey_path
                .as_deref()
                .ok_or_else(|| git2::Error::from_str("ssh private key not specified"))?;
            let passphrase = config.ssh_key_passphrase.as_deref();
            Cred::ssh_key(username, pubkey_path, privkey_path, passphrase)
        }
    }
}

#[tracing::instrument(skip(config))]
fn clone_or_open_repository(config: &IndexConfig) -> Result<git2::Repository, git2::Error> {
    let path = config.local_path.as_path();

    if path.exists() {
        tracing::info!("open index repository: {:?}", path);
        git2::Repository::open(path)
    } else {
        tracing::info!("try to clone index repository into {:?}", path);

        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(credentials_callback(config));
        let mut fetch_options = git2::FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);

        let mut builder = git2::build::RepoBuilder::default();
        builder.branch(&config.branch);
        builder.fetch_options(fetch_options);

        builder.clone(&config.remote_url, path)
    }
}

#[tracing::instrument(skip(repository, config))]
fn fetch<'a>(
    repository: &'a Repository,
    config: &IndexConfig,
) -> Result<AnnotatedCommit<'a>, git2::Error> {
    tracing::info!("fetches latest commit from origin/{}", config.branch);

    let mut remote = repository.find_remote("origin")?;

    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(credentials_callback(config));
    let mut fetch_options = git2::FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    fetch_options.download_tags(git2::AutotagOption::All);

    let refspec = format!("refs/heads/{0}:refs/remotes/origin/{}", config.branch);
    remote.fetch(&[refspec], Some(&mut fetch_options), None)?;

    let fetch_head = repository.find_reference("FETCH_HEAD")?;
    repository.reference_to_annotated_commit(&fetch_head)
}

#[tracing::instrument(skip(repository, reference, annotated_commit))]
fn fast_forward(
    repository: &Repository,
    reference: &mut Reference,
    annotated_commit: &AnnotatedCommit,
) -> Result<(), git2::Error> {
    tracing::info!("fast-forward merging");

    let name = reference.name().map_or_else(
        || String::from_utf8_lossy(reference.name_bytes()).to_string(),
        ToOwned::to_owned,
    );
    let message = format!("Fast-Forward: Setting id: {}", annotated_commit.id());
    reference.set_target(annotated_commit.id(), &message)?;
    repository.set_head(&name)?;
    let mut checkout_builder = git2::build::CheckoutBuilder::default();
    checkout_builder.force();
    repository.checkout_head(Some(&mut checkout_builder))
}

#[tracing::instrument(skip(repository, local_commit, remote_commit))]
fn normal_merge(
    repository: &Repository,
    local_commit: &AnnotatedCommit,
    remote_commit: &AnnotatedCommit,
) -> Result<(), git2::Error> {
    tracing::info!("normal merging");

    let local_tree = repository.find_commit(local_commit.id())?.tree()?;
    let remote_tree = repository.find_commit(remote_commit.id())?.tree()?;
    let ancestor = repository
        .find_commit(repository.merge_base(local_commit.id(), remote_commit.id())?)?
        .tree()?;
    let mut index = repository.merge_trees(&ancestor, &local_tree, &remote_tree, None)?;

    if index.has_conflicts() {
        return repository.checkout_index(Some(&mut index), None);
    }

    let oid = index.write_tree_to(&repository)?;
    let result_tree = repository.find_tree(oid)?;

    let message = format!("Merge: {} into {}", remote_commit.id(), local_commit.id());
    let signature = repository.signature()?;
    let local_commit = repository.find_commit(local_commit.id())?;
    let remote_commit = repository.find_commit(remote_commit.id())?;

    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            &message,
            &result_tree,
            &[&local_commit, &remote_commit],
        )
        .map(drop)?;
    repository.checkout_head(None)
}

#[tracing::instrument(skip(repository, config, fetch_commit))]
fn merge(
    repository: &Repository,
    config: &IndexConfig,
    fetch_commit: AnnotatedCommit,
) -> Result<(), git2::Error> {
    tracing::info!("start merging");

    let analysis = repository.merge_analysis(&[&fetch_commit])?;

    if analysis.0.is_fast_forward() {
        let refname = format!("refs/heads/{}", config.branch);
        match repository.find_reference(&refname) {
            Ok(mut reference) => fast_forward(repository, &mut reference, &fetch_commit),
            Err(_) => {
                tracing::info!("failed to fast forward merging");
                repository.reference(
                    &refname,
                    fetch_commit.id(),
                    true,
                    &format!("Setting {} to {}", config.branch, fetch_commit.id()),
                )?;
                repository.set_head(&refname)?;
                let mut checkout_builder = git2::build::CheckoutBuilder::default();
                checkout_builder
                    .allow_conflicts(true)
                    .conflict_style_merge(true)
                    .safe();
                repository.checkout_head(Some(&mut checkout_builder))
            }
        }
    } else if analysis.0.is_normal() {
        let head_commit = repository.reference_to_annotated_commit(&repository.head()?)?;
        normal_merge(repository, &head_commit, &fetch_commit)
    } else {
        tracing::info!("nothing to do");
        Ok(())
    }
}

#[tracing::instrument(skip(repository))]
fn add_all(repository: &Repository) -> Result<(), git2::Error> {
    tracing::info!("add all unstaged files");

    let mut index = repository.index()?;
    index.add_all(std::iter::once("."), git2::IndexAddOption::DEFAULT, None)?;
    index.write()
}

#[tracing::instrument(skip(repository))]
fn find_last_commit(repository: &Repository) -> Result<Commit, git2::Error> {
    let obj = repository.head()?.resolve()?.peel(ObjectType::Commit)?;
    obj.into_commit()
        .map_err(|_| git2::Error::from_str("Couldn't find commit"))
}

#[tracing::instrument(skip(repository, config, message))]
fn commit(
    repository: &Repository,
    config: &IndexConfig,
    message: impl AsRef<str>,
) -> Result<(), git2::Error> {
    tracing::info!("commit changes");

    let mut index = repository.index()?;
    let oid = index.write_tree_to(repository)?;
    let tree = repository.find_tree(oid)?;
    let last_commit = find_last_commit(repository)?;
    let signature = Signature::now(
        &config.name,
        config.email.as_deref().unwrap_or("undefined@example.com"),
    )?;

    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message.as_ref(),
            &tree,
            &[&last_commit],
        )
        .map(drop)?;
    repository.checkout_head(None)
}

#[tracing::instrument(skip(repository, config))]
fn push_to_origin(repository: &Repository, config: &IndexConfig) -> Result<(), git2::Error> {
    tracing::debug!("push commits to origin");
    let mut remote = repository.find_remote("origin")?;

    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(credentials_callback(config));
    let mut push_options = PushOptions::default();
    push_options.remote_callbacks(callbacks);

    let refs = format!("refs/heads/{0}:refs/heads/{}", config.branch);
    remote.push(&[refs], Some(&mut push_options))
}
