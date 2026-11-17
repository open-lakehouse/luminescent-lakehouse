//! The hand-written protos against the pinned server, as `just omnigent-openapi <tag>`
//! fetched it: its OpenAPI document (`fixtures/openapi-v0.22.0.json`), and for what the
//! document leaves out, its source (`fixtures/source-v0.22.0.json`: the request models of
//! `schemas.py`, and the routes hidden from the document). Every route exists with its
//! method, and every field exists in the schema or model it mirrors. Catches drift when
//! the pin moves, instead of at runtime.
//!
//! `OMNIGENT_OPENAPI` and `OMNIGENT_SOURCE` (paths) check other fixtures instead:
//! `just omnigent-contract [tag]` runs this against a newer release, and the scheduled
//! `omnigent-drift` workflow against the newest.
//!
//! The protos are read as text: they are hand-written in one regular style, which the
//! parse below holds them to.

use std::collections::BTreeMap;

use serde_json::Value;

/// The Omnigent release the protos are pinned to: its fixtures are the default.
const PIN: &str = "v0.22.0";
const PROTOS: &[(&str, &str)] = &[
    (
        "common.proto",
        include_str!("../proto/omnigent/v1/common.proto"),
    ),
    (
        "sessions.proto",
        include_str!("../proto/omnigent/v1/sessions.proto"),
    ),
    (
        "agents.proto",
        include_str!("../proto/omnigent/v1/agents.proto"),
    ),
    (
        "hosts.proto",
        include_str!("../proto/omnigent/v1/hosts.proto"),
    ),
];

/// Each proto message and the schema it mirrors in the document.
const SCHEMAS: &[(&str, &str)] = &[
    ("Session", "SessionResponse"),
    ("SessionList", "SessionList"),
    ("SessionListItem", "SessionListItem"),
    ("ConversationItem", "ConversationItem"),
    ("UpdateSessionRequest", "UpdateSessionRequest"),
    ("AgentList", "AgentList"),
    ("Agent", "AgentObject"),
    ("GitOptions", "SessionGitOptions"),
    ("PullRequestList", "SessionPullRequestList"),
    ("PullRequest", "SessionPullRequest"),
    ("PullRequestSummary", "SessionPullRequestSummary"),
];

/// Each proto message the document has no schema for, and the model of `schemas.py` it
/// mirrors.
const SOURCE_MODELS: &[(&str, &str)] = &[
    // POST /v1/sessions parses JSON or multipart by hand, so the document has no body.
    ("CreateSessionRequest", "SessionCreateRequest"),
    // The hidden routes' bodies.
    ("SessionEventInput", "SessionEventInput"),
    ("PostEventRequest", "SessionEventInput"),
    ("ResolveElicitationRequest", "ElicitationResult"),
];

/// Messages with neither, and why.
const UNDOCUMENTED: &[(&str, &str)] = &[
    ("EventAck", "a hidden route's answer, a plain dict"),
    (
        "HostList",
        "GET /v1/hosts answers an untyped dict: hosts.py",
    ),
    ("Host", "GET /v1/hosts answers an untyped dict: hosts.py"),
    ("GetSessionRequest", "query parameters, checked as such"),
    ("ListSessionsRequest", "query parameters, checked as such"),
    ("ListAgentsRequest", "query parameters, checked as such"),
    ("ListHostsRequest", "no parameters"),
    (
        "ListPullRequestsRequest",
        "query parameters, checked as such",
    ),
];

#[derive(Debug)]
struct Field {
    /// The JSON name.
    name: String,
}

#[derive(Debug)]
struct Rpc {
    name: String,
    request: String,
    method: String,
    path: String,
    body: Option<String>,
}

#[derive(Default)]
struct Protos {
    messages: BTreeMap<String, Vec<Field>>,
    rpcs: Vec<Rpc>,
}

fn parse() -> Protos {
    let mut protos = Protos::default();
    for (file, text) in PROTOS {
        let mut message: Option<String> = None;
        let mut rpc: Option<Rpc> = None;
        for line in text.lines().map(|l| l.split("//").next().unwrap().trim()) {
            if let Some(rest) = line.strip_prefix("message ") {
                let name = rest.trim_end_matches(['{', ' ', '}']).to_owned();
                protos.messages.entry(name.clone()).or_default();
                message = (!rest.ends_with('}')).then_some(name);
            } else if let Some(rest) = line.strip_prefix("rpc ") {
                let (name, rest) = rest.split_once('(').unwrap();
                let (request, _) = rest.split_once(')').unwrap();
                rpc = Some(Rpc {
                    name: name.trim().into(),
                    request: request.trim().into(),
                    method: String::new(),
                    path: String::new(),
                    body: None,
                });
            } else if let Some(rpc) = rpc.as_mut() {
                for method in ["get", "post", "patch", "put", "delete"] {
                    if let Some(rest) = line
                        .split_once(&format!("{method}: \""))
                        .map(|(_, rest)| rest)
                    {
                        rpc.method = method.into();
                        rpc.path = rest.split('"').next().unwrap().into();
                    }
                }
                if let Some((_, rest)) = line.split_once("body: \"") {
                    rpc.body = Some(rest.split('"').next().unwrap().into());
                }
                if line.starts_with('}') && !line.starts_with("};") {
                    let done = std::mem::replace(rpc, rpc_placeholder());
                    assert!(
                        !done.path.is_empty(),
                        "{file}: {} has no http rule",
                        done.name
                    );
                    protos.rpcs.push(done);
                }
            } else if let Some(name) = &message {
                if line == "}" {
                    message = None;
                } else if let Some(field) = parse_field(line) {
                    protos.messages.get_mut(name).unwrap().push(field);
                }
            }
            if rpc.as_ref().is_some_and(|r| r.name.is_empty()) {
                rpc = None;
            }
        }
    }
    protos
}

fn rpc_placeholder() -> Rpc {
    Rpc {
        name: String::new(),
        request: String::new(),
        method: String::new(),
        path: String::new(),
        body: None,
    }
}

/// `[optional|repeated] <type> <name> = <n> [json_name = "x"];`
fn parse_field(line: &str) -> Option<Field> {
    let (declaration, _) = line.split_once('=')?;
    let name = declaration.split_whitespace().last()?.to_owned();
    let name = match line.split_once("json_name = \"") {
        Some((_, rest)) => rest.split('"').next()?.to_owned(),
        None => name,
    };
    Some(Field { name })
}

/// The fixture `kind` (`openapi`, `source`): at the path in `var`, or the pin's.
fn fixture(var: &str, kind: &str) -> Value {
    let path = std::env::var_os(var).map(Into::into).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{kind}-{PIN}.json"))
    });
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the {kind} fixture {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn spec() -> Value {
    fixture("OMNIGENT_OPENAPI", "openapi")
}

fn source() -> Value {
    fixture("OMNIGENT_SOURCE", "source")
}

/// The fields of `message` that are path parameters of a route it is the request of.
fn path_params<'a>(protos: &'a Protos, message: &str) -> Vec<&'a str> {
    protos
        .rpcs
        .iter()
        .filter(|rpc| rpc.request == message)
        .flat_map(|rpc| path_fields(&rpc.path))
        .collect()
}

fn schema_properties<'a>(spec: &'a Value, name: &str) -> &'a serde_json::Map<String, Value> {
    spec["components"]["schemas"][name]["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("no schema {name} in the document"))
}

fn path_fields(path: &str) -> Vec<&str> {
    path.split('{')
        .skip(1)
        .filter_map(|s| s.split('}').next())
        .collect()
}

#[test]
fn every_message_is_mapped_or_explained() {
    let protos = parse();
    for name in protos.messages.keys() {
        let mapped = [SCHEMAS, SOURCE_MODELS]
            .iter()
            .filter(|table| table.iter().any(|(m, _)| m == name))
            .count();
        let explained = UNDOCUMENTED.iter().any(|(m, _)| m == name);
        assert!(
            mapped + usize::from(explained) == 1,
            "{name}: map it to its schema in SCHEMAS or its model in SOURCE_MODELS, or \
             explain it in UNDOCUMENTED (exactly one)"
        );
    }
}

#[test]
fn fields_exist_in_their_schemas() {
    let spec = spec();
    let protos = parse();
    let mut missing = Vec::new();
    for (message, schema) in SCHEMAS {
        let properties = schema_properties(&spec, schema);
        let fields = &protos.messages[*message];
        assert!(!fields.is_empty(), "{message} has no fields");
        let path = path_params(&protos, message);
        for field in fields {
            // A request's path fields aren't in its body schema.
            if !properties.contains_key(&field.name) && !path.contains(&field.name.as_str()) {
                missing.push(format!("{message}.{} (schema {schema})", field.name));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "fields not in the document: {missing:#?}"
    );
}

#[test]
fn fields_exist_in_their_source_models() {
    let source = source();
    let protos = parse();
    let mut missing = Vec::new();
    for (message, model) in SOURCE_MODELS {
        let fields: Vec<&str> = source["models"][model]
            .as_array()
            .unwrap_or_else(|| panic!("no model {model} in schemas.py"))
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        let path = path_params(&protos, message);
        for field in &protos.messages[*message] {
            if !fields.contains(&field.name.as_str()) && !path.contains(&field.name.as_str()) {
                missing.push(format!("{message}.{} (model {model})", field.name));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "fields not in the source's models: {missing:#?}"
    );
}

#[test]
fn routes_exist_with_their_parameters() {
    let spec = spec();
    let source = source();
    let protos = parse();
    assert_eq!(protos.rpcs.len(), 9);
    // The routes the server hides from its document (`include_in_schema=False`), from its
    // source.
    let hidden: Vec<(&str, &str)> = source["hidden_routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r[0].as_str().unwrap(), r[1].as_str().unwrap()))
        .collect();
    let mut problems = Vec::new();
    for rpc in &protos.rpcs {
        if hidden.contains(&(rpc.method.as_str(), rpc.path.as_str())) {
            assert!(
                spec["paths"][&rpc.path][&rpc.method].is_null(),
                "{} is hidden and documented at once",
                rpc.path
            );
            continue;
        }
        let operation = &spec["paths"][&rpc.path][&rpc.method];
        if operation.is_null() {
            problems.push(format!("{}: no {} {}", rpc.name, rpc.method, rpc.path));
            continue;
        }
        let parameters: Vec<(&str, &str)> = operation["parameters"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .map(|p| (p["in"].as_str().unwrap(), p["name"].as_str().unwrap()))
                    .collect()
            })
            .unwrap_or_default();
        let path = path_fields(&rpc.path);
        for name in &path {
            if !parameters.contains(&("path", name)) {
                problems.push(format!("{}: no path parameter {name}", rpc.name));
            }
        }
        if rpc.body.is_none() {
            // The other fields are query parameters.
            for field in &protos.messages[&rpc.request] {
                if !path.contains(&field.name.as_str())
                    && !parameters.contains(&("query", field.name.as_str()))
                {
                    problems.push(format!("{}: no query parameter {}", rpc.name, field.name));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
