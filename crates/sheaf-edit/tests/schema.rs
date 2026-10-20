//! The Op JSON schema only grows within a major version: what a reader of
//! an earlier minor read, this one reads, and nothing it wrote becomes
//! wrong. The schema of every version is committed, `schema/ops-<v>.json`;
//! this holds today's to every one of its major (nothing lost or retyped,
//! nothing newly required, so every field added has a default), and to
//! the one of [`VERSION`] exactly. `SHEAF_SCHEMA_UPDATE=1` writes that
//! one when the schema has only grown.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sheaf_edit::{Envelope, VERSION};

/// How `new` fails to read everything `old` reads, at `at`: a type,
/// property, variant or value it lost, one it retyped, a constraint it
/// added (a pattern, a bound, a closed object, ...), or a property it now
/// requires. Words (`description`) may change, and be added.
fn narrowed(at: &str, old: &Value, new: &Value, out: &mut Vec<String>) {
    let (Some(old), Some(new)) = (old.as_object(), new.as_object()) else {
        if old != new {
            out.push(format!("{at}: was {old}, is {new}"));
        }
        return;
    };
    let none = Value::Null;
    for (key, was) in old {
        let is = new.get(key).unwrap_or(&none);
        let at = format!("{at}/{key}");
        match key.as_str() {
            "description" | "title" | "$schema" | "$defs" | "required" => {}
            "type" => {
                let types = |v: &Value| match v {
                    Value::Array(all) => all.clone(),
                    one => vec![one.clone()],
                };
                if !types(was).iter().all(|t| types(is).contains(t)) {
                    out.push(format!("{at}: was {was}, is {is}"));
                }
            }
            "enum" => {
                let is = is.as_array().cloned().unwrap_or_default();
                for value in was.as_array().into_iter().flatten() {
                    if !is.contains(value) {
                        out.push(format!("{at}: lost {value}"));
                    }
                }
            }
            "properties" => {
                for (name, schema) in was.as_object().into_iter().flatten() {
                    match is.get(name) {
                        Some(now) => narrowed(&format!("{at}/{name}"), schema, now, out),
                        None => out.push(format!("{at}: lost {name}")),
                    }
                }
            }
            // Each variant read as one that reads all it did; when none
            // does, the nearest says why.
            "oneOf" | "anyOf" => {
                let now = is.as_array().cloned().unwrap_or_default();
                for (i, variant) in was.as_array().into_iter().flatten().enumerate() {
                    let why = |v: &Value| {
                        let mut why = Vec::new();
                        narrowed(&format!("{at}/{i}"), variant, v, &mut why);
                        why
                    };
                    let nearest = now.iter().map(why).min_by_key(Vec::len);
                    match nearest {
                        Some(why) if why.is_empty() => {}
                        Some(why) => out.push(format!(
                            "{at}/{i}: lost {variant}; the nearest now: {}",
                            why.join("; ")
                        )),
                        None => out.push(format!("{at}/{i}: lost {variant}")),
                    }
                }
            }
            "items" | "additionalProperties" | "not" => narrowed(&at, was, is, out),
            _ if was != is => out.push(format!("{at}: was {was}, is {is}")),
            _ => {}
        }
    }
    for (key, is) in new {
        let words = matches!(
            key.as_str(),
            "description" | "title" | "default" | "examples" | "$comment" | "$schema" | "$defs"
        );
        // `required` is below.
        if !old.contains_key(key) && !words && key != "required" {
            out.push(format!("{at}/{key}: newly {is}"));
        }
    }
    let required = |m: &Map<String, Value>| m.get("required").cloned().unwrap_or_default();
    let was = required(old);
    for name in required(new).as_array().into_iter().flatten() {
        if !was.as_array().is_some_and(|was| was.contains(name)) {
            out.push(format!("{at}: requires {name}, which has no default"));
        }
    }
}

/// [`narrowed`] for a whole schema: its root, and every type it defines.
fn schema_narrowed(old: &Value, new: &Value) -> Vec<String> {
    let mut out = Vec::new();
    narrowed("", old, new, &mut out);
    for (name, was) in old["$defs"].as_object().into_iter().flatten() {
        match new["$defs"].get(name) {
            Some(is) => narrowed(&format!("$defs/{name}"), was, is, &mut out),
            None => out.push(format!("$defs: lost {name}")),
        }
    }
    out
}

fn snapshots() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("schema")
}

#[test]
fn the_schema_only_grows() {
    let schema = serde_json::to_value(schemars::schema_for!(Envelope)).unwrap();
    let major = VERSION.split('.').next().unwrap();
    let mut problems = Vec::new();
    for entry in fs::read_dir(snapshots()).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.starts_with(&format!("ops-{major}.")) {
            continue;
        }
        let old: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let narrowed = schema_narrowed(&old, &schema);
        problems.extend(narrowed.into_iter().map(|p| format!("{name}: {p}")));
    }
    assert!(
        problems.is_empty(),
        "the Op JSON schema reads less than it did (a field added needs \
         #[serde(default)]; anything else is a new major version):\n{}",
        problems.join("\n")
    );
    let path = snapshots().join(format!("ops-{VERSION}.json"));
    let kept = fs::read_to_string(&path).ok();
    let kept: Option<Value> = kept.map(|s| serde_json::from_str(&s).unwrap());
    if kept.as_ref() == Some(&schema) {
        return;
    }
    if std::env::var_os("SHEAF_SCHEMA_UPDATE").is_some_and(|v| v == "1") {
        let text = serde_json::to_string_pretty(&schema).unwrap();
        fs::write(&path, text + "\n").unwrap();
        return;
    }
    match kept {
        Some(_) => panic!(
            "the Op JSON schema has grown since {}; SHEAF_SCHEMA_UPDATE=1 records it",
            path.display()
        ),
        None => panic!(
            "no Op JSON schema is recorded at {}; SHEAF_SCHEMA_UPDATE=1 records it",
            path.display()
        ),
    }
}

/// The check itself: what it lets through and what it doesn't.
#[test]
fn growing_passes_and_narrowing_fails() {
    let old = serde_json::json!({
        "$defs": {
            "Op": {"oneOf": [
                {"type": "object", "properties": {"op": {"const": "a"}, "n": {"type": "integer"}},
                 "required": ["op", "n"], "additionalProperties": false},
                {"type": "object", "properties": {"op": {"const": "b"}}, "required": ["op"]},
            ]},
            "Turn": {"enum": [90, 180]},
            "Text": {"type": "string"},
        },
        "type": "object",
        "properties": {"ops": {"type": "array", "items": {"$ref": "#/$defs/Op"}}},
        "required": ["ops"],
    });
    let grown = |edit: &dyn Fn(&mut Value)| {
        let mut new = old.clone();
        edit(&mut new);
        schema_narrowed(&old, &new)
    };
    assert!(grown(&|_| {}).is_empty());
    // An optional field, a variant, a value, a wider type, new words.
    assert!(
        grown(&|s| {
            s["$defs"]["Op"]["oneOf"][0]["properties"]["m"] = serde_json::json!({"type": "string"});
            s["$defs"]["Op"]["oneOf"][1]["properties"]["n"]["type"] = "integer".into();
            let c = serde_json::json!({"type": "object", "properties": {"op": {"const": "c"}}});
            s["$defs"]["Op"]["oneOf"].as_array_mut().unwrap().push(c);
            s["$defs"]["Turn"]["enum"]
                .as_array_mut()
                .unwrap()
                .push(270.into());
            s["$defs"]["Op"]["oneOf"][0]["properties"]["n"]["type"] =
                serde_json::json!(["integer", "null"]);
            s["$defs"]["Op"]["description"] = "An edit.".into();
            s["$defs"]["Text"]["title"] = "Text".into();
            s["$defs"]["Text"]["default"] = "".into();
            s["$defs"]["Text"]["examples"] = serde_json::json!(["a"]);
        })
        .is_empty()
    );
    for (what, edit) in [
        (
            "requires",
            &(|s: &mut Value| {
                s["$defs"]["Op"]["oneOf"][0]["properties"]["m"] =
                    serde_json::json!({"type": "string"});
                let required = s["$defs"]["Op"]["oneOf"][0]["required"].as_array_mut();
                required.unwrap().push("m".into());
            }) as &dyn Fn(&mut Value),
        ),
        ("lost", &|s| {
            let props = s["$defs"]["Op"]["oneOf"][0]["properties"].as_object_mut();
            props.unwrap().remove("n");
        }),
        ("was", &|s| {
            s["$defs"]["Op"]["oneOf"][0]["properties"]["n"]["type"] = "string".into();
        }),
        ("lost", &|s| {
            s["$defs"]["Op"]["oneOf"].as_array_mut().unwrap().remove(1);
        }),
        ("lost", &|s| {
            s["$defs"]["Turn"]["enum"].as_array_mut().unwrap().remove(0);
        }),
        ("lost", &|s| {
            s["$defs"].as_object_mut().unwrap().remove("Turn");
        }),
        ("was", &|s| {
            s["properties"]["ops"]["items"]["$ref"] = "#/$defs/Other".into();
        }),
        // A constraint where there was none.
        ("newly", &|s| {
            s["$defs"]["Op"]["oneOf"][1]["additionalProperties"] = false.into();
        }),
        ("newly", &|s| {
            s["$defs"]["Op"]["oneOf"][0]["properties"]["n"]["minimum"] = 0.into();
        }),
        ("newly", &|s| {
            s["$defs"]["Op"]["oneOf"][0]["properties"]["n"]["maximum"] = 9.into();
        }),
        ("newly", &|s| {
            s["$defs"]["Op"]["oneOf"][0]["properties"]["n"]["format"] = "uint8".into();
        }),
        ("newly", &|s| {
            s["$defs"]["Text"]["pattern"] = "^[a-z]+$".into();
        }),
        ("newly", &|s| {
            s["$defs"]["Text"]["enum"] = serde_json::json!(["a", "b"]);
        }),
        ("newly", &|s| {
            s["$defs"]["Text"]["maxLength"] = 9.into();
        }),
        ("newly", &|s| {
            s["$defs"]["Text"]["const"] = "a".into();
        }),
        ("newly", &|s| {
            s["$defs"]["Text"]["oneOf"] = serde_json::json!([{"type": "string"}]);
        }),
        ("newly", &|s| {
            s["properties"]["ops"]["minItems"] = 1.into();
        }),
        ("newly", &|s| {
            s["properties"]["ops"]["uniqueItems"] = true.into();
        }),
    ] {
        let problems = grown(edit);
        assert!(
            problems.iter().any(|p| p.contains(what)),
            "{what}: {problems:?}"
        );
    }
}
