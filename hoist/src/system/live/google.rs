//! The `google` live probe (spec §17.5).

use super::{Ctx, LiveProbe, LiveResult};
use crate::system::Credential;

pub struct Probe;

impl Probe {
    pub fn new(_ctx: &Ctx) -> Probe {
        Probe
    }
}

impl LiveProbe for Probe {
    fn provider(&self) -> &str {
        "google"
    }

    fn probe(&self, _cred: &Credential) -> LiveResult {
        LiveResult::Unknown("not implemented".into())
    }
}
