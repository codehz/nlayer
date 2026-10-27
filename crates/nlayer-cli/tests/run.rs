//! `nlayer run` serves the page and replaces it only after a good rebuild.

use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use nlayer_cli::{RunOptions, serve};

fn png() -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    out.extend(2u32.to_be_bytes());
    out.extend(2u32.to_be_bytes());
    out.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    out
}

fn source(label: &str) -> String {
    r#"import { mount } from "@nlayer/runtime";
import { Image, Text, View } from "@nlayer/ui";
import icon from "./img/icon.png";

function App() {
  return (
    <View style={{ width: 20, height: 20 }}>
      <Text>LABEL</Text>
      <Image src={icon} />
    </View>
  );
}
mount(App);
"#
    .replace("LABEL", label)
}

struct Response {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

fn get(port: u16, path: &str) -> Response {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        socket,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut buf = Vec::new();
    socket.read_to_end(&mut buf).unwrap();
    let split = buf
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response headers");
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let content_type = head
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                (name.eq_ignore_ascii_case("content-type")).then(|| value.trim().to_owned())
            })
        })
        .unwrap_or_default();
    Response {
        status,
        content_type,
        body: buf[split + 4..].to_vec(),
    }
}

fn text(response: &Response) -> String {
    String::from_utf8_lossy(&response.body).into_owned()
}

fn wait_until(limit: Duration, mut pred: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < limit {
        if pred() {
            return;
        }
        thread::sleep(Duration::from_millis(30));
    }
    assert!(pred(), "timed out");
}

#[test]
fn run_serves_the_page_and_keeps_it_across_a_failed_rebuild() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nlayer-run-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("img")).unwrap();
    let image = png();
    fs::write(dir.join("img/icon.png"), &image).unwrap();
    fs::write(dir.join("App.tsx"), source("Hello")).unwrap();
    fs::write(dir.join("quote.json"), b"{\"title\":\"Gallery\"}").unwrap();

    let module = vec![7_u8; 1 << 20];
    let server = serve(RunOptions {
        entry: dir.clone(),
        port: 0,
        release: false,
        wasm: module.clone(),
        on_build: None,
    })
    .expect("the server starts");
    assert_eq!(
        server.generation(),
        1,
        "{}",
        server.last_error().unwrap_or_default()
    );
    let port = server.port();

    let page = get(port, "/");
    assert_eq!(page.status, 200);
    let html = text(&page);
    assert!(html.contains("__nlayer/generation"), "{html}");
    assert!(html.contains("location.reload"), "{html}");
    assert!(html.contains("startPage"), "{html}");
    assert_eq!(get(port, "/__nlayer/generation").status, 200);
    assert_eq!(text(&get(port, "/__nlayer/generation")), "1");

    let bundle = text(&get(port, "/bundle.js"));
    assert!(bundle.contains("Hello"), "{bundle}");
    assert!(bundle.contains("asset:/img/icon.png"), "{bundle}");
    assert!(
        bundle.contains("sourceMappingURL=bundle.js.map"),
        "{bundle}"
    );
    assert!(text(&get(port, "/bundle.js.map")).contains("App.tsx"));
    assert!(text(&get(port, "/boot.js")).contains("startPage"));
    assert!(text(&get(port, "/browser/runtime.js")).contains("createFacade"));
    assert!(text(&get(port, "/base.css")).contains("nl-root"));

    let wasm = get(port, "/nlayer.wasm");
    assert_eq!(wasm.status, 200);
    assert_eq!(wasm.content_type, "application/wasm");
    assert_eq!(wasm.body, module);
    let icon = get(port, "/img/icon.png");
    assert_eq!(icon.status, 200);
    assert_eq!(icon.content_type, "image/png");
    assert_eq!(icon.body, image);
    let quote = get(port, "/quote.json");
    assert_eq!(quote.status, 200);
    assert_eq!(quote.content_type, "application/json");
    assert_eq!(text(&quote), "{\"title\":\"Gallery\"}");
    assert_eq!(get(port, "/missing").status, 404);
    assert_eq!(get(port, "/../Cargo.toml").status, 400);
    assert_eq!(get(port, "/%2e%2e/Cargo.toml").status, 400);

    fs::write(dir.join("App.tsx"), source("Hola")).unwrap();
    wait_until(Duration::from_secs(30), || {
        text(&get(port, "/bundle.js")).contains("Hola")
    });
    assert_eq!(server.generation(), 2);
    assert!(server.last_error().is_none());
    assert!(!text(&get(port, "/bundle.js")).contains("Hello"));

    fs::write(dir.join("App.tsx"), "const = ;\n").unwrap();
    wait_until(Duration::from_secs(30), || server.last_error().is_some());
    assert_eq!(server.generation(), 2, "a failed rebuild must not reload");
    assert!(text(&get(port, "/bundle.js")).contains("Hola"));
    assert!(!server.last_error().unwrap().is_empty());

    fs::write(dir.join("App.tsx"), source("Adios")).unwrap();
    wait_until(Duration::from_secs(30), || {
        text(&get(port, "/bundle.js")).contains("Adios")
    });
    assert_eq!(server.generation(), 3);
    assert!(server.last_error().is_none());

    drop(server);
    let _ = fs::remove_dir_all(Path::new(&dir));
}
