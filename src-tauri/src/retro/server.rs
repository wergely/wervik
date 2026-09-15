//! A local-only HTTP server that hands one EmulatorJS play session its ROM,
//! optional BIOS, and bundled runtime files.
//!
//! One server instance is spun up per launched game and torn down the
//! moment the player exits it — it is never long-lived and never shared
//! across games. That is a deliberate simplification over routing multiple
//! sessions through one server by id: with exactly one session per server,
//! there is no id to guess and no cross-session lookup to get wrong, so the
//! "session-scoped resource exposure" requirement holds by construction
//! rather than by an access check that could have a bug in it.
//!
//! Bound to `127.0.0.1` on a kernel-assigned port (`:0`) so it is never
//! reachable from outside the machine and never collides with anything
//! else running locally.

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use tiny_http::{Header, Response, Server};

use super::session::EmulatorSession;

pub struct RunningServer {
    pub port: u16,
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RunningServer {
    pub fn session_url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Starts serving `session` in the background. `runtime_dir` is where
/// Wervik's (separately vendored) copy of the EmulatorJS runtime lives; if
/// it hasn't been set up yet, the session page itself explains that
/// honestly instead of the emulator silently failing to boot.
pub fn start(
    session: EmulatorSession,
    runtime_dir: Option<PathBuf>,
) -> Result<RunningServer, String> {
    let server = Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = match server.server_addr().to_ip() {
        Some(SocketAddr::V4(addr)) => addr.port(),
        Some(SocketAddr::V6(addr)) => addr.port(),
        None => return Err("local server did not bind to an IP address".into()),
    };

    let stop_flag = Arc::new(AtomicBool::new(false));
    let thread_stop_flag = stop_flag.clone();

    let handle = std::thread::spawn(move || {
        while !thread_stop_flag.load(Ordering::SeqCst) {
            match server.recv_timeout(Duration::from_millis(200)) {
                Ok(Some(request)) => handle_request(request, &session, runtime_dir.as_deref()),
                Ok(None) => continue,
                Err(_) => break,
            }
        }
    });

    Ok(RunningServer {
        port,
        stop_flag,
        handle: Some(handle),
    })
}

fn handle_request(
    request: tiny_http::Request,
    session: &EmulatorSession,
    runtime_dir: Option<&Path>,
) {
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("");

    let response = match path {
        "/" | "/index.html" => html_response(session, runtime_dir),
        "/rom" => file_response(&session.rom_path),
        "/bios" => match &session.bios_path {
            Some(path) => file_response(path),
            None => not_found(),
        },
        other if other.starts_with("/runtime/") => match runtime_dir {
            Some(dir) => serve_runtime_file(dir, &other["/runtime/".len()..]),
            None => not_found(),
        },
        _ => not_found(),
    };

    let _ = respond(request, response);
}

enum ServedResponse {
    Bytes(Vec<u8>, &'static str),
    File(std::fs::File, String),
    NotFound,
}

fn html_response(session: &EmulatorSession, runtime_dir: Option<&Path>) -> ServedResponse {
    let html = if runtime_dir.is_none() {
        runtime_missing_page()
    } else {
        session_page(session)
    };
    ServedResponse::Bytes(html.into_bytes(), "text/html; charset=utf-8")
}

fn file_response(path: &Path) -> ServedResponse {
    match std::fs::File::open(path) {
        Ok(file) => ServedResponse::File(file, guess_mime(path)),
        Err(_) => ServedResponse::NotFound,
    }
}

fn not_found() -> ServedResponse {
    ServedResponse::NotFound
}

/// Resolves `/runtime/<requested>` safely: rejects any component that could
/// escape `dir` (`..`, absolute paths, root/prefix components) before ever
/// touching the filesystem, rather than joining first and hoping.
fn serve_runtime_file(dir: &Path, requested: &str) -> ServedResponse {
    let requested = requested.split('?').next().unwrap_or("");
    let relative = Path::new(requested);

    let is_safe = relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    if !is_safe {
        return ServedResponse::NotFound;
    }

    file_response(&dir.join(relative))
}

fn guess_mime(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("data") => "application/octet-stream",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn respond(request: tiny_http::Request, response: ServedResponse) -> std::io::Result<()> {
    match response {
        ServedResponse::Bytes(bytes, content_type) => {
            let response =
                Response::from_data(bytes).with_header(content_type_header(content_type));
            request.respond(response)
        }
        ServedResponse::File(file, content_type) => {
            let response =
                Response::from_file(file).with_header(content_type_header(&content_type));
            request.respond(response)
        }
        ServedResponse::NotFound => {
            request.respond(Response::from_string("Not found").with_status_code(404))
        }
    }
}

fn content_type_header(content_type: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("static header name/value is always valid")
}

fn runtime_missing_page() -> String {
    "<!doctype html><html><body style=\"background:#0b0713;color:#e8e3f0;font-family:sans-serif;\
display:flex;align-items:center;justify-content:center;height:100vh;margin:0;text-align:center;\">\
<div><h1>Emulator runtime not installed</h1>\
<p>Wervik hasn't set up the EmulatorJS runtime on this machine yet.</p></div></body></html>"
        .to_string()
}

fn session_page(session: &EmulatorSession) -> String {
    let bios_line = if session.bios_path.is_some() {
        "EJS_biosUrl = '/bios';"
    } else {
        ""
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
html,body{{margin:0;height:100%;background:#000;}}#game{{width:100%;height:100%;}}\
</style></head><body>\
<div id=\"game\"></div>\
<script>\
EJS_player = '#game';\
EJS_gameUrl = '/rom';\
EJS_core = '{core}';\
{bios_line}\
EJS_pathtodata = '/runtime/';\
EJS_startOnLoaded = true;\
</script>\
<script src=\"/runtime/loader.js\"></script>\
</body></html>",
        core = session.core,
        bios_line = bios_line,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn sample_session(rom_path: PathBuf) -> EmulatorSession {
        EmulatorSession {
            rom_path,
            core: "snes9x".into(),
            bios_path: None,
        }
    }

    #[test]
    fn serves_rom_bytes_over_http() {
        let dir = std::env::temp_dir().join(format!("wervik-retro-server-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rom_path = dir.join("game.sfc");
        std::fs::write(&rom_path, b"fake-rom-bytes").unwrap();

        let server = start(sample_session(rom_path), None).unwrap();
        let body = http_get(server.port, "/rom");
        assert_eq!(body, b"fake-rom-bytes");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shows_runtime_missing_page_when_no_runtime_dir_configured() {
        let dir = std::env::temp_dir().join(format!(
            "wervik-retro-server-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rom_path = dir.join("game.sfc");
        std::fs::write(&rom_path, b"data").unwrap();

        let server = start(sample_session(rom_path), None).unwrap();
        let body = http_get(server.port, "/");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("not installed"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Exercises the actual vendored EmulatorJS runtime under
    /// `resources/emulatorjs-runtime` (not a fixture) so a packaging
    /// mistake — a renamed/missing file, a broken relative path — fails a
    /// test instead of only surfacing as a blank screen at runtime.
    #[test]
    fn serves_the_real_vendored_runtime() {
        let runtime_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/emulatorjs-runtime");
        assert!(
            runtime_dir.join("loader.js").exists(),
            "expected the vendored EmulatorJS runtime at {runtime_dir:?}"
        );

        let dir =
            std::env::temp_dir().join(format!("wervik-retro-server-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rom_path = dir.join("game.sfc");
        std::fs::write(&rom_path, b"data").unwrap();

        let server = start(sample_session(rom_path), Some(runtime_dir)).unwrap();

        let index = String::from_utf8_lossy(&http_get(server.port, "/")).to_string();
        assert!(
            !index.contains("not installed"),
            "runtime is vendored, session page should boot the emulator"
        );
        assert!(index.contains("loader.js"));

        let loader =
            String::from_utf8_lossy(&http_get(server.port, "/runtime/loader.js")).to_string();
        assert!(loader.contains("EJS_pathtodata"));

        let cores_json = http_get(server.port, "/runtime/cores/cores.json");
        assert!(!cores_json.is_empty());

        assert_eq!(
            http_get_status(server.port, "/runtime/cores/fceumm-wasm.data"),
            200
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_path_traversal_outside_runtime_dir() {
        let root =
            std::env::temp_dir().join(format!("wervik-retro-server-trav-{}", std::process::id()));
        let runtime = root.join("runtime");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(root.join("secret.txt"), b"should not be reachable").unwrap();
        let rom_path = runtime.join("game.sfc");
        std::fs::write(&rom_path, b"data").unwrap();

        let server = start(sample_session(rom_path), Some(runtime)).unwrap();
        let status = http_get_status(server.port, "/runtime/../secret.txt");
        assert_eq!(status, 404);

        let _ = std::fs::remove_dir_all(&root);
    }

    fn http_get(port: u16, path: &str) -> Vec<u8> {
        let (status, body) = raw_get(port, path);
        assert_eq!(status, 200, "expected 200 for {path}");
        body
    }

    fn http_get_status(port: u16, path: &str) -> u16 {
        raw_get(port, path).0
    }

    fn raw_get(port: u16, path: &str) -> (u16, Vec<u8>) {
        use std::io::Write;
        use std::net::TcpStream;

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();

        let text = String::from_utf8_lossy(&raw);
        let status = text
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);

        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .unwrap_or(raw.len());
        (status, raw[split..].to_vec())
    }
}
