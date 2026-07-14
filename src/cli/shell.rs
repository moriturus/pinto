//! Interactive shell (REPL) input helpers: line splitting, completion, and `rustyline` setup.
//!
//! The REPL loop and dispatch live in [`super::commands`], which reuses the existing `clap`
//! [`Command`] definition. This module provides three focused helpers:
//!
//! - [`split_args`] — split one input line into `clap` arguments (spaces and quotes are supported).
//! - [`complete_line`] — calculate completion candidates from the `clap` definition.
//! - [`build_editor`] — build a `rustyline` editor with editing, history, and completion.
//!
//! `rustyline` handles history, line editing, and completion. It is more robust than handwritten
//! terminal handling and falls back to single-line input for non-TTY streams. Completion logic is
//! kept pure so it can be tested without a terminal.

use clap::CommandFactory;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::FileHistory;
use rustyline::validate::Validator;
use rustyline::{Editor, Helper};
use std::fmt;

/// Reason for failure to split input line.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SplitError {
    /// Unclosed quote (line ended in the middle of argument).
    UnterminatedQuote(char),
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplitError::UnterminatedQuote(q) => write!(f, "unterminated {q} quote"),
        }
    }
}

/// Split the input line into an argv-like sequence of tokens.
///
/// - Separate tokens with spaces (including consecutive spaces).
/// - Single quotes `'...'` are verbatim without escaping.
/// - Double quotes `"..."` only interpret `\"` and `\\` escapes, and the others are interpreted verbatim.
/// - `\` outside quotes escapes the next character.
///
/// Unterminated quote is [`SplitError::UnterminatedQuote`].
pub(super) fn split_args(line: &str) -> Result<Vec<String>, SplitError> {
    let mut args = Vec::new();
    let mut cur = String::new();
    // Preserve an empty quoted token (for example, `""`) as one argument by tracking whether
    // the current token has started separately from its contents.
    let mut has_token = false;
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if has_token {
                    args.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            '\'' => {
                has_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => cur.push(ch),
                        None => return Err(SplitError::UnterminatedQuote('\'')),
                    }
                }
            }
            '"' => {
                has_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(next @ ('"' | '\\')) => cur.push(next),
                            // Unrecognized escapes are left verbatim, including the backslash.
                            Some(other) => {
                                cur.push('\\');
                                cur.push(other);
                            }
                            None => return Err(SplitError::UnterminatedQuote('"')),
                        },
                        Some(ch) => cur.push(ch),
                        None => return Err(SplitError::UnterminatedQuote('"')),
                    }
                }
            }
            '\\' => {
                has_token = true;
                match chars.next() {
                    Some(ch) => cur.push(ch),
                    // A bare backslash at the end of a line is treated as a single character.
                    None => cur.push('\\'),
                }
            }
            other => {
                has_token = true;
                cur.push(other);
            }
        }
    }
    if has_token {
        args.push(cur);
    }
    Ok(args)
}

/// REPL built-in termination word (not in `clap`, but provided in completion).
const BUILTINS: [&str; 2] = ["exit", "quit"];

/// Return completion candidates for `line` at cursor position `pos` (in bytes).
///
/// The return value is `(replacement start position, candidate list)`. `rustyline` replaces text
/// from the returned start position through `pos`. Completion covers two cases:
///
/// - **First word**: subcommand names, visible aliases, and REPL built-ins (`exit` / `quit`).
/// - **Word starting with `-`**: long options (`--xxx`) for the preceding subcommand.
///
/// No suggestions are made for ordinary argument positions. Candidates are derived from the
/// [`Command`] definition each time, so completion follows changes to subcommand options.
///
/// [`Command`]: clap::Command
pub(super) fn complete_line(cmd: &clap::Command, line: &str, pos: usize) -> (usize, Vec<String>) {
    let before = &line[..pos];
    // The starting position of the currently completed word (after the previous blank, or at the beginning of the line if none exists).
    let start = before
        .rfind(char::is_whitespace)
        .map(|i| i + 1)
        .unwrap_or(0);
    let word = &before[start..];
    let preceding = before[..start].trim();

    let candidates = if preceding.is_empty() {
        // First word: subcommand name + built-in.
        subcommand_names(cmd)
            .into_iter()
            .chain(BUILTINS.iter().map(std::string::ToString::to_string))
            .filter(|c| c.starts_with(word))
            .collect()
    } else if word.starts_with('-') {
        // A word starting with `-`: long options of the preceding subcommand.
        let sub = preceding.split_whitespace().next().unwrap_or("");
        option_names(cmd, sub)
            .into_iter()
            .filter(|c| c.starts_with(word))
            .collect()
    } else {
        Vec::new()
    };

    (start, candidates)
}

/// Collect subcommand names (including visible aliases).
fn subcommand_names(cmd: &clap::Command) -> Vec<String> {
    let mut names = Vec::new();
    for sub in cmd.get_subcommands() {
        names.push(sub.get_name().to_string());
        names.extend(
            sub.get_visible_aliases()
                .map(std::string::ToString::to_string),
        );
    }
    names
}

/// Collect long options (`--xxx`) of the subcommand `sub` (name or visible alias).
fn option_names(cmd: &clap::Command, sub: &str) -> Vec<String> {
    let Some(subcmd) = cmd
        .get_subcommands()
        .find(|c| c.get_name() == sub || c.get_visible_aliases().any(|a| a == sub))
    else {
        return Vec::new();
    };
    subcmd
        .get_arguments()
        .filter_map(|arg| arg.get_long().map(|l| format!("--{l}")))
        .collect()
}

/// `rustyline` helper that provides completion while leaving hints, highlighting, and validation
/// at their default implementations.
///
/// Capture the [`Command`](clap::Command) definition once at startup so completion uses the same
/// command tree as the CLI.
pub(super) struct ShellHelper {
    cmd: clap::Command,
}

impl Completer for ShellHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let (start, candidates) = complete_line(&self.cmd, line, pos);
        let pairs = candidates
            .into_iter()
            .map(|c| Pair {
                display: c.clone(),
                replacement: c,
            })
            .collect();
        Ok((start, pairs))
    }
}

// Hints, highlighting, and validation are not used; the helper provides completion only.
impl Hinter for ShellHelper {
    type Hint = String;
}
impl Highlighter for ShellHelper {}
impl Validator for ShellHelper {}
impl Helper for ShellHelper {}

/// Build a rustyline editor with completion, history, and line editing.
///
/// History is kept in memory for the current session. It is not persisted and never writes to
/// `.pinto/`, keeping the board data lightweight and uncontaminated.
pub(super) fn build_editor() -> rustyline::Result<Editor<ShellHelper, FileHistory>> {
    let mut editor = Editor::new()?;
    editor.set_helper(Some(ShellHelper {
        cmd: super::args::Cli::command(),
    }));
    Ok(editor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(split_args("add title").unwrap(), vec!["add", "title"]);
    }

    #[test]
    fn collapses_repeated_whitespace() {
        assert_eq!(split_args("  add   title  ").unwrap(), vec!["add", "title"]);
    }

    #[test]
    fn empty_line_yields_no_tokens() {
        assert_eq!(split_args("   ").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn double_quotes_preserve_spaces() {
        assert_eq!(
            split_args("add \"Hello world\"").unwrap(),
            vec!["add", "Hello world"]
        );
    }

    #[test]
    fn single_quotes_preserve_spaces_verbatim() {
        assert_eq!(split_args("add 'a b\\c'").unwrap(), vec!["add", "a b\\c"]);
    }

    #[test]
    fn double_quote_escapes_quote_and_backslash() {
        assert_eq!(
            split_args("add \"a\\\"b\\\\c\"").unwrap(),
            vec!["add", "a\"b\\c"]
        );
    }

    #[test]
    fn backslash_escapes_outside_quotes() {
        assert_eq!(split_args("a\\ b").unwrap(), vec!["a b"]);
    }

    #[test]
    fn empty_quotes_become_empty_argument() {
        assert_eq!(
            split_args("edit T-1 --body \"\"").unwrap(),
            vec!["edit", "T-1", "--body", ""]
        );
    }

    #[test]
    fn unterminated_double_quote_errors() {
        assert_eq!(
            split_args("add \"oops"),
            Err(SplitError::UnterminatedQuote('"'))
        );
    }

    #[test]
    fn unterminated_single_quote_errors() {
        assert_eq!(
            split_args("add 'oops"),
            Err(SplitError::UnterminatedQuote('\''))
        );
    }

    #[test]
    fn split_errors_display_and_edge_case_escapes_are_stable() {
        assert_eq!(
            SplitError::UnterminatedQuote('"').to_string(),
            "unterminated \" quote"
        );
        assert_eq!(split_args("add \"a\\q\"").unwrap(), vec!["add", "a\\q"]);
        assert_eq!(split_args("add \\").unwrap(), vec!["add", "\\"]);
        assert_eq!(
            split_args("add \"a\\").expect_err("quote after a trailing escape"),
            SplitError::UnterminatedQuote('"')
        );
    }

    // --- Completion logic (complete_line) ---

    fn cmd() -> clap::Command {
        super::super::args::Cli::command()
    }

    #[test]
    fn completes_subcommands_at_start() {
        let (start, cands) = complete_line(&cmd(), "ad", 2);
        assert_eq!(start, 0);
        assert_eq!(cands, vec!["add"]);
    }

    #[test]
    fn empty_first_word_lists_all_subcommands_and_builtins() {
        let (start, cands) = complete_line(&cmd(), "", 0);
        assert_eq!(start, 0);
        // Candidates include major subcommands and REPL built-in terminators.
        for expected in [
            "add", "list", "show", "move", "board", "shell", "exit", "quit",
        ] {
            assert!(cands.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn completes_visible_alias_ls_for_list() {
        let (_start, cands) = complete_line(&cmd(), "l", 1);
        assert!(cands.contains(&"list".to_string()));
        assert!(cands.contains(&"ls".to_string()));
    }

    #[test]
    fn completes_long_options_for_subcommand() {
        let (start, cands) = complete_line(&cmd(), "list --", 7);
        assert_eq!(start, 5);
        assert!(cands.contains(&"--json".to_string()));
        assert!(cands.contains(&"--status".to_string()));
    }

    #[test]
    fn completes_partial_long_option() {
        let (start, cands) = complete_line(&cmd(), "add --po", 8);
        assert_eq!(start, 4);
        assert_eq!(cands, vec!["--points"]);
    }

    #[test]
    fn no_candidates_for_plain_argument_word() {
        // No suggestions are given for positions (arguments) that are neither options nor subcommands.
        let (_start, cands) = complete_line(&cmd(), "show T", 6);
        assert!(cands.is_empty());
    }

    #[test]
    fn unknown_subcommand_has_no_option_candidates() {
        let (_start, cands) = complete_line(&cmd(), "unknown --", 10);
        assert!(cands.is_empty());
    }

    #[test]
    fn rustyline_helper_converts_completion_candidates_to_pairs() {
        let helper = ShellHelper { cmd: cmd() };
        let history = rustyline::history::DefaultHistory::new();
        let context = rustyline::Context::new(&history);
        let (start, pairs) = helper
            .complete("ad", 2, &context)
            .expect("completion succeeds");

        assert_eq!(start, 0);
        assert_eq!(
            pairs
                .iter()
                .map(|pair| (&pair.display, &pair.replacement))
                .collect::<Vec<_>>(),
            vec![(&"add".to_string(), &"add".to_string())]
        );
    }
}
