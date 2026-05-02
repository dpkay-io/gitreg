use crate::db::Database;
use serde_json::json;
use std::io::Write;
use std::thread;

pub fn dispatch(db: &Database, event: &str, data: serde_json::Value) {
    let listeners = match db.get_active_listeners_for_event(event) {
        Ok(l) if l.is_empty() => return,
        Ok(l) => l,
        Err(_) => return,
    };

    let payload = json!({
        "event": event,
        "app": "gitreg",
        "data": data,
    });

    let payload_str = match serde_json::to_string(&payload) {
        Ok(s) => s,
        Err(_) => return,
    };

    for socket_path in listeners {
        let p_str = payload_str.clone();
        thread::spawn(move || {
            let _ = send_to_socket(&socket_path, &p_str);
        });
    }
}

#[cfg(unix)]
fn send_to_socket(path: &str, payload: &str) -> std::io::Result<()> {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_millis(100)))?;
    stream.write_all(payload.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

#[cfg(windows)]
fn send_to_socket(path: &str, payload: &str) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    // On Windows, integrators should register pipes like \\.\pipe\my-pipe
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(payload.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}
