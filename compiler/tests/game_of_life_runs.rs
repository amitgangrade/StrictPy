//! Integration test for examples/game_of_life.spy.
//!
//! Compiles the program, runs it through the VM with stdout captured,
//! and checks that:
//!   - the run exits 0
//!   - every generation header (0..=GENERATIONS) appears
//!   - both '#' and '.' appear in the output (we're rendering a real grid)
//!   - the glider starts with the canonical 5 live cells + a 3-cell
//!     blinker = 8 live cells in generation 0
//!   - blinker oscillation is visible: it survives past generation 10
//!     (a pure glider would die long before that on a 30x15 grid)

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn game_of_life_runs_and_produces_frames() {
    let src_path = project_root().join("examples").join("game_of_life.spy");
    let src = fs::read_to_string(&src_path).expect("read game_of_life.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile game_of_life.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("game_of_life.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run game_of_life");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // Every generation header should show up.
    for gen in 0..=20i32 {
        let header = format!("-- generation {gen} --");
        assert!(
            out.contains(&header),
            "missing {header:?} in stdout:\n{out}"
        );
    }

    // We should see both live and dead cells.
    assert!(out.contains('#'), "no '#' (live cells) in output:\n{out}");
    assert!(out.contains('.'), "no '.' (dead cells) in output:\n{out}");

    // Generation 0 should have exactly 8 live cells (5-cell glider + 3-cell
    // blinker). We assert by counting '#' in the gen-0 frame block.
    let gen0_start = out.find("-- generation 0 --").expect("gen 0 header");
    let gen1_start = out.find("-- generation 1 --").expect("gen 1 header");
    let gen0_block = &out[gen0_start..gen1_start];
    let gen0_live = gen0_block.chars().filter(|c| *c == '#').count();
    assert_eq!(gen0_live, 8, "expected 8 live cells in gen 0, block:\n{gen0_block}");

    // Blinker oscillates with period 2 so we expect live cells well past
    // gen 10 (glider on a 30x15 grid walks off the SE corner after roughly
    // 12 generations).
    let gen15_start = out.find("-- generation 15 --").expect("gen 15 header");
    let gen16_start = out.find("-- generation 16 --").expect("gen 16 header");
    let gen15_block = &out[gen15_start..gen16_start];
    let gen15_live = gen15_block.chars().filter(|c| *c == '#').count();
    assert!(
        gen15_live >= 3,
        "expected at least the blinker (3 cells) alive at gen 15; block was:\n{gen15_block}"
    );

    // The final-live-cells summary line must be present.
    assert!(
        out.contains("final live cells:"),
        "summary line missing:\n{out}"
    );
}
