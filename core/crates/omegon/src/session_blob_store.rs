use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const MAX_SESSION_BLOB_BYTES: u64 = 16 * 1024 * 1024;
const METADATA_VERSION: u16 = 1;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionBlobError {
    #[error("session blob I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("session blob JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session blob is invalid: {0}")]
    Invalid(String),
}

pub(crate) type Result<T> = std::result::Result<T, SessionBlobError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectionClass {
    Default,
    RestrictedContinuity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DigestAlgorithm {
    Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StorageClass {
    SessionBlobV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ContentRef {
    digest_algorithm: DigestAlgorithm,
    digest: String,
    media_type: String,
    byte_length: u64,
    storage_class: StorageClass,
    projection_class: ProjectionClass,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentRefWire {
    digest_algorithm: DigestAlgorithm,
    digest: String,
    media_type: String,
    byte_length: u64,
    storage_class: StorageClass,
    projection_class: ProjectionClass,
}

impl<'de> Deserialize<'de> for ContentRef {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ContentRefWire::deserialize(deserializer)?;
        Self::from_wire(wire).map_err(serde::de::Error::custom)
    }
}

impl ContentRef {
    fn from_wire(wire: ContentRefWire) -> Result<Self> {
        validate_digest(&wire.digest)?;
        validate_media_type(&wire.media_type)?;
        if wire.byte_length > MAX_SESSION_BLOB_BYTES {
            return Err(SessionBlobError::Invalid(
                "content reference exceeds 16 MiB".into(),
            ));
        }
        Ok(Self {
            digest_algorithm: wire.digest_algorithm,
            digest: wire.digest,
            media_type: wire.media_type,
            byte_length: wire.byte_length,
            storage_class: wire.storage_class,
            projection_class: wire.projection_class,
        })
    }

    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn media_type(&self) -> &str {
        &self.media_type
    }

    pub(crate) fn byte_length(&self) -> u64 {
        self.byte_length
    }

    pub(crate) fn projection_class(&self) -> ProjectionClass {
        self.projection_class
    }

    pub(crate) fn storage_reference(&self) -> SessionBlobStorageRef {
        SessionBlobStorageRef {
            digest: self.digest.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionBlobStorageRef {
    digest: String,
}

impl SessionBlobStorageRef {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let mut parts = value.split('/');
        if parts.next() != Some("sha256") {
            return Err(SessionBlobError::Invalid(
                "storage reference must use the sha256 namespace".into(),
            ));
        }
        let digest = parts
            .next()
            .ok_or_else(|| SessionBlobError::Invalid("storage reference has no digest".into()))?;
        if parts.next().is_some() {
            return Err(SessionBlobError::Invalid(
                "storage reference must contain exactly two components".into(),
            ));
        }
        validate_digest(digest)?;
        Ok(Self {
            digest: digest.into(),
        })
    }

    pub(crate) fn as_relative_path(&self) -> PathBuf {
        PathBuf::from("sha256").join(&self.digest)
    }
}

impl std::fmt::Display for SessionBlobStorageRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "sha256/{}", self.digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlobMetadata {
    metadata_version: u16,
    digest_algorithm: DigestAlgorithm,
    digest: String,
    byte_length: u64,
    media_types: Vec<String>,
    projection_class: ProjectionClass,
}

impl BlobMetadata {
    fn new(content_ref: &ContentRef) -> Self {
        Self {
            metadata_version: METADATA_VERSION,
            digest_algorithm: DigestAlgorithm::Sha256,
            digest: content_ref.digest.clone(),
            byte_length: content_ref.byte_length,
            media_types: vec![content_ref.media_type.clone()],
            projection_class: content_ref.projection_class,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.metadata_version != METADATA_VERSION {
            return Err(SessionBlobError::Invalid(
                "unsupported blob metadata version".into(),
            ));
        }
        validate_digest(&self.digest)?;
        if self.byte_length > MAX_SESSION_BLOB_BYTES {
            return Err(SessionBlobError::Invalid(
                "blob metadata exceeds 16 MiB".into(),
            ));
        }
        if self.media_types.is_empty() {
            return Err(SessionBlobError::Invalid(
                "blob metadata has no admitted media type".into(),
            ));
        }
        let mut previous = None;
        for media_type in &self.media_types {
            validate_media_type(media_type)?;
            if previous.is_some_and(|value| value >= media_type.as_str()) {
                return Err(SessionBlobError::Invalid(
                    "blob metadata media types are not unique and sorted".into(),
                ));
            }
            previous = Some(media_type.as_str());
        }
        Ok(())
    }

    fn validate_ref(&self, content_ref: &ContentRef) -> Result<()> {
        self.validate()?;
        if self.digest != content_ref.digest
            || self.byte_length != content_ref.byte_length
            || self.projection_class != content_ref.projection_class
            || self
                .media_types
                .binary_search(&content_ref.media_type)
                .is_err()
        {
            return Err(SessionBlobError::Invalid(
                "content reference contradicts durable blob metadata".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SessionBlobStore {
    root: PathBuf,
    publication_lock: Arc<Mutex<()>>,
}

impl SessionBlobStore {
    pub(crate) fn at(root: PathBuf) -> Self {
        Self {
            root,
            publication_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn open(&self) -> Result<()> {
        ensure_directory(&self.root)?;
        ensure_directory(&self.digest_dir())?;
        sync_directory(&self.root)?;
        Ok(())
    }

    pub(crate) fn write(
        &self,
        bytes: &[u8],
        media_type: &str,
        projection_class: ProjectionClass,
    ) -> Result<ContentRef> {
        validate_media_type(media_type)?;
        let byte_length = u64::try_from(bytes.len())
            .map_err(|_| SessionBlobError::Invalid("blob length does not fit u64".into()))?;
        if byte_length > MAX_SESSION_BLOB_BYTES {
            return Err(SessionBlobError::Invalid(
                "session blob exceeds 16 MiB".into(),
            ));
        }
        let content_ref = ContentRef {
            digest_algorithm: DigestAlgorithm::Sha256,
            digest: format!("{:x}", Sha256::digest(bytes)),
            media_type: media_type.into(),
            byte_length,
            storage_class: StorageClass::SessionBlobV1,
            projection_class,
        };

        let _guard = self
            .publication_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.open()?;
        let blob_path = self.blob_path(&content_ref.storage_reference())?;
        publish_bytes_no_clobber(&self.digest_dir(), &blob_path, bytes)?;
        verify_file(&blob_path, &content_ref.digest, content_ref.byte_length)?;
        self.publish_metadata(&content_ref)?;
        self.validate_locked(&content_ref, projection_class)?;
        Ok(content_ref)
    }

    pub(crate) fn read(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<Vec<u8>> {
        let _guard = self
            .publication_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.validate_locked(content_ref, required_projection)
    }

    pub(crate) fn validate(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<()> {
        self.read(content_ref, required_projection).map(|_| ())
    }

    /// Fail-fast reader for bounded replay. Publication is atomic, so this reader
    /// need not wait on the writer mutex; an inconsistent snapshot is unavailable.
    pub(crate) fn read_bounded(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
        max_file_bytes: usize,
        check: &mut dyn FnMut(usize) -> Result<()>,
    ) -> Result<Vec<u8>> {
        check(0)?;
        if content_ref.projection_class != required_projection {
            return Err(SessionBlobError::Invalid(
                "content projection is not authorized".into(),
            ));
        }
        validate_digest(&content_ref.digest)?;
        validate_media_type(&content_ref.media_type)?;
        ensure_existing_directory(&self.root)?;
        ensure_existing_directory(&self.digest_dir())?;
        let storage = content_ref.storage_reference();
        let metadata_bytes = read_bounded_file(
            &self.metadata_path(&storage)?,
            max_file_bytes.min(1024 * 1024),
            check,
        )?;
        let metadata: BlobMetadata = serde_json::from_slice(&metadata_bytes)?;
        metadata.validate_ref(content_ref)?;
        check(0)?;
        let bytes = read_bounded_file(
            &self.blob_path(&storage)?,
            max_file_bytes.min(MAX_SESSION_BLOB_BYTES as usize),
            check,
        )?;
        if bytes.len() as u64 != content_ref.byte_length {
            return Err(SessionBlobError::Invalid("content length changed".into()));
        }
        let mut digest = Sha256::new();
        for chunk in bytes.chunks(64 * 1024) {
            check(0)?;
            digest.update(chunk);
        }
        if format!("{:x}", digest.finalize()) != content_ref.digest {
            return Err(SessionBlobError::Invalid("content digest changed".into()));
        }
        check(0)?;
        Ok(bytes)
    }

    fn validate_locked(
        &self,
        content_ref: &ContentRef,
        required_projection: ProjectionClass,
    ) -> Result<Vec<u8>> {
        validate_digest(&content_ref.digest)?;
        validate_media_type(&content_ref.media_type)?;
        if content_ref.byte_length > MAX_SESSION_BLOB_BYTES {
            return Err(SessionBlobError::Invalid(
                "content reference exceeds 16 MiB".into(),
            ));
        }
        if content_ref.projection_class != required_projection {
            return Err(SessionBlobError::Invalid(
                "content reference projection class is not authorized".into(),
            ));
        }
        ensure_existing_directory(&self.root)?;
        ensure_existing_directory(&self.digest_dir())?;
        let storage_ref = content_ref.storage_reference();
        let blob_path = self.blob_path(&storage_ref)?;
        let metadata = self.read_metadata(content_ref)?;
        metadata.validate_ref(content_ref)?;
        read_and_verify_file(&blob_path, &content_ref.digest, content_ref.byte_length)
    }

    fn publish_metadata(&self, content_ref: &ContentRef) -> Result<()> {
        let path = self.metadata_path(&content_ref.storage_reference())?;
        let exists = path_exists_without_following(&path)?;
        let mut metadata = if exists {
            let metadata = read_strict_metadata(&path)?;
            metadata.validate_ref(content_ref).or_else(|error| {
                if metadata.digest == content_ref.digest
                    && metadata.byte_length == content_ref.byte_length
                    && metadata.projection_class == content_ref.projection_class
                    && !metadata.media_types.contains(&content_ref.media_type)
                {
                    Ok(())
                } else {
                    Err(error)
                }
            })?;
            metadata
        } else {
            BlobMetadata::new(content_ref)
        };

        if !exists {
            let encoded = serde_json::to_vec(&metadata)?;
            publish_bytes_no_clobber(&self.digest_dir(), &path, &encoded)?;
            return read_strict_metadata(&path)?.validate_ref(content_ref);
        }
        if metadata.media_types.contains(&content_ref.media_type) {
            return Ok(());
        }
        metadata.media_types.push(content_ref.media_type.clone());
        metadata.media_types.sort();
        metadata.validate()?;
        let encoded = serde_json::to_vec(&metadata)?;
        replace_bytes_atomically(&self.digest_dir(), &path, &encoded)?;
        Ok(())
    }

    fn read_metadata(&self, content_ref: &ContentRef) -> Result<BlobMetadata> {
        let path = self.metadata_path(&content_ref.storage_reference())?;
        let metadata = read_strict_metadata(&path)?;
        metadata.validate()?;
        Ok(metadata)
    }

    fn digest_dir(&self) -> PathBuf {
        self.root.join("sha256")
    }

    fn blob_path(&self, storage_ref: &SessionBlobStorageRef) -> Result<PathBuf> {
        let relative = storage_ref.as_relative_path();
        if relative.parent() != Some(Path::new("sha256")) {
            return Err(SessionBlobError::Invalid(
                "blob storage reference escaped its digest namespace".into(),
            ));
        }
        Ok(self.root.join(relative))
    }

    fn metadata_path(&self, storage_ref: &SessionBlobStorageRef) -> Result<PathBuf> {
        let blob = self.blob_path(storage_ref)?;
        Ok(blob.with_file_name(format!("{}.meta.json", storage_ref.digest)))
    }
}

fn validate_digest(digest: &str) -> Result<()> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SessionBlobError::Invalid(
            "SHA-256 digest must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

fn validate_media_type(media_type: &str) -> Result<()> {
    let Some((kind, subtype)) = media_type.split_once('/') else {
        return Err(SessionBlobError::Invalid(
            "media type must contain one type/subtype separator".into(),
        ));
    };
    if kind.is_empty()
        || subtype.is_empty()
        || subtype.contains('/')
        || !kind.bytes().all(is_media_token_byte)
        || !subtype.bytes().all(is_media_token_byte)
    {
        return Err(SessionBlobError::Invalid(
            "media type must be normalized lowercase ASCII without parameters".into(),
        ));
    }
    Ok(())
}

fn is_media_token_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase()
        || byte.is_ascii_digit()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

fn ensure_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(SessionBlobError::Invalid(format!(
            "blob storage component is not a real directory: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_restricted_directory(path)?;
            sync_parent(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn ensure_existing_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(SessionBlobError::Invalid(format!(
            "blob storage component is not a real directory: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn create_restricted_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_restricted_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)
}

fn create_restricted_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn open_regular_file(path: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(SessionBlobError::Invalid(format!(
            "blob storage entry is not a regular file: {}",
            path.display()
        )));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(SessionBlobError::Invalid(
            "blob storage entry changed type while opening".into(),
        ));
    }
    Ok(file)
}

/// Charge the whole bounded allocation before reading; poll between fixed-size
/// reads. Reject devices/FIFOs and files that change during this snapshot.
pub(crate) fn read_bounded_file(
    path: &Path,
    maximum: usize,
    check: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<Vec<u8>> {
    check(0)?;
    let mut file = open_regular_file(path)?;
    let before = file.metadata()?;
    let length = usize::try_from(before.len())
        .ok()
        .filter(|length| *length <= maximum)
        .ok_or_else(|| SessionBlobError::Invalid("replay file byte limit exceeded".into()))?;
    check(length)?;
    let mut bytes = vec![0; length];
    for chunk in bytes.chunks_mut(64 * 1024) {
        check(0)?;
        file.read_exact(chunk)?;
    }
    check(0)?;
    let after = file.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(SessionBlobError::Invalid(
            "replay file changed during read".into(),
        ));
    }
    Ok(bytes)
}

struct TemporaryFile {
    path: PathBuf,
    file: Option<File>,
}

impl TemporaryFile {
    fn create(parent: &Path) -> Result<Self> {
        for _ in 0..32 {
            let path = parent.join(format!(".tmp-{}", Uuid::new_v4()));
            match create_restricted_file(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(SessionBlobError::Invalid(
            "could not allocate an exclusive blob temporary file".into(),
        ))
    }

    fn write_and_sync(&mut self, bytes: &[u8]) -> Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| SessionBlobError::Invalid("temporary file is closed".into()))?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }

    fn close(&mut self) {
        self.file.take();
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        self.close();
        let _ = fs::remove_file(&self.path);
    }
}

fn publish_bytes_no_clobber(parent: &Path, destination: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary = TemporaryFile::create(parent)?;
    temporary.write_and_sync(bytes)?;
    temporary.close();
    match fs::hard_link(&temporary.path, destination) {
        Ok(()) => {
            sync_directory(parent)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn replace_bytes_atomically(parent: &Path, destination: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary = TemporaryFile::create(parent)?;
    temporary.write_and_sync(bytes)?;
    temporary.close();
    fs::rename(&temporary.path, destination)?;
    sync_directory(parent)?;
    Ok(())
}

fn verify_file(path: &Path, digest: &str, byte_length: u64) -> Result<()> {
    read_and_verify_file(path, digest, byte_length).map(|_| ())
}

fn read_and_verify_file(path: &Path, digest: &str, byte_length: u64) -> Result<Vec<u8>> {
    let file = open_regular_file(path)?;
    let metadata = file.metadata()?;
    if metadata.len() != byte_length || metadata.len() > MAX_SESSION_BLOB_BYTES {
        return Err(SessionBlobError::Invalid(
            "blob byte length does not match its content reference".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(byte_length as usize);
    Read::take(file, MAX_SESSION_BLOB_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != byte_length {
        return Err(SessionBlobError::Invalid(
            "blob was truncated while reading".into(),
        ));
    }
    if format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err(SessionBlobError::Invalid(
            "blob digest does not match its content reference".into(),
        ));
    }
    Ok(bytes)
}

fn read_strict_metadata(path: &Path) -> Result<BlobMetadata> {
    let mut file = open_regular_file(path)?;
    if file.metadata()?.len() > 1024 * 1024 {
        return Err(SessionBlobError::Invalid(
            "blob metadata exceeds 1 MiB".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let metadata = BlobMetadata::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(metadata)
}

fn path_exists_without_following(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_blob_reads_enforce_metadata_budget_and_mid_read_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionBlobStore::at(directory.path().join("blobs"));
        let bytes = vec![b'x'; 200_000];
        let reference = store
            .write(&bytes, "text/plain", ProjectionClass::Default)
            .unwrap();
        let mut charged = 0;
        let mut polls = 0;
        assert_eq!(
            store
                .read_bounded(&reference, ProjectionClass::Default, 300_000, &mut |n| {
                    charged += n;
                    polls += 1;
                    Ok(())
                })
                .unwrap(),
            bytes
        );
        assert!(charged > bytes.len(), "metadata is part of the read budget");
        assert!(polls > 8, "large blobs poll across reads and hashing");
        let mut reading_blob = false;
        let mut subsequent_polls = 0;
        let error = store
            .read_bounded(&reference, ProjectionClass::Default, 300_000, &mut |n| {
                if n == bytes.len() {
                    reading_blob = true;
                }
                if reading_blob && n == 0 {
                    subsequent_polls += 1;
                    if subsequent_polls == 2 {
                        return Err(SessionBlobError::Invalid("controlled cancellation".into()));
                    }
                }
                Ok(())
            })
            .unwrap_err();
        assert!(error.to_string().contains("controlled cancellation"));
        let metadata = store.metadata_path(&reference.storage_reference()).unwrap();
        fs::write(metadata, vec![b' '; 300_001]).unwrap();
        assert!(
            store
                .read_bounded(&reference, ProjectionClass::Default, 300_000, &mut |_| Ok(
                    ()
                ))
                .unwrap_err()
                .to_string()
                .contains("byte limit")
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_blob_file_reader_rejects_symlinks_and_special_files() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("data");
        fs::write(&target, b"data").unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_bounded_file(&link, 100, &mut |_| Ok(())).is_err());
        assert!(read_bounded_file(Path::new("/dev/null"), 100, &mut |_| Ok(())).is_err());
    }
    use std::sync::Barrier;

    fn store(directory: &tempfile::TempDir, name: &str) -> SessionBlobStore {
        let store = SessionBlobStore::at(directory.path().join(name));
        store.open().unwrap();
        store
    }

    fn temp_entries(store: &SessionBlobStore) -> Vec<PathBuf> {
        fs::read_dir(store.digest_dir())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".tmp-"))
            })
            .collect()
    }

    #[test]
    fn content_round_trip_and_wire_shape_are_exact() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory, "session.authority.blobs");
        let content_ref = store
            .write(b"hello", "text/plain", ProjectionClass::Default)
            .unwrap();

        assert_eq!(
            store.read(&content_ref, ProjectionClass::Default).unwrap(),
            b"hello"
        );
        assert_eq!(content_ref.byte_length(), 5);
        assert_eq!(content_ref.media_type(), "text/plain");
        assert_eq!(
            content_ref.storage_reference().to_string(),
            format!("sha256/{}", content_ref.digest())
        );
        let value = serde_json::to_value(&content_ref).unwrap();
        assert_eq!(value["digest_algorithm"], "sha256");
        assert_eq!(value["storage_class"], "session_blob_v1");
        assert_eq!(value["projection_class"], "default");
        assert_eq!(
            serde_json::from_value::<ContentRef>(value).unwrap(),
            content_ref
        );
    }

    #[test]
    fn dedup_is_deterministic_and_admits_multiple_media_types() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory, "blobs");
        let first = store
            .write(b"same", "text/plain", ProjectionClass::Default)
            .unwrap();
        let second = store
            .write(b"same", "text/plain", ProjectionClass::Default)
            .unwrap();
        let json = store
            .write(b"same", "application/json", ProjectionClass::Default)
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.digest(), json.digest());
        assert_eq!(
            store.read(&json, ProjectionClass::Default).unwrap(),
            b"same"
        );
        let metadata = store.read_metadata(&first).unwrap();
        assert_eq!(metadata.media_types, ["application/json", "text/plain"]);
    }

    #[test]
    fn tampering_truncation_and_digest_mismatch_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory, "blobs");
        let reference = store
            .write(b"original", "text/plain", ProjectionClass::Default)
            .unwrap();
        let path = store.blob_path(&reference.storage_reference()).unwrap();
        fs::write(&path, b"altered!").unwrap();
        assert!(store.read(&reference, ProjectionClass::Default).is_err());

        fs::write(&path, b"short").unwrap();
        assert!(store.read(&reference, ProjectionClass::Default).is_err());

        let mut wire = serde_json::to_value(&reference).unwrap();
        wire["digest"] = serde_json::Value::String("0".repeat(64));
        let mismatched: ContentRef = serde_json::from_value(wire).unwrap();
        assert!(store.read(&mismatched, ProjectionClass::Default).is_err());
    }

    #[test]
    fn media_projection_and_closed_wire_validation_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory, "blobs");
        for invalid in [
            "Text/plain",
            "text/plain; charset=utf-8",
            "text",
            "text/plain/extra",
            "text/pla in",
        ] {
            assert!(
                store
                    .write(b"x", invalid, ProjectionClass::Default)
                    .is_err()
            );
        }
        let restricted = store
            .write(
                b"opaque",
                "application/octet-stream",
                ProjectionClass::RestrictedContinuity,
            )
            .unwrap();
        assert!(store.read(&restricted, ProjectionClass::Default).is_err());
        assert_eq!(
            store
                .read(&restricted, ProjectionClass::RestrictedContinuity)
                .unwrap(),
            b"opaque"
        );

        let mut unknown = serde_json::to_value(&restricted).unwrap();
        unknown["path"] = serde_json::json!("/tmp/escape");
        assert!(serde_json::from_value::<ContentRef>(unknown).is_err());
        let mut oversized = serde_json::to_value(&restricted).unwrap();
        oversized["byte_length"] = serde_json::json!(MAX_SESSION_BLOB_BYTES + 1);
        assert!(serde_json::from_value::<ContentRef>(oversized).is_err());
    }

    #[test]
    fn storage_reference_rejects_traversal_absolute_and_cross_session_forms() {
        let digest = "a".repeat(64);
        assert!(SessionBlobStorageRef::parse(&format!("sha256/{digest}")).is_ok());
        for invalid in [
            format!("../other/sha256/{digest}"),
            format!("/sha256/{digest}"),
            format!("sha256/../{digest}"),
            format!("other-session/sha256/{digest}"),
            format!("sha256/{digest}/extra"),
        ] {
            assert!(SessionBlobStorageRef::parse(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn a_reference_cannot_resolve_against_another_session_root() {
        let directory = tempfile::tempdir().unwrap();
        let first = store(&directory, "first.authority.blobs");
        let second = store(&directory, "second.authority.blobs");
        let reference = first
            .write(b"session one", "text/plain", ProjectionClass::Default)
            .unwrap();
        assert!(second.read(&reference, ProjectionClass::Default).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_roots_blobs_and_metadata_are_rejected() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        fs::create_dir(&target).unwrap();
        let linked_root = SessionBlobStore::at(directory.path().join("linked"));
        symlink(&target, &linked_root.root).unwrap();
        assert!(linked_root.open().is_err());

        let store = store(&directory, "blobs");
        let reference = store
            .write(b"safe", "text/plain", ProjectionClass::Default)
            .unwrap();
        let blob_path = store.blob_path(&reference.storage_reference()).unwrap();
        let metadata_path = store.metadata_path(&reference.storage_reference()).unwrap();
        fs::remove_file(&blob_path).unwrap();
        symlink(&metadata_path, &blob_path).unwrap();
        assert!(store.read(&reference, ProjectionClass::Default).is_err());

        fs::remove_file(&blob_path).unwrap();
        fs::write(&blob_path, b"safe").unwrap();
        fs::remove_file(&metadata_path).unwrap();
        symlink(&blob_path, &metadata_path).unwrap();
        assert!(store.read(&reference, ProjectionClass::Default).is_err());
    }

    #[test]
    fn concurrent_same_content_writes_publish_one_valid_blob() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory, "blobs");
        let root = store.root.clone();
        let barrier = Arc::new(Barrier::new(8));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let store = SessionBlobStore::at(root.clone());
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                store
                    .write(b"concurrent", "text/plain", ProjectionClass::Default)
                    .unwrap()
            }));
        }
        let references: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(references.iter().all(|value| value == &references[0]));
        assert_eq!(
            store
                .read(&references[0], ProjectionClass::Default)
                .unwrap(),
            b"concurrent"
        );
        assert!(temp_entries(&store).is_empty());
    }

    #[test]
    fn publication_failures_remove_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory, "blobs");
        let digest = format!("{:x}", Sha256::digest(b"blocked"));
        fs::create_dir(store.digest_dir().join(&digest)).unwrap();

        assert!(
            store
                .write(b"blocked", "text/plain", ProjectionClass::Default)
                .is_err()
        );
        assert!(temp_entries(&store).is_empty());

        fs::remove_dir(store.digest_dir().join(&digest)).unwrap();
        fs::create_dir(store.digest_dir().join(format!("{digest}.meta.json"))).unwrap();
        assert!(
            store
                .write(b"blocked", "text/plain", ProjectionClass::Default)
                .is_err()
        );
        assert!(temp_entries(&store).is_empty());
    }

    #[test]
    fn reopened_store_revalidates_durable_bytes_and_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("blobs");
        let first = SessionBlobStore::at(root.clone());
        first.open().unwrap();
        let reference = first
            .write(b"durable", "application/json", ProjectionClass::Default)
            .unwrap();
        drop(first);

        let reopened = SessionBlobStore::at(root);
        reopened.open().unwrap();
        reopened
            .validate(&reference, ProjectionClass::Default)
            .unwrap();
        assert_eq!(
            reopened.read(&reference, ProjectionClass::Default).unwrap(),
            b"durable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn temporary_and_published_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let store = store(&directory, "blobs");
        let reference = store
            .write(b"private", "text/plain", ProjectionClass::Default)
            .unwrap();
        let blob = store.blob_path(&reference.storage_reference()).unwrap();
        let metadata = store.metadata_path(&reference.storage_reference()).unwrap();
        assert_eq!(
            fs::metadata(blob).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(metadata).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
