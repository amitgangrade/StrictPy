//! M58 in-process test for the high-score persistence logic shared by
//! the three reference games (the canonical reference is
//! `examples/games/highscores.spy`; the games inline copies because
//! StrictPy v0.2 has no user-module import).
//!
//! We compile a snippet that mirrors `save_score` / `top_scores` exactly,
//! run several inserts against a temp-file db (a real file, NOT
//! `:memory:` — each call opens its own Connection, so the rows must
//! survive a reconnect), then assert the top-N comes back score-DESC.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m58_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn save_and_top_scores_orders_desc() {
    // Unique temp db path; remove any stale file from a prior run so the
    // assertions are deterministic.
    let mut db = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    db.push("m58_highscores_test.db");
    let _ = fs::remove_file(&db);
    // Escape backslashes for embedding the Windows path in a .spy literal.
    let db_lit = db.display().to_string().replace('\\', "\\\\");

    let src = format!(
        "\
import sqlite3
import time

fn hs_ensure_schema(conn: Connection) -> None:
    conn.execute(\"CREATE TABLE IF NOT EXISTS scores (game TEXT, name TEXT, score INTEGER, ts INTEGER)\")

fn hs_save_score(db_path: str, game: str, name: str, score: i64) -> None:
    conn: Connection = sqlite3.open(db_path)
    hs_ensure_schema(conn)
    params: List[str] = []
    params.append(game)
    params.append(name)
    params.append(str(score))
    params.append(str(time.now_ms()))
    conn.execute_params(\"INSERT INTO scores (game, name, score, ts) VALUES (?, ?, ?, ?)\", params)
    conn.close()

fn hs_top_scores(db_path: str, game: str, n: i64) -> List[Tuple[str, i64]]:
    conn: Connection = sqlite3.open(db_path)
    hs_ensure_schema(conn)
    sql: str = \"SELECT name, score FROM scores WHERE game = ? ORDER BY score DESC, ts ASC LIMIT \" + str(n)
    qp: List[str] = []
    qp.append(game)
    cur: Cursor = conn.query_params(sql, qp)
    out: List[Tuple[str, i64]] = []
    rows: List[List[str]] = cur.fetchall()
    i: i32 = 0i32
    rn: i32 = i32(i64(len(rows)))
    while i < rn:
        row: List[str] = rows[i]
        out.append((row[0i32], parse_i64(row[1i32])))
        i = i + 1i32
    conn.close()
    return out

fn main() -> i32:
    db: str = \"{db}\"
    hs_save_score(db, \"snake\", \"ALICE\", 500i64)
    hs_save_score(db, \"snake\", \"BOB\", 1200i64)
    hs_save_score(db, \"snake\", \"CARA\", 300i64)
    hs_save_score(db, \"tetris\", \"ZED\", 9999i64)
    top: List[Tuple[str, i64]] = hs_top_scores(db, \"snake\", 3i64)
    i: i32 = 0i32
    n: i32 = i32(i64(len(top)))
    while i < n:
        e: Tuple[str, i64] = top[i]
        println(\"rank\" + str(i + 1i32) + \"=\" + e.0 + \":\" + str(e.1))
        i = i + 1i32
    return 0
",
        db = db_lit
    );

    let p = compile_snippet("highscores", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    // Top-3 for "snake" must be score-DESC: BOB(1200) > ALICE(500) > CARA(300).
    // The "tetris" row must be excluded by the game filter.
    assert!(out.contains("rank1=BOB:1200"), "got: {out:?}");
    assert!(out.contains("rank2=ALICE:500"), "got: {out:?}");
    assert!(out.contains("rank3=CARA:300"), "got: {out:?}");
    assert!(!out.contains("ZED"), "tetris row should be filtered out; got: {out:?}");

    let _ = fs::remove_file(&db);
}
