//! M66 (wave-3 Lane B) — pure `ndarray` math kernels.
//!
//! Plain functions over `&[i64]` shapes and `&[f64]` row-major data.
//! No VM types, no heap, no locks — the builtins layer owns the slot
//! table and error->exception conversion; this module owns the maths.
//!
//! Every array is f64, 1-D or 2-D, row-major, copies-not-views
//! (§9.52). Structural helpers return `(Vec<i64>, Vec<f64>)`; scalar
//! reductions return `f64` / `i64`. Errors carry a StrictPy exception
//! `kind` (`"ValueError"` / `"IndexError"`) plus a message.

/// A single slot-table entry: shape + row-major data.
#[derive(Debug, Clone)]
pub struct NdSlot {
    pub shape: Vec<i64>,
    pub data: Vec<f64>,
}

/// An error that maps 1:1 onto a StrictPy uncaught exception.
#[derive(Debug)]
pub struct NdError {
    pub kind: &'static str,
    pub msg: String,
}

pub type NdResult = Result<(Vec<i64>, Vec<f64>), NdError>;

fn verr(msg: String) -> NdError {
    NdError { kind: "ValueError", msg }
}
fn ierr(msg: String) -> NdError {
    NdError { kind: "IndexError", msg }
}

/// numpy-ish shape formatting: `(3,)` for 1-D, `(2, 3)` for 2-D.
pub fn fmt_shape(shape: &[i64]) -> String {
    match shape.len() {
        1 => format!("({},)", shape[0]),
        _ => {
            let inner = shape
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", inner)
        }
    }
}

/// Total element count for a shape (product of dims).
pub fn numel(shape: &[i64]) -> i64 {
    shape.iter().product()
}

// ── Constructors ────────────────────────────────────────────────────

/// Validate a `zeros`/`ones`/`full` shape: length 1 or 2, no negatives.
fn check_shape(shape: &[i64]) -> Result<(), NdError> {
    if shape.is_empty() || shape.len() > 2 {
        return Err(verr(format!(
            "ndarray: shape must be 1-D or 2-D, got {} dims",
            shape.len()
        )));
    }
    if shape.iter().any(|&d| d < 0) {
        return Err(verr(format!(
            "ndarray: negative dimension in shape {}",
            fmt_shape(shape)
        )));
    }
    Ok(())
}

pub fn zeros(shape: &[i64]) -> NdResult {
    check_shape(shape)?;
    let n = numel(shape).max(0) as usize;
    Ok((shape.to_vec(), vec![0.0; n]))
}

pub fn ones(shape: &[i64]) -> NdResult {
    check_shape(shape)?;
    let n = numel(shape).max(0) as usize;
    Ok((shape.to_vec(), vec![1.0; n]))
}

pub fn full(shape: &[i64], value: f64) -> NdResult {
    check_shape(shape)?;
    let n = numel(shape).max(0) as usize;
    Ok((shape.to_vec(), vec![value; n]))
}

pub fn array1(data: &[f64]) -> NdResult {
    Ok((vec![data.len() as i64], data.to_vec()))
}

/// 2-D from rows; `ValueError` if the rows are ragged.
pub fn array2(rows: &[Vec<f64>]) -> NdResult {
    if rows.is_empty() {
        return Ok((vec![0, 0], vec![]));
    }
    let cols = rows[0].len();
    for (i, r) in rows.iter().enumerate() {
        if r.len() != cols {
            return Err(verr(format!(
                "ndarray.array2: ragged rows — row 0 has {} cols but row {} has {}",
                cols,
                i,
                r.len()
            )));
        }
    }
    let mut data = Vec::with_capacity(rows.len() * cols);
    for r in rows {
        data.extend_from_slice(r);
    }
    Ok((vec![rows.len() as i64, cols as i64], data))
}

pub fn arange(start: f64, stop: f64, step: f64) -> NdResult {
    if step == 0.0 {
        return Err(verr("ndarray.arange: step must be non-zero".into()));
    }
    let mut data = Vec::new();
    let mut v = start;
    if step > 0.0 {
        while v < stop {
            data.push(v);
            v += step;
        }
    } else {
        while v > stop {
            data.push(v);
            v += step;
        }
    }
    Ok((vec![data.len() as i64], data))
}

pub fn linspace(start: f64, stop: f64, num: i64) -> NdResult {
    if num < 0 {
        return Err(verr(format!(
            "ndarray.linspace: num must be non-negative, got {}",
            num
        )));
    }
    let num = num as usize;
    let mut data = Vec::with_capacity(num);
    if num == 0 {
        // empty
    } else if num == 1 {
        data.push(start);
    } else {
        let denom = (num - 1) as f64;
        for i in 0..num {
            data.push(start + (i as f64) * (stop - start) / denom);
        }
    }
    Ok((vec![data.len() as i64], data))
}

pub fn eye(n: i64) -> NdResult {
    if n < 0 {
        return Err(verr(format!(
            "ndarray.eye: n must be non-negative, got {}",
            n
        )));
    }
    let n = n as usize;
    let mut data = vec![0.0; n * n];
    for i in 0..n {
        data[i * n + i] = 1.0;
    }
    Ok((vec![n as i64, n as i64], data))
}

// ── Structural ──────────────────────────────────────────────────────

pub fn reshape(_shape: &[i64], data: &[f64], new_shape: &[i64]) -> NdResult {
    check_shape(new_shape)?;
    let old_n = data.len() as i64;
    let new_n = numel(new_shape);
    if new_n != old_n {
        return Err(verr(format!(
            "ndarray.reshape: cannot reshape array of size {} into shape {}",
            old_n,
            fmt_shape(new_shape)
        )));
    }
    Ok((new_shape.to_vec(), data.to_vec()))
}

pub fn transpose(shape: &[i64], data: &[f64]) -> NdResult {
    if shape.len() == 1 {
        // 1-D transpose is an identity copy.
        return Ok((shape.to_vec(), data.to_vec()));
    }
    let (r, c) = (shape[0] as usize, shape[1] as usize);
    let mut out = vec![0.0; r * c];
    for i in 0..r {
        for j in 0..c {
            out[j * r + i] = data[i * c + j];
        }
    }
    Ok((vec![c as i64, r as i64], out))
}

pub fn flatten(_shape: &[i64], data: &[f64]) -> NdResult {
    Ok((vec![data.len() as i64], data.to_vec()))
}

// ── Broadcasting binary ops ─────────────────────────────────────────

fn build_rc<F: Fn(usize, usize) -> f64>(r: usize, c: usize, f: F) -> Vec<f64> {
    let mut out = Vec::with_capacity(r * c);
    for i in 0..r {
        for j in 0..c {
            out.push(f(i, j));
        }
    }
    out
}

/// Broadcast + apply `f` per §9.52: equal shapes, OR a 1-D operand of
/// length `cols` against a 2-D `(rows, cols)` (row broadcast), OR a
/// `(rows, 1)` operand against `(rows, cols)` (column broadcast).
/// Anything else raises `ValueError` naming both shapes.
pub fn broadcast_binop<F: Fn(f64, f64) -> f64>(
    sa: &[i64],
    da: &[f64],
    sb: &[i64],
    db: &[f64],
    f: F,
) -> NdResult {
    if sa == sb {
        let data = da.iter().zip(db.iter()).map(|(x, y)| f(*x, *y)).collect();
        return Ok((sa.to_vec(), data));
    }
    // a is the 2-D full operand
    if sa.len() == 2 {
        let (r, c) = (sa[0] as usize, sa[1] as usize);
        if sb.len() == 1 && sb[0] == sa[1] {
            let data = build_rc(r, c, |i, j| f(da[i * c + j], db[j]));
            return Ok((vec![sa[0], sa[1]], data));
        }
        if sb.len() == 2 && sb[0] == sa[0] && sb[1] == 1 {
            let data = build_rc(r, c, |i, j| f(da[i * c + j], db[i]));
            return Ok((vec![sa[0], sa[1]], data));
        }
    }
    // b is the 2-D full operand
    if sb.len() == 2 {
        let (r, c) = (sb[0] as usize, sb[1] as usize);
        if sa.len() == 1 && sa[0] == sb[1] {
            let data = build_rc(r, c, |i, j| f(da[j], db[i * c + j]));
            return Ok((vec![sb[0], sb[1]], data));
        }
        if sa.len() == 2 && sa[0] == sb[0] && sa[1] == 1 {
            let data = build_rc(r, c, |i, j| f(da[i], db[i * c + j]));
            return Ok((vec![sb[0], sb[1]], data));
        }
    }
    Err(verr(format!(
        "ndarray: operands could not be broadcast together with shapes {} {}",
        fmt_shape(sa),
        fmt_shape(sb)
    )))
}

// ── Linalg ──────────────────────────────────────────────────────────

pub fn matmul(sa: &[i64], da: &[f64], sb: &[i64], db: &[f64]) -> NdResult {
    if sa.len() != 2 || sb.len() != 2 {
        return Err(verr(format!(
            "ndarray.matmul: both operands must be 2-D, got {} and {}",
            fmt_shape(sa),
            fmt_shape(sb)
        )));
    }
    let (m, k) = (sa[0] as usize, sa[1] as usize);
    let (k2, n) = (sb[0] as usize, sb[1] as usize);
    if k != k2 {
        return Err(verr(format!(
            "ndarray.matmul: shapes {} and {} are not aligned",
            fmt_shape(sa),
            fmt_shape(sb)
        )));
    }
    let mut out = vec![0.0; m * n];
    for i in 0..m {
        for j in 0..n {
            let mut acc = 0.0;
            for p in 0..k {
                acc += da[i * k + p] * db[p * n + j];
            }
            out[i * n + j] = acc;
        }
    }
    Ok((vec![m as i64, n as i64], out))
}

pub fn dot(sa: &[i64], da: &[f64], sb: &[i64], db: &[f64]) -> Result<f64, NdError> {
    if sa.len() != 1 || sb.len() != 1 {
        return Err(verr(format!(
            "ndarray.dot: both operands must be 1-D, got {} and {}",
            fmt_shape(sa),
            fmt_shape(sb)
        )));
    }
    if sa[0] != sb[0] {
        return Err(verr(format!(
            "ndarray.dot: length mismatch {} vs {}",
            sa[0], sb[0]
        )));
    }
    Ok(da.iter().zip(db.iter()).map(|(x, y)| x * y).sum())
}

// ── Reductions (whole-array) ────────────────────────────────────────

pub fn sum(data: &[f64]) -> f64 {
    data.iter().sum()
}

pub fn mean(data: &[f64]) -> Result<f64, NdError> {
    if data.is_empty() {
        return Err(verr("ndarray.mean: empty array".into()));
    }
    Ok(sum(data) / data.len() as f64)
}

pub fn min(data: &[f64]) -> Result<f64, NdError> {
    if data.is_empty() {
        return Err(verr("ndarray.min: empty array".into()));
    }
    Ok(data.iter().copied().fold(f64::INFINITY, f64::min))
}

pub fn max(data: &[f64]) -> Result<f64, NdError> {
    if data.is_empty() {
        return Err(verr("ndarray.max: empty array".into()));
    }
    Ok(data.iter().copied().fold(f64::NEG_INFINITY, f64::max))
}

pub fn std(data: &[f64]) -> Result<f64, NdError> {
    if data.is_empty() {
        return Err(verr("ndarray.std: empty array".into()));
    }
    let m = sum(data) / data.len() as f64;
    let var = data.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / data.len() as f64;
    Ok(var.sqrt())
}

/// Flat index of the first minimum. `data` must be non-empty.
pub fn argmin(data: &[f64]) -> Result<i64, NdError> {
    if data.is_empty() {
        return Err(verr("ndarray.argmin: empty array".into()));
    }
    let mut best = 0usize;
    for i in 1..data.len() {
        if data[i] < data[best] {
            best = i;
        }
    }
    Ok(best as i64)
}

pub fn argmax(data: &[f64]) -> Result<i64, NdError> {
    if data.is_empty() {
        return Err(verr("ndarray.argmax: empty array".into()));
    }
    let mut best = 0usize;
    for i in 1..data.len() {
        if data[i] > data[best] {
            best = i;
        }
    }
    Ok(best as i64)
}

// ── Reductions (per-axis, 2-D only) ─────────────────────────────────

fn check_2d_axis(shape: &[i64], axis: i64, who: &str) -> Result<(usize, usize), NdError> {
    if shape.len() != 2 {
        return Err(verr(format!(
            "ndarray.{}: requires a 2-D array, got {}",
            who,
            fmt_shape(shape)
        )));
    }
    if axis != 0 && axis != 1 {
        return Err(verr(format!(
            "ndarray.{}: axis must be 0 or 1, got {}",
            who, axis
        )));
    }
    Ok((shape[0] as usize, shape[1] as usize))
}

/// axis 0 = collapse rows (down columns) → length `cols`;
/// axis 1 = collapse cols (across rows) → length `rows`.
pub fn sum_axis(shape: &[i64], data: &[f64], axis: i64) -> NdResult {
    let (r, c) = check_2d_axis(shape, axis, "sum_axis")?;
    if axis == 0 {
        let mut out = vec![0.0; c];
        for i in 0..r {
            for j in 0..c {
                out[j] += data[i * c + j];
            }
        }
        Ok((vec![c as i64], out))
    } else {
        let mut out = vec![0.0; r];
        for i in 0..r {
            for j in 0..c {
                out[i] += data[i * c + j];
            }
        }
        Ok((vec![r as i64], out))
    }
}

pub fn mean_axis(shape: &[i64], data: &[f64], axis: i64) -> NdResult {
    let (r, c) = check_2d_axis(shape, axis, "mean_axis")?;
    let (out_shape, mut out) = sum_axis(shape, data, axis)?;
    let divisor = if axis == 0 { r as f64 } else { c as f64 };
    for v in out.iter_mut() {
        *v /= divisor;
    }
    Ok((out_shape, out))
}

// ── Element access ──────────────────────────────────────────────────

pub fn get(shape: &[i64], data: &[f64], i: i64) -> Result<f64, NdError> {
    let n = data.len() as i64;
    if i < 0 || i >= n {
        return Err(ierr(format!(
            "ndarray.get: index {} out of bounds for size {} (shape {})",
            i,
            n,
            fmt_shape(shape)
        )));
    }
    Ok(data[i as usize])
}

pub fn get2(shape: &[i64], data: &[f64], i: i64, j: i64) -> Result<f64, NdError> {
    if shape.len() != 2 {
        return Err(ierr(format!(
            "ndarray.get2: requires a 2-D array, got {}",
            fmt_shape(shape)
        )));
    }
    let (r, c) = (shape[0], shape[1]);
    if i < 0 || i >= r || j < 0 || j >= c {
        return Err(ierr(format!(
            "ndarray.get2: index ({}, {}) out of bounds for shape {}",
            i,
            j,
            fmt_shape(shape)
        )));
    }
    Ok(data[(i * c + j) as usize])
}

/// In-place 1-D set. Mutates `data`.
pub fn set(shape: &[i64], data: &mut [f64], i: i64, v: f64) -> Result<(), NdError> {
    let n = data.len() as i64;
    if i < 0 || i >= n {
        return Err(ierr(format!(
            "ndarray.set: index {} out of bounds for size {} (shape {})",
            i,
            n,
            fmt_shape(shape)
        )));
    }
    data[i as usize] = v;
    Ok(())
}

/// In-place 2-D set. Mutates `data`.
pub fn set2(shape: &[i64], data: &mut [f64], i: i64, j: i64, v: f64) -> Result<(), NdError> {
    if shape.len() != 2 {
        return Err(ierr(format!(
            "ndarray.set2: requires a 2-D array, got {}",
            fmt_shape(shape)
        )));
    }
    let (r, c) = (shape[0], shape[1]);
    if i < 0 || i >= r || j < 0 || j >= c {
        return Err(ierr(format!(
            "ndarray.set2: index ({}, {}) out of bounds for shape {}",
            i,
            j,
            fmt_shape(shape)
        )));
    }
    data[(i * c + j) as usize] = v;
    Ok(())
}

pub fn row(shape: &[i64], data: &[f64], i: i64) -> NdResult {
    if shape.len() != 2 {
        return Err(ierr(format!(
            "ndarray.row: requires a 2-D array, got {}",
            fmt_shape(shape)
        )));
    }
    let (r, c) = (shape[0], shape[1]);
    if i < 0 || i >= r {
        return Err(ierr(format!(
            "ndarray.row: row {} out of bounds for shape {}",
            i,
            fmt_shape(shape)
        )));
    }
    let start = (i * c) as usize;
    Ok((vec![c], data[start..start + c as usize].to_vec()))
}

pub fn col(shape: &[i64], data: &[f64], j: i64) -> NdResult {
    if shape.len() != 2 {
        return Err(ierr(format!(
            "ndarray.col: requires a 2-D array, got {}",
            fmt_shape(shape)
        )));
    }
    let (r, c) = (shape[0], shape[1]);
    if j < 0 || j >= c {
        return Err(ierr(format!(
            "ndarray.col: col {} out of bounds for shape {}",
            j,
            fmt_shape(shape)
        )));
    }
    let mut out = Vec::with_capacity(r as usize);
    for i in 0..r {
        out.push(data[(i * c + j) as usize]);
    }
    Ok((vec![r], out))
}

/// 1-D: `[lo, hi)` element range → 1-D. 2-D: `[lo, hi)` row range → 2-D.
pub fn slice(shape: &[i64], data: &[f64], lo: i64, hi: i64) -> NdResult {
    if shape.len() == 1 {
        let n = shape[0];
        if lo < 0 || hi > n || lo > hi {
            return Err(ierr(format!(
                "ndarray.slice: range [{}, {}) invalid for size {}",
                lo, hi, n
            )));
        }
        Ok((vec![hi - lo], data[lo as usize..hi as usize].to_vec()))
    } else {
        let (r, c) = (shape[0], shape[1]);
        if lo < 0 || hi > r || lo > hi {
            return Err(ierr(format!(
                "ndarray.slice: row range [{}, {}) invalid for shape {}",
                lo,
                hi,
                fmt_shape(shape)
            )));
        }
        let start = (lo * c) as usize;
        let end = (hi * c) as usize;
        Ok((vec![hi - lo, c], data[start..end].to_vec()))
    }
}

// ── Comparisons / clip / where ──────────────────────────────────────

pub fn compare<F: Fn(f64) -> bool>(shape: &[i64], data: &[f64], pred: F) -> NdResult {
    let out = data.iter().map(|&x| if pred(x) { 1.0 } else { 0.0 }).collect();
    Ok((shape.to_vec(), out))
}

pub fn clip(shape: &[i64], data: &[f64], lo: f64, hi: f64) -> NdResult {
    let out = data
        .iter()
        .map(|&x| if x < lo { lo } else if x > hi { hi } else { x })
        .collect();
    Ok((shape.to_vec(), out))
}

/// `where_mask`: element i is `a[i]` where `mask[i] != 0.0`, else `b[i]`.
/// All three shapes must be identical.
pub fn where_mask(
    sm: &[i64],
    dm: &[f64],
    sa: &[i64],
    da: &[f64],
    sb: &[i64],
    db: &[f64],
) -> NdResult {
    if sm != sa || sm != sb {
        return Err(verr(format!(
            "ndarray.where_mask: shape mismatch mask {} a {} b {}",
            fmt_shape(sm),
            fmt_shape(sa),
            fmt_shape(sb)
        )));
    }
    let out = (0..dm.len())
        .map(|i| if dm[i] != 0.0 { da[i] } else { db[i] })
        .collect();
    Ok((sm.to_vec(), out))
}

// ── Elementwise unary / scalar helpers ──────────────────────────────

pub fn map_unary<F: Fn(f64) -> f64>(shape: &[i64], data: &[f64], f: F) -> (Vec<i64>, Vec<f64>) {
    (shape.to_vec(), data.iter().map(|&x| f(x)).collect())
}

// ── Display ─────────────────────────────────────────────────────────

/// numpy-ish float formatting: integral values keep a trailing `.0`;
/// non-finite values render as `nan` / `inf` / `-inf`.
fn fmt_num(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let s = format!("{}", v);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// Edge count for truncation: dims longer than `2*EDGE` collapse to
/// `first EDGE … last EDGE` with an `...` marker (numpy-style).
const EDGE: usize = 3;
const THRESHOLD: usize = 2 * EDGE;

fn format_row(vals: &[f64]) -> String {
    let n = vals.len();
    let mut parts: Vec<String> = Vec::new();
    if n > THRESHOLD {
        for &v in &vals[..EDGE] {
            parts.push(fmt_num(v));
        }
        parts.push("...".into());
        for &v in &vals[n - EDGE..] {
            parts.push(fmt_num(v));
        }
    } else {
        for &v in vals {
            parts.push(fmt_num(v));
        }
    }
    format!("[{}]", parts.join(", "))
}

/// numpy-ish repr: 1-D `[1.0, 2.0]`; 2-D one row per line inside an
/// outer bracket, truncated past `THRESHOLD` rows/cols per edge.
pub fn show(shape: &[i64], data: &[f64]) -> String {
    if shape.len() == 1 {
        return format_row(data);
    }
    let (r, c) = (shape[0] as usize, shape[1] as usize);
    let row_at = |i: usize| -> String {
        let start = i * c;
        format_row(&data[start..start + c])
    };
    let mut rows: Vec<String> = Vec::new();
    if r > THRESHOLD {
        for i in 0..EDGE {
            rows.push(row_at(i));
        }
        rows.push("...".into());
        for i in r - EDGE..r {
            rows.push(row_at(i));
        }
    } else {
        for i in 0..r {
            rows.push(row_at(i));
        }
    }
    format!("[{}]", rows.join("\n "))
}
