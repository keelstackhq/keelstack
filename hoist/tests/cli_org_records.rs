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
fn records_that_differ_in_proxied_are_no_twins() {
    // One live record, two declared that differ only in `proxied`: the first is present, the
    // second is no repeat of it, so it is created (it never patches the claimed live record).
    let api = api(LIVE_A);
    let d = dir(&toml(&[
        ("A", "www.example.dev", "1.1.1.1", false),
        ("A", "www.example.dev", "1.1.1.1", true),
    ]));
    let r = run(&["plan"], &d, &env(&api), 0);
    let both = records(&r, "A www.example.dev 1.1.1.1");
    assert_eq!(both.len(), 2, "{r:#}");
    let actions: Vec<&str> = both.iter().map(|i| i["action"].as_str().unwrap()).collect();
    assert!(
        actions.contains(&"ok") && actions.contains(&"create"),
        "{actions:?}"
    );
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
