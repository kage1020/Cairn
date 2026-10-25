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
  -f, --format <FORMAT>
          How to print the stage

      --stage <STAGE>
          Which pipeline stage to print

          Possible values:
          - logic:     Logic IR: the edition-neutral DAG
          - netlist:   Netlist IR: Logical Cells over the Logic IR
          - edition:   Edition Netlist IR: Edition Cells against `--edition`
          - placement: Placement IR: coordinates against `--edition`

          [default: logic]

      --edition <EDITION>
          Target edition. Required when `--stage edition` or `--stage placement` is
          set; refused for `logic` and `netlist`

          Possible values:
          - java:    Java Edition
          - bedrock: Bedrock Edition

  -h, --help
          Print help (see a summary with '-h')
";

const STAGES: &[&str] = &["logic", "netlist", "edition", "placement"];

const TAGGED: &[&str] = &["edition", "placement"];

fn edit(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "{from:?} is not in the fixture");
    text.replacen(from, to, 1)
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn help_fault(help: &str) -> String {
    synth_help(help).expect_err("the help is refused")
}

#[test]
fn synth_help_gives_the_stages_and_those_edition_is_required_for() {
    assert_eq!(synth_help(SYNTH_HELP), Ok((owned(STAGES), owned(TAGGED))));
}

#[test]
fn a_wrapped_value_help_does_not_end_the_values() {
    let help = edit(
        SYNTH_HELP,
        "Logical Cells over the Logic IR\n",
        "Logical Cells over\n                       the Logic IR\n",
    );
    assert_eq!(synth_help(&help), Ok((owned(STAGES), owned(TAGGED))));
}

#[test]
fn a_stage_option_with_a_short_flag_is_read() {
    let help = edit(SYNTH_HELP, "      --stage <STAGE>", "  -s, --stage <STAGE>");
    assert_eq!(synth_help(&help), Ok((owned(STAGES), owned(TAGGED))));
}

#[test]
fn an_option_block_ends_at_the_next_option() {
    let format = option_help(SYNTH_HELP, "--format").expect("`--format` is listed");
    assert_eq!(
        possible_values("--format", &format),
        Err(
            "`cairn synth --help` lists no values under `--format`; restore its \
             `Possible values:` list in `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
                .to_owned()
        )
    );
}

#[test]
fn a_help_without_a_stage_option_is_reported() {
    let help = edit(SYNTH_HELP, "--stage <STAGE>", "--phase <PHASE>");
    assert_eq!(
        help_fault(&help),
        "`cairn synth --help` lists no `--stage` option; restore it in \
         `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

#[test]
fn stage_values_printed_inline_are_reported() {
    let help = "\
      --stage <STAGE>
          Which pipeline stage to print

          [default: logic]
          [possible values: logic, netlist, edition, placement]
";
    assert_eq!(
        help_fault(help),
        "`cairn synth --help` prints `--stage`'s values inline, as `[possible values: …]`, which \
         clap does when no value has help; give each value its help again in \
         `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

#[test]
fn a_stage_option_without_possible_values_is_reported() {
    let help = edit(SYNTH_HELP, "Possible values:", "Stages:");
    assert_eq!(
        help_fault(&help),
        "`cairn synth --help` lists no values under `--stage`; restore its `Possible values:` \
         list in `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

#[test]
fn an_empty_possible_values_list_is_reported() {
    let help = edit(
        SYNTH_HELP,
        "          - logic:     Logic IR: the edition-neutral DAG
          - netlist:   Netlist IR: Logical Cells over the Logic IR
          - edition:   Edition Netlist IR: Edition Cells against `--edition`
          - placement: Placement IR: coordinates against `--edition`
",
        "",
    );
    assert_eq!(
        help_fault(&help),
        "`cairn synth --help` lists no values under `--stage`; restore its `Possible values:` \
         list in `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

#[test]
fn a_possible_values_line_of_another_shape_is_reported() {
    let help = edit(SYNTH_HELP, "          - netlist:", "          * netlist:");
    assert_eq!(
        help_fault(&help),
        "`cairn synth --help` has \"* netlist:   Netlist IR: Logical Cells over the Logic IR\" \
         among `--stage`'s possible values, which is neither a `- <value>: <help>` line nor one \
         wrapped from it; restore that shape in `crates/cairn-lang-cli/src/cli.rs`, or update \
         this rule"
    );
}

#[test]
fn a_help_without_an_edition_option_is_reported() {
    let help = edit(SYNTH_HELP, "--edition <EDITION>", "--target <TARGET>");
    assert_eq!(
        help_fault(&help),
        "`cairn synth --help` lists no `--edition` option; restore it in \
         `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

#[test]
fn edition_help_without_its_required_when_sentence_is_reported() {
    let help = edit(SYNTH_HELP, "Required when", "Needed when");
    assert_eq!(
        help_fault(&help),
        "`cairn synth --help` has no \"Required when … is set\" under `--edition` to read the \
         stages that require it from; restore that sentence in \
         `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

#[test]
fn a_required_when_span_that_is_not_a_stage_is_reported() {
    let help = edit(SYNTH_HELP, "`--stage placement` is\n", "`placement` is\n");
    assert_eq!(
        help_fault(&help),
        "`--edition`'s \"Required when … is set\" in `cairn synth --help` names `placement`, \
         which is not a `--stage <stage>`; write each stage that requires `--edition` as one in \
         `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

#[test]
fn a_required_when_sentence_naming_no_stage_is_reported() {
    let help = edit(
        SYNTH_HELP,
        "`--stage edition` or `--stage placement` is\n",
        "a later stage is\n",
    );
    assert_eq!(
        help_fault(&help),
        "`--edition`'s \"Required when … is set\" in `cairn synth --help` names no \
         `--stage <stage>`; name each stage that requires `--edition` as one in \
         `crates/cairn-lang-cli/src/cli.rs`, or update this rule"
    );
}

const SYNTH_README: &str = "\
# cairn-lang-cli

## `cairn synth` (experimental)

`cairn synth --stage <s>` prints one stage: `logic`, `netlist`, `edition`, or `placement`. \
`--edition` is required from `edition` onwards and refused before it.

## License
";

fn synth_findings(text: &str, stages: &[&str], tagged: &[&str]) -> Vec<String> {
    synth_section(Path::new("README.md"), text, &owned(stages), &owned(tagged))
        .iter()
        .map(|finding| finding.render(SYNTH_STAGES.name))
        .collect()
}

#[test]
fn a_synth_section_listing_every_stage_and_the_first_tagged_one_passes() {
    assert_eq!(
        synth_findings(SYNTH_README, STAGES, TAGGED),
        Vec::<String>::new()
    );
}

#[test]
fn a_stage_the_list_leaves_out_is_reported() {
    let text = edit(SYNTH_README, "`edition`, or `placement`", "or `edition`");
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md:5: readme-synth-stages: `--stage placement` ships but the `cairn synth` \
             section does not list it among its stages; add `placement` to that list",
        ]
    );
}

#[test]
fn a_stage_left_out_of_the_list_but_named_later_is_reported() {
    let text = edit(
        SYNTH_README,
        "`edition`, or `placement`.",
        "or `edition`. `placement` also fills `buffer_coords`.",
    );
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md:5: readme-synth-stages: `--stage placement` ships but the `cairn synth` \
             section does not list it among its stages; add `placement` to that list",
        ]
    );
}

#[test]
fn a_stage_named_only_in_the_edition_sentence_is_not_listed() {
    let text = edit(SYNTH_README, "`netlist`, `edition`, or", "`netlist`, or");
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md:5: readme-synth-stages: `--stage edition` ships but the `cairn synth` \
             section does not list it among its stages; add `edition` to that list",
        ]
    );
}

#[test]
fn a_name_in_the_list_that_is_not_a_stage_is_reported() {
    let text = edit(SYNTH_README, "`edition`, or", "`edition`, `merge`, or");
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md:5: readme-synth-stages: `merge` is in the section's list of stages but is \
             not a `--stage` value; `cairn synth --help` lists `logic`, `netlist`, `edition`, \
             `placement`, so drop it or name one of those",
        ]
    );
}

#[test]
fn a_stage_spelled_with_a_hyphen_is_one_stage() {
    let text = SYNTH_README.replace("`edition`", "`edition-netlist`");
    assert_eq!(
        synth_findings(
            &text,
            &["logic", "netlist", "edition-netlist", "placement"],
            &["edition-netlist", "placement"]
        ),
        Vec::<String>::new()
    );
}

#[test]
fn stages_listed_out_of_order_are_reported() {
    let text = edit(SYNTH_README, "`logic`, `netlist`", "`netlist`, `logic`");
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md:5: readme-synth-stages: the section lists the stages as `netlist`, \
             `logic`, `edition`, `placement`, but `--stage` offers them as `logic`, `netlist`, \
             `edition`, `placement`, the order \"from … onwards\" reads against; list them in \
             that order",
        ]
    );
}

#[test]
fn a_stage_named_above_the_list_does_not_reorder_it() {
    let text = edit(
        SYNTH_README,
        "\n`cairn synth --stage",
        "\nStages up to `edition` are stable enough to debug with.\n\n`cairn synth --stage",
    );
    assert_eq!(synth_findings(&text, STAGES, TAGGED), Vec::<String>::new());
}

#[test]
fn the_run_naming_the_most_stages_is_the_list() {
    let text = edit(
        SYNTH_README,
        "\n`cairn synth --stage",
        "\n`logic`, `netlist` and their inputs need no edition.\n\n`cairn synth --stage",
    );
    assert_eq!(synth_findings(&text, STAGES, TAGGED), Vec::<String>::new());
}

#[test]
fn the_editions_in_parentheses_are_not_stages() {
    let text = edit(
        SYNTH_README,
        "refused before it.",
        "refused before it. Pass `--edition` (`java` or `bedrock`) with them.",
    );
    assert_eq!(synth_findings(&text, STAGES, TAGGED), Vec::<String>::new());
}

#[test]
fn a_section_without_a_list_of_stages_is_reported() {
    let text = edit(
        SYNTH_README,
        "`logic`, `netlist`, `edition`, or `placement`",
        "a stage",
    );
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md: readme-synth-stages: the section has no comma-separated list of the \
             stages; list them as `cairn synth --help` does: `logic`, `netlist`, `edition`, \
             `placement`",
        ]
    );
}

#[test]
fn from_onwards_naming_another_stage_than_the_first_tagged_one_is_reported() {
    assert_eq!(
        synth_findings(SYNTH_README, STAGES, &["placement"]),
        [
            "README.md:5: readme-synth-stages: \"from `edition` onwards\" names `edition`, but \
             the first stage `cairn synth --help` requires `--edition` for is `placement`; write \
             \"from `placement` onwards\"",
        ]
    );
}

#[test]
fn another_field_filled_from_a_stage_onwards_is_not_the_edition_sentence() {
    let text = edit(
        SYNTH_README,
        "refused before it.",
        "refused before it. `wire_length` is filled from `placement` onwards.",
    );
    assert_eq!(synth_findings(&text, STAGES, TAGGED), Vec::<String>::new());
}

#[test]
fn a_section_not_saying_where_edition_starts_is_reported() {
    let text = edit(
        SYNTH_README,
        " from `edition` onwards and refused before it",
        "",
    );
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md: readme-synth-stages: the section does not say which stage `--edition` \
             starts at; write \"`--edition` is required from `edition` onwards\"",
        ]
    );
}

#[test]
fn an_edition_sentence_reworded_away_from_required_from_is_reported() {
    let text = edit(
        SYNTH_README,
        "`--edition` is required from `edition` onwards and refused before it",
        "`--edition` is refused from `edition` onwards and required before it",
    );
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md: readme-synth-stages: the section does not say which stage `--edition` \
             starts at; write \"`--edition` is required from `edition` onwards\"",
        ]
    );
}

#[test]
fn tagged_stages_that_do_not_run_to_the_last_stage_are_reported() {
    assert_eq!(
        synth_findings(SYNTH_README, STAGES, &["edition"]),
        [
            "README.md: readme-synth-stages: the stages `cairn synth --help` requires \
             `--edition` for, `edition`, do not run to the end of `--stage`'s order (`logic`, \
             `netlist`, `edition`, `placement`), so \"from … onwards\" cannot describe them; \
             reword the section, and this rule with it",
        ]
    );
}

#[test]
fn tagged_stages_in_another_order_than_stage_are_reported() {
    assert_eq!(
        synth_findings(SYNTH_README, STAGES, &["placement", "edition"]),
        [
            "README.md: readme-synth-stages: `cairn synth --help` names the stages that require \
             `--edition` as `placement`, `edition`, but `--stage` orders them `edition`, \
             `placement`; name them in that order in `crates/cairn-lang-cli/src/cli.rs`",
        ]
    );
}

#[test]
fn a_readme_without_a_synth_section_is_reported() {
    assert_eq!(
        synth_findings("# cairn-lang-cli\n\n## Subcommands\n", STAGES, TAGGED),
        ["README.md: readme-synth-stages: no `##` section whose title starts with `cairn synth`"]
    );
}

#[test]
fn an_unpaired_backtick_is_reported_alone() {
    let text = edit(SYNTH_README, "or `placement`.", "or `placement.");
    assert_eq!(
        synth_findings(&text, STAGES, TAGGED),
        [
            "README.md:5: readme-synth-stages: unpaired backtick; close the code span on this \
             line, since the stages are read from this section's code spans",
        ]
    );
}
