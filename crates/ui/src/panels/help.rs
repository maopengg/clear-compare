//! Offline, searchable help for implemented workflows; no network navigation.
use crate::{
    preferences::{self, Theme},
    tr,
};
use leptos::prelude::*;
#[derive(Clone, Copy, PartialEq)]
struct Topic {
    zh: &'static str,
    en: &'static str,
    body_zh: &'static str,
    body_en: &'static str,
}
const TOPICS: &[Topic] = &[
    Topic {
        zh: "文件夹比较",
        en: "Folder comparison",
        body_zh: "选择两个独立文件夹后点击比较。勾选 SHA-256 才会按内容验证；仅大小和时间相同显示为未验证。链接、类型冲突和读取失败会单独标识。目录状态汇总子项，文件统计不计目录。\n搜索、排序、仅差异和递归设置作用于当前目录范围的全部结果，再分页显示；点击目录可进入，面包屑可返回。列表是扫描结果，外部修改后需重新比较。",
        body_en: "Choose separate directories and compare. Enable SHA-256 for content verification; matching size/time alone remains unverified. Links, type conflicts and read errors remain distinct. Directory status aggregates descendants; file totals exclude directories.\nSearch, sorting, differences-only and recursion apply to the entire directory scope before paging. Enter folders and return through breadcrumbs. Results describe the scan; compare again after external changes.",
    },
    Topic {
        zh: "排除规则",
        en: "Exclusion rules",
        body_zh: "规则用分号或换行分隔，可带前缀 -。./ 表示从根目录匹配，尾部 / 只匹配目录；裸名称匹配任意层级。示例：-./.git/;-node_modules/;-README.md。\n被排除的目录不会继续扫描。当前支持排除规则，不支持包含规则优先级。会话可保存路径和规则，请勿把私有路径或比较数据上传到公开问题中。",
        body_en: "Separate rules with semicolons or newlines; an optional - prefix is accepted. ./ anchors a pattern at the root; a trailing / matches directories. Bare names match at any depth. Example: -./.git/;-node_modules/;-README.md.\nExcluded directories are pruned. Inclusion priorities are not implemented. Sessions can retain paths and rules; keep private paths and comparison data out of public issues.",
    },
    Topic {
        zh: "复制、备份和恢复",
        en: "Copy, backups and recovery",
        body_zh: "选择可读普通文件并明确复制方向，先预览再执行。操作菜单也可明确删除左侧或右侧所选普通文件，先验证备份再移除；目录不删除。重命名模式恰好选择一个普通文件，输入同目录的新文件名；支持仅大小写变化，拒绝覆盖已有名称。编辑器跟随新路径，未保存内容与撤销记录保留。来源和目标会重新校验，覆盖前创建并验证备份，执行状态写入日志。普通复制保留目标独有文件。文件镜像模式按整个比较范围复制/备份删除，不受筛选或勾选影响，排除项保留；需要内容校验，空目录保留，链接/错误/类型冲突阻止计划。当前没有完整目录镜像或双向同步策略。\n取消或错误会停止后续文件，先前操作可能已完成。操作历史可在重启后打开，预览回退再执行；删除文件可恢复到仍为空的原路径，已重新创建的路径不覆盖。重命名也可改回原名或从日志临时名称恢复，原名被占用则拒绝执行。外部修改会阻止不安全恢复。备份目录和日志需一起保留，当前不支持自动续传、恢复重做或跨系统移动日志。",
        body_en: "Select readable regular files and an explicit direction, then review the copy plan before applying. The operation menu can also explicitly delete selected regular files on either side after verified backups; directories remain. Rename mode requires exactly one regular file and a new name within the same directory. Case-only changes are supported; existing names are never overwritten. Editors follow the new path while retaining drafts and undo history. Sources and targets are revalidated, overwrite backups are verified, and action states are journaled. Ordinary copy retains destination-only files. File mirror copies/deletes across the entire comparison regardless of filters or selection, retaining exclusions and empty directories. It requires content verification and rejects links/errors/type conflicts. Exact directory-tree mirroring and bidirectional policies are not implemented.\nCancellation/errors stop later actions; earlier actions may have completed. Reopen operation history after restart, preview rollback, then apply. Deleted files restore only to absent paths without overwriting recreated files. Renames can be reversed or restored from their journaled temporary name; occupied original names block recovery. External changes block unsafe recovery. Keep journals with their backup directories; automatic resume, redo and cross-OS journal migration are not implemented.",
    },
    Topic {
        zh: "文本比较和编辑",
        en: "Text comparison and editing",
        body_zh: "支持 UTF-8 与带 BOM 的 UTF-16 LE/BE。大小写、行内空白和行尾规则只改变比较方式，原文保留。差异导航跨全部结果；查找区分大小写，按原文匹配行定位。每页最多 500 行，单行显示最多 8192 个 Unicode 字符，截断会标识。\n编辑器保留或显式转换编码、BOM 和换行；混合换行保存时需明确选择。保存检查原文件摘要，先验证备份，再原子替换并校验。撤销/重做有内存和步数上限；未保存内容应在关闭前处理。",
        body_en: "Supports UTF-8 and BOM-marked UTF-16 LE/BE. Case, within-line whitespace and line-ending rules affect comparison while preserving source text. Change navigation covers all results; literal search is case-sensitive and locates matching original-text rows. Pages contain up to 500 rows; displayed lines clip after 8192 Unicode characters with an indicator.\nThe editor retains or explicitly converts encoding, BOM and newlines; mixed endings require an explicit save policy. Save checks the original digest, verifies a backup, atomically replaces and verifies the result. Undo/redo has memory/step limits. Handle unsaved changes before closing.",
    },
    Topic {
        zh: "文本与目录合并",
        en: "Text and folder merge",
        body_zh: "文本三路合并使用基线、左侧和右侧缓冲区，独立修改自动合并。冲突可定位并选择原始版本，也可手工编辑；带标签的冲突未解决时，普通保存会被阻止。生成结果是缓冲区，不会自动写回文件。\n目录合并使用独立且为空的输出目录。预览删除/修改、可能的重命名/修改冲突后选择版本，再执行。输入会重新验证，输出有执行日志；中断可能留下部分结果，当前不能原地合并或自动清理/续传。",
        body_en: "Three-way text merge uses base, left and right buffers and combines independent changes. Navigate conflicts, choose original versions or edit manually; ordinary save blocks unresolved tagged conflicts. Generated results are buffers, not automatic writes.\nFolder merge requires a separate empty output directory. Review delete/modify and probable rename/modify conflicts, choose versions and apply. Inputs are revalidated and output is journaled. Interruption can leave partial output; in-place merge and automatic cleanup/resume are not implemented.",
    },
    Topic {
        zh: "表格比较",
        en: "Table comparison",
        body_zh: "支持 CSV、TSV 和静态 HTML 表格，可分别选择两侧格式和 HTML 表格序号。HTML 只读取静态文本，不运行脚本、CSS 或网络请求；合并单元格按网格重复值。表头为第一逻辑行；可按精确唯一列名对齐不同列顺序，此时主键序号按左列再右侧独有列的映射填写。支持唯一行键与绝对数值容差。\n打开后使用冻结的原文调整规则，外部修改不会混入同一次表格会话。结果按行分页，每次显示 25 列。缺列、缺单元格与空值分别显示。Excel、任意重命名列映射和表格编辑尚未实现。",
        body_en: "Supports CSV, TSV and static HTML tables with independent formats/table indices. HTML reads static text without scripts, CSS evaluation or network requests; spanned values repeat in covered grid cells. The first logical row can be headers. Exact unique header-name alignment handles reordered columns; key indices then use the left-then-right-only logical union. Unique row keys and absolute numeric tolerance are supported.\nRule changes use frozen source text after opening, so external edits do not mix into that table session. Rows are paged and columns shown in windows of 25. Missing columns/cells remain distinct from empty strings. Excel, arbitrary renamed-column mapping and table editing are not implemented.",
    },
    Topic {
        zh: "二进制比较",
        en: "Binary comparison",
        body_zh: "十六进制与 ASCII 每页最多 4096 字节，下划线标识逐字节差异。位置支持十进制或 0x 十六进制，跳转到包含该位置的一页。查找可选 Hex、UTF-8、UTF-16 LE/BE，支持重叠及跨块匹配，不自动添加 BOM。\n每次最多检查 64 MiB 候选位置并返回 256 个结果；注意显示的范围，必要时继续查找。点击匹配可定位并高亮。实时读取只核对大小和修改时间，不能证明整个文件内容一致。当前不能编辑/替换字节或对齐插入段。",
        body_en: "Hex/ASCII pages contain up to 4096 bytes; underlines mark byte differences. Offsets accept decimal or 0x hex and open the containing page. Search Hex, UTF-8 or UTF-16 LE/BE bytes with overlapping/cross-block matches; no BOM is added.\nEach call checks up to 64 MiB of candidate positions and returns 256 matches. Inspect the displayed range and continue when needed. Click a match to locate/highlight it. Live reads check size/time only and cannot establish whole-file equality. Byte editing/replacement and inserted-byte alignment are not implemented.",
    },
    Topic {
        zh: "图片比较",
        en: "Picture comparison",
        body_zh: "支持 PNG、JPEG、BMP 和 WebP，按 EXIF 方向归一化后比较 RGBA 像素，可设置通道容差和忽略透明度。原图、叠加和差异热图帮助定位，摘要显示完整像素统计与差异边界。\n预览可能缩小，比较仍基于完整解码像素。单侧最多 8 百万像素、16 MiB 编码输入；不进行 ICC 色彩管理、动画或感知相似度判断。",
        body_en: "Supports PNG, JPEG, BMP and WebP, comparing EXIF-oriented RGBA pixels with channel tolerance and optional ignored alpha. Source, overlay and heatmap previews help locate changes; summaries cover full pixel counts and difference bounds.\nPreviews may be reduced while comparison uses full decoded pixels. Limits include 8 million pixels and 16 MiB encoded bytes per side. ICC color management, animation and perceptual similarity are not implemented.",
    },
    Topic {
        zh: "归档、快照和 Git",
        en: "Archives, snapshots and Git",
        body_zh: "ZIP 目前只读，支持常规 Stored/Deflate；不解压到源目录，不支持 ZIP64、加密、多卷、嵌套更新等。目录快照只含路径、类型、大小、时间与错误信息，不含正文或摘要，因此历史元数据匹配不代表内容相同。\nGit 可对照提交、暂存区索引和工作区原始文件，不暂存、不检出、不拉取、不执行过滤/换行转换。未跟踪文件须显式纳入，并遵循 Git 忽略规则。工作区预览会重新校验已捕获摘要；外部修改要求刷新。未合并索引标错，intent-to-add 显示索引空对象。支持对象身份、可执行位差异与有界文本预览；Git LFS 指针不展开，子模块单独标错。稀疏检出中实际缺失的工作文件保持缺失。远程存储尚未实现。",
        body_en: "ZIP is read-only and supports classic Stored/Deflate entries without extracting into source directories. ZIP64, encryption, multi-volume and nested updates are unsupported. Directory snapshots retain names/types/sizes/times/errors without bodies or digests, so historical metadata matches do not establish content equality.\nGit compares commits, captured index object IDs and raw working files without staging, checkout, fetching or clean/smudge/EOL conversion. Untracked inclusion is explicit and follows Git ignores. Working previews verify captured digests; edits require refresh. Unmerged index paths are errors; intent-to-add uses indexed empty objects. It compares object identity/executable modes and offers bounded text previews. LFS pointers are not expanded; submodules are flagged. Sparse missing working files remain absent. Remote storage is not implemented.",
    },
    Topic {
        zh: "会话与报告",
        en: "Sessions and reports",
        body_zh: "会话库可命名保存路径、排除和视图设置，并通过版本号避免覆盖更新；归档不等于删除比较源。当前不会恢复扫描结果或完整工作区。\n本地目录、ZIP、快照与 Git 结果可导出 HTML/JSON；报告必须写到比较根目录之外的新文件。HTML 自包含且没有脚本；内容可能含路径与错误信息，分享前自行检查。CLI 输出 JSON，退出码和 next_offset 等字段应结合命令文档解释。",
        body_en: "The named session library stores paths/rules/view settings and uses revisions to avoid lost updates. Archiving a session does not delete comparison sources. Cached scan results and the full workspace are not restored.\nLocal-directory, ZIP, snapshot and Git results can export HTML/JSON to new files outside comparison roots. HTML is self-contained without scripts. Reports can expose paths/errors; review before sharing. Interpret CLI exit codes and fields such as next_offset using each command's documented contract.",
    },
    Topic {
        zh: "键盘与显示设置",
        en: "Keyboard and presentation",
        body_zh: "文本比较区获得焦点后：F7 下一差异，Shift+F7 上一差异；查找框 Enter 执行查找。编辑器与合并结果框内：Ctrl/Cmd+Z 撤销，Ctrl/Cmd+Shift+Z 或 Ctrl/Cmd+Y 重做；输入法组字时不会抢占这些快捷键。保存使用明确的保存按钮。\n主题可选跟随系统、浅色和深色，保存在当前客户端的 WebView 存储中；存储失败会显示提示。语言默认中文，可切换英文，现有会话继续记录其语言设置。遵循系统减少动态效果偏好；快捷键自定义尚未实现。",
        body_en: "With the text-comparison panel focused: F7 next change, Shift+F7 previous; Enter in its search box runs search. In editor/merge output: Ctrl/Cmd+Z undo, Ctrl/Cmd+Shift+Z or Ctrl/Cmd+Y redo; IME composition does not trigger them. Use the explicit Save button to save.\nThemes are System, Light and Dark, stored in this client's WebView; storage failure displays a warning. Chinese is the default with an English switch; existing sessions retain their language setting. System reduced-motion preferences are respected. Custom keyboard bindings are not implemented.",
    },
];
pub fn panel(
    english: RwSignal<bool>,
    visible: RwSignal<bool>,
    theme: RwSignal<Theme>,
    storage_error: RwSignal<String>,
) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let topics = Memo::new(move |_| {
        let q = query.get().trim().to_lowercase();
        TOPICS
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, t)| {
                [t.zh, t.en, t.body_zh, t.body_en]
                    .iter()
                    .any(|s| s.to_lowercase().contains(&q))
            })
            .collect::<Vec<_>>()
    });
    view! {<Show when=move ||visible.get()><section id="help-panel" class="help-panel" aria-labelledby="help-heading">
        <header><h2 id="help-heading">{move ||tr(english.get(),"帮助与显示设置","Help and presentation")}</h2><button class="quiet" on:click=move |_|visible.set(false)>{move ||tr(english.get(),"关闭帮助","Close help")}</button></header>
        <div class="help-controls"><label for="theme-choice">{move ||tr(english.get(),"主题","Theme")}</label><select id="theme-choice" prop:value=move ||theme.get().value() on:change=move |e|{let next=Theme::from_value(&event_target_value(&e));theme.set(next);storage_error.set(preferences::save(next).err().unwrap_or_default());}>
            <option value="system">{move ||tr(english.get(),"跟随系统","System")}</option><option value="light">{move ||tr(english.get(),"浅色","Light")}</option><option value="dark">{move ||tr(english.get(),"深色","Dark")}</option>
        </select><label for="help-search">{move ||tr(english.get(),"搜索帮助","Search help")}</label><input id="help-search" type="search" maxlength="256" prop:value=move ||query.get() on:input=move |e|query.set(event_target_value(&e))/></div>
        <Show when=move ||!storage_error.get().is_empty()><p class="help-storage-error" role="status">{move ||tr(english.get(),"主题存储提示：","Theme storage: ")}{move ||storage_error.get()}</p></Show>
        <p class="help-preview">{move ||tr(english.get(),"0.1 开发预览 · MIT。此帮助描述当前已实现的功能；完整计划和三平台安装验收尚未完成。转换器、远程/云存储等模块仍在开发计划中。","0.1 development preview · MIT. This guide describes implemented features. The full plan and three-platform installation acceptance remain incomplete; converters and remote/cloud storage are among planned modules.")}</p>
        <p class="help-count" aria-live="polite">{move ||format!("{} {}",topics.get().len(),tr(english.get(),"个主题","topics"))}</p>
        <div class="help-topics"><For each=move ||topics.get() key=|(i,_)|*i children=move |(_,topic)|view! {<details><summary>{move ||if english.get(){topic.en}else{topic.zh}}</summary><p>{move ||if english.get(){topic.body_en}else{topic.body_zh}}</p></details>}/></div>
    </section></Show>}
}
