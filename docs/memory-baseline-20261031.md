# OpenLess desktop memory baseline

Measured on the installed ARM64 beta app on macOS 27.0.1, October 31, 2026, after six minutes of idle time.

| Process | Physical footprint |
| --- | ---: |
| OpenLess Rust host | 65.2 MB |
| WebKit WebContent (main window) | 252.3 MB |
| WebKit WebContent (capsule window) | 120.5 MB |
| WebKit GPU process | about 107.7 MB RSS |

The idle total is about 545 MB across the app and its WebKit helper processes. The capsule is a high-frequency voice input surface and should remain resident for immediate access. The low-frequency main window is the optimization target: closing it now destroys its WebView so WebKit can release the main content process and the existing tray path recreates the page on the next open.

The page transition path retains the outgoing page only during the handoff. Once the incoming page reaches its first useful frame and its enter motion completes, the outgoing surface unmounts. Memory optimization should be measured again after the main-window destroy change before any further page cache changes are attempted.
