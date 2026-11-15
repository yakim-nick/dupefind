//! Find duplicate files by content.
//!
//! Strategy (cheapest check first, so most files are never fully read):
//! 1. group regular files by size,
//! 2. within a size group, group by the SHA-256 of the first [`PARTIAL_BYTES`] bytes,
//! 3. within those, group by the SHA-256 of the whole file.
//!
//! Symlinks are not followed, hard links to the same inode count once, and
//! unreadable files are reported in [`Report::errors`] instead of aborting the scan.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// How many leading bytes the cheap second pass hashes.
pub const PARTIAL_BYTES: usize = 4096;

/// Scan options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Ignore files smaller than this many bytes (empty files are all "equal" and just noise).
    pub min_size: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self { min_size: 1 }
    }
}

/// Files with identical content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateGroup {
    /// Size in bytes of each file in the group.
    pub size: u64,
    /// Hex SHA-256 of the content.
    pub sha256: String,
    /// Paths, sorted.
    pub files: Vec<PathBuf>,
}

impl DuplicateGroup {
    /// Bytes that could be freed by keeping only one copy.
    pub fn wasted_bytes(&self) -> u64 {
        self.size * (self.files.len() as u64 - 1)
    }
}

/// Result of a scan.
#[derive(Debug, Default)]
pub struct Report {
    /// Duplicate groups, largest waste first.
    pub groups: Vec<DuplicateGroup>,
    /// Number of regular files that were considered.
    pub scanned: usize,
    /// Human-readable problems (unreadable files, walk errors).
    pub errors: Vec<String>,
}

impl Report {
    /// Total bytes that could be freed across all groups.
    pub fn wasted_bytes(&self) -> u64 {
        self.groups.iter().map(DuplicateGroup::wasted_bytes).sum()
    }
}

/// Scan `roots` and return the duplicate groups found.
pub fn find_duplicates(roots: &[PathBuf], options: &Options) -> Report {
    let mut report = Report::default();
    let by_size = collect_by_size(roots, options, &mut report);

    let mut groups = Vec::new();
    for (size, paths) in by_size {
        if paths.len() < 2 {
            continue;
        }
        let mut partial: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in paths {
            match hash_file(&path, Some(PARTIAL_BYTES)) {
                Ok(h) => partial.entry(h).or_default().push(path),
                Err(e) => report.errors.push(format!("{}: {e}", path.display())),
            }
        }
        for candidates in partial.into_values().filter(|c| c.len() > 1) {
            let mut full: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for path in candidates {
                match hash_file(&path, None) {
                    Ok(h) => full.entry(h).or_default().push(path),
                    Err(e) => report.errors.push(format!("{}: {e}", path.display())),
                }
            }
            for (sha256, mut files) in full.into_iter().filter(|(_, f)| f.len() > 1) {
                files.sort();
                groups.push(DuplicateGroup {
                    size,
                    sha256,
                    files,
                });
            }
        }
    }

    groups.sort_by(|a, b| {
        b.wasted_bytes()
            .cmp(&a.wasted_bytes())
            .then_with(|| a.files.cmp(&b.files))
    });
    report.groups = groups;
    report.errors.sort();
    report
}

/// Walk the roots and bucket regular files by size.
fn collect_by_size(
    roots: &[PathBuf],
    options: &Options,
    report: &mut Report,
) -> BTreeMap<u64, Vec<PathBuf>> {
    let mut by_size: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    let mut seen_files: HashSet<FileId> = HashSet::new();

    for root in roots {
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    report.errors.push(e.to_string());
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue; // directories, symlinks, sockets...
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    report
                        .errors
                        .push(format!("{}: {e}", entry.path().display()));
                    continue;
                }
            };
            // Same file reached twice (overlapping roots or a hard link): count once.
            if !seen_files.insert(file_id(entry.path(), &meta)) {
                continue;
            }
            report.scanned += 1;
            if meta.len() >= options.min_size {
                by_size
                    .entry(meta.len())
                    .or_default()
                    .push(entry.into_path());
            }
        }
    }
    by_size
}

/// Identity of a file on disk: (device, inode) on Unix, canonical path elsewhere.
#[derive(Debug, PartialEq, Eq, Hash)]
enum FileId {
    #[cfg(unix)]
    Inode(u64, u64),
    #[cfg(not(unix))]
    Path(PathBuf),
}

#[cfg(unix)]
fn file_id(_path: &Path, meta: &fs::Metadata) -> FileId {
    use std::os::unix::fs::MetadataExt;
    FileId::Inode(meta.dev(), meta.ino())
}

#[cfg(not(unix))]
fn file_id(path: &Path, _meta: &fs::Metadata) -> FileId {
    FileId::Path(fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}

/// SHA-256 of a file (hex). With `limit`, only the first `limit` bytes are hashed.
pub fn hash_file(path: &Path, limit: Option<usize>) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut remaining = limit;
    loop {
        let want = match remaining {
            Some(0) => break,
            Some(r) => r.min(buf.len()),
            None => buf.len(),
        };
        let n = file.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(r) = remaining.as_mut() {
            *r -= n;
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn scan(dir: &Path) -> Report {
        find_duplicates(&[dir.to_path_buf()], &Options::default())
    }

    #[test]
    fn finds_identical_files_across_directories() {
        let tmp = tempdir().unwrap();
        let a = write(tmp.path(), "a.txt", b"same content");
        let b = write(tmp.path(), "sub/b.txt", b"same content");
        write(tmp.path(), "c.txt", b"different!!!!");

        let report = scan(tmp.path());
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].files, vec![a, b]);
        assert_eq!(report.groups[0].size, 12);
        assert_eq!(report.wasted_bytes(), 12);
        assert_eq!(report.scanned, 3);
    }

    #[test]
    fn same_size_but_different_content_is_not_a_duplicate() {
        let tmp = tempdir().unwrap();
        write(tmp.path(), "a", b"aaaa");
        write(tmp.path(), "b", b"bbbb");
        assert!(scan(tmp.path()).groups.is_empty());
    }

    #[test]
    fn files_that_differ_only_after_the_partial_hash_are_told_apart() {
        let tmp = tempdir().unwrap();
        let mut one = vec![7u8; PARTIAL_BYTES + 100];
        let mut two = one.clone();
        one[PARTIAL_BYTES + 50] = 1;
        two[PARTIAL_BYTES + 50] = 2;
        write(tmp.path(), "one", &one);
        write(tmp.path(), "two", &two);
        write(tmp.path(), "three", &one);

        let report = scan(tmp.path());
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].files.len(), 2);
    }

    #[test]
    fn empty_files_are_ignored_by_default_but_can_be_included() {
        let tmp = tempdir().unwrap();
        write(tmp.path(), "e1", b"");
        write(tmp.path(), "e2", b"");
        assert!(scan(tmp.path()).groups.is_empty());

        let opts = Options { min_size: 0 };
        let report = find_duplicates(&[tmp.path().to_path_buf()], &opts);
        assert_eq!(report.groups.len(), 1);
    }

    #[test]
    fn min_size_filters_small_files() {
        let tmp = tempdir().unwrap();
        write(tmp.path(), "a", b"tiny");
        write(tmp.path(), "b", b"tiny");
        let opts = Options { min_size: 100 };
        assert!(
            find_duplicates(&[tmp.path().to_path_buf()], &opts)
                .groups
                .is_empty()
        );
    }

    #[test]
    fn groups_are_sorted_by_wasted_space() {
        let tmp = tempdir().unwrap();
        write(tmp.path(), "s1", b"small");
        write(tmp.path(), "s2", b"small");
        let big = vec![9u8; 1000];
        write(tmp.path(), "b1", &big);
        write(tmp.path(), "b2", &big);

        let report = scan(tmp.path());
        assert_eq!(report.groups[0].size, 1000);
        assert_eq!(report.groups[1].size, 5);
    }

    #[test]
    fn overlapping_roots_do_not_report_a_file_as_its_own_duplicate() {
        let tmp = tempdir().unwrap();
        write(tmp.path(), "only.txt", b"unique content");
        let roots = [tmp.path().to_path_buf(), tmp.path().to_path_buf()];
        let report = find_duplicates(&roots, &Options::default());
        assert!(report.groups.is_empty());
        assert_eq!(report.scanned, 1);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_count_once_and_symlinks_are_not_followed() {
        let tmp = tempdir().unwrap();
        let original = write(tmp.path(), "orig", b"linked data");
        fs::hard_link(&original, tmp.path().join("hard")).unwrap();
        std::os::unix::fs::symlink(&original, tmp.path().join("sym")).unwrap();

        let report = scan(tmp.path());
        assert!(
            report.groups.is_empty(),
            "links share one file, nothing to free"
        );
        assert_eq!(report.scanned, 1);
    }

    #[test]
    fn missing_root_is_reported_not_fatal() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let report = find_duplicates(&[missing], &Options::default());
        assert!(report.groups.is_empty());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn partial_hash_reads_only_the_requested_prefix() {
        let tmp = tempdir().unwrap();
        let a = write(tmp.path(), "a", b"prefix-AAAA");
        let b = write(tmp.path(), "b", b"prefix-BBBB");
        assert_eq!(
            hash_file(&a, Some(7)).unwrap(),
            hash_file(&b, Some(7)).unwrap()
        );
        assert_ne!(hash_file(&a, None).unwrap(), hash_file(&b, None).unwrap());
    }

    #[test]
    fn known_sha256_value() {
        let tmp = tempdir().unwrap();
        let p = write(tmp.path(), "abc", b"abc");
        assert_eq!(
            hash_file(&p, None).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
