use std::{fs::File, path::Path};

#[cfg(unix)]
use omegon_maintenance_contracts::MaintenanceStateV1;
use omegon_maintenance_contracts::{
    AuthorityKey, ContributionAdmissionGuard, ContributionKind, ContributionMutationGuard,
};

const MAX_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;

#[cfg(unix)]
type FileMode = libc::mode_t;
#[cfg(not(unix))]
type FileMode = u32;

pub(crate) struct GuardedContributionDirectory {
    directory: File,
    admission: ContributionAdmissionGuard,
}

pub(crate) struct GuardedContributionMutationDirectory {
    root: File,
    components: Vec<Vec<u8>>,
    directory: File,
    directory_identity: omegon_maintenance_contracts::PathIdentityV1,
    _mutation: ContributionMutationGuard,
}

pub(crate) struct ContributionSnapshot {
    path: std::path::PathBuf,
    source_identity: omegon_maintenance_contracts::PathIdentityV1,
}

pub(crate) fn is_internal_contribution_entry(raw_name: &[u8]) -> bool {
    let Some(stem) = raw_name.strip_prefix(b".").and_then(|name| {
        name.strip_suffix(b".tmp")
            .or_else(|| name.strip_suffix(b".old"))
    }) else {
        return false;
    };
    std::str::from_utf8(stem)
        .ok()
        .is_some_and(|stem| uuid::Uuid::parse_str(stem).is_ok())
}

impl ContributionSnapshot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn source_identity(&self) -> &omegon_maintenance_contracts::PathIdentityV1 {
        &self.source_identity
    }
}

impl Drop for ContributionSnapshot {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            tracing::warn!(path = %self.path.display(), error = %error, "could not remove contribution snapshot");
        }
    }
}

impl GuardedContributionMutationDirectory {
    #[cfg(unix)]
    pub(crate) fn open_existing(
        root_path: &Path,
        components: &[&[u8]],
        home_path: &Path,
        kind: ContributionKind,
        scope: &str,
    ) -> anyhow::Result<Option<Self>> {
        let root = omegon_maintenance_contracts::open_secure_root(root_path)?;
        let Some(directory) = open_relative_directory(&root, components)? else {
            return Ok(None);
        };
        Self::finish_open(root, directory, components, home_path, kind, scope).map(Some)
    }

    #[cfg(not(unix))]
    pub(crate) fn open_existing(
        _root_path: &Path,
        _components: &[&[u8]],
        _home_path: &Path,
        _kind: ContributionKind,
        _scope: &str,
    ) -> anyhow::Result<Option<Self>> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn open_or_create(
        root_path: &Path,
        components: &[&[u8]],
        home_path: &Path,
        kind: ContributionKind,
        scope: &str,
    ) -> anyhow::Result<Self> {
        ensure_home_exists(home_path)?;
        let root = omegon_maintenance_contracts::open_secure_root(root_path)?;
        let directory = open_or_create_relative_directory(&root, components)?;
        Self::finish_open(root, directory, components, home_path, kind, scope)
    }

    #[cfg(unix)]
    fn finish_open(
        root: File,
        directory: File,
        components: &[&[u8]],
        home_path: &Path,
        kind: ContributionKind,
        scope: &str,
    ) -> anyhow::Result<Self> {
        let parent_identity = omegon_maintenance_contracts::path_identity(&directory)?;
        let directory_identity = parent_identity.clone();
        ensure_home_exists(home_path)?;
        let home = omegon_maintenance_contracts::open_secure_root(home_path)?;
        let home_identity = omegon_maintenance_contracts::path_identity(&home)?;
        let state = MaintenanceStateV1::bootstrap(
            &home,
            home_identity,
            &uuid::Uuid::new_v4().to_string(),
            false,
        )?;
        let mutation = state.lock_contribution_scope_mutation(
            kind,
            scope,
            &parent_identity,
            &uuid::Uuid::new_v4().to_string(),
            false,
        )?;
        Ok(Self {
            root,
            components: components
                .iter()
                .map(|component| component.to_vec())
                .collect(),
            directory,
            directory_identity,
            _mutation: mutation,
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn open_or_create(
        _root_path: &Path,
        _components: &[&[u8]],
        _home_path: &Path,
        _kind: ContributionKind,
        _scope: &str,
    ) -> anyhow::Result<Self> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn write_single_file_directory(
        &self,
        raw_name: &[u8],
        file_name: &[u8],
        bytes: &[u8],
        overwrite: bool,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        omegon_maintenance_contracts::validate_child_name(file_name)?;
        self.stage_and_replace(raw_name, overwrite, |staging| {
            replace_file_at(staging, file_name, bytes, 0o600)
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn write_single_file_directory(
        &self,
        _raw_name: &[u8],
        _file_name: &[u8],
        _bytes: &[u8],
        _overwrite: bool,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn write_files_directory(
        &self,
        raw_name: &[u8],
        files: &[(&[u8], &[u8], FileMode)],
        overwrite: bool,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        self.stage_and_replace(raw_name, overwrite, |staging| {
            for (name, bytes, mode) in files {
                omegon_maintenance_contracts::validate_child_name(name)?;
                replace_file_at(staging, name, bytes, *mode)?;
            }
            Ok(())
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn write_files_directory(
        &self,
        _raw_name: &[u8],
        _files: &[(&[u8], &[u8], FileMode)],
        _overwrite: bool,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn import_directory(
        &self,
        raw_name: &[u8],
        source: &File,
        overwrite: bool,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        let mut entries = 0_usize;
        let mut bytes = 0_u64;
        self.stage_and_replace(raw_name, overwrite, |staging| {
            copy_source_tree(source, staging, 0, &mut entries, &mut bytes, true)
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn import_directory(
        &self,
        _raw_name: &[u8],
        _source: &File,
        _overwrite: bool,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn import_extension_directory(
        &self,
        raw_name: &[u8],
        source: &File,
        binary_path: Option<&Path>,
        expected_manifest: &[u8],
        overwrite: bool,
    ) -> anyhow::Result<()> {
        self.import_extension_directory_with_state(
            raw_name,
            source,
            binary_path,
            expected_manifest,
            overwrite,
            None,
            None,
            None,
            None,
        )
    }

    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn import_extension_directory_with_state(
        &self,
        raw_name: &[u8],
        source: &File,
        binary_path: Option<&Path>,
        expected_manifest: &[u8],
        overwrite: bool,
        expected_existing: Option<&omegon_maintenance_contracts::PathIdentityV1>,
        install_source: Option<&[u8]>,
        config: Option<&[u8]>,
        state: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        let binary = binary_path.map(relative_path_components).transpose()?;
        let mut entries = 0_usize;
        let mut bytes = 0_u64;
        self.stage_and_replace_expected(raw_name, overwrite, expected_existing, |staging| {
            copy_extension_source_tree(
                source,
                staging,
                &mut Vec::new(),
                binary.as_deref(),
                &mut entries,
                &mut bytes,
            )?;
            let copied_manifest = read_file_at(staging, b"manifest.toml", 1024 * 1024)?
                .ok_or_else(|| anyhow::anyhow!("extension manifest disappeared during import"))?;
            if copied_manifest != expected_manifest {
                anyhow::bail!("extension manifest changed during import");
            }
            if let Some(config) = config {
                replace_file_at(staging, b"config.toml", config, 0o600)?;
            }
            if install_source.is_some() || state.is_some() {
                let (internal, _) = open_or_create_child_directory(staging, b".omegon")?;
                if let Some(install_source) = install_source {
                    replace_file_at(&internal, b"install-source.toml", install_source, 0o600)?;
                }
                if let Some(state) = state {
                    replace_file_at(&internal, b"state.toml", state, 0o600)?;
                }
                internal.sync_all()?;
            }
            Ok(())
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn import_extension_directory(
        &self,
        _raw_name: &[u8],
        _source: &File,
        _binary_path: Option<&Path>,
        _expected_manifest: &[u8],
        _overwrite: bool,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(not(unix))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn import_extension_directory_with_state(
        &self,
        _raw_name: &[u8],
        _source: &File,
        _binary_path: Option<&Path>,
        _expected_manifest: &[u8],
        _overwrite: bool,
        _expected_existing: Option<&omegon_maintenance_contracts::PathIdentityV1>,
        _install_source: Option<&[u8]>,
        _config: Option<&[u8]>,
        _state: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn replace_from_snapshot(
        &self,
        raw_name: &[u8],
        source: &File,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        let mut entries = 0_usize;
        let mut bytes = 0_u64;
        self.stage_and_replace(raw_name, true, |staging| {
            copy_source_tree(source, staging, 0, &mut entries, &mut bytes, false)
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn replace_from_snapshot(
        &self,
        _raw_name: &[u8],
        _source: &File,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn remove_directory(&self, raw_name: &[u8]) -> anyhow::Result<bool> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        self.validate_binding()?;
        let Some(existing) = open_child_directory(&self.directory, raw_name)? else {
            return Ok(false);
        };
        drop(existing);
        let backup_name = format!(".{}.old", uuid::Uuid::new_v4()).into_bytes();
        rename_at(&self.directory, raw_name, &backup_name)?;
        self.directory.sync_all()?;
        self.validate_binding()?;
        if let Err(error) = remove_tree_at(&self.directory, &backup_name) {
            tracing::warn!(error = %error, "removed contribution but could not clean detached tree");
        }
        Ok(true)
    }

    #[cfg(not(unix))]
    pub(crate) fn remove_directory(&self, _raw_name: &[u8]) -> anyhow::Result<bool> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn remove_entry(&self, raw_name: &[u8]) -> anyhow::Result<bool> {
        use std::ffi::CString;

        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        self.validate_binding()?;
        let mode = match entry_mode_at(&self.directory, raw_name) {
            Ok(mode) => mode,
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        if mode & libc::S_IFMT == libc::S_IFDIR {
            return self.remove_directory(raw_name);
        }
        if mode & libc::S_IFMT != libc::S_IFLNK {
            anyhow::bail!("contribution entry is neither a directory nor a symlink");
        }
        let raw_name = CString::new(raw_name)?;
        // SAFETY: the validated name is confined to the held directory descriptor.
        if unsafe {
            libc::unlinkat(
                std::os::fd::AsRawFd::as_raw_fd(&self.directory),
                raw_name.as_ptr(),
                0,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        self.directory.sync_all()?;
        self.validate_binding()?;
        Ok(true)
    }

    #[cfg(not(unix))]
    pub(crate) fn remove_entry(&self, _raw_name: &[u8]) -> anyhow::Result<bool> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn open_directory(&self, raw_name: &[u8]) -> anyhow::Result<Option<File>> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        self.validate_binding()?;
        open_child_directory(&self.directory, raw_name)
    }

    #[cfg(not(unix))]
    pub(crate) fn open_directory(&self, _raw_name: &[u8]) -> anyhow::Result<Option<File>> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn entry_names(&self, limit: usize) -> anyhow::Result<Vec<Vec<u8>>> {
        self.validate_binding()?;
        read_directory_names(&self.directory, limit)
    }

    #[cfg(not(unix))]
    pub(crate) fn entry_names(&self, _limit: usize) -> anyhow::Result<Vec<Vec<u8>>> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn write_file(
        &self,
        raw_name: &[u8],
        bytes: &[u8],
        overwrite: bool,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        self.validate_binding()?;
        if overwrite {
            let _ = entry_exists_at(&self.directory, raw_name)?;
        }
        write_file_at(&self.directory, raw_name, bytes, 0o600, overwrite)?;
        self.validate_binding()
    }

    #[cfg(not(unix))]
    pub(crate) fn write_file(
        &self,
        _raw_name: &[u8],
        _bytes: &[u8],
        _overwrite: bool,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn remove_file(&self, raw_name: &[u8]) -> anyhow::Result<bool> {
        use std::ffi::CString;

        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        self.validate_binding()?;
        if !entry_exists_at(&self.directory, raw_name)? {
            return Ok(false);
        }
        let raw_name = CString::new(raw_name)?;
        // SAFETY: the directory/name remain valid and unlinkat retains no pointer.
        if unsafe {
            libc::unlinkat(
                std::os::fd::AsRawFd::as_raw_fd(&self.directory),
                raw_name.as_ptr(),
                0,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        self.directory.sync_all()?;
        self.validate_binding()?;
        Ok(true)
    }

    #[cfg(not(unix))]
    pub(crate) fn remove_file(&self, _raw_name: &[u8]) -> anyhow::Result<bool> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn write_file_in_directory(
        &self,
        raw_name: &[u8],
        child_name: &[u8],
        file_name: &[u8],
        bytes: &[u8],
        expected_identity: &omegon_maintenance_contracts::PathIdentityV1,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        omegon_maintenance_contracts::validate_child_name(child_name)?;
        omegon_maintenance_contracts::validate_child_name(file_name)?;
        self.validate_binding()?;
        let directory = open_child_directory(&self.directory, raw_name)?
            .ok_or_else(|| anyhow::anyhow!("contribution directory disappeared during mutation"))?;
        if &omegon_maintenance_contracts::path_identity(&directory)? != expected_identity {
            anyhow::bail!("contribution identity changed before nested file mutation");
        }
        let (child, _) = open_or_create_child_directory(&directory, child_name)?;
        replace_file_at(&child, file_name, bytes, 0o600)?;
        child.sync_all()?;
        directory.sync_all()?;
        self.validate_binding()?;
        let current = open_child_directory(&self.directory, raw_name)?
            .ok_or_else(|| anyhow::anyhow!("contribution directory disappeared during mutation"))?;
        if &omegon_maintenance_contracts::path_identity(&current)? != expected_identity {
            anyhow::bail!("contribution identity changed during nested file mutation");
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn write_file_in_directory(
        &self,
        _raw_name: &[u8],
        _child_name: &[u8],
        _file_name: &[u8],
        _bytes: &[u8],
        _expected_identity: &omegon_maintenance_contracts::PathIdentityV1,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn read_file_in_directory(
        &self,
        raw_name: &[u8],
        child_name: &[u8],
        file_name: &[u8],
        limit: usize,
    ) -> anyhow::Result<(
        omegon_maintenance_contracts::PathIdentityV1,
        Option<Vec<u8>>,
    )> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        omegon_maintenance_contracts::validate_child_name(child_name)?;
        omegon_maintenance_contracts::validate_child_name(file_name)?;
        self.validate_binding()?;
        let directory = open_child_directory(&self.directory, raw_name)?
            .ok_or_else(|| anyhow::anyhow!("contribution directory not found"))?;
        let identity = omegon_maintenance_contracts::path_identity(&directory)?;
        let Some(child) = open_child_directory(&directory, child_name)? else {
            return Ok((identity, None));
        };
        let bytes = read_file_at(&child, file_name, limit)?;
        self.validate_binding()?;
        Ok((identity, bytes))
    }

    #[cfg(not(unix))]
    pub(crate) fn read_file_in_directory(
        &self,
        _raw_name: &[u8],
        _child_name: &[u8],
        _file_name: &[u8],
        _limit: usize,
    ) -> anyhow::Result<(
        omegon_maintenance_contracts::PathIdentityV1,
        Option<Vec<u8>>,
    )> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    pub(crate) fn write_file_in_existing_directory(
        &self,
        raw_name: &[u8],
        file_name: &[u8],
        bytes: &[u8],
        expected_identity: &omegon_maintenance_contracts::PathIdentityV1,
    ) -> anyhow::Result<()> {
        omegon_maintenance_contracts::validate_child_name(raw_name)?;
        omegon_maintenance_contracts::validate_child_name(file_name)?;
        self.validate_binding()?;
        let directory = open_child_directory(&self.directory, raw_name)?
            .ok_or_else(|| anyhow::anyhow!("contribution directory disappeared during mutation"))?;
        if &omegon_maintenance_contracts::path_identity(&directory)? != expected_identity {
            anyhow::bail!("contribution identity changed before file mutation");
        }
        replace_file_at(&directory, file_name, bytes, 0o600)?;
        directory.sync_all()?;
        self.validate_binding()?;
        let current = open_child_directory(&self.directory, raw_name)?
            .ok_or_else(|| anyhow::anyhow!("contribution directory disappeared during mutation"))?;
        if &omegon_maintenance_contracts::path_identity(&current)? != expected_identity {
            anyhow::bail!("contribution identity changed during file mutation");
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub(crate) fn write_file_in_existing_directory(
        &self,
        _raw_name: &[u8],
        _file_name: &[u8],
        _bytes: &[u8],
        _expected_identity: &omegon_maintenance_contracts::PathIdentityV1,
    ) -> anyhow::Result<()> {
        anyhow::bail!("guarded contribution mutation requires Unix")
    }

    #[cfg(unix)]
    fn stage_and_replace(
        &self,
        raw_name: &[u8],
        overwrite: bool,
        populate: impl FnOnce(&File) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.stage_and_replace_expected(raw_name, overwrite, None, populate)
    }

    #[cfg(unix)]
    fn stage_and_replace_expected(
        &self,
        raw_name: &[u8],
        overwrite: bool,
        expected_existing: Option<&omegon_maintenance_contracts::PathIdentityV1>,
        populate: impl FnOnce(&File) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.validate_binding()?;
        let existing = open_child_directory(&self.directory, raw_name)?;
        if let Some(expected) = expected_existing {
            let current = existing
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("contribution disappeared before replacement"))?;
            if &omegon_maintenance_contracts::path_identity(current)? != expected {
                anyhow::bail!("contribution identity changed before replacement");
            }
        }
        if existing.is_some() && !overwrite {
            anyhow::bail!(
                "contribution '{}' already exists",
                String::from_utf8_lossy(raw_name)
            );
        }
        drop(existing);
        let staging_name = format!(".{}.tmp", uuid::Uuid::new_v4()).into_bytes();
        let (staging, created) = open_or_create_child_directory(&self.directory, &staging_name)?;
        if !created {
            anyhow::bail!("contribution staging directory already exists");
        }
        if let Err(error) = populate(&staging) {
            let _ = remove_tree_at(&self.directory, &staging_name);
            return Err(error);
        }
        let current = open_child_directory(&self.directory, raw_name)?;
        if let Some(expected) = expected_existing {
            let current = current
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("contribution disappeared during replacement"))?;
            if &omegon_maintenance_contracts::path_identity(current)? != expected {
                let _ = remove_tree_at(&self.directory, &staging_name);
                anyhow::bail!("contribution identity changed during replacement");
            }
        }
        let replaced = current.is_some();
        drop(current);
        let commit = if replaced {
            exchange_at(&self.directory, &staging_name, raw_name)
        } else {
            rename_at(&self.directory, &staging_name, raw_name)
        };
        if let Err(error) = commit {
            let _ = remove_tree_at(&self.directory, &staging_name);
            return Err(error);
        }
        if let Err(error) = self.directory.sync_all() {
            anyhow::bail!("contribution was replaced but parent durability is uncertain: {error}");
        }
        self.validate_binding()?;
        if replaced
            && open_child_directory(&self.directory, &staging_name)?.is_some()
            && let Err(error) = remove_tree_at(&self.directory, &staging_name)
        {
            tracing::warn!(error = %error, "committed contribution but could not remove prior staging tree");
        }
        Ok(())
    }

    #[cfg(unix)]
    fn validate_binding(&self) -> anyhow::Result<()> {
        let components = self
            .components
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>();
        let current = open_relative_directory(&self.root, &components)?
            .ok_or_else(|| anyhow::anyhow!("contribution root disappeared during mutation"))?;
        if omegon_maintenance_contracts::path_identity(&current)? != self.directory_identity {
            anyhow::bail!("contribution root identity changed during mutation");
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn snapshot_contribution_directory(
    source: &File,
) -> anyhow::Result<ContributionSnapshot> {
    use std::os::unix::fs::DirBuilderExt;

    let path = std::env::temp_dir().join(format!("omegon-plugin-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new().mode(0o700).create(&path)?;
    let snapshot = ContributionSnapshot {
        path,
        source_identity: omegon_maintenance_contracts::path_identity(source)?,
    };
    let destination = File::open(snapshot.path())?;
    let mut entries = 0_usize;
    let mut bytes = 0_u64;
    copy_source_tree(source, &destination, 0, &mut entries, &mut bytes, false)?;
    Ok(snapshot)
}

#[cfg(not(unix))]
pub(crate) fn snapshot_contribution_directory(
    _source: &File,
) -> anyhow::Result<ContributionSnapshot> {
    anyhow::bail!("guarded contribution snapshots require Unix")
}

impl GuardedContributionDirectory {
    #[cfg(unix)]
    pub(crate) fn open(
        root_path: &Path,
        components: &[&[u8]],
        home_path: &Path,
        kind: ContributionKind,
        scope: &str,
    ) -> anyhow::Result<Option<Self>> {
        let root = omegon_maintenance_contracts::open_secure_root(root_path)?;
        let Some(directory) = open_relative_directory(&root, components)? else {
            return Ok(None);
        };
        let parent_identity = omegon_maintenance_contracts::path_identity(&directory)?;
        ensure_home_exists(home_path)?;
        let home = omegon_maintenance_contracts::open_secure_root(home_path)?;
        let home_identity = omegon_maintenance_contracts::path_identity(&home)?;
        let state = MaintenanceStateV1::bootstrap(
            &home,
            home_identity,
            &uuid::Uuid::new_v4().to_string(),
            false,
        )?;
        let admission = state.admit_contribution_scope(
            kind,
            scope,
            &parent_identity,
            &uuid::Uuid::new_v4().to_string(),
            false,
        )?;
        Ok(Some(Self {
            directory,
            admission,
        }))
    }

    #[cfg(not(unix))]
    pub(crate) fn open(
        _root_path: &Path,
        _components: &[&[u8]],
        _home_path: &Path,
        _kind: ContributionKind,
        _scope: &str,
    ) -> anyhow::Result<Option<Self>> {
        anyhow::bail!("guarded contribution loading requires Unix")
    }

    pub(crate) fn scope_key(&self) -> AuthorityKey {
        self.admission.scope_key
    }

    pub(crate) fn allows(&self, raw_name: &[u8]) -> anyhow::Result<bool> {
        self.admission.allows(raw_name).map_err(Into::into)
    }

    pub(crate) fn entry_names(&self, limit: usize) -> anyhow::Result<Vec<Vec<u8>>> {
        read_directory_names(&self.directory, limit)
    }

    pub(crate) fn read_file(
        &self,
        raw_name: &[u8],
        limit: usize,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        read_file_at(&self.directory, raw_name, limit)
    }

    #[cfg(unix)]
    pub(crate) fn open_child_directory(&self, raw_name: &[u8]) -> anyhow::Result<Option<File>> {
        let mode = entry_mode_at(&self.directory, raw_name)?;
        if mode & libc::S_IFMT != libc::S_IFDIR {
            return Ok(None);
        }
        open_child_directory(&self.directory, raw_name)
    }

    #[cfg(not(unix))]
    pub(crate) fn open_child_directory(&self, _raw_name: &[u8]) -> anyhow::Result<Option<File>> {
        anyhow::bail!("guarded contribution loading requires Unix")
    }
}

#[cfg(unix)]
pub(crate) fn read_file_at(
    parent: &File,
    name: &[u8],
    limit: usize,
) -> anyhow::Result<Option<Vec<u8>>> {
    use std::{ffi::CString, io::Read, os::fd::FromRawFd};

    omegon_maintenance_contracts::validate_child_name(name)?;
    let name = CString::new(name)?;
    // SAFETY: parent/name are valid for this call; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.into())
        };
    }
    // SAFETY: openat returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    if !file.metadata()?.is_file() {
        anyhow::bail!("contribution entry is not a regular file");
    }
    let before = omegon_maintenance_contracts::file_identity(&file)?;
    if before.size > limit as u64 {
        anyhow::bail!("contribution file exceeds the {limit}-byte limit");
    }
    let mut bytes = Vec::with_capacity(before.size as usize);
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    let after = omegon_maintenance_contracts::file_identity(&file)?;
    if bytes.len() > limit || before != after {
        anyhow::bail!("contribution file exceeded its limit or changed during read");
    }
    Ok(Some(bytes))
}

#[cfg(not(unix))]
pub(crate) fn read_file_at(
    _parent: &File,
    _name: &[u8],
    _limit: usize,
) -> anyhow::Result<Option<Vec<u8>>> {
    anyhow::bail!("guarded contribution loading requires Unix")
}

#[cfg(unix)]
fn ensure_home_exists(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    if !path.exists()
        && let Err(error) = std::fs::DirBuilder::new().mode(0o700).create(path)
        && error.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(error.into());
    }
    Ok(())
}

#[cfg(unix)]
fn open_relative_directory(root: &File, components: &[&[u8]]) -> anyhow::Result<Option<File>> {
    let mut current = root.try_clone()?;
    for component in components {
        let Some(next) = open_child_directory(&current, component)? else {
            return Ok(None);
        };
        current = next;
    }
    Ok(Some(current))
}

#[cfg(unix)]
fn open_or_create_relative_directory(root: &File, components: &[&[u8]]) -> anyhow::Result<File> {
    let mut current = root.try_clone()?;
    for component in components {
        current = open_or_create_child_directory(&current, component)?.0;
    }
    Ok(current)
}

#[cfg(unix)]
fn open_or_create_child_directory(parent: &File, name: &[u8]) -> anyhow::Result<(File, bool)> {
    use std::ffi::CString;

    if let Some(directory) = open_child_directory(parent, name)? {
        return Ok((directory, false));
    }
    omegon_maintenance_contracts::validate_child_name(name)?;
    let encoded = CString::new(name)?;
    // SAFETY: parent/name are valid for this call and no pointers are retained.
    let created = if unsafe {
        libc::mkdirat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            encoded.as_ptr(),
            0o700,
        )
    } == 0
    {
        parent.sync_all()?;
        true
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
        false
    };
    let directory = open_child_directory(parent, name)?
        .ok_or_else(|| anyhow::anyhow!("contribution directory disappeared after creation"))?;
    Ok((directory, created))
}

#[cfg(unix)]
fn replace_file_at(
    parent: &File,
    name: &[u8],
    bytes: &[u8],
    mode: libc::mode_t,
) -> anyhow::Result<()> {
    write_file_at(parent, name, bytes, mode, true)
}

#[cfg(unix)]
fn write_file_at(
    parent: &File,
    name: &[u8],
    bytes: &[u8],
    mode: libc::mode_t,
    overwrite: bool,
) -> anyhow::Result<()> {
    use std::{ffi::CString, io::Write, os::fd::FromRawFd};

    omegon_maintenance_contracts::validate_child_name(name)?;
    let name = CString::new(name)?;
    let temporary_name = format!(".{}.tmp", uuid::Uuid::new_v4()).into_bytes();
    let temporary = CString::new(temporary_name.clone())?;
    // SAFETY: parent/temporary are valid; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openat returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        // SAFETY: parent/temporary remain valid and unlinkat retains no pointer.
        unsafe {
            libc::unlinkat(
                std::os::fd::AsRawFd::as_raw_fd(parent),
                temporary.as_ptr(),
                0,
            )
        };
        return Err(error.into());
    }
    let commit = if overwrite {
        // SAFETY: both names are confined to parent and renameat retains no pointer.
        if unsafe {
            libc::renameat(
                std::os::fd::AsRawFd::as_raw_fd(parent),
                temporary.as_ptr(),
                std::os::fd::AsRawFd::as_raw_fd(parent),
                name.as_ptr(),
            )
        } == 0
        {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    } else {
        omegon_maintenance_contracts::rename_entry_no_replace_at(
            parent,
            &temporary_name,
            parent,
            name.as_bytes(),
        )
        .map_err(Into::into)
    };
    if let Err(error) = commit {
        // SAFETY: parent/temporary remain valid and unlinkat retains no pointer.
        unsafe {
            libc::unlinkat(
                std::os::fd::AsRawFd::as_raw_fd(parent),
                temporary.as_ptr(),
                0,
            )
        };
        return Err(error);
    }
    parent.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn relative_path_components(path: &Path) -> anyhow::Result<Vec<Vec<u8>>> {
    use std::os::unix::ffi::OsStrExt;

    let components = path
        .components()
        .map(|component| match component {
            std::path::Component::Normal(component) => Ok(component.as_bytes().to_vec()),
            _ => anyhow::bail!("extension binary must be a relative path within its bundle"),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if components.is_empty() {
        anyhow::bail!("extension binary path is empty");
    }
    if components
        .first()
        .is_some_and(|component| matches!(component.as_slice(), b".git" | b".omegon"))
    {
        anyhow::bail!("extension binary cannot be stored in VCS or Omegon state directories");
    }
    Ok(components)
}

#[cfg(unix)]
fn copy_extension_source_tree(
    source: &File,
    destination: &File,
    path: &mut Vec<Vec<u8>>,
    binary: Option<&[Vec<u8>]>,
    entries: &mut usize,
    total_bytes: &mut u64,
) -> anyhow::Result<()> {
    if path.len() > 32 {
        anyhow::bail!("extension bundle exceeds the directory depth limit");
    }
    for raw_name in read_directory_names(source, 10_000)? {
        if path.is_empty()
            && raw_name.starts_with(b".")
            && binary.is_none_or(|binary| binary.first() != Some(&raw_name))
        {
            continue;
        }
        path.push(raw_name.clone());
        let restricted_root = path
            .first()
            .is_some_and(|component| component == b"target" || component.starts_with(b"."));
        let mode = entry_mode_at(source, &raw_name)?;
        let include = if restricted_root {
            binary.is_some_and(|binary| {
                if mode & libc::S_IFMT == libc::S_IFDIR {
                    binary.starts_with(path)
                } else {
                    binary == path.as_slice()
                }
            })
        } else {
            true
        };
        if !include || mode & libc::S_IFMT == libc::S_IFLNK {
            path.pop();
            continue;
        }
        *entries += 1;
        if *entries > 10_000 {
            anyhow::bail!("extension bundle exceeds the entry limit");
        }
        if mode & libc::S_IFMT == libc::S_IFDIR {
            let source_child = open_child_directory(source, &raw_name)?
                .ok_or_else(|| anyhow::anyhow!("extension source directory disappeared"))?;
            let (child, created) = open_or_create_child_directory(destination, &raw_name)?;
            if !created {
                anyhow::bail!("duplicate extension bundle directory entry");
            }
            copy_extension_source_tree(&source_child, &child, path, binary, entries, total_bytes)?;
            child.sync_all()?;
        } else if mode & libc::S_IFMT == libc::S_IFREG {
            let bytes = read_file_at(source, &raw_name, 256 * 1024 * 1024)?
                .ok_or_else(|| anyhow::anyhow!("extension source file disappeared"))?;
            *total_bytes = total_bytes
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| anyhow::anyhow!("extension bundle size overflow"))?;
            if *total_bytes > 512 * 1024 * 1024 {
                anyhow::bail!("extension bundle exceeds the total size limit");
            }
            replace_file_at(
                destination,
                &raw_name,
                &bytes,
                if mode & 0o111 != 0 || binary.is_some_and(|binary| binary == path.as_slice()) {
                    0o700
                } else {
                    0o600
                },
            )?;
        }
        path.pop();
    }
    destination.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn copy_source_tree(
    source: &File,
    destination: &File,
    depth: usize,
    entries: &mut usize,
    total_bytes: &mut u64,
    skip_hidden: bool,
) -> anyhow::Result<()> {
    if depth > 32 {
        anyhow::bail!("skill bundle exceeds the directory depth limit");
    }
    for raw_name in read_directory_names(source, 10_000)? {
        if skip_hidden && raw_name.starts_with(b".") {
            continue;
        }
        *entries += 1;
        if *entries > 10_000 {
            anyhow::bail!("skill bundle exceeds the entry limit");
        }
        let mode = entry_mode_at(source, &raw_name)?;
        if mode & libc::S_IFMT == libc::S_IFLNK {
            continue;
        }
        if mode & libc::S_IFMT == libc::S_IFDIR {
            let source_child = open_child_directory(source, &raw_name)?
                .ok_or_else(|| anyhow::anyhow!("skill source directory disappeared"))?;
            let (child, created) = open_or_create_child_directory(destination, &raw_name)?;
            if !created {
                anyhow::bail!("duplicate skill bundle directory entry");
            }
            copy_source_tree(
                &source_child,
                &child,
                depth + 1,
                entries,
                total_bytes,
                skip_hidden,
            )?;
            child.sync_all()?;
        } else if mode & libc::S_IFMT == libc::S_IFREG {
            let bytes = read_file_at(source, &raw_name, MAX_SNAPSHOT_BYTES)?
                .ok_or_else(|| anyhow::anyhow!("skill source file disappeared"))?;
            *total_bytes = total_bytes
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| anyhow::anyhow!("skill bundle size overflow"))?;
            if *total_bytes > MAX_SNAPSHOT_BYTES as u64 {
                anyhow::bail!("skill bundle exceeds the total size limit");
            }
            let executable = mode & 0o111 != 0;
            replace_file_at(
                destination,
                &raw_name,
                &bytes,
                if executable { 0o700 } else { 0o600 },
            )?;
        }
    }
    destination.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn entry_mode_at(parent: &File, name: &[u8]) -> anyhow::Result<libc::mode_t> {
    use std::ffi::CString;

    let name = CString::new(name)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstatat initializes metadata on success and retains no pointer.
    if unsafe {
        libc::fstatat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fstatat succeeded and initialized metadata.
    Ok(unsafe { metadata.assume_init() }.st_mode)
}

#[cfg(unix)]
fn entry_exists_at(parent: &File, name: &[u8]) -> anyhow::Result<bool> {
    use std::ffi::CString;

    omegon_maintenance_contracts::validate_child_name(name)?;
    let name = CString::new(name)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstatat initializes metadata on success and retains no pointer.
    if unsafe {
        libc::fstatat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        // SAFETY: fstatat succeeded and initialized metadata.
        let metadata = unsafe { metadata.assume_init() };
        if metadata.st_mode & libc::S_IFMT != libc::S_IFREG {
            anyhow::bail!("contribution entry is not a regular file");
        }
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(false)
    } else {
        Err(error.into())
    }
}

#[cfg(unix)]
fn rename_at(parent: &File, source: &[u8], destination: &[u8]) -> anyhow::Result<()> {
    use std::ffi::CString;

    omegon_maintenance_contracts::validate_child_name(source)?;
    omegon_maintenance_contracts::validate_child_name(destination)?;
    let source = CString::new(source)?;
    let destination = CString::new(destination)?;
    // SAFETY: both names are confined to parent and renameat retains no pointer.
    if unsafe {
        libc::renameat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            source.as_ptr(),
            std::os::fd::AsRawFd::as_raw_fd(parent),
            destination.as_ptr(),
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn exchange_at(parent: &File, source: &[u8], destination: &[u8]) -> anyhow::Result<()> {
    use std::ffi::CString;

    let source = CString::new(source)?;
    let destination = CString::new(destination)?;
    // SAFETY: names and the directory descriptor remain valid for this syscall.
    if unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            std::os::fd::AsRawFd::as_raw_fd(parent),
            source.as_ptr(),
            std::os::fd::AsRawFd::as_raw_fd(parent),
            destination.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn exchange_at(parent: &File, source: &[u8], destination: &[u8]) -> anyhow::Result<()> {
    use std::ffi::CString;

    let source = CString::new(source)?;
    let destination = CString::new(destination)?;
    // SAFETY: names and the directory descriptor remain valid for this call.
    if unsafe {
        libc::renameatx_np(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            source.as_ptr(),
            std::os::fd::AsRawFd::as_raw_fd(parent),
            destination.as_ptr(),
            libc::RENAME_SWAP,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(unix)]
fn remove_tree_at(parent: &File, name: &[u8]) -> anyhow::Result<()> {
    use std::ffi::CString;

    let directory = open_child_directory(parent, name)?
        .ok_or_else(|| anyhow::anyhow!("contribution cleanup directory disappeared"))?;
    for child in read_directory_names(&directory, 10_000)? {
        if entry_is_directory_at(&directory, &child)? {
            remove_tree_at(&directory, &child)?;
        } else {
            let child = CString::new(child)?;
            // SAFETY: directory/child are valid and unlinkat retains no pointer.
            if unsafe {
                libc::unlinkat(
                    std::os::fd::AsRawFd::as_raw_fd(&directory),
                    child.as_ptr(),
                    0,
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    directory.sync_all()?;
    let name = CString::new(name)?;
    // SAFETY: parent/name are valid and unlinkat retains no pointer.
    if unsafe {
        libc::unlinkat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::AT_REMOVEDIR,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    parent.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn entry_is_directory_at(parent: &File, name: &[u8]) -> anyhow::Result<bool> {
    use std::ffi::CString;

    omegon_maintenance_contracts::validate_child_name(name)?;
    let name = CString::new(name)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstatat initializes metadata on success and retains no pointer.
    if unsafe {
        libc::fstatat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fstatat succeeded and initialized metadata.
    let metadata = unsafe { metadata.assume_init() };
    Ok(metadata.st_mode & libc::S_IFMT == libc::S_IFDIR)
}

#[cfg(unix)]
pub(crate) fn open_child_directory(parent: &File, name: &[u8]) -> anyhow::Result<Option<File>> {
    use std::{ffi::CString, os::fd::FromRawFd};

    omegon_maintenance_contracts::validate_child_name(name)?;
    let name = CString::new(name)?;
    // SAFETY: parent/name are valid for this call; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.into())
        };
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(Some(unsafe { File::from_raw_fd(descriptor) }))
}

#[cfg(not(unix))]
pub(crate) fn open_child_directory(_parent: &File, _name: &[u8]) -> anyhow::Result<Option<File>> {
    anyhow::bail!("guarded contribution loading requires Unix")
}

#[cfg(unix)]
pub(crate) fn read_directory_names(directory: &File, limit: usize) -> anyhow::Result<Vec<Vec<u8>>> {
    use std::{ffi::CStr, os::fd::AsRawFd};

    // SAFETY: dup returns a new descriptor consumed by fdopendir.
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: duplicate is an owned directory descriptor.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: fdopendir did not consume duplicate on failure.
        unsafe { libc::close(duplicate) };
        return Err(std::io::Error::last_os_error().into());
    }
    let mut entries = Vec::new();
    loop {
        clear_errno();
        // SAFETY: stream remains valid until it is closed below.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let error = current_errno();
            // SAFETY: stream is a live DIR pointer and closed exactly once.
            unsafe { libc::closedir(stream) };
            return if error == 0 {
                Ok(entries)
            } else {
                Err(std::io::Error::from_raw_os_error(error).into())
            };
        }
        // SAFETY: d_name is NUL-terminated for a successful readdir result.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if matches!(name, b"." | b"..") {
            continue;
        }
        entries.push(name.to_vec());
        if entries.len() > limit {
            // SAFETY: stream is a live DIR pointer and closed exactly once.
            unsafe { libc::closedir(stream) };
            anyhow::bail!("contribution directory exceeds {limit} entries");
        }
    }
}

#[cfg(not(unix))]
pub(crate) fn read_directory_names(
    _directory: &File,
    _limit: usize,
) -> anyhow::Result<Vec<Vec<u8>>> {
    anyhow::bail!("guarded contribution loading requires Unix")
}

#[cfg(target_os = "macos")]
fn clear_errno() {
    // SAFETY: __error returns the calling thread's errno pointer.
    unsafe { *libc::__error() = 0 };
}

#[cfg(target_os = "macos")]
fn current_errno() -> i32 {
    // SAFETY: __error returns the calling thread's errno pointer.
    unsafe { *libc::__error() }
}

#[cfg(target_os = "linux")]
fn clear_errno() {
    // SAFETY: __errno_location returns the calling thread's errno pointer.
    unsafe { *libc::__errno_location() = 0 };
}

#[cfg(target_os = "linux")]
fn current_errno() -> i32 {
    // SAFETY: __errno_location returns the calling thread's errno pointer.
    unsafe { *libc::__errno_location() }
}
