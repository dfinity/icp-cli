use std::sync::Arc;

use ic_agent::Agent;

mod start;
pub(crate) use start::*;

mod stop;
pub(crate) use stop::*;

#[allow(clippy::type_complexity)]
pub(crate) struct Initializers {
    pub(crate) start: Box<dyn Fn(&Agent) -> Arc<dyn Start>>,
    pub(crate) stop: Box<dyn Fn(&Agent) -> Arc<dyn Stop>>,
}

impl Default for Initializers {
    fn default() -> Self {
        Self {
            start: Box::new(|_| unimplemented!()),
            stop: Box::new(|_| unimplemented!()),
        }
    }
}
