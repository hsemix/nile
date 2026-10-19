#![allow(dead_code)]
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub fn root() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root");
    match path.to_str().and_then(|value| value.strip_prefix(r"\\?\")) {
        Some(plain) if !plain.starts_with("UNC\\") => PathBuf::from(plain),
        _ => path,
    }
}
pub fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if ["target", ".git", "__pycache__"]
            .iter()
            .any(|name| entry.file_name() == *name)
        {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
pub fn output(command: &mut Command) -> Result<Output> {
    let result = command.output()?;
    if !result.status.success() {
        return Err(format!(
            "{command:?} failed: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(result)
}
pub fn host(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes)?
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        .ok_or_else(|| "rustc did not report its host target".into())
}
pub fn platform(target: &str) -> Result<(&'static str, &'static str, &'static str)> {
    Ok(match target {
        "aarch64-apple-darwin" => ("macos-aarch64", "lib", ".dylib"),
        "x86_64-apple-darwin" => ("macos-x86_64", "lib", ".dylib"),
        "x86_64-unknown-linux-gnu" => ("linux-x86_64-gnu", "lib", ".so"),
        "aarch64-unknown-linux-gnu" => ("linux-aarch64-gnu", "lib", ".so"),
        "x86_64-unknown-linux-musl" => ("linux-x86_64-musl", "lib", ".so"),
        "aarch64-unknown-linux-musl" => ("linux-aarch64-musl", "lib", ".so"),
        "x86_64-pc-windows-msvc" => ("windows-x86_64-msvc", "", ".dll"),
        "aarch64-pc-windows-msvc" => ("windows-aarch64-msvc", "", ".dll"),
        _ => return Err(format!("unsupported native package target: {target}").into()),
    })
}
pub fn package_names(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let metadata: serde_json::Value = serde_json::from_slice(bytes)?;
    metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?
        .iter()
        .map(|package| {
            package["name"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "package has no name".into())
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_output_is_strict_utf8() {
        assert_eq!(
            host(
                "rustc version 名\nhost: aarch64-apple-darwin\n"
                    .as_bytes()
                    .to_vec()
            )
            .unwrap(),
            "aarch64-apple-darwin"
        );
        assert!(host(vec![0xff]).is_err());
        assert!(
            package_names("{\"packages\":[{\"name\":\"native-名\"}]}".as_bytes())
                .unwrap()
                .contains("native-名")
        );
        assert!(package_names(b"{\"packages\":[{\"name\":\"\xff\"}]}").is_err());
    }
    #[test]
    fn target_mapping_rejects_unknown_abis() {
        assert_eq!(
            platform("x86_64-pc-windows-msvc").unwrap(),
            ("windows-x86_64-msvc", "", ".dll")
        );
        assert!(platform("x86_64-pc-windows-gnu").is_err());
    }
}
