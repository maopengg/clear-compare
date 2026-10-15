//! Streaming, self-contained comparison reports; exports never replace existing files.
use crate::{Error, Result, compare::check_cancel};
use clear_contracts::{
    Comparison, Entry, Kind, ReportFormat, ReportRequest, ReportResult, Status, Summary,
};
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
const MAX_BYTES: u64 = 128 * 1024 * 1024;
struct Bounded<'a> {
    file: &'a mut File,
    bytes: u64,
    cancel: &'a AtomicBool,
}
impl Write for Bounded<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("Operation cancelled"));
        }
        if self.bytes.saturating_add(b.len() as u64) > MAX_BYTES {
            return Err(std::io::Error::other("Report exceeds 128 MiB"));
        }
        let n = self.file.write(b)?;
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
fn escape(w: &mut impl Write, text: &str) -> std::io::Result<()> {
    let mut start = 0;
    for (i, c) in text.char_indices() {
        let escaped = match c {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            '\'' => "&#39;",
            _ => continue,
        };
        w.write_all(&text.as_bytes()[start..i])?;
        w.write_all(escaped.as_bytes())?;
        start = i + c.len_utf8();
    }
    w.write_all(&text.as_bytes()[start..])
}
fn tr(en: bool, zh: &'static str, en_text: &'static str) -> &'static str {
    if en { en_text } else { zh }
}
fn state(s: &Status, en: bool) -> &'static str {
    match s {
        Status::Same => tr(en, "一致", "Same"),
        Status::Changed => tr(en, "已修改", "Changed"),
        Status::LeftOnly => tr(en, "仅左侧", "Left only"),
        Status::RightOnly => tr(en, "仅右侧", "Right only"),
        Status::MetadataMatch => tr(en, "内容未验证", "Unverified"),
        Status::TypeConflict => tr(en, "类型冲突", "Type conflict"),
        Status::Error => tr(en, "读取错误", "Read error"),
    }
}

fn basis(value: &clear_contracts::VerificationBasis, en: bool) -> &'static str {
    use clear_contracts::VerificationBasis::*;
    match value {
        Sha256 => tr(en, "SHA-256 内容摘要", "SHA-256 content digests"),
        SizeAndTime => tr(
            en,
            "大小与时间（正文未验证）",
            "Size and time (contents unverified)",
        ),
        SizeOnly => tr(en, "大小（正文未验证）", "Size (contents unverified)"),
        GitObjectIdentity => tr(
            en,
            "Git 对象身份与可执行位",
            "Git object identity and executable mode",
        ),
    }
}
fn summary(w: &mut impl Write, s: &Summary, en: bool) -> std::io::Result<()> {
    for (label, n) in [
        (tr(en, "一致", "Same"), s.same),
        (tr(en, "修改", "Changed"), s.changed),
        (tr(en, "仅左侧", "Left only"), s.left_only),
        (tr(en, "仅右侧", "Right only"), s.right_only),
        (tr(en, "未验证", "Unverified"), s.unverified),
        (tr(en, "错误/冲突", "Errors/conflicts"), s.errors),
    ] {
        write!(w, "<li>{label}: <strong>{n}</strong></li>")?;
    }
    Ok(())
}
fn entry(w: &mut impl Write, e: Option<&Entry>, en: bool) -> std::io::Result<()> {
    w.write_all(b"<td>")?;
    if let Some(e) = e {
        let kind = match e.kind {
            Kind::File => tr(en, "文件", "File"),
            Kind::Directory => tr(en, "目录", "Directory"),
            Kind::Link => tr(en, "链接（不跟随）", "Link (not followed)"),
            Kind::Other => tr(en, "其他", "Other"),
        };
        write!(w, "{kind}<br>")?;
        escape(w, &e.bytes)?;
        w.write_all(b" B")?;
        if let Some(ms) = &e.modified_ms {
            write!(
                w,
                "<br><small>{}: ",
                tr(en, "修改时间（Unix 毫秒）", "Modified (Unix ms)")
            )?;
            escape(w, ms)?;
            w.write_all(b"</small>")?;
        }
        if let Some(hash) = &e.digest {
            w.write_all(b"<br><code>")?;
            escape(w, hash)?;
            w.write_all(b"</code>")?;
        }
        if let Some(error) = &e.error {
            w.write_all(b"<p class=error>")?;
            escape(w, error)?;
            w.write_all(b"</p>")?;
        }
    } else {
        w.write_all("—".as_bytes())?;
    }
    w.write_all(b"</td>")
}
fn html(
    w: &mut impl Write,
    report: &Comparison,
    request: &ReportRequest,
) -> std::io::Result<usize> {
    let en = request.english;
    write!(
        w,
        "<!doctype html><html lang=\"{}\"><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><meta http-equiv=Content-Security-Policy content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>Clear Compare</title>",
        if en { "en" } else { "zh-CN" }
    )?;
    w.write_all(b"<style>body{font:14px/1.6 system-ui,sans-serif;color:#20313b;background:#fff;max-width:1500px;margin:auto;padding:24px}h1{font-size:24px}p,td,code{overflow-wrap:anywhere}ul{display:flex;gap:20px;flex-wrap:wrap;padding:0;list-style:none}table{border-collapse:collapse;width:100%}th,td{border:1px solid #bac4cc;padding:8px;text-align:left;vertical-align:top}th{background:#f1f5f7}code,small{font-size:11px}pre{white-space:pre-wrap;overflow-wrap:anywhere}.error{color:#962b2b}.changed{background:#fff1d9}.uncertain{background:#f0f4f7}caption{text-align:left;margin:18px 0;font-weight:600}@media print{body{padding:0}thead{display:table-header-group}tr{break-inside:avoid}}</style>")?;
    write!(
        w,
        "<h1>Clear Compare · {}</h1><p>{}</p><p><strong>{}</strong>: ",
        tr(en, "比较报告", "Comparison report"),
        tr(
            en,
            "历史比较结果，不会重新扫描或证明文件当前状态。报告不包含文件正文，未验证与错误不会被当成一致。",
            "Historical comparison result; no rescan or proof of current filesystem state. No file bodies are included. Unverified/error entries do not establish equality."
        ),
        tr(en, "左侧", "Left")
    )?;
    escape(w, &report.left)?;
    write!(w, "<br><strong>{}</strong>: ", tr(en, "右侧", "Right"))?;
    escape(w, &report.right)?;
    write!(w, "<br><small>{}: ", tr(en, "比较 ID", "Comparison ID"))?;
    escape(w, &report.session)?;
    w.write_all(b"</small></p><ul>")?;
    summary(w, &report.summary, en)?;
    w.write_all(b"</ul>")?;
    if let Some(policy) = &report.policy {
        write!(
            w,
            "<p>{}: {}</p><details><summary>{}</summary><pre>",
            tr(en, "比较依据", "Verification basis"),
            basis(&policy.verification, en),
            tr(en, "本次排除规则", "Comparison exclusions")
        )?;
        escape(w, &policy.exclusions)?;
        w.write_all(b"</pre></details>")?;
    } else {
        write!(
            w,
            "<p>{}</p>",
            tr(
                en,
                "旧结果未记录比较规则。",
                "This older result did not record its comparison policy."
            )
        )?;
    }
    for warning in &report.warnings {
        w.write_all(b"<p class=uncertain>")?;
        escape(w, warning)?;
        w.write_all(b"</p>")?;
    }
    let rows = report
        .rows
        .iter()
        .filter(|r| !request.differences_only || r.status != Status::Same)
        .count();
    write!(
        w,
        "<table><caption>{}: {rows} / {} · {}</caption><thead><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
        tr(en, "导出行数", "Exported rows"),
        report.rows.len(),
        tr(
            en,
            "汇总为完整比较统计，不因筛选而改变",
            "Summary covers the full comparison, regardless of filtering"
        ),
        tr(en, "相对路径", "Relative path"),
        tr(en, "状态", "Status"),
        tr(en, "左侧元数据", "Left metadata"),
        tr(en, "右侧元数据", "Right metadata")
    )?;
    for row in report
        .rows
        .iter()
        .filter(|r| !request.differences_only || r.status != Status::Same)
    {
        let class = match row.status {
            Status::Error | Status::TypeConflict => "error",
            Status::MetadataMatch => "uncertain",
            Status::Same => "same",
            _ => "changed",
        };
        write!(w, "<tr class={class}><td>")?;
        escape(w, &row.path)?;
        if let Some(s) = &row.directory_summary {
            write!(
                w,
                "<details><summary>{}</summary><ul>",
                tr(en, "子项汇总", "Descendant summary")
            )?;
            summary(w, s, en)?;
            w.write_all(b"</ul></details>")?;
        }
        write!(w, "</td><td>{}</td>", state(&row.status, en))?;
        entry(w, row.left.as_ref(), en)?;
        entry(w, row.right.as_ref(), en)?;
        w.write_all(b"</tr>")?;
    }
    w.write_all(b"</tbody></table></html>")?;
    Ok(rows)
}
pub fn export(
    report: &Comparison,
    request: &ReportRequest,
    protected_roots: &[PathBuf],
    cancel: &AtomicBool,
) -> Result<ReportResult> {
    check_cancel(cancel)?;
    if request.session != report.session {
        return Err(Error::Invalid("Report comparison expired".into()));
    }
    let destination = Path::new(&request.destination);
    let name = destination
        .file_name()
        .ok_or_else(|| Error::Invalid("Choose a report filename".into()))?;
    let parent = fs::canonicalize(
        destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    if !parent.is_dir() || protected_roots.iter().any(|root| parent.starts_with(root)) {
        return Err(Error::Invalid(
            "Save reports outside the compared directories/repository".into(),
        ));
    }
    let destination = parent.join(name);
    if destination.try_exists()? || fs::symlink_metadata(&destination).is_ok() {
        return Err(Error::Invalid(
            "Report destination exists. Choose a new file.".into(),
        ));
    }
    let mut temp = tempfile::NamedTempFile::new_in(&parent)?;
    let result = (|| -> Result<(usize, u64)> {
        let mut bounded = Bounded {
            file: temp.as_file_mut(),
            bytes: 0,
            cancel,
        };
        let rows;
        {
            let mut writer = BufWriter::new(&mut bounded);
            rows = match request.format {
                ReportFormat::Html => html(&mut writer, report, request)?,
                ReportFormat::Json => {
                    // Keep the full comparison schema with filtered rows and original summary.
                    #[derive(serde::Serialize)]
                    struct Filtered<'a> {
                        schema: &'static str,
                        version: u32,
                        source: clear_contracts::ReportSource,
                        policy: Option<&'a clear_contracts::ComparisonPolicy>,
                        filtered: bool,
                        session: &'a str,
                        left: &'a str,
                        right: &'a str,
                        summary: &'a Summary,
                        rows: Vec<&'a clear_contracts::Row>,
                        warnings: &'a [String],
                    }
                    let rows = report
                        .rows
                        .iter()
                        .filter(|r| !request.differences_only || r.status != Status::Same)
                        .collect::<Vec<_>>();
                    let count = rows.len();
                    serde_json::to_writer_pretty(
                        &mut writer,
                        &Filtered {
                            schema: "clear-compare-report",
                            version: 1,
                            source: request.source,
                            policy: report.policy.as_ref(),
                            filtered: request.differences_only,
                            session: &report.session,
                            left: &report.left,
                            right: &report.right,
                            summary: &report.summary,
                            rows,
                            warnings: &report.warnings,
                        },
                    )?;
                    count
                }
            };
            writer.flush()?;
        }
        Ok((rows, bounded.bytes))
    })();
    let (rows, bytes) = result.map_err(|e| {
        if cancel.load(Ordering::Relaxed) {
            Error::Cancelled
        } else {
            e
        }
    })?;
    temp.as_file().sync_all()?;
    check_cancel(cancel)?;
    temp.persist_noclobber(&destination)
        .map_err(|e| Error::Io(e.error))?;
    Ok(ReportResult {
        path: destination.display().to_string(),
        rows,
        bytes: bytes.to_string(),
        session: report.session.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_limit_and_mid_write_cancellation_do_not_retry_forever() {
        let mut file = tempfile::tempfile().unwrap();
        let cancel = AtomicBool::new(false);
        let mut writer = Bounded {
            file: &mut file,
            bytes: MAX_BYTES - 4,
            cancel: &cancel,
        };
        writer.write_all(b"1234").unwrap();
        assert!(writer.write_all(b"5").is_err());
        assert_eq!(writer.file.metadata().unwrap().len(), 4);
        cancel.store(true, Ordering::Relaxed);
        assert!(writer.write_all(b"cancel").is_err());
        assert_eq!(writer.file.metadata().unwrap().len(), 4);
    }
}
