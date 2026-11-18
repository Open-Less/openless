// Exercise the production WebSocket handler without native desktop/ASR DLLs.
#[allow(dead_code)] // The CA field is consumed by the HTTPS host outside this test crate.
#[path = "../../src/remote_server/websocket.rs"]
mod websocket;
