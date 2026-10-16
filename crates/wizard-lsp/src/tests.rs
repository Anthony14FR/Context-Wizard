#![allow(clippy::unwrap_used)]

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Client, Location, by_file, path_of_uri, read_message, uri};

fn reply(writer: &mut impl Write, message: &Value) {
    let body = message.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    writer.flush().unwrap();
}

#[test]
fn asks_a_server_for_references_and_waits_while_it_indexes() {
    let (from_server, mut server_out) = std::io::pipe().unwrap();
    let (server_in, to_server) = std::io::pipe().unwrap();
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("lib.rs");
    std::fs::write(&file, "fn save() {}\n").unwrap();
    let file_uri = uri(&file);
    let server = thread::spawn(move || {
        let mut input = BufReader::new(server_in);
        let mut methods = Vec::new();
        let mut asked = 0;
        while let Ok(Some(message)) = read_message(&mut input) {
            let method = message["method"].as_str().unwrap_or_default().to_owned();
            methods.push(method.clone());
            let id = message.get("id").cloned();
            match method.as_str() {
                "initialize" => {
                    reply(
                        &mut server_out,
                        &json!({ "jsonrpc": "2.0", "id": 99, "method": "workspace/configuration", "params": { "items": [{}, {}] } }),
                    );
                    reply(
                        &mut server_out,
                        &json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } }),
                    );
                }
                "textDocument/references" => {
                    asked += 1;
                    let result = if asked == 1 {
                        json!([])
                    } else {
                        json!([
                            { "uri": file_uri, "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 7 } } },
                            { "uri": file_uri, "range": { "start": { "line": 4, "character": 1 }, "end": { "line": 4, "character": 5 } } }
                        ])
                    };
                    reply(
                        &mut server_out,
                        &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    );
                }
                "shutdown" => {
                    reply(
                        &mut server_out,
                        &json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                    );
                }
                "exit" => break,
                _ if message.get("result").is_some() => {
                    assert_eq!(message["result"], json!([null, null]));
                }
                _ => {}
            }
        }
        methods
    });
    let mut client = Client::new(from_server, to_server);
    let found = client
        .references(
            project.path(),
            &file,
            "rust",
            "fn save() {}\n",
            0,
            3,
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[1].line, 4);
    drop(client);
    let methods = server.join().unwrap();
    assert_eq!(
        methods
            .iter()
            .filter(|method| !method.is_empty())
            .cloned()
            .collect::<Vec<_>>(),
        [
            "initialize",
            "initialized",
            "textDocument/didOpen",
            "textDocument/references",
            "textDocument/references",
            "shutdown",
            "exit"
        ]
    );
    let files = by_file(project.path(), &found);
    assert_eq!(files.get("lib.rs"), Some(&vec![1, 5]));
}

#[test]
fn converts_paths_to_uris_and_back() {
    assert_eq!(
        path_of_uri("file:///g%3A/My%20Project/a.rs"),
        Some(PathBuf::from("g:/My Project/a.rs"))
    );
    assert_eq!(
        path_of_uri("file:///home/me/a%23b.rs"),
        Some(PathBuf::from("/home/me/a#b.rs"))
    );
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("with space.rs");
    std::fs::write(&file, "").unwrap();
    let encoded = uri(&file);
    assert!(encoded.starts_with("file:///"), "{encoded}");
    assert!(encoded.ends_with("with%20space.rs"), "{encoded}");
    let back = path_of_uri(&encoded).unwrap();
    assert_eq!(back.canonicalize().unwrap(), file.canonicalize().unwrap());
    assert!(
        by_file(
            Path::new("/elsewhere"),
            &[Location {
                path: back,
                line: 0,
                column: 0
            }]
        )
        .is_empty()
    );
}
