//! The `progress` and `log` interfaces: a plugin's tasks under the call's
//! tracker, and its log lines under its id.

use in_progress::{Level, Progress, Tracker, Unit};
use wasmtime::component::Resource;

use super::api::{self, progress as wp};
use crate::progress::Task;
use crate::runtime::HostState;

/// A plugin's units are sheaf's, with the nouns the status bar shows.
fn unit(u: wp::Unit) -> Unit {
    match u {
        wp::Unit::Items => Unit::ITEMS,
        wp::Unit::Pages => sheaf_doc::PAGES,
        wp::Unit::Bytes => Unit::Bytes,
    }
}

impl wp::Host for HostState {}

impl wp::HostTask for HostState {
    fn new(&mut self, label: String, total: Option<u64>, u: wp::Unit) -> Resource<Task> {
        let t = match self.progress() {
            Some(root) if self.has_task_room() => root.child_tracker(&label, 1.0),
            _ => Tracker::new(),
        };
        t.set_total(total, unit(u));
        self.push_task(t)
    }

    fn set_total(&mut self, this: Resource<Task>, total: Option<u64>, u: wp::Unit) {
        if let Some(t) = self.task(&this) {
            t.set_total(total, unit(u));
        }
    }

    fn set(&mut self, this: Resource<Task>, done: u64) {
        if let Some(t) = self.task(&this) {
            t.set(done);
        }
    }

    fn inc(&mut self, this: Resource<Task>, n: u64) {
        if let Some(t) = self.task(&this) {
            t.inc(n);
        }
    }

    fn message(&mut self, this: Resource<Task>, level: wp::Level, message: String) {
        if let Some(t) = self.task(&this) {
            let level = match level {
                wp::Level::Info => Level::Info,
                wp::Level::Warn => Level::Warn,
                wp::Level::Error => Level::Error,
            };
            // A status line, not a document.
            let message: String = message.chars().take(500).collect();
            t.message(level, &message);
        }
    }

    /// Only the call is ever cancelled, so ask it: a task past the cap
    /// isn't under it.
    fn is_cancelled(&mut self, _this: Resource<Task>) -> bool {
        self.progress().is_some_and(|t| t.is_cancelled())
    }

    fn child(&mut self, this: Resource<Task>, label: String, weight: f32) -> Resource<Task> {
        let weight = if weight.is_finite() {
            f64::from(weight)
        } else {
            0.0
        };
        let child = match self.task(&this) {
            Some(t) if self.has_task_room() => t.child_tracker(&label, weight),
            _ => Tracker::new(),
        };
        self.push_task(child)
    }

    fn drop(&mut self, this: Resource<Task>) -> wasmtime::Result<()> {
        let rep = this.rep();
        self.task_handles.retain(|r| *r != rep);
        let _ = self.table.delete(this);
        Ok(())
    }
}

impl api::log::Host for HostState {
    fn log(&mut self, level: api::log::Level, message: String) {
        use api::log::Level;
        let level = match level {
            Level::Trace => log::Level::Trace,
            Level::Debug => log::Level::Debug,
            Level::Info => log::Level::Info,
            Level::Warn => log::Level::Warn,
            Level::Error => log::Level::Error,
        };
        log::log!(target: "plugin", level, "[{}] {message}", self.plugin);
    }
}

#[cfg(test)]
mod tests {
    use in_progress::Unit;

    use super::wp;

    /// A plugin's units are sheaf's, with the nouns the status bar shows.
    #[test]
    fn units_cross_the_api() {
        let units = [wp::Unit::Items, wp::Unit::Pages, wp::Unit::Bytes].map(super::unit);
        assert_eq!(units, [Unit::ITEMS, sheaf_doc::PAGES, Unit::Bytes]);
        assert_eq!(units.map(Unit::noun), ["items", "pages", "bytes"]);
    }
}
