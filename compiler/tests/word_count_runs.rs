//! Subprocess integration test for `examples/word_count.spy` (M22 P2A).
//!
//! Pipes a small canned input through `spy.exe` and asserts the M22
//! `collections.counter_*` API produces the expected ranked output.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn word_count_compiles() {
    let src_path = project_root().join("examples").join("word_count.spy");
    let src = fs::read_to_string(&src_path).expect("read word_count.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile word_count.spy: {e}"));
}

#[test]
fn word_count_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("word_count.spy");
    let src = fs::read_to_string(&src_path).expect("read word_count.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile word_count.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("word_count.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let mut child = Command::new(&spy_bin)
        .arg(&spyc_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn spy.exe");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        // Canned corpus: `the` appears 4×, `cat` and `sat` each 2×,
        // `on` `mat` each 1×, sentinel `:END:` ends the read loop.
        stdin
            .write_all(b"the cat sat on the mat\n")
            .expect("write stdin");
        stdin
            .write_all(b"the cat sat\n")
            .expect("write stdin");
        stdin.write_all(b"the\n").expect("write stdin");
        stdin.write_all(b":END:\n").expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    // `the` is most frequent (4).  cat & sat tie at 2 (alphabetical →
    // cat before sat).  on & mat tie at 1.
    let lines: Vec<&str> = stdout.lines().collect();
    // The first line should be the highest-count word.
    assert!(
        lines.iter().any(|l| l.starts_with("the: 4")),
        "expected `the: 4` line; got:\n{stdout}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("cat: 2")),
        "expected `cat: 2` line; got:\n{stdout}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("sat: 2")),
        "expected `sat: 2` line; got:\n{stdout}"
    );
    // Tie-break for sat-vs-cat at count 2: alphabetical → cat before sat.
    let cat_idx = lines.iter().position(|l| l.starts_with("cat:")).unwrap();
    let sat_idx = lines.iter().position(|l| l.starts_with("sat:")).unwrap();
    assert!(
        cat_idx < sat_idx,
        "tie-break order wrong; cat at {cat_idx}, sat at {sat_idx}\n{stdout}"
    );
}
