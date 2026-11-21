//! Which commands report `W_EDIT_RIPPLE`: each sees the ripples its own lowering derives
//! (`spec/components-editing-sites` "Editing model").

use crate::common::{Fixture, Reported, cairn_argv, reported, stderr_of, stderr_stream};

/// A struct of walls under a flat roof of `overhang`, then `members` and `edit`.
fn source(overhang: u32, members: &str, edit: &str) -> String {
    format!(
        "@cairn 2026.06\n@requires version>=1.20\n\n\
         theme t:\n  slot wall -> @cobblestone\n\n\
         struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang={overhang}\n{members}\n{edit}\n"
    )
}

/// Each command, with the arguments after its source path; `OUT` stands for an output directory.
const COMMANDS: [(&str, &[&str]); 7] = [
    ("check", &[]),
    ("check", &["--edition", "java", "--target", "1.21.4"]),
    (
        "compile",
        &["--edition", "java", "--target", "1.21.4", "--out", OUT],
    ),
    ("lower", &[]),
    ("lower", &["--edition", "java"]),
    ("info", &[]),
    (
        "synth",
        &[
            "--experimental-logic-synth",
            "--stage",
            "edition",
            "--edition",
            "java",
        ],
    ),
];

const OUT: &str = "OUT";

/// What each of [`COMMANDS`] run on `source` reports, each run exiting 0.
fn reported_by_each(source: &str) -> Vec<(String, Vec<Reported>)> {
    COMMANDS
        .iter()
        .map(|(command, args)| {
            let fixture = Fixture::new(source);
            let mut argv = vec![
                (*command).to_owned(),
                fixture.source().display().to_string(),
            ];
            argv.extend(args.iter().map(|arg| {
                if *arg == OUT {
                    fixture.out().display().to_string()
                } else {
                    (*arg).to_owned()
                }
            }));
            let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
            let out = cairn_argv(&argv);
            let stderr = stderr_of(&out);
            assert_eq!(out.status.code(), Some(0), "{argv:?}: {stderr}");
            (
                format!("{command} {}", args.join(" ")),
                stderr_stream(&stderr),
            )
        })
        .collect()
}

/// [`COMMANDS`] paired with what each should report: `ripple` for those `sees` keeps.
fn expected(
    ripple: (u64, u64),
    sees: impl Fn(&str, &[&str]) -> bool,
) -> Vec<(String, Vec<Reported>)> {
    let (line, col) = ripple;
    COMMANDS
        .iter()
        .map(|(command, args)| {
            let found = if sees(command, args) {
                reported(&[(line, col, "warning", "W_EDIT_RIPPLE")])
            } else {
                Vec::new()
            };
            (format!("{command} {}", args.join(" ")), found)
        })
        .collect()
}

#[test]
fn a_ripple_through_a_connection_is_seen_by_every_command_that_reads_a_pack() {
    let source = source(
        1,
        "  chest at=inside.back offset=1 y=1\n  chest at=inside.back offset=2 y=1\n",
        "edit chest@inside.back[1] set offset=4",
    );
    assert_eq!(
        reported_by_each(&source),
        expected((10, 3), |command, args| !(command == "check"
            && args.is_empty())),
    );
}

#[test]
fn a_ripple_in_where_blocks_stand_is_seen_by_every_command() {
    // The walls an edit makes taller lift the roof on them.
    let source = source(0, "  floor mat_slot=wall\n", "edit walls[0] set height=4");
    assert_eq!(reported_by_each(&source), expected((9, 3), |_, _| true));
}
