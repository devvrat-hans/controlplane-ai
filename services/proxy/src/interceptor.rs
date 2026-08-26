use controlplane_common::models::InterceptedCall;

pub struct Interceptor;

impl Interceptor {
    pub fn new() -> Self {
        Self
    }

    pub fn capture(&self, _call: &InterceptedCall) {
        // SCAFFOLD: capture logic lives in the proxy handler directly.
        // This struct is reserved for future extraction if capture needs to
        // be decoupled from the proxy request path.
    }
}

impl Default for Interceptor {
    fn default() -> Self {
        Self::new()
    }
}
