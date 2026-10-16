//! Exact references from a language server, when a free one is installed.
//!
//! Tree-sitter references are names: two functions called `save` in different classes share
//! them. A language server resolves each use to its definition, so its references are exact.
//! Context Wizard looks for a known server on the `PATH`, starts it on demand for one question,
//! and stops it right after. Nothing is installed or downloaded.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// A language server Context Wizard knows how to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Server {
    /// Executable name looked up on the `PATH`.
    pub command: &'static str,
    /// Arguments that start it over standard input and output.
    pub args: &'static [&'static str],
    /// Index languages it serves.
    pub languages: &'static [&'static str],
    /// LSP language identifier of each of those languages, in the same order.
    pub language_ids: &'static [&'static str],
    /// How to install it, for the message shown when none is found.
    pub install: &'static str,
}

/// Known free language servers, in order of preference per language.
pub const SERVERS: &[Server] = &[
    Server {
        command: "rust-analyzer",
        args: &[],
        languages: &["rust"],
        language_ids: &["rust"],
        install: "rustup component add rust-analyzer",
    },
    Server {
        command: "typescript-language-server",
        args: &["--stdio"],
        languages: &["typescript", "tsx", "javascript"],
        language_ids: &["typescript", "typescriptreact", "javascript"],
        install: "npm install -g typescript typescript-language-server",
    },
    Server {
        command: "pyright-langserver",
        args: &["--stdio"],
        languages: &["python"],
        language_ids: &["python"],
        install: "npm install -g pyright",
    },
    Server {
        command: "pylsp",
        args: &[],
        languages: &["python"],
        language_ids: &["python"],
        install: "pip install python-lsp-server",
    },
    Server {
        command: "intelephense",
        args: &["--stdio"],
        languages: &["php"],
        language_ids: &["php"],
        install: "npm install -g intelephense",
    },
    Server {
        command: "gopls",
        args: &[],
        languages: &["go"],
        language_ids: &["go"],
        install: "go install golang.org/x/tools/gopls@latest",
    },
    Server {
        command: "clangd",
        args: &[],
        languages: &["c", "cpp"],
        language_ids: &["c", "cpp"],
        install: "install clangd from your LLVM distribution",
    },
];

/// Failure to get references from a language server.
#[derive(Debug)]
#[non_exhaustive]
pub enum LspError {
    /// No known server handles the language.
    Unsupported(String),
    /// No known server for the language is installed.
    NotInstalled {
        /// The language.
        language: String,
        /// Install hints of the servers that would handle it.
        hints: Vec<String>,
    },
    /// The server could not be started or its pipes failed.
    Io(io::Error),
    /// The server answered with an error.
    Server(String),
    /// The server did not answer in time.
    Timeout,
}

impl fmt::Display for LspError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(language) => {
                write!(formatter, "no known language server handles {language}")
            }
            Self::NotInstalled { language, hints } => write!(
                formatter,
                "no language server for {language} is installed; install one with: {}",
                hints.join(", or ")
            ),
            Self::Io(source) => write!(formatter, "language server failed: {source}"),
            Self::Server(message) => write!(formatter, "language server error: {message}"),
            Self::Timeout => write!(formatter, "language server did not answer in time"),
        }
    }
}

impl Error for LspError {}

impl From<io::Error> for LspError {
    fn from(source: io::Error) -> Self {
        Self::Io(source)
    }
}

/// A place in a file, with 0-based line and UTF-16 column as LSP counts them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Location {
    /// Absolute path of the file.
    pub path: PathBuf,
    /// Line, counted from 0.
    pub line: u32,
    /// Column in UTF-16 code units, counted from 0.
    pub column: u32,
}

/// The installed server for `language` with its executable, if any.
///
/// # Errors
///
/// Returns an error when no known server handles the language or none is installed.
pub fn find_server(language: &str) -> Result<(Server, PathBuf), LspError> {
    let candidates: Vec<&Server> = SERVERS
        .iter()
        .filter(|server| server.languages.contains(&language))
        .collect();
    if candidates.is_empty() {
        return Err(LspError::Unsupported(language.to_owned()));
    }
    for server in &candidates {
        if let Some(path) = which(server.command) {
            return Ok((**server, path));
        }
    }
    Err(LspError::NotInstalled {
        language: language.to_owned(),
        hints: candidates
            .iter()
            .map(|server| format!("`{}`", server.install))
            .collect(),
    })
}

/// Exact references of the symbol at `line` and `column` of `file`, in the project `root`,
/// including its declaration, sorted.
///
/// # Errors
///
/// Returns an error when the server is missing, fails or does not answer before `timeout`.
pub fn references(
    root: &Path,
    file: &Path,
    language: &str,
    line: u32,
    column: u32,
    timeout: Duration,
) -> Result<Vec<Location>, LspError> {
    let (server, executable) = find_server(language)?;
    let language_id = server
        .languages
        .iter()
        .position(|known| *known == language)
        .and_then(|position| server.language_ids.get(position))
        .copied()
        .unwrap_or(language);
    let mut child = Command::new(executable)
        .args(server.args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| {
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no stdin"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?;
        let mut client = Client::new(output, input);
        let text = fs::read_to_string(file)?;
        client.references(root, file, language_id, &text, line, column, timeout)
    })();
    stop(&mut child);
    result
}

/// A JSON-RPC connection to a language server.
pub struct Client<W: Write> {
    writer: W,
    messages: Receiver<Value>,
    next_id: u64,
}

impl<W: Write> fmt::Debug for Client<W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Client")
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl<W: Write> Client<W> {
    /// Connects to a server that reads from `writer`'s other end and writes to `reader`.
    pub fn new<R: Read + Send + 'static>(reader: R, writer: W) -> Self {
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Ok(Some(message)) = read_message(&mut reader) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            writer,
            messages,
            next_id: 1,
        }
    }

    /// Asks for the references of the symbol at `line` and `column` of `file`, retrying while
    /// the server is still indexing and answers with nothing. The declaration is included, so
    /// an indexed symbol always has at least one reference.
    ///
    /// # Errors
    ///
    /// Returns an error when the server fails or does not answer before `timeout`.
    #[allow(
        clippy::too_many_arguments,
        reason = "each value is a distinct part of the LSP request"
    )]
    pub fn references(
        &mut self,
        root: &Path,
        file: &Path,
        language_id: &str,
        text: &str,
        line: u32,
        column: u32,
        timeout: Duration,
    ) -> Result<Vec<Location>, LspError> {
        let deadline = Instant::now() + timeout;
        let root_uri = uri(root);
        self.request(
            "initialize",
            &json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": "project" }],
                "capabilities": {
                    "textDocument": { "references": { "dynamicRegistration": false } },
                    "workspace": { "workspaceFolders": true, "configuration": true },
                    "window": { "workDoneProgress": true }
                }
            }),
            deadline,
        )?;
        self.notify("initialized", &json!({}))?;
        let file_uri = uri(file);
        self.notify(
            "textDocument/didOpen",
            &json!({
                "textDocument": { "uri": file_uri, "languageId": language_id, "version": 1, "text": text }
            }),
        )?;
        let parameters = json!({
            "textDocument": { "uri": file_uri },
            "position": { "line": line, "character": column },
            "context": { "includeDeclaration": true }
        });
        loop {
            let answer = self.request("textDocument/references", &parameters, deadline);
            let found = match answer {
                Ok(value) => locations(&value),
                Err(LspError::Server(_)) if Instant::now() < deadline => Vec::new(),
                Err(error) => return Err(error),
            };
            if !found.is_empty() || Instant::now() + RETRY_DELAY >= deadline {
                let _ = self.request("shutdown", &Value::Null, Instant::now() + SHUTDOWN_GRACE);
                let _ = self.notify("exit", &Value::Null);
                return Ok(found);
            }
            thread::sleep(RETRY_DELAY);
        }
    }

    fn notify(&mut self, method: &str, parameters: &Value) -> Result<(), LspError> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": parameters }))
    }

    fn request(
        &mut self,
        method: &str,
        parameters: &Value,
        deadline: Instant,
    ) -> Result<Value, LspError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": parameters }))?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let message = match self.messages.recv_timeout(left) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => return Err(LspError::Timeout),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(LspError::Io(io::Error::other("the server exited")));
                }
            };
            if message.get("method").is_some() {
                if let Some(request) = message.get("id") {
                    let answer = server_request_answer(&message);
                    self.send(&json!({ "jsonrpc": "2.0", "id": request, "result": answer }))?;
                }
                continue;
            }
            if message.get("id") == Some(&json!(id)) {
                if let Some(error) = message.get("error") {
                    return Err(LspError::Server(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown error")
                            .to_owned(),
                    ));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), LspError> {
        let body = message.to_string();
        write!(self.writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
        self.writer.flush()?;
        Ok(())
    }
}

const RETRY_DELAY: Duration = Duration::from_millis(500);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

fn server_request_answer(message: &Value) -> Value {
    match message.get("method").and_then(Value::as_str) {
        Some("workspace/configuration") => {
            let items = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; items])
        }
        _ => Value::Null,
    }
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or_else(|| io::Error::other("message without length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body).ok())
}

fn locations(value: &Value) -> Vec<Location> {
    let mut found: Vec<Location> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|location| {
            let path = path_of_uri(location.get("uri")?.as_str()?)?;
            let start = location.pointer("/range/start")?;
            Some(Location {
                path,
                line: u32::try_from(start.get("line")?.as_u64()?).ok()?,
                column: u32::try_from(start.get("character")?.as_u64()?).ok()?,
            })
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The `file:` URI of `path`.
#[must_use]
pub fn uri(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = absolute.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let mut encoded = String::from("file://");
    if !text.starts_with('/') {
        encoded.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// The path of a `file:` URI.
#[must_use]
pub fn path_of_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let mut iter = rest.bytes();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let high = iter.next()?;
            let low = iter.next()?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(&[high, low]).ok()?, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    let text = String::from_utf8(bytes).ok()?;
    let windows_drive = text.len() > 2 && text.as_bytes()[2] == b':' && text.starts_with('/');
    Some(PathBuf::from(if windows_drive {
        &text[1..]
    } else {
        &text[..]
    }))
}

fn which(command: &str) -> Option<PathBuf> {
    let extensions: Vec<OsString> = if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_owned())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(OsString::from)
            .collect()
    } else {
        vec![OsString::new()]
    };
    let directories = env::split_paths(&env::var_os("PATH")?).collect::<Vec<_>>();
    for directory in directories {
        for extension in &extensions {
            let mut name = OsString::from(command);
            name.push(extension);
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn stop(child: &mut Child) {
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Groups `locations` by path relative to `root`, with 1-based lines, skipping files outside.
#[must_use]
pub fn by_file(root: &Path, locations: &[Location]) -> BTreeMap<String, Vec<u32>> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut files: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for location in locations {
        let path = location
            .path
            .canonicalize()
            .unwrap_or_else(|_| location.path.clone());
        if let Ok(relative) = path.strip_prefix(&root) {
            let relative = relative.to_string_lossy().replace('\\', "/");
            files.entry(relative).or_default().push(location.line + 1);
        }
    }
    files
}

#[cfg(test)]
mod tests;
