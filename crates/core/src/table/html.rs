//! Bounded HTML5 static-table extraction. No scripting, CSS evaluation or resource loading.
use crate::{Error, Result, compare::check_cancel};
use clear_contracts::HtmlTableInfo;
use html5ever::tendril::{StrTendril, TendrilSink};
use scraper::{ElementRef, Html, HtmlTreeSink, Node};
use std::{
    collections::BTreeMap,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
const MAX_HTML: usize = 1024 * 1024;
const MAX_NODES: usize = 100000;
const MAX_TABLES: usize = 64;
const MAX_TEXT: usize = 8 * 1024 * 1024;
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn tag(e: ElementRef<'_>, name: &str) -> bool {
    e.value().name.ns.as_ref() == "http://www.w3.org/1999/xhtml" && e.value().name() == name
}
fn hidden(e: ElementRef<'_>) -> bool {
    e.value().attr("hidden").is_some()
        || e.ancestors()
            .filter_map(ElementRef::wrap)
            .any(|p| p.value().attr("hidden").is_some() || tag(p, "template"))
}
fn text(e: ElementRef<'_>, cancel: &AtomicBool) -> Result<String> {
    if hidden(e) {
        return Ok(String::new());
    }
    let mut output = String::new();
    let mut stack = e.children().rev().map(|n| (n, false)).collect::<Vec<_>>();
    while let Some((node, exit)) = stack.pop() {
        if exit {
            output.push(' ');
            continue;
        }
        check_cancel(cancel)?;
        match node.value() {
            Node::Text(t) => output.push_str(t),
            Node::Element(_) => {
                let e = ElementRef::wrap(node).unwrap();
                if hidden(e)
                    || ["script", "style", "template", "table"]
                        .iter()
                        .any(|name| e.value().name() == *name)
                {
                    output.push(' ');
                    continue;
                }
                if ["br", "p", "div", "li", "ul", "ol", "pre", "h1", "h2", "h3"]
                    .iter()
                    .any(|name| tag(e, name))
                {
                    output.push(' ');
                    stack.push((node, true));
                }
                stack.extend(node.children().rev().map(|n| (n, false)));
            }
            _ => {}
        }
        if output.len() > MAX_HTML {
            return Err(invalid("HTML cell text exceeds 1 MiB"));
        }
    }
    if output.len() > MAX_HTML {
        return Err(invalid("HTML cell text exceeds 1 MiB"));
    }
    // HTML ASCII layout whitespace collapses; NBSP and other Unicode characters remain data.
    let mut result = String::new();
    let mut space = false;
    for c in output.chars() {
        if c.is_ascii_whitespace() {
            space = !result.is_empty();
        } else {
            if space {
                result.push(' ');
            }
            result.push(c);
            space = false;
        }
    }
    Ok(result)
}
fn groups(table: ElementRef<'_>) -> Vec<Vec<ElementRef<'_>>> {
    let mut grouped = Vec::new();
    for e in table
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|e| !hidden(*e))
    {
        if tag(e, "tr") {
            grouped.push((1, vec![e]));
        } else if ["thead", "tbody", "tfoot"].iter().any(|n| tag(e, n)) {
            let rows = e
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|r| tag(*r, "tr") && !hidden(*r))
                .collect::<Vec<_>>();
            grouped.push((
                if tag(e, "thead") {
                    0
                } else if tag(e, "tfoot") {
                    2
                } else {
                    1
                },
                rows,
            ));
        }
    }
    grouped.sort_by_key(|(order, _)| *order);
    grouped.into_iter().map(|(_, rows)| rows).collect()
}
pub struct Document {
    document: Html,
    pub tables: Vec<HtmlTableInfo>,
}
impl Document {
    pub fn parse(input: &str, cancel: &AtomicBool) -> Result<Self> {
        if input.len() > MAX_HTML {
            return Err(invalid("HTML table input exceeds 1 MiB decoded UTF-8"));
        }
        check_cancel(cancel)?;
        let started = Instant::now();
        let mut options = html5ever::ParseOpts::default();
        options.tree_builder.scripting_enabled = false;
        let mut parser =
            html5ever::parse_document(HtmlTreeSink::new(Html::new_document()), options);
        let mut begin = 0;
        while begin < input.len() {
            check_cancel(cancel)?;
            if started.elapsed() > Duration::from_secs(2) {
                return Err(invalid("HTML parsing exceeded 2 seconds"));
            }
            let mut end = (begin + 256).min(input.len());
            while !input.is_char_boundary(end) {
                end -= 1;
            }
            parser.process(StrTendril::from_slice(&input[begin..end]));
            begin = end;
            if parser.tokenizer.sink.sink.0.borrow().tree.nodes().len() > MAX_NODES {
                return Err(invalid("HTML document exceeds 100000 nodes"));
            }
        }
        let document = parser.finish();
        if started.elapsed() > Duration::from_secs(2) {
            return Err(invalid("HTML parsing exceeded 2 seconds"));
        }
        if document.tree.nodes().len() > MAX_NODES {
            return Err(invalid("HTML document exceeds 100000 nodes"));
        }
        let mut stack = vec![(document.tree.root(), 0)];
        while let Some((node, depth)) = stack.pop() {
            check_cancel(cancel)?;
            if depth > 256 {
                return Err(invalid("HTML nesting exceeds 256 levels"));
            }
            stack.extend(node.children().map(|c| (c, depth + 1)));
        }
        let mut tables = Vec::new();
        for table in document
            .tree
            .root()
            .descendants()
            .filter_map(ElementRef::wrap)
            .filter(|e| tag(*e, "table") && !hidden(*e))
        {
            if tables.len() >= MAX_TABLES {
                return Err(invalid("HTML document exceeds 64 tables"));
            }
            let caption = table
                .children()
                .filter_map(ElementRef::wrap)
                .find(|e| tag(*e, "caption"))
                .map(|e| text(e, cancel))
                .transpose()?
                .unwrap_or_default();
            let rows = groups(table).iter().map(Vec::len).sum();
            tables.push(HtmlTableInfo {
                index: tables.len(),
                caption: caption.chars().take(256).collect(),
                element_id: table
                    .value()
                    .attr("id")
                    .unwrap_or_default()
                    .chars()
                    .take(256)
                    .collect(),
                rows,
            });
        }
        check_cancel(cancel)?;
        if started.elapsed() > Duration::from_secs(2) {
            return Err(invalid("HTML parsing exceeded 2 seconds"));
        }
        Ok(Self { document, tables })
    }
    pub fn selected(&self, index: usize, cancel: &AtomicBool) -> Result<Vec<Vec<String>>> {
        let table = self
            .document
            .tree
            .root()
            .descendants()
            .filter_map(ElementRef::wrap)
            .filter(|e| tag(*e, "table") && !hidden(*e))
            .nth(index)
            .ok_or_else(|| invalid("HTML table index does not exist; choose an available table"))?;
        let mut cells = BTreeMap::new();
        let mut bytes = 0;
        let mut start_row = 0usize;
        let mut width = 0usize;
        for group in groups(table) {
            let explicit = group.len();
            let mut height = explicit;
            let mut downward = Vec::new();
            for (r, row) in group.into_iter().enumerate() {
                check_cancel(cancel)?;
                let mut col = 0;
                for cell in row
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| tag(*e, "td") || tag(*e, "th"))
                {
                    while cells.contains_key(&(start_row + r, col)) {
                        col += 1;
                    }
                    let columns = span(cell, "colspan", false)?.clamp(1, 1000);
                    let rows = span(cell, "rowspan", true)?.min(65534);
                    let value = if hidden(cell) {
                        String::new()
                    } else {
                        text(cell, cancel)?
                    };
                    let covered = if rows == 0 { explicit - r } else { rows };
                    height = height.max(r + covered);
                    width = width.max(col + columns);
                    if start_row + height > super::MAX_ROWS || width > super::MAX_COLUMNS {
                        return Err(invalid("Expanded HTML row/column limit exceeded"));
                    }
                    for y in r..r + covered {
                        for x in col..col + columns {
                            insert(&mut cells, start_row + y, x, &value, &mut bytes)?;
                        }
                    }
                    if rows == 0 {
                        downward.push((r, col, columns, value));
                    }
                    col += columns;
                }
            }
            for (r, col, columns, value) in downward {
                for y in explicit.max(r)..height {
                    for x in col..col + columns {
                        insert(&mut cells, start_row + y, x, &value, &mut bytes)?;
                    }
                }
            }
            start_row += height;
        }
        if start_row.saturating_mul(width) > super::MAX_CELLS {
            return Err(invalid("Expanded HTML grid exceeds 200000 cells"));
        }
        let mut rows = Vec::new();
        for r in 0..start_row {
            check_cancel(cancel)?;
            let row = (0..width)
                .map(|c| cells.remove(&(r, c)).unwrap_or_default())
                .collect();
            rows.push(row);
        }
        Ok(rows)
    }
}
fn insert(
    cells: &mut BTreeMap<(usize, usize), String>,
    row: usize,
    col: usize,
    value: &str,
    bytes: &mut usize,
) -> Result<()> {
    if cells.len() >= super::MAX_CELLS || bytes.saturating_add(value.len()) > MAX_TEXT {
        return Err(invalid("Expanded HTML exceeds 200000 cells or 8 MiB text"));
    }
    if cells.contains_key(&(row, col)) {
        return Err(invalid("HTML cells overlap; table model is ambiguous"));
    }
    *bytes += value.len();
    cells.insert((row, col), value.into());
    Ok(())
}
fn span(cell: ElementRef<'_>, attribute: &str, zero: bool) -> Result<usize> {
    let Some(value) = cell.value().attr(attribute) else {
        return Ok(1);
    };
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid("HTML span must contain a nonnegative integer"));
    }
    let n = value
        .parse::<usize>()
        .map_err(|_| invalid("HTML span integer is too large"))?;
    if n == 0 && !zero { Ok(1) } else { Ok(n) }
}
