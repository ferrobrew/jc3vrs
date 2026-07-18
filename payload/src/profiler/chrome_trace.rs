//! Converts captured puffin frames into a Chrome trace-event JSON file, viewable in
//! `ui.perfetto.dev` (or `chrome://tracing`). Each puffin scope becomes one complete ("X") event;
//! each puffin thread stream — including the synthetic GPU track — becomes its own named thread
//! lane on the shared timeline.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    sync::Arc,
};

use anyhow::Context;
use puffin::{FrameData, Reader, Scope, ScopeCollection, Stream};

/// Writes `frames` as a Chrome trace-event JSON array to `path`.
///
/// Scope names resolve through `base_scopes` (the profiler's continuously harvested collection)
/// plus the frames' own `scope_delta` records for anything registered mid-capture.
pub fn write_chrome_trace(
    path: &Path,
    frames: &[Arc<FrameData>],
    base_scopes: ScopeCollection,
) -> anyhow::Result<()> {
    let mut scopes = base_scopes;
    for frame in frames {
        for details in &frame.scope_delta {
            scopes.insert(details.clone());
        }
    }

    // Rebase all timestamps to the capture's start: puffin's nanosecond clock is epoch-scale, and
    // the trace-event format carries microseconds as JSON doubles, so absolute values would lose
    // the sub-microsecond fraction to f64 precision.
    let base_ns = frames
        .iter()
        .map(|f| f.meta().range_ns.0)
        .min()
        .unwrap_or_default();

    let file = File::create(path)
        .with_context(|| format!("chrome trace: could not create {}", path.display()))?;
    let mut writer = EventWriter {
        out: BufWriter::new(file),
        first: true,
        thread_ids: HashMap::new(),
        scopes,
        base_ns,
    };
    writer.out.write_all(b"[")?;

    for frame in frames {
        let frame = frame
            .unpacked()
            .map_err(|e| anyhow::anyhow!("chrome trace: could not unpack a frame: {e}"))?;
        for (thread_info, stream_info) in &frame.thread_streams {
            let tid = writer.thread_lane(thread_info)?;
            for scope in Reader::from_start(&stream_info.stream) {
                let scope =
                    scope.map_err(|e| anyhow::anyhow!("chrome trace: bad scope record: {e:?}"))?;
                writer.write_scope(&stream_info.stream, &scope, tid)?;
            }
        }
    }

    writer.out.write_all(b"\n]")?;
    writer
        .out
        .flush()
        .with_context(|| format!("chrome trace: could not flush {}", path.display()))?;
    Ok(())
}

struct EventWriter {
    out: BufWriter<File>,
    first: bool,
    /// Chrome wants integer thread ids; puffin identifies streams by a whole [`ThreadInfo`]
    /// (name plus start time). Key the lanes by that full identity — two distinct threads can
    /// share a name (foreign threads all report an empty one when unlabelled), and merging them
    /// would interleave concurrent scopes onto one garbled track. An "M" metadata row labels each
    /// lane on first sight.
    thread_ids: HashMap<(String, Option<i64>), u64>,
    scopes: ScopeCollection,
    /// The capture's earliest nanosecond, subtracted from every event timestamp.
    base_ns: i64,
}

impl EventWriter {
    /// The Chrome tid for a puffin thread, emitting the naming metadata row the first time.
    fn thread_lane(&mut self, info: &puffin::ThreadInfo) -> anyhow::Result<u64> {
        let key = (info.name.clone(), info.start_time_ns);
        if let Some(&tid) = self.thread_ids.get(&key) {
            return Ok(tid);
        }
        let tid = self.thread_ids.len() as u64;
        self.thread_ids.insert(key, tid);
        let label = if info.name.is_empty() {
            match info.start_time_ns {
                Some(start) => format!("thread @{start}"),
                None => format!("thread #{tid}"),
            }
        } else {
            info.name.clone()
        };
        self.begin_event()?;
        write!(
            self.out,
            r#"{{"ph":"M","name":"thread_name","pid":0,"tid":{tid},"args":{{"name":{}}}}}"#,
            serde_json::to_string(&label)?,
        )?;
        Ok(tid)
    }

    /// Writes `scope` and, recursively, its children as complete events on `tid`.
    fn write_scope(&mut self, stream: &Stream, scope: &Scope<'_>, tid: u64) -> anyhow::Result<()> {
        let name = self
            .scopes
            .fetch_by_id(&scope.id)
            .map(|details| details.name().to_string())
            .unwrap_or_else(|| format!("scope #{}", scope.id.0));
        self.begin_event()?;
        // Timestamps are microseconds in the trace-event format; keep nanosecond precision via
        // the fractional part.
        write!(
            self.out,
            r#"{{"ph":"X","name":{},"cat":"scope","pid":0,"tid":{tid},"ts":{},"dur":{}"#,
            serde_json::to_string(&name)?,
            (scope.record.start_ns - self.base_ns) as f64 / 1000.0,
            scope.record.duration_ns as f64 / 1000.0,
        )?;
        if !scope.record.data.is_empty() {
            write!(
                self.out,
                r#","args":{{"data":{}}}"#,
                serde_json::to_string(scope.record.data)?
            )?;
        }
        self.out.write_all(b"}")?;

        for child in Reader::with_offset(stream, scope.child_begin_position)
            .map_err(|e| anyhow::anyhow!("chrome trace: bad child offset in a stream: {e:?}"))?
        {
            let child =
                child.map_err(|e| anyhow::anyhow!("chrome trace: bad scope record: {e:?}"))?;
            self.write_scope(stream, &child, tid)?;
        }
        Ok(())
    }

    fn begin_event(&mut self) -> std::io::Result<()> {
        if self.first {
            self.first = false;
            self.out.write_all(b"\n")
        } else {
            self.out.write_all(b",\n")
        }
    }
}
