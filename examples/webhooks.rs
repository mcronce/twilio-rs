use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use twilio::twiml::{Say, Twiml, Voice};

async fn handle(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let app_id = "<app-id>";
    let auth_token = "<auth-token>";
    let client = twilio::Client::new(app_id, auth_token);

    let cloned_uri = req.uri().clone();
    println!("Got a request for: {}", cloned_uri);

    let response = match cloned_uri.path() {
        "/message" => {
            client
                .respond_to_webhook(req, |msg: twilio::Message| {
                    let mut t = Twiml::default();
                    t.add(&twilio::twiml::Message {
                        txt: format!("You told me: '{}'", msg.body.unwrap()),
                    });
                    t
                })
                .await
        }
        "/call" => {
            client
                .respond_to_webhook(req, |_: twilio::Call| {
                    let mut t = Twiml::default();
                    t.add(&Say {
                        txt: "Thanks for using twilio-rs. Bye!".to_string(),
                        voice: Voice::Woman,
                        language: "en".to_string(),
                    });
                    t
                })
                .await
        }
        _ => panic!("Hit an unknown path."),
    };

    Ok(response)
}

#[tokio::main]
async fn main() {
    let service = service_fn(handle);

    let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("Listening on http://{}", addr);

    let mut tasks = JoinSet::new();
    loop {
        let (stream, _addr) = listener.accept().await.unwrap();
        let serve_connection = async move {
            let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .http1()
                .http2()
                .serve_connection(TokioIo::new(stream), service)
                .await;
        };
        tasks.spawn(serve_connection);
    }
}
