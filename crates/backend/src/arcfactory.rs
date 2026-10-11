use std::sync::Arc;

#[derive(Default)]
pub struct ArcStrFactory {
    last: Arc<str>,
}

impl ArcStrFactory {
    pub fn create(&mut self, string: &str) -> Arc<str> {
        if &*self.last != string {
            self.last = string.into();
        }
        self.last.clone()
    }
}
