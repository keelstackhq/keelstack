//! Pure text editors for release preparation: manifests, dependents and changelogs.

use super::commits::{ChangeEntry, Section};
use super::planner::NextVersion;
use crate::timeutil::{civil_from_days, now_secs};

/// A string member of a JSON object: its key path and the byte range of its content.
struct StrMember {
    path: Vec<String>,
    start: usize,
    end: usize,
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    out: Vec<StrMember>,
}

impl Scanner<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    /// Skip a string starting at the opening quote; returns the content range.
    fn string(&mut self) -> Result<(usize, usize), String> {
        let start = self.i + 1;
        self.i += 1;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return Ok((start, self.i - 1));
                }
                _ => self.i += 1,
            }
        }
        Err("invalid JSON: unterminated string".to_string())
    }

    fn value(&mut self, path: &mut Vec<String>, as_member: bool) -> Result<(), String> {
        self.ws();
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(());
                }
                loop {
                    self.ws();
                    if self.b.get(self.i) != Some(&b'"') {
                        return Err("invalid JSON: expected a key".to_string());
                    }
                    let (s, e) = self.string()?;
                    let key = String::from_utf8_lossy(&self.b[s..e]).into_owned();
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err("invalid JSON: expected `:`".to_string());
                    }
                    self.i += 1;
                    path.push(key);
                    self.value(path, true)?;
                    path.pop();
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(());
                        }
                        _ => return Err("invalid JSON: expected `,` or `}`".to_string()),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(());
                }
                loop {
                    self.value(path, false)?;
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(());
                        }
                        _ => return Err("invalid JSON: expected `,` or `]`".to_string()),
                    }
                }
            }
            Some(b'"') => {
                let (start, end) = self.string()?;
                if as_member {
                    self.out.push(StrMember {
                        path: path.clone(),
                        start,
                        end,
                    });
                }
                Ok(())
            }
            Some(_) => {
                let from = self.i;
                while self.i < self.b.len()
                    && !matches!(self.b[self.i], b',' | b'}' | b']')
                    && !self.b[self.i].is_ascii_whitespace()
                {
                    self.i += 1;
                }
                if self.i == from {
                    return Err("invalid JSON: unexpected character".to_string());
                }
                Ok(())
            }
            None => Err("invalid JSON: unexpected end".to_string()),
        }
    }
}

fn json_strings(text: &str) -> Result<Vec<StrMember>, String> {
    let mut s = Scanner {
        b: text.as_bytes(),
        i: 0,
        out: Vec::new(),
    };
    s.ws();
    if s.b.get(s.i) != Some(&b'{') {
        return Err("package.json must be a JSON object".to_string());
    }
    s.value(&mut Vec::new(), false)?;
    s.ws();
    if s.i != s.b.len() {
        return Err("invalid JSON: trailing characters after the root value".to_string());
    }
    Ok(s.out)
}

/// Decode the JSON string escapes of a string's raw content (`\"`, `\\`, `\n`, `\uXXXX`, ...).
/// A `\uD83D\uDE00` surrogate pair is one char; a lone surrogate (or a malformed `\u`) is
/// U+FFFD, and the escape after a lone high surrogate is decoded on its own.
fn json_unescape(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let hex4 = |at: usize| -> Option<u32> {
        let h: String = chars.get(at..at + 4)?.iter().collect();
        u32::from_str_radix(&h, 16).ok().filter(|_| h.len() == 4)
    };
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = chars.get(i) else { break };
        i += 1;
        match e {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'u' => {
                let Some(cp) = hex4(i) else {
                    out.push('\u{FFFD}');
                    i = chars.len().min(i + 4);
                    continue;
                };
                i += 4;
                if (0xD800..0xDC00).contains(&cp) {
                    let lo = (chars.get(i) == Some(&'\\') && chars.get(i + 1) == Some(&'u'))
                        .then(|| hex4(i + 2))
                        .flatten()
                        .filter(|lo| (0xDC00..0xE000).contains(lo));
                    if let Some(lo) = lo {
                        i += 6;
                        let pair = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                        out.push(char::from_u32(pair).unwrap_or('\u{FFFD}'));
                        continue;
                    }
                }
                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            }
            other => out.push(other),
        }
    }
    out
}

fn json_escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

fn splice(text: &str, edits: &[(usize, usize, String)]) -> String {
    let mut out = text.to_string();
    for (s, e, v) in edits.iter().rev() {
        out.replace_range(*s..*e, v);
    }
    out
}

/// Replace the top-level `"version"` string of a package.json; every other byte stays.
pub fn set_package_json_version(text: &str, v: &str) -> Result<String, String> {
    let members = json_strings(text)?;
    let mut found = members.iter().filter(|m| m.path == ["version"]);
    let m = found
        .next()
        .ok_or_else(|| "package.json has no top-level string `version`".to_string())?;
    if found.next().is_some() {
        return Err("package.json has several top-level `version` keys".to_string());
    }
    Ok(splice(text, &[(m.start, m.end, json_escape(v))]))
}

/// Set the requirement of `dep` in top-level `dependencies`, `optionalDependencies` and
/// `peerDependencies`; `workspace:` specifiers stay as they are.
pub fn set_package_json_dep(text: &str, dep: &str, req: &str) -> Result<String, String> {
    let members = json_strings(text)?;
    let mut found = false;
    let mut edits = Vec::new();
    for m in &members {
        if m.path.len() == 2
            && m.path[1] == dep
            && matches!(
                m.path[0].as_str(),
                "dependencies" | "optionalDependencies" | "peerDependencies"
            )
        {
            found = true;
            if !text[m.start..m.end].starts_with("workspace:") {
                edits.push((m.start, m.end, json_escape(req)));
            }
        }
    }
    if !found {
        return Err(format!("package.json does not depend on `{dep}`"));
    }
    Ok(splice(text, &edits))
}

/// Rewrite the range of `dep` with `f(old)` in the given top-level dependency `sections`
/// (changesets mode, changesets spec §5.2).
pub fn rewrite_package_json_dep(
    text: &str,
    dep: &str,
    sections: &[&str],
    f: impl Fn(&str) -> String,
) -> Result<String, String> {
    let edits: Vec<_> = json_strings(text)?
        .into_iter()
        .filter(|m| m.path.len() == 2 && m.path[1] == dep && sections.contains(&&*m.path[0]))
        .map(|m| {
            let old = json_unescape(&text[m.start..m.end]);
            (m.start, m.end, json_escape(&f(&old)))
        })
        .collect();
    if edits.is_empty() {
        return Err(format!("package.json does not depend on `{dep}`"));
    }
    Ok(splice(text, &edits))
}

fn toml_doc(text: &str) -> Result<toml_edit::DocumentMut, String> {
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("invalid TOML: {e}"))
}

fn set_string(item: &mut toml_edit::Item, v: &str) {
    // Keep the trailing comment and spacing of the old value.
    let decor = item.as_value().map(|o| o.decor().clone());
    let mut value = toml_edit::Value::from(v);
    if let Some(d) = decor {
        *value.decor_mut() = d;
    }
    *item = toml_edit::Item::Value(value);
}

/// Set `[package] version`. An inherited version (`version.workspace = true`) is an error:
/// it must be bumped in the root `Cargo.toml`.
pub fn set_cargo_version(text: &str, v: &str) -> Result<String, String> {
    let mut doc = toml_doc(text)?;
    match doc.get_mut("package").and_then(|p| p.get_mut("version")) {
        Some(item) if item.is_str() => set_string(item, v),
        Some(item)
            if item
                .as_table_like()
                .and_then(|t| t.get("workspace"))
                .and_then(|w| w.as_bool())
                == Some(true) =>
        {
            return Err(
                "the version is inherited from the workspace; bump it in the root Cargo.toml"
                    .to_string(),
            );
        }
        _ => return Err("Cargo.toml has no `[package] version`".to_string()),
    }
    Ok(doc.to_string())
}

type Req<'a> = &'a dyn Fn(&str) -> String;

const CARGO_KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// `found`: a path or workspace entry; `inherits`: one with `workspace = true`.
struct Hit {
    found: bool,
    inherits: bool,
}

fn update_dep_tables(
    table: &mut dyn toml_edit::TableLike,
    dep: &str,
    kinds: &[&str],
    v: Req,
    hit: &mut Hit,
) {
    for kind in kinds {
        let Some(deps) = table.get_mut(kind).and_then(|d| d.as_table_like_mut()) else {
            continue;
        };
        for (key, item) in deps.iter_mut() {
            let Some(t) = item.as_table_like_mut() else {
                continue;
            };
            let real = t.get("package").and_then(|p| p.as_str()).unwrap_or(&key);
            if real != dep {
                continue;
            }
            let ws = t.get("workspace").and_then(|w| w.as_bool()) == Some(true);
            if t.contains_key("path") || ws {
                hit.found = true;
                hit.inherits |= ws;
                if let Some(ver) = t.get_mut("version") {
                    let new = v(ver.as_str().unwrap_or(""));
                    set_string(ver, &new);
                }
            }
        }
    }
}

/// Set `version` of the path dependency `dep` (all dependency tables, `[workspace.dependencies]`
/// and `[target.*]` tables). A dependency without `version` or with `workspace = true` is kept.
pub fn set_cargo_dep_version(text: &str, dep: &str, v: &str) -> Result<String, String> {
    Ok(rewrite_cargo_dep(text, dep, &CARGO_KINDS, &|_| v.to_string())?.0)
}

/// `set_cargo_dep_version` limited to the `kinds` tables, with the new requirement `f(old)`;
/// also says whether an entry has `workspace = true`.
pub fn rewrite_cargo_dep(
    text: &str,
    dep: &str,
    kinds: &[&str],
    v: Req,
) -> Result<(String, bool), String> {
    rewrite_cargo(text, dep, kinds, v, false)
}

/// Rewrite `dep` in `[workspace.dependencies]` only; a root package's own `[dependencies]` and
/// `[target.*]` tables are left alone.
pub fn rewrite_workspace_dep(text: &str, dep: &str, v: Req) -> Result<String, String> {
    Ok(rewrite_cargo(text, dep, &[], v, true)?.0)
}

fn rewrite_cargo(
    text: &str,
    dep: &str,
    kinds: &[&str],
    v: Req,
    workspace_only: bool,
) -> Result<(String, bool), String> {
    let mut doc = toml_doc(text)?;
    let mut hit = Hit {
        found: false,
        inherits: false,
    };
    if !workspace_only {
        update_dep_tables(doc.as_table_mut(), dep, kinds, v, &mut hit);
    }
    if let Some(ws) = doc.get_mut("workspace").and_then(|w| w.as_table_like_mut()) {
        // `[workspace.dependencies]` has the shape of `[dependencies]`.
        if let Some(deps) = ws.get_mut("dependencies") {
            let mut holder = toml_edit::Table::new();
            holder.insert("dependencies", std::mem::take(deps));
            update_dep_tables(&mut holder, dep, &["dependencies"], v, &mut hit);
            if let Some(item) = holder.remove("dependencies") {
                *deps = item;
            }
        }
    }
    let targets = doc.get_mut("target").and_then(|t| t.as_table_like_mut());
    if let Some(targets) = targets.filter(|_| !workspace_only) {
        for (_, cfg) in targets.iter_mut() {
            if let Some(t) = cfg.as_table_like_mut() {
                update_dep_tables(t, dep, kinds, v, &mut hit);
            }
        }
    }
    if !hit.found {
        return Err(format!("Cargo.toml has no path dependency on `{dep}`"));
    }
    Ok((doc.to_string(), hit.inherits))
}

/// Today as `YYYY-MM-DD` (UTC; honours `HOIST_NOW`).
pub fn today() -> String {
    let (y, m, d) = civil_from_days(now_secs().div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

fn section_block(title: &str, section: Section, entries: &[ChangeEntry], nl: &str) -> String {
    let lines: Vec<String> = entries
        .iter()
        .filter(|e| e.section == section)
        .map(|e| format!("- {} ({}){nl}", e.text, e.short_sha))
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!("### {title}{nl}{nl}{}", lines.concat())
}

/// Put the section of `nv` above the previous ones; adds the `# Changelog` header if missing.
pub fn prepend_changelog(existing: Option<&str>, nv: &NextVersion, date: &str) -> String {
    let nl = "\n";
    let mut blocks = Vec::new();
    for (title, section) in [
        ("Breaking changes", Section::Breaking),
        ("Features", Section::Features),
        ("Fixes", Section::Fixes),
    ] {
        let b = section_block(title, section, &nv.entries, nl);
        if !b.is_empty() {
            blocks.push(b);
        }
    }
    if blocks.is_empty() {
        let line = if nv.first_release {
            "First release."
        } else {
            "Dependency updates."
        };
        blocks.push(format!("- {line}{nl}"));
    }
    prepend_section(existing, &nv.to.to_string(), date, &blocks, false)
}

/// Track a fenced code block over `line`; true when the line is a fence or inside one.
pub(crate) fn fence_step(fence: &mut Option<(char, usize)>, line: &str) -> bool {
    let t = line.trim_start_matches(' ');
    let c = match t.chars().next() {
        Some(c @ ('`' | '~')) => c,
        _ => return fence.is_some(),
    };
    let n = t.chars().take_while(|x| *x == c).count();
    match *fence {
        Some((fc, fnn)) if fc == c && n >= fnn && t[n..].trim().is_empty() => {
            *fence = None;
            true
        }
        Some(_) => true,
        None if n >= 3 && !(c == '`' && t[n..].contains('`')) => {
            *fence = Some((c, n));
            true
        }
        None => false,
    }
}

/// Byte offset of the first `## ` heading line outside code fences.
fn first_heading(text: &str) -> Option<usize> {
    let (mut fence, mut pos) = (None, 0);
    text.split_inclusive('\n').find_map(|l| {
        let inside = fence_step(&mut fence, l);
        pos += l.len();
        (!inside && l.starts_with("## ")).then_some(pos - l.len())
    })
}

/// Put `## <version> (<date>)` with `blocks` (each ends in a newline, `\n` here) above the
/// previous sections. `unreleased`: a `## Unreleased` section's body moves under it.
pub fn prepend_section(
    existing: Option<&str>,
    version: &str,
    date: &str,
    blocks: &[String],
    unreleased: bool,
) -> String {
    let existing = existing.unwrap_or("");
    let nl = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut blocks: Vec<String> = blocks.iter().map(|b| b.replace('\n', nl)).collect();
    let mut body = existing.trim_start_matches(['\r', '\n']);
    if let Some(r) = body.strip_prefix("# Changelog")
        && (r.is_empty() || r.starts_with(['\r', '\n']))
    {
        body = r.trim_start_matches(['\r', '\n']);
    }
    let kept;
    // `## Unreleased` counts only before the first other `## ` heading and outside fences.
    let mut at = 0;
    let mut fence: Option<(char, usize)> = None;
    let line = body.split_inclusive('\n').find_map(|l| {
        at += l.len();
        if fence_step(&mut fence, l) {
            return None;
        }
        let t = l.trim_end();
        (t == "## Unreleased").then_some((at - l.len(), l.len()))
    });
    let line = line.filter(|&(i, _)| {
        let mut fence = None;
        !body[..i].split_inclusive('\n').any(|l| {
            let inside = fence_step(&mut fence, l);
            !inside && l.starts_with("## ")
        })
    });
    if unreleased && let Some((i, len)) = line {
        let after = &body[i + len..];
        let end = first_heading(after).unwrap_or(after.len());
        // Drop blank lines around the block, but keep the first line's indentation.
        let block = &after[..end];
        let start = (block.split_inclusive('\n'))
            .take_while(|l| l.trim().is_empty())
            .map(str::len)
            .sum::<usize>();
        let moved = block[start..].trim_end();
        if !moved.is_empty() {
            blocks.push(format!("{moved}{nl}"));
        }
        kept = format!("{}{}", &body[..i], &after[end..]);
        body = &kept;
    }
    let new = format!("## {version} ({date}){nl}{nl}{}", blocks.join(nl));
    // Text before the first `## ` heading (a preamble) stays above the new section.
    let split = first_heading(body).unwrap_or(body.len());
    let (pre, rest) = body.split_at(split);
    let pre = pre.trim_end_matches(['\r', '\n']);
    let mut out = format!("# Changelog{nl}{nl}");
    if !pre.is_empty() {
        out.push_str(&format!("{pre}{nl}{nl}"));
    }
    out.push_str(&new);
    if !rest.is_empty() {
        out.push_str(nl);
        out.push_str(rest);
    }
    out
}

/// The body of the `## <version> (` or bare `## <version>` section of a changelog, up to the
/// next `## ` heading, without surrounding blank lines. `None` when absent or empty.
pub fn changelog_section(text: &str, version: &str) -> Option<String> {
    let heading = format!("## {version}");
    let mut lines = text.split_inclusive('\n');
    let mut fence = None;
    lines.find(|l| {
        !fence_step(&mut fence, l)
            && l.strip_prefix(&heading)
                .is_some_and(|r| r.starts_with(" (") || r.trim_end().is_empty())
    })?;
    let mut fence = None;
    let body: String = lines
        .take_while(|l| {
            let inside = fence_step(&mut fence, l);
            inside || !l.starts_with("## ")
        })
        .collect();
    let body = body.trim_matches(['\r', '\n']);
    (!body.trim().is_empty()).then(|| body.to_string())
}
