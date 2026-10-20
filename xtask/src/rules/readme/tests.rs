use super::*;

fn exported(source: &str) -> Vec<Result<String, String>> {
    let file = syn::parse_file(source).unwrap();
    let mut paths = Vec::new();
    for item in file.items {
        if let syn::Item::Use(item) = item
            && matches!(item.vis, Visibility::Public(_))
        {
            expand_use(&item.tree, &mut Vec::new(), &mut paths);
        }
    }
    paths
}

#[test]
fn re_exports_expand_to_full_paths() {
    let source = "\
pub use tag::{Compound, List};
pub use crate::registry::{load::PackSource, PackEdition};
pub use self::bedrock::write;
pub(crate) use hidden::Item;
use private::Item;
";
    let paths: Vec<_> = exported(source).into_iter().map(Result::unwrap).collect();
    assert_eq!(
        paths,
        [
            "tag::Compound",
            "tag::List",
            "registry::load::PackSource",
            "registry::PackEdition",
            "bedrock::write",
        ]
    );
}

#[test]
fn shapes_a_row_cannot_name_are_refused() {
    for source in [
        "pub use tag::*;",
        "pub use tag::Tag as Tree;",
        "pub use registry::{self, PackEdition};",
        "pub use crate::Tag;",
    ] {
        assert!(
            exported(source).iter().any(Result::is_err),
            "{source} was accepted"
        );
    }
}

#[test]
fn clap_help_lists_its_subcommands() {
    let help = "Usage: cairn <COMMAND>\n\nCommands:\n  parse  Lex and parse\n  check  Run checks\n  help   Print this message\n\nOptions:\n  -h, --help\n";
    let names: Vec<_> = listed_subcommands(help).unwrap().into_iter().collect();
    assert_eq!(names, ["check", "parse"]);
}

#[test]
fn the_help_comes_back_plain_when_the_environment_forces_colour() {
    let repo = Repo::open(None, None).expect("open the checkout");
    let output = help_command(&repo, &[])
        .env("CLICOLOR_FORCE", "1")
        .output()
        .expect("run cargo");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8(output.stdout).expect("help is utf-8");
    assert_eq!(help.find('\x1b'), None, "{help:?}");
}

#[test]
fn a_subcommand_row_names_a_cairn_invocation() {
    assert_eq!(subcommand_of("cairn check <file.crn>"), Some("check"));
    assert_eq!(subcommand_of("carn lint"), None);
    assert_eq!(subcommand_of("frob <file.crn>"), None);
    assert_eq!(subcommand_of("cairn"), None);
}

const SYNTH_HELP: &str = "\
Usage: cairn synth [OPTIONS] <FILE>

Options:
      --stage <STAGE>
          Which pipeline stage to print

          Possible values:
          - logic:     Edition-neutral Logic IR DAG
          - netlist:   Netlist IR
          - edition:   Edition Netlist IR against `--edition`
          - placement: Placement IR against `--edition`

          [default: logic]

      --edition <EDITION>
          Target edition. Required when `--stage edition` or `--stage placement` is set; refused \
for `logic` and `netlist`

          Possible values:
          - java:    Java Edition
          - bedrock: Bedrock Edition

  -h, --help
          Print help (see a summary with '-h')
";

#[test]
fn synth_help_lists_the_stages_and_those_edition_is_required_for() {
    let stage = option_help(SYNTH_HELP, "--stage").unwrap();
    assert_eq!(
        stage,
        [
            "Which pipeline stage to print",
            "",
            "Possible values:",
            "- logic:     Edition-neutral Logic IR DAG",
            "- netlist:   Netlist IR",
            "- edition:   Edition Netlist IR against `--edition`",
            "- placement: Placement IR against `--edition`",
            "",
            "[default: logic]",
            "",
        ]
    );
    assert_eq!(
        possible_values(&stage),
        ["logic", "netlist", "edition", "placement"]
    );
    let edition = option_help(SYNTH_HELP, "--edition").unwrap();
    assert_eq!(possible_values(&edition), ["java", "bedrock"]);
    assert_eq!(
        edition_tagged(&edition),
        Some(vec!["edition".to_owned(), "placement".to_owned()])
    );
    assert_eq!(option_help(SYNTH_HELP, "--format"), None);
}

#[test]
fn edition_help_without_a_stage_list_names_no_tagged_stage() {
    for help in [
        "Target edition, for the later stages",
        "Required when `--stage edition` or `placement` is set",
        "Required when no stage is set",
    ] {
        assert_eq!(edition_tagged(&[help]), None, "{help}");
    }
}

const SYNTH_README: &str = "\
# cairn-lang-cli

## `cairn synth` (experimental)

`cairn synth --stage <s>` prints one stage: `logic`, `netlist`, `edition`, or `placement`. \
`--edition` is required from `edition` onwards and refused before it.

## License
";

const TAGGED: &[&str] = &["edition", "placement"];

fn synth_findings(text: &str, tagged: &[&str]) -> Vec<String> {
    let stages = ["logic", "netlist", "edition", "placement"].map(str::to_owned);
    let tagged: Vec<String> = tagged.iter().map(|stage| (*stage).to_owned()).collect();
    synth_section(Path::new("README.md"), text, &stages, &tagged)
        .iter()
        .map(|finding| finding.render(SYNTH_STAGES.name))
        .collect()
}

#[test]
fn a_synth_section_naming_every_stage_and_the_first_tagged_one_passes() {
    assert_eq!(synth_findings(SYNTH_README, TAGGED), Vec::<String>::new());
}

#[test]
fn a_stage_the_synth_section_leaves_out_or_invents_is_reported() {
    let text = SYNTH_README.replace("`edition`, or `placement`", "`edition`, or `merge`");
    assert_eq!(
        synth_findings(&text, TAGGED),
        [
            "README.md: readme-synth-stages: `--stage placement` ships but the `cairn synth` \
             section does not name it; add `placement` to its list of stages",
            "README.md:5: readme-synth-stages: `merge` is not a `--stage` value; a one-word \
             code span in this section names a stage, and `cairn synth --help` lists `logic`, \
             `netlist`, `edition`, `placement`",
        ]
    );
}

#[test]
fn a_stage_named_only_in_from_onwards_is_not_listed() {
    let text = SYNTH_README.replace("`netlist`, `edition`, or", "`netlist`, or");
    assert_eq!(
        synth_findings(&text, TAGGED),
        [
            "README.md: readme-synth-stages: `--stage edition` ships but the `cairn synth` \
             section does not name it; add `edition` to its list of stages",
        ]
    );
}

#[test]
fn stages_listed_out_of_order_are_reported() {
    let text = SYNTH_README.replace("`logic`, `netlist`", "`netlist`, `logic`");
    assert_eq!(
        synth_findings(&text, TAGGED),
        [
            "README.md: readme-synth-stages: the section lists the stages as `netlist`, `logic`, \
             `edition`, `placement`, but `--stage` offers them as `logic`, `netlist`, `edition`, \
             `placement`, the order \"from … onwards\" reads against; list them in that order",
        ]
    );
}

#[test]
fn from_onwards_naming_another_stage_than_the_first_tagged_one_is_reported() {
    assert_eq!(
        synth_findings(SYNTH_README, &["placement"]),
        [
            "README.md:5: readme-synth-stages: \"from `edition` onwards\" names `edition`, but \
             the first stage `cairn synth --help` requires `--edition` for is `placement`; write \
             \"from `placement` onwards\"",
        ]
    );
    let text = SYNTH_README.replace(" from `edition` onwards and refused before it", "");
    assert_eq!(
        synth_findings(&text, TAGGED),
        [
            "README.md: readme-synth-stages: the section does not say which stage `--edition` \
             starts at; say it is required from `edition` onwards",
        ]
    );
}

#[test]
fn tagged_stages_that_do_not_run_to_the_last_stage_are_reported() {
    assert_eq!(
        synth_findings(SYNTH_README, &["edition"]),
        [
            "README.md: readme-synth-stages: `cairn synth --help` requires `--edition` for \
             `edition`, which are not the last stages in `--stage`'s order (`logic`, \
             `netlist`, `edition`, `placement`), so \"from … onwards\" cannot describe them; \
             reword the section, and this rule with it",
        ]
    );
}

#[test]
fn a_readme_without_a_synth_section_or_with_an_unpaired_backtick_is_reported() {
    assert_eq!(
        synth_findings("# cairn-lang-cli\n\n## Subcommands\n", TAGGED),
        ["README.md: readme-synth-stages: no `##` section whose title starts with `cairn synth`"]
    );
    let text = SYNTH_README.replace(
        "\n\n## License",
        "\n\nSee `cairn synth --help.\n\n## License",
    );
    assert_eq!(
        synth_findings(&text, TAGGED),
        [
            "README.md:7: readme-synth-stages: unpaired backtick; the stages are read from this \
             section's code spans",
        ]
    );
}
