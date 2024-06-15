use hudsucker::{
    certificate_authority::RcgenAuthority,
    hyper::{Request, Response},
    rcgen::{CertificateParams, KeyPair},
    tokio_tungstenite::tungstenite::Message,
    *,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::print_err;

pub enum Signal {
    StartListening,
    StopListening(tokio::sync::oneshot::Sender<Vec<String>>),
}

#[derive(Clone)]
struct Interceptor {
    tx: Arc<Sender<String>>,
}

impl HttpHandler for Interceptor {
    async fn handle_request(
        &mut self,
        _ctx: &HttpContext,
        req: Request<Body>,
    ) -> RequestOrResponse {
        if req
            .uri()
            .to_string()
            .contains("https://player02.getcourse.ru:443/player")
        {
            print_err!(self.tx.send(req.uri().to_string()).await, ());
        }
        req.into()
    }
}

pub async fn run_interceptor(wd_rx: Receiver<Signal>) {
    let key_pair = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCaHrZ9VUL7SKqg
JdeG/dciikuummsrDs4Cn+p01J5mURQ/bq62EU5IiWFsM0LgwkO0UlTGE9gU+U1w
1GeDxlDeVqcwitx+sPjup59ybC2l/iNihETjrLuFUaZVFH/eqdXqNgiRf1fBHV/R
D27BlPN6OPo8Z6qTfuRlVAUbpj+IViIkdQtmymgCJDrQabtnnwpPqZA0MQFnsAvm
elOrWyTX1ASjzFAlWWq7JDvFsrYcqPeMv8QaN/teESFN5pMSNDbvYm2guwcRu4jV
L75hMezqm6StXe5pCbbZOKPHLGqtRUAHnUFHEV1khOEz5HhOsC6m0DvWvKf5QiUv
qcYFYxFZAgMBAAECggEARsXZyV4w3xG0gMw/19aTR2I4dNqmYeRvh9cFpFbK0nNj
F+nswuDZkQe9PCGiEXJEAvdXxInyTVdaT3jKfEHCewdRyUHLFUaRWY6R8spof/Rf
LWtN8zsr9YHUHvfF7GsTN3VOo/nVQ3IIwQkUNEMBN9wYVUrJkufPXBSkL9k9DY7B
uoUghTbpj2nZhSQYRWyAkbc5CcpJzuBDhSE8m2X+0ZMuoW91rOw+G2OLlBwiIOFg
PHPiS0wAMyKycmDnCwm4NDu8BK/I6ucORfFlCyroh63riz/BzJo6FFrzRTd70pHG
2VwAo+58LkRYkJjW7gJZ0XzoAxEKntsIYf8r25jzXQKBgQDHq8BOTDGkEtUPR8PE
ICKUFck3fHRlI39Sv2P2JBoPgGP9zy2xyycV/Uc3+tbNnmkUrTFPIWTQSorb0i4R
fhg/3Jqtm2NUb1lrLo9oEDEqRgCSLbLYO1zLFD42s7dW7DOEr+q/54pcAh/xVrDB
L4CSnuWpkGB5PiuTugaJB8TOAwKBgQDFmUUMosNxofUFE5HNYX3+N0rMNX4XgXG7
+TS36XJrKo7PQMTnqD5VUr7Iii/1Ncmr/dANSTUzRErkTjm9gbHllakDqy0rTweB
EtskWoN2s45g+i7xocMVNSkLWDd03NzyMM6riArRz2hkWeTSkJLn+AmFm+q+cFHj
yL+6Nx+CcwKBgQCgT4tE0fQBMYWSkSHic5KPprY5MFkbYta1Dyko1G+ABqtBenfL
ibpF82ac0W5pBEiF60/tongYq+C1ARkvvjel/m7J+DpV7liyr11ARc/TiwSmWL6A
0Zh9DDGvJbeLuHTckYk+rp3tpV8UG3AqiwMFtUHbVCnA7mN6Zh8dIfmnFQKBgH0q
66xnZfqTJwxCKze4LAFesQjOUcM+AfeakqR1Qj9URAZQ9unvjxypP6T0tBBWNBu4
uZPQ7dw9xFr+mmDKyQ+vT9K9Ge23L/+5HAvZMjF86BHSKO5zE4pZlFhVVzu1tFfO
RvwtPv1Mrsnyj5o6bnR2kEGMVJSxvY3W2mxxAoq1AoGAT2sgBDO0CFydMsrxZPno
YYu8LfNefE2KuVFcoLxiseRK/OHOzw7RtSOa0ZW0omXnawwMEje+LavYCO2GerdT
cwJvbfFUqX1F3FqnbyE7vOFkQnzDboLZuw7kY8JeqEdP34CniG1bX4o3Ni2FrAb/
01/t/macxPNVtcpvKsABzJY=
-----END PRIVATE KEY-----";
    let ca_cert = "-----BEGIN CERTIFICATE-----
MIIDkzCCAnugAwIBAgIJAMdLw5xpuf6yMA0GCSqGSIb3DQEBCwUAMGYxHTAbBgNV
BAMMFEh1ZHN1Y2tlciBJbmR1c3RyaWVzMR0wGwYDVQQKDBRIdWRzdWNrZXIgSW5k
dXN0cmllczELMAkGA1UEBgwCVVMxCzAJBgNVBAgMAk5ZMQwwCgYDVQQHDANOWUMw
IBcNNzUwMTAxMDAwMDAwWhgPNDA5NjAxMDEwMDAwMDBaMGYxHTAbBgNVBAMMFEh1
ZHN1Y2tlciBJbmR1c3RyaWVzMR0wGwYDVQQKDBRIdWRzdWNrZXIgSW5kdXN0cmll
czELMAkGA1UEBgwCVVMxCzAJBgNVBAgMAk5ZMQwwCgYDVQQHDANOWUMwggEiMA0G
CSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCaHrZ9VUL7SKqgJdeG/dciikuummsr
Ds4Cn+p01J5mURQ/bq62EU5IiWFsM0LgwkO0UlTGE9gU+U1w1GeDxlDeVqcwitx+
sPjup59ybC2l/iNihETjrLuFUaZVFH/eqdXqNgiRf1fBHV/RD27BlPN6OPo8Z6qT
fuRlVAUbpj+IViIkdQtmymgCJDrQabtnnwpPqZA0MQFnsAvmelOrWyTX1ASjzFAl
WWq7JDvFsrYcqPeMv8QaN/teESFN5pMSNDbvYm2guwcRu4jVL75hMezqm6StXe5p
CbbZOKPHLGqtRUAHnUFHEV1khOEz5HhOsC6m0DvWvKf5QiUvqcYFYxFZAgMBAAGj
QjBAMA4GA1UdDwEB/wQEAwIBBjAdBgNVHQ4EFgQUsv65aZzDS8dPy3NWpXkAOKf0
2rMwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEAVnXJDTCCcV/c
79IidGy3Hh/st+4e2A6R3YueE01Rwo340Asp9Tp3IewDQcF3oRosgDp/i9daRrxv
c2q76CNmo57qUSjbdyu4o5SDqj7lmr263YgM4ZnVOQR9CaWwCL21C65tpgHa8Grm
hNil9REdnpM7br4H0yeX2nFjOYI8sUguxNle3ojTFLl0sWXZIPJE/koEaaHGSJD1
XR72llJbbExYbTzaEV3uw7sJsuwldMC/QL+oWm/Jnwc2WfLTl3HjLOaK9r/smF/E
RtYk5yo7J6pMALrIP7SPHpFooez5JHn2ucP42HcUwOXmrDIOUt6gJQ4w8DBE46Bo
oMKSHK2k0g==
-----END CERTIFICATE-----";

    let key_pair =
        KeyPair::from_pem(key_pair).expect("Failed to parse private key");
    let ca_cert = CertificateParams::from_ca_cert_pem(ca_cert)
        .expect("Failed to parse CA certificate")
        .self_signed(&key_pair)
        .expect("Failed to sign CA certificate");

    let ca = RcgenAuthority::new(key_pair, ca_cert, 1_000);

    let (tx, req_rx) = tokio::sync::mpsc::channel(10000);

    let proxy = Proxy::builder()
        .with_addr(SocketAddr::from(([127, 0, 0, 1], 8080)))
        .with_rustls_client()
        .with_ca(ca)
        .with_http_handler(Interceptor { tx: Arc::new(tx) })
        .build();

    spawn_interceptor_task(wd_rx, req_rx);

    tokio::spawn(async move {
        if let Err(e) = proxy.start().await {
            tracing::error!("Proxy error: {}", e);
        }
    });
}

fn spawn_interceptor_task(
    mut wd_rx: Receiver<Signal>,
    mut proxy_rx: Receiver<String>,
) {
    tokio::spawn(async move {
        let mut is_listening = false;
        let mut collected = Vec::with_capacity(10);
        loop {
            tokio::select! {
                signal = wd_rx.recv() => {
                    match signal {
                        Some(Signal::StartListening) => is_listening = true,
                        Some(Signal::StopListening(tx)) => {
                            is_listening = false;
                            if let Err(vec) = tx.send(collected.clone()) {
                                tracing::error!("Failed to send {:?} to main app", vec);
                            }
                            collected.clear();
                        }
                        None => break,
                    }
                }
                req = proxy_rx.recv(), if is_listening => {
                    match req {
                        Some(url) => {
                            collected.push(url);
                        },
                        None => {
                            tracing::error!("Error, channel with proxy is closed");
                        },
                    }
                }
            }
        }
    });
}
