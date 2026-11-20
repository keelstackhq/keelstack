use crate::exec::Runner;
use crate::manifest::Manifest;
use crate::pins::Pins;
use std::path::PathBuf;
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    pub wait_secs: u64,
    pub poll_secs: u64,
    pub token_stdin: bool,
    pub dry_run: bool,
}

pub struct Ctx<'a> {
    pub runner: &'a dyn Runner,
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub pins: Pins,
    pub opts: Opts,
    pub sleep: &'a dyn Fn(Duration),
    pub stdin: &'a dyn Fn() -> std::io::Result<Zeroizing<String>>,
}
