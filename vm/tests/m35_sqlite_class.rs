//! M35 P4-B in-process regression tests for the typed
//! `sqlite3.Connection` + `sqlite3.Cursor` classes.  Same shape as
//! `m23_p3a_d_sqlite.rs` — compile a tiny `.spy` snippet, write
//! bytecode to `CARGO_TARGET_TMPDIR`, run through `run_file_capture`,
//! assert on stdout.  All tests use `:memory:` databases so they
//! leave no filesystem residue and can run in parallel.
//!
//! The flat-function surface (`sqlite3.connect` / `execute` / `query`
//! / etc.) is preserved unchanged; coverage for that lives in
//! `m23_p3a_d_sqlite.rs`.  This file exercises the typed `Connection`
//! / `Cursor` ergonomics in isolation.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m35_p4b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn open_create_insert_select() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)\")
    p: List[str] = []
    p.append(\"alice\")
    conn.execute_params(\"INSERT INTO t (name) VALUES (?)\", p)
    cur: Cursor = conn.query(\"SELECT id, name FROM t\")
    rc: i64 = cur.row_count()
    println(\"rows=\" + str(rc))
    all: List[List[str]] = cur.fetchall()
    n: i32 = i32(i64(len(all)))
    println(\"n=\" + str(n))
    if n >= 1i32:
        r: List[str] = all[0i32]
        println(\"id=\" + r[0i32] + \" name=\" + r[1i32])
    conn.close()
    return 0
";
    let p = compile_snippet("open_insert_select", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("rows=1"), "got: {out:?}");
    assert!(out.contains("n=1"), "got: {out:?}");
    assert!(out.contains("id=1 name=alice"), "got: {out:?}");
}

#[test]
fn fetchone_iterates_and_exhausts_to_none() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)\")
    names: List[str] = []
    names.append(\"a\")
    names.append(\"b\")
    names.append(\"c\")
    i: i32 = 0i32
    while i < 3i32:
        p: List[str] = []
        p.append(names[i])
        conn.execute_params(\"INSERT INTO t (name) VALUES (?)\", p)
        i = i + 1i32
    cur: Cursor = conn.query(\"SELECT id, name FROM t ORDER BY id\")
    n: i32 = 0i32
    while true:
        row: List[str]? = cur.fetchone()
        if row is not none:
            println(\"row=\" + row[0i32] + \":\" + row[1i32])
            n = n + 1i32
        else:
            break
    println(\"final n=\" + str(n))
    # Calling fetchone again after exhaustion still returns none.
    after: List[str]? = cur.fetchone()
    if after is none:
        println(\"still_none\")
    conn.close()
    return 0
";
    let p = compile_snippet("fetchone_exhausts", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("row=1:a"), "got: {out:?}");
    assert!(out.contains("row=2:b"), "got: {out:?}");
    assert!(out.contains("row=3:c"), "got: {out:?}");
    assert!(out.contains("final n=3"), "got: {out:?}");
    assert!(out.contains("still_none"), "got: {out:?}");
}

#[test]
fn query_params_filters_rows() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (id INTEGER PRIMARY KEY, tag TEXT, body TEXT)\")
    p1: List[str] = []
    p1.append(\"x\")
    p1.append(\"first\")
    conn.execute_params(\"INSERT INTO t (tag, body) VALUES (?, ?)\", p1)
    p2: List[str] = []
    p2.append(\"y\")
    p2.append(\"second\")
    conn.execute_params(\"INSERT INTO t (tag, body) VALUES (?, ?)\", p2)
    p3: List[str] = []
    p3.append(\"x\")
    p3.append(\"third\")
    conn.execute_params(\"INSERT INTO t (tag, body) VALUES (?, ?)\", p3)
    q: List[str] = []
    q.append(\"x\")
    cur: Cursor = conn.query_params(\"SELECT id, body FROM t WHERE tag = ? ORDER BY id\", q)
    println(\"count=\" + str(cur.row_count()))
    all: List[List[str]] = cur.fetchall()
    i: i32 = 0i32
    while i < i32(i64(len(all))):
        r: List[str] = all[i]
        println(\"r=\" + r[0i32] + \":\" + r[1i32])
        i = i + 1i32
    conn.close()
    return 0
";
    let p = compile_snippet("query_params_filter", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("count=2"), "got: {out:?}");
    assert!(out.contains("r=1:first"), "got: {out:?}");
    assert!(out.contains("r=3:third"), "got: {out:?}");
    assert!(!out.contains("second"), "got: {out:?}");
}

#[test]
fn column_names_match_select_list() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, body TEXT)\")
    cur: Cursor = conn.query(\"SELECT id, name, body FROM t\")
    cols: List[str] = cur.column_names()
    println(\"ncols=\" + str(i32(i64(len(cols)))))
    if i32(i64(len(cols))) >= 3i32:
        println(\"c0=\" + cols[0i32])
        println(\"c1=\" + cols[1i32])
        println(\"c2=\" + cols[2i32])
    conn.close()
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
fn changes_and_last_insert_rowid() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (id INTEGER PRIMARY KEY, tag TEXT)\")
    p: List[str] = []
    p.append(\"a\")
    conn.execute_params(\"INSERT INTO t (tag) VALUES (?)\", p)
    conn.execute_params(\"INSERT INTO t (tag) VALUES (?)\", p)
    conn.execute_params(\"INSERT INTO t (tag) VALUES (?)\", p)
    rid: i64 = conn.last_insert_rowid()
    println(\"rowid=\" + str(rid))
    conn.execute(\"UPDATE t SET tag = 'z' WHERE id <= 2\")
    n: i32 = conn.changes()
    println(\"changes=\" + str(n))
    conn.close()
    return 0
";
    let p = compile_snippet("changes_rowid", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("rowid=3"), "got: {out:?}");
    assert!(out.contains("changes=2"), "got: {out:?}");
}

#[test]
fn bad_sql_raises_value_error() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    try:
        cur: Cursor = conn.query(\"SELECT FROM bogus\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught\")
    conn.close()
    return 0
";
    let p = compile_snippet("bad_sql", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn close_is_idempotent_and_blocks_followup_calls() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.close()
    conn.close()
    conn.close()
    println(\"closed\")
    try:
        conn.execute(\"SELECT 1\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("close_idempotent", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("closed"), "got: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn fetchall_after_fetchone_returns_only_remaining() {
    // After draining one row with fetchone, fetchall should return the
    // remaining 2 rows (not all 3).  This is the iteration-position
    // contract that lets Connection / Cursor coexist with code that
    // mixes fetchone and fetchall.
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)\")
    p: List[str] = []
    p.append(\"a\")
    conn.execute_params(\"INSERT INTO t (name) VALUES (?)\", p)
    q: List[str] = []
    q.append(\"b\")
    conn.execute_params(\"INSERT INTO t (name) VALUES (?)\", q)
    r: List[str] = []
    r.append(\"c\")
    conn.execute_params(\"INSERT INTO t (name) VALUES (?)\", r)
    cur: Cursor = conn.query(\"SELECT id, name FROM t ORDER BY id\")
    first: List[str]? = cur.fetchone()
    if first is not none:
        println(\"first=\" + first[0i32] + \":\" + first[1i32])
    rest: List[List[str]] = cur.fetchall()
    println(\"rest_n=\" + str(i32(i64(len(rest)))))
    i: i32 = 0i32
    while i < i32(i64(len(rest))):
        row: List[str] = rest[i]
        println(\"rest=\" + row[0i32] + \":\" + row[1i32])
        i = i + 1i32
    # A second fetchall is empty (cursor is exhausted).
    rest2: List[List[str]] = cur.fetchall()
    println(\"rest2_n=\" + str(i32(i64(len(rest2)))))
    conn.close()
    return 0
";
    let p = compile_snippet("fetchall_after_one", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("first=1:a"), "got: {out:?}");
    assert!(out.contains("rest_n=2"), "got: {out:?}");
    assert!(out.contains("rest=2:b"), "got: {out:?}");
    assert!(out.contains("rest=3:c"), "got: {out:?}");
    assert!(out.contains("rest2_n=0"), "got: {out:?}");
}

#[test]
fn flat_and_typed_apis_coexist() {
    // The flat M23 P3a-D surface (`sqlite3.connect` / `execute` /
    // `query` / `close`) must still work — the M29 web framework depends
    // on it, and we must not break it.  This test exercises both
    // surfaces in the same program to prove they are independent.
    let src = "\
import sqlite3
fn main() -> i32:
    # Flat surface — opaque i64 handle.
    flat: i64 = sqlite3.connect(\":memory:\")
    sqlite3.execute(flat, \"CREATE TABLE t (v TEXT)\")
    p: List[str] = []
    p.append(\"hi\")
    sqlite3.execute_params(flat, \"INSERT INTO t (v) VALUES (?)\", p)
    rows: List[List[str]] = sqlite3.query(flat, \"SELECT v FROM t\")
    println(\"flat_n=\" + str(i32(i64(len(rows)))))
    if i32(i64(len(rows))) >= 1i32:
        r: List[str] = rows[0i32]
        println(\"flat_v=\" + r[0i32])
    sqlite3.close(flat)
    # Typed surface — Connection class.
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE u (v TEXT)\")
    q: List[str] = []
    q.append(\"hello\")
    conn.execute_params(\"INSERT INTO u (v) VALUES (?)\", q)
    cur: Cursor = conn.query(\"SELECT v FROM u\")
    println(\"typed_n=\" + str(cur.row_count()))
    row: List[str]? = cur.fetchone()
    if row is not none:
        println(\"typed_v=\" + row[0i32])
    conn.close()
    return 0
";
    let p = compile_snippet("flat_typed_coexist", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("flat_n=1"), "got: {out:?}");
    assert!(out.contains("flat_v=hi"), "got: {out:?}");
    assert!(out.contains("typed_n=1"), "got: {out:?}");
    assert!(out.contains("typed_v=hello"), "got: {out:?}");
}

#[test]
fn empty_query_fetchone_returns_none_first_call() {
    let src = "\
import sqlite3
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (v TEXT)\")
    cur: Cursor = conn.query(\"SELECT v FROM t\")
    println(\"rc=\" + str(cur.row_count()))
    r: List[str]? = cur.fetchone()
    if r is none:
        println(\"empty_none\")
    else:
        println(\"UNREACHED\")
    conn.close()
    return 0
";
    let p = compile_snippet("empty_query", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("rc=0"), "got: {out:?}");
    assert!(out.contains("empty_none"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}
