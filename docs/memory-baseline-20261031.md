# OpenLess desktop memory baseline

Measured on the installed ARM64 beta app on macOS 27.0.1, October 31, 2026, after six minutes of idle time.

| Process | Physical footprint |
| --- | ---: |
| OpenLess Rust host | 65.2 MB |
| WebKit WebContent (main window) | 252.3 MB |
| WebKit WebContent (capsule window) | 120.5 MB |
| WebKit GPU process | about 107.7 MB RSS |

The idle total is about 545 MB across the app and its WebKit helper processes. The capsule WebView is hidden but created during startup, so it contributes a second WebContent process before the user records anything.

The memory follow-up changes capsule creation to lazy creation. The capsule and Windows transcript rail are created only when their presentation path needs them. This keeps the visual behavior and native panel setup while removing the hidden capsule WebView from the idle launch footprint.

The page transition path retains the outgoing page only during the handoff. Once the incoming page reaches its first useful frame and its enter motion completes, the outgoing surface unmounts. Memory optimization should be measured again after the lazy capsule change before any further page cache changes are attempted.
