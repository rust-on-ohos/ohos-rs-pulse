use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    pub rust_version: String,
    pub cargo_ohos_version: String,
    pub sdk_release: String,
    pub sdk_version: String,
    pub sdk_api_version: u32,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub distribution: String,
    pub emulator_action: String,
    pub image_repository: String,
    pub image_release: String,
    pub image_asset: String,
    pub image_asset_id: u64,
    pub image_sha256: String,
    pub connect_key: String,
    pub guest_base: String,
    pub execution_timeout_seconds: u64,
    pub transport_timeout_seconds: u64,
}

pub fn load_build(root: &Path) -> Result<BuildConfig> {
    let config: BuildConfig =
        serde_json::from_slice(&fs::read(root.join("config/feasibility.json"))?)?;
    ensure!(
        config.target == "x86_64-unknown-linux-ohos",
        "only the reviewed x86_64 target is supported"
    );
    Ok(config)
}

pub fn load_runtime(root: &Path) -> Result<RuntimeConfig> {
    let config: RuntimeConfig =
        serde_json::from_slice(&fs::read(root.join("config/runtime.json"))?)?;
    ensure!(
        config.guest_base == "/data/local/tmp",
        "unsupported guest base"
    );
    ensure!(
        config.execution_timeout_seconds > 0 && config.execution_timeout_seconds <= 600,
        "invalid execution budget"
    );
    ensure!(
        config.transport_timeout_seconds > 0 && config.transport_timeout_seconds <= 120,
        "invalid transport budget"
    );
    ensure!(
        config.image_sha256.len() == 64
            && config.image_sha256.bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid image digest"
    );
    Ok(config)
}
