//! Where the game finds the original client and the L2J datapack.
//!
//! Each path is taken from, in order:
//! 1. the environment variable (`L2_CLIENT_DIR`, `L2_DATAPACK`);
//! 2. the config file `l2survivor.cfg` in the working directory (or the file
//!    named by `L2_CONFIG`), lines `client = <dir>` and `datapack = <dir>`;
//!    `#` starts a comment;
//! 3. the built-in default below.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DEFAULT_CLIENT_DIR: &str = "G:/visery/l2-client";
const DEFAULT_DATAPACK: &str = "G:/visery/l2-datapack/classic";
const CONFIG_FILE: &str = "l2survivor.cfg";

#[derive(Default)]
struct Config {
    client: Option<PathBuf>,
    datapack: Option<PathBuf>,
}

fn parse(text: &str) -> Config {
    let mut cfg = Config::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches('"');
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "client" => cfg.client = Some(PathBuf::from(value)),
            "datapack" => cfg.datapack = Some(PathBuf::from(value)),
            other => eprintln!("{CONFIG_FILE}: unknown key `{other}`"),
        }
    }
    cfg
}

/// The config file this run reads (it may not exist).
pub fn config_file() -> PathBuf {
    let file = std::env::var_os("L2_CONFIG").map(PathBuf::from).unwrap_or_else(|| CONFIG_FILE.into());
    std::path::absolute(&file).unwrap_or(file)
}

/// Where a path came from, for error messages.
pub fn source_of(env: &str, from_config: bool) -> String {
    if std::env::var_os(env).is_some() {
        format!("the {env} environment variable")
    } else if from_config {
        format!("{}", config_file().display())
    } else {
        format!("the built-in default ({} not found or without this key)", config_file().display())
    }
}

/// Explains how to point the game at a folder that was not found.
pub fn missing_hint(what: &str, key: &str, env: &str, dir: &Path, from_config: bool) -> String {
    format!(
        "{what} not found at {} (taken from {}). Add `{key} = <folder>` to {} or set {env}.",
        dir.display(),
        source_of(env, from_config),
        config_file().display()
    )
}

/// Whether the config file sets the datapack / client folder.
pub fn configured() -> (bool, bool) {
    (config().client.is_some(), config().datapack.is_some())
}

fn config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let file = config_file();
        match std::fs::read_to_string(&file) {
            Ok(text) => parse(&text),
            Err(_) => Config::default(),
        }
    })
}

fn resolve(env: &str, from_config: Option<&Path>, default: &str) -> PathBuf {
    std::env::var_os(env)
        .map(PathBuf::from)
        .or_else(|| from_config.map(Path::to_path_buf))
        .unwrap_or_else(|| default.into())
}

/// Root of the original Lineage II client (read-only).
pub fn client_dir() -> PathBuf {
    resolve("L2_CLIENT_DIR", config().client.as_deref(), DEFAULT_CLIENT_DIR)
}

/// The datapack's data folder.
pub fn datapack_dir() -> PathBuf {
    resolve("L2_DATAPACK", config().datapack.as_deref(), DEFAULT_DATAPACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_comments_and_quotes() {
        let cfg = parse(
            "# paths\nclient = D:/games/l2\ndatapack = \"/data/l2j classic\"  # trailing\n\nbogus line\n",
        );
        assert_eq!(cfg.client, Some(PathBuf::from("D:/games/l2")));
        assert_eq!(cfg.datapack, Some(PathBuf::from("/data/l2j classic")));
    }

    #[test]
    fn empty_values_are_ignored() {
        let cfg = parse("client =\ndatapack = ");
        assert!(cfg.client.is_none() && cfg.datapack.is_none());
    }

    #[test]
    fn config_beats_default() {
        let p = resolve("L2_TEST_UNSET_VARIABLE", Some(Path::new("/x")), "/default");
        assert_eq!(p, PathBuf::from("/x"));
        assert_eq!(resolve("L2_TEST_UNSET_VARIABLE", None, "/default"), PathBuf::from("/default"));
    }
}
