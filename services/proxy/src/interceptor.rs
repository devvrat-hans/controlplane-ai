use controlplane_common::models::InterceptedCall;

pub struct Interceptor;

impl Interceptor {
    pub fn new() -> Self {
        Self
    }

    pub fn capture(&self, _call: &InterceptedCall) {
        // Will be implemented: persist + publish to shadow path
    }
}

impl Default for Interceptor {
    fn default() -> Self {
        Self::new()
    }
}
