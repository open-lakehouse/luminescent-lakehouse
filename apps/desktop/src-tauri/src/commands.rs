//! The commands the UI invokes (through `src/host/bindings.ts`, generated from these).
//! Each is a thin adapter over `shell-core`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use futures::future::{AbortHandle, Abortable};
use serde::{Deserialize, Serialize};
use shell_core::services::{ProxyError, ProxyRequest, ResponseHead, ServiceInfo};
use shell_core::toolchain::{self, DoctorReport, HostSystem};
use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};
use tauri::{AppHandle, State};

use crate::AppState;

/// In-flight proxied bodies, by the UI's request id, so the UI can cancel one.
pub type Inflight = Arc<Mutex<HashMap<u32, AbortHandle>>>;

/// Probe Docker, Compose, the daemon and the Neon CLI.
#[tauri::command]
#[specta::specta]
pub async fn doctor() -> Result<DoctorReport, String> {
    tauri::async_runtime::spawn_blocking(|| toolchain::doctor(&HostSystem))
        .await
        .map_err(|e| e.to_string())
}

/// The services the current environment binds (no URLs).
#[tauri::command]
#[specta::specta]
pub fn list_services(state: State<'_, AppState>) -> Vec<ServiceInfo> {
    state.proxy.services()
}

/// One message on a proxied body's channel. Chunks go as raw bytes (an `ArrayBuffer` in the
/// UI, no JSON array of numbers); the end, or an error, as a small JSON object.
pub enum BodyMessage {
    Chunk(Vec<u8>),
    End,
    Error(ProxyError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum BodyControl {
    End,
    Error(ProxyError),
}

impl IpcResponse for BodyMessage {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        match self {
            BodyMessage::Chunk(bytes) => Ok(InvokeResponseBody::Raw(bytes)),
            BodyMessage::End => BodyControl::End.body(),
            BodyMessage::Error(e) => BodyControl::Error(e).body(),
        }
    }
}

/// The UI sees `Channel<BodyMessage>`, with `BodyMessage` as `ArrayBuffer | "end" | { error }`
/// written by hand in `src/host/service-fetch.ts`: specta has no `ArrayBuffer`.
#[derive(specta::Type)]
#[specta(remote = BodyMessage)]
#[allow(dead_code)]
struct BodyMessageShape(specta_typescript::Unknown);

/// Proxy a request to a bound service. Resolves with the status and headers once they
/// arrive; the body follows on `on_body`, ending with `"end"` (or `{ error }`).
#[tauri::command]
#[specta::specta]
pub async fn service_fetch(
    state: State<'_, AppState>,
    request_id: u32,
    request: ProxyRequest,
    on_body: Channel<BodyMessage>,
) -> Result<ResponseHead, ProxyError> {
    let (head, mut body) = state.proxy.send(request).await?;
    let (handle, registration) = AbortHandle::new_pair();
    let inflight = state.inflight.clone();
    inflight
        .lock()
        .expect("inflight lock")
        .insert(request_id, handle);
    tauri::async_runtime::spawn(async move {
        let pump = async {
            while let Some(chunk) = body.next().await {
                let message = match chunk {
                    Ok(bytes) => BodyMessage::Chunk(bytes.to_vec()),
                    Err(e) => {
                        let _ = on_body.send(BodyMessage::Error(e));
                        return;
                    }
                };
                // The webview is gone (or reloaded): stop reading.
                if on_body.send(message).is_err() {
                    return;
                }
            }
            let _ = on_body.send(BodyMessage::End);
        };
        let _ = Abortable::new(pump, registration).await;
        inflight.lock().expect("inflight lock").remove(&request_id);
    });
    Ok(head)
}

/// Stop streaming a proxied body (the UI's `AbortSignal`, or a cancelled `ReadableStream`).
#[tauri::command]
#[specta::specta]
pub fn service_fetch_cancel(state: State<'_, AppState>, request_id: u32) {
    if let Some(handle) = state
        .inflight
        .lock()
        .expect("inflight lock")
        .remove(&request_id)
    {
        handle.abort();
    }
}

/// What the in-app smoke test (`--smoke`) found.
#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SmokeReport {
    pub passed: Vec<String>,
    pub failures: Vec<String>,
}

/// End a `--smoke` run: print the report and exit 0 (all passed) or 1.
#[tauri::command]
#[specta::specta]
pub fn smoke_report(
    app: AppHandle,
    state: State<'_, AppState>,
    report: SmokeReport,
) -> Result<(), String> {
    if !state.smoke {
        return Err("not running with --smoke".into());
    }
    for check in &report.passed {
        println!("smoke: ok   {check}");
    }
    for failure in &report.failures {
        eprintln!("smoke: FAIL {failure}");
    }
    let code = i32::from(!report.failures.is_empty() || report.passed.is_empty());
    println!("smoke: {}", if code == 0 { "passed" } else { "failed" });
    app.exit(code);
    Ok(())
}
