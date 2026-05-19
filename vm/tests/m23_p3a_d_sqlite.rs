//! M23 P3a-D in-process regression tests for the `sqlite3` stdlib
//! module.  Each test compiles a tiny `.spy` snippet, writes the
//! bytecode to `CARGO_TARGET_TMPDIR`, runs through `run_file_capture`,
//! and asserts on stdout.  All tests use `:memory:` databases so they
//! leave no filesystem residue and can run in parallel.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m23_p3a_d_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn sqlite_connect_memory_and_close() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    if c > 0i64:
        println(\"connected\")
    sqlite3.close(c)
    println(\"closed\")
    return 0
";
    let p = compile_snippet("connect_close", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("connected"), "got: {out:?}");
    assert!(out.contains("closed"), "got: {out:?}");
}

#[test]
fn sqlite_create_insert_select_roundtrip() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(c, \"CREATE TABLE t (id INTEGER PRIMARY KEY, x TEXT)\")
    p: List[str] = []
    p.append(\"alpha\")
    sqlite3.execute_params(c, \"INSERT INTO t (x) VALUES (?)\", p)
    q: List[str] = []
    q.append(\"beta\")
    sqlite3.execute_params(c, \"INSERT INTO t (x) VALUES (?)\", q)
    rows: List[List[str]] = sqlite3.query(c, \"SELECT id, x FROM t ORDER BY id\")
    n: i32 = i32(i64(len(rows)))
    println(\"n=\" + str(n))
    if n >= 2i32:
        r0: List[str] = rows[0i32]
        r1: List[str] = rows[1i32]
        println(\"r0=\" + r0[0i32] + \",\" + r0[1i32])
        println(\"r1=\" + r1[0i32] + \",\" + r1[1i32])
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("roundtrip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("n=2"), "got: {out:?}");
    assert!(out.contains("r0=1,alpha"), "got: {out:?}");
    assert!(out.contains("r1=2,beta"), "got: {out:?}");
}

#[test]
fn sqlite_last_insert_rowid_and_changes() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(c, \"CREATE TABLE t (id INTEGER PRIMARY KEY, x TEXT)\")
    p: List[str] = []
    p.append(\"row\")
    sqlite3.execute_params(c, \"INSERT INTO t (x) VALUES (?)\", p)
    sqlite3.execute_params(c, \"INSERT INTO t (x) VALUES (?)\", p)
    sqlite3.execute_params(c, \"INSERT INTO t (x) VALUES (?)\", p)
    rid: i64 = sqlite3.last_insert_rowid(c)
    println(\"rowid=\" + str(rid))
    sqlite3.execute(c, \"UPDATE t SET x = 'y' WHERE id <= 2\")
    n: i32 = sqlite3.changes(c)
    println(\"changes=\" + str(n))
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("rowid_changes", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("rowid=3"), "got: {out:?}");
    assert!(out.contains("changes=2"), "got: {out:?}");
}

#[test]
fn sqlite_column_names() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(c, \"CREATE TABLE q (id INTEGER PRIMARY KEY, name TEXT, body TEXT)\")
    cols: List[str] = sqlite3.column_names(c, \"SELECT id, name, body FROM q\")
    n: i32 = i32(i64(len(cols)))
    println(\"ncols=\" + str(n))
    if n >= 3i32:
        println(\"c0=\" + cols[0i32])
        println(\"c1=\" + cols[1i32])
        println(\"c2=\" + cols[2i32])
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("column_names", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("ncols=3"), "got: {out:?}");
    assert!(out.contains("c0=id"), "got: {out:?}");
    assert!(out.contains("c1=name"), "got: {out:?}");
    assert!(out.contains("c2=body"), "got: {out:?}");
}

#[test]
fn sqlite_bad_sql_raises_value_error() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    try:
        rows: List[List[str]] = sqlite3.query(c, \"SELECT FROM bogus\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught\")
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("bad_sql", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn sqlite_param_binding_prevents_injection() {
    // Bind an apostrophe-containing string and a SQL-injection-shaped
    // body.  Both round-trip verbatim because parameter binding treats
    // them as TEXT, not parsed SQL.
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(c, \"CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT, body TEXT)\")
    p: List[str] = []
    p.append(\"O'Brien\")
    p.append(\"'; drop table notes;--\")
    sqlite3.execute_params(c, \"INSERT INTO notes (title, body) VALUES (?, ?)\", p)
    # If injection had succeeded, the table would be gone and this
    # SELECT would raise.  The hand-rolled escape would have produced
    # `INSERT INTO notes (title, body) VALUES ('O''Brien', '''; drop table notes;--')`
    # but the bound version stores them as raw text.
    rows: List[List[str]] = sqlite3.query(c, \"SELECT title, body FROM notes\")
    n: i32 = i32(i64(len(rows)))
    println(\"n=\" + str(n))
    if n >= 1i32:
        r: List[str] = rows[0i32]
        println(\"title=\" + r[0i32])
        println(\"body=\" + r[1i32])
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("injection_safe", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("n=1"), "got: {out:?}");
    assert!(out.contains("title=O'Brien"), "got: {out:?}");
    assert!(out.contains("body='; drop table notes;--"), "got: {out:?}");
}

#[test]
fn sqlite_query_params_filters_rows() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(c, \"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)\")
    names: List[str] = []
    names.append(\"a\")
    names.append(\"b\")
    names.append(\"c\")
    i: i32 = 0i32
    while i < 3i32:
        p: List[str] = []
        p.append(names[i])
        sqlite3.execute_params(c, \"INSERT INTO t (name) VALUES (?)\", p)
        i = i + 1i32
    q: List[str] = []
    q.append(\"b\")
    rows: List[List[str]] = sqlite3.query_params(c, \"SELECT id, name FROM t WHERE name = ?\", q)
    n: i32 = i32(i64(len(rows)))
    println(\"n=\" + str(n))
    if n >= 1i32:
        r: List[str] = rows[0i32]
        println(\"match=\" + r[0i32] + \":\" + r[1i32])
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("query_params", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("n=1"), "got: {out:?}");
    assert!(out.contains("match=2:b"), "got: {out:?}");
}

#[test]
fn sqlite_null_cell_renders_as_empty() {
    // SELECT NULL should come back as the empty string, per the v0.2
    // stringification rule documented in spec §9.24.
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    rows: List[List[str]] = sqlite3.query(c, \"SELECT NULL, 42, 3.5, 'text'\")
    n: i32 = i32(i64(len(rows)))
    println(\"n=\" + str(n))
    if n >= 1i32:
        r: List[str] = rows[0i32]
        println(\"null=[\" + r[0i32] + \"]\")
        println(\"int=[\" + r[1i32] + \"]\")
        println(\"real=[\" + r[2i32] + \"]\")
        println(\"text=[\" + r[3i32] + \"]\")
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("null_cell", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("null=[]"), "got: {out:?}");
    assert!(out.contains("int=[42]"), "got: {out:?}");
    assert!(out.contains("real=[3.5]"), "got: {out:?}");
    assert!(out.contains("text=[text]"), "got: {out:?}");
}

#[test]
fn sqlite_invalid_handle_raises() {
    let src = "\
import sqlite3
fn main() -> i32:
    try:
        sqlite3.execute(0i64, \"SELECT 1\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("invalid_handle", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn sqlite_two_connections_independent() {
    // Two `:memory:` connections must not see each other's data —
    // they're separate processes from SQLite's point of view.
    let src = "\
import sqlite3
fn main() -> i32:
    a: i64 = sqlite3.connect(\":memory:\")
    b: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(a, \"CREATE TABLE x (v TEXT)\")
    p: List[str] = []
    p.append(\"only-in-a\")
    sqlite3.execute_params(a, \"INSERT INTO x (v) VALUES (?)\", p)
    ra: List[List[str]] = sqlite3.query(a, \"SELECT v FROM x\")
    println(\"a_rows=\" + str(i32(i64(len(ra)))))
    # Connection b should not see table x.
    try:
        rb: List[List[str]] = sqlite3.query(b, \"SELECT v FROM x\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"b_isolated\")
    sqlite3.close(a)
    sqlite3.close(b)
    return 0
";
    let p = compile_snippet("two_conns", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("a_rows=1"), "got: {out:?}");
    assert!(out.contains("b_isolated"), "got: {out:?}");
}

#[test]
fn sqlite_close_idempotent() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.close(c)
    sqlite3.close(c)
    sqlite3.close(c)
    println(\"ok\")
    return 0
";
    let p = compile_snippet("close_idempotent", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("ok"), "got: {out:?}");
}

#[test]
fn sqlite_empty_query_returns_empty_list() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(c, \"CREATE TABLE t (v TEXT)\")
    rows: List[List[str]] = sqlite3.query(c, \"SELECT v FROM t\")
    println(\"n=\" + str(i32(i64(len(rows)))))
    sqlite3.close(c)
    return 0
";
    let p = compile_snippet("empty_query", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("n=0"), "got: {out:?}");
}

#[test]
fn sqlite_use_after_close_raises() {
    let src = "\
import sqlite3
fn main() -> i32:
    c: i64 = sqlite3.connect(\":memory:\")
    sqlite3.close(c)
    try:
        sqlite3.execute(c, \"SELECT 1\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("use_after_close", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("caught"), "got: {out:?}");
}
