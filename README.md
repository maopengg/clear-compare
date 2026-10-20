# Clear Compare

A local desktop file comparison and synchronization tool built with **Rust, Tauri 2 and Leptos**, licensed under **MIT**.

中文默认，可切换英文。项目范围为本地文件比较与同步，不提供远程文件系统或云存储连接。比较、扫描和写入由原生Rust核心执行，Leptos界面以WebAssembly运行于系统WebView。

## Status

**0.1 preview, under active development.** This is the first local-file implementation, not full Beyond Compare parity. See the [feature matrix](docs/feature-matrix.md), [complete plan](docs/development-plan.md) and [validation log](docs/roadmap.md).

Implemented: bounded recursive local scans, metadata/SHA-256 comparison, exclusions, directory-subtree status/size aggregation, scoped global search/sorting and paged results, UTF-8/UTF-16 text comparison with intraline hints, aligned paging, change navigation/literal search and editing with bounded undo/redo, normalization rules, three-way text merge buffers with conflict navigation/version choices and undo/redo, two/three-way folder merge to independent output, paged hexadecimal preview with byte differences, offset navigation and bounded overlapping byte search, pixel picture comparison with overlay/difference views, CSV/TSV/static-HTML/Excel-OOXML table comparison with independent source formats and frozen native inputs, file selection, reviewed directional copy and selected regular-file deletion with recovery, common-baseline bidirectional file/tree reconciliation with explicit conflict winners and coordinated baseline rollback, reviewed forward restart, verified backups, operation journals, persistent copy history and reviewed rollback, native folder selection, named revision-aware session library, read-only ZIP/directory browsing and comparison, content-free directory snapshots, read-only local Git commit/index/raw-working-tree comparison, persisted view settings, HTML/JSON comparison reports and a JSON CLI.

Planned: advanced merge workflows, legacy Excel formats and advanced picture workflows, archive updates/nested formats and automatic recovery. A workflow definition is not evidence that all platforms or these features are validated.

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
2. 设置排除规则，点击“比较文件夹”。新会话默认按文件大小快速比较，修改时间只展示、不参与差异判断；大小相同暂标为未验证内容。“只看差异”开启时自动在后台校验两侧大小相同的文件，确认一致后移出差异列表；不会读取单侧独有文件。勾选完整 SHA-256 校验时先显示快速列表，再读取全部未排除文件并更新结果。已保存会话保留原来的校验选择。
3. 目录结果默认左右并排显示，同一相对路径保持同行；各侧名称、大小和修改时间独立展示，缺失侧明确标识。点击三角展开/折叠目录，支持全部展开/折叠；双击文件或选中后点“查看内容差异”，直接打开左右逐行差异。关闭“目录树”可使用平面导航；搜索保留并展开匹配条目的父目录，过滤在分页之前生效。检查差异，点击文本文件阅读左右差异；支持 UTF-8 及带 BOM 的 UTF-16，比较规则可忽略大小写、空白或换行风格。
4. 勾选需要复制的新增或修改文件，点击“预览同步”。
5. 核对方向、文件清单和备份位置后执行。
6. 完成后重新比较。保留备份和操作日志，直到确认结果。

通过每行的“左 / 右”按钮编辑对应文件，保存前生成校验备份；外部修改会阻止保存。三方合并面板接受共同基础版本及左右版本，结果可载入已打开且没有未保存修改的编辑器。Hex 按钮打开分页二进制预览。

勾选“右 → 左”可切换同步方向；切换后需重新选择文件并预览。

复制模式保留目标独有文件；只有明确选择删除模式并执行预览计划时才移除所选文件。符号链接不跟随或复制。取消任务不会撤销已完成动作。目录状态会聚合按当前规则扫描到的子项；目录大小是已列出的常规文件大小之和，不是实际磁盘占用。子项有差异、错误或未验证内容时，目录不会标为内容一致。

### 文件删除、重命名与恢复

在操作下拉框选择“删除左侧文件”或“删除右侧文件”，选择可读普通文件后预览。计划显示目标目录、文件名、大小及备份位置；执行时先创建并验证备份，再重新校验并移除。可从操作历史预览恢复，原路径若已被重新创建则拒绝覆盖。目录、链接及未选择文件不会移除；需要查看相同文件时先关闭“仅差异”。操作切换会清空选择，删除模式不随会话保存。

删除恢复保留原始字节和基本权限，不是完整 ACL/时间戳备份；也不使用系统废纸篓。独立目录删除、跨目录移动、批量重命名、类型替换、完整元数据复制和高级双向同步策略仍在计划中。详细格式、兼容性与中断恢复规则见 [文件操作说明](docs/file-operations.md)。

在操作菜单选择“重命名左侧文件”或“重命名右侧文件”，恰好选择一个普通文件并输入同目录的新文件名，然后预览执行。支持仅大小写变化，拒绝覆盖任何已有名称；执行前验证备份，再通过日志中记录的临时名称完成原生重命名。操作历史可预览改回原名，也可恢复中断后仍位于临时名称的文件。已打开的编辑器跟随路径，保留未保存内容和撤销记录；外部变更仍会阻止保存。

### 普通文件镜像

操作菜单支持“文件镜像：左 → 右 / 右 → 左”，对整个已比较范围生成计划：复制源端新增/修改的普通文件，并备份删除目标独有普通文件。范围不受搜索、当前目录、分页或勾选影响，排除项保留，预览明确列出复制/删除数量。需要内容校验；链接、读取错误和类型冲突会阻止计划。预览与执行前会重新校验整个范围，包括原本相同的文件。此模式保留空目录，需要目录创建/移除时使用下方目录镜像；详见 [文件镜像说明](docs/file-mirror.md)。

### 目录镜像（含空目录）

选择“目录镜像：左 → 右 / 右 → 左”可同步整个已比较范围的普通文件和目录：按父子顺序创建缺失目录，复制/备份删除文件，再移除已清空的多余目录。含排除项的目录及其必需父目录会保留并显示提示；链接、读取错误、类型冲突和目录身份变更会阻止计划。预览分别列出复制、删除文件、创建目录和移除空目录数量。

目录元数据和创建身份记录保存在备份集内，回退按依赖逆序恢复，拒绝覆盖重建路径或清理含外部内容的目录。目录权限可恢复，原属主、ACL、时间戳不属于完整备份；中断和部分回退的限制见 [目录镜像说明](docs/tree-mirror.md)。

### 基于共同基线的双向同步

先开启内容校验并比较目录，在“双向同步与共同基线”中建立或选择 `.ccbase`。首次基线要求两侧已一致（包括空目录），保存位置必须在两侧之外。后续同步会按上次共同的 SHA-256 状态区分独立修改、新增和删除，而非按时间戳猜测。

预览会列出两个方向的写入及基线/左右状态。双方修改、删除与修改冲突需明确选择保留侧；目录选择作用于整个分支，含排除项的目录禁止选择清空。只有两侧都达到预览结果后，基线才会更新。基线提交前取消或失败不会推进基线，并保留已完成动作的备份；可选择返回的 `run.json` 预览协调回退，同时恢复两侧动作与原基线。已有更新修订时拒绝回退旧运行。类型替换、完整元数据同步和部分回退续跑仍待实现。详见 [双向同步说明](docs/bidirectional-sync.md)。

### 任务暂停、进度与重连

执行已预览的文件操作或回退时，可查看动作进度及读取/写入计数，并暂停、继续或取消。界面区分“等待安全暂停点”和“已暂停”；替换、删除前的最终校验及日志写入不会被暂停打断。恢复执行前会重新校验暂停期间可能变化的文件。

“连接文件任务”可找回同一原生进程中仍在运行或尚未领取结果的任务，界面重新加载时也会自动尝试连接。程序重启后可在原操作历史选择“预览续跑”，核对已完成、待重试和临时文件保留动作，再继续原同步；也可预览回退。新操作保留原始意图及每次重试的独立备份，外部变更会阻止不安全续跑。旧日志没有续跑上下文时仍使用回退。见 [重启续跑说明](docs/operation-resume.md)。详细状态、结果领取与限制见 [任务说明](docs/operation-tasks.md)。

### Excel / HTML / CSV / TSV 表格

点击文件行中的“表”按钮，默认识别 Excel OOXML 工作簿（XLSX / XLSM / XLTX / XLTM）、HTML / HTM、TSV，其余使用 CSV。两侧格式和表序号可独立指定，支持 Excel、HTML 与分隔文本对照。原始文件只在原生端读取一次，之后改主键列、容差或表序号都复用同一份内容；重新点击文件列表中的表按钮可刷新。开启目录内容校验时，文件已变更会要求先重新比较目录。

HTML 可按 0 起始序号选择每侧表格；“列出表格 / 工作表”展示标题、元素 ID 和 DOM 行数。解析不运行脚本、不加载资源，支持实体、可省略的行/单元格结束标签及 rowspan/colspan 展开。仅比较静态文本值；详细规则和限制见 [表格格式说明](docs/table-formats.md)。

预览按行分页，并每次展示最多 25 列。单元格超过 8192 个 Unicode 字符会显示截断标签，原始比较数据仍保留；CLI JSON 返回完整值。可开启“按列名对齐”，按精确且唯一的表头匹配不同列顺序；映射顺序为左侧列，再追加右侧独有列。此时主键列填写映射序号，缺列与空单元格分别标识。

Excel 可选择每侧工作表，包括标为隐藏的工作表；比较保存的缓存值，不重新计算公式。缺少公式缓存或单元格错误会明确拒绝，日期按工作簿日期体系转为本地 ISO 日期时间，数值格式和合并单元格外观不参与比较。旧版 XLS、XLSB、公式文本比较、任意重命名列映射、表格编辑和更丰富容差仍在计划中。

### 比较报告

目录、ZIP/目录、目录快照和 Git 比较均可导出 HTML 或 JSON 报告。导出读取原生端保存的完整结果，不受当前搜索、目录范围或 500 行分页限制。可选择仅导出差异，这仍会保留未验证、读取错误和类型冲突；汇总数字始终对应完整比较。

报告包含来源、排除规则、比较依据、状态、文件元数据及已有摘要，不包含文件正文。HTML 可离线打开，无脚本或外部资源；所有动态内容转义。JSON 使用带 schema/version 的格式，见 [报告格式](docs/report-format.md)。报告记录历史结果，不重新扫描，不证明文件当前状态，也不是文件恢复输入。

请保存到比较目录、Git 工作树与 Git 数据目录之外的新文件，现有文件不会被覆盖。导出采用有大小上限的临时文件，成功后才保留目标；取消或失败会清理临时输出。文本/表格/图片专用报告与同步任务报告尚未实现。

### 文本差异导航

本地文本比较支持在中间差异箭头双向合并：左右两侧可以同时保留未保存修改。macOS 使用 ⌘S，Windows/Linux 使用 Ctrl+S，仅保存当前聚焦的文本侧；点击“保存两侧”保存左右所有未保存修改。各侧按原编码和换行格式保存，并验证外部变更及备份。两侧依次保存，若一侧失败，已经保存的一侧保留，另一侧缓冲区仍可重试；保存会重置本轮撤销历史。“放弃合并”只丢弃尚未保存的内容。

文本预览按修改组中的行序对齐两侧，左右面板的垂直及水平滚动联动；这不是语义最佳行匹配，也尚不支持手动对齐。行内下划线遵循大小写、空白和换行比较规则，原文不变。行尾标签显示 LF、CRLF、CR 或 ∅（无末尾换行）。

“上一/下一差异”跨全部结果跳转，预览区聚焦时也可使用 F7 / Shift+F7。逐行字面查找区分大小写，搜索全部左右原文，每个匹配行计一次，可跨页定位。每页最多 500 个对齐行；每行最多显示前 8192 个 Unicode 字符，并显示截断标签。匹配可能位于截断部分；载入合并和 CLI JSON 仍使用完整原文。

行内细分限制每对原始行合计 16 KiB、规范化后 4096 个 Unicode 标量，单次比较累计 100,000 个标量及 150 ms 计算预算。超预算或无法细分的行会显示“整行提示”，不表示其中每个字节都不同。标记偏移是原文 UTF-8 字节范围；不承诺按完整字素簇细分。完整文本读取/行数上限仍生效。

### 文本冲突处理

三方合并结果保留在缓冲区。使用“定位当前冲突”和上一/下一冲突跳转，再选择左侧、基础或右侧原文，也可按左后右直接拼接。选择会保留原始换行字节，拼接不自动增加换行；即使原片段末尾没有换行也不会补入；载入编辑器后，最终磁盘编码及换行格式仍由目标编辑器设置决定。已经手动修改的冲突组会阻止版本选择，请手动完成或先撤销修改。

合并缓冲区支持独立的撤销/重做，以及 Ctrl/Cmd+Z、Shift+Z、Ctrl/Cmd+Y。定位使用 UTF-16 选区偏移，中文和 emoji 不会因 UTF-8 字节长度而错位。标记被破坏、复制或缺失时停止自动处理，原生保存检查仍会阻止带有未解决标记的结果落盘。更改任一输入或重新生成结果会重置本轮历史。

输入与生成结果均限制为 8 MiB 解码文本，输入最多 30,000 行。结果可载入没有未保存修改的编辑器，再经过备份和外部变更检查保存；尚未提供独立工作副本导出。合并 JSON 新增原始冲突片段与稳定组 ID，旧式标记需手动处理。

### 多项目与排除项

顶部“项目”入口支持新建、打开、重命名、另存为和归档项目配置。每个项目独立保存左右文件夹、排除规则、比较和视图设置；切换项目、关闭项目管理面板以及开始比较前保存当前已命名项目。未命名项目填写名称和两个路径后点击“保存项目”。主界面修改配置后，“•”提示未保存，可点击“保存项目”。原有会话库继续可用；首次启动会把原来的最后一次比较配置转成项目，后续启动恢复当前项目。打开项目只加载配置，再点击“比较文件夹”扫描；比较结果和未保存文件编辑不持久化。

文件、目录右键“排除此项”会以捕获的原生条目 ID 生成精确的根目录规则，目录包含子树，匹配两侧，不删除磁盘文件。规则写入当前项目后重新比较。顶部“排除”打开规则编辑器，支持添加、移除和批量编辑，例如 `*.log`、`.venv/`、`./docs/`；无效规则保留输入，不覆盖已保存配置。右键生成会转义通配字符；无法用当前文本规则表示的文件名会明确报错。

项目仍使用本地版本化会话存储：每份配置最多 64 KiB，最多 200 个活动项目。保存检查修订版本，冲突时保留界面修改，可另存为新项目；归档只移动配置到本机 `sessions/trash/`，不操作项目目录。主题设置独立于项目。

### ZIP 只读比较

“归档比较”支持 ZIP 对 ZIP、ZIP 对目录，右侧留空可浏览单包。可搜索全部成员、分页、导入排除规则并预览文本差异；不解压到磁盘、不更新原包。当前支持经典 ZIP 的 Stored/Deflate，拒绝 ZIP64、多卷、加密、重复/不安全路径及超预算输入；其他压缩算法报告读取错误。符号链接只列出、不跟随。元数据匹配不代表内容一致。

输入上限 256 MiB，最多 50,000 个成员及虚拟目录、16 MiB 中央目录；内容校验限制单成员 128 MiB、总展开内容 1 GiB，并限制异常膨胀比例。文本预览限制 4 MiB，容器或已校验成员发生变化后需要重新比较。嵌套包、写入更新及其他格式尚未实现。

### 目录快照

在“归档 / 快照”面板切换到“目录快照”，左侧选择目录后点击“保存左侧目录快照…”。`.cctree` 文件仅保存路径、类型、大小、修改时间、读取错误及捕获规则，不包含文件正文或内容摘要。快照须保存到来源目录之外，现有文件不会被覆盖。比较时按面板当前的排除规则同时筛选两侧；捕获时已排除的条目无法补回。快照是逐项扫描的历史清单，不是文件系统某一瞬间的一致性备份，也不能用于恢复正文。

可浏览快照、比较两个快照，或与当前目录比较。大小/修改时间变化显示为修改；匹配仍是“未验证内容”。不提供编辑、同步、正文预览或恢复操作。最多 200,000 项、64 MiB；Unicode 组件使用可移植路径，非可移植名称保留原生编码并限同系统读取。应用会拒绝损坏、重复、越界或结构不完整的清单。

### 多项目工作台与 Git 写操作

首页同时收录已保存的比较项目和 Git 仓库，可配置多个扫描目录及跳过的目录名称。扫描识别普通仓库、嵌套仓库、子模块、worktree 和裸仓库；SSH/HTTPS 远程地址归一后用于排序归组，本地副本分别保留。支持项目搜索、仓库收藏、最近打开、状态/分支/远程查看。扫描和打开仓库不会自动获取远程内容。

仓库工作台提供整文件暂存、取消暂存、提交/修改最近提交、分支新建/切换/重命名/删除、检出提交、合并、变基、拣选、反向提交、轻量标签、上游设置、Fetch、Pull、Push，以及保存 Stash。进行中的合并、变基、拣选和反向提交提供适用的继续/跳过/中止操作。Git hooks 使用本机配置，成功输出和失败信息会显示在界面。

工作区按未暂存/已暂存/未跟踪/冲突分组，支持路径筛选、平面/目录树、多选和分页。点击文件查看未暂存、已暂存或 HEAD 对工作区差异；普通文本可按块或选中行暂存/取消暂存，操作不改写工作文件。可直接打开工作区文件编辑器，返回保留 Git 差异。范围、恢复引用及预览限制见 [Git 工作区差异说明](docs/git-working-diff.md)。

写操作使用原生捕获的仓库和文件 ID，并在执行前检查状态版本；放弃修改、历史重写、强制删除等操作先展示确认范围。删除分支、标签或 Stash 的核心操作保留对应对象的恢复引用，工作区丢弃和硬重置保留可恢复快照。网络操作使用本机 Git/SSH/凭据配置，当前命令有时间和输出上限；失败或取消可能留下需要处理的 Git 状态，应刷新查看，禁止自动重试写操作。

历史页支持分页提交关系图、按引用/作者/日期/相对路径/说明或哈希查询、提交说明及变更文件、选择合并父提交、提交文件路径筛选/200 项分页、逐文件只读补丁预览/500 行分页和父提交导航；按需查找可达的直接子提交并分页跳转，不受当前历史筛选影响。筛选关系图只绘制可见提交的直接连接，完整父提交仍可在详情中查看。右键可复制哈希、拣选、反向提交、检出、Soft/Mixed/Hard 重置及创建分支/轻量标签。重置先确认并保留恢复引用；当前硬重置会拒绝存在未跟踪文件或未解决冲突的仓库，相关界面操作禁用。

“文件 / 全局搜索”提供工作树目录浏览、创建文件/目录、多标签文本编辑、当前文件查找替换、跨仓库搜索和带预览的跨文件替换。支持原编码/换行保留、格式转换、保存快捷键、外部修改检查与未保存保护；返回首页或比较页面保留打开的缓冲区。项目文件规则持久化，编辑标签当前仅在本次进程内保留。具体范围和限制见 [文件工作台说明](docs/source-workspace.md)。

文件树右键提供复制名称/路径、系统中显示、排除及文件/目录操作；重命名、复制、同卷移动和废纸篓先预览，再复查与备份执行。拒绝覆盖已有名称、处理包含 Git 元数据/链接的目录，以及移动或删除仍打开的文件。跨卷移动、大小写改名、备份 GUI 恢复和三系统废纸篓运行验收仍待补齐。

分支入口提供搜索、最近/收藏/本地/远程/标签分组、明确的跟踪检出、切换并更新/变基、指定目标推送及快捷键。收藏与最近记录按仓库保留，失败时保留表单，复合操作失败保留实际 Git 现场。详细语义与限制见 [分支菜单说明](docs/git-branches.md)。

分支菜单支持直接进入工作台内的分支/当前提交/暂存区/工作区只读比较，保留原工作区与历史视图。来源使用捕获 ID，工作区变化后要求重新比较；支持路径筛选、分页与对齐文本。详见 [分支比较说明](docs/git-reference-comparison.md)。

贮藏管理提供分页记录、保存时的已暂存/未暂存/未跟踪差异、应用/弹出、删除、从贮藏新建分支和可选索引恢复。写操作先确认并保留恢复引用，冲突保留记录和实际 Git 状态。详见 [贮藏说明](docs/git-stashes.md)。

远程管理已提供添加/修改/删除、获取与推送地址分离、移除仓库内独立推送配置、只读连接检查及获取/清理。修改先确认，并保存配置和跟踪引用的恢复材料；不改写继承的共享配置。详见 [远程管理说明](docs/git-remotes.md)。

工作区管理提供 Worktree 列表、筛选、从分支/标签创建新分支或分离 HEAD 工作区、检出已有分支、打开与系统定位、锁定/解锁、受保护的移除和失效注册清理。写操作先确认并复查，清理保护 detached HEAD 对象；失败保留输入和实际状态，详见 [Worktree 管理说明](docs/git-worktrees.md)。

子模块管理提供递归状态、父项目记录/实际 HEAD、地址、初始化注册、普通/递归更新和打开独立仓库。更新检出父项目记录的提交，使用确认与复查、私有配置快照和恢复引用；也保护已撤销检出模块的缓存 HEAD。失败保留列表与实际现场，详见 [子模块管理说明](docs/git-submodules.md)。

完整 Git 客户端仍在开发：历史文件树与补丁导出、重命名等特殊差异的部分操作、交互式变基、可视化冲突解决、子模块高级维护、批量替换恢复浏览器及非 Git 编辑根目录等尚未全部接入。完整范围及验收清单见 [工作台验收清单](docs/workbench-acceptance.md)。本节工作台与下方的只读版本比较面板分别提供功能。

### Git 提交 / 暂存区 / 工作区比较

“Git”面板选择本地仓库，输入两个提交、分支或标签（例如 `HEAD~1` / `HEAD`），即可搜索版本树、筛选差异并查看 UTF-8/UTF-16 文本。引用在比较时解析为完整提交 ID，后续分支移动不会切换已经打开的结果。可执行位变化也计为修改；一致使用 Git 对象身份判定，未运行额外的 SHA-256 全树读取。

每侧可选择“暂存区索引”，按捕获时的对象 ID 比较已索引版本，后续暂存或工作区修改不会切换已打开的结果。未合并的 stages 1–3 标为错误；intent-to-add 显示索引中的空对象，不读取工作区正文。两侧都选索引时共用同一次捕获；空仓库也可浏览索引。分离的工作树使用各自索引，裸仓库不支持索引模式。详细语义见 [Git 比较说明](docs/git-comparison.md)。

工作区来源读取当前文件的原始字节，可与提交或暂存区对照；默认只包含索引跟踪路径，可显式纳入遵循 Git 忽略规则的未跟踪文件。不会运行 clean/smudge/CRLF 转换，因此结果可能与 Git 的规范化差异不同。逐文件流式计算 Git blob 身份及 SHA-256；打开文本预览会重新验证，外部修改要求刷新。稀疏检出中实际缺失的文件显示为缺失，不以索引正文替代。读取失败、链接和类型冲突保持区分。

需要 PATH 中安装支持 `--no-lazy-fetch` 的 Git（本地已验证 Git 2.50.1）；不支持该参数会报错。只读提交或索引所指向的对象，仅工作区模式读取当前文件正文，不执行暂存、检出、提交或下载缺失对象。符号链接不跟随，子模块不展开，Git LFS 比较指针文本。树输出限制 64 MiB、比较最多 200,000 条路径，单条命令限时 15 秒，文本预览限制 4 MiB。该只读面板不执行写操作；Git 工作台写操作见上节。Git difftool/mergetool 注册和 SVN 尚未实现。

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
npm ci
npm run build:editor
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cargo install tauri-cli --version '^2' --locked
cargo tauri build
```

Packages are under `target/release/bundle/`. Install Node.js 22 and run `npm ci` before building. A Trunk pre-build hook bundles the pinned CodeMirror editor locally with esbuild; Trunk then builds Leptos CSR and Tauri bundles both with the Rust executable. The client does not download editor assets at runtime. Generated JavaScript loads WASM and bridges desktop calls. The workspace defaults to two compile jobs.

`cargo tauri dev` starts the desktop client and a local frontend watcher on port1420; stop both when finished. For a frontend build without running a service:

```sh
trunk --no-color build
```

## Help and presentation

The top-bar Help / theme button opens searchable offline guidance in Chinese or English for current workflows, constraints, backups/recovery and keyboard shortcuts. Choose System, Light or Dark; System follows the operating system's color preference, including changes while the app is open. Themes use semantic colors across comparison/editor/dialog panels. Reduced-motion behavior follows the OS preference.

Theme selection is stored in this client's WebView local storage, separately from native named sessions. It stores only a version number and theme, with no paths or compared contents. A storage/validation failure falls back to System and shows a message in Help; changing the theme explicitly retries persistence. Session language behavior remains unchanged. Custom key bindings and an updater are still planned. Packaged-client display and interaction acceptance remains deferred until feature implementation is complete.

## CLI

```sh
cargo build -p clear-cli --release --locked
./target/release/clear-compare compare ./source ./target --exclude '-./.git/;-node_modules/'

# Binary pages and streaming search (inspect next_offset for continuation).
./target/release/clear-compare hex ./old.bin ./new.bin --offset 0x1000
./target/release/clear-compare binary-search ./file.bin 'DE AD BE EF' --mode hex --limit 256

# Explicit regular-file deletion: preview by default; add --apply to execute a fresh plan.
./target/release/clear-compare delete ./source ./target --side right --path old.txt --backup-dir ./backups

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
./target/release/clear-compare git-compare ./repository --right-index
./target/release/clear-compare git-compare ./repository --left-index --right-worktree --include-untracked
./target/release/clear-compare report HEAD ignored ./staged.json --source git --repository ./repository --right-index --format json

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

# HTML catalog / independent table selection / mixed HTML and CSV comparison.
./target/release/clear-compare table ./left.html ./right.csv --list-tables
./target/release/clear-compare table ./left.csv ./right.csv --align-columns-by-name --key-column 0
./target/release/clear-compare table ./left.html ./right.csv --left-html 1 --key-column 0 --numeric-tolerance 0.01
# Force delimited text even if a filename ends in .html.
./target/release/clear-compare table ./left.html ./right.csv --delimited

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
- Merge results are buffers with conflict markers, not automatic file writes. Two-second diff deadlines can yield coarser alignment on pathological inputs. Hex viewing uses fixed 4 KiB pages and per-byte differences. Hex/UTF-8/UTF-16 search supports overlapping matches, bounded scanning and continuation; [binary details](docs/binary-compare.md) define consistency and CLI exit codes. Hex editing is not implemented.
- Editor backups: app data `editor-backups/<id>/original.bin` and a digest/state `manifest.json`. Original encoding/BOM and uniform newline style are retained by default; explicit conversion is available. The editor can preview and undo its most recent save, backing up the current contents before restoring. CLI can reopen matching completed editor backups across process restarts; manually inspect unresolved merge markers before saving.
- Scan: at most200,000 entries per root and500 rows per page. Search, difference filtering and sorting operate across the entire selected directory scope before pagination. Folder navigation uses opaque native IDs and breadcrumbs; optional recursion includes descendants. Directories remain before files in both ascending and descending sorts.
- Desktop backups: app data directory, `backups/<plan-id>/files/`, with `journal.json`. CLI uses the selected backup base.
- Reviewed forward restart of new sync journals is available from operation history or `resume-sync`; immutable intent, identity/content checks and per-attempt backups preserve the original rollback history. Startup does not automatically apply it. Recovery-journal resume/redo, editor-backup browsing, timestamp/ACL/xattr preservation and full encoding support remain planned. See [forward restart](docs/operation-resume.md).
- Copy/deletion history is available from the desktop “Operation history” button after restart. Reviewed rollback restores overwritten/deleted files and removes only hash-matching created files, backing up current files before replacement/removal. Deleted paths must still be absent; restored files use no-clobber persistence. `running`/`failed` journal entries are reconciled against actual contents; conflicting changes block recovery.
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
