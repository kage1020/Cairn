//! What the model is told, and how a program is read out of its reply.

use eval::conversation::{
    EMPTY, Failure, NO_PROGRAM, PREAMBLE, edit_task, extract_program, feedback, system, task,
};
use eval::spec::SpecPages;

use crate::common::target;

#[test]
fn the_task_names_the_target_and_carries_the_prompt_as_written() {
    assert_eq!(
        task("A shed with {program} in it.", &target()),
        "Write a Cairn program for the build below. It is compiled with `cairn compile --edition \
         java --target 1.21.4 main.crn`. Reply with the whole program in one fenced code block \
         marked `cairn`.\n\nA shed with {program} in it."
    );
}

#[test]
fn an_edit_task_quotes_the_program_it_changes() {
    assert_eq!(
        edit_task("struct a size=3x3\n", "Make it {prompt}.", &target()),
        "Here is a Cairn program, compiled with `cairn compile --edition java --target 1.21.4 \
         main.crn`:\n\n```cairn\nstruct a size=3x3\n```\n\nChange it as asked below and leave the \
         rest of the build as it is. Reply with the whole changed program in one fenced code block \
         marked `cairn`.\n\nMake it {prompt}."
    );
}

#[test]
fn feedback_frames_a_report_with_the_command_that_printed_it() {
    let report = "main.crn:2:3: error[E_X]: wrong\n  note: fix it\n";
    assert_eq!(
        feedback(Failure::Check(report), &target()),
        "`cairn check --edition java --target 1.21.4 main.crn` reports:\n\nmain.crn:2:3: \
         error[E_X]: wrong\n  note: fix it\n\nFix the program and reply with the whole corrected \
         program in one fenced code block marked `cairn`."
    );
    assert_eq!(
        feedback(Failure::Compile(report), &target()),
        "`cairn compile --edition java --target 1.21.4 main.crn` reports:\n\nmain.crn:2:3: \
         error[E_X]: wrong\n  note: fix it\n\nFix the program and reply with the whole corrected \
         program in one fenced code block marked `cairn`."
    );
    assert_eq!(feedback(Failure::NoProgram, &target()), NO_PROGRAM);
    assert_eq!(feedback(Failure::Empty, &target()), EMPTY);
}

#[test]
fn the_program_is_the_last_block_marked_cairn() {
    let reply = "First a sketch.\n\n```cairn\nstruct a size=3x3\n```\n\nNot this:\n\n```text\n\
                 struct b size=3x3\n```\n\nThe program:\n\n~~~~crn title\nstruct c size=3x3\n\
                 ```\n  floor mat_slot=f\n~~~~\n";
    assert_eq!(
        extract_program(reply).as_deref(),
        Some("struct c size=3x3\n```\n  floor mat_slot=f\n")
    );
}

#[test]
fn a_reply_with_no_closed_block_marked_cairn_has_no_program() {
    assert_eq!(extract_program("```\nstruct a size=3x3\n```\n"), None);
    assert_eq!(extract_program("```cairn\nstruct a size=3x3\n"), None);
    assert_eq!(extract_program("No program."), None);
}

#[test]
fn the_system_prompt_carries_every_english_spec_page_and_no_other() {
    let pages = SpecPages::embedded();
    let spec =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../website/src/content/docs/spec");
    let files = std::fs::read_dir(spec)
        .expect("the spec directory reads")
        .filter(|entry| {
            entry
                .as_ref()
                .is_ok_and(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        })
        .count();
    assert_eq!(pages.count(), files);
    assert_eq!(pages.text().matches("<page path=\"/spec/").count(), files);
    assert_eq!(pages.text().matches("<page path=\"/ja/").count(), 0);
    assert!(pages.text().starts_with("<page path=\"/spec/\">\n# "));
    assert_eq!(system(&pages), format!("{PREAMBLE}\n\n{}", pages.text()));
}
