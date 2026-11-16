//! `GET /v1/sessions/{id}/stream` over reqwest's `bytes_stream()`.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::Stream;
use url::Url;

use crate::codegen::error::{Error, Result, parse_error_response};
use crate::codegen::transport::HttpClient;
use crate::events::Event;
use crate::sse::{Frame, SseDecoder};

type Body = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

/// A session's events, until the server ends the stream (`[DONE]`) or the connection
/// drops. Not restartable: open a new one, and resync from a snapshot.
pub struct EventStream {
    body: Option<Body>,
    decoder: SseDecoder,
    ready: VecDeque<Event>,
}

impl std::fmt::Debug for EventStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStream")
            .field("open", &self.body.is_some())
            .field("ready", &self.ready.len())
            .finish()
    }
}

pub(crate) async fn open(
    http: &HttpClient,
    base_url: &Url,
    session_id: &str,
) -> Result<EventStream> {
    let mut url = base_url.join("v1/sessions/")?;
    url.path_segments_mut()
        .map_err(|()| Error::Url(url::ParseError::RelativeUrlWithCannotBeABaseBase))?
        .pop_if_empty()
        .push(session_id)
        .push("stream");
    let response = http
        .get(url)
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(parse_error_response(response).await);
    }
    Ok(EventStream {
        body: Some(Box::pin(response.bytes_stream())),
        decoder: SseDecoder::new(),
        ready: VecDeque::new(),
    })
}

impl EventStream {
    /// A stream over `body`, an SSE body in chunks (for tests, and for a caller with its
    /// own connection).
    pub fn from_body(body: impl Stream<Item = reqwest::Result<Bytes>> + Send + 'static) -> Self {
        Self {
            body: Some(Box::pin(body)),
            decoder: SseDecoder::new(),
            ready: VecDeque::new(),
        }
    }

    fn take_frames(&mut self, frames: Vec<Frame>) {
        for frame in frames {
            match frame {
                Frame::Event { event, data } => {
                    self.ready.push_back(Event::parse(event.as_deref(), &data));
                }
                Frame::Done => self.body = None,
            }
        }
    }
}

impl Stream for EventStream {
    type Item = Result<Event>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.ready.pop_front() {
                return Poll::Ready(Some(Ok(event)));
            }
            let Some(body) = self.body.as_mut() else {
                return Poll::Ready(None);
            };
            match body.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => {
                    let frames = self.decoder.push(&chunk);
                    self.take_frames(frames);
                }
                Poll::Ready(Some(Err(error))) => {
                    self.body = None;
                    return Poll::Ready(Some(Err(Error::Http(error))));
                }
                Poll::Ready(None) => {
                    self.body = None;
                    let frames = self.decoder.finish();
                    self.take_frames(frames);
                }
            }
        }
    }
}
