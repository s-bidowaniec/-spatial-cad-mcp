use mcp_dxf::edit::{DxfEditOp, DxfSelect, edit_dxf_entities};
use mcp_dxf::parser::collect_pairs;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn sample_dxf() -> &'static str {
    "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1015\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  5\n1A\n  8\nWALLS\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n0.0\n  0\nLINE\n  5\n1B\n  8\nAXIS\n 10\n50.0\n 20\n50.0\n 11\n60.0\n 21\n50.0\n  0\nCIRCLE\n  5\n1C\n  8\nWALLS\n 10\n5.0\n 20\n5.0\n 40\n2.0\n  0\nENDSEC\n  0\nEOF\n"
}

fn file_hash(path: &Path) -> u64 {
    let bytes = fs::read(path).expect("read file for hash");
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn make_temp_workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp_dxf_task01_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn entities_of(path: &Path) -> Vec<Vec<(i32, String)>> {
    let content = fs::read_to_string(path).unwrap();
    let pairs = collect_pairs(&content).unwrap();
    let owned: Vec<(i32, String)> = pairs.into_iter().map(|(c, v)| (c, v.to_string())).collect();
    let mut out: Vec<Vec<(i32, String)>> = Vec::new();
    let mut in_entities = false;
    let mut i = 0usize;
    while i < owned.len() {
        let (c, v) = &owned[i];
        if *c == 0 && v == "SECTION" {
            in_entities = owned
                .get(i + 1)
                .is_some_and(|(c2, v2)| *c2 == 2 && v2 == "ENTITIES");
            i += 2;
            continue;
        }
        if *c == 0 && v == "ENDSEC" {
            in_entities = false;
            i += 1;
            continue;
        }
        if in_entities && *c == 0 && v != "EOF" {
            let mut end = i + 1;
            while end < owned.len() && owned[end].0 != 0 {
                end += 1;
            }
            out.push(owned[i..end].to_vec());
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

fn group_value(ent: &[(i32, String)], code: i32) -> Option<&str> {
    ent.iter()
        .find(|(c, _)| *c == code)
        .map(|(_, v)| v.as_str())
}

#[test]
fn happy_path_select_layer_move_and_set_color() {
    let ws = make_temp_workspace("happy");
    let input_path = ws.join("input.dxf");
    fs::write(&input_path, sample_dxf()).unwrap();
    let before_hash = file_hash(&input_path);

    let out_path = ws.join("outputs/edited_walls.dxf");
    let (stats, clamped) = edit_dxf_entities(
        &input_path,
        &out_path,
        &DxfSelect {
            layer: Some("WALLS".into()),
            ..Default::default()
        },
        &[
            DxfEditOp::Move {
                dx: 100.0,
                dy: -20.0,
            },
            DxfEditOp::SetColor { color: 3 },
        ],
        None,
    )
    .unwrap();

    assert_eq!(stats.selected, 2);
    assert_eq!(stats.edited, 2);
    assert_eq!(stats.deleted, 0);
    assert_eq!(stats.per_op.get("move"), Some(&2));
    assert_eq!(stats.per_op.get("set_color"), Some(&2));
    assert_eq!(clamped, None);
    assert_eq!(before_hash, file_hash(&input_path), "input must not change");

    let ents = entities_of(&out_path);
    assert_eq!(ents.len(), 3);
    assert_eq!(group_value(&ents[0], 62), Some("3"));
    assert_eq!(group_value(&ents[0], 10), Some("100.000000"));
    assert_eq!(group_value(&ents[0], 20), Some("-20.000000"));
    assert_eq!(
        group_value(&ents[1], 10),
        Some("50.0"),
        "untouched entity kept verbatim"
    );
    assert_eq!(stats.entity_type_counts_before.get("LINE"), Some(&2));
    assert_eq!(stats.entity_type_counts_after.get("CIRCLE"), Some(&1));
}

#[test]
fn delete_op_and_untouched_entities_identical() {
    let ws = make_temp_workspace("delete");
    let input_path = ws.join("input.dxf");
    fs::write(&input_path, sample_dxf()).unwrap();

    let out_path = ws.join("outputs/after_delete.dxf");
    let (stats, _) = edit_dxf_entities(
        &input_path,
        &out_path,
        &DxfSelect {
            entity_type: Some("CIRCLE".into()),
            ..Default::default()
        },
        &[DxfEditOp::Delete],
        None,
    )
    .unwrap();
    assert_eq!(stats.selected, 1);
    assert_eq!(stats.deleted, 1);
    assert_eq!(stats.edited, 0);

    let orig = entities_of(&input_path);
    let out = entities_of(&out_path);
    assert_eq!(out.len(), 2);
    assert_eq!(orig[0], out[0]);
    assert_eq!(orig[1], out[1]);
}

#[test]
fn rejection_paths() {
    let ws = make_temp_workspace("reject");
    let input_path = ws.join("input.dxf");
    fs::write(&input_path, sample_dxf()).unwrap();
    let out_path = ws.join("outputs/out.dxf");

    let err =
        edit_dxf_entities(&input_path, &out_path, &DxfSelect::default(), &[], None).unwrap_err();
    assert_eq!(err.code, "invalid_input");

    let err = edit_dxf_entities(
        &input_path,
        &out_path,
        &DxfSelect {
            bbox: Some([10.0, 0.0, 5.0, 10.0]),
            ..Default::default()
        },
        &[DxfEditOp::Delete],
        None,
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");

    let err = edit_dxf_entities(
        &input_path,
        &out_path,
        &DxfSelect {
            handle_from: Some("zz".into()),
            ..Default::default()
        },
        &[DxfEditOp::Delete],
        None,
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");

    let err = edit_dxf_entities(
        &ws.join("missing.dxf"),
        &out_path,
        &DxfSelect::default(),
        &[DxfEditOp::Delete],
        None,
    )
    .unwrap_err();
    assert_eq!(err.code, "cannot_open_dxf");

    let err = edit_dxf_entities(
        &input_path,
        &out_path,
        &DxfSelect::default(),
        &[DxfEditOp::SetColor { color: 999 }],
        None,
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");
}

#[test]
fn limit_is_clamped_and_determinism_holds() {
    let ws = make_temp_workspace("determinism");
    let input_path = ws.join("input.dxf");
    fs::write(&input_path, sample_dxf()).unwrap();

    let (stats, clamped) = edit_dxf_entities(
        &input_path,
        &ws.join("outputs/run1.dxf"),
        &DxfSelect {
            handle_from: Some("1A".into()),
            handle_to: Some("1B".into()),
            ..Default::default()
        },
        &[
            DxfEditOp::Rotate {
                cx: 0.0,
                cy: 0.0,
                angle_deg: 90.0,
            },
            DxfEditOp::Scale {
                cx: 0.0,
                cy: 0.0,
                factor: 2.0,
            },
        ],
        Some(500),
    )
    .unwrap();
    assert_eq!(stats.selected, 2);
    assert_eq!(clamped, Some(mcp_dxf::edit::MAX_EDIT_LIMIT));

    edit_dxf_entities(
        &input_path,
        &ws.join("outputs/run2.dxf"),
        &DxfSelect {
            handle_from: Some("1A".into()),
            handle_to: Some("1B".into()),
            ..Default::default()
        },
        &[
            DxfEditOp::Rotate {
                cx: 0.0,
                cy: 0.0,
                angle_deg: 90.0,
            },
            DxfEditOp::Scale {
                cx: 0.0,
                cy: 0.0,
                factor: 2.0,
            },
        ],
        Some(500),
    )
    .unwrap();
    assert_eq!(
        fs::read(ws.join("outputs/run1.dxf")).unwrap(),
        fs::read(ws.join("outputs/run2.dxf")).unwrap()
    );
}
