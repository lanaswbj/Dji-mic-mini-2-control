//! Registers this app's own hook entries in `~/.claude/settings.json`.
//!
//! Until now the two loopback listeners (`hook_bridge`, `permission_server`)
//! were only half a feature: the app bound the ports and then the preferences
//! screen told the user, in prose, to go and hand-edit a JSON file outside the
//! repo. Everyone who did it got it slightly different, and everyone who didn't
//! saw two open sockets that never received anything.
//!
//! Not to be confused with `permission_server`'s own settings writing, which
//! targets `{cwd}/.claude/settings.local.json`'s `permissions.allow` — a
//! per-project allow rule produced by the pie menu's "always allow" pick. This
//! module is about the user-level `hooks` object, and the two never touch the
//! same file.
//!
//! ## What it writes
//!
//! Two hooks, both loopback-only:
//! - `PermissionRequest` → an `"http"` hook pointing at `permission_server`
//!   (port 47216). That is a real allow/deny *decision* channel, not a
//!   notification — see that module's doc comment.
//! - a set of lifecycle events → a `"command"` hook that pipes the event JSON,
//!   unmodified, into `hook_bridge`'s raw TCP listener (port 47215) with a
//!   PowerShell one-liner. It reads stdin as explicit UTF-8, because the
//!   default encoding mangles any non-ASCII payload, and swallows connection
//!   errors in its own `try {} catch {}` so a closed app can never make a hook
//!   fail and block Claude Code.
//!
//! ## The three rules this file exists to enforce
//!
//! 1. **Never clobber.** `settings.json` is not ours. It routinely carries
//!    other tools' hooks (this project was developed alongside one), plus
//!    permissions, model settings and env. Everything is parsed as a generic
//!    `Value` and mutated in place; anything not recognised is copied through
//!    untouched, including hook groups belonging to other tools.
//! 2. **Be idempotent.** Installing twice must not register the hook twice, and
//!    installing over a *hand-written* registration — which is what every
//!    existing user has — must adopt it rather than duplicate it. Our entries
//!    are recognised by the port number in the command/url rather than by a
//!    marker field, precisely because the hand-written ones carry no marker.
//! 3. **Be reversible.** The first write backs the file up next to itself, and
//!    uninstall removes exactly what install added, cleaning up now-empty
//!    groups and arrays rather than leaving debris.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::hook_bridge;
use crate::permission_server;

/// Lifecycle events forwarded to `hook_bridge`. Kept in step with
/// `claude_status::apply_event`, which is the only consumer of most of them —
/// registering an event nothing maps to would cost a process spawn per
/// occurrence for no observable effect.
///
/// `PermissionRequest` is deliberately absent: it goes to the http hook below,
/// and a second, command-type registration for the same event would spawn a
/// PowerShell process alongside every decision.
const EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "Notification",
    "PreCompact",
    "PostCompact",
];

/// The two events whose hook groups are matched against a tool name. Claude
/// Code ignores `matcher` on the others, and writing one there would be noise
/// in a file the user is expected to be able to read.
const MATCHED_EVENTS: &[&str] = &["PreToolUse", "PostToolUse"];

/// What the preferences screen shows, and what `set_claude_hooks` returns so
/// the UI never has to guess what it just did.
#[derive(Serialize)]
pub struct ClaudeHooks {
    /// Absolute path of the settings file, whether or not it exists yet.
    pub path: String,
    pub settings_exist: bool,
    /// False when the file is present but is not a JSON object — the one case
    /// where installing would have to overwrite something it cannot understand,
    /// so it refuses instead.
    pub readable: bool,
    pub permission_hook: bool,
    /// How many of `EVENTS` currently carry our command hook.
    pub event_hooks: usize,
    pub event_total: usize,
    pub installed: bool,
}

fn settings_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".claude").join("settings.json"))
}

/// The PowerShell one-liner. Reads stdin as explicit UTF-8 (the default
/// encoding corrupts any non-ASCII payload) and forwards the bytes verbatim.
fn event_command() -> String {
    format!(
        "$reader = New-Object System.IO.StreamReader([Console]::OpenStandardInput(), \
         [System.Text.Encoding]::UTF8); $json = $reader.ReadToEnd(); try {{ \
         $c = New-Object System.Net.Sockets.TcpClient('127.0.0.1', {port}); $s = $c.GetStream(); \
         $b = [System.Text.Encoding]::UTF8.GetBytes($json); $s.Write($b, 0, $b.Length); \
         $s.Close(); $c.Close() }} catch {{}}",
        port = hook_bridge::PORT
    )
}

fn permission_url() -> String {
    format!("http://127.0.0.1:{}/permission", permission_server::PORT)
}

/// Is this one inner hook object ours? Matched on the port rather than on a
/// marker field, so a registration typed by hand — which is what every user of
/// this app has today — is recognised and replaced instead of duplicated.
fn is_ours(hook: &Value) -> bool {
    match hook.get("type").and_then(Value::as_str) {
        Some("http") => hook
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|u| u.contains(&permission_server::PORT.to_string())),
        Some("command") => hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|c| c.contains(&hook_bridge::PORT.to_string())),
        _ => false,
    }
}

fn has_ours(hooks: &Map<String, Value>, event: &str) -> bool {
    hooks
        .get(event)
        .and_then(Value::as_array)
        .is_some_and(|groups| {
            groups.iter().any(|g| {
                g.get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|inner| inner.iter().any(is_ours))
            })
        })
}

/// Drop our inner hooks from one event's group list, then drop any group that
/// is now empty. A group whose `hooks` key isn't an array is a shape we don't
/// understand and is left exactly as found.
fn strip_ours(groups: &mut Vec<Value>) {
    groups.retain_mut(
        |group| match group.get_mut("hooks").and_then(Value::as_array_mut) {
            Some(inner) => {
                inner.retain(|h| !is_ours(h));
                !inner.is_empty()
            }
            None => true,
        },
    );
}

fn add(hooks: &mut Map<String, Value>, event: &str, hook: Value) {
    let mut group = Map::new();
    if MATCHED_EVENTS.contains(&event) {
        group.insert("matcher".into(), json!("*"));
    }
    group.insert("hooks".into(), json!([hook]));

    match hooks
        .entry(event.to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
    {
        Some(groups) => {
            strip_ours(groups);
            groups.push(Value::Object(group));
        }
        // The key exists but isn't an array: someone hand-wrote something that
        // cannot be merged into. Replacing it is the only option that leaves a
        // working hook, and the backup taken above is the undo.
        None => {
            hooks.insert(event.to_string(), json!([Value::Object(group)]));
        }
    }
}

fn remove(hooks: &mut Map<String, Value>, event: &str) {
    let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
        return;
    };
    strip_ours(groups);
    if groups.is_empty() {
        hooks.remove(event);
    }
}

/// `(file exists, parsed contents)`. An empty file counts as an empty object
/// rather than as a parse failure — that is what a freshly `touch`ed
/// `settings.json` looks like, and refusing there would be unhelpful.
fn read(path: &PathBuf) -> (bool, Option<Value>) {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => (true, Some(json!({}))),
        Ok(text) => (true, serde_json::from_str::<Value>(&text).ok()),
        Err(_) => (false, None),
    }
}

fn describe(path: &PathBuf) -> ClaudeHooks {
    let (settings_exist, parsed) = read(path);
    let root = parsed.as_ref().and_then(Value::as_object);
    let hooks = root
        .and_then(|r| r.get("hooks"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let permission_hook = has_ours(&hooks, "PermissionRequest");
    let event_hooks = EVENTS.iter().filter(|e| has_ours(&hooks, e)).count();

    ClaudeHooks {
        path: path.display().to_string(),
        settings_exist,
        // A file that exists but doesn't parse as an object is the one state
        // install refuses to touch.
        readable: !settings_exist || root.is_some(),
        permission_hook,
        event_hooks,
        event_total: EVENTS.len(),
        installed: permission_hook && event_hooks == EVENTS.len(),
    }
}

/// What is registered right now. Never fails: a missing file is a legitimate
/// answer ("nothing is installed"), not an error to surface as a toast.
#[tauri::command]
pub fn claude_hooks_status() -> ClaudeHooks {
    match settings_path() {
        Some(path) => describe(&path),
        None => ClaudeHooks {
            path: String::new(),
            settings_exist: false,
            readable: false,
            permission_hook: false,
            event_hooks: 0,
            event_total: EVENTS.len(),
            installed: false,
        },
    }
}

/// Add or remove our hook entries, leaving everything else in the file alone.
#[tauri::command]
pub fn set_claude_hooks(enabled: bool) -> Result<ClaudeHooks, String> {
    let path = settings_path().ok_or("找不到用户主目录，无法定位 ~/.claude/settings.json")?;

    let (exists, parsed) = read(&path);
    if exists && parsed.as_ref().and_then(Value::as_object).is_none() {
        return Err(format!(
            "{} 不是有效的 JSON 对象，已放弃修改——请先手动检查这个文件。",
            path.display()
        ));
    }
    if !exists && !enabled {
        return Ok(describe(&path)); // nothing to remove
    }

    let mut root = match parsed {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };

    // Back up before the first modification, and only then: a backup rewritten
    // on every toggle would, after one round trip, be a copy of our own output
    // rather than of what the user had.
    let backup = path.with_extension("json.djimic-backup");
    if exists && !backup.exists() {
        fs::copy(&path, &backup).map_err(|e| format!("无法备份 {}：{e}", path.display()))?;
    }

    let mut hooks = root
        .get("hooks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if enabled {
        add(
            &mut hooks,
            "PermissionRequest",
            json!({ "type": "http", "url": permission_url() }),
        );
        let command = event_command();
        for event in EVENTS {
            add(
                &mut hooks,
                event,
                json!({ "type": "command", "command": command }),
            );
        }
    } else {
        remove(&mut hooks, "PermissionRequest");
        for event in EVENTS {
            remove(&mut hooks, event);
        }
    }

    if hooks.is_empty() {
        root.remove("hooks");
    } else {
        root.insert("hooks".into(), Value::Object(hooks));
    }

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| format!("无法序列化设置：{e}"))?;
    fs::write(&path, format!("{text}\n")).map_err(|e| format!("无法写入 {}：{e}", path.display()))?;

    Ok(describe(&path))
}
