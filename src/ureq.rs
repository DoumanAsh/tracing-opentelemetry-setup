use core::{time, fmt};
use core::pin::Pin;
use core::future::{self, Future};

use ureq::Agent;
use opentelemetry_http::{Request, Response, HttpError, Bytes};

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
#[repr(transparent)]
pub struct HttpClient(Agent);

impl HttpClient {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder().user_agent(USER_AGENT)
                                                  .proxy(ureq::Proxy::try_from_env())
                                                  .max_redirects(5)
                                                  .timeout_per_call(Some(time::Duration::from_secs(5)))
                                                  .timeout_connect(Some(time::Duration::from_secs(1)))
                                                  .build();
        Self(Agent::new_with_config(config))
    }
}

impl fmt::Debug for HttpClient {
    #[inline(always)]
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, fmt)
    }
}

impl opentelemetry_http::HttpClient for HttpClient {
    //Handle async_trait garbage
    fn send_bytes<'life0, 'async_trait>(&'life0 self, request: Request<Bytes>) -> Pin<Box<dyn Future<Output = Result<Response<Bytes>, HttpError>> + Send + 'async_trait>> where Self: 'async_trait, 'life0: 'async_trait {

        let request: Request<Vec<u8>> = request.map(|body| body.into());
        let result = self.0.run(request).and_then(|response| {
            let (parts, mut body) = response.into_parts();
            body.read_to_vec().map(|body| {
                Response::from_parts(parts, body.into())
            })
        });
        Box::pin(future::ready(result.map_err(Into::into)))
    }
}
