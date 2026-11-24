//! The crates.io Authorization header (spec §16.5): reads (`GET`) send the bare token, as cargo
//! does; every write (the trusted-publishing token revoke on the release path included) sends
//! `Bearer`. Other services always send `Bearer`.

mod common;

use common::{FakeApi, RouteSpec};
use hoist::auth::Secret;
use hoist::http::{Http, Method, Service};

const TOKEN: &str = "canary-crates-token-5e1d";

#[test]
fn crates_reads_send_the_bare_token_and_writes_bearer() {
    let routes: Vec<RouteSpec> = vec![
        ("GET /api/v1/crates/x/owners", 200, r#"{"users":[]}"#, &[]),
        ("DELETE /api/v1/trusted_publishing/tokens", 204, "", &[]),
    ];
    let api = FakeApi::start(&routes);
    let http = Http::with_base(Service::Crates, Secret::from(TOKEN), &api.url());
    http.get_json("/api/v1/crates/x/owners").unwrap();
    http.send_json(Method::Delete, "/api/v1/trusted_publishing/tokens", None)
        .unwrap();
    let reqs = api.requests();
    assert_eq!(reqs.len(), 2, "{reqs:?}");
    assert_eq!(reqs[0].method, "GET");
    assert_eq!(reqs[0].auth.as_deref(), Some(TOKEN));
    assert_eq!(reqs[1].method, "DELETE");
    assert_eq!(reqs[1].auth.as_deref(), Some(&*format!("Bearer {TOKEN}")));
}

#[test]
fn other_services_read_with_bearer_and_an_empty_token_sends_none() {
    let routes: Vec<RouteSpec> = vec![("GET /x", 200, r#"{}"#, &[])];
    let api = FakeApi::start(&routes);
    Http::with_base(Service::Npm, Secret::from(TOKEN), &api.url())
        .get_json("/x")
        .unwrap();
    Http::with_base(Service::Crates, Secret::from(""), &api.url())
        .get_json("/x")
        .unwrap();
    let reqs = api.requests();
    assert_eq!(reqs[0].auth.as_deref(), Some(&*format!("Bearer {TOKEN}")));
    assert_eq!(reqs[1].auth, None);
}
