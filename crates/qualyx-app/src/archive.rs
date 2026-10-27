//! Optional S3 evidence archive, using the Apache object_store client. Keys include a
//! project digest; reads never accept traversal and native artifacts retain their SHA-256.

use anyhow::{Context, Result, bail};
use object_store::{ObjectStoreExt, aws::AmazonS3Builder, path::Path as ObjectPath};
use qualyx_core::RunManifest;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

/// True only when the operator explicitly selected an S3 bucket.
pub fn configured() -> bool {
    std::env::var("QUALYX_S3_BUCKET").is_ok_and(|s| !s.is_empty())
}

/// Reject platform-dependent separators and traversal before local or remote reads.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('\\')
        || name.contains(':')
        || Path::new(name)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        bail!("invalid artifact name");
    }
    Ok(())
}

fn key(project: &Path, id: &str, name: &str) -> Result<ObjectPath> {
    validate_name(name)?;
    if !qualyx_store::is_valid_run_id(id) {
        bail!("invalid run id");
    }
    let digest: String = Sha256::digest(project.to_string_lossy().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(ObjectPath::from(format!("{digest}/{id}/{name}")))
}

fn client() -> Result<object_store::aws::AmazonS3> {
    // Explicit credentials avoid implicit metadata-service credential lookups.
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(std::env::var("QUALYX_S3_BUCKET")?)
        .with_region(std::env::var("QUALYX_S3_REGION").unwrap_or_else(|_| "us-east-1".into()))
        .with_access_key_id(std::env::var("QUALYX_S3_ACCESS_KEY")?)
        .with_secret_access_key(std::env::var("QUALYX_S3_SECRET_KEY")?);
    if let Ok(endpoint) = std::env::var("QUALYX_S3_ENDPOINT") {
        builder = builder.with_endpoint(endpoint);
    }
    builder = builder.with_allow_http(std::env::var("QUALYX_S3_ALLOW_HTTP").as_deref() == Ok("1"));
    builder.build().context("invalid S3 configuration")
}

/// Archive the manifest, report and declared native artifacts. A configured upload failure
/// fails the durable job instead of silently reporting that evidence was archived.
pub fn upload(manifest: &RunManifest, run_dir: &Path) -> Result<()> {
    if !configured() {
        return Ok(());
    }
    let store = client()?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut names = vec!["manifest.json".to_string(), "report.md".to_string()];
    names.extend(manifest.runners.iter().flat_map(|r| r.artifacts.clone()));
    names.sort();
    names.dedup();
    let root = run_dir.canonicalize()?;
    rt.block_on(async {
        for name in names {
            let location = key(&manifest.project_root, &manifest.run_id, &name)?;
            let path = run_dir.join(&name).canonicalize()?;
            if !path.starts_with(&root) {
                bail!("artifact escapes run directory");
            }
            let bytes = std::fs::read(path)?;
            store
                .put(&location, bytes.into())
                .await
                .context("S3 evidence upload failed")?;
        }
        Ok(())
    })
}

/// Retrieve one archived file. Callers validate manifest membership and native hashes.
pub fn read(project: &Path, id: &str, name: &str) -> Result<Vec<u8>> {
    let location = key(project, id, name)?;
    let store = client()?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async { Ok(store.get(&location).await?.bytes().await?.to_vec()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_names_reject_cross_platform_escapes() {
        for name in [
            "",
            "../secret",
            "/etc/passwd",
            "nested/../../secret",
            r"..\secret",
            "C:secret",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(validate_name("pytest.junit.xml").is_ok());
        assert!(validate_name("nested/report.json").is_ok());
    }

    #[test]
    fn archive_keys_separate_project_and_run_identity() {
        let a = key(Path::new("project-a"), "run-1", "manifest.json").unwrap();
        assert_ne!(
            a,
            key(Path::new("project-b"), "run-1", "manifest.json").unwrap()
        );
        assert_ne!(
            a,
            key(Path::new("project-a"), "run-2", "manifest.json").unwrap()
        );
        assert!(key(Path::new("project-a"), "../run", "manifest.json").is_err());
    }
}
