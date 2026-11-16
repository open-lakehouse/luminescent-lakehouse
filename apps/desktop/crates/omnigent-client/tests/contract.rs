//! The hand-written protos against the OpenAPI document of the pinned server
//! (`fixtures/openapi-v0.21.0.json`, refreshed by `just omnigent-openapi <tag>`): every
//! route exists with its method, and every field exists in the schema it mirrors. Catches
//! drift when the pin moves, instead of at runtime.
//!
//! The protos are read as text: they are hand-written in one regular style, which the
//! parse below holds them to.

use std::collections::BTreeMap;

use serde_json::Value;

const SPEC: &str = include_str!("../fixtures/openapi-v0.21.0.json");
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

/// Routes the server hides from its document (`include_in_schema=False`), checked by hand
/// against the v0.21.0 source.
const HIDDEN_ROUTES: &[(&str, &str, &str)] = &[
    (
        "post",
        "/v1/sessions/{session_id}/events",
        "omnigent/server/routes/sessions/routes_events.py",
    ),
    (
        "post",
        "/v1/sessions/{session_id}/elicitations/{elicitation_id}/resolve",
        "omnigent/server/routes/sessions/routes_elicitations.py",
    ),
];

/// Each proto message and the schema it mirrors.
const SCHEMAS: &[(&str, &str)] = &[
    ("Session", "SessionResponse"),
    ("SessionList", "SessionList"),
    ("SessionListItem", "SessionListItem"),
    ("ConversationItem", "ConversationItem"),
    ("UpdateSessionRequest", "UpdateSessionRequest"),
    ("AgentList", "AgentList"),
    ("Agent", "AgentObject"),
];

/// Messages with no schema in the document, and why.
const UNDOCUMENTED: &[(&str, &str)] = &[
    (
        "CreateSessionRequest",
        "POST /v1/sessions parses JSON or multipart by hand: SessionCreateRequest, schemas.py",
    ),
    (
        "SessionEventInput",
        "a hidden route's body: SessionEventInput, schemas.py",
    ),
    ("PostEventRequest", "a hidden route's body"),
    ("EventAck", "a hidden route's answer, a plain dict"),
    (
        "ResolveElicitationRequest",
        "a hidden route's body: ElicitationResult, schemas.py",
    ),
    (
        "HostList",
        "GET /v1/hosts answers an untyped dict: hosts.py",
    ),
    ("Host", "GET /v1/hosts answers an untyped dict: hosts.py"),
    ("GetSessionRequest", "query parameters, checked as such"),
    ("ListSessionsRequest", "query parameters, checked as such"),
    ("ListAgentsRequest", "query parameters, checked as such"),
    ("ListHostsRequest", "no parameters"),
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

fn spec() -> Value {
    serde_json::from_str(SPEC).expect("the OpenAPI fixture")
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
        let mapped = SCHEMAS.iter().any(|(m, _)| m == name);
        let explained = UNDOCUMENTED.iter().any(|(m, _)| m == name);
        assert!(
            mapped ^ explained,
            "{name}: map it to its schema in SCHEMAS, or explain it in UNDOCUMENTED"
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
        for field in fields {
            // A request's path fields aren't in its body schema.
            if !properties.contains_key(&field.name) && field.name != "session_id" {
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
fn routes_exist_with_their_parameters() {
    let spec = spec();
    let protos = parse();
    assert_eq!(protos.rpcs.len(), 8);
    let mut problems = Vec::new();
    for rpc in &protos.rpcs {
        if let Some((_, _, source)) = HIDDEN_ROUTES
            .iter()
            .find(|(m, p, _)| *m == rpc.method && *p == rpc.path)
        {
            assert!(
                spec["paths"][&rpc.path][&rpc.method].is_null(),
                "{} is documented now ({source}): drop it from HIDDEN_ROUTES",
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
