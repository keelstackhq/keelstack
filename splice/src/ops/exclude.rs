//! `splice channel exclude NAME PATH...`: add paths to a channel's stored `exclude`.
//!
//! The paths leave the channel's lock `files` too, so `check` stops reporting them (deleted or
//! edited) and the next `update` neither restores nor removes them. The disk is never touched.

use crate::cli::{ChannelExcludeArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::filter::Filter;
use crate::lock::ChannelLock;
use crate::output::{Action, Class, Report};

pub fn add(g: &GlobalArgs, a: &ChannelExcludeArgs) -> Result<Report, SpliceError> {
    let repo = g.dir.as_path();
    let Some(mut cl) = ChannelLock::read(repo, &a.name)? else {
        return Err(SpliceError::Usage(format!(
            "no channel `{}` in {}; see .splice/channels/",
            a.name,
            repo.display()
        )));
    };
    // Every glob must compile before anything is written.
    Filter::new(&[], &a.paths)?;
    let mut report = Report::new("channel exclude");
    let mut dirty = false;
    for p in &a.paths {
        if !cl.meta.exclude.contains(p) {
            cl.meta.exclude.push(p.clone());
            dirty = true;
        }
        if let Some(f) = cl.lock.files.remove(p) {
            dirty = true;
            report.push(p.clone(), Action::Skip, f.class);
        } else {
            report.push(p.clone(), Action::Skip, Class::Owned);
        }
    }
    if dirty {
        cl.write_atomic(repo)?;
    } else {
        report.msg(format!("channel {}: already excluded", a.name));
    }
    Ok(report)
}
