//! M34 integration tests for the typed `JsonValue` tree (sealed
//! hierarchy: JsonValue → JNull / JBool / JInt / JFloat / JString /
//! JList / JObject).
//!
//! Each test compiles a small `.spy` snippet, runs it through
//! `run_file_capture`, and asserts on stdout.  The tests are
//! intentionally end-to-end (compile + lower + execute) because M34
//! exercises the resolver, the IR, the codegen, and the VM runtime in
//! one go.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m34_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn parse_primitive_values_round_trip() {
    let src = r#"
import json

fn main() -> i32:
    null_v: JsonValue = json.parse("null")
    bool_v: JsonValue = json.parse("true")
    int_v: JsonValue = json.parse("42")
    float_v: JsonValue = json.parse("3.14")
    str_v: JsonValue = json.parse("\"hello\"")

    println(json.stringify(null_v))
    println(json.stringify(bool_v))
    println(json.stringify(int_v))
    println(json.stringify(float_v))
    println(json.stringify(str_v))
    return 0
"#;
    let p = compile_snippet("primitive_round_trip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("null"), "got: {out:?}");
    assert!(out.contains("true"), "got: {out:?}");
    assert!(out.contains("42"), "got: {out:?}");
    assert!(out.contains("3.14"), "got: {out:?}");
    assert!(out.contains("\"hello\""), "got: {out:?}");
}

#[test]
fn match_isinstance_on_primitive_variants() {
    let src = r#"
import json

fn describe(v: JsonValue) -> str:
    match v:
        case JNull():
            return "null"
        case JBool(b):
            return "bool"
        case JInt(n):
            return "int"
        case JString(s):
            return "string: " + s
    return "other"

fn main() -> i32:
    a: JsonValue = json.parse("null")
    b: JsonValue = json.parse("true")
    c: JsonValue = json.parse("99")
    d: JsonValue = json.parse("\"world\"")
    println(describe(a))
    println(describe(b))
    println(describe(c))
    println(describe(d))
    return 0
"#;
    let p = compile_snippet("match_primitives", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(out.contains("null"), "got: {out:?}");
    assert!(out.contains("bool"), "got: {out:?}");
    assert!(out.contains("int"), "got: {out:?}");
    assert!(out.contains("string: world"), "got: {out:?}");
}

#[test]
fn jlist_length_and_get_via_isinstance_narrowing() {
    // `isinstance(v, JList)` narrows v to JList in the then-branch
    // (per M16's flow-narrowing rule), so `v.length()` / `v.get()`
    // dispatch directly to the JList methods.
    let src = r#"
import json

fn main() -> i32:
    v: JsonValue = json.parse("[1, 2, 3]")
    if isinstance(v, JList):
        println("len = " + str(v.length()))
        first: JsonValue = v.get(0i64)
        match first:
            case JInt(n):
                println("first = " + str(n))
    return 0
"#;
    let p = compile_snippet("jlist_methods", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(out.contains("len = 3"), "got: {out:?}");
    assert!(out.contains("first = 1"), "got: {out:?}");
}

#[test]
fn jlist_destructure_yields_inner_list() {
    // The other ergonomic shape: pattern-bind the inner List[JsonValue]
    // directly and use the standard list ops.
    let src = r#"
import json

fn main() -> i32:
    v: JsonValue = json.parse("[10, 20, 30]")
    match v:
        case JList(items):
            println("len = " + str(len(items)))
            first: JsonValue = items[0]
            match first:
                case JInt(n):
                    println("first = " + str(n))
    return 0
"#;
    let p = compile_snippet("jlist_destructure", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(out.contains("len = 3"), "got: {out:?}");
    assert!(out.contains("first = 10"), "got: {out:?}");
}

#[test]
fn jobject_get_and_has() {
    // Use `isinstance` narrowing so `v: JsonValue` becomes `v: JObject`
    // in the then-branch and we can call .get / .has / .keys / .length.
    let src = r#"
import json

fn main() -> i32:
    v: JsonValue = json.parse("{\"name\": \"alice\", \"age\": 30}")
    if isinstance(v, JObject):
        if v.has("name"):
            println("has name")
        name_v: JsonValue? = v.get("name")
        if name_v is not none:
            match name_v:
                case JString(s):
                    println("name = " + s)
        missing: JsonValue? = v.get("missing")
        if missing is none:
            println("missing is absent")
        println("len = " + str(v.length()))
    return 0
"#;
    let p = compile_snippet("jobject_methods", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(out.contains("has name"), "got: {out:?}");
    assert!(out.contains("name = alice"), "got: {out:?}");
    assert!(out.contains("missing is absent"), "got: {out:?}");
    assert!(out.contains("len = 2"), "got: {out:?}");
}

#[test]
fn nested_object_in_list_in_object_round_trips() {
    let src = r#"
import json

fn main() -> i32:
    s: str = "{\"items\": [{\"name\": \"a\"}, {\"name\": \"b\"}]}"
    v: JsonValue = json.parse(s)
    out: str = json.stringify(v)
    println(out)
    return 0
"#;
    let p = compile_snippet("nested_round_trip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    // Insertion-ordered keys; stringify follows that order.
    assert!(
        out.contains(r#"{"items":[{"name":"a"},{"name":"b"}]}"#),
        "got: {out:?}"
    );
}

#[test]
fn programmatic_construction_via_helpers() {
    let src = r#"
import json

fn main() -> i32:
    s: JsonValue = json.j_string("hi")
    n: JsonValue = json.j_int(42i64)
    b: JsonValue = json.j_bool(true)
    println(json.stringify(s))
    println(json.stringify(n))
    println(json.stringify(b))
    lst: List[JsonValue] = [s, n, b]
    arr: JsonValue = json.j_list(lst)
    println(json.stringify(arr))
    return 0
"#;
    let p = compile_snippet("construct_helpers", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(out.contains("\"hi\""), "got: {out:?}");
    assert!(out.contains("42"), "got: {out:?}");
    assert!(out.contains("true"), "got: {out:?}");
    assert!(out.contains("[\"hi\",42,true]"), "got: {out:?}");
}

#[test]
fn programmatic_object_construction() {
    let src = r#"
import json

fn main() -> i32:
    pairs: List[Tuple[str, JsonValue]] = [
        ("k1", json.j_int(1i64)),
        ("k2", json.j_string("v")),
    ]
    obj: JsonValue = json.j_object(pairs)
    println(json.stringify(obj))
    return 0
"#;
    let p = compile_snippet("construct_object", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(
        out.contains(r#"{"k1":1,"k2":"v"}"#),
        "got: {out:?}"
    );
}

#[test]
fn malformed_json_raises_value_error() {
    let src = r#"
import json

fn main() -> i32:
    try:
        v: JsonValue = json.parse("{not json}")
        println("should not print")
    except ValueError as e:
        println("caught: " + e.type_name)
    return 0
"#;
    let p = compile_snippet("malformed_input", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(out.contains("caught: ValueError"), "got: {out:?}");
    assert!(!out.contains("should not print"), "got: {out:?}");
}

#[test]
fn isinstance_disambiguates_subclass() {
    let src = r#"
import json

fn main() -> i32:
    v: JsonValue = json.parse("123")
    if isinstance(v, JInt):
        println("yes int")
    if isinstance(v, JString):
        println("yes string")
    else:
        println("not string")
    return 0
"#;
    let p = compile_snippet("isinstance_check", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    assert!(out.contains("yes int"), "got: {out:?}");
    assert!(out.contains("not string"), "got: {out:?}");
    assert!(!out.contains("yes string"), "got: {out:?}");
}

#[test]
fn stringify_pretty_produces_indented_output() {
    let src = r#"
import json

fn main() -> i32:
    v: JsonValue = json.parse("{\"a\":1,\"b\":[2,3]}")
    println(json.stringify_pretty(v, 2i32))
    return 0
"#;
    let p = compile_snippet("stringify_pretty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout was: {out}");
    // Top-level object opens at column 0, keys indented by 2 spaces.
    assert!(out.contains("{\n  \"a\":"), "got: {out:?}");
    // The inner array is at indent 2; its elements at indent 4.
    assert!(out.contains("\"b\": [\n    2,\n    3\n  ]"), "got: {out:?}");
}
