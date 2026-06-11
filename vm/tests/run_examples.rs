//! Integration test: compile each acceptance example with the real
//! compiler, hand the resulting `.spyc` bytes to the VM via a temp file,
//! and assert it runs end-to-end.
//!
//! ## Status after M7
//!
//! All 7 examples run end-to-end with real output.
//!
//! | Example         | Output                                                   |
//! |-----------------|----------------------------------------------------------|
//! | hello.spy       | "Hello, StrictPy!" — exits 0                             |
//! | fib.spy         | fib(0) through fib(15) = 610                             |
//! | dot.spy         | dot(u, v) = 70.0                                         |
//! | tree.spy        | tree sum = 15 (real user-class vtable dispatch)          |
//! | mandelbrot.spy  | 60×30 ASCII fractal                                      |
//! | producer.spy    | "got 0".."got 99" (real OS threads on shared channel)    |
//! | wordcount.spy   | unique-word count + frequency table from input.txt       |

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

fn compile_to_temp(name: &str) -> PathBuf {
    let src_path = examples_dir().join(name);
    let src = fs::read_to_string(&src_path).expect("read example");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_run_examples_{}.spyc", name.replace('.', "_")));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn hello_prints_greeting() {
    let p = compile_to_temp("hello.spy");
    let (code, out) = run_file_capture(&p).expect("hello.spy must run cleanly");
    assert_eq!(code, 0, "expected exit code 0");
    assert_eq!(out, "Hello, StrictPy!\n", "stdout was: {:?}", out);
}

#[test]
fn dot_computes_real_dot_product() {
    let p = compile_to_temp("dot.spy");
    let (code, out) = run_file_capture(&p).expect("dot.spy must run cleanly");
    assert_eq!(code, 0);
    assert!(out.contains("dot(u, v) = 70"), "stdout was: {out:?}");
}

#[test]
fn tree_exits_clean() {
    let p = compile_to_temp("tree.spy");
    let (code, out) = run_file_capture(&p).expect("tree.spy must run cleanly");
    assert_eq!(code, 0);
    assert!(out.contains("tree sum = 15"), "stdout was: {out:?}");
}



#[test]
fn fib_prints_sequence() {
    let p = compile_to_temp("fib.spy");
    let (code, out) = run_file_capture(&p).expect("fib.spy must run cleanly");
    assert_eq!(code, 0);
    // Fibonacci 0..15 = 0,1,1,2,3,5,8,13,21,34,55,89,144,233,377,610
    assert!(out.contains("fib(0) = 0"), "missing fib(0): {out:?}");
    assert!(out.contains("fib(15) = 610"), "missing fib(15): {out:?}");
}

#[test]
fn mandelbrot_renders_fractal() {
    let p = compile_to_temp("mandelbrot.spy");
    let (code, out) = run_file_capture(&p).expect("mandelbrot.spy must run cleanly");
    assert_eq!(code, 0);
    // Real fractal output contains both spaces and '#' chars.
    assert!(out.contains('#'), "no '#' in mandelbrot output: {out:?}");
    assert!(out.contains(' '), "no ' ' in mandelbrot output: {out:?}");
    // At least 20 rows of output expected.
    assert!(out.lines().count() >= 20, "too few rows: {}", out.lines().count());
}

/// Producer/consumer end-to-end (spec §16). Real threading landed in M6-B
/// (this work); lambda lifting landed in M6-A. If the M6-A compiler still
/// hasn't fully wired lambda-capture lowering through the
/// `Thread(fn() -> None: ...)` form, this test surfaces the resulting
/// VmError so the failure is visible.
///
/// BUG-044: the example's consumer originally polled with `try_recv`,
/// which returns the same `none` sentinel for both "empty" and
/// "disconnected" (M5 limitation, see vm/src/builtins.rs::ChannelTryRecv).
/// A consumer that won the race exited early, the producer blocked
/// forever on the full 16-slot channel, and `t1.join()` deadlocked —
/// this test hung the whole suite (observed holding a CI runner for
/// hours). The consumer now uses blocking `recv()` + `except
/// ChannelClosedError`, so the drain is deterministic: all 100 lines,
/// every run, with both threads genuinely concurrent.
#[test]
fn producer_runs() {
    let p = compile_to_temp("producer.spy");
    let (code, out) = run_file_capture(&p).expect("producer.spy must run cleanly");
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");
    // Blocking recv drains everything the producer sent before
    // ChannelClosedError fires, so all 100 lines must be present.
    let count = (0..100)
        .take_while(|i| out.contains(&format!("got {i}\n")))
        .count();
    assert_eq!(
        count, 100,
        "expected all 100 `got N` lines from the blocking-recv consumer; \
         got {count}. stdout was: {out:?}"
    );
}

#[test]
fn wordcount_runs() {
    // Materialise the input file the program reads.
    let dir = std::env::temp_dir();
    let input_path = dir.join("input.txt");
    std::fs::write(&input_path, "the quick brown fox the lazy dog the quick fox\n")
        .expect("write input.txt");

    // Run the program with the working dir set to where input.txt lives.
    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&dir).expect("chdir");
    let result = std::panic::catch_unwind(|| {
        let p = compile_to_temp("wordcount.spy");
        run_file_capture(&p)
    });
    let _ = std::env::set_current_dir(original);
    let _ = std::fs::remove_file(&input_path);

    let outcome = result.expect("compile/run did not panic");
    // M7 acceptance: program runs to completion AND prints a sensible
    // unique-word count. For "the quick brown fox the lazy dog the quick fox"
    // there are 6 unique words.
    let (code, out) = outcome.expect("wordcount.spy must run cleanly");
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");
    assert!(
        out.contains("unique words: 6"),
        "expected `unique words: 6` in stdout; got: {out:?}"
    );
    // Each unique word should appear at least once in the per-line
    // <word>\t<count> dump.
    for w in &["the", "quick", "brown", "fox", "lazy", "dog"] {
        assert!(out.contains(w), "missing word `{w}` in stdout: {out:?}");
    }
}
