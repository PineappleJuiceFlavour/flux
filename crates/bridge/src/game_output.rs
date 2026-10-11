#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameOutputLogLevel {
    Fatal,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
    Other,
}
