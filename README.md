# dupefind

[![CI](https://github.com/yakim-nick/dupefind/actions/workflows/ci.yml/badge.svg)](https://github.com/yakim-nick/dupefind/actions/workflows/ci.yml)

A small command-line tool that finds duplicate files by content. Written in Rust
as a learning project: ownership and borrowing, error handling without panics,
iterators, a library plus a thin binary, and tests at two levels.

```console
$ dupefind ~/Downloads ~/Documents
Group 1: 2 files, 4194304 bytes each (4194304 bytes wasted)
  /home/me/Downloads/report.pdf
  /home/me/Documents/report-copy.pdf
1 duplicate group(s), 4194304 bytes reclaimable, 5231 file(s) scanned
```

## Install and run

Needs Rust 1.85 or newer (edition 2024).

```bash
git clone https://github.com/yakim-nick/dupefind && cd dupefind
cargo run --release -- <DIR>...        # or: cargo install --path .
```

| Option | Meaning |
|--------|---------|
| `--min-size <BYTES>` | Ignore smaller files (default 1, so empty files are skipped) |
| `--json` | Machine-readable output (`scanned`, `wasted_bytes`, `groups`, `errors`) |
| `--fail-on-duplicates` | Exit status 1 when duplicates exist, handy in scripts |

The tool only **reads** files. It never deletes or modifies anything.

## How it works

Cheapest check first, so most files are never read in full:

1. Group regular files by **size**. A file with a unique size cannot be a duplicate.
2. Within a size group, group by the SHA-256 of the **first 4 KiB**.
3. Within those, compare the **full SHA-256**.

Details worth knowing:

- Symlinks are not followed.
- Hard links to the same inode count once (they share storage, so there is
  nothing to reclaim). On non-Unix systems the canonical path is used instead.
- The same directory given twice does not report files as their own duplicates.
- Unreadable files and missing directories are reported as warnings, and the
  scan continues.
- Results are sorted by reclaimable space, then path, so output is stable.

## Tests

```bash
cargo test                  # 11 unit tests + 4 end-to-end tests that run the binary
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

CI runs formatting, clippy (warnings are errors) and the tests on Linux and macOS.

## Measured, not promised

One run on my Linux machine: 18,856 files in two `node_modules` folders were
scanned in about 4.3 seconds wall-clock (`--min-size 1024`, release build, one run,
not a controlled benchmark). It found 2,868 duplicate groups. Treat it as a rough
indication, not a performance claim.

## Known limitations

- Hashing is single-threaded.
- Whole-file hashing reads full contents of candidate files; very large trees on
  slow disks will be slow.
- Paths are compared as given. Case-insensitive file systems and Windows have not
  been tested (CI covers Linux and macOS only).
- No interactive delete or hard-link replacement mode, by design for now.

## Roadmap

- Parallel hashing (for example with `rayon`) with a measured before/after.
- `--exclude` globs and a progress indicator.
- More small tools in the same style, which is why this is the first of a set.

## License

MIT
