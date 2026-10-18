//! Captured reflog selections and pinned stash trees; no checkout when browsing.
use super::*;
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn read(
    root: &Path,
    repository: &str,
    offset: usize,
    count: usize,
    cancel: &AtomicBool,
) -> Result<Vec<StashEntry>> {
    let exists = runner::optional(
        root,
        &[
            "rev-parse".into(),
            "--verify".into(),
            "--quiet".into(),
            "refs/stash".into(),
        ],
        &[],
        cancel,
    )?;
    if exists.is_empty() {
        return Ok(vec![]);
    }
    let raw = runner::run_owned(
        root,
        &[
            "reflog".into(),
            "show".into(),
            "-z".into(),
            "--format=%H%x00%gd%x00%gs%x00%an%x00%aI%x00%P".into(),
            format!("--skip={offset}"),
            format!("--max-count={count}"),
            "refs/stash".into(),
            "--".into(),
        ],
        cancel,
    )?;
    let mut fields = raw.split(|b| *b == 0).collect::<Vec<_>>();
    if fields.last() == Some(&b"".as_slice()) {
        fields.pop();
    }
    if fields.len() % 6 != 0 {
        return Err(invalid("Malformed stash listing"));
    }
    fields
        .as_chunks::<6>()
        .0
        .iter()
        .enumerate()
        .map(|(n, f)| {
            let text = |i: usize| String::from_utf8_lossy(f[i]).into_owned();
            let object = text(0);
            let selector = text(1);
            if !oid(&object) || selector != format!("stash@{{{}}}", offset + n) {
                return Err(invalid("Malformed stash identity"));
            }
            let parents = text(5)
                .split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>();
            if !(2..=3).contains(&parents.len()) || parents.iter().any(|p| !oid(p)) {
                return Err(invalid("Stash record has unsupported parents"));
            }
            let mut identity = repository.as_bytes().to_vec();
            for field in f {
                identity.push(0);
                identity.extend_from_slice(field);
            }
            Ok(StashEntry {
                id: id(&identity),
                position: offset + n,
                object,
                message: text(2),
                author: text(3),
                date: text(4),
                parents,
            })
        })
        .collect()
}
impl Workspace {
    pub fn stashes(
        &mut self,
        repository: &str,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<StashPage> {
        if offset > 10_000 {
            return Err(invalid("Stash page exceeds 10000-entry browsing limit"));
        }
        let state = self.open(repository, cancel)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let mut entries = read(&repo.root, repository, offset, 101, cancel)?;
        let has_more = entries.len() > 100;
        entries.truncate(100);
        if self.stash_entries.len() + entries.len() > 2048 {
            self.stash_entries.clear();
        }
        for entry in &entries {
            self.stash_entries
                .insert(entry.id.clone(), (repository.into(), entry.clone()));
        }
        Ok(StashPage {
            state,
            entries,
            offset,
            has_more,
        })
    }
    pub(super) fn stash_selection(
        &self,
        repository: &str,
        selection: &str,
        cancel: &AtomicBool,
    ) -> Result<StashEntry> {
        let (owner, entry) = self
            .stash_entries
            .get(selection)
            .ok_or_else(|| invalid("Stash selection expired; refresh the stash list"))?;
        if owner != repository {
            return Err(invalid("Stash belongs to another repository"));
        }
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let current = read(&repo.root, repository, entry.position, 1, cancel)?;
        if current.first().is_none_or(|e| e.id != selection) {
            return Err(invalid(
                "Stash order or identity changed; refresh before continuing",
            ));
        }
        Ok(entry.clone())
    }
    pub fn stash_detail(
        &mut self,
        repository: &str,
        selection: &str,
        area: &str,
        cancel: &AtomicBool,
    ) -> Result<StashDetail> {
        self.open(repository, cancel)?;
        let entry = self.stash_selection(repository, selection, cancel)?;
        let (target, parent) = match area {
            "tracked" => (entry.object.clone(), 0),
            "index" => (entry.parents[1].clone(), 0),
            "unstaged" => (entry.object.clone(), 1),
            "untracked" => (
                entry
                    .parents
                    .get(2)
                    .cloned()
                    .ok_or_else(|| invalid("This stash has no untracked-file tree"))?,
                0,
            ),
            _ => return Err(invalid("Unknown stash comparison area")),
        };
        let detail = self.commit_detail(repository, &target, parent, cancel)?;
        Ok(StashDetail {
            entry,
            area: area.into(),
            detail,
        })
    }
}
