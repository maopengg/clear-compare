# Clear Compare

A local-first desktop file comparison and synchronization tool built with **Rust, Tauri 2 and Leptos**, licensed under **MIT**.

中文默认，可切换英文。比较、扫描和写入由原生Rust核心执行，Leptos界面以WebAssembly运行于系统WebView。

## Status

**0.1 preview, under active development.** This is the first local-file implementation, not full Beyond Compare parity. See the [feature matrix](docs/feature-matrix.md), [complete plan](docs/development-plan.md) and [validation log](docs/roadmap.md).

Implemented: bounded recursive local scans, metadata/SHA-256 comparison, exclusions, paged results, UTF-8/UTF-16 text comparison and editing, normalization rules, three-way merge buffers, paged hexadecimal preview, CSV/TSV key/position table comparison, file selection, reviewed directional copy, verified overwrite backups, operation journals, native folder selection, persisted settings and a JSON CLI.

Planned: advanced merge workflows, Excel/HTML table and image viewers, archives, remote/cloud storage and automatic recovery. A workflow definition is not evidence that all platforms or these features are validated.

## Download

Packaged previews appear on [GitHub Releases](https://github.com/maopengg/clear-compare/releases) after their builds and validation finish. If no asset exists for your platform, build from source below.

| Target | Package | Validation requirement |
|---|---|---|
| macOS Apple silicon | `.app` / `.dmg` | Local build and end-to-end UI test |
| macOS Intel | Build from source initially | Separate runtime test |
| Windows x64 | `.exe` / `.msi` | CI build plus Windows acceptance |
| Linux x64 | `.deb` / `.AppImage` | CI build plus distro acceptance |

Previews are unsigned unless release notes explicitly state otherwise. Read release-specific limitations and validation before using a package.

## 使用方法

1. 选择互不包含的两个文件夹：左侧为来源，右侧为目标。
2. 设置排除规则，点击“比较文件夹”。默认校验内容。
3. 检查差异，点击文本文件阅读左右差异；支持 UTF-8 及带 BOM 的 UTF-16，比较规则可忽略大小写、空白或换行风格。
4. 勾选需要复制的新增或修改文件，点击“预览同步”。
5. 核对方向、文件清单和备份位置后执行。
6. 完成后重新比较。保留备份和操作日志，直到确认结果。

通过每行的“左 / 右”按钮编辑对应文件，保存前生成校验备份；外部修改会阻止保存。三方合并面板接受共同基础版本及左右版本，结果可载入已打开且没有未保存修改的编辑器。Hex 按钮打开分页二进制预览。

勾选“右 → 左”可切换同步方向；切换后需重新选择文件并预览。

目标独有文件保留，符号链接不跟随或复制。取消任务不会撤销已完成动作。目录行的一致仅表示两侧目录存在，不代表整个子树内容相同。

### 排除规则

```text
-README.md;-.DS_Store;-./.git/;-node_modules/;-__pycache__/;-./docs/
```

裸文件名在任意层级匹配；`./docs/`仅匹配根目录下的docs及子树；`node_modules/`匹配任意层级的同名目录。规则以分号或换行分隔，前导减号可省略。当前支持排除规则，不支持包含规则覆盖。

## Build from source

Use the tested Rust 1.99.0 toolchain pinned in `rust-toolchain.toml` and the [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/). macOS needs Apple development tools; Windows needs C++ build tools and WebView2; Linux needs the appropriate WebKitGTK and system development packages.

```sh
git clone https://github.com/maopengg/clear-compare.git
cd clear-compare
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo install tauri-cli --version '^2' --locked
cargo tauri build
```

Packages are under `target/release/bundle/`. There is no npm build pipeline: Trunk builds Leptos CSR and Tauri bundles it with the Rust executable. Generated JavaScript loads WASM and bridges desktop calls. The workspace defaults to two compile jobs.

`cargo tauri dev` starts the desktop client and a local frontend watcher on port1420; stop both when finished. For a frontend build without running a service:

```sh
trunk --no-color build
```

## CLI

```sh
cargo build -p clear-cli --release --locked
./target/release/clear-compare compare ./source ./target --exclude '-./.git/;-node_modules/'

# Text diff: JSON output, optional normalization.
./target/release/clear-compare diff ./old.txt ./new.txt --ignore-line-endings

# CSV comparison: zero-based key column, optional absolute numeric tolerance.
./target/release/clear-compare table ./old.csv ./new.csv --key-column 0 --numeric-tolerance 0.01

# Three-way merge: JSON result, conflict markers, no file writes.
./target/release/clear-compare merge ./base.txt ./left.txt ./right.txt

# Preview only; backups must already exist outside both selected roots.
./target/release/clear-compare sync ./source ./target --backup-dir ./backups

# Editor restore preview; target must match the completed backup manifest.
# Backup base must be outside the target file parent directory.
./target/release/clear-compare restore-editor ./files/edited.txt ./backup-id/original.bin --backup-dir ./backups
# Add --apply to generate and execute a fresh restore plan.

# Reverse direction, still preview-only.
./target/release/clear-compare sync ./left ./right --backup-dir ./backups --reverse

# Generate and apply a fresh plan.
./target/release/clear-compare sync ./source ./target --backup-dir ./backups --apply
```

CLI `--apply` generates a fresh plan; it does not consume previously exported JSON. The desktop consumes its exact stored plan ID. Compare exits0 for verified-equal files,1 for differences/unverified contents and2 for errors; failed sync exits2. Text diff and merge exit1 for differences or unresolved conflicts,0 for equality or conflict-free output,2 for errors. Empty-directory-only differences are visible but do not count in the file summary in this release.

## Limits and integrity

- Text comparison/editing: UTF-8 and BOM-marked UTF-16 LE/BE, at most 4 MiB and 30,000 comparison lines per side. Mixed line-ending documents require an explicit LF/CRLF/CR policy when saving. Encoding/BOM can be converted explicitly; UTF-16 always includes a BOM. Unsupported encodings/binary files return explicit errors.
- Ignore-whitespace removes whitespace within a line for comparison; it does not change displayed text or saved files. Missing final newlines remain significant.
- CSV/TSV: strict quoting, up to 4 MiB / 30,000 rows / 200,000 cells / 1,024 columns; columns compare by position. Duplicate keys are errors. Optional finite absolute numeric tolerance uses floating-point values. Desktop results are paged in blocks of 500.
- Merge results are buffers with conflict markers, not automatic file writes. Two-second diff deadlines can yield coarser alignment on pathological inputs. Hex viewing uses fixed 4 KiB pages; hex editing is not implemented.
- Editor backups: app data `editor-backups/<id>/original.bin` and a digest/state `manifest.json`. Original encoding/BOM and uniform newline style are retained by default; explicit conversion is available. The editor can preview and undo its most recent save, backing up the current contents before restoring. CLI can reopen matching completed editor backups across process restarts; manually inspect unresolved merge markers before saving.
- Scan: at most200,000 entries per root and500 rows per page. Search and difference filtering operate on the current page.
- Desktop backups: app data directory, `backups/<plan-id>/files/`, with `journal.json`. CLI uses the selected backup base.
- Synchronization rollback, backup-history browsing, crash resume, timestamp/ACL/xattr preservation and full encoding support are not implemented yet.
- A multi-file copy is not a transaction. Failure stops later actions and reports partial completion.
- Revalidation handles ordinary external changes, but does not guarantee safety against adversarial filesystem races.

## Development

```sh
cargo fmt --all --check
cargo test -p clear-core -p clear-contracts -p clear-cli --locked
cargo clippy -p clear-core -p clear-contracts -p clear-cli --locked -- -D warnings
cargo check -p clear-ui --target wasm32-unknown-unknown --locked
```

Read [architecture](docs/architecture.md), [contributing](CONTRIBUTING.md) and [security](SECURITY.md). Use synthetic fixtures in issues; do not upload private paths or real comparison data.

## License

Original code is [MIT](LICENSE). Dependencies retain their own licenses; see [third-party notices](THIRD_PARTY_NOTICES.md) and the [resolved dependency list](docs/third-party-dependencies.md). License notices are included in desktop packages. Beyond Compare is a functional reference; Clear Compare is independently implemented and is not affiliated with Scooter Software. No proprietary code, icons or branding is distributed.
