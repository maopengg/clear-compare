# Clear Compare

A local-first desktop file comparison and synchronization tool built with **Rust, Tauri 2 and Leptos**, licensed under **MIT**.

中文默认，可切换英文。比较、扫描和写入由原生Rust核心执行，Leptos界面以WebAssembly运行于系统WebView。

## Status

**0.1 preview, under active development.** This is the first local-file implementation, not full Beyond Compare parity. See the [feature matrix](docs/feature-matrix.md), [complete plan](docs/development-plan.md) and [validation log](docs/roadmap.md).

Implemented: bounded recursive local scans, metadata/SHA-256 comparison, exclusions, directory-subtree status/size aggregation, scoped global search/sorting and paged results, UTF-8/UTF-16 text comparison with intraline hints, aligned paging, change navigation/literal search and editing with bounded undo/redo, normalization rules, three-way text merge buffers with conflict navigation/version choices and undo/redo, two/three-way folder merge to independent output, paged hexadecimal preview, pixel picture comparison with overlay/difference views, CSV/TSV key/position table comparison, file selection, reviewed directional copy, verified overwrite backups, operation journals, persistent copy history and reviewed rollback, native folder selection, named revision-aware session library, read-only ZIP/directory browsing and comparison, content-free directory snapshots, read-only local Git commit-tree comparison, persisted view settings, HTML/JSON comparison reports and a JSON CLI.

Planned: advanced merge workflows, Excel/HTML table viewers and advanced picture workflows, archive updates/nested formats, remote/cloud storage and automatic recovery. A workflow definition is not evidence that all platforms or these features are validated.

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
3. 点击目录进入子目录，通过面包屑返回；可切换“包含子目录”，搜索和排序作用于当前范围的全部条目。检查差异，点击文本文件阅读左右差异；支持 UTF-8 及带 BOM 的 UTF-16，比较规则可忽略大小写、空白或换行风格。
4. 勾选需要复制的新增或修改文件，点击“预览同步”。
5. 核对方向、文件清单和备份位置后执行。
6. 完成后重新比较。保留备份和操作日志，直到确认结果。

通过每行的“左 / 右”按钮编辑对应文件，保存前生成校验备份；外部修改会阻止保存。三方合并面板接受共同基础版本及左右版本，结果可载入已打开且没有未保存修改的编辑器。Hex 按钮打开分页二进制预览。

勾选“右 → 左”可切换同步方向；切换后需重新选择文件并预览。

目标独有文件保留，符号链接不跟随或复制。取消任务不会撤销已完成动作。目录状态会聚合按当前规则扫描到的子项；目录大小是已列出的常规文件大小之和，不是实际磁盘占用。子项有差异、错误或未验证内容时，目录不会标为内容一致。

### 比较报告

目录、ZIP/目录、目录快照和 Git 比较均可导出 HTML 或 JSON 报告。导出读取原生端保存的完整结果，不受当前搜索、目录范围或 500 行分页限制。可选择仅导出差异，这仍会保留未验证、读取错误和类型冲突；汇总数字始终对应完整比较。

报告包含来源、排除规则、比较依据、状态、文件元数据及已有摘要，不包含文件正文。HTML 可离线打开，无脚本或外部资源；所有动态内容转义。JSON 使用带 schema/version 的格式，见 [报告格式](docs/report-format.md)。报告记录历史结果，不重新扫描，不证明文件当前状态，也不是文件恢复输入。

请保存到比较目录、Git 工作树与 Git 数据目录之外的新文件，现有文件不会被覆盖。导出采用有大小上限的临时文件，成功后才保留目标；取消或失败会清理临时输出。文本/表格/图片专用报告与同步任务报告尚未实现。

### 文本差异导航

文本预览按修改组中的行序对齐两侧，左右面板的垂直及水平滚动联动；这不是语义最佳行匹配，也尚不支持手动对齐。行内下划线遵循大小写、空白和换行比较规则，原文不变。行尾标签显示 LF、CRLF、CR 或 ∅（无末尾换行）。

“上一/下一差异”跨全部结果跳转，预览区聚焦时也可使用 F7 / Shift+F7。逐行字面查找区分大小写，搜索全部左右原文，每个匹配行计一次，可跨页定位。每页最多 500 个对齐行；每行最多显示前 8192 个 Unicode 字符，并显示截断标签。匹配可能位于截断部分；载入合并和 CLI JSON 仍使用完整原文。

行内细分限制每对原始行合计 16 KiB、规范化后 4096 个 Unicode 标量，单次比较累计 100,000 个标量及 150 ms 计算预算。超预算或无法细分的行会显示“整行提示”，不表示其中每个字节都不同。标记偏移是原文 UTF-8 字节范围；不承诺按完整字素簇细分。完整文本读取/行数上限仍生效。

### 文本冲突处理

三方合并结果保留在缓冲区。使用“定位当前冲突”和上一/下一冲突跳转，再选择左侧、基础或右侧原文，也可按左后右直接拼接。选择会保留原始换行字节，拼接不自动增加换行；即使原片段末尾没有换行也不会补入；载入编辑器后，最终磁盘编码及换行格式仍由目标编辑器设置决定。已经手动修改的冲突组会阻止版本选择，请手动完成或先撤销修改。

合并缓冲区支持独立的撤销/重做，以及 Ctrl/Cmd+Z、Shift+Z、Ctrl/Cmd+Y。定位使用 UTF-16 选区偏移，中文和 emoji 不会因 UTF-8 字节长度而错位。标记被破坏、复制或缺失时停止自动处理，原生保存检查仍会阻止带有未解决标记的结果落盘。更改任一输入或重新生成结果会重置本轮历史。

输入与生成结果均限制为 8 MiB 解码文本，输入最多 30,000 行。结果可载入没有未保存修改的编辑器，再经过备份和外部变更检查保存；尚未提供独立工作副本导出。合并 JSON 新增原始冲突片段与稳定组 ID，旧式标记需手动处理。

### 会话库

“会话库”可命名保存两侧路径、排除规则、语言、内容校验、搜索、排序、递归范围和文本比较规则。打开会话只加载配置，不触发比较或同步；目录位置回到根范围。更新使用修订校验，归档将配置移到本机会话库的 `trash/` 目录。每份配置最多 64 KiB，库最多 200 个活动会话。最新比较配置会在下次启动时恢复；结果和未保存编辑缓冲区不持久化。

### ZIP 只读比较

“归档比较”支持 ZIP 对 ZIP、ZIP 对目录，右侧留空可浏览单包。可搜索全部成员、分页、导入排除规则并预览文本差异；不解压到磁盘、不更新原包。当前支持经典 ZIP 的 Stored/Deflate，拒绝 ZIP64、多卷、加密、重复/不安全路径及超预算输入；其他压缩算法报告读取错误。符号链接只列出、不跟随。元数据匹配不代表内容一致。

输入上限 256 MiB，最多 50,000 个成员及虚拟目录、16 MiB 中央目录；内容校验限制单成员 128 MiB、总展开内容 1 GiB，并限制异常膨胀比例。文本预览限制 4 MiB，容器或已校验成员发生变化后需要重新比较。嵌套包、写入更新及其他格式尚未实现。

### 目录快照

在“归档 / 快照”面板切换到“目录快照”，左侧选择目录后点击“保存左侧目录快照…”。`.cctree` 文件仅保存路径、类型、大小、修改时间、读取错误及捕获规则，不包含文件正文或内容摘要。快照须保存到来源目录之外，现有文件不会被覆盖。比较时按面板当前的排除规则同时筛选两侧；捕获时已排除的条目无法补回。快照是逐项扫描的历史清单，不是文件系统某一瞬间的一致性备份，也不能用于恢复正文。

可浏览快照、比较两个快照，或与当前目录比较。大小/修改时间变化显示为修改；匹配仍是“未验证内容”。不提供编辑、同步、正文预览或恢复操作。最多 200,000 项、64 MiB；Unicode 组件使用可移植路径，非可移植名称保留原生编码并限同系统读取。应用会拒绝损坏、重复、越界或结构不完整的清单。

### Git 修订版本比较

“Git”面板选择本地仓库，输入两个提交、分支或标签（例如 `HEAD~1` / `HEAD`），即可搜索版本树、筛选差异并查看 UTF-8/UTF-16 文本。引用在比较时解析为完整提交 ID，后续分支移动不会切换已经打开的结果。可执行位变化也计为修改；一致使用 Git 对象身份判定，未运行额外的 SHA-256 全树读取。

需要 PATH 中安装支持 `--no-lazy-fetch` 的 Git（本地已验证 Git 2.50.1）；不支持该参数会报错。只读提交对象，不参与工作区/暂存区修改，不检出、提交或下载缺失对象。符号链接不跟随，子模块不展开，Git LFS 比较指针文本。树输出限制 64 MiB、比较最多 200,000 条路径，单条命令限时 15 秒，文本预览限制 4 MiB。Git difftool/mergetool 注册、工作区对照、SVN 和写入集成尚未实现。

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

# Folder merge preview; all directories must exist and output must be empty.
./target/release/clear-compare merge-folders ./base ./left ./right ./merged --journal-dir ./merge-journals
# Two-way mode uses - as the baseline; choices apply to exact scanned relative paths.
./target/release/clear-compare merge-folders - ./left ./right ./merged --journal-dir ./merge-journals --choose sub=right --apply

# Reports must be NEW files outside compared roots.
./target/release/clear-compare report ./source ./target ./comparison.html --exclude '-./.git/'
./target/release/clear-compare report ./source ./target ./comparison.json --format json --differences-only
./target/release/clear-compare report ./backup.zip ./folder ./archive.html --source archive
./target/release/clear-compare report ./saved.cctree ./folder ./snapshot.json --source snapshot --format json
./target/release/clear-compare report 'HEAD~1' HEAD ./git-report.html --source git --repository ./repository
# Exit 1 means differences/unverified entries even when the report was saved.
# Exit 2 can mean recorded comparison errors or an export failure; inspect JSON stdout/stderr.

# Compare locally available Git commits without checkout or fetching.
./target/release/clear-compare git-compare ./repository 'HEAD~1' HEAD --exclude '-./docs/'

# Export a content-free inventory to a NEW file outside the source.
./target/release/clear-compare snapshot ./folder ./saved.cctree --exclude '-node_modules/'
./target/release/clear-compare snapshots ./saved.cctree
./target/release/clear-compare snapshots ./saved.cctree ./folder
# Paired metadata matches return 1 (unverified), not verified equality (0).

# Read-only ZIP browse or compare, JSON output.
./target/release/clear-compare archives ./backup.zip
./target/release/clear-compare archives ./backup.zip ./folder --exclude '-node_modules/'

# Picture statistics (PNG/JPEG/BMP/WebP), optional channel tolerance.
./target/release/clear-compare picture ./left.png ./right.png --tolerance 2

# CSV comparison: zero-based key column, optional absolute numeric tolerance.
./target/release/clear-compare table ./old.csv ./new.csv --key-column 0 --numeric-tolerance 0.01

# Three-way merge: JSON result, conflict markers, no file writes.
./target/release/clear-compare merge ./base.txt ./left.txt ./right.txt

# Preview only; backups must already exist outside both selected roots.
./target/release/clear-compare sync ./source ./target --backup-dir ./backups

# Synchronization recovery: preview rollback; --apply executes a fresh plan.
./target/release/clear-compare recover-sync ./backups/plan-id/journal.json --backup-dir ./recovery-backups

# Editor restore preview; target must match the completed backup manifest.
# Backup base must be outside the target file parent directory.
./target/release/clear-compare restore-editor ./files/edited.txt ./backup-id/original.bin --backup-dir ./backups
# Add --apply to generate and execute a fresh restore plan.

# Reverse direction, still preview-only.
./target/release/clear-compare sync ./left ./right --backup-dir ./backups --reverse

# Generate and apply a fresh plan.
./target/release/clear-compare sync ./source ./target --backup-dir ./backups --apply
```

CLI `--apply` generates a fresh plan; it does not consume previously exported JSON. The desktop consumes its exact stored plan ID. Compare exits0 for verified-equal files,1 for differences/unverified contents and2 for errors; failed sync exits2. Text diff and merge exit1 for differences or unresolved conflicts,0 for equality or conflict-free output,2 for errors. Empty-directory-only differences produce compare exit1; ordinary directory rows remain outside the file-count summary.

## Limits and integrity

- Text comparison/editing: UTF-8 and BOM-marked UTF-16 LE/BE, at most 4 MiB encoded file bytes, 8 MiB decoded UTF-8 text and 30,000 comparison lines per side. Mixed line-ending documents require an explicit LF/CRLF/CR policy when saving. Encoding/BOM can be converted explicitly; UTF-16 always includes a BOM. Unsupported encodings/binary files return explicit errors.
- Ignore-whitespace removes whitespace within a line for comparison; it does not change displayed text or saved files. Missing final newlines remain significant.
- CSV/TSV: strict quoting, up to 4 MiB / 30,000 rows / 200,000 cells / 1,024 columns; columns compare by position. Duplicate keys are errors. Optional finite absolute numeric tolerance uses floating-point values. Desktop results are paged in blocks of 500.
- Folder merge: separate empty output, at most 200,000 union entries, content-verified inputs and paged conflict choices. Directory choices apply to their entire subtree. Deletions omit output entries; input trees are preserved. Every unresolved conflict blocks execution.
- Folder auto text merge uses UTF-8/BOM-marked UTF-16, preserves the left encoding/BOM/uniform newline style, and uses at most 4 MiB per input file and 64 MiB of generated buffers. Unsupported text/links/special files become conflicts; manual supported-version selection or omission is available. Unique content matches indicate probable rename-versus-modification conflicts, not all possible renames.
- Folder-merge manifests, generated text artifacts and execution journals are stored under app data `merge-runs/` or the CLI journal base. All inputs are revalidated before output creation; final output structure and contents are verified. Partial output can remain after an error/cancellation; automatic output cleanup/resume is not implemented.
- Picture comparison: PNG/JPEG/BMP/WebP, EXIF orientation, top-left alignment and RGBA8 channel differences. Inputs are limited to 16 MiB, each side to 16,384 pixels, and each image/aligned canvas to 8 million pixels. Decoder allocation budget is best-effort 128 MiB, separate from application buffers.
- Picture tolerance uses the maximum selected channel difference (0–255); missing pixels always differ. Optional alpha-channel ignoring still compares RGB values, including hidden RGB values. High-bit-depth inputs are converted to RGBA8; ICC/perceptual color comparison and animation-frame comparison are not implemented.
- Picture statistics use the full canvas; aligned previews have at most a 1,024-pixel side. Red heatmap buckets preserve any changed pixel in that region. Preview zoom and opacity do not change statistics.
- Text editing has UTF-8-safe splice undo/redo (up to 128 steps and 16 MiB of history), toolbar controls and Ctrl/Cmd+Z, Shift+Z or Y shortcuts. Input-method composition is recorded as a single committed edit. Logical LF buffers preserve chosen file newline styles; text history survives saves and resets on document switch/restore.
- Generated text merge conflicts include unique `[cc:...]` tags. Loading into an editor registers native provenance; ordinary save rejects unresolved tags or newly introduced conflict labels. The guard survives saves so undo cannot reintroduce unresolved output into a save. Existing tagged working copies are recognized when reopened. Bare Markdown `=======` lines are not treated as conflicts.
- Merge results are buffers with conflict markers, not automatic file writes. Two-second diff deadlines can yield coarser alignment on pathological inputs. Hex viewing uses fixed 4 KiB pages; hex editing is not implemented.
- Editor backups: app data `editor-backups/<id>/original.bin` and a digest/state `manifest.json`. Original encoding/BOM and uniform newline style are retained by default; explicit conversion is available. The editor can preview and undo its most recent save, backing up the current contents before restoring. CLI can reopen matching completed editor backups across process restarts; manually inspect unresolved merge markers before saving.
- Scan: at most200,000 entries per root and500 rows per page. Search, difference filtering and sorting operate across the entire selected directory scope before pagination. Folder navigation uses opaque native IDs and breadcrumbs; optional recursion includes descendants. Directories remain before files in both ascending and descending sorts.
- Desktop backups: app data directory, `backups/<plan-id>/files/`, with `journal.json`. CLI uses the selected backup base.
- Recovery-journal resume/redo, editor-backup browsing, automatic crash resume, timestamp/ACL/xattr preservation and full encoding support are not implemented yet.
- Copy history is available from the desktop “Operation history” button after restart. Reviewed rollback restores overwritten files and removes only hash-matching created files, backing up their current contents first. `running`/`failed` journal entries are reconciled against actual contents; conflicting changes block recovery.
- Durable native paths are recorded for new sync journals. Older journals without these fields cannot be automatically recovered. Journals must stay with their original backup directory and on the originating operating system. Recovery journals are currently evidence/backups, not resumable redo plans.
- Recovery reads at most 64 MiB of source journal JSON. Desktop history lists at most 2,000 backup-directory entries. Unicode backup-directory paths are required; supported native entry names are stored losslessly.
- A multi-file copy or recovery is not a transaction. Failure stops later actions and reports partial completion.
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
