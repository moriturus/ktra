use crate::error::Error;
use crate::utils::random_alphanumeric_string;
use argon2::{self, ThreadMode, Variant};

const WINDOWS_NG_FILENAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9",
];

#[tracing::instrument(skip(name))]
pub fn normalized_crate_name(name: &str) -> String {
    name.to_ascii_lowercase()
        .replace('_', "=")
        .replace('-', "=")
}

#[tracing::instrument(skip(name))]
pub fn check_crate_name(name: &str) -> Result<(), Error> {
    let name = name.to_lowercase();
    let length = name.chars().count();
    let first_char_check = name
        .chars()
        .next()
        .map_or(false, |c| c.is_ascii_alphabetic());
    let chars_check = name
        .chars()
        .all(|c| (c.is_alphanumeric() || c == '-' || c == '_') && c.is_ascii());
    let filename_check = !WINDOWS_NG_FILENAMES.contains(&name.as_str());

    if length <= 65 && first_char_check && chars_check && filename_check {
        Ok(())
    } else {
        Err(Error::InvalidCrateName(name))
    }
}

#[tracing::instrument]
pub async fn argon2_config_and_salt<'a>() -> Result<(argon2::Config<'a>, String), Error> {
    let config = argon2::Config {
        variant: Variant::Argon2id,
        lanes: 4,
        thread_mode: ThreadMode::Parallel,
        ..Default::default()
    };
    let salt: String = random_alphanumeric_string(32).await?;
    Ok((config, salt))
}
