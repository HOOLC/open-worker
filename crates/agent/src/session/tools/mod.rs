mod context_handoff;
mod end;
mod read_context_handoff;
mod read_session_history;
mod wait_for;

use std::{
    fs,
    future::Future,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    time::Duration,
};

use serde_json::{json, Value};
use tokio::io::AsyncReadExt;

use crate::session::runtime::{
    EventStore, StoreError, ToolConcurrency, ToolDefinition, ToolError, ToolExecutionResult,
    ToolExecutor, ToolInvocation, ToolResourceAccess, READ_CONTEXT_HANDOFF_TOOL_NAME,
    READ_SESSION_HISTORY_TOOL_NAME,
};
use crate::session::state::{SessionEvent, SessionState};

const BASH_TOOL: &str = "bash";
const READ_TOOL: &str = "read";
const EDIT_TOOL: &str = "edit";
const WRITE_TOOL: &str = "write";
const MODEL_OUTPUT_MAX_LINES: usize = 2_000;
const MODEL_OUTPUT_MAX_BYTES: usize = 50 * 1024;
const HISTORY_EVENT_SCAN_PAGE: usize = 128;

pub fn coding_tool_names() -> Vec<String> {
    [READ_TOOL, BASH_TOOL, EDIT_TOOL, WRITE_TOOL]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub(crate) fn runtime_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        context_handoff::definition(),
        end::definition(),
        wait_for::definition(),
        read_context_handoff::definition(),
        read_session_history::definition(),
    ]
}

pub(crate) fn provider_runtime_tool_definitions(state: &SessionState) -> Vec<ToolDefinition> {
    let mut definitions = vec![
        end::definition(),
        wait_for::definition(),
        context_handoff::definition(),
    ];
    if state.latest_context_handoff.is_some() {
        definitions.push(read_context_handoff::definition());
        definitions.push(read_session_history::definition());
    }
    definitions
}

pub(crate) fn context_handoff_document(input: &Value) -> Result<String, ToolError> {
    context_handoff::document(input)
}

pub(crate) fn execute_runtime_read_tool(
    state: &SessionState,
    store: &dyn EventStore,
    session_id: &str,
    tool_name: &str,
    input: &Value,
) -> Result<ToolExecutionResult, ToolError> {
    let value = match tool_name {
        READ_CONTEXT_HANDOFF_TOOL_NAME => {
            let latest = state
                .latest_context_handoff
                .as_ref()
                .ok_or(ToolError::Unavailable)?;
            let record = store
                .read_event(session_id, &latest.handoff_id)
                .map_err(store_read_error)?
                .ok_or(ToolError::Unavailable)?;
            let SessionEvent::ContextHandoffCreated { handoff } = &record.event else {
                return Err(ToolError::Unavailable);
            };
            if handoff.handoff_id != latest.handoff_id {
                return Err(ToolError::Unavailable);
            }
            read_context_handoff::execute(handoff, input)?
        }
        READ_SESSION_HISTORY_TOOL_NAME => match read_session_history::parse(input)? {
            read_session_history::HistoryRequest::Content {
                event_id,
                content_offset,
            } => {
                let record = store
                    .read_event(session_id, &event_id)
                    .map_err(store_read_error)?
                    .ok_or(ToolError::InvalidInvocation)?;
                let message = read_session_history::message_from_record(&record)
                    .ok_or(ToolError::InvalidInvocation)?;
                read_session_history::content(&message, content_offset)?
            }
            read_session_history::HistoryRequest::List {
                before_event_id,
                limit,
            } => {
                let (messages, has_older) =
                    read_history_messages(store, session_id, before_event_id, limit)?;
                read_session_history::list(&messages, has_older)
            }
        },
        _ => return Err(ToolError::InvalidSelection),
    };
    Ok(ToolExecutionResult::success(
        serde_json::to_string(&value).map_err(|_| ToolError::Unavailable)?,
    ))
}

fn read_history_messages(
    store: &dyn EventStore,
    session_id: &str,
    mut before_event_id: Option<String>,
    limit: usize,
) -> Result<(Vec<read_session_history::HistoryMessage>, bool), ToolError> {
    let wanted = limit.checked_add(1).ok_or(ToolError::InvalidInvocation)?;
    let mut newest_first = Vec::with_capacity(wanted);
    while newest_first.len() < wanted {
        let records = store
            .read_stream_before(
                session_id,
                before_event_id.as_deref(),
                HISTORY_EVENT_SCAN_PAGE,
            )
            .map_err(store_read_error)?;
        let reached_start = records.len() < HISTORY_EVENT_SCAN_PAGE;
        let Some(oldest_event_id) = records.first().map(|record| record.event_id.clone()) else {
            break;
        };
        before_event_id = Some(oldest_event_id);
        for record in records.iter().rev() {
            if let Some(message) = read_session_history::message_from_record(record) {
                newest_first.push(message);
                if newest_first.len() == wanted {
                    break;
                }
            }
        }
        if reached_start {
            break;
        }
    }
    let has_older = newest_first.len() > limit;
    newest_first.truncate(limit);
    newest_first.reverse();
    Ok((newest_first, has_older))
}

fn store_read_error(error: StoreError) -> ToolError {
    match error {
        StoreError::InvalidEventId => ToolError::InvalidInvocation,
        _ => ToolError::Unavailable,
    }
}

/// Built-in coding tools whose relative paths use the caller-owned Session
/// workspace as cwd. Absolute paths retain their ordinary filesystem meaning,
/// matching the shell and Pi coding tools.
#[derive(Default)]
pub struct WorkspaceToolExecutor;

impl WorkspaceToolExecutor {
    pub fn new() -> Self {
        Self
    }

    fn definition(name: &str) -> Option<ToolDefinition> {
        let (description, input_schema) = match name {
            BASH_TOOL => (
                "Execute a shell command in the session workspace. Returns combined stdout and stderr. Output is truncated to the last 2000 lines or 50KB (whichever is hit first). If truncated, full output is saved inside the session workspace at the returned path; use read with offset and limit to inspect it. Optionally provide a timeout in seconds.",
                json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "minLength": 1},
                        "timeout": {
                            "type": "number",
                            "exclusiveMinimum": 0,
                            "description": "Timeout in seconds (optional, no default timeout)"
                        }
                    },
                    "required": ["command"],
                    "additionalProperties": false
                }),
            ),
            READ_TOOL => (
                "Read the contents of a text file. Output is truncated to 2000 lines or 50KB (whichever is hit first). Use offset and limit for large files. When you need the full file, continue with offset until complete.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Path to the file to read (relative or absolute)"
                        },
                        "offset": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Line number to start reading from (1-indexed)"
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of lines to read"
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
            ),
            EDIT_TOOL => (
                "Edit a single file using exact text replacement. Every edits[].oldText must match a unique, non-overlapping region of the original file. If two changes affect the same block or nearby lines, merge them into one edit instead of emitting overlapping edits. Do not include large unchanged regions just to connect distant changes.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Path to the file to edit (relative or absolute)"
                        },
                        "edits": {
                            "type": "array",
                            "minItems": 1,
                            "description": "One or more targeted replacements. Each edit is matched against the original file, not incrementally. Do not include overlapping or nested edits. If two changes touch the same block or nearby lines, merge them into one edit instead.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "oldText": {
                                        "type": "string",
                                        "minLength": 1,
                                        "description": "Exact text for one targeted replacement. It must be unique in the original file and must not overlap with any other edits[].oldText in the same call."
                                    },
                                    "newText": {
                                        "type": "string",
                                        "description": "Replacement text for this targeted edit."
                                    }
                                },
                                "required": ["oldText", "newText"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["path", "edits"],
                    "additionalProperties": false
                }),
            ),
            WRITE_TOOL => (
                "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Automatically creates parent directories. Use write only for new files or complete rewrites.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Path to the file to write (relative or absolute)"
                        },
                        "content": {
                            "type": "string",
                            "description": "Complete file content"
                        }
                    },
                    "required": ["path", "content"],
                    "additionalProperties": false
                }),
            ),
            _ => return None,
        };
        Some(ToolDefinition {
            name: name.to_owned(),
            description: description.to_owned(),
            input_schema,
        })
    }
}

impl ToolExecutor for WorkspaceToolExecutor {
    fn definitions(&self, selected: &[String]) -> Result<Vec<ToolDefinition>, ToolError> {
        selected
            .iter()
            .map(|name| Self::definition(name).ok_or(ToolError::InvalidSelection))
            .collect()
    }

    fn concurrency(&self, invocation: &ToolInvocation) -> ToolConcurrency {
        let access = match invocation.tool_name.as_str() {
            READ_TOOL => ToolResourceAccess::Read,
            EDIT_TOOL | WRITE_TOOL => ToolResourceAccess::Write,
            BASH_TOOL => return ToolConcurrency::Parallel,
            _ => return ToolConcurrency::Parallel,
        };
        let Some(path) = invocation
            .input
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
        else {
            return ToolConcurrency::Parallel;
        };
        let Ok(workspace) = resolve_workspace(&invocation.workspace) else {
            return ToolConcurrency::Parallel;
        };
        ToolConcurrency::Resource {
            key: lexical_path(&resolve_path(&workspace, path))
                .to_string_lossy()
                .into_owned(),
            access,
        }
    }

    fn execute<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<ToolExecutionResult, ToolError>> + Send + 'a>> {
        Box::pin(async move {
            let workspace = resolve_workspace(&invocation.workspace)?;
            match invocation.tool_name.as_str() {
                READ_TOOL => {
                    let path = required_string(&invocation.input, "path")?;
                    let offset = optional_positive_usize(&invocation.input, "offset")?.unwrap_or(1);
                    let limit = optional_positive_usize(&invocation.input, "limit")?;
                    let result = tokio::task::spawn_blocking(move || {
                        read_text_file(&workspace, &path, offset, limit)
                    })
                    .await
                    .map_err(|_| ToolError::Unavailable)?;
                    Ok(result.unwrap_or_else(ToolExecutionResult::error))
                }
                EDIT_TOOL => {
                    let path = required_string(&invocation.input, "path")?;
                    let edits = parse_edits(&invocation.input)?;
                    let result = tokio::task::spawn_blocking(move || {
                        edit_text_file(&workspace, &path, &edits)
                    })
                    .await
                    .map_err(|_| ToolError::Unavailable)?;
                    Ok(result.unwrap_or_else(ToolExecutionResult::error))
                }
                WRITE_TOOL => {
                    let path = required_string(&invocation.input, "path")?;
                    let content = required_string_allow_empty(&invocation.input, "content")?;
                    let result = tokio::task::spawn_blocking(move || {
                        write_text_file(&workspace, &path, &content)
                    })
                    .await
                    .map_err(|_| ToolError::Unavailable)?;
                    Ok(result.unwrap_or_else(ToolExecutionResult::error))
                }
                BASH_TOOL => {
                    let command = required_string(&invocation.input, "command")?;
                    let timeout = optional_positive_duration(&invocation.input, "timeout")?;
                    Ok(execute_bash(&workspace, &command, timeout, &invocation.environment).await)
                }
                _ => Err(ToolError::InvalidSelection),
            }
        })
    }
}

#[derive(Clone)]
struct TextEdit {
    old_text: String,
    new_text: String,
}

fn required_string(input: &Value, field: &str) -> Result<String, ToolError> {
    input
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(ToolError::InvalidInvocation)
}

fn required_string_allow_empty(input: &Value, field: &str) -> Result<String, ToolError> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(ToolError::InvalidInvocation)
}

fn optional_positive_usize(input: &Value, field: &str) -> Result<Option<usize>, ToolError> {
    input
        .get(field)
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value > 0)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or(ToolError::InvalidInvocation)
        })
        .transpose()
}

fn optional_positive_duration(input: &Value, field: &str) -> Result<Option<Duration>, ToolError> {
    input
        .get(field)
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(Duration::from_secs_f64)
                .ok_or(ToolError::InvalidInvocation)
        })
        .transpose()
}

fn parse_edits(input: &Value) -> Result<Vec<TextEdit>, ToolError> {
    let edits = input
        .get("edits")
        .and_then(Value::as_array)
        .filter(|edits| !edits.is_empty())
        .ok_or(ToolError::InvalidInvocation)?;
    edits
        .iter()
        .map(|edit| {
            Ok(TextEdit {
                old_text: required_string(edit, "oldText")?,
                new_text: required_string_allow_empty(edit, "newText")?,
            })
        })
        .collect()
}

fn resolve_workspace(workspace: &Path) -> Result<PathBuf, ToolError> {
    fs::canonicalize(workspace)
        .ok()
        .filter(|path| path.is_dir())
        .ok_or(ToolError::Unavailable)
}

fn resolve_path(workspace: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}

fn lexical_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn read_text_file(
    workspace: &Path,
    value: &str,
    offset: usize,
    requested_limit: Option<usize>,
) -> Result<ToolExecutionResult, String> {
    let path = resolve_path(workspace, value);
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    let lines = content.split_inclusive('\n').collect::<Vec<_>>();
    let total = lines.len();
    if offset > total && !(total == 0 && offset == 1) {
        return Err(format!(
            "Offset {offset} is beyond the end of {} ({total} lines).",
            path.display()
        ));
    }
    let start = offset.saturating_sub(1).min(total);
    let requested_end = requested_limit
        .map(|limit| start.saturating_add(limit))
        .unwrap_or(total)
        .min(total);
    let hard_end = start
        .saturating_add(MODEL_OUTPUT_MAX_LINES)
        .min(requested_end);
    let mut output = String::new();
    let mut end = start;
    for line in &lines[start..hard_end] {
        if output.len().saturating_add(line.len()) > MODEL_OUTPUT_MAX_BYTES {
            break;
        }
        output.push_str(line);
        end += 1;
    }
    if end == start && start < requested_end {
        return Ok(ToolExecutionResult::success(format!(
            "Line {offset} in {} exceeds the 50KB output limit. Use bash with sed/head -c to inspect that line.",
            path.display()
        )));
    }
    if end < total {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&format!(
            "[Showing lines {}-{end} of {total}. Use offset={} to continue.]",
            start + 1,
            end + 1
        ));
    }
    Ok(ToolExecutionResult::success(output))
}

fn write_text_file(
    workspace: &Path,
    value: &str,
    content: &str,
) -> Result<ToolExecutionResult, String> {
    let path = resolve_path(workspace, value);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    fs::write(&path, content)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    Ok(ToolExecutionResult::success(format!(
        "Successfully wrote {} bytes to {}",
        content.len(),
        path.display()
    )))
}

fn edit_text_file(
    workspace: &Path,
    value: &str,
    edits: &[TextEdit],
) -> Result<ToolExecutionResult, String> {
    let path = resolve_path(workspace, value);
    let original = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    let (bom, without_bom) = original
        .strip_prefix('\u{feff}')
        .map_or((false, original.as_str()), |text| (true, text));
    let line_ending = if without_bom.contains("\r\n") {
        "\r\n"
    } else if without_bom.contains('\r') {
        "\r"
    } else {
        "\n"
    };
    let normalized = without_bom.replace("\r\n", "\n").replace('\r', "\n");
    let mut replacements = Vec::with_capacity(edits.len());
    for (index, edit) in edits.iter().enumerate() {
        let old = edit.old_text.replace("\r\n", "\n").replace('\r', "\n");
        let new = edit.new_text.replace("\r\n", "\n").replace('\r', "\n");
        let matches = find_edit_matches(&normalized, &old);
        match matches.as_slice() {
            [] => {
                return Err(format!(
                    "Edit {} failed: oldText was not found in {}.",
                    index + 1,
                    path.display()
                ))
            }
            [range] => replacements.push((range.0, range.1, new)),
            _ => {
                return Err(format!(
                "Edit {} failed: oldText matches multiple locations in {}. Include more context.",
                index + 1,
                path.display()
            ))
            }
        }
    }
    replacements.sort_by_key(|replacement| replacement.0);
    for pair in replacements.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(format!("Edits overlap in {}.", path.display()));
        }
    }
    let mut updated = normalized.clone();
    for (start, end, replacement) in replacements.iter().rev() {
        updated.replace_range(*start..*end, replacement);
    }
    if updated == normalized {
        return Err(format!("No changes made to {}.", path.display()));
    }
    if line_ending != "\n" {
        updated = updated.replace('\n', line_ending);
    }
    if bom {
        updated.insert(0, '\u{feff}');
    }
    fs::write(&path, updated)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))?;
    Ok(ToolExecutionResult::success(format!(
        "Successfully replaced {} block(s) in {}.",
        edits.len(),
        path.display()
    )))
}

fn find_edit_matches(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    let exact = haystack
        .match_indices(needle)
        .map(|(start, value)| (start, start + value.len()))
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return exact;
    }
    let (canonical_haystack, map) = canonical_text_with_map(haystack);
    let (canonical_needle, _) = canonical_text_with_map(needle);
    canonical_haystack
        .match_indices(&canonical_needle)
        .filter_map(|(start, value)| {
            let end = start + value.len();
            let original_start = map.get(start)?.0;
            let original_end = map.get(end.checked_sub(1)?)?.1;
            Some((original_start, original_end))
        })
        .collect()
}

fn canonical_text_with_map(value: &str) -> (String, Vec<(usize, usize)>) {
    let mut output = String::new();
    let mut map = Vec::new();
    let mut line_start = 0;
    for line in value.split_inclusive('\n') {
        let has_newline = line.ends_with('\n');
        let body = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = body.trim_end_matches([' ', '\t', '\u{00a0}']);
        for (relative, character) in trimmed.char_indices() {
            let canonical = match character {
                '\u{2018}' | '\u{2019}' | '\u{201b}' => '\'',
                '\u{201c}' | '\u{201d}' | '\u{201f}' => '"',
                '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
                '\u{00a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' => ' ',
                value => value,
            };
            let start = line_start + relative;
            let end = start + character.len_utf8();
            let before = output.len();
            output.push(canonical);
            map.extend(std::iter::repeat_n((start, end), output.len() - before));
        }
        if has_newline {
            let position = line_start + line.len() - 1;
            output.push('\n');
            map.push((position, position + 1));
        }
        line_start += line.len();
    }
    (output, map)
}

async fn execute_bash(
    workspace: &Path,
    command: &str,
    timeout: Option<Duration>,
    environment: &std::collections::BTreeMap<String, String>,
) -> ToolExecutionResult {
    let output_id = ulid::Ulid::new();
    let output_relative_path = PathBuf::from(".zork").join(format!("bash-output-{output_id}.log"));
    let full_output_path = workspace.join(&output_relative_path);
    let output_dir = full_output_path
        .parent()
        .expect("bash output path always has a parent");
    if let Err(error) = fs::create_dir_all(output_dir) {
        return ToolExecutionResult::error(format!(
            "Failed to create bash output directory: {error}"
        ));
    }
    let spool = BashOutputSpool::new(
        std::env::temp_dir().join(format!("zork-bash-output-{output_id}.tmp")),
    );
    let output_file = match fs::File::create(spool.path()) {
        Ok(file) => file,
        Err(error) => {
            return ToolExecutionResult::error(format!(
                "Failed to create bash output file: {error}"
            ))
        }
    };

    let mut process = tokio::process::Command::new("/bin/sh");
    process.env_clear();
    for key in [
        "HOME",
        "PATH",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TERM",
        "USER",
        "LOGNAME",
        "SHELL",
        "SSH_AUTH_SOCK",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            process.env(key, value);
        }
    }
    process
        .arg("-c")
        .arg(format!("exec 2>&1\n{command}"))
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .envs(environment)
        .kill_on_drop(true);
    #[cfg(unix)]
    process.process_group(0);
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ToolExecutionResult::error(format!("Failed to start command: {error}"))
        }
    };
    let mut process_group = BashProcessGroup::new(child.id());
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return ToolExecutionResult::error("Failed to capture command output"),
    };
    let drain = tokio::spawn(async move {
        let mut file = output_file;
        let mut total_bytes = 0usize;
        let mut newline_count = 0usize;
        let mut current_line_bytes = 0usize;
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            let read = stdout.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            file.write_all(&chunk[..read])?;
            total_bytes = total_bytes.saturating_add(read);
            for byte in &chunk[..read] {
                if *byte == b'\n' {
                    newline_count = newline_count.saturating_add(1);
                    current_line_bytes = 0;
                } else {
                    current_line_bytes = current_line_bytes.saturating_add(1);
                }
            }
        }
        file.flush()?;
        let total_lines = newline_count.saturating_add(usize::from(current_line_bytes > 0));
        Ok::<_, std::io::Error>(BashOutputStats {
            total_bytes,
            total_lines,
            last_line_bytes: current_line_bytes,
        })
    });

    let status = if let Some(timeout) = timeout {
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(status) => status,
            Err(_) => {
                process_group.terminate();
                let _ = child.kill().await;
                let _ = child.wait().await;
                let partial = drain.await.ok().and_then(Result::ok);
                return format_bash_result(
                    partial,
                    Some(format!(
                        "Command timed out after {:.3} seconds",
                        timeout.as_secs_f64()
                    )),
                    &spool,
                    &full_output_path,
                    &output_relative_path,
                );
            }
        }
    } else {
        child.wait().await
    };
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            process_group.terminate();
            let partial = drain.await.ok().and_then(Result::ok);
            return format_bash_result(
                partial,
                Some(format!("Failed to wait for command: {error}")),
                &spool,
                &full_output_path,
                &output_relative_path,
            );
        }
    };
    process_group.terminate();
    let drained = drain.await.ok().and_then(Result::ok);
    let failure = (!status.success()).then(|| {
        status.code().map_or_else(
            || "Command terminated by signal".to_owned(),
            |code| format!("Command exited with code {code}"),
        )
    });
    format_bash_result(
        drained,
        failure,
        &spool,
        &full_output_path,
        &output_relative_path,
    )
}

#[derive(Clone, Copy)]
struct BashOutputStats {
    total_bytes: usize,
    total_lines: usize,
    last_line_bytes: usize,
}

fn format_bash_result(
    drained: Option<BashOutputStats>,
    failure: Option<String>,
    spool: &BashOutputSpool,
    full_output_path: &Path,
    output_relative_path: &Path,
) -> ToolExecutionResult {
    let Some(stats) = drained else {
        return ToolExecutionResult::error(
            failure.unwrap_or_else(|| "Failed to read command output".to_owned()),
        );
    };
    let truncated =
        stats.total_bytes > MODEL_OUTPUT_MAX_BYTES || stats.total_lines > MODEL_OUTPUT_MAX_LINES;
    let output = if truncated {
        spool
            .publish(full_output_path)
            .and_then(|()| read_bash_output_tail(full_output_path, stats))
    } else {
        fs::read(spool.path()).map(|bytes| BashOutputView {
            content: bytes,
            output_lines: stats.total_lines,
            truncated_by_lines: false,
            last_line_partial: false,
        })
    };
    let Ok(output) = output else {
        return ToolExecutionResult::error(
            failure.unwrap_or_else(|| "Failed to read command output".to_owned()),
        );
    };
    let mut content = String::from_utf8_lossy(&output.content).into_owned();
    if content.is_empty() && failure.is_none() {
        content.push_str("(no output)");
    }
    if truncated {
        content.push_str("\n\n");
        let notice = if output.last_line_partial {
            format!(
                "[Showing last {} of line {} (line is {}). Full output: {}. Use read with offset and limit on this path.]",
                format_size(output.content.len()),
                stats.total_lines,
                format_size(stats.last_line_bytes),
                output_relative_path.display()
            )
        } else {
            let start_line = stats
                .total_lines
                .saturating_sub(output.output_lines)
                .saturating_add(1);
            let limit = if output.truncated_by_lines {
                String::new()
            } else {
                format!(" ({} limit)", format_size(MODEL_OUTPUT_MAX_BYTES))
            };
            format!(
                "[Showing lines {start_line}-{} of {}{limit}. Full output: {}. Use read with offset and limit on this path.]",
                stats.total_lines,
                stats.total_lines,
                output_relative_path.display()
            )
        };
        content.push_str(&notice);
    }
    if let Some(failure) = failure {
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(&failure);
        ToolExecutionResult::error(content)
    } else {
        ToolExecutionResult::success(content)
    }
}

struct BashOutputSpool {
    path: PathBuf,
}

impl BashOutputSpool {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(&self, destination: &Path) -> std::io::Result<()> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::rename(&self.path, destination) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
                fs::copy(&self.path, destination)?;
                fs::remove_file(&self.path)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for BashOutputSpool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct BashOutputView {
    content: Vec<u8>,
    output_lines: usize,
    truncated_by_lines: bool,
    last_line_partial: bool,
}

fn read_bash_output_tail(
    full_output_path: &Path,
    stats: BashOutputStats,
) -> std::io::Result<BashOutputView> {
    let mut file = fs::File::open(full_output_path)?;
    let tail_start = stats.total_bytes.saturating_sub(MODEL_OUTPUT_MAX_BYTES);
    let read_start = tail_start.saturating_sub(1);
    file.seek(SeekFrom::Start(read_start as u64))?;
    let mut tail_window = Vec::with_capacity(stats.total_bytes.saturating_sub(read_start));
    file.read_to_end(&mut tail_window)?;
    let mut tail = if tail_start == 0 {
        tail_window
    } else {
        let starts_at_line_boundary = tail_window.first() == Some(&b'\n');
        let mut tail = tail_window.split_off(1);
        if !starts_at_line_boundary {
            if let Some(newline) = tail.iter().position(|byte| *byte == b'\n') {
                tail.drain(..=newline);
            }
        }
        tail
    };
    let last_line_partial = tail_start > 0
        && logical_line_count(&tail) == 1
        && stats.last_line_bytes > MODEL_OUTPUT_MAX_BYTES;
    if last_line_partial {
        let utf8_start = tail
            .iter()
            .position(|byte| byte & 0xc0 != 0x80)
            .unwrap_or(tail.len());
        tail.drain(..utf8_start);
    }
    let available_lines = logical_line_count(&tail);
    let truncated_by_lines =
        stats.total_lines > MODEL_OUTPUT_MAX_LINES && available_lines >= MODEL_OUTPUT_MAX_LINES;
    truncate_tail_lines(&mut tail, MODEL_OUTPUT_MAX_LINES);
    let output_lines = logical_line_count(&tail);
    if tail.last() == Some(&b'\n') {
        tail.pop();
    }

    Ok(BashOutputView {
        content: tail,
        output_lines,
        truncated_by_lines,
        last_line_partial,
    })
}

fn truncate_tail_lines(bytes: &mut Vec<u8>, max_lines: usize) {
    let total_lines = logical_line_count(bytes);
    let lines_to_drop = total_lines.saturating_sub(max_lines);
    if lines_to_drop == 0 {
        return;
    }
    let mut dropped = 0usize;
    let mut start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            dropped += 1;
            if dropped == lines_to_drop {
                start = index + 1;
                break;
            }
        }
    }
    bytes.drain(..start);
}

fn logical_line_count(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    bytes
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        .saturating_add(usize::from(bytes.last() != Some(&b'\n')))
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

struct BashProcessGroup {
    #[cfg(unix)]
    process_group_id: Option<i32>,
}

impl BashProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self {
            #[cfg(unix)]
            process_group_id: pid.and_then(|pid| i32::try_from(pid).ok()),
        }
    }

    fn terminate(&mut self) {
        #[cfg(unix)]
        if let Some(process_group_id) = self.process_group_id.take() {
            unsafe {
                libc::kill(-process_group_id, libc::SIGKILL);
            }
        }
    }
}

impl Drop for BashProcessGroup {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coding_tools_are_presented_in_pi_order() {
        assert_eq!(
            coding_tool_names(),
            vec![
                "read".to_owned(),
                "bash".to_owned(),
                "edit".to_owned(),
                "write".to_owned(),
            ]
        );
    }

    #[test]
    fn coding_tool_definitions_explain_the_pi_contract_to_the_model() {
        let read = WorkspaceToolExecutor::definition(READ_TOOL).unwrap();
        assert!(read.description.contains("2000 lines or 50KB"));
        assert!(read
            .description
            .contains("When you need the full file, continue with offset until complete."));
        assert_eq!(
            read.input_schema["properties"]["offset"]["description"],
            "Line number to start reading from (1-indexed)"
        );

        let edit = WorkspaceToolExecutor::definition(EDIT_TOOL).unwrap();
        assert!(edit
            .description
            .contains("Every edits[].oldText must match a unique, non-overlapping region"));
        assert!(edit
            .description
            .contains("merge them into one edit instead of emitting overlapping edits"));
        assert_eq!(
            edit.input_schema["properties"]["edits"]["description"],
            "One or more targeted replacements. Each edit is matched against the original file, not incrementally. Do not include overlapping or nested edits. If two changes touch the same block or nearby lines, merge them into one edit instead."
        );

        let write = WorkspaceToolExecutor::definition(WRITE_TOOL).unwrap();
        assert!(write
            .description
            .contains("Use write only for new files or complete rewrites."));
        assert!(write
            .description
            .contains("Automatically creates parent directories."));
    }

    #[test]
    fn bash_timeout_contract_is_visible_to_the_model_and_uses_seconds() {
        let definition = WorkspaceToolExecutor::definition(BASH_TOOL).unwrap();

        assert!(definition
            .description
            .contains("Optionally provide a timeout in seconds."));
        assert_eq!(
            definition.input_schema["properties"]["timeout"]["description"],
            "Timeout in seconds (optional, no default timeout)"
        );
        assert_eq!(
            optional_positive_duration(&json!({"timeout": 0.05}), "timeout").unwrap(),
            Some(Duration::from_millis(50))
        );
    }

    #[test]
    fn bash_tail_matches_pi_line_limit_and_drops_the_original_trailing_newline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.log");
        let content = (1..=2_401)
            .map(|line| format!("line-{line:04}\n"))
            .collect::<String>();
        fs::write(&path, &content).unwrap();

        let view = read_bash_output_tail(
            &path,
            BashOutputStats {
                total_bytes: content.len(),
                total_lines: 2_401,
                last_line_bytes: 0,
            },
        )
        .unwrap();

        let visible = String::from_utf8(view.content).unwrap();
        assert!(visible.starts_with("line-0402\n"));
        assert!(visible.ends_with("line-2401"));
        assert!(!visible.ends_with('\n'));
        assert!(view.truncated_by_lines);
    }

    #[test]
    fn bash_tail_starts_a_huge_utf8_line_on_a_character_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.log");
        let content = "€".repeat(20_000);
        fs::write(&path, &content).unwrap();

        let view = read_bash_output_tail(
            &path,
            BashOutputStats {
                total_bytes: content.len(),
                total_lines: 1,
                last_line_bytes: content.len(),
            },
        )
        .unwrap();

        let visible = String::from_utf8(view.content).unwrap();
        assert!(visible.ends_with('€'));
        assert!(visible.len() <= MODEL_OUTPUT_MAX_BYTES);
        assert!(view.last_line_partial);
    }

    #[tokio::test]
    async fn bash_does_not_write_its_pending_output_next_to_a_caller_owned_workspace() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(workspace.join(".zork")).unwrap();

        let result = execute_bash(
            &workspace,
            "find .. -maxdepth 1 -type f -name '.bash-output-*.tmp' -print; find .zork -maxdepth 1 -type f -print",
            None,
            &std::collections::BTreeMap::new(),
        )
        .await;

        assert!(!result.is_error);
        assert_eq!(result.content, "(no output)");
        assert_eq!(fs::read_dir(workspace.join(".zork")).unwrap().count(), 0);
    }
}
