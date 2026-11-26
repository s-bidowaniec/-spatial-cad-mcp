//! Pure-Rust readers for open-standard hydrodynamic modeling formats:
//! - TELEMAC Selafin / SERAFIN / SERAFIND (`.slf`, `.res`) binary meshes & results
//! - EPA SWMM (`.inp`) urban drainage network & time-series inputs
//! - SMS `.2dm` 2D unstructured hydrodynamic meshes (nodes, elements, node-strings, materials)

use cad_envelope::ToolError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HdmFormat {
    TelemacSelafin,
    SwmmInp,
    Sms2dm,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BBox2D {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct VariableInfo {
    pub name: String,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeWindow {
    pub step_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_datetime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_datetime: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HdmSummary {
    pub format: HdmFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<String>,
    pub node_count: usize,
    pub element_count: usize,
    pub extent: Option<BBox2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_window: Option<TimeWindow>,
    pub variables: Vec<VariableInfo>,
    pub timeseries_names: Vec<String>,
    pub details: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeriesPoint {
    pub step: usize,
    pub time: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeriesStats {
    pub count: usize,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ExtractedSeries {
    pub format: HdmFormat,
    pub series_name: String,
    pub unit: Option<String>,
    pub resolved_node_index: Option<usize>,
    pub distance_to_xy: Option<f64>,
    pub stats: SeriesStats,
    pub points: Vec<SeriesPoint>,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

pub fn detect_hdm_format(path: &Path) -> Result<HdmFormat, ToolError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "slf" | "res" => Ok(HdmFormat::TelemacSelafin),
        "inp" => Ok(HdmFormat::SwmmInp),
        "2dm" => Ok(HdmFormat::Sms2dm),
        other => Err(ToolError::new(
            "unsupported_format",
            format!(
                "Unsupported HDM extension '.{other}'. Supported formats: .slf/.res (TELEMAC Selafin), .inp (EPA SWMM), .2dm (SMS mesh)."
            ),
        )),
    }
}

pub fn summarize_hdm(path: &Path) -> Result<HdmSummary, ToolError> {
    match detect_hdm_format(path)? {
        HdmFormat::TelemacSelafin => summarize_selafin(path),
        HdmFormat::SwmmInp => summarize_swmm(path),
        HdmFormat::Sms2dm => summarize_2dm(path),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfilePoint {
    pub station: f64,
    pub x: f64,
    pub y: f64,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SwmmConduitProfile {
    pub conduit: String,
    pub from_node: String,
    pub to_node: String,
    pub length: f64,
    pub from_invert: Option<f64>,
    pub to_invert: Option<f64>,
    pub slope: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfilePayload {
    SelafinLine {
        variable: String,
        unit: Option<String>,
        time_step: usize,
        time_seconds: f64,
        length: f64,
        hit_count: usize,
        min: Option<f64>,
        max: Option<f64>,
        mean: Option<f64>,
        points: Vec<ProfilePoint>,
    },
    SwmmConduits {
        conduits: Vec<SwmmConduitProfile>,
    },
}

#[derive(Debug, Clone)]
pub struct CsvExportStats {
    pub format: HdmFormat,
    pub series_name: String,
    pub rows_written: usize,
    pub stats: SeriesStats,
}

/// Export a time series (Selafin node/XY variable or SWMM `[TIMESERIES]`) to a new CSV file.
pub fn export_timeseries_csv(
    path: &Path,
    series_name: &str,
    node_index: Option<usize>,
    xy: Option<[f64; 2]>,
    output_path: &Path,
) -> Result<CsvExportStats, ToolError> {
    let extracted = query_timeseries(path, series_name, node_index, xy)?;
    let mut csv = String::from("step,time,value\n");
    for p in &extracted.points {
        csv.push_str(&format!("{},{},{}\n", p.step, p.time, p.value));
    }
    std::fs::write(output_path, csv)
        .map_err(|e| ToolError::new("internal_error", format!("Failed writing CSV slice: {e}")))?;
    Ok(CsvExportStats {
        format: extracted.format,
        series_name: extracted.series_name,
        rows_written: extracted.points.len(),
        stats: extracted.stats,
    })
}

/// Extract a cross-section profile (TELEMAC Selafin along a 2D line at a time step,
/// or EPA SWMM conduit longitudinal profile).
pub fn query_profiles(
    path: &Path,
    variable: Option<&str>,
    time_step: Option<usize>,
    start: Option<[f64; 2]>,
    end: Option<[f64; 2]>,
    samples: usize,
) -> Result<ProfilePayload, ToolError> {
    match detect_hdm_format(path)? {
        HdmFormat::TelemacSelafin => {
            let var = variable.ok_or_else(|| {
                ToolError::new(
                    "invalid_input",
                    "variable is required when querying profiles from a TELEMAC Selafin (.slf/.res) file.",
                )
            })?;
            let (Some(p0), Some(p1)) = (start, end) else {
                return Err(ToolError::new(
                    "invalid_input",
                    "start ([x0, y0]) and end ([x1, y1]) are required for Selafin line profiles.",
                ));
            };
            query_selafin_profile(path, var, time_step, p0, p1, samples)
        }
        HdmFormat::SwmmInp => query_swmm_conduit_profile(path),
        HdmFormat::Sms2dm => Err(ToolError::new(
            "invalid_input",
            "For SMS .2dm elevation profiles, use `mesh_section_line` in mcp-mesh.",
        )),
    }
}

fn bary_interp_val(
    p: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
    c: [f64; 2],
    va: f64,
    vb: f64,
    vc: f64,
) -> Option<f64> {
    let v0 = [b[0] - a[0], b[1] - a[1]];
    let v1 = [c[0] - a[0], c[1] - a[1]];
    let v2 = [p[0] - a[0], p[1] - a[1]];
    let denom = v0[0] * v1[1] - v1[0] * v0[1];
    if denom.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / denom;
    let u = (v2[0] * v1[1] - v1[0] * v2[1]) * inv;
    let v = (v0[0] * v2[1] - v2[0] * v0[1]) * inv;
    let w = 1.0 - u - v;
    const EPS: f64 = -1e-9;
    if u >= EPS && v >= EPS && w >= EPS {
        Some(w * va + u * vb + v * vc)
    } else {
        None
    }
}

fn query_selafin_profile(
    path: &Path,
    var_name: &str,
    requested_step: Option<usize>,
    start: [f64; 2],
    end: [f64; 2],
    samples: usize,
) -> Result<ProfilePayload, ToolError> {
    let raw = std::fs::read(path).map_err(|e| {
        ToolError::new(
            "cannot_open_hdm",
            format!("Failed to read Selafin file: {e}"),
        )
    })?;
    let hdr = parse_selafin_header(&raw)?;
    if hdr.ndp != 3 {
        return Err(ToolError::new(
            "unsupported_format",
            format!(
                "Selafin profile interpolation currently supports triangular meshes (ndp=3), got ndp={}.",
                hdr.ndp
            ),
        ));
    }

    let want = var_name.trim().to_ascii_lowercase();
    let var_idx = hdr
        .variables
        .iter()
        .position(|v| v.name.to_ascii_lowercase() == want)
        .or_else(|| {
            hdr.variables
                .iter()
                .position(|v| v.name.to_ascii_lowercase().contains(&want))
        })
        .ok_or_else(|| {
            ToolError::new(
                "invalid_input",
                format!("Variable '{var_name}' not found in Selafin file."),
            )
        })?;

    // Collect time steps for the requested variable
    let mut r = FortranReader {
        data: &raw,
        pos: hdr.body_offset,
    };
    let mut steps: Vec<(f64, Vec<f64>)> = Vec::new();
    while r.has_more() {
        let Ok(trec) = r.next_record() else {
            break;
        };
        let Ok(tvals) = read_be_floats(trec, hdr.is_double) else {
            break;
        };
        let Some(&t) = tvals.first() else {
            break;
        };
        let mut chosen: Option<Vec<f64>> = None;
        let mut ok = true;
        for vi in 0..hdr.variables.len() {
            let Ok(vrec) = r.next_record() else {
                ok = false;
                break;
            };
            if vi == var_idx {
                chosen = read_be_floats(vrec, hdr.is_double).ok();
            }
        }
        if !ok {
            break;
        }
        if let Some(vals) = chosen {
            steps.push((t, vals));
        }
    }
    if steps.is_empty() {
        return Err(ToolError::new(
            "cannot_open_hdm",
            "Selafin file contains no time steps.",
        ));
    }
    let step_idx = match requested_step {
        Some(s) if s < steps.len() => s,
        Some(s) => {
            return Err(ToolError::new(
                "invalid_input",
                format!(
                    "time_step {s} out of bounds (available steps: 0..{}).",
                    steps.len()
                ),
            ));
        }
        None => steps.len() - 1,
    };
    let (time_seconds, ref values) = steps[step_idx];

    let n = samples.clamp(2, 200);
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length = dx.hypot(dy);

    let mut points = Vec::with_capacity(n);
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    let mut sum_v = 0.0;
    let mut hit_count = 0usize;

    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        let px = start[0] + dx * t;
        let py = start[1] + dy * t;
        let mut found = None;

        for e in 0..hdr.nelem {
            let i0 = (hdr.ikle[3 * e] - 1) as usize;
            let i1 = (hdr.ikle[3 * e + 1] - 1) as usize;
            let i2 = (hdr.ikle[3 * e + 2] - 1) as usize;
            if i0 >= hdr.npoin || i1 >= hdr.npoin || i2 >= hdr.npoin {
                continue;
            }
            if let Some(v) = bary_interp_val(
                [px, py],
                [hdr.xs[i0], hdr.ys[i0]],
                [hdr.xs[i1], hdr.ys[i1]],
                [hdr.xs[i2], hdr.ys[i2]],
                values[i0],
                values[i1],
                values[i2],
            ) {
                found = Some(round4(v));
                break;
            }
        }

        if let Some(v) = found {
            min_v = min_v.min(v);
            max_v = max_v.max(v);
            sum_v += v;
            hit_count += 1;
        }
        points.push(ProfilePoint {
            station: round3(length * t),
            x: round3(px),
            y: round3(py),
            value: found,
        });
    }

    Ok(ProfilePayload::SelafinLine {
        variable: hdr.variables[var_idx].name.clone(),
        unit: (!hdr.variables[var_idx].unit.is_empty())
            .then(|| hdr.variables[var_idx].unit.clone()),
        time_step: step_idx,
        time_seconds: round3(time_seconds),
        length: round3(length),
        hit_count,
        min: (hit_count > 0).then_some(round4(min_v)),
        max: (hit_count > 0).then_some(round4(max_v)),
        mean: (hit_count > 0).then_some(round4(sum_v / hit_count as f64)),
        points,
    })
}

fn query_swmm_conduit_profile(path: &Path) -> Result<ProfilePayload, ToolError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ToolError::new("cannot_open_hdm", format!("Failed to read SWMM .inp: {e}")))?;

    let mut section = String::new();
    let mut node_inverts: BTreeMap<String, f64> = BTreeMap::new();
    let mut conduits_raw: Vec<(String, String, String, f64)> = Vec::new();

    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_uppercase();
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match section.as_str() {
            "JUNCTIONS" | "OUTFALLS" | "STORAGE" => {
                if parts.len() >= 2
                    && let Ok(inv) = parts[1].parse::<f64>()
                {
                    node_inverts.insert(parts[0].to_string(), inv);
                }
            }
            "CONDUITS" => {
                if parts.len() >= 4
                    && let Ok(len) = parts[3].parse::<f64>()
                {
                    conduits_raw.push((
                        parts[0].to_string(),
                        parts[1].to_string(),
                        parts[2].to_string(),
                        len,
                    ));
                }
            }
            _ => {}
        }
    }

    let mut conduits: Vec<SwmmConduitProfile> = conduits_raw
        .into_iter()
        .map(|(conduit, from_node, to_node, length)| {
            let from_inv = node_inverts.get(&from_node).copied();
            let to_inv = node_inverts.get(&to_node).copied();
            let slope = match (from_inv, to_inv) {
                (Some(z0), Some(z1)) if length > 1e-9 => Some(round4((z0 - z1) / length)),
                _ => None,
            };
            SwmmConduitProfile {
                conduit,
                from_node,
                to_node,
                length: round3(length),
                from_invert: from_inv.map(round3),
                to_invert: to_inv.map(round3),
                slope,
            }
        })
        .collect();
    conduits.sort_by(|a, b| a.conduit.cmp(&b.conduit));
    Ok(ProfilePayload::SwmmConduits { conduits })
}

pub fn query_timeseries(
    path: &Path,
    name: &str,
    node_index: Option<usize>,
    xy: Option<[f64; 2]>,
) -> Result<ExtractedSeries, ToolError> {
    match detect_hdm_format(path)? {
        HdmFormat::TelemacSelafin => query_selafin_series(path, name, node_index, xy),
        HdmFormat::SwmmInp => query_swmm_series(path, name),
        HdmFormat::Sms2dm => Err(ToolError::new(
            "invalid_input",
            "SMS .2dm files contain mesh geometry only (no time series). Query a .slf/.res or SWMM .inp file instead.",
        )),
    }
}

fn compute_stats(points: &[SeriesPoint]) -> SeriesStats {
    if points.is_empty() {
        return SeriesStats {
            count: 0,
            min: None,
            max: None,
            mean: None,
        };
    }
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    let mut n = 0usize;
    for p in points {
        if p.value.is_finite() {
            min = min.min(p.value);
            max = max.max(p.value);
            sum += p.value;
            n += 1;
        }
    }
    if n == 0 {
        return SeriesStats {
            count: 0,
            min: None,
            max: None,
            mean: None,
        };
    }
    SeriesStats {
        count: n,
        min: Some(round4(min)),
        max: Some(round4(max)),
        mean: Some(round4(sum / n as f64)),
    }
}

// ---------- TELEMAC Selafin (Fortran Big-Endian Sequential Records) ----------

struct FortranReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> FortranReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn has_more(&self) -> bool {
        self.pos + 8 <= self.data.len()
    }

    fn next_record(&mut self) -> Result<&'a [u8], ToolError> {
        let bad =
            |msg: &str| ToolError::new("cannot_open_hdm", format!("Invalid Selafin record: {msg}"));
        let len_bytes: [u8; 4] = self
            .data
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| bad("unexpected EOF reading record header"))?
            .try_into()
            .map_err(|_| bad("header slice"))?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        let payload_start = self.pos + 4;
        let payload_end = payload_start
            .checked_add(len)
            .ok_or_else(|| bad("record length overflow"))?;
        let trailer_end = payload_end
            .checked_add(4)
            .ok_or_else(|| bad("record trailer overflow"))?;
        let payload = self
            .data
            .get(payload_start..payload_end)
            .ok_or_else(|| bad("truncated record payload"))?;
        let trail_bytes: [u8; 4] = self
            .data
            .get(payload_end..trailer_end)
            .ok_or_else(|| bad("missing record trailer"))?
            .try_into()
            .map_err(|_| bad("trailer slice"))?;
        let trail = u32::from_be_bytes(trail_bytes) as usize;
        if trail != len {
            return Err(bad("record header/trailer length mismatch"));
        }
        self.pos = trailer_end;
        Ok(payload)
    }
}

fn read_be_i32_vec(rec: &[u8]) -> Result<Vec<i32>, ToolError> {
    if !rec.len().is_multiple_of(4) {
        return Err(ToolError::new(
            "cannot_open_hdm",
            "Selafin integer record is not a multiple of 4 bytes.",
        ));
    }
    Ok(rec
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&c| i32::from_be_bytes(c))
        .collect())
}

fn read_be_floats(rec: &[u8], is_double: bool) -> Result<Vec<f64>, ToolError> {
    if is_double {
        if !rec.len().is_multiple_of(8) {
            return Err(ToolError::new(
                "cannot_open_hdm",
                "Selafin double-precision record is not a multiple of 8 bytes.",
            ));
        }
        Ok(rec
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&c| f64::from_be_bytes(c))
            .collect())
    } else {
        if !rec.len().is_multiple_of(4) {
            return Err(ToolError::new(
                "cannot_open_hdm",
                "Selafin single-precision record is not a multiple of 4 bytes.",
            ));
        }
        Ok(rec
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| f32::from_be_bytes(c) as f64)
            .collect())
    }
}

struct SelafinHeader {
    title: String,
    is_double: bool,
    variables: Vec<VariableInfo>,
    nelem: usize,
    npoin: usize,
    ndp: usize,
    start_date: Option<String>,
    ikle: Vec<i32>,
    xs: Vec<f64>,
    ys: Vec<f64>,
    body_offset: usize,
}

fn parse_selafin_header(raw: &[u8]) -> Result<SelafinHeader, ToolError> {
    let mut r = FortranReader::new(raw);
    let title_rec = r.next_record()?;
    if title_rec.len() < 72 {
        return Err(ToolError::new(
            "cannot_open_hdm",
            "Selafin title record is shorter than 72 bytes.",
        ));
    }
    let fmt_tag = if title_rec.len() >= 80 {
        String::from_utf8_lossy(&title_rec[72..80])
            .trim()
            .to_string()
    } else {
        String::new()
    };
    let title = String::from_utf8_lossy(&title_rec[..72]).trim().to_string();

    let nbv = read_be_i32_vec(r.next_record()?)?;
    if nbv.len() < 2 {
        return Err(ToolError::new(
            "cannot_open_hdm",
            "Invalid Selafin NBV record.",
        ));
    }
    let total_vars = (nbv[0].max(0) + nbv[1].max(0)) as usize;
    let mut variables = Vec::with_capacity(total_vars);
    for _ in 0..total_vars {
        let vrec = r.next_record()?;
        let name = String::from_utf8_lossy(&vrec[..vrec.len().min(16)])
            .trim()
            .to_string();
        let unit = if vrec.len() > 16 {
            String::from_utf8_lossy(&vrec[16..vrec.len().min(32)])
                .trim()
                .to_string()
        } else {
            String::new()
        };
        variables.push(VariableInfo { name, unit });
    }

    let iparam = read_be_i32_vec(r.next_record()?)?;
    let start_date = if iparam.len() >= 10 && iparam[9] == 1 {
        let d = read_be_i32_vec(r.next_record()?)?;
        if d.len() >= 6 {
            Some(format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                d[0], d[1], d[2], d[3], d[4], d[5]
            ))
        } else {
            None
        }
    } else {
        None
    };

    let dims = read_be_i32_vec(r.next_record()?)?;
    if dims.len() < 3 {
        return Err(ToolError::new(
            "cannot_open_hdm",
            "Invalid Selafin dimensions record.",
        ));
    }
    let nelem = dims[0].max(0) as usize;
    let npoin = dims[1].max(0) as usize;
    let ndp = dims[2].max(0) as usize;

    let ikle = read_be_i32_vec(r.next_record()?)?;
    let _ipobo = r.next_record()?;
    let x_rec = r.next_record()?;
    let y_rec = r.next_record()?;

    let is_double =
        fmt_tag.eq_ignore_ascii_case("SERAFIND") || (npoin > 0 && x_rec.len() == npoin * 8);

    let xs = read_be_floats(x_rec, is_double)?;
    let ys = read_be_floats(y_rec, is_double)?;

    Ok(SelafinHeader {
        title,
        is_double,
        variables,
        nelem,
        npoin,
        ndp,
        start_date,
        ikle,
        xs,
        ys,
        body_offset: r.pos,
    })
}

fn summarize_selafin(path: &Path) -> Result<HdmSummary, ToolError> {
    let raw = std::fs::read(path).map_err(|e| {
        ToolError::new(
            "cannot_open_hdm",
            format!("Failed to read Selafin file: {e}"),
        )
    })?;
    let hdr = parse_selafin_header(&raw)?;

    let mut r = FortranReader {
        data: &raw,
        pos: hdr.body_offset,
    };
    let mut step_count = 0usize;
    let mut t_start: Option<f64> = None;
    let mut t_end: Option<f64> = None;

    while r.has_more() {
        let Ok(trec) = r.next_record() else {
            break;
        };
        let Ok(tvals) = read_be_floats(trec, hdr.is_double) else {
            break;
        };
        let Some(&t) = tvals.first() else {
            break;
        };
        let mut complete = true;
        for _ in 0..hdr.variables.len() {
            if r.next_record().is_err() {
                complete = false;
                break;
            }
        }
        if !complete {
            break;
        }
        if t_start.is_none() {
            t_start = Some(round3(t));
        }
        t_end = Some(round3(t));
        step_count += 1;
    }

    let extent = compute_extent_xy(&hdr.xs, &hdr.ys);
    let mut details = BTreeMap::new();
    details.insert("nodes_per_element".to_string(), serde_json::json!(hdr.ndp));
    let timeseries_names = hdr.variables.iter().map(|v| v.name.clone()).collect();

    Ok(HdmSummary {
        format: HdmFormat::TelemacSelafin,
        title: (!hdr.title.is_empty()).then_some(hdr.title),
        precision: Some(if hdr.is_double {
            "f64 (SERAFIND)".to_string()
        } else {
            "f32 (SERAFIN)".to_string()
        }),
        node_count: hdr.npoin,
        element_count: hdr.nelem,
        extent,
        time_window: Some(TimeWindow {
            step_count,
            start_seconds: t_start,
            end_seconds: t_end,
            start_datetime: hdr.start_date,
            end_datetime: None,
        }),
        variables: hdr.variables,
        timeseries_names,
        details,
    })
}

fn query_selafin_series(
    path: &Path,
    var_name: &str,
    node_index: Option<usize>,
    xy: Option<[f64; 2]>,
) -> Result<ExtractedSeries, ToolError> {
    let raw = std::fs::read(path).map_err(|e| {
        ToolError::new(
            "cannot_open_hdm",
            format!("Failed to read Selafin file: {e}"),
        )
    })?;
    let hdr = parse_selafin_header(&raw)?;

    let want = var_name.trim().to_ascii_lowercase();
    let var_idx = hdr
        .variables
        .iter()
        .position(|v| v.name.to_ascii_lowercase() == want)
        .or_else(|| {
            hdr.variables
                .iter()
                .position(|v| v.name.to_ascii_lowercase().contains(&want))
        })
        .ok_or_else(|| {
            let names: Vec<&str> = hdr.variables.iter().map(|v| v.name.as_str()).collect();
            ToolError::new(
                "invalid_input",
                format!(
                    "Variable '{var_name}' not found in Selafin file. Available variables: {}",
                    names.join(", ")
                ),
            )
        })?;

    let (resolved_idx, dist) = if let Some(idx) = node_index {
        if idx >= hdr.npoin {
            return Err(ToolError::new(
                "invalid_input",
                format!(
                    "node_index {idx} is out of bounds (node_count = {}).",
                    hdr.npoin
                ),
            ));
        }
        (idx, None)
    } else if let Some([qx, qy]) = xy {
        if hdr.xs.is_empty() || hdr.ys.is_empty() {
            return Err(ToolError::new(
                "invalid_input",
                "Selafin file contains no node coordinates for xy lookup.",
            ));
        }
        let mut best_i = 0usize;
        let mut best_d2 = f64::INFINITY;
        for (i, (&x, &y)) in hdr.xs.iter().zip(&hdr.ys).enumerate() {
            let d2 = (x - qx) * (x - qx) + (y - qy) * (y - qy);
            if d2 < best_d2 {
                best_d2 = d2;
                best_i = i;
            }
        }
        (best_i, Some(round3(best_d2.sqrt())))
    } else {
        return Err(ToolError::new(
            "invalid_input",
            "Provide either `node_index` (0-based) or `xy` ([x, y]) to query a Selafin time series.",
        ));
    };

    let mut r = FortranReader {
        data: &raw,
        pos: hdr.body_offset,
    };
    let mut points = Vec::new();
    let mut step = 0usize;
    let elem_size = if hdr.is_double { 8 } else { 4 };

    while r.has_more() {
        let Ok(trec) = r.next_record() else {
            break;
        };
        let Ok(tvals) = read_be_floats(trec, hdr.is_double) else {
            break;
        };
        let Some(&t) = tvals.first() else {
            break;
        };
        let mut val_opt = None;
        let mut ok = true;
        for vi in 0..hdr.variables.len() {
            let Ok(vrec) = r.next_record() else {
                ok = false;
                break;
            };
            if vi == var_idx {
                let off = resolved_idx * elem_size;
                if let Some(chunk) = vrec.get(off..off + elem_size) {
                    let v = if hdr.is_double {
                        f64::from_be_bytes([
                            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6],
                            chunk[7],
                        ])
                    } else {
                        f32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64
                    };
                    val_opt = Some(round4(v));
                }
            }
        }
        if !ok {
            break;
        }
        if let Some(val) = val_opt {
            points.push(SeriesPoint {
                step,
                time: format!("{}", round3(t)),
                value: val,
            });
        }
        step += 1;
    }

    let stats = compute_stats(&points);
    Ok(ExtractedSeries {
        format: HdmFormat::TelemacSelafin,
        series_name: hdr.variables[var_idx].name.clone(),
        unit: (!hdr.variables[var_idx].unit.is_empty())
            .then(|| hdr.variables[var_idx].unit.clone()),
        resolved_node_index: Some(resolved_idx),
        distance_to_xy: dist,
        stats,
        points,
    })
}

// ---------- EPA SWMM (.inp) ----------

fn summarize_swmm(path: &Path) -> Result<HdmSummary, ToolError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ToolError::new("cannot_open_hdm", format!("Failed to read SWMM .inp: {e}")))?;

    let mut section = String::new();
    let mut title_lines: Vec<String> = Vec::new();
    let mut options: BTreeMap<String, String> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut ts_names: BTreeSet<String> = BTreeSet::new();
    let mut xs = Vec::new();
    let mut ys = Vec::new();

    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_uppercase();
            continue;
        }
        match section.as_str() {
            "TITLE" => title_lines.push(line.to_string()),
            "OPTIONS" => {
                let mut parts = line.split_whitespace();
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    options.insert(k.to_ascii_uppercase(), v.to_string());
                }
            }
            "COORDINATES" => {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3
                    && let (Ok(x), Ok(y)) = (parts[1].parse::<f64>(), parts[2].parse::<f64>())
                {
                    xs.push(x);
                    ys.push(y);
                }
            }
            "TIMESERIES" => {
                if let Some(name) = line.split_whitespace().next() {
                    ts_names.insert(name.to_string());
                }
            }
            "JUNCTIONS" | "OUTFALLS" | "STORAGE" | "CONDUITS" | "PUMPS" | "ORIFICES" | "WEIRS"
            | "SUBCATCHMENTS" | "RAINGAGES" => {
                *counts.entry(section.to_ascii_lowercase()).or_insert(0) += 1;
            }
            _ => {}
        }
    }

    if options.is_empty() && counts.is_empty() && title_lines.is_empty() {
        return Err(ToolError::new(
            "cannot_open_hdm",
            "Input does not look like a valid EPA SWMM .inp file.",
        ));
    }

    let node_count = counts.get("junctions").copied().unwrap_or(0)
        + counts.get("outfalls").copied().unwrap_or(0)
        + counts.get("storage").copied().unwrap_or(0);
    let element_count = counts.get("conduits").copied().unwrap_or(0)
        + counts.get("pumps").copied().unwrap_or(0)
        + counts.get("orifices").copied().unwrap_or(0)
        + counts.get("weirs").copied().unwrap_or(0);

    let start_dt = options.get("START_DATE").map(|d| {
        let t = options
            .get("START_TIME")
            .map(String::as_str)
            .unwrap_or("00:00:00");
        format!("{d} {t}")
    });
    let end_dt = options.get("END_DATE").map(|d| {
        let t = options
            .get("END_TIME")
            .map(String::as_str)
            .unwrap_or("00:00:00");
        format!("{d} {t}")
    });

    let mut details = BTreeMap::new();
    for (k, v) in counts {
        details.insert(k, serde_json::json!(v));
    }
    if let Some(fu) = options.get("FLOW_UNITS") {
        details.insert("flow_units".to_string(), serde_json::json!(fu));
    }
    if let Some(rm) = options.get("ROUTING_MODEL") {
        details.insert("routing_model".to_string(), serde_json::json!(rm));
    }

    Ok(HdmSummary {
        format: HdmFormat::SwmmInp,
        title: (!title_lines.is_empty()).then(|| title_lines.join(" | ")),
        precision: None,
        node_count,
        element_count,
        extent: compute_extent_xy(&xs, &ys),
        time_window: (start_dt.is_some() || end_dt.is_some()).then_some(TimeWindow {
            step_count: 0,
            start_seconds: None,
            end_seconds: None,
            start_datetime: start_dt,
            end_datetime: end_dt,
        }),
        variables: Vec::new(),
        timeseries_names: ts_names.into_iter().collect(),
        details,
    })
}

fn query_swmm_series(path: &Path, name: &str) -> Result<ExtractedSeries, ToolError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ToolError::new("cannot_open_hdm", format!("Failed to read SWMM .inp: {e}")))?;

    let mut section = String::new();
    let mut available: BTreeSet<String> = BTreeSet::new();
    let mut points = Vec::new();
    let mut step = 0usize;

    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_uppercase();
            continue;
        }
        if section != "TIMESERIES" {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        available.insert(parts[0].to_string());
        if !parts[0].eq_ignore_ascii_case(name) {
            continue;
        }
        if parts.len() >= 4
            && let Ok(val) = parts[3].parse::<f64>()
        {
            points.push(SeriesPoint {
                step,
                time: format!("{} {}", parts[1], parts[2]),
                value: round4(val),
            });
            step += 1;
        } else if parts.len() >= 3
            && let Ok(val) = parts[2].parse::<f64>()
        {
            points.push(SeriesPoint {
                step,
                time: parts[1].to_string(),
                value: round4(val),
            });
            step += 1;
        }
    }

    if points.is_empty() {
        let avail: Vec<String> = available.into_iter().collect();
        return Err(ToolError::new(
            "invalid_input",
            format!(
                "Time series '{name}' not found in SWMM [TIMESERIES]. Available: {}",
                avail.join(", ")
            ),
        ));
    }

    let stats = compute_stats(&points);
    Ok(ExtractedSeries {
        format: HdmFormat::SwmmInp,
        series_name: name.to_string(),
        unit: None,
        resolved_node_index: None,
        distance_to_xy: None,
        stats,
        points,
    })
}

// ---------- SMS .2dm Mesh Summary ----------

fn summarize_2dm(path: &Path) -> Result<HdmSummary, ToolError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ToolError::new("cannot_open_hdm", format!("Failed to read .2dm: {e}")))?;

    let mut nodes = 0usize;
    let mut tris = 0usize;
    let mut quads = 0usize;
    let mut nodestrings = 0usize;
    let mut mats: BTreeSet<i64> = BTreeSet::new();
    let mut xs = Vec::new();
    let mut ys = Vec::new();

    for raw in text.lines() {
        let s = raw.trim();
        let mut p = s.split_whitespace();
        match p.next().unwrap_or("") {
            "ND" => {
                nodes += 1;
                let _id = p.next();
                if let (Some(x), Some(y)) = (
                    p.next().and_then(|v| v.parse::<f64>().ok()),
                    p.next().and_then(|v| v.parse::<f64>().ok()),
                ) {
                    xs.push(x);
                    ys.push(y);
                }
            }
            "E3T" => {
                tris += 1;
                if let Some(m) = p.nth(4).and_then(|v| v.parse::<i64>().ok()) {
                    mats.insert(m);
                }
            }
            "E4Q" => {
                quads += 1;
                if let Some(m) = p.nth(5).and_then(|v| v.parse::<i64>().ok()) {
                    mats.insert(m);
                }
            }
            "NS" if s.split_whitespace().skip(1).any(|tok| tok.starts_with('-')) => {
                nodestrings += 1;
            }
            _ => {}
        }
    }

    if nodes == 0 && tris == 0 && quads == 0 {
        return Err(ToolError::new(
            "cannot_open_hdm",
            "No ND/E3T/E4Q records found in .2dm file.",
        ));
    }

    let mut details = BTreeMap::new();
    details.insert("triangles".to_string(), serde_json::json!(tris));
    details.insert("quads".to_string(), serde_json::json!(quads));
    details.insert("material_count".to_string(), serde_json::json!(mats.len()));
    details.insert(
        "nodestring_count".to_string(),
        serde_json::json!(nodestrings),
    );

    Ok(HdmSummary {
        format: HdmFormat::Sms2dm,
        title: None,
        precision: None,
        node_count: nodes,
        element_count: tris + quads,
        extent: compute_extent_xy(&xs, &ys),
        time_window: None,
        variables: Vec::new(),
        timeseries_names: Vec::new(),
        details,
    })
}

fn compute_extent_xy(xs: &[f64], ys: &[f64]) -> Option<BBox2D> {
    if xs.is_empty() || ys.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (&x, &y) in xs.iter().zip(ys) {
        if x.is_finite() && y.is_finite() {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    min_x.is_finite().then_some(BBox2D {
        min: [round3(min_x), round3(min_y)],
        max: [round3(max_x), round3(max_y)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_record(buf: &mut Vec<u8>, payload: &[u8]) {
        let len = (payload.len() as u32).to_be_bytes();
        buf.extend_from_slice(&len);
        buf.extend_from_slice(payload);
        buf.extend_from_slice(&len);
    }

    fn build_synthetic_selafin() -> Vec<u8> {
        let mut buf = Vec::new();
        let mut title = [b' '; 80];
        title[..12].copy_from_slice(b"DEMO CHANNEL");
        title[72..80].copy_from_slice(b"SERAFIN ");
        write_record(&mut buf, &title);

        let nbv: Vec<u8> = [1i32, 0i32].iter().flat_map(|v| v.to_be_bytes()).collect();
        write_record(&mut buf, &nbv);

        let mut var = [b' '; 32];
        var[..11].copy_from_slice(b"WATER DEPTH");
        var[16..17].copy_from_slice(b"M");
        write_record(&mut buf, &var);

        let iparam: Vec<u8> = [0i32; 10].iter().flat_map(|v| v.to_be_bytes()).collect();
        write_record(&mut buf, &iparam);

        // nelem=1, npoin=3, ndp=3, i1=1
        let dims: Vec<u8> = [1i32, 3, 3, 1]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        write_record(&mut buf, &dims);

        let ikle: Vec<u8> = [1i32, 2, 3].iter().flat_map(|v| v.to_be_bytes()).collect();
        write_record(&mut buf, &ikle);
        let ipobo: Vec<u8> = [1i32, 2, 3].iter().flat_map(|v| v.to_be_bytes()).collect();
        write_record(&mut buf, &ipobo);

        let xs: Vec<u8> = [0.0f32, 10.0, 5.0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let ys: Vec<u8> = [0.0f32, 0.0, 10.0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        write_record(&mut buf, &xs);
        write_record(&mut buf, &ys);

        // Two time steps (t=0.0, t=60.0)
        for (t, vals) in [(0.0f32, [0.5f32, 1.0, 1.5]), (60.0f32, [0.8f32, 1.4, 2.0])] {
            write_record(&mut buf, &t.to_be_bytes());
            let vbytes: Vec<u8> = vals.iter().flat_map(|v| v.to_be_bytes()).collect();
            write_record(&mut buf, &vbytes);
        }
        buf
    }

    #[test]
    fn parses_selafin_summary_and_timeseries() {
        let dir = std::env::temp_dir().join(format!("mcp-hdm-slf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("result.slf");
        std::fs::write(&p, build_synthetic_selafin()).unwrap();

        let sum = summarize_hdm(&p).unwrap();
        assert_eq!(sum.format, HdmFormat::TelemacSelafin);
        assert_eq!(sum.node_count, 3);
        assert_eq!(sum.element_count, 1);
        assert_eq!(sum.time_window.as_ref().unwrap().step_count, 2);

        let ts = query_timeseries(&p, "WATER DEPTH", None, Some([10.1, 0.0])).unwrap();
        assert_eq!(ts.resolved_node_index, Some(1));
        assert_eq!(ts.points.len(), 2);
        assert_eq!(ts.stats.min, Some(1.0));
        assert_eq!(ts.stats.max, Some(1.4));

        // Barycentric line profile across triangle (0,0)-(10,0)-(5,10) at step 1
        // Values at step 1: [0.8, 1.4, 2.0]. Midpoint (5, 0) = 1.1; centroid (5, 3.333) = 1.4
        let prof = query_profiles(
            &p,
            Some("WATER DEPTH"),
            Some(1),
            Some([5.0, 0.0]),
            Some([5.0, 5.0]),
            3,
        )
        .unwrap();
        match prof {
            ProfilePayload::SelafinLine {
                time_step,
                time_seconds,
                hit_count,
                points,
                ..
            } => {
                assert_eq!(time_step, 1);
                assert_eq!(time_seconds, 60.0);
                assert_eq!(hit_count, 3);
                assert_eq!(points[0].value, Some(1.1));
                assert_eq!(points[2].value, Some(1.55));
            }
            _ => panic!("expected SelafinLine profile"),
        }

        let csv_path = dir.join("depth.csv");
        let csv_stats = export_timeseries_csv(&p, "WATER DEPTH", Some(1), None, &csv_path).unwrap();
        assert_eq!(csv_stats.rows_written, 2);
        let csv_text = std::fs::read_to_string(&csv_path).unwrap();
        assert!(csv_text.starts_with("step,time,value\n"));
        assert!(csv_text.contains("0,0,1\n"));
        assert!(csv_text.contains("1,60,1.4\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_swmm_inp_summary_and_timeseries() {
        let dir = std::env::temp_dir().join(format!("mcp-hdm-swmm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let inp = "\
[TITLE]
Synthetic Urban Catchment

[OPTIONS]
FLOW_UNITS CMS
START_DATE 01/01/2025
START_TIME 00:00:00
END_DATE 01/01/2025
END_TIME 06:00:00

[JUNCTIONS]
J1 100.0 2.0
J2 99.0 2.0

[OUTFALLS]
OUT1 98.0 FREE

[CONDUITS]
C1 J1 J2 100.0 0.013
C2 J2 OUT1 100.0 0.013

[COORDINATES]
J1 0.0 0.0
J2 100.0 0.0
OUT1 200.0 0.0

[TIMESERIES]
INFLOW_1 0:00 0.5
INFLOW_1 1:00 2.5
INFLOW_1 2:00 1.0
";
        let p = dir.join("model.inp");
        std::fs::write(&p, inp).unwrap();

        let sum = summarize_hdm(&p).unwrap();
        assert_eq!(sum.format, HdmFormat::SwmmInp);
        assert_eq!(sum.node_count, 3);
        assert_eq!(sum.element_count, 2);
        assert_eq!(sum.timeseries_names, vec!["INFLOW_1".to_string()]);

        let ts = query_timeseries(&p, "INFLOW_1", None, None).unwrap();
        assert_eq!(ts.points.len(), 3);
        assert_eq!(ts.stats.max, Some(2.5));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
