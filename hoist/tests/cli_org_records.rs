//! `hoist org` Cloudflare records, the #68 review follow-ups (spec §16.3, §16.4): identical
//! declared records (twins) are one and their repeat is reported `ok`; a twin keys on type, name
//! (any case), normalised content and `proxied`; an unreadable zone or record list reports each
//! record once; a multi-chunk TXT value (`"a" "b"`) matches its joined value; a declined prompt
//! writes nothing and exits 1.

mod common;

use common::{FakeApi, RouteSpec, hoist};
use serde_json::Value;
use tempfile::TempDir;

fn dir(toml: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("org.toml"), toml).unwrap();
    d
}

fn start(routes: &[(&str, u16, &str)]) -> FakeApi {
    let specs: Vec<RouteSpec> = routes
        .iter()
        .map(|(k, s, b)| (*k, *s, *b, &[][..]))
        .collect();
    FakeApi::start(&specs)
}

fn env(api: &FakeApi) -> Vec<(&'static str, String)> {
    vec![
        ("HOIST_CLOUDFLARE_API", api.url()),
        ("CLOUDFLARE_API_TOKEN", "cf-test-token".into()),
    ]
}

fn run(args: &[&str], d: &TempDir, envs: &[(&str, String)], code: i32) -> Value {
    let mut c = hoist();
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c
        .current_dir(d.path())
        .args(["--json", "org"])
        .args(args)
        .assert()
        .code(code)
        .get_output()
        .clone();
    serde_json::from_slice(&out.stdout).unwrap()
}

fn records(r: &Value, key: &str) -> Vec<Value> {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "dns_record" && i["key"] == key)
        .cloned()
        .collect()
}

fn toml(records: &[(&str, &str, &str, bool)]) -> String {
    let mut t = String::from(
        "[cloudflare]\naccount_id = \"acct\"\n[[cloudflare.zones]]\nname = \"example.dev\"\n",
    );
    for (ty, name, content, proxied) in records {
        t.push_str(&format!(
            "[[cloudflare.zones.records]]\ntype = \"{ty}\"\nname = \"{name}\"\ncontent = {content:?}\nproxied = {proxied}\n"
        ));
    }
    t
}

/// Zone z1 with `live` as its record list (a JSON array body), and writes that succeed.
fn api(live: &str) -> FakeApi {
    let list =
        format!(r#"{{"success":true,"result":{live},"result_info":{{"page":1,"total_pages":1}}}}"#);
    start(&[
        (
            "GET /zones?name=example.dev",
            200,
            r#"{"success":true,"result":[{"id":"z1","name":"example.dev"}]}"#,
        ),
        ("GET /zones/z1/dns_records", 200, &list),
        (
            "POST /zones/z1/dns_records",
            200,
            r#"{"success":true,"result":{"id":"n"}}"#,
        ),
        (
            "PATCH /zones/z1/dns_records/r0",
            200,
            r#"{"success":true,"result":{"id":"r0"}}"#,
        ),
    ])
}

const LIVE_A: &str =
    r#"[{"id":"r0","type":"A","name":"www.example.dev","content":"1.1.1.1","proxied":false}]"#;

#[test]
fn an_identical_repeat_is_reported_ok_and_claims_nothing() {
    let api = api(LIVE_A);
    let d = dir(&toml(&[
        ("A", "www.example.dev", "1.1.1.1", false),
        ("A", "WWW.example.dev", "1.1.1.1", false),
    ]));
    let r = run(&["check"], &d, &env(&api), 0);
    let first = records(&r, "A www.example.dev 1.1.1.1");
    let repeat = records(&r, "A WWW.example.dev 1.1.1.1");
    assert_eq!(first.len(), 1, "{r:#}");
    assert_eq!(first[0]["action"], "ok");
    assert_eq!(repeat.len(), 1, "the repeat is reported: {r:#}");
    assert_eq!(repeat[0]["action"], "ok");
    assert!(
        repeat[0]["reason"].as_str().unwrap().contains("repeat"),
        "{r:#}"
    );
    api.assert_only_get();
}

#[test]
fn records_that_differ_only_in_proxied_are_a_config_error() {
    // Cloudflare holds one record per type, name and content, so two declared records that
    // differ only in `proxied` cannot both exist: a create would repeat the live record and be
    // rejected. It is a config error (exit 3, naming the record) before any read.
    let api = api(LIVE_A);
    let d = dir(&toml(&[
        ("A", "www.example.dev", "1.1.1.1", false),
        ("A", "WWW.example.dev", "1.1.1.1", true),
    ]));
    let out = hoist()
        .current_dir(d.path())
        .envs(env(&api))
        .args(["--json", "org", "plan"])
        .assert()
        .code(3)
        .get_output()
        .clone();
    let err = String::from_utf8_lossy(&out.stdout);
    assert!(
        err.contains("www.example.dev") && err.contains("proxied"),
        "{err}"
    );
    assert!(api.requests().is_empty(), "no read, no write");
}

#[test]
fn an_unreadable_record_list_reports_a_twin_once() {
    let api = start(&[
        (
            "GET /zones?name=example.dev",
            200,
            r#"{"success":true,"result":[{"id":"z1","name":"example.dev"}]}"#,
        ),
        (
            "GET /zones/z1/dns_records",
            403,
            r#"{"success":false,"errors":[{"code":9109,"message":"no"}]}"#,
        ),
    ]);
    let twins = toml(&[
        ("A", "www.example.dev", "1.1.1.1", false),
        ("A", "www.example.dev", "1.1.1.1", false),
    ]);
    let d = dir(&twins);
    let r = run(&["check"], &d, &env(&api), 0);
    let items = records(&r, "A www.example.dev 1.1.1.1");
    assert_eq!(items.len(), 1, "{r:#}");
    assert_eq!(items[0]["action"], "unknown");
    // No token at all: one unknown item too.
    let r = run(&["check"], &d, &[("HOIST_CLOUDFLARE_API", api.url())], 0);
    assert_eq!(records(&r, "A www.example.dev 1.1.1.1").len(), 1, "{r:#}");
}

#[test]
fn a_multi_chunk_txt_matches_its_joined_value() {
    let live = r#"[{"id":"r0","type":"TXT","name":"example.dev","content":"\"v=DKIM1; k=rsa; p=AAAA\" \"BBBB\"","proxied":false}]"#;
    let api = api(live);
    let d = dir(&toml(&[(
        "TXT",
        "example.dev",
        "v=DKIM1; k=rsa; p=AAAABBBB",
        false,
    )]));
    let r = run(&["check"], &d, &env(&api), 0);
    let it = records(&r, "TXT example.dev v=DKIM1; k=rsa; p=AAAABBBB");
    assert_eq!(it.len(), 1, "{r:#}");
    assert_eq!(it[0]["action"], "ok", "{r:#}");
    api.assert_only_get();
}

#[test]
fn a_declined_prompt_writes_nothing_and_exits_1() {
    let api = api("[]");
    let d = dir(&toml(&[("A", "www.example.dev", "1.1.1.1", false)]));
    let mut c = hoist();
    for (k, v) in env(&api) {
        c.env(k, v);
    }
    let out = c
        .current_dir(d.path())
        .env("HOIST_ASSUME_TTY", "1")
        .args(["--json", "org", "apply"])
        .write_stdin("n\n")
        .assert()
        .code(1)
        .get_output()
        .clone();
    let r: Value = serde_json::from_slice(&out.stdout).unwrap();
    let it = records(&r, "A www.example.dev 1.1.1.1");
    assert_eq!(it[0]["action"], "create", "{r:#}");
    assert_eq!(it[0]["reason"], "not applied: declined", "{r:#}");
    api.assert_only_get();
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("hoist org apply will make 1 change(s):")
    );
}

fn zone_route() -> (&'static str, u16, &'static str) {
    (
        "GET /zones?name=example.dev",
        200,
        r#"{"success":true,"result":[{"id":"z1","name":"example.dev"}]}"#,
    )
}

#[test]
fn a_record_list_without_result_info_is_read_past_page_1() {
    let p1 = r#"{"success":true,"result":[{"id":"r0","type":"A","name":"www.example.dev","content":"1.1.1.1","proxied":false}]}"#;
    let p2 = r#"{"success":true,"result":[{"id":"r1","type":"A","name":"www.example.dev","content":"2.2.2.2","proxied":false}]}"#;
    let p3 = r#"{"success":true,"result":[]}"#;
    let api = start(&[
        zone_route(),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=1",
            200,
            p1,
        ),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=2",
            200,
            p2,
        ),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=3",
            200,
            p3,
        ),
    ]);
    let d = dir(&toml(&[("A", "www.example.dev", "2.2.2.2", false)]));
    let r = run(&["check"], &d, &env(&api), 0);
    assert_eq!(
        records(&r, "A www.example.dev 2.2.2.2")[0]["action"],
        "ok",
        "{r:#}"
    );
    // An endpoint that ignores `page` answers page 1 again: read once, no endless loop.
    let api = start(&[zone_route(), ("GET /zones/z1/dns_records", 200, p1)]);
    let d = dir(&toml(&[("A", "www.example.dev", "1.1.1.1", false)]));
    let r = run(&["check"], &d, &env(&api), 0);
    assert_eq!(records(&r, "A www.example.dev 1.1.1.1")[0]["action"], "ok");
    let reads = (api.requests().iter())
        .filter(|q| q.path == "/zones/z1/dns_records")
        .count();
    assert_eq!(reads, 2, "page 1 and its repeat");
}

#[test]
fn without_result_info_only_a_404_short_empty_or_repeated_page_ends_the_list() {
    let p1 = r#"{"success":true,"result":[{"id":"r0","type":"A","name":"www.example.dev","content":"1.1.1.1","proxied":false},{"id":"r1","type":"A","name":"www.example.dev","content":"2.2.2.2","proxied":false}]}"#;
    let short = r#"{"success":true,"result":[{"id":"r2","type":"A","name":"www.example.dev","content":"3.3.3.3","proxied":false}]}"#;
    let d = dir(&toml(&[("A", "www.example.dev", "1.1.1.1", false)]));
    // Page 2 is 404: the list has ended, page 1's records count.
    let api = start(&[
        zone_route(),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=1",
            200,
            p1,
        ),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=2",
            404,
            r#"{"success":false}"#,
        ),
    ]);
    let r = run(&["check"], &d, &env(&api), 0);
    assert_eq!(
        records(&r, "A www.example.dev 1.1.1.1")[0]["action"],
        "ok",
        "{r:#}"
    );
    // Page 2 is refused (403): the list is unknown, never a silent partial one.
    let api = start(&[
        zone_route(),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=1",
            200,
            p1,
        ),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=2",
            403,
            r#"{"success":false,"errors":[{"code":1,"message":"no"}]}"#,
        ),
    ]);
    let r = run(&["check"], &d, &env(&api), 0);
    assert_eq!(
        records(&r, "A www.example.dev 1.1.1.1")[0]["action"],
        "unknown",
        "{r:#}"
    );
    // Page 2 is a 5xx or not a list: an error (exit 3), as on page 1.
    for (status, body) in [
        (500, r#"{"success":false}"#),
        (200, r#"{"success":true,"result":7}"#),
    ] {
        let api = start(&[
            zone_route(),
            (
                "GET /zones/z1/dns_records?name=www.example.dev&page=1",
                200,
                p1,
            ),
            (
                "GET /zones/z1/dns_records?name=www.example.dev&page=2",
                status,
                body,
            ),
        ]);
        hoist()
            .current_dir(d.path())
            .envs(env(&api))
            .args(["--json", "org", "check"])
            .assert()
            .code(3);
    }
    // A page shorter than the one before is the last: no page 3 is asked for.
    let api = start(&[
        zone_route(),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=1",
            200,
            p1,
        ),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=2",
            200,
            short,
        ),
    ]);
    let d3 = dir(&toml(&[("A", "www.example.dev", "3.3.3.3", false)]));
    let r = run(&["check"], &d3, &env(&api), 0);
    assert_eq!(
        records(&r, "A www.example.dev 3.3.3.3")[0]["action"],
        "ok",
        "{r:#}"
    );
    let pages: Vec<String> = (api.requests().iter())
        .filter(|q| q.path == "/zones/z1/dns_records")
        .map(|q| q.url.clone())
        .collect();
    assert_eq!(pages.len(), 2, "{pages:?}");
}

#[test]
fn names_that_differ_only_in_case_read_the_records_once() {
    let api = api(LIVE_A);
    let d = dir(&toml(&[
        ("A", "www.example.dev", "1.1.1.1", false),
        ("A", "WWW.example.dev", "2.2.2.2", false),
    ]));
    let r = run(&["plan"], &d, &env(&api), 0);
    assert_eq!(
        records(&r, "A www.example.dev 1.1.1.1")[0]["action"],
        "ok",
        "{r:#}"
    );
    assert_eq!(
        records(&r, "A WWW.example.dev 2.2.2.2")[0]["action"],
        "create",
        "{r:#}"
    );
    let reads = (api.requests().iter())
        .filter(|q| q.path == "/zones/z1/dns_records")
        .count();
    assert_eq!(reads, 1, "one read per DNS name");
}

#[test]
fn result_info_without_total_pages_reads_on_like_no_result_info() {
    let p1 = r#"{"success":true,"result":[{"id":"r0","type":"A","name":"www.example.dev","content":"1.1.1.1","proxied":false}],"result_info":{"page":1}}"#;
    let p2 = r#"{"success":true,"result":[{"id":"r1","type":"A","name":"www.example.dev","content":"2.2.2.2","proxied":false}],"result_info":{"page":2}}"#;
    let p3 = r#"{"success":true,"result":[],"result_info":{"page":3}}"#;
    let api = start(&[
        zone_route(),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=1",
            200,
            p1,
        ),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=2",
            200,
            p2,
        ),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=3",
            200,
            p3,
        ),
    ]);
    let d = dir(&toml(&[("A", "www.example.dev", "2.2.2.2", false)]));
    let r = run(&["check"], &d, &env(&api), 0);
    assert_eq!(
        records(&r, "A www.example.dev 2.2.2.2")[0]["action"],
        "ok",
        "{r:#}"
    );
}

#[test]
fn records_are_queried_by_the_compared_name() {
    let api = api(LIVE_A);
    let d = dir(&toml(&[("A", "WWW.Example.dev", "1.1.1.1", false)]));
    run(&["check"], &d, &env(&api), 0);
    let urls: Vec<String> = (api.requests().iter())
        .filter(|q| q.path == "/zones/z1/dns_records")
        .map(|q| q.url.clone())
        .collect();
    assert!(
        urls.iter().all(|u| u.contains("name=www.example.dev&")),
        "{urls:?}"
    );
}

#[test]
fn a_short_page_without_total_pages_ends_the_list_with_no_further_request() {
    // `per_page` 25 and one record: the list is complete, nothing guesses at a page 2.
    let p1 = r#"{"success":true,"result":[{"id":"r0","type":"A","name":"www.example.dev","content":"1.1.1.1","proxied":false}],"result_info":{"page":1,"per_page":25}}"#;
    let api = start(&[
        zone_route(),
        (
            "GET /zones/z1/dns_records?name=www.example.dev&page=1",
            200,
            p1,
        ),
    ]);
    let d = dir(&toml(&[("A", "www.example.dev", "1.1.1.1", false)]));
    let r = run(&["check"], &d, &env(&api), 0);
    assert_eq!(
        records(&r, "A www.example.dev 1.1.1.1")[0]["action"],
        "ok",
        "{r:#}"
    );
    let reads = (api.requests().iter())
        .filter(|q| q.path == "/zones/z1/dns_records")
        .count();
    assert_eq!(reads, 1, "a short page is the last page");
}

#[test]
fn exactly_identical_declared_records_make_one_item() {
    let api = api(LIVE_A);
    let d = dir(&toml(&[
        ("A", "www.example.dev", "1.1.1.1", false),
        ("A", "www.example.dev", "1.1.1.1", false),
        ("A", "www.example.dev", "1.1.1.1", false),
    ]));
    let r = run(&["check"], &d, &env(&api), 0);
    let items = records(&r, "A www.example.dev 1.1.1.1");
    assert_eq!(items.len(), 1, "{r:#}");
    assert_eq!(items[0]["action"], "ok");
    // Not a create either, when the record is missing live.
    let empty = self::api("[]");
    let r = run(&["plan"], &d, &env(&empty), 0);
    let items = records(&r, "A www.example.dev 1.1.1.1");
    assert_eq!(items.len(), 1, "{r:#}");
    assert_eq!(items[0]["action"], "create");
}
