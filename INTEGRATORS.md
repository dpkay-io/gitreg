# Gitreg Integrator Platform

`gitreg` is designed to be a foundational layer for your local development environment. It provides an event-driven system that allows external applications ("integrators") to receive real-time notifications about repository changes and user actions.

## 🚀 Getting Started

To become an integrator, your application needs to:
1.  **Expose a Socket**: Create a Unix domain socket (on Linux/macOS) or a Named Pipe (on Windows).
2.  **Register with Gitreg**: Tell `gitreg` which events you want to listen for and where your socket/pipe is located.
3.  **Handle Payloads**: Listen on your socket for JSON-encoded event payloads.

## 🛠️ Registration Commands

You can manage your application's registrations using the `gitreg integrator` subcommand.

### Register an App
```sh
gitreg integrator register --app <app-name> --event <event-name> --socket <path>
```
*   `--app`: A unique identifier for your application.
*   `--event`: The event you want to subscribe to.
*   `--socket`: The absolute path to your Unix socket or Windows named pipe (e.g., `\\.\pipe\my-app-pipe`).

### List Registrations
```sh
gitreg integrator ls
```

### Unregister an App
```sh
gitreg integrator unregister --app <app-name> --event <event-name>
```

### Block/Unblock an App
You can temporarily stop an app from receiving any events without removing its registrations.
```sh
gitreg integrator block --app <app-name>
gitreg integrator unblock --app <app-name>
```

### Remove an App
Remove an app and all its associated event registrations.
```sh
gitreg integrator rm --app <app-name>
```

## 📅 Available Events

Use `gitreg integrator events` to see the current list of supported events.

| Event | Description | Payload Data |
|---|---|---|
| `registered` | A new repository has been added to the registry. | `{ "path": "/absolute/path" }` |
| `removed` | A repository has been removed from the registry. | `{ "path": "/absolute/path" }` |
| `tagged` | A tag has been added to a repository. | `{ "target": "id|name|path", "tag": "work" }` |
| `untagged` | A tag has been removed from a repository. | `{ "target": "id|name|path", "tag": "work" }` |
| `upgraded` | `gitreg` has been upgraded to a new version. | `{ "version": "1.2.3" }` |
| `git.<COMMAND>` | A specific `git` command was run via `gitreg git`. | `{ "args": ["commit", "-m", "..."], "repos": ["/path1", ...] }` |

## 📦 Event Payload Format

Every message sent to your socket is a JSON object with the following structure:

```json
{
  "event": "registered",
  "app": "gitreg",
  "data": {
    "path": "/home/user/projects/my-repo"
  }
}
```

*Note: Each event is followed by a newline (`\n`) character.*

## 💻 Implementation Examples

### Unix (Python)
Integrators on Unix can use the `socket` module to listen on a Unix Domain Socket.

```python
import socket
import os
import json

SOCKET_PATH = "/tmp/my-integrator.sock"

if os.path.exists(SOCKET_PATH):
    os.remove(SOCKET_PATH)

server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(SOCKET_PATH)
server.listen(1)

print(f"Listening on {SOCKET_PATH}...")

# Register with gitreg
os.system(f"gitreg integrator register --app my-py-app --event registered --socket {SOCKET_PATH}")

try:
    while True:
        conn, _ = server.accept()
        data = conn.recv(1024)
        if data:
            payload = json.loads(data.decode('utf-8'))
            print(f"Received event: {payload['event']} for {payload['data'].get('path')}")
        conn.close()
finally:
    os.remove(SOCKET_PATH)
```

### Windows (PowerShell)
On Windows, integrators should use Named Pipes.

```powershell
$pipeName = "gitreg-integration"
$pipePath = "\\.\pipe\$pipeName"

# Register with gitreg
gitreg integrator register --app my-pwsh-app --event registered --socket $pipePath

Write-Host "Listening on $pipePath..."

while ($true) {
    $nps = New-Object System.IO.Pipes.NamedPipeServerStream($pipeName, [System.IO.Pipes.PipeDirection]::In)
    $nps.WaitForConnection()
    
    $reader = New-Object System.IO.StreamReader($nps)
    $line = $reader.ReadLine()
    if ($line) {
        Write-Host "Received event: $line"
    }
    
    $nps.Dispose()
}
```

### Quick Test (Linux/macOS)
You can use `nc` (netcat) to quickly test your registration.

```sh
# In terminal 1: Start listening
nc -lkU /tmp/test.sock

# In terminal 2: Register and trigger
gitreg integrator register --app test --event registered --socket /tmp/test.sock
gitreg repo scan . # Triggers 'registered' if new repos found
```
