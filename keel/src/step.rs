use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::HumanStep;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Done(String),
    Todo(String),
    Human(HumanStep),
    Failed(String),
}

pub trait Step {
    fn id(&self) -> &'static str;
    /// Read-only.
    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError>;
    /// Do the work; returns the re-check.
    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError>;
}
