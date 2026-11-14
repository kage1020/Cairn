//! What the harness says to a model, and how it reads a program back out of a reply.
//!
//! Every word the model is told besides the specification and the prompts is one of the templates
//! here, and [`INSTRUCTIONS`] lists them so that a run record's `prompt_set` hash covers them.

use crate::spec::SpecPages;

/// The file each program is checked and compiled as.
pub const SOURCE: &str = "main.crn";

/// The system prompt, before the specification pages.
pub const PREAMBLE: &str = "You write programs in Cairn, a language that describes Minecraft \
builds and compiles them to structure files. The Cairn specification follows, one page per \
<page> element. It is the only reference you have: write what it specifies.";

/// The first message of a conversation that builds what a prompt asks for.
pub const TASK: &str = "Write a Cairn program for the build below. It is compiled with \
`cairn compile --edition {edition} --target {target} main.crn`. Reply with the whole program in \
one fenced code block marked `cairn`.

{prompt}";

/// The first message of a conversation that edits a converged program.
pub const EDIT_TASK: &str = "Here is a Cairn program, compiled with \
`cairn compile --edition {edition} --target {target} main.crn`:

```cairn
{program}
```

Change it as asked below and leave the rest of the build as it is. Reply with the whole changed \
program in one fenced code block marked `cairn`.

{prompt}";

/// The reply to an attempt whose program `cairn check` or `cairn compile` refused.
pub const REPORT: &str = "`{command}` reports:

{report}

Fix the program and reply with the whole corrected program in one fenced code block marked \
`cairn`.";

/// The reply to an attempt that holds no program.
pub const NO_PROGRAM: &str = "Your reply has no fenced code block marked `cairn`. Reply with the \
whole program in one fenced code block marked `cairn`.";

/// The reply to an attempt whose program builds nothing.
pub const EMPTY: &str = "`main.crn` checks and compiles clean, but it builds no block. Reply with \
the whole program, building what was asked, in one fenced code block marked `cairn`.";

/// Every template, in the order a run record's `prompt_set` hash takes them.
pub const INSTRUCTIONS: [&str; 6] = [PREAMBLE, TASK, EDIT_TASK, REPORT, NO_PROGRAM, EMPTY];

/// The `(edition, version)` pair a run checks and compiles every program at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `java` or `bedrock`.
    pub edition: String,
    /// The version label, as `--target` takes it.
    pub version: String,
}

/// Why an attempt did not pass, as the next message tells the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure<'a> {
    /// The reply holds no program.
    NoProgram,
    /// `cairn check` refused the program, printing this.
    Check(&'a str),
    /// `cairn compile` refused the program `cairn check` passed, printing this.
    Compile(&'a str),
    /// The program builds no block.
    Empty,
}

/// `template` with each `{name}` of `values` replaced in one pass, so a value is never rescanned.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let named = after.find('}').and_then(|close| {
            values
                .iter()
                .find(|(name, _)| *name == &after[..close])
                .map(|(_, value)| (close, *value))
        });
        if let Some((close, value)) = named {
            out.push_str(value);
            rest = &after[close + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The system prompt: [`PREAMBLE`], then the specification pages.
#[must_use]
pub fn system(pages: &SpecPages) -> String {
    format!("{PREAMBLE}\n\n{}", pages.text())
}

/// The first message of a conversation asking for `prompt`.
#[must_use]
pub fn task(prompt: &str, target: &Target) -> String {
    fill(
        TASK,
        &[
            ("edition", &target.edition),
            ("target", &target.version),
            ("prompt", prompt),
        ],
    )
}

/// The first message of a conversation asking for `prompt` as a change to `program`.
#[must_use]
pub fn edit_task(program: &str, prompt: &str, target: &Target) -> String {
    fill(
        EDIT_TASK,
        &[
            ("edition", &target.edition),
            ("target", &target.version),
            ("program", program.trim_end()),
            ("prompt", prompt),
        ],
    )
}

/// The message that tells the model why its attempt did not pass.
#[must_use]
pub fn feedback(failure: Failure<'_>, target: &Target) -> String {
    let report = |command: &str, report: &str| {
        let command = format!(
            "cairn {command} --edition {} --target {} {SOURCE}",
            target.edition, target.version
        );
        fill(
            REPORT,
            &[("command", &command), ("report", report.trim_end())],
        )
    };
    match failure {
        Failure::NoProgram => NO_PROGRAM.to_owned(),
        Failure::Check(printed) => report("check", printed),
        Failure::Compile(printed) => report("compile", printed),
        Failure::Empty => EMPTY.to_owned(),
    }
}

/// A fenced code block not closed yet.
struct Open<'a> {
    mark: char,
    run: usize,
    marked: bool,
    lines: Vec<&'a str>,
}

/// The fence character and its run length, when `line` opens or closes a fenced code block.
fn fence(line: &str) -> Option<(char, usize)> {
    let mark = line
        .chars()
        .next()
        .filter(|mark| *mark == '`' || *mark == '~')?;
    let run = line.chars().take_while(|c| *c == mark).count();
    (run >= 3).then_some((mark, run))
}

/// The program a reply holds: its last fenced code block marked `cairn` or `crn`.
#[must_use]
pub fn extract_program(reply: &str) -> Option<String> {
    let mut found = None;
    let mut open: Option<Open<'_>> = None;
    for line in reply.lines() {
        let trimmed = line.trim_start();
        let fence = fence(trimmed);
        let closes = open.as_ref().is_some_and(|block| {
            fence.is_some_and(|(mark, run)| {
                mark == block.mark
                    && run >= block.run
                    && trimmed.trim_end().chars().all(|c| c == mark)
            })
        });
        if closes {
            if let Some(block) = open.take().filter(|block| block.marked) {
                found = Some(block.lines.iter().fold(String::new(), |mut program, line| {
                    program.push_str(line);
                    program.push('\n');
                    program
                }));
            }
        } else if let Some(block) = &mut open {
            block.lines.push(line);
        } else if let Some((mark, run)) = fence {
            let language = trimmed[run..].split_whitespace().next().unwrap_or_default();
            open = Some(Open {
                mark,
                run,
                marked: matches!(language, "cairn" | "crn"),
                lines: Vec::new(),
            });
        }
    }
    found
}
